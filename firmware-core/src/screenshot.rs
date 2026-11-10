//! Lossless storage for 240×135 RGB565 frames with up to 64 colors.
//!
//! X1S2 and X1S3 store a CRC and length, a palette, and palette indices
//! XORed against the row above. X1S3 adds short backreferences and one-byte
//! runs of zero differences. The encoder picks the smaller record. X1S1
//! records from the first build remain readable.

use crate::buttons::{Detail, Page, Tab};
use crate::panel::{PIXELS, WIDTH};
use crate::screenshot_report::{
    CABLE_MAGIC, DetailReport, MAGIC as REPORT_MAGIC, MIN_REPORT_BYTES,
};

pub const FRAME_BYTES: usize = PIXELS * 2;
pub const COLORS: usize = 64;
const PALETTE_BYTES: usize = COLORS * 2;
const LEGACY_HEADER: usize = 8;
pub const LEGACY_RECORD_BYTES: usize = LEGACY_HEADER + PALETTE_BYTES + PIXELS * 3 / 4;
pub const COMPACT_HEADER_BYTES: usize = 12;
/// An 8 KiB sector minus the flash journal's five metadata words.
pub const MAX_RECORD_BYTES: usize = 8192 - 20;
pub const CAPTURE_SECTORS: usize = 6;
pub const CAPTURE_SPARE_SECTORS: usize = 2;
// Screen records are larger: even the densest packet covers at most 64 pixels per byte.
const _: () =
    assert!(COMPACT_HEADER_BYTES + PALETTE_BYTES + PIXELS.div_ceil(64) >= MIN_REPORT_BYTES);
// Allow room to export every record that could fit, even before journal overhead.
pub const CAPTURE_TABLE_LEN: usize =
    (CAPTURE_SECTORS + CAPTURE_SPARE_SECTORS) * (MAX_RECORD_BYTES / MIN_REPORT_BYTES);
const _: () = assert!(PIXELS.is_multiple_of(4) && PIXELS.is_multiple_of(6));
const MATCH_MIN: usize = 3;
const MATCH_MAX: usize = 66;
const MAX_DISTANCE: usize = 256;
const MATCH_TABLE_SIZE: usize = 128;
const _: () = assert!(PIXELS < u16::MAX as usize);

/// Stable IDs stored in the compact screen header. Old captures have ID zero.
#[derive(Clone, Copy)]
#[repr(u8)]
pub enum CapturePage {
    Unnamed = 0,
    Overview,
    Graph,
    Lines,
    Energy,
    Ripple,
    PdProfiles,
    Pps,
    Avs,
    Qc2,
    Qc3,
    Scp,
    Fcp,
    Afc,
    Pd,
    Charger,
    Cable,
    System,
    Settings,
}

impl CapturePage {
    pub fn from_page(page: Page) -> Self {
        match page.tab() {
            None => Self::Settings,
            Some(tab) => match tab {
                Tab::Overview => Self::Overview,
                Tab::Graph => Self::Graph,
                Tab::Lines => Self::Lines,
                Tab::Energy => Self::Energy,
                Tab::Ripple => Self::Ripple,
                Tab::Fixed => Self::PdProfiles,
                Tab::Pps => Self::Pps,
                Tab::Avs => Self::Avs,
                Tab::Qc2 => Self::Qc2,
                Tab::Qc3 => Self::Qc3,
                Tab::Scp => Self::Scp,
                Tab::Fcp => Self::Fcp,
                Tab::Afc => Self::Afc,
                Tab::Detail { page, .. } => Self::from_detail(page),
            },
        }
    }

    pub const fn from_detail(page: Detail) -> Self {
        match page {
            Detail::Pd => Self::Pd,
            Detail::Charger => Self::Charger,
            Detail::Cable => Self::Cable,
            Detail::System => Self::System,
        }
    }

    pub(crate) fn name(id: u8) -> &'static str {
        const NAMES: [&str; CapturePage::Settings as usize + 1] = [
            "SCREEN",
            "OVERVIEW",
            "GRAPH",
            "LINES",
            "ENERGY",
            "RIPPLE",
            "PDPROFILES",
            "PPS",
            "AVS",
            "QC2",
            "QC3",
            "SCP",
            "FCP",
            "AFC",
            "PD",
            "CHARGER",
            "CABLE",
            "SYSTEM",
            "SETTINGS",
        ];
        NAMES.get(usize::from(id)).copied().unwrap_or(NAMES[0])
    }
}

#[derive(Clone, Copy)]
enum Format {
    Runs,
    Matches,
}

pub struct Encoder<'a> {
    frame: &'a [[u8; 2]],
    palette: [[u8; 2]; COLORS],
    used: usize,
}

