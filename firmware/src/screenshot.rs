//! Save complete captures in erased sectors. Export reads them as a batch.
//! Only a separately confirmed clear can erase capture-owned sectors.

use core::sync::atomic::{AtomicU8, Ordering};

use embassy_sync::{blocking_mutex::raw::CriticalSectionRawMutex, signal::Signal};
use embassy_time::Timer;
use firmware_core::buttons::Detail;
use firmware_core::screenshot::{
    CAPTURE_SECTORS, CAPTURE_SPARE_SECTORS, CAPTURE_TABLE_LEN, COMPACT_HEADER_BYTES, Capture,
    CapturePage, Encoder, FRAME_BYTES, LEGACY_RECORD_BYTES, MAX_RECORD_BYTES, compact_record_len,
};
use firmware_core::screenshot_report::{HEADER_RGB_BYTES, encode_report};

use crate::{I2C3, flash, render::Frame, ui};

const START: usize = firmware_image::UPPER_END as usize;
const END: usize = 0x6e000;
const LEGACY_BASES: [usize; 2] = [START, START + 3 * flash::SECTOR];
const OWNER: u32 = 0x3243_5358;
const LEGACY_OWNER: u32 = 0x3143_5358;
const OWNER_CHECK: u32 = 0xd9a2_651e;
const COMMITTED: u32 = 0x4d4f_4358;
const META_BYTES: usize = 5 * 4;
/// Each append reserves a length word, then the sequence, complement and commit.
const APPEND_META: usize = 4 * 4;
const WRITE_WORDS: usize = 8;
const _: () = assert!(START + CAPTURE_SECTORS * flash::SECTOR == END);
const _: () = assert!(MAX_RECORD_BYTES + META_BYTES == flash::SECTOR);
const _: () = assert!(LEGACY_RECORD_BYTES + META_BYTES <= 3 * flash::SECTOR);
const _: () = assert!(HEADER_RGB_BYTES + MAX_RECORD_BYTES <= FRAME_BYTES);

static REQUEST: AtomicU8 = AtomicU8::new(0);
static READY: Signal<CriticalSectionRawMutex, Pending> = Signal::new();
static STATE: AtomicU8 = AtomicU8::new(0);

enum Pending {
    Screen(CapturePage),
    Report(usize),
}

#[derive(Clone, Copy, Eq, PartialEq)]
#[repr(u8)]
pub enum Kind {
    Screen = 1,
    Report,
}

pub fn request(kind: Kind) {
    if !crate::CAPTURE_DIAGNOSTIC {
        REQUEST.store(kind as u8, Ordering::Relaxed);
    }
}

pub fn take_request() -> Option<Kind> {
    match REQUEST.swap(0, Ordering::Relaxed) {
        1 => Some(Kind::Screen),
        2 => Some(Kind::Report),
        _ => None,
    }
}

#[derive(Clone, Copy, Eq, PartialEq)]
#[repr(u8)]
pub enum Status {
    Idle,
    Saving,
    Saved,
    Failed,
    Full,
    Unavailable,
    TooLarge,
    ReportSaved,
}

pub fn status() -> Status {
    match STATE.load(Ordering::Relaxed) {
        1 => Status::Saving,
        2 => Status::Saved,
        3 => Status::Failed,
        4 => Status::Full,
        5 => Status::Unavailable,
        6 => Status::TooLarge,
        7 => Status::ReportSaved,
        _ => Status::Idle,
    }
}

/// # Safety
/// The display owner must yield the idle framebuffer until the save task
/// leaves `Saving`.
pub unsafe fn capture_screen(frame_shown: Option<CapturePage>) {
    if status() == Status::Saving {
        return;
    }
    if let Some(page) = frame_shown {
        STATE.store(Status::Saving as u8, Ordering::Relaxed);
        READY.signal(Pending::Screen(page));
    } else {
        STATE.store(Status::Failed as u8, Ordering::Relaxed);
    }
}

/// Use the idle framebuffer below the header as the report staging area.
pub fn stage_report(
    frame: &mut Frame,
    detail: Detail,
    sections: &mut dyn Iterator<Item = [[u8; ui::WIDE_COLUMNS]; 8]>,
) -> Option<usize> {
    // SAFETY: Frame contains initialized u16 pixels in wire byte order, and
    // the whole buffer is uniquely borrowed. The two slices do not overlap.
    let frame = unsafe { &mut *frame.as_mut_ptr().cast::<[u8; FRAME_BYTES]>() };
    let (header, staging) = frame.split_at_mut(HEADER_RGB_BYTES);
    encode_report(
        detail,
        header.first_chunk().unwrap(),
        ui::report_capture_colors(detail),
        sections,
        staging.first_chunk_mut().unwrap(),
    )
}

