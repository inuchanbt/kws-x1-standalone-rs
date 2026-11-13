//! Exclusive logger variant. Only the meter task owns a Writer.
use crate::{I2C3, flash, render::Frame, ui};
use core::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use firmware_core::{buttons::Detail, charging_log as format, screenshot::CapturePage};
const START: usize = 0x62000;
const END: usize = 0x6e000;
const _: () = assert!(START == firmware_image::UPPER_END as usize);
use embassy_sync::{blocking_mutex::raw::CriticalSectionRawMutex, channel::Channel};
static RECORDING: AtomicBool = AtomicBool::new(false);
static DROPPED: AtomicU32 = AtomicU32::new(0);
struct Event {
    at: u64,
    words: [u32; 8],
    count: usize,
    kind: u32,
}
static EVENTS: Channel<CriticalSectionRawMutex, Event, 32> = Channel::new();
/// Called only by the passive sniffer: copy checked RX bytes, never wait or write flash.
#[inline(never)]
pub fn pd_packet(bytes: &[u8]) {
    if !RECORDING.load(Ordering::Relaxed) {
        return;
    }
    let header = u16::from_le_bytes([bytes[1], bytes[2]]);
    let count = usize::from((header >> 12) & 7);
    let mut words = [0; 8];
    words[0] = u32::from(header) | (u32::from(bytes[0] >> 5) << 16);
    for i in 0..count {
        words[i + 1] = u32::from_le_bytes(bytes[3 + i * 4..7 + i * 4].try_into().unwrap());
    }
    enqueue(Event {
        at: embassy_time::Instant::now().as_millis(),
        words,
        count: count + 1,
        kind: 0,
    });
}
#[inline(never)]
pub fn pd_stopped() {
    if RECORDING.load(Ordering::Relaxed) {
        enqueue(Event {
            at: embassy_time::Instant::now().as_millis(),
            words: [1; 8],
            count: 1,
            kind: 9,
        });
    }
}
fn enqueue(event: Event) {
    if EVENTS.try_send(event).is_err() {
        DROPPED.fetch_add(1, Ordering::Relaxed);
    }
}
static TOGGLE: AtomicBool = AtomicBool::new(false);

