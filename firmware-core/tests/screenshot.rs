use firmware_core::buttons::{Detail, Page, SettingsFocus, Tab};
use firmware_core::panel::{PIXELS, WIDTH};
use firmware_core::screenshot::{
    CAPTURE_TABLE_LEN, COLORS, Capture, CapturePage, Encoder, FRAME_BYTES, LEGACY_RECORD_BYTES,
    MAX_RECORD_BYTES, Record, decode,
};
use firmware_core::screenshot_disk::{BLOCK_BYTES, BLOCKS, Disk};
use firmware_core::screenshot_report::{BAND_BYTES, HEADER_BYTES, HEADER_RGB_BYTES, encode_report};

fn test_cable_band(heading: bool, line: &str, out: &mut [u8; BAND_BYTES]) {
    out[0] = if heading {
        0x20
    } else if line.ends_with('7') {
        0x10
    } else {
        0
    };
}

fn encode(frame: &[u8; FRAME_BYTES]) -> Vec<u8> {
    encode_for(frame, CapturePage::Unnamed)
}

fn encode_for(frame: &[u8; FRAME_BYTES], page: CapturePage) -> Vec<u8> {
    let encoder = Encoder::new(frame).unwrap();
    let (size, bytes) = encoder.record_for(page).unwrap();
    let record: Vec<_> = bytes.collect();
    assert_eq!(record.len(), size);
    record
}

#[test]
fn round_trips_all_palette_indices_in_wire_order() {
    let colors = [0xf800, 0x07e0, 0x001f, 0xffff]
        .into_iter()
        .chain((0..60).map(|n| 0x4010 + n))
        .cycle();
    let mut frame = [0; FRAME_BYTES];
    for row in frame.chunks_exact_mut(usize::from(WIDTH) * 2) {
        for (pixel, color) in row.as_chunks_mut::<2>().0.iter_mut().zip(colors.clone()) {
            *pixel = u16::to_be_bytes(color);
        }
    }

    let record = encode(&frame);
    assert!(record.len() < 1024);
    let expected = frame
        .as_chunks::<2>()
        .0
        .iter()
        .copied()
        .map(u16::from_be_bytes);
    assert!(decode(&record, &mut [0; PIXELS]).unwrap().eq(expected));
}

#[test]
fn rejects_damaged_and_incomplete_records() {
    let mut record = encode(&[0; FRAME_BYTES]);
    assert!(Record::open(&record[..record.len() - 1]).is_none());
    *record.last_mut().unwrap() ^= 1;
    assert!(Record::open(&record).is_none());
}

#[test]
fn rejects_a_65th_color() {
    let mut frame = [0; FRAME_BYTES];
    for (pixel, color) in frame.as_chunks_mut::<2>().0.iter_mut().zip(0..65) {
        *pixel = u16::to_be_bytes(color);
    }
    assert!(Encoder::new(&frame).is_none());
}

#[test]
fn declines_an_incompressible_frame_before_writing_a_sector() {
    let mut frame = [0; FRAME_BYTES];
    let mut state = 1u32;
    for pixel in frame.as_chunks_mut::<2>().0 {
        state = state.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        *pixel = [0, (state >> 26) as u8];
    }
    assert!(Encoder::new(&frame).unwrap().record().is_none());
}

#[test]
fn backreferences_round_trip_overlapping_patterns_across_rows() {
    let mut frame = [0; FRAME_BYTES];
    for (at, pixel) in frame.as_chunks_mut::<2>().0.iter_mut().enumerate() {
        *pixel = ((at % 13) as u16).to_be_bytes();
    }
    let record = encode(&frame);
    assert_eq!(&record[..4], b"X1S3");
    assert!(
        decode(&record, &mut [0; PIXELS])
            .unwrap()
            .eq((0..PIXELS).map(|at| (at % 13) as u16))
    );
}

#[test]
fn dense_two_color_frames_still_use_the_smaller_previous_encoding() {
    let mut frame = [0; FRAME_BYTES];
    let mut state = 1u32;
    for pixel in frame.as_chunks_mut::<2>().0 {
        state = state.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        *pixel = [0, (state >> 31) as u8];
    }
    let record = encode(&frame);
    assert_eq!(&record[..4], b"X1S2");
    assert!(
        decode(&record, &mut [0; PIXELS]).unwrap().eq(frame
            .as_chunks::<2>()
            .0
            .iter()
            .copied()
            .map(u16::from_be_bytes))
    );
}

