//! A read-only FAT12 volume with one BMP per saved capture.

use crate::panel::{HEIGHT, PIXELS, WIDTH};
use crate::screenshot::{CAPTURE_TABLE_LEN, COLORS, Capture, CapturePage, Picture};
use crate::screenshot_report::{
    BAND_BYTES, DetailReport, HEADER_BYTES, HEADER_ROWS, MAX_ROWS, PALETTE_LEN, ROW_BYTES,
    ROW_HEIGHT,
};

pub const BLOCK_BYTES: usize = 512;
#[cfg(feature = "capture-backup")]
pub const FLASH_SNAPSHOT_BYTES: usize = crate::screenshot::CAPTURE_SECTORS * 8192;
const FILE_SLOTS: usize = CAPTURE_TABLE_LEN + cfg!(feature = "capture-backup") as usize;
const CLUSTER_BLOCKS: usize = 64;
const ENTRIES_PER_BLOCK: usize = BLOCK_BYTES / 32;
const BMP_HEADER_BYTES: usize = 54;
const BMP_PIXELS: usize = BMP_HEADER_BYTES + COLORS * 4;
const BMP_BYTES: usize = BMP_PIXELS + PIXELS;
const REPORT_PIXELS: usize = BMP_HEADER_BYTES + PALETTE_LEN * 4;
const MAX_REPORT_BYTES: usize = REPORT_PIXELS + (HEADER_ROWS + MAX_ROWS * ROW_HEIGHT) * ROW_BYTES;
const FILE_CLUSTERS: usize = MAX_REPORT_BYTES.div_ceil(BLOCK_BYTES * CLUSTER_BLOCKS);
const FILE_BLOCKS: usize = FILE_CLUSTERS * CLUSTER_BLOCKS;
const CLUSTERS: usize = FILE_SLOTS * FILE_CLUSTERS;
const FAT_BLOCKS: usize = ((2 + CLUSTERS) * 3).div_ceil(2 * BLOCK_BYTES);
// Each BMP can have two long-name entries plus its short alias.
// Leave room for the volume label and the end-of-directory marker.
const ROOT_BLOCKS: usize = (3 * FILE_SLOTS + 2).div_ceil(ENTRIES_PER_BLOCK);
const LONG_NAME_CHARS: usize = 13;
const MAX_BMP_NAME: usize = b"PDPROFILES999.BMP".len();
const _: () = assert!(MAX_BMP_NAME <= 2 * LONG_NAME_CHARS);
const FIRST_ROOT: u32 = 1 + FAT_BLOCKS as u32;
const FIRST_DATA: u32 = FIRST_ROOT + ROOT_BLOCKS as u32;
pub const BLOCKS: u32 = FIRST_DATA + (FILE_SLOTS * FILE_BLOCKS) as u32;
const LABEL: &[u8; 11] = b"KWS X1     ";

const _: () = assert!(WIDTH.is_multiple_of(8));
const _: () = assert!(BMP_BYTES <= MAX_REPORT_BYTES && MAX_REPORT_BYTES < u32::MAX as usize);
const _: () = assert!(CLUSTERS < 4085);
const _: () = assert!(CAPTURE_TABLE_LEN < 1000);

pub type DrawReportRow = fn(bool, &str, &mut [u8; BAND_BYTES]);

enum File<'a> {
    Capture(Capture<'a>),
    #[cfg(feature = "capture-backup")]
    Flash(&'a [u8; FLASH_SNAPSHOT_BYTES]),
}

pub struct Disk<'a> {
    records: &'a [Option<Capture<'a>>; CAPTURE_TABLE_LEN],
    #[cfg(feature = "capture-backup")]
    flash: Option<&'a [u8; FLASH_SNAPSHOT_BYTES]>,
    scratch: &'a mut [u8; PIXELS],
    expanded: Option<(usize, usize)>,
    draw_report_row: DrawReportRow,
}

