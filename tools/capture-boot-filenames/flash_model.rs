//! Host fault injection. Run serially; flash model is shared and not concurrent.
//! A partial word model is bounded evidence, not a physical power-cut qualification.
extern crate self as cortex_m;
extern crate self as embassy_sync;
extern crate self as embassy_time;
extern crate self as firmware_image;
extern crate firmware_core;
use std::sync::atomic::{AtomicUsize, AtomicU32, Ordering::SeqCst};
use firmware_core::screenshot::{Encoder, CapturePage, FRAME_BYTES};
pub const UPPER_END: u32 = 0x62000;
pub const CAPTURE_DIAGNOSTIC: bool = false;
#[path = "storage_adapter.rs"]
#[allow(dead_code)] mod capture;
pub mod render { pub type Frame = [u16; firmware_core::panel::PIXELS]; }
pub mod ui {
    pub const WIDE_COLUMNS: usize = 36;
    pub fn report_capture_colors(_: firmware_core::buttons::Detail) -> [u16;3] { [0;3] }
}
pub mod blocking_mutex { pub mod raw { pub struct CriticalSectionRawMutex; } }
pub mod signal {
    use std::marker::PhantomData;
    pub struct Signal<M,T>(PhantomData<fn()->(M,T)>);
    impl<M,T> Signal<M,T> {
        pub const fn new()->Self { Self(PhantomData) }
        pub fn signal(&self,_:T) {}
        pub async fn wait(&self)->T { std::future::pending().await }
    }
}
pub mod interrupt { pub fn free<T>(f:impl FnOnce(())->T)->T { f(()) } }
pub struct Timer;
impl Timer { pub async fn after_millis(_:u64) {} }
pub struct Bus;
pub static I2C3: Bus = Bus;
impl Bus { pub async fn lock(&self) {} }
pub fn framebuffer()->*mut u16 { std::ptr::null_mut() }

mod flash {
    use super::*;
    pub const SECTOR: usize = 8192;
    pub const BLANK: u32 = !0;
    pub const VERIFY_FAILED: u32 = 128;
    pub const SECTORS: [usize;6] = [0x7c000,0x7a000,0x78000,0x76000,0x74000,0x72000];
    pub const ENERGY_SECTORS: [usize;2] = [0x78000,0x7a000];
    static mut DATA: [u32;0x80000/4] = [BLANK;0x80000/4];
    pub static LEFT: AtomicUsize = AtomicUsize::new(usize::MAX);
    pub static WRITTEN: AtomicUsize = AtomicUsize::new(0);
    pub static TORN: AtomicU32 = AtomicU32::new(0);
    fn word(at:usize)->*mut u32 {
        assert!(at<0x80000 && at%4==0);
        unsafe { std::ptr::addr_of_mut!(DATA).cast::<u32>().add(at/4) }
    }
    pub unsafe fn bytes(at:usize,len:usize)-> &'static [u8] {
        assert!(at+len<=0x80000);
        unsafe { std::slice::from_raw_parts(word(at).cast(),len) }
    }
    pub fn snapshot()->Vec<u8> { unsafe { bytes(0,0x80000).to_vec() } }
    pub fn restore(data:&[u8]) {
        assert_eq!(data.len(),0x80000);
        unsafe { std::ptr::copy_nonoverlapping(data.as_ptr(),word(0).cast::<u8>(),data.len()) };
        LEFT.store(usize::MAX,SeqCst); WRITTEN.store(0,SeqCst); TORN.store(0,SeqCst);
    }
    pub fn reset() { restore(&vec![0xff;0x80000]); }
    pub fn read(at:usize)->u32 { unsafe { word(at).read() } }
    pub fn force(at:usize,value:u32) { unsafe { word(at).write(value) }; }
    pub fn blank(base:usize)->bool { (base..base+SECTOR).step_by(4).all(|at|read(at)==BLANK) }
    pub fn note_failure(_:usize,_:u32) {}
    pub fn program_blank(base:usize,at:usize,words:&[u32])->bool {
        assert!((0x62000..0x6e000).contains(&base) || SECTORS.contains(&base));
        assert!(!ENERGY_SECTORS.contains(&base));
        assert!(base%SECTOR==0 && at>=base && at+words.len()*4<=base+SECTOR);
        for (i,&data) in words.iter().enumerate() {
            let at=at+i*4;
            assert_eq!(read(at),BLANK);
            if LEFT.load(SeqCst)==0 {
                force(at,data | !TORN.load(SeqCst));
                return false;
            }
            LEFT.fetch_sub(1,SeqCst); WRITTEN.fetch_add(1,SeqCst);
            force(at,data);
        }
        true
    }
    pub unsafe extern "C" fn erase_sector(_:u32)->u32 { panic!("capture save must not erase"); }
}
fn finish<F:std::future::Future>(f:F)->F::Output {
    let mut f=std::pin::pin!(f);
    match f.as_mut().poll(&mut std::task::Context::from_waker(std::task::Waker::noop())) {
        std::task::Poll::Ready(out)=>out,
        _=>panic!("host timers and bus complete immediately"),
    }
}
fn record()->Vec<u8> {
    let frame=[0u8;FRAME_BYTES];
    let encoder=Encoder::new(&frame).unwrap();
    let (_,bytes)=encoder.record_for(CapturePage::Overview).unwrap();
    bytes.collect()
}
fn save(record:&[u8])->capture::Status { finish(capture::save(record.len(),&mut record.iter().copied())) }
fn count()->usize { unsafe {capture::export_count()} }
