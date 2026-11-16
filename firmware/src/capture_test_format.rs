//! Test-only, bounded trace format. Persisted only after the observation ends.
pub const MAGIC: u32 = 0x5454_3158; // X1TT
pub const VERSION: u32 = 1;
pub const COMMIT: u32 = 0x4b4f_5458;
pub const CAPACITY: usize = 128;
pub const HEADER_WORDS: usize = 16;
pub const START: u32 = 1;
pub const REQUEST: u32 = 2;
pub const PREP_BEGIN: u32 = 3;
pub const PREP_END: u32 = 4;
pub const SAVE_BEGIN: u32 = 5;
pub const SAVE_END: u32 = 6;
pub const END: u32 = 7;
pub const MISSED: u32 = 8;
pub const SNIFF_STOP: u32 = 9;
#[cfg(test)]
pub const PD_PARTNER: u32 = 0x107;

pub struct Trace {
    pub header: [u32; HEADER_WORDS],
    records: [[u32; 4]; CAPACITY],
}

impl Trace {
    pub const fn new() -> Self {
        Self {
            header: [0; HEADER_WORDS],
            records: [[0; 4]; CAPACITY],
        }
    }

    pub fn begin(&mut self, mode: u32, ms: u32) {
        self.header = [
            MAGIC, VERSION, !MAGIC, !VERSION, 0, mode, ms, 0, 0, 0, 0, 0, 0, 0, 0, 0,
        ];
        self.push(ms, START, mode, 0);
    }

    pub fn push(&mut self, ms: u32, kind_header: u32, a: u32, b: u32) {
        let at = self.header[4] as usize;
        if at < CAPACITY {
            self.records[at] = [ms, kind_header, a, b];
            self.header[4] += 1;
        } else {
            self.header[11] = self.header[11].saturating_add(1);
        }
    }

    pub fn finish(&mut self, ms: u32) {
        self.push(ms, END, 0, 0);
        self.header[7] = ms;
        let mut hash = 0x811c_9dc5;
        for i in 0..self.words() {
            if i != 15 {
                for byte in self.word(i).to_le_bytes() {
                    hash = (hash ^ u32::from(byte)).wrapping_mul(0x0100_0193);
                }
            }
        }
        self.header[15] = hash;
    }

    pub fn words(&self) -> usize {
        HEADER_WORDS + self.header[4] as usize * 4
    }

    pub fn word(&self, at: usize) -> u32 {
        if at < HEADER_WORDS {
            self.header[at]
        } else {
            self.records[(at - HEADER_WORDS) / 4][(at - HEADER_WORDS) % 4]
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn overflow_is_counted_without_overwriting_prior_evidence() {
        let mut t = Trace::new();
        t.begin(1, 100);
        for i in 1..CAPACITY + 7 {
            t.push(i as u32 + 100, REQUEST, i as u32, 0);
        }
        assert_eq!(t.header[4] as usize, CAPACITY);
        assert_eq!(t.header[11], 7);
        assert_eq!(t.word(HEADER_WORDS + 2), 1); // START mode
        assert_eq!(
            t.word(HEADER_WORDS + (CAPACITY - 1) * 4 + 2),
            (CAPACITY - 1) as u32
        );
    }

    #[test]
    fn restart_has_no_stale_records_and_checksum_covers_the_trace() {
        let mut t = Trace::new();
        t.begin(2, 100);
        t.push(110, PD_PARTNER << 16 | 0x2b89, 0xb046e00a, 0xd7c096f0);
        t.finish(200);
        let mut other = Trace::new();
        other.begin(2, 100);
        other.push(110, PD_PARTNER << 16 | 0x2b89, 0xb046e00a, 0xd7c096f1);
        other.finish(200);
        assert_ne!(t.header[15], other.header[15]);
        t.begin(0, 300);
        t.finish(400);
        assert_eq!(t.header[4], 2);
        assert_eq!(t.header[11], 0);
        assert_eq!(t.words(), HEADER_WORDS + 8);
        assert_eq!(t.header[5], 0);
    }
}
