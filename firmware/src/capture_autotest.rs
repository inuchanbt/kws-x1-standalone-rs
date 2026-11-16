//! Experimental capture scheduling and RAM trace. No trace flash I/O while RUNNING.
use crate::{I2C3, capture_test_format as f, flash, screenshot};
use core::{
    cell::RefCell,
    sync::atomic::{AtomicU8, AtomicU32, Ordering},
};
use embassy_sync::blocking_mutex::{Mutex, raw::CriticalSectionRawMutex};
use embassy_time::{Instant, Timer};

// Ready, start requested, observing, finish requested, flushing, saved, failed.
static STATE: AtomicU8 = AtomicU8::new(0);
static MODE: AtomicU8 = AtomicU8::new(0);
static PENDING: AtomicU32 = AtomicU32::new(0);
static CURRENT: AtomicU32 = AtomicU32::new(0);
static BASE: AtomicU32 = AtomicU32::new(0);
static TRACE: Mutex<CriticalSectionRawMutex, RefCell<f::Trace>> =
    Mutex::new(RefCell::new(f::Trace::new()));

fn ms() -> u32 {
    Instant::now().as_millis() as u32
}
fn observing() -> bool {
    matches!(STATE.load(Ordering::Relaxed), 2 | 3)
}
pub fn locked() -> bool {
    STATE.load(Ordering::Relaxed) != 0
}

fn note(kind: u32, a: u32, b: u32) {
    if observing() {
        let time = ms();
        TRACE.lock(|t| t.borrow_mut().push(time, kind, a, b));
    }
}

pub fn cycle() {
    if STATE.load(Ordering::Relaxed) == 0 {
        MODE.store((MODE.load(Ordering::Relaxed) + 1) % 3, Ordering::Relaxed);
    }
}

pub fn toggle() {
    let state = STATE.load(Ordering::Relaxed);
    if state == 0 {
        STATE.store(1, Ordering::Relaxed);
    }
    if state == 2 {
        STATE.store(3, Ordering::Relaxed);
    }
}

pub fn message() -> &'static str {
    match STATE.load(Ordering::Relaxed) {
        0 => match MODE.load(Ordering::Relaxed) {
            0 => "BASE READY",
            1 => "SCREEN READY",
            _ => "TALL READY",
        },
        1 => "RESERVING",
        2 => match MODE.load(Ordering::Relaxed) {
            0 => "BASE RUNNING",
            1 => "SCREEN RUNNING",
            _ => "TALL RUNNING",
        },
        3 | 4 => "FINISHING",
        5 => "TEST SAVED",
        _ => "TEST FAILED",
    }
}

pub fn prepared(begin: bool) {
    note(
        if begin { f::PREP_BEGIN } else { f::PREP_END },
        CURRENT.load(Ordering::Relaxed),
        0,
    );
    if !begin && screenshot::status() != screenshot::Status::Saving {
        saving(false, screenshot::status() as u32);
    }
}

pub fn saving(begin: bool, result: u32) {
    if !begin && observing() {
        TRACE.lock(|t| {
            let mut t = t.borrow_mut();
            let i = if matches!(result, 2 | 7) { 13 } else { 14 };
            t.header[i] += 1;
        });
    }
    note(
        if begin { f::SAVE_BEGIN } else { f::SAVE_END },
        CURRENT.load(Ordering::Relaxed),
        result,
    );
}

pub fn stopped() {
    note(f::SNIFF_STOP, 0, 0);
}

