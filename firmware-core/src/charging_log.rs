//! Append-only charging log format and random-access, read-only CSV/FAT12 export.
//! All integers are little endian. The checksum word is programmed last.
pub const SECTOR: usize = 8192;
pub const HEADER: usize = 16;
pub const RECORD: usize = 16;
pub const SAMPLES: usize = (SECTOR - HEADER) / RECORD;
pub const MAGIC: u32 = 0x31474c58; // XLG1
pub const EVENT_MAGIC: u32 = 0x31454c58; // XLE1: PD event fragments
pub const INTERVAL_MS: u64 = 1000;
/// Packet fragments may cross a sector, but never start a packet that cannot fit.
pub fn packet_fits(remaining: usize, words: usize, spare: bool) -> bool {
    words <= 8 && (words <= remaining || spare)
}

/// Exact backup matches augment recognized owners, never authorize other bytes.
pub fn backup_clear_mask(owned: [bool; 6], matches: [bool; 5]) -> [bool; 6] {
    core::array::from_fn(|i| owned[i] || (i < 5 && matches[i]))
}
/// Advance from the previous deadline; skip missed slots without catch-up bursts.
pub fn next_deadline(previous: u64, now: u64) -> u64 {
    previous + ((now - previous) / INTERVAL_MS + 1) * INTERVAL_MS
}
pub fn event_header(session: u32, segment: u32) -> [u32; 4] {
    let mut w = [EVENT_MAGIC, session, segment, 0];
    w[3] = checksum(&w[..3]);
    w
}
pub const BLOCK_BYTES: usize = 512;
pub const BLOCKS: u32 = 512;
const FIRST_DATA: usize = 4; // boot, two FAT sectors, root
const FILE_BLOCKS: usize = 48;
const CSV_HEADER: &[u8] = b"session,elapsed_ms,voltage_mV,current_mA\r\n";
const EVENT_CSV_HEADER: &[u8] = b"session,elapsed_ms,event_tag,raw_u32\r\n";
const ROW: usize = 46;

pub fn checksum(words: &[u32]) -> u32 {
    let mut crc = !0u32;
    for word in words {
        for byte in word.to_le_bytes() {
            crc ^= u32::from(byte);
            for _ in 0..8 {
                crc = (crc >> 1) ^ (0xedb88320 & 0u32.wrapping_sub(crc & 1));
            }
        }
    }
    // Never use the erased value as a commit marker.
    crc & 0x7fffffff
}

pub fn header(session: u32, segment: u32) -> [u32; 4] {
    let mut w = [MAGIC, session, segment, 0];
    w[3] = checksum(&w[..3]);
    w
}
pub fn record(elapsed_ms: u32, mv: u32, ma: i32) -> [u32; 4] {
    let mut w = [elapsed_ms, mv, ma as u32, 0];
    w[3] = checksum(&w[..3]);
    w
}
fn words(bytes: &[u8]) -> Option<[u32; 4]> {
    let b = bytes.get(..16)?;
    Some(core::array::from_fn(|i| {
        u32::from_le_bytes(b[i * 4..i * 4 + 4].try_into().unwrap())
    }))
}
pub fn valid_header(bytes: &[u8]) -> Option<u32> {
    let w = words(bytes)?;
    (matches!(w[0], MAGIC | EVENT_MAGIC) && w[3] == checksum(&w[..3])).then_some(w[1])
}
pub fn valid_record(bytes: &[u8]) -> Option<[u32; 3]> {
    let w = words(bytes)?;
    (w[3] == checksum(&w[..3])).then_some([w[0], w[1], w[2]])
}