pub struct Writer {
    active: bool,
    stopping: bool,
    pd_base: usize,
    pd_index: usize,
    event_id: u32,
    start: u64,
    next: u64,
    session: u32,
    segment: u32,
    base: usize,
    index: usize,
    message: &'static str,
}
impl Writer {
    pub fn new() -> Self {
        Self {
            active: false,
            stopping: false,
            pd_base: 0,
            pd_index: 0,
            event_id: 0,
            start: 0,
            next: 0,
            session: 0,
            segment: 0,
            base: 0,
            index: 0,
            message: "LOGGER READY - HOLD L+R",
        }
    }
    pub fn message(&self) -> &'static str {
        self.message
    }
    pub fn handle_request(&mut self, now: u64) {
        if !TOGGLE.swap(false, Ordering::Relaxed) {
            return;
        }
        if self.active {
            RECORDING.store(false, Ordering::Relaxed);
            self.stopping = true;
            self.message = "LOGGER STOPPING";
            return;
        }
        let latest = (START..END)
            .step_by(flash::SECTOR)
            .filter_map(|b| format::valid_header(bytes(b)))
            .max()
            .unwrap_or(0);
        let Some(session) = latest.checked_add(1) else {
            self.message = "LOGGER SESSION LIMIT";
            return;
        };
        while EVENTS.try_receive().is_ok() {}
        DROPPED.store(0, Ordering::Relaxed);
        self.stopping = false;
        self.pd_base = 0;
        self.pd_index = 0;
        self.event_id = 0;
        self.session = session;
        self.segment = 0;
        self.base = 0;
        self.index = 0;
        self.start = now;
        self.next = now;
        self.active = true;
        self.message = "LOGGER REC 1s - L+R STOP";
        RECORDING.store(true, Ordering::Relaxed);
    }
    pub async fn sample(&mut self, now: u64, reading: Option<(u32, i32)>) {
        if !self.active {
            return;
        }
        // Bound each drain so the meter and PD tasks continue making progress.
        for _ in 0..4 {
            let Ok(event) = EVENTS.try_receive() else {
                break;
            };
            let Ok(elapsed) = u32::try_from(event.at.saturating_sub(self.start)) else {
                self.fail("LOGGER TIME LIMIT");
                return;
            };
            if self.event_id == 0x00ff_ffff {
                self.fail("LOGGER EVENT LIMIT");
                return;
            }
            self.event_id += 1;
            let bus = I2C3.lock().await;
            // Reserve the whole packet before committing its first fragment.
            let remaining = if self.pd_base == 0 {
                0
            } else {
                format::SAMPLES - self.pd_index
            };
            if !format::packet_fits(
                remaining,
                event.count,
                remaining < event.count && (START..END).step_by(flash::SECTOR).any(available),
            ) {
                self.fail("LOGGER FULL - COPY/CLEAR");
                return;
            }
            for i in 0..event.count {
                let tag = (self.event_id << 8) | (event.kind + i as u32);
                if !self.append(true, format::record(elapsed, tag, event.words[i] as i32)) {
                    return;
                }
            }
            drop(bus);
            embassy_time::Timer::after_millis(1).await;
        }
        let _bus = I2C3.lock().await;
        let dropped = DROPPED.swap(0, Ordering::Relaxed);
        let Ok(elapsed) = u32::try_from(now - self.start) else {
            self.fail("LOGGER TIME LIMIT");
            return;
        };
        if dropped != 0 && !self.append(true, format::record(elapsed, 8, dropped as i32)) {
            return;
        }
        if self.stopping {
            if !EVENTS.is_empty() {
                return;
            }
            self.active = false;
            self.stopping = false;
            self.message = "LOGGER STOPPED - L+R START";
            return;
        }
        if now < self.next {
            return;
        }
        self.next = format::next_deadline(self.next, now);
        if let Some((mv, ma)) = reading {
            self.append(false, format::record(elapsed, mv, ma));
        }
    }
    fn append(&mut self, event: bool, words: [u32; 4]) -> bool {
        let (mut base, mut index) = if event {
            (self.pd_base, self.pd_index)
        } else {
            (self.base, self.index)
        };
        if base == 0 || index == format::SAMPLES {
            let Some(free) = (START..END).step_by(flash::SECTOR).find(|&b| available(b)) else {
                self.fail("LOGGER FULL - COPY/CLEAR");
                return false;
            };
            base = free;
            let h = if event {
                format::event_header(self.session, self.segment)
            } else {
                format::header(self.session, self.segment)
            };
            if !commit(base, base, h) {
                self.fail("LOGGER WRITE FAILED");
                return false;
            }
            index = 0;
            self.segment += 1;
        }
        if !commit(base, base + format::HEADER + index * format::RECORD, words) {
            self.fail("LOGGER WRITE FAILED");
            return false;
        }
        if event {
            self.pd_base = base;
            self.pd_index = index + 1;
        } else {
            self.base = base;
            self.index = index + 1;
        }
        true
    }
    fn fail(&mut self, message: &'static str) {
        RECORDING.store(false, Ordering::Relaxed);
        self.active = false;
        self.message = message;
    }
}
// Old three-sector captures own padding sectors too, even when all bytes
// of an individual padding sector are erased. Do not allocate inside them.
fn available(base: usize) -> bool {
    let legacy = START + ((base - START) / (3 * flash::SECTOR)) * (3 * flash::SECTOR);
    let meta = legacy + 3 * flash::SECTOR - 20;
    !(flash::read(meta) == 0x3143_5358 && flash::read(meta + 4) == 0xd9a2_651e)
        && flash::blank(base)
}
fn commit(base: usize, at: usize, words: [u32; 4]) -> bool {
    // No erase during logging. Commit/checksum is a separate final operation.
    (START..END).contains(&base)
        && flash::program_blank(base, at, &words[..3])
        && flash::program_blank(base, at + 12, &words[3..])
}
fn bytes(base: usize) -> &'static [u8] {
    // Private helper: only called for the six fixed sectors. Callers finish
    // reading before a write; export boot starts no writer tasks.
    unsafe { core::slice::from_raw_parts(base as *const u8, flash::SECTOR) }
}
pub unsafe fn export_disk() -> format::Disk<'static> {
    format::Disk::new(core::array::from_fn(|i| bytes(START + i * flash::SECTOR)))
}
pub unsafe fn export_count() -> usize {
    unsafe { export_disk() }.file_count()
}
/// Called only in the existing confirmed clear boot, before measurement/USB.
pub unsafe fn clear_owned() -> bool {
    let owned = format::clear_mask(core::array::from_fn(|i| bytes(START + i * flash::SECTOR)));
    let Some(backed_up) = crate::capture_reclaim::logger_backup_mask(owned) else {
        return false;
    };
    let mask =
        format::backup_clear_mask(owned, core::array::from_fn(|i| backed_up & (1 << i) != 0));
    for (i, base) in (START..END).step_by(flash::SECTOR).enumerate() {
        if !mask[i] {
            continue;
        }
        let erase: unsafe extern "C" fn(u32) -> u32 = core::hint::black_box(flash::erase_sector);
        let errors = cortex_m::interrupt::free(|_| unsafe { erase(base as u32) });
        if errors != 0 || !flash::blank(base) {
            flash::note_failure(base, errors | flash::VERIFY_FAILED);
            return false;
        }
    }
    true
}

// Compatibility surface for the common display/input pipeline. No capture
// codec, framebuffer borrowing or save task is linked into this variant.
#[allow(dead_code)]
#[derive(Clone, Copy, Eq, PartialEq)]
pub enum Kind {
    Screen,
    Report,
}
#[allow(dead_code)]
#[derive(Clone, Copy, Eq, PartialEq)]
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
pub fn request(_: Kind) {
    if !crate::CAPTURE_DIAGNOSTIC {
        TOGGLE.store(true, Ordering::Relaxed);
    }
}
pub fn take_request() -> Option<Kind> {
    None
}
pub fn status() -> Status {
    Status::Idle
}
pub unsafe fn capture_screen(_: Option<CapturePage>) {}
pub unsafe fn capture_report(_: Result<usize, Status>) {}
pub fn stage_report(
    _: &mut Frame,
    _: Detail,
    _: &mut dyn Iterator<Item = [[u8; ui::WIDE_COLUMNS]; 8]>,
) -> Option<usize> {
    None
}

pub fn clear_message() -> &'static str {
    match (START..END)
        .step_by(flash::SECTOR)
        .filter(|&b| available(b))
        .count()
    {
        6 => "CLEARED - 6/6 FREE",
        5 => "PARTIAL - 5/6 FREE",
        4 => "PARTIAL - 4/6 FREE",
        3 => "PARTIAL - 3/6 FREE",
        2 => "PARTIAL - 2/6 FREE",
        1 => "PARTIAL - 1/6 FREE",
        _ => "NO SPACE - UNKNOWN DATA",
    }
}