#[test]
fn exports_the_first_firmware_versions_record() {
    // X1S1 with only the first palette entry used, RGB565 red.
    let mut record = [0; LEGACY_RECORD_BYTES];
    record[..4].copy_from_slice(b"X1S1");
    record[4..8].copy_from_slice(&0x26c7_8384u32.to_le_bytes());
    record[8] = 0xf8;
    let mut scratch = [0; PIXELS];
    assert!(
        decode(&record, &mut scratch)
            .unwrap()
            .all(|color| color == 0xf800)
    );
}

#[test]
fn fat_chains_yield_every_saved_bitmap() {
    let encoded: [Vec<u8>; 13] = core::array::from_fn(|file| {
        let mut frame = [0; FRAME_BYTES];
        for (at, pixel) in frame.as_chunks_mut::<2>().0.iter_mut().enumerate() {
            *pixel = if (at + file).is_multiple_of(13) {
                [0xf8, 0]
            } else {
                [0, 0x1f]
            };
        }
        encode(&frame)
    });
    let records = core::array::from_fn(|file| Capture::open(&encoded[file % encoded.len()]));
    let mut scratch = [0; PIXELS];
    let mut disk = Disk::new(&records, &mut scratch, test_cable_band);
    let mut boot = [0; BLOCK_BYTES];
    disk.read(0, &mut boot);
    assert_eq!(&boot[510..], &[0x55, 0xaa]);
    assert_eq!(&boot[19..21], &[0, 0]);
    assert_eq!(u32::from_le_bytes(boot[32..36].try_into().unwrap()), BLOCKS);
    let cluster_blocks = u32::from(boot[13]);
    let (first_root, first_data) = volume_layout(&boot);
    let fat = read_blocks(&mut disk, 1..first_root);
    let root = read_blocks(&mut disk, first_root..first_data);
    let mut block = [0; BLOCK_BYTES];
    let mut used = vec![false; BLOCKS as usize];
    let mut first_pixel_block = block;
    let files = directory_files(&root);
    assert_eq!(files.len(), CAPTURE_TABLE_LEN);
    for (file, (name, entry)) in files.into_iter().enumerate() {
        let number = file + 1;
        assert_eq!(name, format!("SCREEN{number:02}.BMP"));
        assert_eq!(entry[11] & 1, 1);
        let file_size = u32::from_le_bytes(entry[28..32].try_into().unwrap()) as usize;
        let mut cluster = u16::from_le_bytes(entry[26..28].try_into().unwrap());
        let mut bmp = Vec::new();
        for _ in 0..file_size.div_ceil(BLOCK_BYTES * cluster_blocks as usize) {
            assert!(cluster >= 2);
            let first_lba = first_data + u32::from(cluster - 2) * cluster_blocks;
            assert!(first_lba + cluster_blocks <= BLOCKS);
            for lba in first_lba..first_lba + cluster_blocks {
                assert!(!std::mem::replace(&mut used[lba as usize], true));
                disk.read(lba, &mut block);
                bmp.extend_from_slice(&block);
            }
            cluster = fat_next(&fat, cluster);
        }
        assert_eq!(cluster, 0xfff);
        bmp.truncate(file_size);
        assert_eq!(&bmp[..2], b"BM");
        assert_eq!(i32::from_le_bytes(bmp[22..26].try_into().unwrap()), -135);
        let pixels = u32::from_le_bytes(bmp[10..14].try_into().unwrap()) as usize;
        assert_eq!(pixels, 54 + COLORS * 4);
        assert_eq!(bmp.len() - pixels, PIXELS);
        for (at, &index) in bmp[pixels..].iter().enumerate() {
            let palette_entry = 54 + 4 * usize::from(index);
            let color = if (at + file % encoded.len()).is_multiple_of(13) {
                [0, 0, 255]
            } else {
                [255, 0, 0]
            };
            assert_eq!(&bmp[palette_entry..palette_entry + 3], &color);
        }
        if file == 0 {
            first_pixel_block.copy_from_slice(&bmp[BLOCK_BYTES..2 * BLOCK_BYTES]);
        }
    }
    // Switching back to the first image must not return the last file's pixels.
    disk.read(first_data + 1, &mut block);
    assert_eq!(block, first_pixel_block);
}

