//! Boot identifiers around existing capture records, without rewriting old data.
//!
//! X1B1 adds a bounded identifier and its complement before an unchanged image.
//! The flash journal still supplies the final commit; inner image CRCs remain.
//! Zero identifies an older untagged capture, whose boot cannot be recovered.
use crate::screenshot::{COMPACT_HEADER_BYTES, Capture, MAX_RECORD_BYTES, compact_record_len};
pub const HEADER_BYTES: usize = 12;
pub const PREFIX_BYTES: usize = HEADER_BYTES + COMPACT_HEADER_BYTES;
pub const MAX_GROUP: u32 = 99_999_999;
const MAGIC: &[u8; 4] = b"X1B1";
pub fn header(group: u32) -> Option<[u8; HEADER_BYTES]> {
    if !(1..=MAX_GROUP).contains(&group) {
        return None;
    }
    let mut out = [0; HEADER_BYTES];
    out[..4].copy_from_slice(MAGIC);
    out[4..8].copy_from_slice(&group.to_le_bytes());
    out[8..12].copy_from_slice(&(!group).to_le_bytes());
    Some(out)
}
fn split(bytes: &[u8]) -> Option<(u32, &[u8])> {
    if bytes.get(..4)? != MAGIC {
        return Some((0, bytes));
    }
    let group = u32::from_le_bytes(bytes.get(4..8)?.try_into().ok()?);
    let check = u32::from_le_bytes(bytes.get(8..12)?.try_into().ok()?);
    if !(1..=MAX_GROUP).contains(&group) || check != !group {
        return None;
    }
    Some((group, bytes.get(HEADER_BYTES..)?))
}
pub fn record_len(prefix: &[u8]) -> Option<usize> {
    let (group, inner) = split(prefix)?;
    let size = compact_record_len(inner)? + if group == 0 { 0 } else { HEADER_BYTES };
    (size <= MAX_RECORD_BYTES).then_some(size)
}
pub fn open(bytes: &[u8]) -> Option<(u32, Capture<'_>)> {
    let (group, inner) = split(bytes)?;
    Some((group, Capture::open(inner)?))
}