/// # Safety
/// The display owner has finished staging, invalidated the displayed frame,
/// and will yield the idle buffer to the save task until `Saving` ends.
pub unsafe fn capture_report(result: Result<usize, Status>) {
    match result {
        Ok(size) => {
            STATE.store(Status::Saving as u8, Ordering::Relaxed);
            READY.signal(Pending::Report(size));
        }
        Err(status) => STATE.store(status as u8, Ordering::Relaxed),
    }
}

#[derive(Clone, Copy)]
enum Slot {
    Legacy(usize),
    Compact(usize),
}

impl Slot {
    fn base(self) -> usize {
        match self {
            Self::Legacy(base) | Self::Compact(base) => base,
        }
    }

    fn size(self) -> usize {
        match self {
            Self::Legacy(_) => 3 * flash::SECTOR,
            Self::Compact(_) => flash::SECTOR,
        }
    }

    fn meta(self) -> usize {
        self.base() + self.size() - META_BYTES
    }

    fn owned(self) -> bool {
        let tag = match self {
            Self::Legacy(_) => LEGACY_OWNER,
            Self::Compact(_) => OWNER,
        };
        flash::read(self.meta()) == tag && flash::read(self.meta() + 4) == OWNER_CHECK
    }

    /// # Safety
    /// Keep the sector immutable while any record passed to `visit` is live.
    /// Return the first unused append address, if the framing can be trusted.
    unsafe fn scan(self, mut visit: impl FnMut(u32, Capture<'static>)) -> Option<usize> {
        if !self.owned() {
            return None;
        }
        let seq = committed_sequence(self.meta() + 8)?;
        let size = match self {
            Self::Legacy(_) => LEGACY_RECORD_BYTES,
            Self::Compact(base) => {
                compact_record_len(unsafe { record_bytes(base, COMPACT_HEADER_BYTES) })?
            }
        };
        if let Some(record) = Capture::open(unsafe { record_bytes(self.base(), size) }) {
            visit(seq, record);
        }
        let Self::Compact(base) = self else {
            return None;
        };

        // The original first record and end-of-sector ownership stay in place.
        // Appends grow into the erased space between them.
        let mut next = base + size.next_multiple_of(4);
        while next + APPEND_META <= self.meta() {
            let word = flash::read(next);
            if word == flash::BLANK {
                return Some(next);
            }
            let length = word as u16;
            let size = usize::from(length);
            let data = next + APPEND_META;
            let end = data + size.next_multiple_of(4);
            if (word >> 16) as u16 != !length || size < COMPACT_HEADER_BYTES || end > self.meta() {
                // A torn reservation cannot tell us where the next entry starts.
                return None;
            }
            if let Some(seq) = committed_sequence(next + 4) {
                let bytes = unsafe { record_bytes(data, size) };
                if compact_record_len(bytes) == Some(size)
                    && let Some(record) = Capture::open(bytes)
                {
                    visit(seq, record);
                }
            }
            // A fully reserved but uncommitted entry can be skipped.
            next = end;
        }
        None
    }
}

fn committed_sequence(address: usize) -> Option<u32> {
    let seq = flash::read(address);
    (flash::read(address + 4) == !seq && flash::read(address + 8) == COMMITTED).then_some(seq)
}

/// # Safety
/// The range is in on-chip flash and immutable for the borrow's lifetime.
unsafe fn record_bytes(address: usize, size: usize) -> &'static [u8] {
    unsafe { core::slice::from_raw_parts(address as *const u8, size) }
}

fn reserved() -> impl Iterator<Item = usize> {
    (START..END).step_by(flash::SECTOR)
}

fn legacy_slot(base: usize) -> Slot {
    Slot::Legacy(LEGACY_BASES[usize::from(base >= LEGACY_BASES[1])])
}

/// # Safety
/// The dedicated capture region must remain immutable throughout USB export.
#[cfg(feature = "capture-backup")]
pub unsafe fn export_flash_snapshot()
-> &'static [u8; firmware_core::screenshot_disk::FLASH_SNAPSHOT_BYTES] {
    use firmware_core::screenshot_disk::FLASH_SNAPSHOT_BYTES;
    const _: () = assert!(END - START == FLASH_SNAPSHOT_BYTES);
    // SAFETY: the array covers only the six dedicated capture sectors.
    unsafe { &*(START as *const [u8; FLASH_SNAPSHOT_BYTES]) }
}

