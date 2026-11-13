//! Backup-matched cleanup in the application's isolated capture boot mode.

use crate::flash::{self, SECTOR};
use hc32f460_pac::{Hash, Pwc};

const BASES: [usize; 5] = [0x62000, 0x64000, 0x66000, 0x68000, 0x6a000];
const _: () =
    assert!(BASES[0] == firmware_image::UPPER_END as usize && BASES[4] + SECTOR == 0x6c000);
type Fingerprints = [[u8; 32]; BASES.len()];

const EXPECTED: Option<Fingerprints> = match option_env!("KWS_X1_RECLAIM_SHA256") {
    Some(hex) => Some(parse_fingerprints(hex.as_bytes())),
    None => None,
};

const fn parse_fingerprints(hex: &[u8]) -> Fingerprints {
    let mut result = [[0; 32]; BASES.len()];
    assert!(hex.len() == BASES.len() * 64);
    let mut i = 0;
    while i < hex.len() / 2 {
        result[i / 32][i % 32] = (nibble(hex[i * 2]) << 4) | nibble(hex[i * 2 + 1]);
        i += 1;
    }
    result
}

const fn nibble(byte: u8) -> u8 {
    match byte {
        b'0'..=b'9' => byte - b'0',
        b'a'..=b'f' => byte - b'a' + 10,
        _ => panic!("expected lowercase SHA-256 hex"),
    }
}

pub const ENABLED: bool = EXPECTED.is_some();
#[cfg_attr(
    not(feature = "charging-logger"),
    unsafe(link_section = ".rodata.capture_reclaim_hash")
)]
static FINGERPRINTS: Fingerprints = match EXPECTED {
    Some(hashes) => hashes,
    None => [[0; 32]; BASES.len()],
};

/// # Safety
/// The capture boot must have exclusive flash access and have confirmed on
/// the LCD and at nominal USB voltage before calling this function.
#[cfg(not(feature = "charging-logger"))]
pub unsafe fn erase_backed_up() -> &'static str {
    if !ENABLED {
        return "RECLAIM DISABLED";
    }
    // SAFETY: only capture boot runs, and no other driver uses HASH.
    let pwc = unsafe { &*Pwc::ptr() };
    let gated = pwc.fcg0().read().hash().bit_is_set();
    set_hash_gate(pwc, false);
    if pwc.fcg0().read().hash().bit_is_set() {
        return "HASH CHECK FAILED";
    }
    let checked = check_all(&FINGERPRINTS);
    set_hash_gate(pwc, gated);
    match checked {
        None => "HASH CHECK FAILED",
        Some(0) => "SPACE ALREADY FREE",
        Some(mask) => {
            for (index, base) in BASES.into_iter().enumerate() {
                if mask & (1 << index) != 0
                    && !unsafe { crate::screenshot::erase_capture_sector(base) }
                {
                    return "RECLAIM FAILED";
                }
            }
            "SPACE RECLAIMED"
        }
    }
}

fn set_hash_gate(pwc: &hc32f460_pac::pwc::RegisterBlock, gated: bool) {
    cortex_m::interrupt::free(|_| {
        // SAFETY: the documented FCG0 write key gates only this RMW.
        pwc.fcg0pc()
            .write(|w| unsafe { w.fcg0pcwe().bits(0xa5a5) }.prt0().set_bit());
        pwc.fcg0().modify(|_, w| w.hash().bit(gated));
        pwc.fcg0pc()
            .write(|w| unsafe { w.fcg0pcwe().bits(0xa5a5) }.prt0().clear_bit());
    });
}

/// In confirmed logger clear boot, authorize only exact backup matches.
/// Known log/capture owners are handled separately; unknown sectors stay intact.
#[cfg(feature = "charging-logger")]
pub fn logger_backup_mask(owned: [bool; 6]) -> Option<u8> {
    if !ENABLED {
        return Some(0);
    }
    let pwc = unsafe { &*Pwc::ptr() };
    let gated = pwc.fcg0().read().hash().bit_is_set();
    set_hash_gate(pwc, false);
    let result = (|| {
        if pwc.fcg0().read().hash().bit_is_set()
            || digest(&[0; 64])?
                != parse_digest(b"f5a5fd42d16a20302798ef6ed309979b43003d2320d9f0e8ea9831a92759fb4b")
        {
            return None;
        }
        let mut mask = 0;
        for (i, base) in BASES.into_iter().enumerate() {
            if owned[i] || flash::blank(base) {
                continue;
            }
            let bytes = unsafe { &*(base as *const [u8; SECTOR]) };
            if digest(bytes)? == FINGERPRINTS[i] {
                mask |= 1 << i;
            }
        }
        Some(mask)
    })();
    set_hash_gate(pwc, gated);
    result
}

