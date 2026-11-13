//! Exclusive logger variant. Only the meter task owns a Writer.
use crate::{I2C3, flash, render::Frame, ui};
use core::sync::atomic::{AtomicBool, Ordering};
use firmware_core::{buttons::Detail, charging_log as format, screenshot::CapturePage};
const START: usize = 0x62000;
const END: usize = 0x6e000;
const _: () = assert!(START == firmware_image::UPPER_END as usize);
static TOGGLE: AtomicBool = AtomicBool::new(false);

pub struct Writer {
    active: bool,
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
            self.active = false;
            self.message = "LOGGER STOPPED - L+R START";
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
        self.session = session;
        self.segment = 0;
        self.base = 0;
        self.index = 0;
        self.start = now;
        self.next = now;
        self.active = true;
        self.message = "LOGGER REC 5s - L+R STOP";
    }
    pub async fn sample(&mut self, now: u64, reading: Option<(u32, i32)>) {
        if !self.active || now < self.next {
            return;
        }
        self.next = now + format::INTERVAL_MS;
        let Some((mv, ma)) = reading else {
            return;
        }; // timestamp gaps expose missing reads
        let Ok(elapsed) = u32::try_from(now - self.start) else {
            self.fail("LOGGER TIME LIMIT");
            return;
        };
        let _bus = I2C3.lock().await;
        if self.base == 0 || self.index == format::SAMPLES {
            let Some(base) = (START..END).step_by(flash::SECTOR).find(|&b| available(b)) else {
                self.fail("LOGGER FULL - COPY/CLEAR");
                return;
            };
            let h = format::header(self.session, self.segment);
            if !commit(base, base, h) {
                self.fail("LOGGER WRITE FAILED");
                return;
            }
            self.base = base;
            self.index = 0;
            self.segment += 1;
        }
        let address = self.base + format::HEADER + self.index * format::RECORD;
        if !commit(self.base, address, format::record(elapsed, mv, ma)) {
            self.fail("LOGGER WRITE FAILED");
            return;
        }
        self.index += 1;
    }
    fn fail(&mut self, message: &'static str) {
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
    let mask = format::clear_mask(core::array::from_fn(|i| bytes(START + i * flash::SECTOR)));
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
    if (START..END).step_by(flash::SECTOR).any(available) {
        "STORAGE CLEARED"
    } else {
        "NO SPACE - UNKNOWN DATA"
    }
}