#[cfg(feature = "capture-probe")]
pub fn probe_lines() -> crate::ui::Lines {
    use crate::render::{hex, put};
    use firmware_image::{REGION_HASH_START, region_hash_update};

    let mut lines = [[b' '; 24]; 8];
    put(&mut lines[0], b"CAPTURE FLASH");
    put(&mut lines[1], b"ADDR  S USED FNV1A");
    for (row, base) in lines[2..].iter_mut().zip(reserved()) {
        let mut used = 0;
        let mut hash = REGION_HASH_START;
        for offset in (0..flash::SECTOR).step_by(4) {
            let word = flash::read(base + offset);
            used += u32::from(word != flash::BLANK);
            hash = region_hash_update(hash, &word.to_le_bytes());
        }
        let slot = Slot::Compact(base);
        row[6] = if legacy_slot(base).owned() {
            b'L'
        } else if slot.owned() {
            if committed_sequence(slot.meta() + 8).is_some() {
                b'C'
            } else {
                b'T'
            }
        } else if used == 0 {
            b'E'
        } else {
            b'U'
        };
        hex(row, 0, 5, base as u32);
        hex(row, 8, 4, used);
        hex(row, 13, 8, hash);
    }
    lines
}

fn spare_settings() -> impl Iterator<Item = usize> + Clone {
    flash::SECTORS
        .into_iter()
        .filter(|base| !flash::ENERGY_SECTORS.contains(base))
}

fn slots() -> impl Iterator<Item = Slot> {
    LEGACY_BASES
        .into_iter()
        .map(Slot::Legacy)
        .chain(reserved().chain(spare_settings()).map(Slot::Compact))
}

fn blank_slot() -> Option<usize> {
    // An owned legacy slot may have blank sectors after an interrupted save.
    // Keep its whole range reserved until the user clears it.
    if let Some(base) = reserved().find(|&base| !legacy_slot(base).owned() && flash::blank(base)) {
        return Some(base);
    }
    // Borrow at most two spare sectors and always leave a blank one for a
    // future settings/log allocation. The dedicated energy pair is excluded.
    let spares = spare_settings();
    if spares
        .clone()
        .filter(|&base| Slot::Compact(base).owned())
        .count()
        >= CAPTURE_SPARE_SECTORS
    {
        return None;
    }
    let mut blank = spares.filter(|&base| flash::blank(base));
    let base = blank.next()?;
    blank.next().map(|_| base)
}

/// # Safety
/// Export boot never starts the measurement or capture tasks. Keep these
/// sectors immutable while the records are in use.
pub unsafe fn export_count() -> usize {
    let mut count = 0;
    for slot in slots() {
        // SAFETY: no other tasks can modify these sectors in export boot.
        unsafe { slot.scan(|_, _| count += 1) };
    }
    count
}

/// # Safety
/// Export boot never starts the measurement or capture tasks. Keep these
/// sectors immutable while the records are in use.
pub unsafe fn export_records() -> [Option<Capture<'static>>; CAPTURE_TABLE_LEN] {
    let mut captures = [(0u32, None); CAPTURE_TABLE_LEN];
    let mut len = 0;
    for slot in slots() {
        // SAFETY: no other tasks can modify these sectors in export boot.
        unsafe {
            slot.scan(|seq, record| {
                if let Some(entry) = captures.get_mut(len) {
                    *entry = (seq, Some(record));
                    len += 1;
                }
            });
        }
    }
    // The live sequences span at most a few captures, including across wrap.
    let anchor = captures[0].0;
    captures[..len].sort_unstable_by_key(|&(seq, _)| seq.wrapping_sub(anchor) as i32);
    captures.map(|(_, record)| record)
}

/// # Safety
/// Call only after explicit on-screen confirmation in the isolated clear
/// boot, with no USB/measurement tasks or borrowed flash records.
pub unsafe fn clear_owned() -> bool {
    for slot in slots().filter(|slot| slot.owned()) {
        for base in (slot.base()..slot.base() + slot.size()).step_by(flash::SECTOR) {
            if !flash::blank(base) && !unsafe { erase_capture_sector(base) } {
                return false;
            }
        }
    }
    true
}

#[cfg(feature = "capture-reclaim")]
pub fn capture_owns(base: usize) -> bool {
    legacy_slot(base).owned() || Slot::Compact(base).owned()
}

