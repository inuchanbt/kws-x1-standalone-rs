//! Host-only replay of the existing two-stage install contract on synthetic RAM.
//! Uses the production decoder/fences; does not execute the MCU or use USB.
use anyhow::{Context, Result, ensure};
use clap::Parser;
use firmware_image::{
    APP_BASE, APP_LIMIT, CONTAINER_PREFIX, ERASE_CEILING, UPPER_BASE, UPPER_END, UPPER_ID_MAGIC,
    UPPER_ID_UNSET,
    packed::{
        CHUNK_BYTES, PACKED_MAGIC, Packed, Stream, WINDOW_BYTES, inflate_sectors, main_erase_margin,
    },
    region_hash, verify_container,
};
use serde_json::json;
use std::{fs, io::Write, ops::Range, path::PathBuf};

#[derive(Parser)]
#[command(about = "Replay a matching two-stage logger package offline; no hardware I/O")]
struct Args {
    #[arg(long)]
    upper_image: PathBuf,
    #[arg(long)]
    main_image: PathBuf,
    #[arg(long)]
    expected_upper: PathBuf,
    #[arg(long)]
    expected_main: PathBuf,
    #[arg(long)]
    out: PathBuf,
}

fn word(bytes: &[u8], at: usize) -> u32 {
    u32::from_le_bytes(bytes[at..at + 4].try_into().unwrap())
}
fn record(payload: &[u8]) -> Result<Packed> {
    let found: Vec<_> = payload
        .windows(36)
        .enumerate()
        .filter(|(_, b)| {
            if word(b, 0) != PACKED_MAGIC[0] || word(b, 4) != PACKED_MAGIC[1] {
                return false;
            }
            // The unpacker also contains compared magic constants. Require a
            // bounded two-stage record, rather than treating those as records.
            let data = word(b, 8) as usize;
            let upper_len = word(b, 12) as usize;
            let upper_packed = word(b, 20) as usize;
            let main_len = word(b, 24) as usize;
            let main_packed = word(b, 32) as usize;
            data >= APP_BASE as usize
                && data.is_multiple_of(4)
                && data <= APP_BASE as usize + APP_LIMIT
                && upper_len > 0
                && upper_len <= (UPPER_END - UPPER_BASE) as usize
                && main_len <= APP_LIMIT
                && upper_packed <= APP_LIMIT
                && main_packed <= APP_LIMIT
                && (upper_packed == 0) != (main_packed == 0)
                && data - APP_BASE as usize + upper_packed + main_packed <= APP_LIMIT
        })
        .collect();
    ensure!(found.len() == 1, "expected exactly one packed record");
    let bytes = found[0].1;
    Ok(Packed {
        magic: PACKED_MAGIC,
        data: word(bytes, 8),
        upper: Stream {
            len: word(bytes, 12),
            hash: word(bytes, 16),
            packed: word(bytes, 20),
        },
        main: Stream {
            len: word(bytes, 24),
            hash: word(bytes, 28),
            packed: word(bytes, 32),
        },
    })
}
fn stream(payload: &[u8], address: u32, length: u32) -> Result<Vec<u8>> {
    let start = address
        .checked_sub(APP_BASE)
        .context("stream before application")? as usize;
    payload
        .get(start..start + length as usize)
        .map(<[u8]>::to_vec)
        .context("stream outside container")
}
fn write_sector(flash: &mut [u8], base: usize, bytes: &[u8], fence: Range<u32>) -> bool {
    assert!(base.is_multiple_of(CHUNK_BYTES));
    assert!(base >= fence.start as usize && base + CHUNK_BYTES <= fence.end as usize);
    assert!(bytes.len() <= CHUNK_BYTES && bytes.len().is_multiple_of(4));
    // Exercise the erase-required path with patterned old bytes, every time.
    flash[base..base + CHUNK_BYTES].fill(255);
    flash[base..base + bytes.len()].copy_from_slice(bytes);
    true
}
fn upload(flash: &mut [u8], image: &[u8]) -> Result<Packed> {
    verify_container(image).map_err(|e| anyhow::anyhow!("container: {e:?}"))?;
    let payload = &image[CONTAINER_PREFIX.len()..];
    ensure!(payload.len() <= (ERASE_CEILING - APP_BASE) as usize);
    flash[APP_BASE as usize..ERASE_CEILING as usize].fill(255);
    flash[APP_BASE as usize..APP_BASE as usize + payload.len()].copy_from_slice(payload);
    record(payload)
}
fn main() -> Result<()> {
    let args = Args::parse();
    ensure!(!args.out.exists(), "refusing to overwrite replay report");
    let upper_image = fs::read(args.upper_image)?;
    let main_image = fs::read(args.main_image)?;
    let upper = fs::read(args.expected_upper)?;
    let mut expected = fs::read(args.expected_main)?;
    ensure!(upper.len() <= (UPPER_END - UPPER_BASE) as usize);
    ensure!(expected.len() <= APP_LIMIT);
    let unset: Vec<_> = UPPER_ID_MAGIC
        .into_iter()
        .chain([UPPER_ID_UNSET; 2])
        .flat_map(u32::to_le_bytes)
        .collect();
    let positions: Vec<_> = expected
        .windows(unset.len())
        .enumerate()
        .filter_map(|(i, b)| (b == unset).then_some(i))
        .collect();
    ensure!(positions.len() == 1, "expected one unset upper identity");
    let at = positions[0] + 8;
    expected[at..at + 4].copy_from_slice(&(upper.len() as u32).to_le_bytes());
    expected[at + 4..at + 8].copy_from_slice(&region_hash(&upper).to_le_bytes());

    let original: Vec<_> = (0..0x80000)
        .map(|i| (i as u8).wrapping_mul(37).wrapping_add(11))
        .collect();
    let mut flash = original.clone();
    let mut window = vec![0; WINDOW_BYTES];
    let window: &mut [u8; WINDOW_BYTES] = window.as_mut_slice().try_into()?;
    let first = upload(&mut flash, &upper_image)?;
    ensure!(
        first.main.packed == 0 && first.upper.packed > 0,
        "wrong first stage"
    );
    ensure!(first.upper.len as usize == upper.len() && first.upper.hash == region_hash(&upper));
    let compressed = stream(&upper_image[2..], first.data, first.upper.packed)?;
    let mut decoded = Vec::new();
    inflate_sectors(&compressed, window, |chunk| {
        decoded.extend_from_slice(chunk.bytes());
        write_sector(
            &mut flash,
            UPPER_BASE as usize + chunk.offset,
            chunk.bytes(),
            UPPER_BASE..UPPER_END,
        )
    })
    .map_err(|e| anyhow::anyhow!("upper decode: {e:?}"))?;
    ensure!(decoded == upper, "upper decode mismatch");
    ensure!(flash[UPPER_BASE as usize..UPPER_BASE as usize + upper.len()] == upper);
    ensure!(flash[..APP_BASE as usize] == original[..APP_BASE as usize]);
    ensure!(flash[UPPER_END as usize..] == original[UPPER_END as usize..]);

    let second = upload(&mut flash, &main_image)?;
    ensure!(
        second.upper.packed == 0 && second.main.packed > 0,
        "wrong second stage"
    );
    ensure!(
        second.upper.len == first.upper.len && second.upper.hash == first.upper.hash,
        "stage pair mismatch"
    );
    ensure!(
        region_hash(&flash[UPPER_BASE as usize..UPPER_BASE as usize + upper.len()])
            == second.upper.hash
    );
    ensure!(
        second.main.len as usize == expected.len() && second.main.hash == region_hash(&expected)
    );
    let compressed = stream(&main_image[2..], second.data, second.main.packed)?;
    let mut trace = Vec::new();
    let mut min_unread_margin = usize::MAX;
    decoded.clear();
    inflate_sectors(&compressed, window, |mut chunk| {
        let margin = main_erase_margin(&second, chunk.offset, chunk.consumed).expect("unsafe erase order");
        let remaining = compressed.len() - chunk.consumed;
        let unread = second.data as usize + chunk.consumed;
        assert_eq!(&flash[unread..unread + remaining], &compressed[chunk.consumed..]);
        if remaining > 0 { min_unread_margin = min_unread_margin.min(margin); }
        decoded.extend_from_slice(chunk.bytes());
        let base = APP_BASE as usize + chunk.offset;
        trace.push(json!({"sector_base":base,"decoded_bytes":chunk.bytes().len(),
            "consumed_compressed_bytes":chunk.consumed,"unread_compressed_bytes":remaining,"erase_margin_bytes":margin}));
        let before = flash[base..base + CHUNK_BYTES].to_vec();
        let payload_len = chunk.bytes().len();
        if base + CHUNK_BYTES == ERASE_CEILING as usize {
            assert_eq!(remaining, 0, "final erase must wait for end of stream");
            let bytes = chunk.preserve_tail(|offset| word(&flash, base + offset)).unwrap();
            write_sector(&mut flash, base, bytes, APP_BASE..ERASE_CEILING);
            assert_eq!(&flash[base + payload_len..base + CHUNK_BYTES], &before[payload_len..]);
        } else {
            write_sector(&mut flash, base, chunk.bytes(), APP_BASE..ERASE_CEILING);
        }
        true
    }).map_err(|e| anyhow::anyhow!("main decode: {e:?}"))?;
    ensure!(
        decoded == expected
            && flash[APP_BASE as usize..APP_BASE as usize + expected.len()] == expected
    );
    ensure!(flash[UPPER_BASE as usize..UPPER_BASE as usize + upper.len()] == upper);
    ensure!(flash[..APP_BASE as usize] == original[..APP_BASE as usize]);
    ensure!(flash[UPPER_END as usize..] == original[UPPER_END as usize..]);
    let report = json!({"hardware_access":false,"model":"patterned synthetic 512 KiB flash; decoder/fence replay, not MCU execution",
        "upper_decoded_bytes":upper.len(),"main_decoded_bytes":expected.len(),"matching_stage_identity":true,
        "bootloader_preserved":true,"all_bytes_at_or_above_0x62000_preserved":true,"final_sector_tail_preserved":true,
        "minimum_margin_while_compressed_bytes_remain":min_unread_margin,"main_sector_trace":trace});
    let mut file = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(args.out)?;
    serde_json::to_writer_pretty(&mut file, &report)?;
    writeln!(file)?;
    println!(
        "Replay passed: upper {} B, main {} B, minimum unread-data margin {} B; no hardware access",
        upper.len(),
        expected.len(),
        min_unread_margin
    );
    Ok(())
}