impl<'a> Disk<'a> {
    pub fn new(
        records: &'a [Option<Capture<'a>>; CAPTURE_TABLE_LEN],
        scratch: &'a mut [u8; PIXELS],
        draw_report_row: DrawReportRow,
    ) -> Self {
        Self {
            records,
            #[cfg(feature = "capture-backup")]
            flash: None,
            scratch,
            expanded: None,
            draw_report_row,
        }
    }

    #[cfg(feature = "capture-backup")]
    pub fn with_flash_snapshot(mut self, snapshot: &'a [u8; FLASH_SNAPSHOT_BYTES]) -> Self {
        self.flash = Some(snapshot);
        self
    }

    fn files(&self) -> impl Iterator<Item = File<'a>> + '_ {
        let files = self.records.iter().flatten().copied().map(File::Capture);
        #[cfg(feature = "capture-backup")]
        let files = files.chain(self.flash.map(File::Flash));
        files
    }

    pub fn read(&mut self, lba: u32, block: &mut [u8; BLOCK_BYTES]) {
        block.fill(0);
        match lba {
            0 => {
                block[..3].copy_from_slice(&[0xeb, 0x3c, 0x90]);
                block[3..11].copy_from_slice(b"MSDOS5.0");
                put16(block, 11, BLOCK_BYTES as u16);
                block[13] = CLUSTER_BLOCKS as u8;
                put16(block, 14, 1); // Reserved sectors.
                block[16] = 1; // One FAT.
                put16(block, 17, (ROOT_BLOCKS * ENTRIES_PER_BLOCK) as u16);
                if BLOCKS <= u16::MAX as u32 {
                    put16(block, 19, BLOCKS as u16);
                } else {
                    put32(block, 32, BLOCKS);
                }
                block[21] = 0xf8;
                put16(block, 22, FAT_BLOCKS as u16);
                put16(block, 24, 1); // Sectors per track.
                put16(block, 26, 1); // Heads.
                block[36] = 0x80;
                block[38] = 0x29;
                put32(block, 39, 0x5831_4345);
                block[43..54].copy_from_slice(LABEL);
                block[54..62].copy_from_slice(b"FAT12   ");
                block[510..].copy_from_slice(&[0x55, 0xaa]);
            }
            1..FIRST_ROOT => {
                let start = (lba - 1) as usize * BLOCK_BYTES;
                if start == 0 {
                    block[..3].copy_from_slice(&[0xf8, 0xff, 0xff]);
                }
                for (file, record) in self.files().enumerate() {
                    let used = record.len().div_ceil(BLOCK_BYTES * CLUSTER_BLOCKS);
                    let first = 2 + file * FILE_CLUSTERS;
                    for cluster in first..first + used {
                        let next = if cluster + 1 == first + used {
                            0xfff
                        } else {
                            (cluster + 1) as u16
                        };
                        let at = cluster + cluster / 2;
                        let entry = (next << (4 * (cluster % 2))).to_le_bytes();
                        // A FAT12 entry can straddle two blocks.
                        for (byte, value) in entry.into_iter().enumerate() {
                            if let Some(out) = block.get_mut((at + byte).wrapping_sub(start)) {
                                *out |= value;
                            }
                        }
                    }
                }
            }
            FIRST_ROOT..FIRST_DATA => self.read_root(lba, block),
            _ if lba < BLOCKS => {
                let data_block = (lba - FIRST_DATA) as usize;
                let file = data_block / FILE_BLOCKS;
                let record = self.files().nth(file);
                if let Some(record) = record {
                    let offset = data_block % FILE_BLOCKS * BLOCK_BYTES;
                    match record {
                        File::Capture(Capture::Screen(screen)) => {
                            if self.expanded != Some((file, 0)) {
                                screen.expand_indices(self.scratch);
                                self.expanded = Some((file, 0));
                            }
                            bmp(&screen.picture(self.scratch), offset, block);
                        }
                        File::Capture(Capture::Report(report)) => {
                            self.report_bmp(report, file, offset, block);
                        }
                        #[cfg(feature = "capture-backup")]
                        File::Flash(bytes) => {
                            if let Some(bytes) = bytes.get(offset..offset + BLOCK_BYTES) {
                                block.copy_from_slice(bytes);
                            }
                        }
                    }
                }
            }
            _ => {}
        }
    }

    #[inline(never)]
    fn read_root(&self, lba: u32, block: &mut [u8; BLOCK_BYTES]) {
        let first = (lba - FIRST_ROOT) as usize * ENTRIES_PER_BLOCK;
        let entries = block.as_chunks_mut::<32>().0;
        if first == 0 {
            entries[0][..11].copy_from_slice(LABEL);
            entries[0][11] = 8;
        }
        let mut position = 1;
        for (index, record) in self.files().enumerate() {
            if position >= first + ENTRIES_PER_BLOCK {
                break;
            }
            let name = record.name(index + 1);
            let long_entries = name.long_entries();
            let short_entry = position + long_entries;
            if short_entry >= first {
                for ordinal in (1..=long_entries).rev() {
                    if let Some(entry) =
                        entries.get_mut((short_entry - ordinal).wrapping_sub(first))
                    {
                        name.write_long(entry, ordinal, long_entries);
                    }
                }
                if let Some(entry) = entries.get_mut(short_entry - first) {
                    entry[..11].copy_from_slice(&name.short);
                    entry[11] = 0x21; // Archive, read only.
                    put16(entry, 24, 0x21); // 1980-01-01.
                    put16(entry, 26, (2 + index * FILE_CLUSTERS) as u16);
                    put32(entry, 28, record.len() as u32);
                }
            }
            position = short_entry + 1;
        }
    }

    fn report_bmp(
        &mut self,
        report: DetailReport<'_>,
        file: usize,
        offset: usize,
        block: &mut [u8; BLOCK_BYTES],
    ) {
        let size = report_bytes(report);
        if offset == 0 {
            bmp_header(block, size, REPORT_PIXELS, report.height(), 4, PALETTE_LEN);
            for (index, entry) in block[BMP_HEADER_BYTES..REPORT_PIXELS]
                .as_chunks_mut::<4>()
                .0
                .iter_mut()
                .enumerate()
            {
                palette_entry(entry, report.color(index));
            }
        }
        let end = (offset + BLOCK_BYTES).min(size);
        let mut at = offset.max(REPORT_PIXELS);
        while at < end {
            let pixel_bytes = at - REPORT_PIXELS;
            let (band, within, len) = if pixel_bytes < HEADER_BYTES {
                (0, pixel_bytes, HEADER_BYTES)
            } else {
                let body = pixel_bytes - HEADER_BYTES;
                (1 + body / BAND_BYTES, body % BAND_BYTES, BAND_BYTES)
            };
            if self.expanded != Some((file, band)) {
                if band == 0 {
                    report.expand_header(self.scratch.first_chunk_mut::<HEADER_BYTES>().unwrap());
                } else {
                    let scratch = self.scratch.first_chunk_mut::<BAND_BYTES>().unwrap();
                    scratch.fill(0);
                    if let Some((heading, line)) = report.row(band - 1) {
                        (self.draw_report_row)(heading, line, scratch);
                    }
                }
                self.expanded = Some((file, band));
            }
            let count = (end - at).min(len - within);
            block[at - offset..at - offset + count]
                .copy_from_slice(&self.scratch[within..within + count]);
            at += count;
        }
    }
}

