//! Host-only policy + native journal/USB replay. No flash hardware access.
#[path = "../src/charging_log.rs"]
mod storage;

use firmware_core::selective_log::{self, Selector};
use serde_json::{Value, json};
use std::{collections::BTreeMap, fs, path::Path};

#[test]
fn changes_use_last_committed_sample_and_heartbeat_resets() {
    let mut s = Selector::new();
    assert!(s.keep(0, 5000, -500));
    s.committed(0, 5000, -500);
    assert!(!s.keep(1000, 5099, -401));
    assert!(!s.keep(59_999, 5000, -500));
    assert!(s.keep(60_000, 5000, -500));
    s.committed(60_000, 5000, -500);
    assert!(!s.keep(60_001, 5000, -500));
    assert!(s.keep(61_000, 5100, -500));
    // A proposed write that fails must not advance the reference.
    assert!(s.keep(62_000, 5100, -500));
    s.committed(62_000, 5100, -500);
    assert!(!s.keep(63_000, 5199, -401));
    assert!(s.keep(63_000, 5100, -400));
    assert!(!s.keep(121_999, 5100, -500));
    assert!(s.keep(122_000, 5100, -500));
    // Signed current and unsigned voltage differences must not overflow.
    s.committed(0, 0, i32::MIN);
    assert!(s.keep(1, u32::MAX, i32::MAX));
    s = Selector::new();
    assert!(s.keep(0, 5100, -500));
}

#[test]
fn only_exact_complete_sop_routine_is_suppressed() {
    for id in 0..8u32 {
        for role in [0, 0x20, 0x100, 0x120] {
            let h = (7 << 16) | (id << 9) | role;
            assert!(selective_log::routine(&[h | 1]));
            for payload in [0x0003_8002, 0x0004_8002] {
                assert!(selective_log::routine(&[h | 0x9010, payload]));
            }
        }
    }
    for sop in 0..7 {
        assert!(!selective_log::routine(&[(sop << 16) | 1]));
        assert!(!selective_log::routine(&[
            (sop << 16) | 0x9010,
            0x0003_8002
        ]));
    }
    for words in [
        vec![],
        vec![0x70001, 0],
        vec![0x71001],
        vec![0x79010],
        vec![0x79010, 0x0003_8003],
        vec![0x79010, 0x0003_8002, 0],
        vec![0x70003],
        vec![0x70006],
        vec![0x72009, 1, 2],
        vec![0x7700f, 1, 2, 3, 4, 5, 6, 7],
    ] {
        assert!(!selective_log::routine(&words), "{words:X?}");
    }
}

fn put(dst: &mut [u8], record: [u32; 4]) {
    for (b, w) in dst.chunks_exact_mut(4).zip(record) {
        b.copy_from_slice(&w.to_le_bytes());
    }
}

/// In-memory replay of native append allocation, not a flash programmer.
struct Replay {
    sectors: [[u8; storage::SECTOR]; 6],
    streams: [Option<usize>; 2],
    counts: [usize; 6],
    used: usize,
    session: u32,
}
impl Replay {
    fn new(session: u32) -> Self {
        Self {
            sectors: [[255; storage::SECTOR]; 6],
            streams: [None; 2],
            counts: [0; 6],
            used: 0,
            session,
        }
    }
    fn append(&mut self, event: bool, record: [u32; 4]) {
        let stream = usize::from(event);
        if self.streams[stream].is_none_or(|i| self.counts[i] == storage::SAMPLES) {
            assert!(self.used < 6, "native storage full");
            let i = self.used;
            put(
                &mut self.sectors[i][..16],
                storage::selective_header(self.session, i as u32, event),
            );
            self.used += 1;
            self.streams[stream] = Some(i);
        }
        let i = self.streams[stream].unwrap();
        let start = storage::HEADER + self.counts[i] * storage::RECORD;
        put(&mut self.sectors[i][start..start + 16], record);
        self.counts[i] += 1;
    }
    fn bytes_used(&self) -> usize {
        self.used * storage::HEADER + self.counts.iter().sum::<usize>() * storage::RECORD
    }
}