fn directory_files(root: &[u8]) -> Vec<(String, &[u8; 32])> {
    let mut files = Vec::new();
    let mut long = Vec::new();
    for entry in root.as_chunks::<32>().0.iter().skip(1) {
        if entry[0] == 0 {
            break;
        }
        if entry[11] == 0x0f {
            long.push(entry);
            continue;
        }
        let name = if long.is_empty() {
            let stem = std::str::from_utf8(&entry[..8]).unwrap().trim_end();
            let ext = std::str::from_utf8(&entry[8..11]).unwrap();
            format!("{stem}.{ext}")
        } else {
            assert_eq!(long[0][0], 0x40 | long.len() as u8);
            let checksum = entry[..11]
                .iter()
                .fold(0u8, |sum, byte| sum.rotate_right(1).wrapping_add(*byte));
            let mut codes = Vec::new();
            for (index, part) in long.iter().rev().enumerate() {
                assert_eq!(usize::from(part[0] & 0x1f), index + 1);
                assert_eq!(part[13], checksum);
                for range in [1..11, 14..26, 28..32] {
                    codes.extend(
                        part[range]
                            .as_chunks::<2>()
                            .0
                            .iter()
                            .map(|&bytes| u16::from_le_bytes(bytes)),
                    );
                }
            }
            let len = codes
                .iter()
                .position(|&code| code == 0)
                .unwrap_or(codes.len());
            if len < codes.len() {
                assert!(codes[len + 1..].iter().all(|&code| code == 0xffff));
            }
            long.clear();
            String::from_utf16(&codes[..len]).unwrap()
        };
        files.push((name, entry));
    }
    assert!(long.is_empty());
    files
}

#[test]
fn named_screen_keeps_pixels_and_exports_its_menu_across_directory_blocks() {
    let frame = [0; FRAME_BYTES];
    let old = encode(&frame);
    let charger = encode_for(
        &frame,
        CapturePage::from_page(Page::Tabs(Tab::detail(Detail::Charger))),
    );
    let settings = encode_for(
        &frame,
        CapturePage::from_page(Page::Settings(SettingsFocus::Brightness)),
    );
    let overview = encode_for(&frame, CapturePage::from_page(Page::default()));
    assert_eq!(charger.len(), old.len());
    assert_eq!(&charger[12..], &old[12..]);
    let mut damaged = charger.clone();
    damaged[11] = 0;
    assert!(Capture::open(&damaged).is_none());

    let mut records = [None; CAPTURE_TABLE_LEN];
    records[..13].fill(Capture::open(&old));
    records[13] = Capture::open(&settings); // Its name crosses a root block.
    records[14] = Capture::open(&charger); // Exactly 13 UTF-16 characters.
    records[15] = Capture::open(&overview);
    let mut scratch = [0; PIXELS];
    let mut disk = Disk::new(&records, &mut scratch, test_cable_band);
    let mut boot = [0; BLOCK_BYTES];
    disk.read(0, &mut boot);
    let (first_root, first_data) = volume_layout(&boot);
    let root = read_blocks(&mut disk, first_root..first_data);
    let files = directory_files(&root);
    assert_eq!(files.len(), 16);
    assert_eq!(files[12].0, "SCREEN13.BMP");
    assert_eq!(files[13].0, "SETTINGS14.BMP");
    assert_eq!(files[14].0, "CHARGER15.BMP");
    assert_eq!(files[15].0, "OVERVIEW16.BMP");
}

fn volume_layout(boot: &[u8; BLOCK_BYTES]) -> (u32, u32) {
    let first_root = 1 + u32::from(u16::from_le_bytes(boot[22..24].try_into().unwrap()));
    let root_entries = u32::from(u16::from_le_bytes(boot[17..19].try_into().unwrap()));
    let first_data = first_root + (root_entries * 32).div_ceil(BLOCK_BYTES as u32);
    (first_root, first_data)
}

fn read_blocks(disk: &mut Disk<'_>, blocks: std::ops::Range<u32>) -> Vec<u8> {
    let mut block = [0; BLOCK_BYTES];
    let mut bytes = Vec::new();
    for lba in blocks {
        disk.read(lba, &mut block);
        bytes.extend_from_slice(&block);
    }
    bytes
}