impl<'a> Encoder<'a> {
    pub fn new(frame: &'a [u8; FRAME_BYTES]) -> Option<Self> {
        let frame = frame.as_chunks::<2>().0;
        let mut palette = [[0; 2]; COLORS];
        let mut used = 0;
        for color in frame {
            if !palette[..used].contains(color) {
                *palette.get_mut(used)? = *color;
                used += 1;
            }
        }
        Some(Self {
            frame,
            palette,
            used,
        })
    }

    /// Measure both encodings before writing. Stream the smaller one from
    /// the unchanged frame without allocating a second frame buffer.
    pub fn record(&self) -> Option<(usize, impl Iterator<Item = u8> + '_)> {
        self.record_for(CapturePage::Unnamed)
    }

    pub fn record_for(&self, page: CapturePage) -> Option<(usize, impl Iterator<Item = u8> + '_)> {
        let (format, header, size) = [Format::Matches, Format::Runs]
            .into_iter()
            .filter_map(|format| {
                let (header, size) = self.header(format, page as u8)?;
                Some((format, header, size))
            })
            .min_by_key(|&(_, _, size)| size)?;
        Some((size, header.into_iter().chain(self.payload(format))))
    }

    fn header(&self, format: Format, page: u8) -> Option<([u8; COMPACT_HEADER_BYTES], usize)> {
        let mut crc = screen_crc_start(page);
        let mut size = COMPACT_HEADER_BYTES;
        for byte in self.payload(format) {
            if size == MAX_RECORD_BYTES {
                return None;
            }
            crc = crc_byte(crc, byte);
            size += 1;
        }
        let mut header = [0; COMPACT_HEADER_BYTES];
        header[..4].copy_from_slice(match format {
            Format::Runs => b"X1S2",
            Format::Matches => b"X1S3",
        });
        header[4..8].copy_from_slice(&(!crc).to_le_bytes());
        header[8..10].copy_from_slice(&(size as u16).to_le_bytes());
        header[11] = page;
        Some((header, size))
    }

    fn palette_index(&self, color: &[u8; 2]) -> u8 {
        self.palette[..self.used]
            .iter()
            .position(|entry| entry == color)
            .expect("the palette was built from this frame") as u8
    }

    fn payload(&self, format: Format) -> impl Iterator<Item = u8> + '_ {
        let mut runs = self.runs();
        let mut matches = self.matches();
        let packets = core::iter::from_fn(move || match format {
            Format::Runs => runs.next(),
            Format::Matches => matches.next(),
        });
        self.palette.as_flattened().iter().copied().chain(packets)
    }

    fn runs(&self) -> impl Iterator<Item = u8> + '_ {
        let mut above = [0u8; WIDTH as usize];
        let mut differences = self
            .frame
            .iter()
            .enumerate()
            .map(move |(pixel, color)| {
                let index = self.palette_index(color);
                let previous = core::mem::replace(&mut above[pixel % WIDTH as usize], index);
                index ^ previous
            })
            .peekable();
        let packets = core::iter::from_fn(move || {
            let difference = differences.next()?;
            if self.used == 2 {
                let mut bits = difference;
                for shift in 1..6 {
                    bits |= differences.next()? << shift;
                }
                return Some([0x40 | bits, 0].into_iter().take(1));
            }
            let mut count = 1;
            while count < 128 && differences.next_if_eq(&difference).is_some() {
                count += 1;
            }
            let (first, len) = match count {
                1 | 2 => (difference, count),
                _ => (0x80 | (count - 1) as u8, 2),
            };
            Some([first, difference].into_iter().take(len))
        });
        packets.flatten()
    }

    fn matches(&self) -> impl Iterator<Item = u8> + '_ {
        let mut position = 0;
        let mut last = [u16::MAX; MATCH_TABLE_SIZE];
        let difference = |at: usize| {
            let above = at
                .checked_sub(WIDTH as usize)
                .map_or(0, |above| self.palette_index(&self.frame[above]));
            self.palette_index(&self.frame[at]) ^ above
        };
        core::iter::from_fn(move || {
            let remaining = PIXELS - position;
            if remaining == 0 {
                return None;
            }
            let value = difference(position);
            let max_run = remaining.min(if value == 0 { 64 } else { MATCH_MAX });
            let mut run = 1;
            while run < max_run && difference(position + run) == value {
                run += 1;
            }
            let mut length = 0;
            let mut distance = 0;
            if remaining >= MATCH_MIN {
                // Try the last packet start with the same three-byte hash.
                let first = usize::from(value) * 31;
                let second = usize::from(difference(position + 1)) * 37;
                let third = usize::from(difference(position + 2)) * 41;
                let key = (first ^ second ^ third) & (MATCH_TABLE_SIZE - 1);
                let prior = usize::from(core::mem::replace(&mut last[key], position as u16));
                if let Some(back) = position.checked_sub(prior)
                    && (1..=MAX_DISTANCE).contains(&back)
                    && run < MATCH_MAX
                    && (value != 0 || run < 4)
                {
                    distance = back;
                    while length < remaining.min(MATCH_MAX)
                        && difference(position + length) == difference(prior + length)
                    {
                        length += 1;
                    }
                }
            }
            let (bytes, used, pixels) = if length >= MATCH_MIN && length > run {
                (
                    [0xc0 | (length - MATCH_MIN) as u8, (distance - 1) as u8],
                    2,
                    length,
                )
            } else if value == 0 {
                ([0x40 | (run - 1) as u8, 0], 1, run)
            } else if run >= MATCH_MIN {
                ([0x80 | (run - MATCH_MIN) as u8, value], 2, run)
            } else {
                ([value, 0], 1, 1)
            };
            position += pixels;
            Some(bytes.into_iter().take(used))
        })
        .flatten()
    }
}

