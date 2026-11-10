//! A snapshot of the LCD header and every printable detail row.

use crate::buttons::Detail;
use crate::panel::WIDTH;
use crate::screenshot::{COMPACT_HEADER_BYTES, MAX_RECORD_BYTES, payload_crc};

pub(crate) const MAGIC: &[u8; 4] = b"X1SR";
pub(crate) const CABLE_MAGIC: &[u8; 4] = b"X1SC";
const HEADING: u8 = 0x80;
const COLUMNS: usize = 36;
pub const MAX_ROWS: usize = 512;
pub const HEADER_ROWS: usize = 24;
pub const ROW_HEIGHT: usize = 12;
pub const ROW_BYTES: usize = WIDTH as usize / 2;
pub const BAND_BYTES: usize = ROW_HEIGHT * ROW_BYTES;
pub const HEADER_BYTES: usize = HEADER_ROWS * ROW_BYTES;
pub const HEADER_RGB_BYTES: usize = HEADER_BYTES * 4;
pub const PALETTE_LEN: usize = 16;
const PALETTE_BYTES: usize = PALETTE_LEN * 2;
const ROWS_OFFSET: usize = PALETTE_BYTES + 2;
// The legacy Cable format has no page tag. Each header byte covers at most 16 pixels.
pub(crate) const MIN_REPORT_BYTES: usize =
    COMPACT_HEADER_BYTES + ROWS_OFFSET + (HEADER_BYTES * 2).div_ceil(16);

/// Store the header as palette runs, then store each nonblank row as text.
/// Palette entries 0, 1, and 2 are the background, text and headings.
pub fn encode_report<const COLS: usize>(
    page: Detail,
    header: &[u8; HEADER_RGB_BYTES],
    colors: [u16; 3],
    sections: impl IntoIterator<Item = [[u8; COLS]; 8]>,
    out: &mut [u8; MAX_RECORD_BYTES],
) -> Option<usize> {
    let mut palette = [0; PALETTE_LEN];
    palette[..3].copy_from_slice(&colors);
    let mut used = 3;
    let palette_at = COMPACT_HEADER_BYTES + 1;
    out[COMPACT_HEADER_BYTES] = match page {
        Detail::Pd => 0,
        Detail::Charger => 1,
        Detail::Cable => 2,
        Detail::System => 3,
    };
    let start = palette_at + ROWS_OFFSET;
    let mut size = start;
    for &pixel in header.as_chunks::<2>().0 {
        let color = u16::from_be_bytes(pixel);
        let index = match palette[..used].iter().position(|&entry| entry == color) {
            Some(index) => index,
            None => {
                *palette.get_mut(used)? = color;
                used += 1;
                used - 1
            }
        } as u8;
        if size > start && out[size - 1] & 0xf == index && out[size - 1] < 0xf0 {
            out[size - 1] += 0x10;
        } else {
            *out.get_mut(size)? = index;
            size += 1;
        }
    }
    let header_len = (size - start) as u16;
    let palette_out = &mut out[palette_at..palette_at + PALETTE_BYTES];
    for (entry, color) in palette_out.as_chunks_mut::<2>().0.iter_mut().zip(palette) {
        *entry = color.to_le_bytes();
    }
    out[start - 2..start].copy_from_slice(&header_len.to_le_bytes());

    let mut count = 0;
    for section in sections {
        for (index, line) in section.iter().enumerate() {
            let Some(last) = line.iter().rposition(u8::is_ascii_graphic) else {
                continue;
            };
            let len = last + 1;
            count += 1;
            if len > COLUMNS || count > MAX_ROWS {
                return None;
            }
            let entry = out.get_mut(size..size + len + 1)?;
            entry[0] = len as u8 | if index == 0 { HEADING } else { 0 };
            for (cell, &byte) in entry[1..].iter_mut().zip(line) {
                *cell = if byte.is_ascii_graphic() { byte } else { b' ' };
            }
            size += entry.len();
        }
    }
    if count == 0 {
        return None;
    }
    let crc = payload_crc(&out[COMPACT_HEADER_BYTES..size]);
    out[..4].copy_from_slice(MAGIC);
    out[4..8].copy_from_slice(&crc.to_le_bytes());
    out[8..COMPACT_HEADER_BYTES].copy_from_slice(&(size as u32).to_le_bytes());
    Some(size)
}

#[derive(Clone, Copy)]
pub struct DetailReport<'a> {
    page: Detail,
    palette: &'a [[u8; 2]; PALETTE_LEN],
    header: &'a [u8],
    rows: &'a [u8],
    count: u16,
}

impl<'a> DetailReport<'a> {
    pub(crate) fn open(payload: &'a [u8], legacy_cable: bool) -> Option<Self> {
        let (page, payload) = if legacy_cable {
            (Detail::Cable, payload)
        } else {
            let (&tag, payload) = payload.split_first()?;
            let page = match tag {
                0 => Detail::Pd,
                1 => Detail::Charger,
                2 => Detail::Cable,
                3 => Detail::System,
                _ => return None,
            };
            (page, payload)
        };
        let (palette, payload) = payload.split_at_checked(PALETTE_BYTES)?;
        let palette = palette.as_chunks::<2>().0.try_into().ok()?;
        let (&length, data) = payload.split_first_chunk::<2>()?;
        let (header, rows) = data.split_at_checked(usize::from(u16::from_le_bytes(length)))?;
        let pixels: usize = header.iter().map(|run| usize::from(run >> 4) + 1).sum();
        if pixels != HEADER_BYTES * 2 {
            return None;
        }
        let mut next = rows;
        let mut count = 0;
        while !next.is_empty() {
            let (_, line) = next_row(&mut next)?;
            if !line.iter().all(|byte| (b' '..=b'~').contains(byte)) {
                return None;
            }
            count += 1;
        }
        (1..=MAX_ROWS).contains(&count).then_some(Self {
            page,
            palette,
            header,
            rows,
            count: count as u16,
        })
    }

    pub fn page(self) -> Detail {
        self.page
    }

    pub fn height(self) -> usize {
        HEADER_ROWS + usize::from(self.count) * ROW_HEIGHT
    }

    pub fn color(self, index: usize) -> u16 {
        u16::from_le_bytes(self.palette[index])
    }

    pub fn expand_header(self, out: &mut [u8; HEADER_BYTES]) {
        let mut pixel = 0usize;
        for &run in self.header {
            let color = run & 0xf;
            for _ in 0..=run >> 4 {
                if pixel.is_multiple_of(2) {
                    out[pixel / 2] = color << 4;
                } else {
                    out[pixel / 2] |= color;
                }
                pixel += 1;
            }
        }
    }

    pub fn row(self, index: usize) -> Option<(bool, &'a str)> {
        let mut rows = self.rows;
        for _ in 0..index {
            next_row(&mut rows)?;
        }
        let (heading, line) = next_row(&mut rows)?;
        Some((heading, core::str::from_utf8(line).ok()?))
    }
}

fn next_row<'a>(rows: &mut &'a [u8]) -> Option<(bool, &'a [u8])> {
    let (&tag, text) = rows.split_first()?;
    let len = usize::from(tag & !HEADING);
    if !(1..=COLUMNS).contains(&len) {
        return None;
    }
    let (row, tail) = text.split_at_checked(len)?;
    *rows = tail;
    Some((tag & HEADING != 0, row))
}