fn fat_next(fat: &[u8], cluster: u16) -> u16 {
    let at = usize::from(cluster + cluster / 2);
    let entry = u16::from_le_bytes([fat[at], fat[at + 1]]);
    (entry >> (4 * (cluster % 2))) & 0xfff
}

#[test]
fn empty_disk_has_no_file() {
    let mut scratch = [0; PIXELS];
    let mut disk = Disk::new(&[None; CAPTURE_TABLE_LEN], &mut scratch, test_cable_band);
    let mut boot = [0; BLOCK_BYTES];
    disk.read(0, &mut boot);
    let (first_root, first_data) = volume_layout(&boot);
    let root = read_blocks(&mut disk, first_root..first_data);
    assert!(root[32..].iter().all(|&byte| byte == 0));
}

#[cfg(feature = "capture-backup")]
#[test]
fn flash_backup_is_read_only_and_does_not_renumber_captures() {
    use firmware_core::screenshot_disk::FLASH_SNAPSHOT_BYTES;

    let mut snapshot = [0; FLASH_SNAPSHOT_BYTES];
    for (index, block) in snapshot.chunks_exact_mut(BLOCK_BYTES).enumerate() {
        block.fill(index as u8);
    }
    let image = encode(&[0; FRAME_BYTES]);
    let named = encode_for(&[0; FRAME_BYTES], CapturePage::Settings);
    let mut records = [None; CAPTURE_TABLE_LEN];
    records[..13].fill(Capture::open(&image));
    records[13..15].fill(Capture::open(&named));
    let mut scratch = [0; PIXELS];
    let mut disk =
        Disk::new(&records, &mut scratch, test_cable_band).with_flash_snapshot(&snapshot);
    let mut boot = [0; BLOCK_BYTES];
    disk.read(0, &mut boot);
    let (first_root, first_data) = volume_layout(&boot);
    let cluster_blocks = u32::from(boot[13]);
    let fat = read_blocks(&mut disk, 1..first_root);
    let root = read_blocks(&mut disk, first_root..first_data);
    let files = directory_files(&root);
    assert_eq!(files.len(), 16);
    assert_eq!(files[12].0, "SCREEN13.BMP");
    assert_eq!(files[13].0, "SETTINGS14.BMP");
    assert_eq!(files[14].0, "SETTINGS15.BMP");
    let (name, entry) = &files[15];
    assert_eq!(name, "CAPFLASH.BIN");
    assert_eq!(entry[11] & 1, 1);
    let size = u32::from_le_bytes(entry[28..32].try_into().unwrap()) as usize;
    assert_eq!(size, snapshot.len());
    let mut cluster = u16::from_le_bytes(entry[26..28].try_into().unwrap());
    let mut bytes = Vec::new();
    for _ in 0..size.div_ceil(BLOCK_BYTES * cluster_blocks as usize) {
        let first = first_data + u32::from(cluster - 2) * cluster_blocks;
        bytes.extend(read_blocks(&mut disk, first..first + cluster_blocks));
        cluster = fat_next(&fat, cluster);
    }
    assert_eq!(cluster, 0xfff);
    assert_eq!(&bytes[..size], &snapshot);
    assert!(bytes[size..].iter().all(|&byte| byte == 0));
}

#[test]
fn exports_a_cable_report_from_the_previous_format() {
    // X1SC contained the palette, runs and rows without a page identifier.
    let mut record = [0; 412];
    record[..4].copy_from_slice(b"X1SC");
    record[4..8].copy_from_slice(&0x0ab1_169cu32.to_le_bytes());
    record[8..12].copy_from_slice(&412u32.to_le_bytes());
    record[14..18].copy_from_slice(&[0xff, 0xff, 0xff, 0x07]);
    record[44..46].copy_from_slice(&360u16.to_le_bytes());
    record[46..406].fill(0xf0);
    record[406..].copy_from_slice(b"\x85CABLE");
    let Some(Capture::Report(report)) = Capture::open(&record) else {
        panic!("legacy Cable report did not open");
    };
    assert_eq!(report.page(), Detail::Cable);
    assert_eq!(report.row(0), Some((true, "CABLE")));
}

