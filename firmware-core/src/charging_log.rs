//! Append-only charging log format and random-access, read-only CSV/FAT12 export.
//! All integers are little endian. The checksum word is programmed last.
pub const SECTOR: usize = 8192;
pub const HEADER: usize = 16;
pub const RECORD: usize = 16;
pub const SAMPLES: usize = (SECTOR - HEADER) / RECORD;
pub const MAGIC: u32 = 0x31474c58; // XLG1
pub const EVENT_MAGIC: u32 = 0x31454c58; // XLE1: PD event fragments
pub const JOURNAL_MAGIC: u32 = 0x32474c58; // XLG2: several sessions per sector
pub const JOURNAL_EVENT_MAGIC: u32 = 0x32454c58; // XLE2
pub const MAX_SESSIONS: u32 = 32;
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
pub fn journal_header(session: u32, segment: u32, event: bool) -> [u32; 4] {
    let mut w = [
        if event {
            JOURNAL_EVENT_MAGIC
        } else {
            JOURNAL_MAGIC
        },
        session,
        segment,
        0,
    ];
    w[3] = checksum(&w[..3]);
    w
}
/// Reserved elapsed value; normal data must stop before u32::MAX milliseconds.
pub fn session_marker(session: u32) -> [u32; 4] {
    record(u32::MAX, session, 0)
}
pub fn is_event(bytes: &[u8]) -> bool {
    matches!(
        words(bytes).map(|w| w[0]),
        Some(EVENT_MAGIC | JOURNAL_EVENT_MAGIC)
    )
}
pub fn is_journal(bytes: &[u8]) -> bool {
    matches!(
        words(bytes).map(|w| w[0]),
        Some(JOURNAL_MAGIC | JOURNAL_EVENT_MAGIC)
    )
}
fn marker(bytes: &[u8], w: [u32; 3]) -> Option<u32> {
    (is_journal(bytes) && w[0] == u32::MAX && w[2] == 0).then_some(w[1])
}
pub fn committed_slots(bytes: &[u8]) -> usize {
    if valid_header(bytes).is_none() {
        return 0;
    }
    bytes[HEADER..]
        .chunks_exact(RECORD)
        .take(SAMPLES)
        .take_while(|r| valid_record(r).is_some())
        .count()
}
pub fn latest_session(bytes: &[u8]) -> u32 {
    let Some(mut latest) = valid_header(bytes) else {
        return 0;
    };
    if is_journal(bytes) {
        for r in bytes[HEADER..]
            .chunks_exact(RECORD)
            .take(committed_slots(bytes))
        {
            if let Some(session) = marker(bytes, valid_record(r).unwrap()) {
                latest = latest.max(session);
            }
        }
    }
    latest
}
/// Never write past a torn record, or reuse a legacy format's tail.
pub fn reusable_tail(bytes: &[u8], event: bool) -> Option<usize> {
    if !is_journal(bytes) || is_event(bytes) != event || valid_header(bytes).is_none() {
        return None;
    }
    let n = committed_slots(bytes);
    (n + 1 < SAMPLES && bytes[HEADER + n * RECORD..].iter().all(|&b| b == 255)).then_some(n)
}
pub const BLOCK_BYTES: usize = 512;
pub const BLOCKS: u32 = 512;
const ROOT_BLOCKS: usize = 5; // 80 entries: volume + 64 streams + diagnostics
const FIRST_DATA: usize = 3 + ROOT_BLOCKS;
const FILES: usize = MAX_SESSIONS as usize * 2;
const RAW_BLOCKS: usize = 6 * SECTOR / BLOCK_BYTES;
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
    (matches!(
        w[0],
        MAGIC | EVENT_MAGIC | JOURNAL_MAGIC | JOURNAL_EVENT_MAGIC
    ) && w[3] == checksum(&w[..3]))
    .then_some(w[1])
}
pub fn valid_record(bytes: &[u8]) -> Option<[u32; 3]> {
    let w = words(bytes)?;
    (w[3] == checksum(&w[..3])).then_some([w[0], w[1], w[2]])
}