/// Length from a compact record's header. This checks framing, before CRC.
pub fn compact_record_len(header: &[u8]) -> Option<usize> {
    let minimum_payload = match header.get(..4)? {
        b"X1S2" | b"X1S3" => PALETTE_BYTES,
        magic if (magic == CABLE_MAGIC || magic == REPORT_MAGIC) && *header.get(11)? == 0 => 2,
        _ => return None,
    };
    let length: [u8; 4] = header.get(8..COMPACT_HEADER_BYTES)?.try_into().ok()?;
    if length[2] != 0 {
        return None;
    }
    let size = usize::from(u16::from_le_bytes([length[0], length[1]]));
    (COMPACT_HEADER_BYTES + minimum_payload..=MAX_RECORD_BYTES)
        .contains(&size)
        .then_some(size)
}

/// One complete flash capture, in either export format.
#[derive(Clone, Copy)]
pub enum Capture<'a> {
    Screen(Record<'a>),
    Report(DetailReport<'a>),
}

impl<'a> Capture<'a> {
    pub fn open(bytes: &'a [u8]) -> Option<Self> {
        let legacy_cable = bytes.starts_with(CABLE_MAGIC);
        if legacy_cable || bytes.starts_with(REPORT_MAGIC) {
            let payload = bytes.get(COMPACT_HEADER_BYTES..compact_record_len(bytes)?)?;
            let expected = u32::from_le_bytes(bytes.get(4..8)?.try_into().ok()?);
            if payload_crc(payload) != expected {
                return None;
            }
            DetailReport::open(payload, legacy_cable).map(Self::Report)
        } else {
            Record::open(bytes).map(Self::Screen)
        }
    }
}

#[derive(Clone, Copy)]
enum Indices<'a> {
    Packed(&'a [[u8; 3]]),
    Differences(&'a [u8], Format),
}

/// A complete, validated record.
#[derive(Clone, Copy)]
pub struct Record<'a> {
    page: u8,
    palette: &'a [[u8; 2]],
    indices: Indices<'a>,
}

impl<'a> Record<'a> {
    pub fn open(bytes: &'a [u8]) -> Option<Self> {
        let expected_crc = u32::from_le_bytes(bytes.get(4..8)?.try_into().ok()?);
        let format = match bytes.get(..4)? {
            b"X1S1" if bytes.len() == LEGACY_RECORD_BYTES => None,
            b"X1S2" => Some(Format::Runs),
            b"X1S3" => Some(Format::Matches),
            _ => return None,
        };
        let payload = if format.is_some() {
            bytes.get(COMPACT_HEADER_BYTES..compact_record_len(bytes)?)?
        } else {
            &bytes[LEGACY_HEADER..]
        };
        let page = if format.is_some() { bytes[11] } else { 0 };
        let crc = payload
            .iter()
            .copied()
            .fold(screen_crc_start(page), crc_byte);
        if !crc != expected_crc {
            return None;
        }
        let (palette, data) = payload.split_at_checked(PALETTE_BYTES)?;
        let indices = if let Some(format) = format {
            let mut pixels = 0;
            let mut bytes = data;
            while !bytes.is_empty() {
                let packet = packet(&mut bytes, format)?;
                if let Packet::Copy(_, distance) = packet
                    && distance > pixels
                {
                    return None;
                }
                pixels += packet.count();
            }
            if pixels != PIXELS {
                return None;
            }
            Indices::Differences(data, format)
        } else {
            Indices::Packed(data.as_chunks::<3>().0)
        };
        Some(Self {
            page,
            palette: palette.as_chunks::<2>().0,
            indices,
        })
    }

    pub(crate) fn page_name(&self) -> &'static str {
        CapturePage::name(self.page)
    }

    /// Expand once into the export-mode frame buffer for random USB block reads.
    pub fn expand(self, out: &'a mut [u8; PIXELS]) -> Picture<'a> {
        self.expand_indices(out);
        self.picture(out)
    }

    pub(crate) fn expand_indices(self, out: &mut [u8; PIXELS]) {
        match self.indices {
            Indices::Packed(packed) => {
                for (&[a, b, c], pixels) in packed.iter().zip(out.as_chunks_mut::<4>().0) {
                    let indices = u32::from_le_bytes([a, b, c, 0]);
                    for (offset, pixel) in pixels.iter_mut().enumerate() {
                        *pixel = ((indices >> (6 * offset)) & 0x3f) as u8;
                    }
                }
            }
            Indices::Differences(mut bytes, format) => {
                let mut pixel = 0usize;
                while let Some(packet) = packet(&mut bytes, format) {
                    for offset in 0..packet.count() {
                        let difference = match packet {
                            Packet::Run(_, value) => value,
                            Packet::Bits(bits) => (bits >> offset) & 1,
                            Packet::Copy(_, distance) => {
                                let source = pixel - distance;
                                out[source] ^ above(out, source)
                            }
                        };
                        out[pixel] = difference ^ above(out, pixel);
                        pixel += 1;
                    }
                }
            }
        }
    }

    pub(crate) fn picture(self, indices: &'a [u8; PIXELS]) -> Picture<'a> {
        Picture {
            palette: self.palette,
            indices,
        }
    }
}

fn above(out: &[u8; PIXELS], pixel: usize) -> u8 {
    pixel
        .checked_sub(WIDTH as usize)
        .map_or(0, |above| out[above])
}

#[derive(Clone, Copy)]
enum Packet {
    Run(usize, u8),
    Bits(u8),
    Copy(usize, usize),
}

impl Packet {
    fn count(self) -> usize {
        match self {
            Self::Run(count, _) | Self::Copy(count, _) => count,
            Self::Bits(_) => 6,
        }
    }
}

fn packet(bytes: &mut &[u8], format: Format) -> Option<Packet> {
    let (&first, tail) = bytes.split_first()?;
    *bytes = tail;
    let low = usize::from(first & 0x3f);
    match (format, first) {
        (_, 0..=0x3f) => Some(Packet::Run(1, first)),
        (Format::Runs, 0x40..=0x7f) => Some(Packet::Bits(first & 0x3f)),
        (Format::Matches, 0x40..=0x7f) => Some(Packet::Run(low + 1, 0)),
        _ => {
            let (&data, tail) = bytes.split_first()?;
            *bytes = tail;
            match (format, first) {
                (Format::Matches, 0xc0..=0xff) => {
                    Some(Packet::Copy(low + MATCH_MIN, usize::from(data) + 1))
                }
                (Format::Matches, _) => {
                    (data < COLORS as u8).then_some(Packet::Run(low + MATCH_MIN, data))
                }
                (Format::Runs, _) => (data < COLORS as u8)
                    .then_some(Packet::Run(usize::from(first & 0x7f) + 1, data)),
            }
        }
    }
}

pub struct Picture<'a> {
    palette: &'a [[u8; 2]],
    indices: &'a [u8; PIXELS],
}

impl Picture<'_> {
    pub fn index(&self, pixel: usize) -> u8 {
        self.indices[pixel]
    }

    pub fn color(&self, index: u8) -> u16 {
        u16::from_be_bytes(self.palette[usize::from(index)])
    }
}

/// Verify a record and iterate its RGB565 pixels in LCD row order.
pub fn decode<'a>(
    record: &'a [u8],
    scratch: &'a mut [u8; PIXELS],
) -> Option<impl Iterator<Item = u16> + 'a> {
    let picture = Record::open(record)?.expand(scratch);
    Some((0..PIXELS).map(move |pixel| picture.color(picture.index(pixel))))
}

fn screen_crc_start(page: u8) -> u32 {
    if page == 0 { !0 } else { crc_byte(!0, page) }
}

fn crc_byte(mut crc: u32, byte: u8) -> u32 {
    crc ^= u32::from(byte);
    for _ in 0..8 {
        crc = (crc >> 1) ^ (0xedb8_8320 & 0u32.wrapping_sub(crc & 1));
    }
    crc
}

pub(crate) fn payload_crc(payload: &[u8]) -> u32 {
    !payload.iter().copied().fold(!0, crc_byte)
}
