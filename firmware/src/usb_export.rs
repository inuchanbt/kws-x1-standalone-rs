//! USB Mass Storage Bulk-Only Transport, with a single read-only SCSI disk.

use firmware_core::screenshot_disk::{BLOCK_BYTES, BLOCKS, Disk};
use usb_device::{UsbError, class_prelude::*, device::UsbRev, prelude::*};

use crate::usb_bus::Bus;

const PACKET: usize = 64;
const CBW: u32 = 0x4342_5355;
const CSW: u32 = 0x5342_5355;
const BAD_COMMAND: [u8; 2] = [0x05, 0x20];
const BAD_FIELD: [u8; 2] = [0x05, 0x24];
const BAD_LBA: [u8; 2] = [0x05, 0x21];
const WRITE_PROTECTED: [u8; 2] = [0x07, 0x27];

#[derive(Clone, Copy)]
enum Phase {
    Command,
    Send {
        lba: Option<u32>,
        size: u32,
        sent: u32,
    },
    Zero,
    Discard(u32),
    Status,
    Recovery,
}

struct Storage<'a, B: UsbBus> {
    interface: InterfaceNumber,
    input: EndpointIn<'a, B>,
    output: EndpointOut<'a, B>,
    disk: Disk<'a>,
    data: [u8; BLOCK_BYTES],
    cached: Option<u32>,
    phase: Phase,
    sense: [u8; 2],
    tag: [u8; 4],
    residue: u32,
    result: u8,
}

impl<'a, B: UsbBus> Storage<'a, B> {
    fn new(usb: &'a UsbBusAllocator<B>, disk: Disk<'a>) -> Self {
        Self {
            interface: usb.interface(),
            input: usb.bulk(PACKET as u16),
            output: usb.bulk(PACKET as u16),
            disk,
            data: [0; BLOCK_BYTES],
            cached: None,
            phase: Phase::Command,
            sense: [0; 2],
            tag: [0; 4],
            residue: 0,
            result: 0,
        }
    }

    fn invalid_cbw(&mut self) {
        self.input.stall();
        self.output.stall();
        self.phase = Phase::Recovery;
    }

    fn command(&mut self, packet: &[u8]) {
        if packet.len() != 31
            || packet[..4] != CBW.to_le_bytes()
            || packet[12] & 0x7f != 0
            || packet[13] != 0
            || !(1..=16).contains(&packet[14])
        {
            self.invalid_cbw();
            return;
        }
        self.tag.copy_from_slice(&packet[4..8]);
        self.residue = u32::from_le_bytes(packet[8..12].try_into().unwrap());
        self.result = 0;
        let host_in = packet[12] == 0x80;
        let cdb = &packet[15..15 + usize::from(packet[14])];

        // Commands with inline responses reuse the sector buffer.
        self.cached = None;
        self.data[..64].fill(0);
        match self.scsi(cdb) {
            Ok((lba, size)) if size != 0 && self.residue != 0 && host_in => {
                if self.residue < size {
                    self.result = 2;
                }
                self.phase = Phase::Send {
                    lba,
                    size: size.min(self.residue),
                    sent: 0,
                };
            }
            Ok((_, size)) => {
                if size != 0 || self.residue != 0 {
                    self.result = 2;
                }
                self.end_data(host_in);
            }
            Err(sense) => {
                self.sense = sense;
                self.result = 1;
                self.end_data(host_in);
            }
        }
    }

    fn end_data(&mut self, host_in: bool) {
        self.phase = if self.residue == 0 {
            Phase::Status
        } else if host_in {
            Phase::Zero
        } else {
            // Receive and discard write attempts without modifying the volume.
            Phase::Discard(self.residue)
        };
    }

