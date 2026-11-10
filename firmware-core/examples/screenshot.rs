//! Exercise the screenshot record format with files on the PC.

use std::fs::{self, File};
use std::io::{BufWriter, Write};
use std::path::PathBuf;

use anyhow::{Context, Result, ensure};
use embedded_graphics::pixelcolor::{Rgb565, Rgb888, raw::RawU16};
use embedded_graphics::prelude::RgbColor;
use firmware_core::panel::{HEIGHT, PIXELS, WIDTH};
use firmware_core::screenshot::{self, Encoder, FRAME_BYTES};
use image_webp::{ColorType, WebPEncoder};

fn main() -> Result<()> {
    let args: Vec<_> = std::env::args_os().skip(1).map(PathBuf::from).collect();
    let [mode, input, output] = args.as_slice() else {
        anyhow::bail!(
            "Usage: screenshot encode <frame.rgb565> <screenshot.x1s>\n       screenshot decode <screenshot.x1s> <screenshot.webp>"
        );
    };
    let input_bytes = fs::read(input).with_context(|| format!("Reading {}", input.display()))?;
    let encoded = match mode.to_str() {
        Some("encode") => {
            let frame: &[u8; FRAME_BYTES] = input_bytes.as_slice().try_into().map_err(|_| {
                anyhow::anyhow!("Expected {FRAME_BYTES} RGB565 bytes for a {WIDTH}x{HEIGHT} frame")
            })?;
            let encoder = Encoder::new(frame).context("Frame exceeds 64 colors")?;
            let (size, bytes) = encoder.record().context("Frame exceeds the flash sector")?;
            let record: Vec<_> = bytes.collect();
            ensure!(record.len() == size, "Encoded size changed");
            record
        }
        Some("decode") => webp(&input_bytes)?,
        _ => anyhow::bail!("Expected encode or decode"),
    };

    let file =
        File::create_new(output).with_context(|| format!("Creating {}", output.display()))?;
    let mut writer = BufWriter::new(file);
    writer.write_all(&encoded)?;
    writer.flush()?;
    Ok(())
}

fn webp(record: &[u8]) -> Result<Vec<u8>> {
    let mut scratch = [0; PIXELS];
    let pixels = screenshot::decode(record, &mut scratch).context("Invalid screenshot record")?;
    let mut rgb = Vec::with_capacity(PIXELS * 3);
    for value in pixels {
        let pixel = Rgb888::from(Rgb565::from(RawU16::new(value)));
        rgb.extend_from_slice(&[pixel.r(), pixel.g(), pixel.b()]);
    }

    let mut encoded = Vec::new();
    WebPEncoder::new(&mut encoded)
        .encode(&rgb, u32::from(WIDTH), u32::from(HEIGHT), ColorType::Rgb8)
        .context("Encoding screenshot")?;
    Ok(encoded)
}