pub struct Disk<'a> {
    sectors: [&'a [u8]; 6],
    counts: [usize; 6],
}
impl<'a> Disk<'a> {
    pub fn new(sectors: [&'a [u8]; 6]) -> Self {
        let counts = sectors.map(|s| {
            if valid_header(s).is_some() {
                s[HEADER..]
                    .chunks_exact(RECORD)
                    .take(SAMPLES)
                    .take_while(|r| valid_record(r).is_some())
                    .count()
            } else {
                0
            }
        });
        Self { sectors, counts }
    }
    pub fn file_count(&self) -> usize {
        self.counts.iter().filter(|&&n| n != 0).count()
    }
    fn csv_header(&self, i: usize) -> &'static [u8] {
        if self.sectors[i].get(..4) == Some(&EVENT_MAGIC.to_le_bytes()) {
            EVENT_CSV_HEADER
        } else {
            CSV_HEADER
        }
    }
    fn len(&self, i: usize) -> usize {
        self.csv_header(i).len() + self.counts[i] * ROW
    }
    #[cfg(test)]
    fn csv_byte(&self, i: usize, offset: usize) -> u8 {
        if offset < self.csv_header(i).len() {
            return self.csv_header(i)[offset];
        }
        if offset >= self.len(i) {
            return 0;
        }
        let offset = offset - self.csv_header(i).len();
        self.row(i, offset / ROW)[offset % ROW]
    }
    fn row(&self, i: usize, index: usize) -> [u8; ROW] {
        let at = HEADER + index * RECORD;
        let w = valid_record(&self.sectors[i][at..]).unwrap();
        let mut row = [b'0'; ROW];
        decimal(&mut row[..10], valid_header(self.sectors[i]).unwrap());
        row[10] = b',';
        decimal(&mut row[11..21], w[0]);
        row[21] = b',';
        decimal(&mut row[22..32], w[1]);
        row[32] = b',';
        let ma = w[2] as i32;
        row[33] = if ma < 0 { b'-' } else { b'+' };
        decimal(&mut row[34..44], ma.unsigned_abs());
        if self.csv_header(i) == EVENT_CSV_HEADER {
            decimal(&mut row[33..44], w[2]);
        }
        row[44..].copy_from_slice(b"\r\n");
        row
    }
    pub fn read(&mut self, lba: u32, block: &mut [u8; BLOCK_BYTES]) {
        block.fill(0);
        match lba {
            0 => {
                block[..3].copy_from_slice(&[0xeb, 0x3c, 0x90]);
                block[3..11].copy_from_slice(b"KWSLOG  ");
                put16(block, 11, 512);
                block[13] = 1;
                put16(block, 14, 1);
                block[16] = 1;
                put16(block, 17, 16);
                put16(block, 19, BLOCKS as u16);
                block[21] = 0xf8;
                put16(block, 22, 2);
                put16(block, 24, 1);
                put16(block, 26, 1);
                block[38] = 0x29;
                put32(block, 39, 0x31474c58);
                block[43..54].copy_from_slice(b"KWS LOGGER ");
                block[54..62].copy_from_slice(b"FAT12   ");
                block[510..].copy_from_slice(&[0x55, 0xaa]);
            }
            1..=2 => {
                let start = (lba as usize - 1) * 512;
                for cluster in 0..(2 + 6 * FILE_BLOCKS) {
                    let value = if cluster == 0 {
                        0xff8
                    } else if cluster == 1 {
                        0xfff
                    } else {
                        let i = (cluster - 2) / FILE_BLOCKS;
                        let pos = (cluster - 2) % FILE_BLOCKS;
                        let used = if self.counts[i] == 0 {
                            0
                        } else {
                            self.len(i).div_ceil(512)
                        };
                        if pos >= used {
                            0
                        } else if pos + 1 == used {
                            0xfff
                        } else {
                            (cluster + 1) as u16
                        }
                    };
                    let at = cluster + cluster / 2;
                    for (j, b) in (value << (4 * (cluster % 2)))
                        .to_le_bytes()
                        .iter()
                        .enumerate()
                    {
                        if let Some(out) = block.get_mut((at + j).wrapping_sub(start)) {
                            *out |= b;
                        }
                    }
                }
            }
            3 => {
                block[..11].copy_from_slice(b"KWS LOGGER ");
                block[11] = 8;
                let mut entry = 32;
                for i in 0..6 {
                    if self.counts[i] == 0 {
                        continue;
                    }
                    block[entry..entry + 11].copy_from_slice(b"LOG00001CSV");
                    block[entry + 7] = b'1' + i as u8;
                    block[entry + 11] = 0x21;
                    put16(block, entry + 24, 0x21);
                    put16(block, entry + 26, (2 + i * FILE_BLOCKS) as u16);
                    put32(block, entry + 28, self.len(i) as u32);
                    entry += 32;
                }
            }
            _ if lba < BLOCKS => {
                let data = lba as usize - FIRST_DATA;
                let i = data / FILE_BLOCKS;
                if i < 6 && self.counts[i] != 0 {
                    let start = (data % FILE_BLOCKS) * 512;
                    let end = (start + 512).min(self.len(i));
                    let mut at = start;
                    while at < end {
                        if at < self.csv_header(i).len() {
                            let n = (self.csv_header(i).len() - at).min(end - at);
                            block[at - start..at - start + n]
                                .copy_from_slice(&self.csv_header(i)[at..at + n]);
                            at += n;
                        } else {
                            let offset = at - self.csv_header(i).len();
                            let row = self.row(i, offset / ROW);
                            let within = offset % ROW;
                            let n = (ROW - within).min(end - at);
                            block[at - start..at - start + n]
                                .copy_from_slice(&row[within..within + n]);
                            at += n;
                        }
                    }
                }
            }
            _ => {}
        }
    }
}
fn decimal(out: &mut [u8], mut n: u32) {
    for b in out.iter_mut().rev() {
        *b = b'0' + (n % 10) as u8;
        n /= 10;
    }
}
fn put16(b: &mut [u8], at: usize, v: u16) {
    b[at..at + 2].copy_from_slice(&v.to_le_bytes());
}
fn put32(b: &mut [u8], at: usize, v: u32) {
    b[at..at + 4].copy_from_slice(&v.to_le_bytes());
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn deadline_skips_missed_slots_without_drift() {
        assert_eq!(next_deadline(500, 500), 1500);
        assert_eq!(next_deadline(1500, 1588), 2500);
        assert_eq!(next_deadline(2500, 5610), 6500);
    }
    #[test]
    fn event_sectors_export_unsigned_payload_and_survive_torn_tail() {
        let mut s = [0xff; SECTOR];
        put(&mut s[..16], event_header(9, 2));
        for i in 0..SAMPLES {
            put(
                &mut s[16 + i * 16..32 + i * 16],
                record(1000, 256 + i as u32, -1),
            );
        }
        let mut disk = Disk::new([&s, &[], &[], &[], &[], &[]]);
        assert_eq!(
            disk.row(0, 0),
            *b"0000000009,0000001000,0000000256,04294967295\r\n"
        );
        let mut b = [0; 512];
        for lba in (4..52).rev() {
            disk.read(lba, &mut b);
            for j in 0..512 {
                assert_eq!(b[j], disk.csv_byte(0, (lba as usize - 4) * 512 + j));
            }
        }
        assert!(clear_mask([&s, &[], &[], &[], &[], &[]])[0]);
        s[SECTOR - 4..].fill(0xff);
        assert_eq!(
            Disk::new([&s, &[], &[], &[], &[], &[]]).counts[0],
            SAMPLES - 1
        );
    }
    #[test]
    fn foreign_and_torn_headers_are_not_exported() {
        let mut s = [0xff; SECTOR];
        let h = header(1, 0);
        for n in 0..4 {
            put(&mut s[..n * 4], h);
            assert!(valid_header(&s).is_none());
            assert_eq!(Disk::new([&s; 6]).file_count(), 0);
        }
        put(&mut s[..16], h);
        assert_eq!(valid_header(&s), Some(1));
        s[0] ^= 1;
        assert!(valid_header(&s).is_none());
    }
    #[test]
    fn six_files_have_separate_complete_fat_chains() {
        let mut s = [0xff; SECTOR];
        put(&mut s[..16], header(1, 0));
        for i in 0..SAMPLES {
            put(&mut s[16 + i * 16..32 + i * 16], record(i as u32, 5000, 0));
        }
        let mut disk = Disk::new([&s; 6]);
        let mut fat = [0; 1024];
        disk.read(1, (&mut fat[..512]).try_into().unwrap());
        disk.read(2, (&mut fat[512..]).try_into().unwrap());
        let used = (CSV_HEADER.len() + SAMPLES * ROW).div_ceil(512);
        for file in 0..6 {
            for pos in 0..FILE_BLOCKS {
                let cluster = 2 + file * FILE_BLOCKS + pos;
                let at = cluster + cluster / 2;
                let value =
                    (u16::from_le_bytes([fat[at], fat[at + 1]]) >> (4 * (cluster % 2))) & 0xfff;
                let expected = if pos >= used {
                    0
                } else if pos + 1 == used {
                    0xfff
                } else {
                    (cluster + 1) as u16
                };
                assert_eq!(value, expected);
            }
        }
        let mut block = [0; 512];
        disk.read(3, &mut block);
        for file in 0..6 {
            let entry = (file + 1) * 32;
            assert_eq!(block[entry + 7], b'1' + file as u8);
            assert_eq!(
                u16::from_le_bytes([block[entry + 26], block[entry + 27]]) as usize,
                2 + file * FILE_BLOCKS
            );
        }
        disk.read(BLOCKS, &mut block);
        assert_eq!(block, [0; 512]);
    }
    fn put(b: &mut [u8], w: [u32; 4]) {
        for (dst, v) in b.chunks_exact_mut(4).zip(w) {
            dst.copy_from_slice(&v.to_le_bytes());
        }
    }
    #[test]
    fn torn_records_and_corruption() {
        let mut b = [0xff; 16];
        let w = record(5000, 48000, -1234);
        for n in 0..4 {
            put(&mut b[..n * 4], w);
            assert!(valid_record(&b).is_none());
        }
        put(&mut b, w);
        assert_eq!(valid_record(&b), Some([5000, 48000, (-1234i32) as u32]));
        b[2] ^= 1;
        assert!(valid_record(&b).is_none());
    }
    #[test]
    fn fat_and_random_csv_reads() {
        let mut s = [0xff; SECTOR];
        put(&mut s[..16], header(7, 0));
        for i in 0..SAMPLES {
            put(
                &mut s[16 + i * 16..32 + i * 16],
                record(i as u32 * 5000, 48000, i32::MIN),
            );
        }
        let mut disk = Disk::new([&s, &[], &[], &[], &[], &[]]);
        assert_eq!(disk.file_count(), 1);
        let mut b = [0; 512];
        disk.read(3, &mut b);
        assert_eq!(&b[32..43], b"LOG00001CSV");
        assert_eq!(
            u32::from_le_bytes(b[60..64].try_into().unwrap()) as usize,
            CSV_HEADER.len() + 511 * 46
        );
        disk.read(4, &mut b);
        assert_eq!(&b[..CSV_HEADER.len()], CSV_HEADER);
        assert_eq!(
            &b[CSV_HEADER.len()..CSV_HEADER.len() + 46],
            b"0000000007,0000000000,0000048000,-2147483648\r\n"
        );
        for lba in (4..52).rev() {
            disk.read(lba, &mut b);
            for j in 0..512 {
                assert_eq!(b[j], disk.csv_byte(0, (lba as usize - 4) * 512 + j));
            }
        }
        disk.read(1, &mut b);
        assert_eq!(&b[..3], &[0xf8, 0xff, 0xff]);
        // An interrupted final record does not hide the earlier committed rows.
        s[SECTOR - 4..].fill(0xff);
        let disk = Disk::new([&s, &[], &[], &[], &[], &[]]);
        assert_eq!(disk.counts[0], 510);
    }
}

