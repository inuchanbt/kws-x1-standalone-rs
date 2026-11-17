
fn seed_compact(bytes: &[u8], base: usize, seq: u32) {
    for (i, chunk) in bytes.chunks(4).enumerate() {
        let mut word=[0xff;4]; word[..chunk.len()].copy_from_slice(chunk);
        flash::force(base+i*4,u32::from_le_bytes(word));
    }
    for (i,word) in [0x32435358,0xd9a2651e,seq,!seq,0x4d4f4358].into_iter().enumerate() {
        flash::force(base+8192-20+i*4,word);
    }
}
fn groups() -> Vec<u32> {
    let (records,ids)=unsafe {capture::export_records()};
    records.iter().zip(ids).filter_map(|(r,id)|r.map(|_|id)).collect()
}
#[test]
fn export_keeps_wrap_aware_sequence_order_across_physical_slots() {
    flash::reset();capture::simulate_boot();
    let bytes=record();
    for (base,seq,group) in [(0x62000,0u32,2),(0x64000,1,3),(0x66000,u32::MAX-1,1)] {
        let mut wrapped=firmware_core::capture_boot::header(group).unwrap().to_vec();
        wrapped.extend_from_slice(&bytes);seed_compact(&wrapped,base,seq);
    }
    assert_eq!(groups(),[1,2,3]);
    assert!(save(&bytes)==capture::Status::Saved);
    assert_eq!(groups(),[1,2,3,4]);
}
#[test]
fn old_compact_and_x1s1_survive_multiple_boots_without_boot_writes() {
    flash::reset(); capture::simulate_boot();
    let bytes=record();
    let mut legacy=vec![0;firmware_core::screenshot::LEGACY_RECORD_BYTES];
    legacy[..4].copy_from_slice(b"X1S1");
    legacy[4..8].copy_from_slice(&0x26c78384u32.to_le_bytes()); legacy[8]=0xf8;
    for (i,chunk) in legacy.chunks(4).enumerate() {
        let mut word=[0xff;4];word[..chunk.len()].copy_from_slice(chunk);
        flash::force(0x62000+4*i,u32::from_le_bytes(word));
    }
    for (i,word) in [0x31435358,0xd9a2651e,8u32,!8u32,0x4d4f4358].into_iter().enumerate() {
        flash::force(0x62000+3*8192-20+4*i,word);
    }
    seed_compact(&bytes,0x68000,9);
    for at in (0x6a000..0x6c000).chain(0x6e000..0x80000).step_by(4) {flash::force(at,0x81234567);}
    let before=flash::snapshot();
    assert_eq!(groups(),[0,0]);
    for _ in 0..4 {capture::simulate_boot(); assert_eq!(flash::snapshot(),before);}
    assert!(save(&bytes)==capture::Status::Saved);
    assert!(save(&bytes)==capture::Status::Saved);
    capture::simulate_boot();
    assert!(save(&bytes)==capture::Status::Saved);
    assert_eq!(groups(),[0,0,1,1,2]);
    let after=flash::snapshot();
    assert_eq!(&before[0x62000..0x68000],&after[0x62000..0x68000]);
    assert_eq!(&before[0x68000..0x68000+bytes.len()],&after[0x68000..0x68000+bytes.len()]);
    assert_eq!(&before[0x69fec..0x6a000],&after[0x69fec..0x6a000]);
    assert_eq!(&before[0x6a000..0x6c000],&after[0x6a000..0x6c000]);
    assert_eq!(&before[0x6e000..],&after[0x6e000..]);
}
#[test]
fn every_program_word_and_partial_word_failure_preserves_prior_captures() {
    let bytes=record(); let mut cases=0;
    for new_sector in [false,true] {
        flash::reset(); capture::simulate_boot();
        seed_compact(&bytes,0x62000,17);
        for at in (0x64000..0x66000).chain(0x6e000..0x80000).step_by(4) {flash::force(at,0xa5a50123);}
        if new_sector {flash::force(0x62000+bytes.len().next_multiple_of(4),0);}
        let before=flash::snapshot();flash::WRITTEN.store(0,SeqCst);
        assert!(save(&bytes)==capture::Status::Saved);
        let total=flash::WRITTEN.load(SeqCst);
        for cut in 0..total {for mask in [0,0x55555555,0xaaaaaaaa,0xffff] {
            flash::restore(&before);capture::simulate_boot();
            flash::LEFT.store(cut,SeqCst);flash::TORN.store(mask,SeqCst);
            assert!(save(&bytes)==capture::Status::Failed);
            let partial=flash::snapshot();flash::restore(&partial);capture::simulate_boot();
            assert_eq!(groups(),[0],"new={new_sector} cut={cut} mask={mask:x}");
            assert_eq!(&before[0x62000..0x62000+bytes.len()],&partial[0x62000..0x62000+bytes.len()]);
            assert_eq!(&before[0x63fec..0x64000],&partial[0x63fec..0x64000]);
            assert_eq!(&before[0x64000..0x66000],&partial[0x64000..0x66000]);
            assert_eq!(&before[0x6e000..],&partial[0x6e000..]);
            assert!(save(&bytes)==capture::Status::Saved);
            assert_eq!(groups(),[0,1]);cases+=1;
        }}
    }
    println!("PC modeled word/partial-word failures: {cases}");
}
#[test]
fn full_protected_store_and_group_limit_never_erase_or_wrap() {
    flash::reset();capture::simulate_boot();
    let bytes=record();
    for at in (0x62000..0x80000).step_by(4) {flash::force(at,0x81234567);}
    let before=flash::snapshot();assert!(save(&bytes)==capture::Status::Unavailable);
    assert_eq!(before,flash::snapshot());
    flash::reset();capture::simulate_boot();
    let mut wrapped=firmware_core::capture_boot::header(firmware_core::capture_boot::MAX_GROUP).unwrap().to_vec();
    wrapped.extend_from_slice(&bytes);seed_compact(&wrapped,0x62000,0);
    let before=flash::snapshot();assert!(save(&bytes)==capture::Status::Unavailable);
    assert_eq!(before,flash::snapshot());
}
#[test]
fn optional_private_capture_backup_remains_readable_after_model_append() {
    let Some(path)=std::env::var_os("KWS_CAPTURE_BACKUP") else {
        println!("Private backup check omitted: KWS_CAPTURE_BACKUP unset");return;
    };
    let data=std::fs::read(path).unwrap();assert_eq!(data.len(),49152);
    flash::reset();capture::simulate_boot();
    let mut all=flash::snapshot();all[0x62000..0x6e000].copy_from_slice(&data);
    flash::restore(&all);
    let old=count();assert!(old>0);assert_eq!(groups(),vec![0;old]);
    let before=flash::snapshot();assert!(save(&record())==capture::Status::Saved);
    assert_eq!(count(),old+1);let mut expected=vec![0;old];expected.push(1);assert_eq!(groups(),expected);
    let after=flash::snapshot();
    for at in (0x62000..0x6e000).step_by(4) {
        if before[at..at+4]!=[0xff;4] {assert_eq!(&before[at..at+4],&after[at..at+4]);}
    }
    assert_eq!(&before[..0x62000],&after[..0x62000]);
    assert_eq!(&before[0x6e000..],&after[0x6e000..]);
    println!("Private backup PC check: {old} old captures valid; all original nonblank words unchanged; new group 1 appended only in RAM model");
}