fn report_bytes(report: DetailReport<'_>) -> usize {
    REPORT_PIXELS + report.height() * ROW_BYTES
}

impl File<'_> {
    fn len(&self) -> usize {
        match *self {
            File::Capture(Capture::Screen(_)) => BMP_BYTES,
            File::Capture(Capture::Report(report)) => report_bytes(report),
            #[cfg(feature = "capture-backup")]
            File::Flash(bytes) => bytes.len(),
        }
    }

    fn name(&self, number: usize) -> FileName {
        let prefix = match *self {
            File::Capture(Capture::Screen(screen)) => screen.page_name(),
            File::Capture(Capture::Report(report)) => {
                CapturePage::name(CapturePage::from_detail(report.page()) as u8)
            }
            #[cfg(feature = "capture-backup")]
            File::Flash(_) => {
                return FileName {
                    short: *b"CAPFLASHBIN",
                    long: [0; MAX_BMP_NAME],
                    len: 0,
                };
            }
        };
        FileName::bmp(prefix.as_bytes(), number)
    }
}

struct FileName {
    short: [u8; 11],
    long: [u8; MAX_BMP_NAME],
    len: usize,
}

impl FileName {
    fn bmp(prefix: &[u8], mut number: usize) -> Self {
        let digits = if number < 100 { 2 } else { 3 };
        let mut suffix = [0; 3];
        let suffix = &mut suffix[..digits];
        for digit in suffix.iter_mut().rev() {
            *digit = b'0' + (number % 10) as u8;
            number /= 10;
        }
        let mut name = Self {
            short: *b"        BMP",
            long: [0; MAX_BMP_NAME],
            len: 0,
        };
        let base_len = prefix.len() + digits;
        let short_prefix = if base_len <= 8 {
            prefix.len()
        } else {
            name.len = base_len + 4;
            name.long[..prefix.len()].copy_from_slice(prefix);
            name.long[prefix.len()..base_len].copy_from_slice(suffix);
            name.long[base_len..name.len].copy_from_slice(b".BMP");
            name.short[7 - digits] = b'~';
            7 - digits
        };
        name.short[..short_prefix].copy_from_slice(&prefix[..short_prefix]);
        let digit_start = if name.len == 0 {
            short_prefix
        } else {
            8 - digits
        };
        name.short[digit_start..digit_start + digits].copy_from_slice(suffix);
        name
    }