    fn scsi(&mut self, cdb: &[u8]) -> Result<(Option<u32>, u32), [u8; 2]> {
        let out = &mut self.data;
        let size = match (cdb[0], cdb.len()) {
            (0x00 | 0x1b | 0x1e, 6..) | (0x2f | 0x35, 10..) | (0x91, 16..) => 0,
            (0x03, 6..) => {
                let [key, asc] = core::mem::take(&mut self.sense);
                if cdb[1] & 1 == 0 {
                    out[0] = 0x70;
                    out[2] = key;
                    out[7] = 10;
                    out[12] = asc;
                    18.min(u32::from(cdb[4]))
                } else {
                    out[..3].copy_from_slice(&[0x72, key, asc]);
                    8.min(u32::from(cdb[4]))
                }
            }
            (0x12, 6..) => {
                let len = if cdb[1] & 1 != 0 {
                    match cdb[2] {
                        0 => {
                            out[3..6].copy_from_slice(&[2, 0, 0x80]);
                            6
                        }
                        0x80 => {
                            out[1..7].copy_from_slice(b"\x80\0\x03X1C");
                            7
                        }
                        _ => return Err(BAD_FIELD),
                    }
                } else if cdb[2] == 0 {
                    out[..5].copy_from_slice(&[0, 0x80, 4, 2, 31]);
                    out[8..36].copy_from_slice(b"KWS X1  SCREEN CAPTURE  1.00");
                    36
                } else {
                    return Err(BAD_FIELD);
                };
                len.min(u32::from(cdb[4]))
            }
            (0x1a, 6..) => {
                out[..4].copy_from_slice(&[3, 0, 0x80, 0]);
                4.min(u32::from(cdb[4]))
            }
            (0x5a, 10..) => {
                out[..4].copy_from_slice(&[0, 6, 0, 0x80]);
                8.min(u32::from(be16(cdb, 7)))
            }
            (0x23, 10..) => {
                out[3] = 8;
                out[4..8].copy_from_slice(&BLOCKS.to_be_bytes());
                out[8..12].copy_from_slice(&[2, 0, 2, 0]); // Formatted, 512 bytes.
                12.min(u32::from(be16(cdb, 7)))
            }
            (0x25, 10..) => {
                out[..4].copy_from_slice(&(BLOCKS - 1).to_be_bytes());
                out[4..8].copy_from_slice(&(BLOCK_BYTES as u32).to_be_bytes());
                8
            }
            (0x9e, 16..) if cdb[1] == 0x10 => {
                out[4..8].copy_from_slice(&(BLOCKS - 1).to_be_bytes());
                out[8..12].copy_from_slice(&(BLOCK_BYTES as u32).to_be_bytes());
                32.min(be32(cdb, 10))
            }
            (0x28, 10..) | (0xa8, 12..) | (0x88, 16..) => {
                let (lba, blocks) = match cdb[0] {
                    0x28 => (be32(cdb, 2), u32::from(be16(cdb, 7))),
                    0xa8 => (be32(cdb, 2), be32(cdb, 6)),
                    _ if be32(cdb, 2) == 0 => (be32(cdb, 6), be32(cdb, 10)),
                    _ => return Err(BAD_LBA),
                };
                if lba > BLOCKS || blocks > BLOCKS - lba {
                    return Err(BAD_LBA);
                }
                return Ok((Some(lba), blocks * BLOCK_BYTES as u32));
            }
            (0xa0, 12..) => {
                out[3] = 8; // One eight-byte LUN address, zero.
                16.min(be32(cdb, 6))
            }
            (0x04 | 0x15 | 0x2a | 0x55 | 0x8a | 0xaa, _) => return Err(WRITE_PROTECTED),
            _ => return Err(BAD_COMMAND),
        };
        Ok((None, size))
    }

    fn transfer(&mut self) -> usb_device::Result<()> {
        let mut packet = [0; PACKET];
        match self.phase {
            Phase::Command => {
                let count = self.output.read(&mut packet)?;
                self.command(&packet[..count]);
            }
            Phase::Send { lba, size, sent } => {
                let offset = if let Some(first) = lba {
                    let sector = first + sent / BLOCK_BYTES as u32;
                    if self.cached != Some(sector) {
                        self.disk.read(sector, &mut self.data);
                        self.cached = Some(sector);
                    }
                    sent as usize % BLOCK_BYTES
                } else {
                    sent as usize
                };
                let count = (size - sent).min(PACKET as u32) as usize;
                self.input.write(&self.data[offset..offset + count])?;
                let sent = sent + count as u32;
                self.residue -= count as u32;
                self.phase = if sent < size {
                    Phase::Send { lba, size, sent }
                } else if self.residue != 0 && count == PACKET {
                    Phase::Zero
                } else {
                    Phase::Status
                };
            }
            Phase::Zero => {
                self.input.write(&[])?;
                self.phase = Phase::Status;
            }
            Phase::Discard(left) => {
                let count = self.output.read(&mut packet)? as u32;
                self.phase = if count >= left || count < PACKET as u32 {
                    Phase::Status
                } else {
                    Phase::Discard(left - count)
                };
            }
            Phase::Status => {
                packet[..4].copy_from_slice(&CSW.to_le_bytes());
                packet[4..8].copy_from_slice(&self.tag);
                packet[8..12].copy_from_slice(&self.residue.to_le_bytes());
                packet[12] = self.result;
                self.input.write(&packet[..13])?;
                self.phase = if self.result == 2 {
                    Phase::Recovery
                } else {
                    Phase::Command
                };
            }
            Phase::Recovery => {}
        }
        Ok(())
    }
}

