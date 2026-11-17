//! Bounded selective logging policy, independent of flash and the PD driver.
//! Storage uses charging_log's versioned selective journals, not host XSC1.

pub const VOLTAGE_MV: u32 = 100;
pub const CURRENT_MA: u32 = 100;
pub const HEARTBEAT_MS: u32 = 60_000;

#[derive(Clone, Copy)]
struct Sample {
    at: u32,
    mv: u32,
    ma: i32,
}

pub struct Selector {
    last: Option<Sample>,
}
impl Default for Selector {
    fn default() -> Self {
        Self::new()
    }
}
impl Selector {
    pub const fn new() -> Self {
        Self { last: None }
    }
    pub fn keep(&self, at: u32, mv: u32, ma: i32) -> bool {
        match self.last {
            None => true,
            Some(last) => {
                mv.abs_diff(last.mv) >= VOLTAGE_MV
                    || ma.abs_diff(last.ma) >= CURRENT_MA
                    || at.saturating_sub(last.at) >= HEARTBEAT_MS
            }
        }
    }
    /// Advance only after the corresponding journal record committed.
    pub fn committed(&mut self, at: u32, mv: u32, ma: i32) {
        self.last = Some(Sample { at, mv, ma });
    }
}

/// Words copied from a COMPLETE, CRC-checked transport packet: word 0 is raw
/// header | (SOP code << 16), followed by every data object. Do not call for
/// incomplete input/diagnostics. Retain unknown SOP and inconsistent lengths.
pub fn routine(words: &[u32]) -> bool {
    let Some(&first) = words.first() else {
        return false;
    };
    if first >> 16 != 7 {
        return false;
    }
    let header = first as u16;
    let count = usize::from((header >> 12) & 7);
    if words.len() != 1 + count {
        return false;
    }
    let extended = header & 0x8000 != 0;
    let kind = header & 31;
    (!extended && kind == 1 && count == 0)
        || (extended && kind == 16 && count == 1 && matches!(words[1], 0x0003_8002 | 0x0004_8002))
}