fn csv(path: &Path) -> Vec<[i64; 4]> {
    fs::read_to_string(path)
        .unwrap()
        .lines()
        .skip(1)
        .filter(|l| !l.is_empty())
        .map(|line| {
            let fields: Vec<_> = line.split(',').map(|s| s.parse::<i64>().unwrap()).collect();
            fields.try_into().unwrap()
        })
        .collect()
}

#[test]
fn real_captures_match_host_selection_and_native_usb_export() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
    for (name, input, vi, pd, expected_bytes) in [
        (
            "v07",
            "logger-observer-v07-avs-20261113",
            "VI000101.CSV",
            &["PD000101.CSV"][..],
            864,
        ),
        (
            "v04",
            "logger-avs-20261113",
            "LOG00001.CSV",
            &["LOG00002.CSV", "LOG00003.CSV"][..],
            896,
        ),
    ] {
        let fixture = root.join("tests/data").join(input);
        let expected: Vec<Value> = serde_json::from_str(
            &fs::read_to_string(
                root.join("tests/data/logger-selective-study")
                    .join(name)
                    .join("events.json"),
            )
            .unwrap(),
        )
        .unwrap();
        let samples = csv(&fixture.join(vi));
        let session = samples[0][0] as u32;
        let mut packets = BTreeMap::<(u32, u32), [Option<u32>; 8]>::new();
        for file in pd {
            for [sid, at, tag, value] in csv(&fixture.join(file)) {
                assert_eq!(sid as u32, session);
                let part = tag as usize & 255;
                assert!(part < 8, "these fixtures have complete packets only");
                let entry = packets.entry((at as u32, (tag >> 8) as u32)).or_default();
                assert!(entry[part].replace(value as u32).is_none());
            }
        }
        let mut got = Vec::new();
        let mut journal = Replay::new(session);
        let mut selector = Selector::new();
        for [sid, at, mv, ma] in samples {
            assert_eq!(sid as u32, session);
            if selector.keep(at as u32, mv as u32, ma as i32) {
                journal.append(false, storage::record(at as u32, mv as u32, ma as i32));
                selector.committed(at as u32, mv as u32, ma as i32);
                got.push(
                    json!({"kind":"SAMPLE", "elapsed_ms":at, "voltage_mV":mv, "current_mA":ma}),
                );
            }
        }
        for ((at, id), parts) in packets {
            let first = parts[0].unwrap();
            let count = ((first >> 12) & 7) as usize;
            assert!(parts[count + 1..].iter().all(Option::is_none));
            let words: Vec<_> = parts[..=count].iter().map(|p| p.unwrap()).collect();
            if selective_log::routine(&words) {
                continue;
            }
            for (fragment, &word) in words.iter().enumerate() {
                journal.append(
                    true,
                    storage::record(at, (id << 8) | fragment as u32, word as i32),
                );
            }
            let sop = match first >> 16 {
                7 => "SOP",
                6 => "SOP_PRIME",
                5 => "SOP_DOUBLE_PRIME",
                4 => "DEBUG",
                3 => "DEBUG_DOUBLE_PRIME",
                _ => panic!("unexpected fixture SOP"),
            };
            got.push(json!({"kind":"PD_RX", "elapsed_ms":at, "event_id":id, "sop":sop,
                "header_hex":format!("{:04X}", first & 0xffff),
                "data_hex":words[1..].iter().map(|w| format!("{w:08X}")).collect::<Vec<_>>().join(" ")}));
        }
        got.sort_by_key(|v| {
            (
                v["elapsed_ms"].as_u64().unwrap(),
                v["event_id"].as_u64().unwrap_or(0),
            )
        });
        assert_eq!(
            got, expected,
            "{name}: native policy differs from host selection"
        );
        assert_eq!(journal.bytes_used(), expected_bytes);
        assert_eq!(journal.used, 2);
        let mut disk = storage::Disk::new(journal.sectors.each_ref().map(|s| s.as_slice()));
        assert_eq!(disk.file_count(), 2);
        // Verify the actual USB block exporter against expected journal rows,
        // including IDs, all objects, and signed current, not just file counts.
        let mut directory = [0; 512];
        disk.read(3, &mut directory);
        for (entry, event) in [(1, false), (2, true)] {
            let row = &directory[entry * 32..entry * 32 + 32];
            let cluster = u16::from_le_bytes(row[26..28].try_into().unwrap()) as u32;
            let length = u32::from_le_bytes(row[28..32].try_into().unwrap()) as usize;
            let mut exported = Vec::new();
            for block in 0..length.div_ceil(512) {
                let mut buf = [0; 512];
                disk.read(8 + cluster - 2 + block as u32, &mut buf);
                exported.extend_from_slice(&buf);
            }
            exported.truncate(length);
            let text = String::from_utf8(exported).unwrap();
            let rows: Vec<_> = text
                .lines()
                .skip(1)
                .map(|l| {
                    l.split(',')
                        .map(|s| s.parse::<i64>().unwrap())
                        .collect::<Vec<_>>()
                })
                .collect();
            let sector = journal.streams[usize::from(event)].unwrap();
            let expected: Vec<_> = journal.sectors[sector][16..]
                .chunks_exact(16)
                .take(journal.counts[sector])
                .map(|r| {
                    let w = storage::valid_record(r).unwrap();
                    vec![
                        session as i64,
                        w[0] as i64,
                        w[1] as i64,
                        if event {
                            w[2] as i64
                        } else {
                            w[2] as i32 as i64
                        },
                    ]
                })
                .collect();
            assert_eq!(rows, expected, "{name}: USB CSV mismatch");
        }
    }
}