/// # Safety
/// The caller has verified sector ownership or a full backup hash, obtained
/// confirmation, and ensured no flash writer or borrowed record is live.
pub unsafe fn erase_capture_sector(base: usize) -> bool {
    let erase: unsafe extern "C" fn(u32) -> u32 = core::hint::black_box(flash::erase_sector);
    let errors = cortex_m::interrupt::free(|_| unsafe { erase(base as u32) });
    if errors != 0 {
        flash::note_failure(base, errors);
        return false;
    }
    if !flash::blank(base) {
        flash::note_failure(base, flash::VERIFY_FAILED);
        return false;
    }
    true
}

#[embassy_executor::task]
pub async fn save_task() {
    loop {
        let fb = crate::framebuffer();
        let result = match READY.wait().await {
            Pending::Screen(page) => {
                // SAFETY: the display owner yielded the flushed frame.
                let frame = unsafe { &*fb.cast::<[u8; FRAME_BYTES]>() };
                save_screen(frame, page).await
            }
            Pending::Report(size) => {
                // SAFETY: the display owner yielded the staging buffer, and
                // size is the length returned by the bounded report encoder.
                let frame = unsafe { &*fb.cast::<[u8; FRAME_BYTES]>() };
                let record = &frame[HEADER_RGB_BYTES..HEADER_RGB_BYTES + size];
                match save(size, &mut record.iter().copied()).await {
                    Status::Saved => Status::ReportSaved,
                    status => status,
                }
            }
        };
        STATE.store(result as u8, Ordering::Relaxed);
    }
}

async fn save_screen(frame: &[u8; FRAME_BYTES], page: CapturePage) -> Status {
    let Some(encoder) = Encoder::new(frame) else {
        return Status::TooLarge;
    };
    let Some((size, mut bytes)) = encoder.record_for(page) else {
        return Status::TooLarge;
    };
    save(size, &mut bytes).await
}

async fn save(size: usize, bytes: &mut dyn Iterator<Item = u8>) -> Status {
    let mut latest = None;
    let mut append = None;
    let span = APPEND_META + size.next_multiple_of(4);
    for slot in slots() {
        // SAFETY: only this task writes to the capture journal. The callback
        // keeps sequence numbers only, so all record borrows end before writing.
        let next = unsafe {
            slot.scan(|seq, _| {
                if latest.is_none_or(|old| (seq.wrapping_sub(old) as i32) > 0) {
                    latest = Some(seq);
                }
            })
        };
        if append.is_none()
            && let Some(next) = next
            && next + span <= slot.meta()
            && (next..next + span)
                .step_by(4)
                .all(|at| flash::read(at) == flash::BLANK)
        {
            append = Some((slot.base(), next));
        }
    }
    let seq = latest.map_or(0, |seq| seq.wrapping_add(1));

    let (base, data, commit) = if let Some((base, next)) = append {
        let length = size as u16;
        let reservation = u32::from(length) | (u32::from(!length) << 16);
        if !write(base, next, &[reservation, seq, !seq]).await {
            return Status::Failed;
        }
        (base, next + APPEND_META, next + APPEND_META - 4)
    } else {
        let Some(base) = blank_slot() else {
            return if slots().any(Slot::owned) {
                Status::Full
            } else {
                Status::Unavailable
            };
        };
        let meta = Slot::Compact(base).meta();
        let _bus = I2C3.lock().await;
        // A settings or diagnostic write may have claimed a spare after we
        // chose it. Verify and claim in one critical section.
        if !cortex_m::interrupt::free(|_| {
            flash::blank(base)
                && (base < END
                    || spare_settings().any(|other| other != base && flash::blank(other)))
                && flash::program_blank(base, meta, &[OWNER, OWNER_CHECK, seq, !seq])
        }) {
            return Status::Failed;
        }
        (base, base, meta + META_BYTES - 4)
    };

    for offset in (0..size).step_by(4 * WRITE_WORDS) {
        let count = (size - offset).div_ceil(4).min(WRITE_WORDS);
        let mut words = [flash::BLANK; WRITE_WORDS];
        for word in &mut words[..count] {
            *word = u32::from_le_bytes(core::array::from_fn(|_| bytes.next().unwrap_or(0xff)));
        }
        if !write(base, data + offset, &words[..count]).await {
            return Status::Failed;
        }
        Timer::after_millis(1).await;
    }
    if write(base, commit, &[COMMITTED]).await {
        Status::Saved
    } else {
        Status::Failed
    }
}

async fn write(base: usize, address: usize, words: &[u32]) -> bool {
    let _bus = I2C3.lock().await;
    flash::program_blank(base, address, words)
}
