"""Host adapter for the production logger Writer; flash/time/channel I/O only is replaced."""
from pathlib import Path
import subprocess

root = Path(__file__).resolve().parents[2]
source = (root / 'firmware/src/logger.rs').read_text(encoding='utf-8')
source = source[:source.index("fn bytes(base: usize)")]
source = source.replace('//! Exclusive logger variant. Only the meter task owns a Writer.', '')
source = source.replace('use crate::{I2C3, flash, render::Frame, ui};', 'use crate::{I2C3, flash, embassy_time};')
source = source.replace('use firmware_core::{buttons::Detail, charging_log as format, screenshot::CapturePage};', 'use firmware_core::charging_log as format;')
source = source.replace('use embassy_sync::{blocking_mutex::raw::CriticalSectionRawMutex, channel::Channel};', '')
source = source.replace('bytes(b)', '&bytes(b)').replace('bytes(base)', '&bytes(base)')
source = source.replace('firmware_image::UPPER_END', '0x62000u32')
adapter = r'''
#![allow(dead_code)]
use std::sync::atomic::{AtomicU64,Ordering};
static CLOCK: AtomicU64 = AtomicU64::new(0);
struct Bus; static I2C3: Bus = Bus;
impl Bus { async fn lock(&self) {} }
mod embassy_time {
    pub struct Instant;
    impl Instant { pub fn now()->Self { Self } pub fn as_millis(&self)->u64 { super::CLOCK.load(super::Ordering::Relaxed) } }
    pub struct Timer; impl Timer { pub async fn after_millis(_:u64) {} }
}
mod flash {
    pub const SECTOR:usize=8192;
    pub static DATA:std::sync::Mutex<[u8;6*SECTOR]>=std::sync::Mutex::new([255;6*SECTOR]);
    pub fn read(at:usize)->u32 { let d=DATA.lock().unwrap();u32::from_le_bytes(d[at-0x62000..at-0x62000+4].try_into().unwrap()) }
    pub fn blank(base:usize)->bool { DATA.lock().unwrap()[base-0x62000..base-0x62000+SECTOR].iter().all(|&b|b==255) }
    pub fn program_blank(base:usize,at:usize,words:&[u32])->bool {
        assert!(at>=base && at+words.len()*4<=base+SECTOR);
        let mut d=DATA.lock().unwrap();let offset=at-0x62000;
        if d[offset..offset+words.len()*4].iter().any(|&b|b!=255) { return false; }
        for (i,w) in words.iter().enumerate() {d[offset+i*4..offset+i*4+4].copy_from_slice(&w.to_le_bytes());} true
    }
}
fn block_on<F:std::future::Future>(f:F)->F::Output {
    let mut f=std::pin::pin!(f);let w=std::task::Waker::noop();let mut c=std::task::Context::from_waker(w);
    match f.as_mut().poll(&mut c){std::task::Poll::Ready(v)=>v,_=>panic!("unexpected I/O wait")}
}
mod logger {
use std::{sync::Mutex,collections::VecDeque,marker::PhantomData};
struct CriticalSectionRawMutex;
struct Channel<M,T,const N:usize>{q:Mutex<VecDeque<T>>,m:PhantomData<M>}
impl<M,T,const N:usize> Channel<M,T,N>{
 const fn new()->Self{Self{q:Mutex::new(VecDeque::new()),m:PhantomData}}
 fn try_send(&self,t:T)->Result<(),()>{let mut q=self.q.lock().unwrap();if q.len()==N{Err(())}else{q.push_back(t);Ok(())}}
 fn try_receive(&self)->Result<T,()>{self.q.lock().unwrap().pop_front().ok_or(())}
 fn is_empty(&self)->bool{self.q.lock().unwrap().is_empty()}
}
'''
tests = (Path(__file__).with_name('writer_tests.rs')).read_text(encoding='utf-8') + '\n}\n'
out = root / 'target/logger-writer-harness.rs'
out.write_text(adapter + source + tests, encoding='utf-8')
libs = root / 'target/logger-host/debug/deps'
rlib = max(libs.glob('libfirmware_core-*.rlib'), key=lambda p:p.stat().st_mtime)
rustc = root / 'target/capture-toolchain/bin-dist/bin/rustc.exe'
exe = root / 'target/logger-writer-harness.exe'
subprocess.run([str(rustc),'--edition=2024','--test',str(out),'--extern',f'firmware_core={rlib}','-L',f'dependency={libs}','-o',str(exe)],check=True)
subprocess.run([str(exe),'--test-threads=1','--nocapture'],check=True)

# Independently follow the FAT12 directory and cluster chains, then use the
# public decoder to check paired files with repeated packet IDs across sessions.
import importlib.util
import struct
image = (root / 'target/logger-v07-host-disk.img').read_bytes()
u16 = lambda at: struct.unpack_from('<H', image, at)[0]
assert u16(11) == 512 and image[13] == 1
fat_start = u16(14) * 512
root_start = (u16(14) + image[16] * u16(22)) * 512
data_start = root_start + ((u16(17) * 32 + 511) // 512) * 512
export = root / 'target/logger-v07-host-export'
export.mkdir(exist_ok=True)
paths = []
claimed = set()
for at in range(root_start, root_start + u16(17) * 32, 32):
    entry = image[at:at+32]
    if entry[0] == 0:
        break
    if entry[11] & 8:
        continue
    name = entry[:8].decode().rstrip() + '.' + entry[8:11].decode().rstrip()
    cluster, size = struct.unpack_from('<HI', entry, 26)
    content = bytearray()
    while cluster < 0xff8:
        assert 2 <= cluster < (len(image) - data_start)//512 + 2
        assert cluster not in claimed, 'overlapping or cyclic FAT chains'
        claimed.add(cluster)
        begin = data_start + (cluster - 2) * 512
        content.extend(image[begin:begin+512])
        off = fat_start + cluster + cluster//2
        cluster = (u16(off) >> (4 * (cluster % 2))) & 0xfff
    assert len(content) >= size
    path = export / name
    path.write_bytes(content[:size])
    if path.suffix == '.CSV':
        paths.append(path)
assert len(paths) == 64
assert {p.name for p in paths} == {f'{kind}{session:04d}01.CSV' for kind in ('VI', 'PD') for session in range(1, 33)}
spec = importlib.util.spec_from_file_location('logger_decode', root / 'tools/logger_decode.py')
decoder = importlib.util.module_from_spec(spec)
spec.loader.exec_module(decoder)
rows = decoder.decode(paths)
for session in range(1, 33):
    group = [r for r in rows if r['session'] == session]
    assert len(group) == 2 and {r['kind'] for r in group} == {'SAMPLE', 'PD_RX'}
    packet = next(r for r in group if r['kind'] == 'PD_RX')
    assert packet['status'] == 'COMPLETE' and packet['event_id'] == 1
assert (export/'STORAGE.BIN').stat().st_size == 49152
print('FAT12 export + public decoder: 32 paired sessions / 64 CSVs passed')