#[derive(Clone, Copy)]
struct File {
    session: u32,
    rows: usize,
    event: bool,
}
const EMPTY_FILE: File = File {
    session: 0,
    rows: 0,
    event: false,
};
pub struct Disk<'a> {
    sectors: [&'a [u8]; 6],
    counts: [usize; 6],
    order: [usize; 6],
    files: [File; FILES],
    nfiles: usize,
}
impl<'a> Disk<'a> {
    pub fn new(sectors: [&'a [u8]; 6]) -> Self {
        let counts = sectors.map(committed_slots);
        let mut order = [0, 1, 2, 3, 4, 5];
        order.sort_unstable_by_key(|&i| {
            let w = words(sectors[i]).unwrap_or([u32::MAX; 4]);
            (w[1], w[2], i)
        });
        let mut disk = Self {
            sectors,
            counts,
            order,
            files: [EMPTY_FILE; FILES],
            nfiles: 0,
        };
        for i in order {
            let Some(mut session) = valid_header(sectors[i]) else {
                continue;
            };
            let event = is_event(sectors[i]);
            for r in sectors[i][HEADER..].chunks_exact(RECORD).take(counts[i]) {
                let w = valid_record(r).unwrap();
                if let Some(next) = marker(sectors[i], w) {
                    session = next;
                    continue;
                }
                let file = (0..disk.nfiles)
                    .find(|&j| disk.files[j].session == session && disk.files[j].event == event);
                let j = if let Some(j) = file {
                    j
                } else {
                    if disk.nfiles == FILES {
                        continue;
                    }
                    let j = disk.nfiles;
                    disk.nfiles += 1;
                    disk.files[j] = File {
                        session,
                        rows: 0,
                        event,
                    };
                    j
                };
                disk.files[j].rows += 1;
            }
        }
        disk.files[..disk.nfiles].sort_unstable_by_key(|f| (f.session, f.event));
        disk
    }
    pub fn file_count(&self) -> usize {
        self.nfiles
    }
    fn filename(&self, i: usize) -> [u8; 11] {
        let f = self.files[i];
        if f.session > 9999 {
            let mut name = *b"LOG00000CSV";
            decimal(&mut name[3..8], i as u32 + 1);
            return name;
        }
        let mut name = *b"VI000001CSV";
        if f.event {
            name[..2].copy_from_slice(b"PD");
        }
        decimal(&mut name[2..6], f.session);
        name
    }
    fn status(&self) -> [u8; 512] {
        let mut out = [b' '; 512];
        let heading =
            b"KWS LOGGER STORAGE\r\nFREE SECTORS: 0/6\r\nSECTOR TYPE    FIRST ID   SLOTS\r\n";
        out[..heading.len()].copy_from_slice(heading);
        let free = free_mask(self.sectors);
        out[34] = b'0' + free.iter().filter(|&&f| f).count() as u8;
        let owned = clear_mask(self.sectors);
        for i in 0..6 {
            let mut row = *b"0      UNKNOWN 0000000000 0000\r\n";
            row[0] = b'1' + i as u8;
            let kind: &[u8; 7] = if free[i] {
                b"FREE   "
            } else if valid_header(self.sectors[i]).is_some() {
                match (is_journal(self.sectors[i]), is_event(self.sectors[i])) {
                    (true, false) => b"VI V2  ",
                    (true, true) => b"PD V2  ",
                    (false, false) => b"VI V1  ",
                    (false, true) => b"PD V1  ",
                }
            } else if owned[i] {
                b"CAPTURE"
            } else {
                b"UNKNOWN"
            };
            row[7..14].copy_from_slice(kind);
            decimal(&mut row[15..25], valid_header(self.sectors[i]).unwrap_or(0));
            decimal(&mut row[26..30], self.counts[i] as u32);
            let at = heading.len() + i * row.len();
            out[at..at + row.len()].copy_from_slice(&row);
        }
        let mut summary = *b"LATEST SESSION: 0000000000 / 0000000032\r\nV2 reuses tails; zero free sectors can still record.\r\n";
        decimal(
            &mut summary[16..26],
            self.sectors
                .map(latest_session)
                .into_iter()
                .max()
                .unwrap_or(0),
        );
        let at = heading.len() + 6 * 32;
        out[at..at + summary.len()].copy_from_slice(&summary);
        out[510..].copy_from_slice(b"\r\n");
        out
    }
    fn csv_header(&self, i: usize) -> &'static [u8] {
        if self.files[i].event {
            EVENT_CSV_HEADER
        } else {
            CSV_HEADER
        }
    }
    fn len(&self, i: usize) -> usize {
        self.csv_header(i).len() + self.files[i].rows * ROW
    }
    fn cluster(&self, i: usize) -> usize {
        2 + (0..i).map(|j| self.len(j).div_ceil(512)).sum::<usize>()
    }
    fn status_cluster(&self) -> usize {
        self.cluster(self.nfiles)
    }
    fn raw_cluster(&self) -> usize {
        self.status_cluster() + 1
    }
    fn row(&self, i: usize, mut index: usize) -> [u8; ROW] {
        let f = self.files[i];
        let mut found = [0; 3];
        'sectors: for &j in &self.order {
            let Some(mut session) = valid_header(self.sectors[j]) else {
                continue;
            };
            if is_event(self.sectors[j]) != f.event {
                continue;
            }
            for r in self.sectors[j][HEADER..]
                .chunks_exact(RECORD)
                .take(self.counts[j])
            {
                // Disk is a read-only snapshot of the CRC-checked prefix above.
                let v = words(r).unwrap();
                let w = [v[0], v[1], v[2]];
                if let Some(next) = marker(self.sectors[j], w) {
                    session = next;
                } else if session == f.session {
                    if index == 0 {
                        found = w;
                        break 'sectors;
                    }
                    index -= 1;
                }
            }
        }
        let mut row = [b'0'; ROW];
        decimal(&mut row[..10], f.session);
        row[10] = b',';
        decimal(&mut row[11..21], found[0]);
        row[21] = b',';
        decimal(&mut row[22..32], found[1]);
        row[32] = b',';
        let ma = found[2] as i32;
        row[33] = if ma < 0 { b'-' } else { b'+' };
        decimal(&mut row[34..44], ma.unsigned_abs());
        if f.event {
            decimal(&mut row[33..44], found[2]);
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
                put16(block, 17, (ROOT_BLOCKS * 16) as u16);
                put16(block, 19, BLOCKS as u16);
                block[21] = 0xf8;
                put16(block, 22, 2);
                put16(block, 24, 1);
                put16(block, 26, 1);
                block[38] = 0x29;
                put32(block, 39, JOURNAL_MAGIC);
                block[43..54].copy_from_slice(b"KWS LOGGER ");
                block[54..62].copy_from_slice(b"FAT12   ");
                block[510..].copy_from_slice(&[0x55, 0xaa]);
            }
            1..=2 => {
                let start = (lba as usize - 1) * 512;
                for cluster in 0..self.raw_cluster() + RAW_BLOCKS {
                    let value = if cluster == 0 {
                        0xff8
                    } else if cluster == 1 || cluster == self.status_cluster() {
                        0xfff
                    } else if cluster >= self.raw_cluster() {
                        if cluster + 1 == self.raw_cluster() + RAW_BLOCKS {
                            0xfff
                        } else {
                            (cluster + 1) as u16
                        }
                    } else {
                        let i = (0..self.nfiles)
                            .find(|&i| cluster < self.cluster(i) + self.len(i).div_ceil(512))
                            .unwrap();
                        if cluster + 1 == self.cluster(i) + self.len(i).div_ceil(512) {
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
            _ if (3..FIRST_DATA as u32).contains(&lba) => {
                for slot in 0..16 {
                    let entry = (lba as usize - 3) * 16 + slot;
                    let at = slot * 32;
                    if entry == 0 {
                        block[..11].copy_from_slice(b"KWS LOGGER ");
                        block[11] = 8;
                        continue;
                    }
                    let i = entry - 1;
                    let (name, cluster, len) = if i < self.nfiles {
                        (self.filename(i), self.cluster(i), self.len(i))
                    } else if i == self.nfiles {
                        (*b"STATUS  TXT", self.status_cluster(), 512)
                    } else if i == self.nfiles + 1 {
                        (*b"STORAGE BIN", self.raw_cluster(), 6 * SECTOR)
                    } else {
                        continue;
                    };
                    block[at..at + 11].copy_from_slice(&name);
                    block[at + 11] = 0x21;
                    put16(block, at + 24, 0x21);
                    put16(block, at + 26, cluster as u16);
                    put32(block, at + 28, len as u32);
                }
            }
            _ if lba >= FIRST_DATA as u32 && lba < BLOCKS => {
                let cluster = lba as usize - FIRST_DATA + 2;
                if cluster == self.status_cluster() {
                    block.copy_from_slice(&self.status());
                } else if (self.raw_cluster()..self.raw_cluster() + RAW_BLOCKS).contains(&cluster) {
                    let offset = (cluster - self.raw_cluster()) * BLOCK_BYTES;
                    if let Some(bytes) = self.sectors[offset / SECTOR]
                        .get(offset % SECTOR..offset % SECTOR + BLOCK_BYTES)
                    {
                        block.copy_from_slice(bytes);
                    }
                } else if let Some(i) = (0..self.nfiles).find(|&i| {
                    (self.cluster(i)..self.cluster(i) + self.len(i).div_ceil(512))
                        .contains(&cluster)
                }) {
                    let start = (cluster - self.cluster(i)) * 512;
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
/// Same blank/legacy-padding policy as the live writer, available for diagnostics.
pub fn free_mask(sectors: [&[u8]; 6]) -> [bool; 6] {
    let mut free = sectors.map(|s| s.len() == SECTOR && s.iter().all(|&b| b == 255));
    for start in [0, 3] {
        if capture_owner(sectors[start + 2], 0x3143_5358) {
            free[start..start + 3].fill(false);
        }
    }
    free
}
fn capture_owner(s: &[u8], tag: u32) -> bool {
    if s.len() != SECTOR {
        return false;
    }
    let at = SECTOR - 20;
    s[at..at + 4] == tag.to_le_bytes() && s[at + 4..at + 8] == 0xd9a2_651eu32.to_le_bytes()
}

#[cfg(test)]
mod tests {
    use super::*;
    fn put(b: &mut [u8], w: [u32; 4]) {
        for (dst, v) in b.chunks_exact_mut(4).zip(w) {
            dst.copy_from_slice(&v.to_le_bytes());
        }
    }
    fn sector(h: [u32; 4], records: &[[u32; 4]]) -> [u8; SECTOR] {
        let mut s = [255; SECTOR];
        put(&mut s[..16], h);
        for (i, &w) in records.iter().enumerate() {
            put(&mut s[16 + i * 16..32 + i * 16], w);
        }
        s
    }
    fn fat(d: &mut Disk<'_>) -> [u8; 1024] {
        let mut f = [0; 1024];
        d.read(1, (&mut f[..512]).try_into().unwrap());
        d.read(2, (&mut f[512..]).try_into().unwrap());
        f
    }
    fn fat_entry(f: &[u8], c: usize) -> usize {
        let at = c + c / 2;
        ((u16::from_le_bytes([f[at], f[at + 1]]) >> (4 * (c % 2))) & 0xfff) as usize
    }
    fn check_files(d: &mut Disk<'_>) {
        let f = fat(d);
        let mut block = [0; 512];
        assert!(d.raw_cluster() + RAW_BLOCKS + FIRST_DATA - 2 <= BLOCKS as usize);
        for i in 0..d.file_count() {
            let first = d.cluster(i);
            let n = d.len(i).div_ceil(512);
            let mut expected = d.csv_header(i).to_vec();
            for row in 0..d.files[i].rows {
                expected.extend_from_slice(&d.row(i, row));
            }
            let entry = i + 1;
            d.read((3 + entry / 16) as u32, &mut block);
            let at = entry % 16 * 32;
            assert_eq!(&block[at..at + 11], &d.filename(i));
            assert_eq!(
                u16::from_le_bytes(block[at + 26..at + 28].try_into().unwrap()) as usize,
                first
            );
            assert_eq!(
                u32::from_le_bytes(block[at + 28..at + 32].try_into().unwrap()) as usize,
                d.len(i)
            );
            for pos in (0..n).rev() {
                let c = first + pos;
                assert_eq!(fat_entry(&f, c), if pos + 1 == n { 0xfff } else { c + 1 });
                d.read((FIRST_DATA + c - 2) as u32, &mut block);
                for j in 0..512 {
                    assert_eq!(
                        block[j],
                        expected.get(pos * 512 + j).copied().unwrap_or(0),
                        "file {i} byte {}",
                        pos * 512 + j
                    );
                }
            }
        }
        assert_eq!(fat_entry(&f, d.status_cluster()), 0xfff);
        for n in 0..RAW_BLOCKS {
            let c = d.raw_cluster() + n;
            assert_eq!(
                fat_entry(&f, c),
                if n + 1 == RAW_BLOCKS { 0xfff } else { c + 1 }
            );
            d.read((FIRST_DATA + c - 2) as u32, &mut block);
            let off = n * 512;
            let expected = d.sectors[off / SECTOR]
                .get(off % SECTOR..off % SECTOR + 512)
                .unwrap_or(&[0; 512]);
            assert_eq!(&block[..], expected);
        }
        d.read(BLOCKS, &mut block);
        assert_eq!(block, [0; 512]);
    }
    #[test]
    fn deadlines_and_packet_capacity() {
        assert_eq!(next_deadline(500, 500), 1500);
        assert_eq!(next_deadline(1500, 1588), 2500);
        assert_eq!(next_deadline(2500, 5610), 6500);
        assert!(packet_fits(1, 3, true));
        assert!(!packet_fits(1, 3, false));
        assert!(packet_fits(3, 3, false));
        assert!(!packet_fits(0, 9, true));
    }
    #[test]
    fn legacy_and_v2_keep_unsigned_pd_and_signed_vi_csv() {
        for h in [header(7, 0), journal_header(7, 0, false)] {
            let s = sector(h, &[record(5000, 48000, i32::MIN)]);
            let mut d = Disk::new([&s, &[], &[], &[], &[], &[]]);
            assert_eq!(
                d.row(0, 0),
                *b"0000000007,0000005000,0000048000,-2147483648\r\n"
            );
            check_files(&mut d);
        }
        for h in [event_header(9, 2), journal_header(9, 2, true)] {
            let s = sector(h, &[record(1000, 256, -1)]);
            let mut d = Disk::new([&s, &[], &[], &[], &[], &[]]);
            assert_eq!(
                d.row(0, 0),
                *b"0000000009,0000001000,0000000256,04294967295\r\n"
            );
            check_files(&mut d);
        }
    }
    #[test]
    fn torn_headers_records_and_markers_keep_committed_prefix() {
        for h in [header(1, 0), journal_header(1, 0, false)] {
            for n in 0..4 {
                let mut s = [255; SECTOR];
                put(&mut s[..n * 4], h);
                assert_eq!(valid_header(&s), None);
                assert_eq!(Disk::new([&s; 6]).file_count(), 0);
            }
        }
        for w in [record(5000, 48000, -1234), session_marker(2)] {
            for n in 0..4 {
                let mut s = sector(journal_header(1, 0, false), &[record(0, 5000, 0)]);
                put(&mut s[32..32 + n * 4], w);
                assert_eq!(committed_slots(&s), 1);
                assert_eq!(Disk::new([&s, &[], &[], &[], &[], &[]]).file_count(), 1);
                // Zero programmed data words is truly blank; any partial write seals it.
                assert_eq!(
                    reusable_tail(&s, false).is_some(),
                    n == 0 || (n == 1 && w[0] == u32::MAX)
                );
                assert_eq!(latest_session(&s), 1);
            }
        }
        let mut s = sector(
            journal_header(1, 0, false),
            &[record(0, 5000, 0), session_marker(2), record(0, 9000, 0)],
        );
        assert_eq!(latest_session(&s), 2);
        s[49] ^= 1;
        assert_eq!(committed_slots(&s), 2);
        let d = Disk::new([&s, &[], &[], &[], &[], &[]]);
        assert_eq!(d.file_count(), 1);
        assert_eq!(latest_session(&s), 2);
    }
    #[test]
    fn sessions_share_sectors_and_continuations_merge_in_segment_order() {
        let vi = sector(
            journal_header(1, 0, false),
            &[record(0, 5000, 0), session_marker(2), record(0, 9000, 500)],
        );
        let pd0 = sector(journal_header(1, 1, true), &[record(0, 256, 1)]);
        let pd1 = sector(
            journal_header(1, 2, true),
            &[record(1, 257, 2), session_marker(2), record(0, 256, 3)],
        );
        // Deliberately reverse the physical continuation sectors.
        let mut d = Disk::new([&vi, &pd1, &pd0, &[], &[], &[]]);
        assert_eq!(d.file_count(), 4);
        for (i, name) in [
            b"VI000101CSV",
            b"PD000101CSV",
            b"VI000201CSV",
            b"PD000201CSV",
        ]
        .into_iter()
        .enumerate()
        {
            assert_eq!(&d.filename(i), name);
        }
        assert_eq!(d.files[1].rows, 2);
        assert_eq!(&d.row(1, 0)[33..44], b"00000000001");
        assert_eq!(&d.row(1, 1)[33..44], b"00000000002");
        assert_eq!(d.files[2].rows, 1);
        assert_eq!(reusable_tail(&vi, false), Some(3));
        assert_eq!(reusable_tail(&vi, true), None);
        check_files(&mut d);
    }
    #[test]
    fn all_64_stream_files_fit_directory_and_disk_even_at_full_capacity() {
        let mut sectors = [[255; SECTOR]; 6];
        for i in 0..6 {
            put(
                &mut sectors[i][..16],
                journal_header(1, i as u32, i % 2 != 0),
            );
            let mut slot = 0;
            for session in 1..=MAX_SESSIONS {
                if session > 1 {
                    put(
                        &mut sectors[i][16 + slot * 16..32 + slot * 16],
                        session_marker(session),
                    );
                    slot += 1;
                }
                put(
                    &mut sectors[i][16 + slot * 16..32 + slot * 16],
                    record(0, session, session as i32),
                );
                slot += 1;
            }
            while slot < SAMPLES {
                put(
                    &mut sectors[i][16 + slot * 16..32 + slot * 16],
                    record(slot as u32, 5000, 0),
                );
                slot += 1;
            }
            assert_eq!(reusable_tail(&sectors[i], i % 2 != 0), None);
        }
        let mut d = Disk::new(sectors.each_ref().map(|s| s.as_slice()));
        assert_eq!(d.file_count(), 64);
        check_files(&mut d);
        let status = d.status();
        let text = core::str::from_utf8(&status).unwrap();
        assert!(text.contains("FREE SECTORS: 0/6"));
        assert!(text.contains("LATEST SESSION: 0000000032"));
    }
    #[test]
    fn six_legacy_sectors_have_complete_separate_fat_chains() {
        let mut sectors = [[255; SECTOR]; 6];
        for i in 0..6 {
            put(&mut sectors[i][..16], header(i as u32 + 1, 0));
            for j in 0..SAMPLES {
                put(
                    &mut sectors[i][16 + j * 16..32 + j * 16],
                    record(j as u32, 5000, 0),
                );
            }
        }
        let mut d = Disk::new(sectors.each_ref().map(|s| s.as_slice()));
        assert_eq!(d.file_count(), 6);
        check_files(&mut d);
        assert_eq!(reusable_tail(&sectors[0], false), None);
    }
    #[test]
    fn free_unknown_and_capture_ownership_are_exact() {
        let mut s = [[255; SECTOR]; 6];
        put(&mut s[0][..16], journal_header(1, 0, false));
        s[1][SECTOR - 20..SECTOR - 16].copy_from_slice(&0x3243_5358u32.to_le_bytes());
        s[1][SECTOR - 16..SECTOR - 12].copy_from_slice(&0xd9a2_651eu32.to_le_bytes());
        s[2][0] = 0;
        s[5][SECTOR - 20..SECTOR - 16].copy_from_slice(&0x3143_5358u32.to_le_bytes());
        s[5][SECTOR - 16..SECTOR - 12].copy_from_slice(&0xd9a2_651eu32.to_le_bytes());
        let refs = s.each_ref().map(|s| s.as_slice());
        assert_eq!(clear_mask(refs), [true, true, false, true, true, true]);
        assert_eq!(free_mask(refs), [false; 6]);
        let d = Disk::new(refs);
        let status = d.status();
        let t = core::str::from_utf8(&status).unwrap();
        assert!(t.contains("3      UNKNOWN"));
        assert!(t.contains("1      VI V2"));
        assert_eq!(
            backup_clear_mask([false; 6], [true, false, true, true, false]),
            [true, false, true, true, false, false]
        );
    }
}