    fn long_entries(&self) -> usize {
        self.len.div_ceil(LONG_NAME_CHARS)
    }

    fn write_long(&self, entry: &mut [u8; 32], ordinal: usize, count: usize) {
        entry[0] = ordinal as u8 | if ordinal == count { 0x40 } else { 0 };
        entry[11] = 0x0f;
        entry[13] = self
            .short
            .iter()
            .fold(0u8, |sum, &byte| sum.rotate_right(1).wrapping_add(byte));
        // FAT stores the UTF-16 name in three runs around its metadata.
        for (index, offset) in [1, 3, 5, 7, 9, 14, 16, 18, 20, 22, 24, 28, 30]
            .into_iter()
            .enumerate()
        {
            let at = (ordinal - 1) * LONG_NAME_CHARS + index;
            let code = if at < self.len {
                u16::from(self.long[at])
            } else if at == self.len {
                0
            } else {
                0xffff
            };
            put16(entry, offset, code);
        }
    }
}

fn bmp(picture: &Picture<'_>, offset: usize, block: &mut [u8; BLOCK_BYTES]) {
    if offset == 0 {
        bmp_header(block, BMP_BYTES, BMP_PIXELS, usize::from(HEIGHT), 8, COLORS);
        for (index, entry) in block[BMP_HEADER_BYTES..BMP_PIXELS]
            .as_chunks_mut::<4>()
            .0
            .iter_mut()
            .enumerate()
        {
            palette_entry(entry, picture.color(index as u8));
        }
    }
    for (position, byte) in block.iter_mut().enumerate() {
        if let Some(pixel) = (offset + position).checked_sub(BMP_PIXELS)
            && pixel < PIXELS
        {
            *byte = picture.index(pixel);
        }
    }
}

fn bmp_header(
    out: &mut [u8; BLOCK_BYTES],
    size: usize,
    pixels: usize,
    height: usize,
    depth: u16,
    colors: usize,
) {
    out[..2].copy_from_slice(b"BM");
    put32(out, 2, size as u32);
    put32(out, 10, pixels as u32);
    put32(out, 14, 40); // BITMAPINFOHEADER.
    put32(out, 18, u32::from(WIDTH));
    put32(out, 22, (-(height as i32)) as u32); // Top-down LCD row order.
    put16(out, 26, 1);
    put16(out, 28, depth);
    put32(out, 34, (size - pixels) as u32);
    put32(out, 46, colors as u32);
}

fn palette_entry(entry: &mut [u8; 4], color: u16) {
    let blue = (color & 31) as u8;
    let green = ((color >> 5) & 63) as u8;
    let red = (color >> 11) as u8;
    *entry = [
        (blue << 3) | (blue >> 2),
        (green << 2) | (green >> 4),
        (red << 3) | (red >> 2),
        0,
    ];
}

fn put16(out: &mut [u8], at: usize, value: u16) {
    out[at..at + 2].copy_from_slice(&value.to_le_bytes());
}

fn put32(out: &mut [u8], at: usize, value: u32) {
    out[at..at + 4].copy_from_slice(&value.to_le_bytes());
}
