//! One-step install: the unpacker takes the upper region from the flasher over USB HID.

use crate::{
    CONTAINER_PREFIX, EXTENDED_PAYLOAD_LEN, SECTOR_BYTES, UPPER_BASE, UPPER_END, region_hash,
};

/// Shared generic HID ID; the flasher also matches both strings.
pub const VID: u16 = 0x16c0;
pub const PID: u16 = 0x05df;
pub const MANUFACTURER: &str = "usb.pd.epr.240w@gmail.com";
pub const PRODUCT: &str = "KWS-X1 Installer";

pub const REPORT_BYTES: usize = 64;
/// Region bytes carried by one DATA report, after its 8-byte header.
pub const DATA_BYTES: usize = 56;
pub const VERSION: u8 = 1;
pub type Report = [u8; REPORT_BYTES];

const MAGIC: [u8; 4] = *b"X1UP";
const QUERY: u8 = b'Q';
const DATA: u8 = b'D';
const INFO: u8 = b'I';
const ACK: u8 = b'A';
const SECTOR: usize = SECTOR_BYTES as usize;

/// Vendor-defined HID: one 64-byte input and output report.
pub const REPORT_DESCRIPTOR: [u8; 25] = [
    0x06, 0x00, 0xff, 0x09, 0x01, 0xa1, 0x01, 0x15, 0x00, 0x26, 0xff, 0x00, 0x75, 0x08, 0x95, 0x40,
    0x09, 0x01, 0x81, 0x02, 0x09, 0x01, 0x91, 0x02, 0xc0,
];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum Status {
    /// The sector is programmed; send from `next`.
    Next = 0,
    /// The whole region is programmed and its hash matches.
    Done = 1,
    /// The report was out of order; resend from `next`.
    BadOffset = 2,
    WriteFailed = 3,
    HashMismatch = 4,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Reply {
    Info { needed: bool, len: u32, hash: u32 },
    Ack { status: Status, next: u32 },
}

fn word(bytes: &[u8], at: usize) -> Option<u32> {
    Some(u32::from_le_bytes(*bytes.get(at..)?.first_chunk()?))
}

pub fn query() -> Report {
    let mut report = [0; REPORT_BYTES];
    report[0] = QUERY;
    report[1..5].copy_from_slice(&MAGIC);
    report[5] = VERSION;
    report
}

/// A DATA report; `bytes` holds at most [`DATA_BYTES`].
pub fn data(offset: u32, bytes: &[u8]) -> Option<Report> {
    if bytes.is_empty() || bytes.len() > DATA_BYTES {
        return None;
    }
    let mut report = [0; REPORT_BYTES];
    report[0] = DATA;
    report[1] = bytes.len() as u8;
    report[4..8].copy_from_slice(&offset.to_le_bytes());
    report[8..8 + bytes.len()].copy_from_slice(bytes);
    Some(report)
}

fn info(needed: bool, len: u32, hash: u32) -> Report {
    let mut report = [0; REPORT_BYTES];
    report[0] = INFO;
    report[1] = VERSION;
    report[2] = u8::from(needed);
    report[4..8].copy_from_slice(&len.to_le_bytes());
    report[8..12].copy_from_slice(&hash.to_le_bytes());
    report
}

fn ack(status: Status, next: usize) -> Report {
    let mut report = [0; REPORT_BYTES];
    report[0] = ACK;
    report[1] = status as u8;
    report[4..8].copy_from_slice(&(next as u32).to_le_bytes());
    report
}

/// Decode a device report.
pub fn parse(report: &[u8]) -> Option<Reply> {
    match *report.first()? {
        INFO if *report.get(1)? == VERSION => Some(Reply::Info {
            needed: *report.get(2)? != 0,
            len: word(report, 4)?,
            hash: word(report, 8)?,
        }),
        ACK => {
            let status = match *report.get(1)? {
                0 => Status::Next,
                1 => Status::Done,
                2 => Status::BadOffset,
                3 => Status::WriteFailed,
                4 => Status::HashMismatch,
                _ => return None,
            };
            Some(Reply::Ack {
                status,
                next: word(report, 4)?,
            })
        }
        _ => None,
    }
}

/// Device side: assembles DATA reports into sectors.
pub struct Receiver {
    len: usize,
    hash: u32,
    present: bool,
    queried: bool,
    done: bool,
    at: usize,
    fill: usize,
}

impl Receiver {
    /// `present`: the installed region already has this length and hash.
    pub const fn new(len: usize, hash: u32, present: bool) -> Self {
        Self {
            len,
            hash,
            present,
            queried: false,
            done: false,
            at: 0,
            fill: 0,
        }
    }

    pub const fn present(&self) -> bool {
        self.present
    }

    /// A host has sent a valid QUERY.
    pub const fn queried(&self) -> bool {
        self.queried
    }

    /// The region is installed and the host has been told.
    pub const fn done(&self) -> bool {
        self.done
    }

    /// Handle one host report; `sector` is one flash sector long.
    pub fn handle(
        &mut self,
        report: &[u8],
        sector: &mut [u8],
        mut program: impl FnMut(usize, &[u8]) -> bool,
        verify: impl FnOnce() -> bool,
    ) -> Option<Report> {
        match *report.first()? {
            QUERY if report.get(1..5)? == MAGIC && *report.get(5)? == VERSION => {
                self.queried = true;
                self.done = self.present;
                (self.at, self.fill) = (0, 0);
                Some(info(!self.present, self.len as u32, self.hash))
            }
            DATA => {
                let count = usize::from(*report.get(1)?);
                let offset = word(report, 4)? as usize;
                let bytes = report.get(8..8 + count);
                let end = self.fill + count;
                let (Some(bytes), true) = (
                    bytes,
                    self.queried
                        && !self.done
                        && (1..=DATA_BYTES).contains(&count)
                        && sector.len() == SECTOR
                        && offset == self.at + self.fill
                        && end <= SECTOR
                        && self.at + end <= self.len,
                ) else {
                    self.fill = 0;
                    return Some(ack(Status::BadOffset, self.at));
                };
                sector[self.fill..end].copy_from_slice(bytes);
                self.fill = end;
                if end != SECTOR && self.at + end != self.len {
                    return None;
                }
                self.fill = 0;
                if !program(self.at, &sector[..end]) {
                    return Some(ack(Status::WriteFailed, self.at));
                }
                self.at += end;
                if self.at != self.len {
                    return Some(ack(Status::Next, self.at));
                }
                self.at = 0;
                self.done = verify();
                Some(if self.done {
                    ack(Status::Done, self.len)
                } else {
                    ack(Status::HashMismatch, 0)
                })
            }
            _ => None,
        }
    }
}

/// Marks the upper region appended to a container file.
pub const SECTION_MAGIC: [u8; 8] = *b"X1UPPER\x01";
pub const SECTION_HEADER_BYTES: usize = SECTION_MAGIC.len() + 8;
/// The container's length in a one-step file.
pub const CONTAINER_BYTES: usize = CONTAINER_PREFIX.len() + EXTENDED_PAYLOAD_LEN;

/// The header that precedes `upper` in a one-step file.
pub fn section_header(upper: &[u8]) -> [u8; SECTION_HEADER_BYTES] {
    let mut header = [0; SECTION_HEADER_BYTES];
    header[..8].copy_from_slice(&SECTION_MAGIC);
    header[8..12].copy_from_slice(&(upper.len() as u32).to_le_bytes());
    header[12..].copy_from_slice(&region_hash(upper).to_le_bytes());
    header
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BadSection;

/// Split a file into its container and a one-step file's checked upper region.
pub fn split(file: &[u8]) -> Result<(&[u8], Option<&[u8]>), BadSection> {
    if file.len() <= CONTAINER_BYTES {
        return Ok((file, None));
    }
    let (container, section) = file.split_at(CONTAINER_BYTES);
    let upper = section.get(SECTION_HEADER_BYTES..).ok_or(BadSection)?;
    let valid = section[..8] == SECTION_MAGIC
        && word(section, 8) == Some(upper.len() as u32)
        && word(section, 12) == Some(region_hash(upper))
        && !upper.is_empty()
        && upper.len().is_multiple_of(4)
        && upper.len() <= (UPPER_END - UPPER_BASE) as usize;
    if valid {
        Ok((container, Some(upper)))
    } else {
        Err(BadSection)
    }
}