/// Recognized owners only. Snapshot this mask before erasing, keeping each
/// legacy three-sector marker (in the last sector) until its payload is gone.
pub fn clear_mask(sectors: [&[u8]; 6]) -> [bool; 6] {
    let mut mask = sectors.map(|s| valid_header(s).is_some() || capture_owner(s, 0x3243_5358));
    for start in [0, 3] {
        if capture_owner(sectors[start + 2], 0x3143_5358) {
            mask[start..start + 3].fill(true);
        }
    }
    mask
}
fn capture_owner(s: &[u8], tag: u32) -> bool {
    if s.len() != SECTOR {
        return false;
    }
    let at = SECTOR - 20;
    s[at..at + 4] == tag.to_le_bytes() && s[at + 4..at + 8] == 0xd9a2_651eu32.to_le_bytes()
}
#[cfg(test)]
mod clear_tests {
    use super::*;
    #[test]
    fn packet_boundary_is_all_or_nothing() {
        assert!(packet_fits(1, 3, true)); // continue into next sector
        assert!(!packet_fits(1, 3, false)); // no partial header/payload
        assert!(packet_fits(3, 3, false));
        assert!(packet_fits(0, 8, true));
        assert!(!packet_fits(0, 1, false));
    }
    #[test]
    fn backup_matches_reclaim_only_reserved_known_bytes() {
        // Reproduce four blocked sectors plus the measurement/event pair.
        assert_eq!(
            backup_clear_mask(
                [false, false, false, false, true, true],
                [true, true, true, true, false]
            ),
            [true; 6]
        );
        // One altered/unknown sector remains protected; repeat clear is safe.
        assert_eq!(
            backup_clear_mask([false; 6], [true, false, true, true, false]),
            [true, false, true, true, false, false]
        );
    }
    #[test]
    fn only_recognized_owners_are_cleared() {
        let mut sectors = [[0xffu8; SECTOR]; 6];
        for (i, word) in header(1, 0).iter().enumerate() {
            sectors[0][i * 4..i * 4 + 4].copy_from_slice(&word.to_le_bytes());
        }
        sectors[1][SECTOR - 20..SECTOR - 16].copy_from_slice(&0x3243_5358u32.to_le_bytes());
        sectors[1][SECTOR - 16..SECTOR - 12].copy_from_slice(&0xd9a2_651eu32.to_le_bytes());
        sectors[2][0] = 0; // unknown occupied sector
        sectors[5][SECTOR - 20..SECTOR - 16].copy_from_slice(&0x3143_5358u32.to_le_bytes());
        sectors[5][SECTOR - 16..SECTOR - 12].copy_from_slice(&0xd9a2_651eu32.to_le_bytes());
        assert_eq!(
            clear_mask(sectors.each_ref().map(|s| s.as_slice())),
            [true, true, false, true, true, true]
        );
        // Missing/partial ownership marker must never authorize an erase.
        sectors[1][SECTOR - 16] ^= 1;
        sectors[5][SECTOR - 16] ^= 1;
        assert_eq!(
            clear_mask(sectors.each_ref().map(|s| s.as_slice())),
            [true, false, false, false, false, false]
        );
    }
}
