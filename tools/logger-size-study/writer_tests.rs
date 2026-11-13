fn bytes(base: usize) -> Vec<u8> {
    crate::flash::DATA.lock().unwrap()[base - START..base - START + flash::SECTOR].to_vec()
}
#[cfg(test)]
mod tests {
    use super::*;
    fn setup() {
        crate::flash::DATA.lock().unwrap().fill(255);
        while EVENTS.try_receive().is_ok() {}
        TOGGLE.store(false, Ordering::Relaxed);
        RECORDING.store(false, Ordering::Relaxed);
    }
    fn free() -> usize {
        (START..END)
            .step_by(flash::SECTOR)
            .filter(|&b| available(b))
            .count()
    }
    fn toggle(w: &mut Writer, t: u64) {
        TOGGLE.store(true, Ordering::Relaxed);
        w.handle_request(t);
    }
    fn record(w: &mut Writer, t: u64) {
        crate::block_on(w.sample(t, Some((20000, 500))));
        enqueue(Event {
            at: t + 1,
            words: [0x71810; 8],
            count: 2,
            kind: 0,
        });
        crate::block_on(w.sample(t + 2, None));
    }
    fn stop(w: &mut Writer, t: u64) {
        toggle(w, t);
        crate::block_on(w.sample(t, None));
        assert!(!w.active);
        assert!(w.message.contains("STOPPED"));
    }
    #[test]
    fn thirty_two_short_sessions_share_two_sectors_across_reboots() {
        setup();
        for session in 1..=32 {
            let mut w = Writer::new();
            let t = session as u64 * 1000;
            toggle(&mut w, t);
            assert!(w.active);
            record(&mut w, t);
            stop(&mut w, t + 10);
            assert_eq!(w.session, session);
            assert_eq!(free(), 4);
        }
        let before = crate::flash::DATA.lock().unwrap().to_vec();
        let mut w = Writer::new();
        toggle(&mut w, 40000);
        assert!(w.message.contains("SESSION LIMIT"));
        assert_eq!(before, crate::flash::DATA.lock().unwrap().to_vec());
        let d = crate::flash::DATA.lock().unwrap();
        let disk = format::Disk::new(core::array::from_fn(|i| &d[i * 8192..(i + 1) * 8192]));
        assert_eq!(disk.file_count(), 64);
        let mut disk = disk;
        let mut image = Vec::new();
        for lba in 0..format::BLOCKS {
            let mut block = [0; 512];
            disk.read(lba, &mut block);
            image.extend_from_slice(&block);
        }
        std::fs::write("target/logger-v07-host-disk.img", image).unwrap();
    }
    #[test]
    fn reboot_preserves_committed_bytes_and_appends_a_session_marker() {
        setup();
        let mut a = Writer::new();
        toggle(&mut a, 0);
        record(&mut a, 0);
        stop(&mut a, 50);
        let before = bytes(START);
        let mut b = Writer::new();
        toggle(&mut b, 100);
        record(&mut b, 100);
        stop(&mut b, 150);
        assert_eq!(&before[..32], &bytes(START)[..32]);
        assert_eq!(format::latest_session(&bytes(START)), 2);
        assert_eq!(b.session, 2);
        assert_eq!(free(), 4);
    }
    #[test]
    fn unknown_sectors_remain_untouched_while_zero_blank_sectors_can_restart() {
        setup();
        for i in 0..4 {
            crate::flash::DATA.lock().unwrap()[i * flash::SECTOR] = 0;
        }
        let unknown = crate::flash::DATA.lock().unwrap()[..4 * 8192].to_vec();
        for n in 0..6 {
            let mut w = Writer::new();
            toggle(&mut w, n * 1000);
            record(&mut w, n * 1000);
            stop(&mut w, n * 1000 + 10);
            assert_eq!(free(), 0);
        }
        assert_eq!(
            &unknown[..],
            &crate::flash::DATA.lock().unwrap()[..4 * 8192]
        );
    }
    #[test]
    fn failed_start_with_one_blank_and_legacy_tails_writes_nothing() {
        setup();
        for i in 0..5 {
            crate::flash::DATA.lock().unwrap()[i * flash::SECTOR] = 0;
        }
        let h = format::header(1, 0);
        {
            let mut d = crate::flash::DATA.lock().unwrap();
            for (i, v) in h.into_iter().enumerate() {
                d[i * 4..i * 4 + 4].copy_from_slice(&v.to_le_bytes());
            }
        }
        let mut w = Writer::new();
        let before = crate::flash::DATA.lock().unwrap().to_vec();
        toggle(&mut w, 20);
        assert!(w.message.contains("FULL"));
        assert_eq!(before, crate::flash::DATA.lock().unwrap().to_vec());
    }
    #[test]
    fn vi_only_restarts_reuse_the_tail_without_reserving_unused_pd_sectors() {
        setup();
        for n in 1..=6 {
            let mut w = Writer::new();
            toggle(&mut w, n * 1000);
            crate::block_on(w.sample(n * 1000, Some((5000, 0))));
            stop(&mut w, n * 1000 + 10);
            assert_eq!(free(), 5);
            assert_eq!(w.session, n as u32);
        }
    }
    #[test]
    fn packet_rollover_and_full_stop_never_commit_partial_packets() {
        setup();
        let mut w = Writer::new();
        toggle(&mut w, 0);
        record(&mut w, 0);
        for i in 0..4 {
            let mut d = crate::flash::DATA.lock().unwrap();
            d[(i + 2) * 8192] = 0;
        }
        let remaining = format::SAMPLES - w.pd_index;
        for _ in 0..remaining - 1 {
            assert!(w.append(true, format::record(1, 8, 0)));
        }
        let before = bytes(w.pd_base);
        enqueue(Event {
            at: 20,
            words: [0x1234; 8],
            count: 8,
            kind: 0,
        });
        crate::block_on(w.sample(20, None));
        assert!(!w.active);
        assert_eq!(before, bytes(w.pd_base));
    }
    #[test]
    fn torn_marker_tail_is_sealed_and_old_session_stays_readable() {
        setup();
        let mut w = Writer::new();
        toggle(&mut w, 0);
        record(&mut w, 0);
        stop(&mut w, 10);
        let at = w.base + 16 + w.index * 16;
        assert!(flash::program_blank(w.base, at, &[u32::MAX, 2]));
        let before = bytes(w.base);
        let mut next = Writer::new();
        toggle(&mut next, 100);
        record(&mut next, 100);
        stop(&mut next, 110);
        assert_eq!(bytes(START), before);
        assert_ne!(next.base, START);
        assert_eq!(next.session, 2);
        assert_eq!(free(), 3);
    }
    #[test]
    fn rollover_after_reused_tail_keeps_packet_fragments_and_old_bytes() {
        setup();
        let mut w = Writer::new();
        toggle(&mut w, 0);
        record(&mut w, 0);
        for _ in 0..format::SAMPLES - w.pd_index - 3 {
            assert!(w.append(true, format::record(0, 8, 0)));
        }
        stop(&mut w, 10);
        let before = bytes(w.pd_base);
        let mut next = Writer::new();
        toggle(&mut next, 100);
        enqueue(Event {
            at: 101,
            words: [0x2222; 8],
            count: 8,
            kind: 0,
        });
        crate::block_on(next.sample(102, Some((5000, 0))));
        stop(&mut next, 110);
        assert_eq!(
            &bytes(START + 8192)[..SECTOR_PREFIX],
            &before[..SECTOR_PREFIX]
        );
        assert_eq!(free(), 3);
        let mut third = Writer::new();
        toggle(&mut third, 200);
        assert_eq!(third.pd_base, next.pd_base);
        record(&mut third, 200);
        stop(&mut third, 210);
        assert_eq!(free(), 3);
        let data = crate::flash::DATA.lock().unwrap();
        let mut disk = format::Disk::new(core::array::from_fn(|i| &data[i * 8192..(i + 1) * 8192]));
        assert_eq!(disk.file_count(), 6);
        let mut root = [0; 512];
        disk.read(3, &mut root);
        assert_eq!(&root[128..139], b"PD000201CSV");
        assert_eq!(
            u32::from_le_bytes(root[156..160].try_into().unwrap()),
            b"session,elapsed_ms,event_tag,raw_u32\r\n".len() as u32 + 8 * 46
        );
    }
    const SECTOR_PREFIX: usize = 8192 - 3 * 16;
}