pub fn packet(bytes: &[u8]) {
    if !observing() || bytes.len() < 7 {
        return;
    }
    let header = u16::from_le_bytes([bytes[1], bytes[2]]);
    let data = &bytes[3..bytes.len() - 4]; // Packet owns a CRC-checked token/header/body/CRC.
    let count = (header >> 12) & 7;
    let word = |offset: usize| -> u32 {
        data.get(offset..offset + 4)
            .map_or(0, |s| u32::from_le_bytes(s.try_into().unwrap()))
    };
    let good = header & 0x801f == 1 && count == 0;
    let keep =
        header & 0x801f == 0x8010 && count == 1 && data.get(2).is_some_and(|b| matches!(b, 3 | 4));
    let request =
        bytes[0] >> 5 == 7 && header & 0x8000 == 0 && matches!(header & 31, 2 | 9) && count != 0;
    let time = ms();
    let id = TRACE.lock(|t| {
        let mut t = t.borrow_mut();
        t.header[8] += 1;
        if good {
            t.header[9] += 1;
            return 0;
        }
        if keep {
            t.header[10] += 1;
            return 0;
        }
        let kind = 0x100 | u32::from(bytes[0] >> 5) | if count > 2 { 0x8000 } else { 0 };
        t.push(time, kind << 16 | u32::from(header), word(0), word(4));
        if request {
            t.header[12] += 1;
            t.header[12]
        } else {
            0
        }
    });
    if id != 0
        && MODE.load(Ordering::Relaxed) != 0
        && STATE.load(Ordering::Relaxed) == 2
        && PENDING
            .compare_exchange(0, id, Ordering::Relaxed, Ordering::Relaxed)
            .is_err()
    {
        note(f::MISSED, id, 1);
    }
}

pub fn owns(base: usize) -> bool {
    (0x62000..0x6e000).contains(&base)
        && base.is_multiple_of(flash::SECTOR)
        && flash::read(base) == f::MAGIC
        && flash::read(base + 4) == f::VERSION
        && flash::read(base + 8) == !f::MAGIC
        && flash::read(base + 12) == !f::VERSION
}

pub async fn poll() {
    let state = STATE.load(Ordering::Relaxed);
    if state == 1 && screenshot::status() != screenshot::Status::Saving {
        // This version is only for passive observation. Do not start a run in sink mode.
        let energy_off = crate::BUTTON_STATE.try_get().is_some_and(|input| {
            input.settings.energy_memory == firmware_core::settings::EnergyMemory::Off
        });
        if crate::latest_observation().pc13_high != Some(true) || !energy_off {
            STATE.store(6, Ordering::Relaxed);
            return;
        }
        let _bus = I2C3.lock().await;
        let free = (0x62000..0x6e000)
            .step_by(flash::SECTOR)
            .find(|&b| flash::blank(b));
        if let Some(base) = free
            && flash::program_blank(base, base, &[f::MAGIC, f::VERSION, !f::MAGIC, !f::VERSION])
        {
            BASE.store(base as u32, Ordering::Relaxed);
            TRACE.lock(|t| {
                t.borrow_mut()
                    .begin(u32::from(MODE.load(Ordering::Relaxed)), ms())
            });
            STATE.store(2, Ordering::Relaxed);
        } else {
            STATE.store(6, Ordering::Relaxed);
        }
    } else if state == 2 && screenshot::status() != screenshot::Status::Saving {
        let id = PENDING.swap(0, Ordering::Relaxed);
        if id != 0 {
            CURRENT.store(id, Ordering::Relaxed);
            note(f::REQUEST, id, u32::from(MODE.load(Ordering::Relaxed)));
            screenshot::request(if MODE.load(Ordering::Relaxed) == 2 {
                screenshot::Kind::Report
            } else {
                screenshot::Kind::Screen
            });
        }
    } else if state == 3 && screenshot::status() != screenshot::Status::Saving {
        PENDING.store(0, Ordering::Relaxed);
        STATE.store(4, Ordering::Relaxed);
        TRACE.lock(|t| t.borrow_mut().finish(ms()));
        let base = BASE.load(Ordering::Relaxed) as usize;
        let words = TRACE.lock(|t| t.borrow().words());
        for at in (4..words).step_by(8) {
            let mut batch = [!0; 8];
            let n = (words - at).min(8);
            TRACE.lock(|t| {
                let t = t.borrow();
                for (i, w) in batch[..n].iter_mut().enumerate() {
                    *w = t.word(at + i);
                }
            });
            {
                let _bus = I2C3.lock().await;
                if !flash::program_blank(base, base + at * 4, &batch[..n]) {
                    STATE.store(6, Ordering::Relaxed);
                    return;
                }
            }
            Timer::after_millis(1).await;
        }
        let _bus = I2C3.lock().await;
        STATE.store(
            if flash::program_blank(base, base + flash::SECTOR - 4, &[f::COMMIT]) {
                5
            } else {
                6
            },
            Ordering::Relaxed,
        );
    }
}