#[test]
fn selected_journals_are_versioned_exportable_and_preserve_old_data() {
    let mut vi = [255; storage::SECTOR];
    put(&mut vi[..16], storage::selective_header(7, 0, false));
    put(&mut vi[16..32], storage::record(1, 5000, -500));
    put(&mut vi[32..48], storage::session_marker(8));
    put(&mut vi[48..64], storage::record(0, 9000, 500));
    let mut pd = [255; storage::SECTOR];
    put(&mut pd[..16], storage::selective_header(8, 1, true));
    // Loss and sniffer-stop records use the same unchanged raw export path.
    put(&mut pd[16..32], storage::record(2, 8, 7));
    put(&mut pd[32..48], storage::record(3, (9 << 8) | 9, 1));
    let mut old = [255; storage::SECTOR];
    put(&mut old[..16], storage::journal_header(6, 0, false));
    put(&mut old[16..32], storage::record(1, 20000, 500));
    let blank = [255; storage::SECTOR];
    let unknown = [0x42; storage::SECTOR];
    let refs = [&vi[..], &pd, &old, &blank, &unknown, &blank];
    assert!(storage::is_selective(&vi));
    assert!(storage::is_event(&pd));
    assert!(!storage::is_selective(&old));
    assert_eq!(storage::latest_session(&vi), 8);
    assert_eq!(storage::reusable_tail(&vi, false), Some(3));
    assert_eq!(storage::reusable_tail(&pd, true), Some(2));
    assert_eq!(
        storage::clear_mask(refs),
        [true, true, true, false, false, false]
    );
    assert_eq!(
        storage::free_mask(refs),
        [false, false, false, true, false, true]
    );
    let mut disk = storage::Disk::new(refs);
    assert_eq!(disk.file_count(), 4);
    let mut directory = [0; 512];
    disk.read(3, &mut directory);
    let status_cluster =
        u16::from_le_bytes(directory[5 * 32 + 26..5 * 32 + 28].try_into().unwrap()) as u32;
    let mut status = [0; 512];
    disk.read(8 + status_cluster - 2, &mut status);
    let status = std::str::from_utf8(&status).unwrap();
    assert!(status.contains("VI SEL1"));
    assert!(status.contains("PD SEL1"));
    assert!(status.contains("100mV/100mA; 60s heartbeat"));
    // CRC-checked prefixes survive a torn native record; never reuse that tail.
    vi[64] = 1;
    assert_eq!(storage::committed_slots(&vi), 3);
    assert_eq!(storage::reusable_tail(&vi, false), None);
}