impl<B: UsbBus> UsbClass<B> for Storage<'_, B> {
    fn get_configuration_descriptors(
        &self,
        writer: &mut DescriptorWriter,
    ) -> usb_device::Result<()> {
        writer.interface(self.interface, 8, 6, 0x50)?;
        writer.endpoint(&self.output)?;
        writer.endpoint(&self.input)
    }

    fn reset(&mut self) {
        self.phase = Phase::Command;
        self.sense = [0; 2];
    }

    fn control_in(&mut self, transfer: ControlIn<B>) {
        let req = transfer.request();
        if req.request_type == control::RequestType::Class
            && req.recipient == control::Recipient::Interface
            && req.index == u16::from(u8::from(self.interface))
            && req.request == 0xfe
            && req.value == 0
            && req.length == 1
        {
            transfer.accept_with(&[0]).ok();
        }
    }

    fn control_out(&mut self, transfer: ControlOut<B>) {
        let req = transfer.request();
        if req.request_type == control::RequestType::Class
            && req.recipient == control::Recipient::Interface
            && req.index == u16::from(u8::from(self.interface))
            && req.request == 0xff
            && req.value == 0
            && req.length == 0
        {
            self.phase = Phase::Command;
            transfer.accept().ok();
        }
    }
}

fn be16(bytes: &[u8], offset: usize) -> u16 {
    u16::from_be_bytes(bytes[offset..offset + 2].try_into().unwrap())
}

fn be32(bytes: &[u8], offset: usize) -> u32 {
    u32::from_be_bytes(bytes[offset..offset + 4].try_into().unwrap())
}

const fn usb_id(hex: &str) -> u16 {
    assert!(hex.len() == 4, "USB IDs must be four hex digits");
    let mut id = 0;
    let mut i = 0;
    while i < 4 {
        let digit = match hex.as_bytes()[i] {
            c @ b'0'..=b'9' => c - b'0',
            c @ b'a'..=b'f' => c - b'a' + 10,
            c @ b'A'..=b'F' => c - b'A' + 10,
            _ => panic!("USB IDs must be four hex digits"),
        };
        id = (id << 4) | digit as u16;
        i += 1;
    }
    assert!(id != 0, "USB IDs must be nonzero");
    id
}

const USB_ID: UsbVidPid = match (option_env!("KWS_X1_USB_VID"), option_env!("KWS_X1_USB_PID")) {
    (Some(vid), Some(pid)) => UsbVidPid(usb_id(vid), usb_id(pid)),
    // pid.codes reserves this pair for private testing.
    (None, None) => UsbVidPid(0x1209, 0x0001),
    _ => panic!("Set both KWS_X1_USB_VID and KWS_X1_USB_PID"),
};

/// # Safety
/// Only export boot calls this, before starting any measurement task. The
/// measurement output is disconnected and the input is a PC's USB port.
pub unsafe fn run(hclk: u32, disk: Disk<'_>) -> ! {
    use embassy_hc32::gpio::{AnalogPin, Port};
    // SAFETY: the USB PHY exclusively owns these dedicated PA11/PA12 pads.
    let _data_pins = unsafe { [AnalogPin::new(Port::A, 11), AnalogPin::new(Port::A, 12)] };
    if let Some(bus) = unsafe { Bus::start(hclk) } {
        let allocator = UsbBusAllocator::new(bus);
        let mut storage = Storage::new(&allocator, disk);
        let mut device = UsbDeviceBuilder::new(&allocator, USB_ID)
            .usb_rev(UsbRev::Usb200)
            .max_packet_size_0(PACKET as u8)
            .unwrap()
            .strings(&[StringDescriptors::default().product("KWS-X1 Screenshot")])
            .unwrap()
            .max_power(100)
            .unwrap()
            .build();
        loop {
            device.poll(&mut [&mut storage]);
            if device.state() == UsbDeviceState::Configured
                && let Err(error) = storage.transfer()
                && error != UsbError::WouldBlock
            {
                storage.invalid_cbw();
            }
        }
    }
    loop {
        cortex_m::asm::wfi();
    }
}