// Called only from the verified upper image's maintenance path. Keep the
// hash preflight together so the low flash image can still fit its container.
#[inline(never)]
#[cfg_attr(
    not(feature = "charging-logger"),
    unsafe(link_section = ".text.capture_reclaim_hash")
)]
#[cfg(not(feature = "charging-logger"))]
fn check_all(expected: &Fingerprints) -> Option<u8> {
    const TEST_DIGEST: [u8; 32] =
        parse_digest(b"f5a5fd42d16a20302798ef6ed309979b43003d2320d9f0e8ea9831a92759fb4b");
    // A full zero block also exercises continuation into the padding block.
    if digest(&[0; 64])? != TEST_DIGEST {
        return None;
    }
    erase_mask(expected, |base, expected| {
        if crate::screenshot::capture_owns(base) {
            return None;
        }
        if flash::blank(base) {
            return Some(false);
        }
        // SAFETY: the fixed bases cover only the five reserved sectors, and
        // capture boot has no flash writers while computing the hashes.
        let bytes = unsafe { &*(base as *const [u8; SECTOR]) };
        (digest(bytes)? == *expected).then_some(true)
    })
}

fn erase_mask(
    expected: &Fingerprints,
    mut classify: impl FnMut(usize, &[u8; 32]) -> Option<bool>,
) -> Option<u8> {
    let mut mask = 0;
    for (index, (base, expected)) in BASES.into_iter().zip(expected).enumerate() {
        mask |= u8::from(classify(base, expected)?) << index;
    }
    Some(mask)
}

const fn parse_digest(hex: &[u8; 64]) -> [u8; 32] {
    let mut result = [0; 32];
    let mut i = 0;
    while i < 32 {
        result[i] = (nibble(hex[i * 2]) << 4) | nibble(hex[i * 2 + 1]);
        i += 1;
    }
    result
}

/// SHA-256 of whole blocks. Register order and byte order follow the HC32F460
/// HASH chapter and the vendor DDL. START self-clears after each block.
#[inline(never)]
#[cfg_attr(
    not(feature = "charging-logger"),
    unsafe(link_section = ".text.capture_reclaim_hash")
)]
fn digest(bytes: &[u8]) -> Option<[u8; 32]> {
    const START: u32 = 1;
    const FIRST: u32 = 2;
    let base = Hash::ptr().cast::<u8>();
    let cr = base.cast_mut().cast::<u32>();
    // SAFETY: these are the contiguous 16 input and 8 output HASH registers.
    let data = unsafe { base.add(0x40).cast_mut().cast::<u32>() };
    let result = unsafe { base.add(0x10).cast::<u32>() };
    let (blocks, rest) = bytes.as_chunks::<64>();
    assert!(rest.is_empty());
    let mut pad = [0; 64];
    pad[0] = 0x80;
    pad[56..].copy_from_slice(&((bytes.len() as u64) * 8).to_be_bytes());
    for (index, block) in blocks.iter().chain([&pad]).enumerate() {
        // SAFETY: capture boot owns HASH and writes only its documented fields.
        unsafe {
            cr.write_volatile(0);
            for (word, chunk) in block.as_chunks::<4>().0.iter().enumerate() {
                data.add(word).write_volatile(u32::from_be_bytes(*chunk));
            }
            let mode = if index == 0 { FIRST } else { 0 };
            cr.write_volatile(mode);
            cr.write_volatile(mode | START);
            if !(0..10_000).any(|_| cr.read_volatile() & START == 0) {
                cr.write_volatile(0);
                return None;
            }
        }
    }
    let mut hash = [0; 32];
    for (word, out) in hash.as_chunks_mut::<4>().0.iter_mut().enumerate() {
        *out = unsafe { result.add(word).read_volatile() }.to_be_bytes();
    }
    unsafe { cr.write_volatile(0) };
    Some(hash)
}