#[test]
fn detail_reports_export_all_rows_under_page_names_alongside_a_bmp() {
    let sections = (0..50).map(|section| {
        let mut lines = [[b' '; 36]; 8];
        lines[0][..5].copy_from_slice(b"A<&>B");
        for (index, line) in lines.iter_mut().enumerate().skip(1) {
            let text = format!("Section {section} field {index}");
            line[..text.len()].copy_from_slice(text.as_bytes());
        }
        lines
    });
    let mut header = [0; HEADER_RGB_BYTES];
    header[0] = 0xf8;
    let pages = [Detail::Pd, Detail::Charger, Detail::Cable, Detail::System];
    let headings = [0x07e0, 0xfd04, 0x07ff, 0xff80];
    let mut encoded = [[0; MAX_RECORD_BYTES]; 4];
    let mut sizes = [0; 4];
    for (index, page) in pages.into_iter().enumerate() {
        sizes[index] = encode_report(
            page,
            &header,
            [0, 0xffff, headings[index]],
            sections.clone(),
            &mut encoded[index],
        )
        .unwrap();
    }
    let bitmap = encode(&[0; FRAME_BYTES]);
    let mut records = [None; CAPTURE_TABLE_LEN];
    // PD15's FAT chain crosses a sector boundary. The next file starts in
    // the second directory sector because the first holds the volume label.
    records[..14].fill(Capture::open(&bitmap));
    for index in 0..4 {
        let capture = Capture::open(&encoded[index][..sizes[index]]);
        assert!(matches!(capture, Some(Capture::Report(report)) if report.page() == pages[index]));
        records[index + 14] = capture;
    }
    let mut scratch = [0; PIXELS];
    let mut disk = Disk::new(&records, &mut scratch, test_cable_band);
    let mut block = [0; BLOCK_BYTES];
    disk.read(0, &mut block);
    let cluster_blocks = u32::from(block[13]);
    let (first_root, first_data) = volume_layout(&block);
    let fat = read_blocks(&mut disk, 1..first_root);
    let root = read_blocks(&mut disk, first_root..first_data);
    let files = directory_files(&root);
    assert_eq!(files[0].0, "SCREEN01.BMP");
    let names = ["PD15.BMP", "CHARGER16.BMP", "CABLE17.BMP", "SYSTEM18.BMP"];
    for (index, ((name, entry), expected)) in files[14..].iter().zip(names).enumerate() {
        assert_eq!(name, expected);
        let file_len = u32::from_le_bytes(entry[28..32].try_into().unwrap()) as usize;
        let mut cluster = u16::from_le_bytes(entry[26..28].try_into().unwrap());
        let mut bmp = Vec::new();
        for _ in 0..file_len.div_ceil(cluster_blocks as usize * BLOCK_BYTES) {
            let first = first_data + (u32::from(cluster) - 2) * cluster_blocks;
            bmp.extend(read_blocks(&mut disk, first..first + cluster_blocks));
            cluster = fat_next(&fat, cluster);
        }
        assert_eq!(cluster, 0xfff);
        bmp.truncate(file_len);
        assert_eq!(&bmp[..2], b"BM");
        assert_eq!(i32::from_le_bytes(bmp[18..22].try_into().unwrap()), 240);
        assert_eq!(
            i32::from_le_bytes(bmp[22..26].try_into().unwrap()),
            -(24 + 400 * 12)
        );
        assert_eq!(u16::from_le_bytes(bmp[28..30].try_into().unwrap()), 4);
        assert_eq!(&bmp[66..70], &[0, 0, 255, 0]);
        let heading = (u16::from(bmp[64] >> 3) << 11)
            | (u16::from(bmp[63] >> 2) << 5)
            | u16::from(bmp[62] >> 3);
        assert_eq!(heading, headings[index]);
        let pixels = u32::from_le_bytes(bmp[10..14].try_into().unwrap()) as usize;
        assert_eq!(bmp[pixels], 0x30);
        assert_eq!(bmp[pixels + HEADER_BYTES], 0x20);
        assert_eq!(bmp[pixels + HEADER_BYTES + 399 * BAND_BYTES], 0x10);
        assert_eq!(bmp.len(), pixels + HEADER_BYTES + 400 * BAND_BYTES);
    }
    disk.read(first_data, &mut block);
    assert_eq!(&block[..2], b"BM");
}
