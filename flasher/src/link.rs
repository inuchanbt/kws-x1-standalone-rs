//! One-step install: send the upper region to the unpacker over USB HID.

use anyhow::{Context, Result, bail, ensure};
use firmware_image::{
    SECTOR_BYTES,
    link::{self, DATA_BYTES, MANUFACTURER, PID, PRODUCT, Reply, Report, Status, VID},
    region_hash,
};
use hidapi::{HidApi, HidDevice};
use std::{
    thread,
    time::{Duration, Instant},
};

// The unpacker checks the main stream before it enumerates.
const APPEAR_TIMEOUT: Duration = Duration::from_secs(90);
const APPEAR_POLL: Duration = Duration::from_millis(250);
// One sector erase and program, with margin.
const REPLY_TIMEOUT: Duration = Duration::from_secs(15);
const SECTOR: usize = SECTOR_BYTES as usize;

pub trait Port {
    fn send(&self, report: &Report) -> Result<()>;
    fn receive(&self, timeout: Duration) -> Result<Option<Report>>;
}

impl Port for HidDevice {
    fn send(&self, report: &Report) -> Result<()> {
        // HIDAPI requires a report ID byte; the link uses unnumbered reports.
        let mut buffer = [0; link::REPORT_BYTES + 1];
        buffer[1..].copy_from_slice(report);
        let written = self.write(&buffer)?;
        ensure!(
            written == buffer.len(),
            "Short HID write: {written}/65 bytes"
        );
        Ok(())
    }

    fn receive(&self, timeout: Duration) -> Result<Option<Report>> {
        let mut report = [0; link::REPORT_BYTES];
        let millis = i32::try_from(timeout.as_millis().max(1))?;
        let read = self.read_timeout(&mut report, millis)?;
        Ok((read == report.len()).then_some(report))
    }
}

#[derive(Debug, PartialEq, Eq)]
pub enum Outcome {
    Sent,
    /// The meter already held this build's upper region.
    AlreadyInstalled,
}

/// The next reply `wanted` accepts; other reports are stale and skipped.
fn reply<T>(port: &impl Port, what: &str, wanted: impl Fn(Reply) -> Option<T>) -> Result<T> {
    let deadline = Instant::now() + REPLY_TIMEOUT;
    while let Some(remaining) = deadline.checked_duration_since(Instant::now()) {
        if let Some(found) = port
            .receive(remaining)?
            .and_then(|report| link::parse(&report))
            .and_then(&wanted)
        {
            return Ok(found);
        }
    }
    bail!(
        "{what}: no reply within {:.0}s",
        REPLY_TIMEOUT.as_secs_f32()
    )
}

/// Send `upper` to the unpacker and wait for it to confirm the installed hash.
pub fn install(port: &impl Port, upper: &[u8]) -> Result<Outcome> {
    port.send(&link::query())?;
    let (needed, len, hash) = reply(port, "Installer query", |reply| match reply {
        Reply::Info { needed, len, hash } => Some((needed, len, hash)),
        Reply::Ack { .. } => None,
    })?;
    ensure!(
        len as usize == upper.len() && hash == region_hash(upper),
        "The meter's installer is from a different build than this file; \
         re-enter bootloader mode and flash this file again"
    );
    if !needed {
        return Ok(Outcome::AlreadyInstalled);
    }
    let mut offset = 0;
    while offset < upper.len() {
        let end = (offset + SECTOR).min(upper.len());
        let mut at = offset;
        for chunk in upper[offset..end].chunks(DATA_BYTES) {
            let report = link::data(at as u32, chunk).context("Building a data report")?;
            port.send(&report)?;
            at += chunk.len();
        }
        let (status, next) = reply(port, "Upper region sector", |reply| match reply {
            Reply::Ack { status, next } => Some((status, next)),
            Reply::Info { .. } => None,
        })?;
        match status {
            Status::Next if next as usize == end && end < upper.len() => {}
            Status::Done if next as usize == end && end == upper.len() => {
                return Ok(Outcome::Sent);
            }
            Status::WriteFailed => bail!(
                "The meter could not program its flash at upper offset {offset:#x}; \
                 run the same command again"
            ),
            Status::HashMismatch => bail!(
                "The installed upper region does not match this file; \
                 run the same command again"
            ),
            _ => bail!(
                "Unexpected installer reply {status:?} at upper offset {offset:#x}; \
                 run the same command again"
            ),
        }
        tracing::info!(
            bytes = end,
            total = upper.len(),
            "Upper region sector written"
        );
        offset = end;
    }
    bail!("The upper region is empty")
}

/// Exactly one attached installer, opened; `None` if none is attached.
pub fn find(api: &mut HidApi) -> Result<Option<HidDevice>> {
    api.refresh_devices().context("Listing HID devices")?;
    let matches: Vec<_> = api
        .device_list()
        .filter(|device| {
            device.vendor_id() == VID
                && device.product_id() == PID
                && device.manufacturer_string() == Some(MANUFACTURER)
                && device.product_string() == Some(PRODUCT)
        })
        .collect();
    match matches[..] {
        [] => Ok(None),
        [device] => api
            .open_path(device.path())
            .map(Some)
            .context("Opening the installer; on Linux check hidraw permissions for 16c0:05df"),
        _ => bail!("Several meters are waiting in the installer; connect only one"),
    }
}

/// Wait for the unpacker to enumerate, then send it `upper`.
pub fn run(api: &mut HidApi, upper: &[u8]) -> Result<Outcome> {
    tracing::info!("Waiting for the meter's installer; keep the cable connected");
    let deadline = Instant::now() + APPEAR_TIMEOUT;
    let mut last_error = None;
    while Instant::now() < deadline {
        match find(api) {
            Ok(Some(device)) => return install(&device, upper),
            Ok(None) => {}
            // Permissions can settle a moment after the device appears.
            Err(error) => last_error = Some(error),
        }
        thread::sleep(APPEAR_POLL);
    }
    match last_error {
        Some(error) => Err(error),
        None => bail!(
            "The installer did not appear within {:.0}s. If the meter shows the \
             measurement screen, its upper region was already installed and the update \
             is complete. Otherwise keep it connected and run the same command again",
            APPEAR_TIMEOUT.as_secs_f32()
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use firmware_image::link::Receiver;
    use std::{cell::RefCell, collections::VecDeque};

    /// The unpacker's receiver over a byte array standing in for flash.
    struct Meter {
        receiver: RefCell<Receiver>,
        flash: RefCell<Vec<u8>>,
        sector: RefCell<Vec<u8>>,
        replies: RefCell<VecDeque<Report>>,
        expected: u32,
        /// A sector offset that fails to program, and a byte to corrupt.
        fail_at: Option<usize>,
        corrupt: Option<usize>,
    }

    impl Meter {
        fn new(len: usize, hash: u32, present: bool) -> Self {
            Self {
                receiver: RefCell::new(Receiver::new(len, hash, present)),
                flash: RefCell::new(vec![0xff; len]),
                sector: RefCell::new(vec![0; SECTOR]),
                replies: RefCell::new(VecDeque::new()),
                expected: hash,
                fail_at: None,
                corrupt: None,
            }
        }
    }

    impl Port for Meter {
        fn send(&self, report: &Report) -> Result<()> {
            let reply = self.receiver.borrow_mut().handle(
                report,
                &mut self.sector.borrow_mut(),
                |offset, bytes| {
                    if self.fail_at == Some(offset) {
                        return false;
                    }
                    self.flash.borrow_mut()[offset..offset + bytes.len()].copy_from_slice(bytes);
                    true
                },
                || {
                    let mut image = self.flash.borrow().clone();
                    if let Some(at) = self.corrupt {
                        image[at] ^= 1;
                    }
                    region_hash(&image) == self.expected
                },
            );
            self.replies.borrow_mut().extend(reply);
            Ok(())
        }

        fn receive(&self, _timeout: Duration) -> Result<Option<Report>> {
            Ok(self.replies.borrow_mut().pop_front())
        }
    }

    fn region(len: usize) -> Vec<u8> {
        (0..len).map(|index| (index * 31 % 251) as u8).collect()
    }

    #[test]
    fn sends_a_region_with_a_partial_last_sector() {
        let upper = region(3 * SECTOR + 1016);
        let meter = Meter::new(upper.len(), region_hash(&upper), false);
        assert_eq!(install(&meter, &upper).unwrap(), Outcome::Sent);
        assert_eq!(*meter.flash.borrow(), upper);
        assert!(meter.receiver.borrow().done());
    }

    #[test]
    fn sends_a_region_of_whole_sectors() {
        let upper = region(2 * SECTOR);
        let meter = Meter::new(upper.len(), region_hash(&upper), false);
        assert_eq!(install(&meter, &upper).unwrap(), Outcome::Sent);
        assert_eq!(*meter.flash.borrow(), upper);
    }

    #[test]
    fn skips_an_installed_region() {
        let upper = region(SECTOR + 8);
        let meter = Meter::new(upper.len(), region_hash(&upper), true);
        assert_eq!(install(&meter, &upper).unwrap(), Outcome::AlreadyInstalled);
        assert!(meter.receiver.borrow().done());
        assert!(meter.flash.borrow().iter().all(|byte| *byte == 0xff));
    }

    #[test]
    fn refuses_another_builds_installer() {
        let upper = region(SECTOR);
        let meter = Meter::new(upper.len(), region_hash(&upper) ^ 1, false);
        let error = install(&meter, &upper).unwrap_err().to_string();
        assert!(error.contains("different build"), "{error}");
        assert!(meter.flash.borrow().iter().all(|byte| *byte == 0xff));
    }

    #[test]
    fn reports_a_failed_sector_write() {
        let upper = region(2 * SECTOR + 4);
        let mut meter = Meter::new(upper.len(), region_hash(&upper), false);
        meter.fail_at = Some(SECTOR);
        let error = install(&meter, &upper).unwrap_err().to_string();
        assert!(error.contains("could not program"), "{error}");
        assert!(!meter.receiver.borrow().done());
    }

    #[test]
    fn reports_a_hash_mismatch_and_allows_a_retry() {
        let upper = region(SECTOR + 400);
        let mut meter = Meter::new(upper.len(), region_hash(&upper), false);
        meter.corrupt = Some(5);
        let error = install(&meter, &upper).unwrap_err().to_string();
        assert!(error.contains("does not match"), "{error}");
        assert!(!meter.receiver.borrow().done());
        meter.corrupt = None;
        assert_eq!(install(&meter, &upper).unwrap(), Outcome::Sent);
    }

    #[test]
    fn a_query_restarts_an_interrupted_transfer() {
        let upper = region(2 * SECTOR);
        let meter = Meter::new(upper.len(), region_hash(&upper), false);
        meter.send(&link::query()).unwrap();
        meter
            .send(&link::data(0, &upper[..DATA_BYTES]).unwrap())
            .unwrap();
        assert_eq!(install(&meter, &upper).unwrap(), Outcome::Sent);
        assert_eq!(*meter.flash.borrow(), upper);
    }

    #[test]
    fn rejects_out_of_order_and_overlong_data() {
        let upper = region(DATA_BYTES + 4);
        let meter = Meter::new(upper.len(), region_hash(&upper), false);
        // Data before a query.
        meter.send(&link::data(0, &upper[..8]).unwrap()).unwrap();
        // A skipped offset, then a report past the region's end.
        meter.send(&link::query()).unwrap();
        meter.send(&link::data(8, &upper[..8]).unwrap()).unwrap();
        meter
            .send(&link::data(0, &upper[..DATA_BYTES]).unwrap())
            .unwrap();
        meter
            .send(&link::data(DATA_BYTES as u32, &upper[..8]).unwrap())
            .unwrap();
        let replies: Vec<_> = meter
            .replies
            .borrow()
            .iter()
            .filter_map(|report| link::parse(report))
            .collect();
        let bad = Reply::Ack {
            status: Status::BadOffset,
            next: 0,
        };
        assert_eq!(replies[0], bad);
        assert!(matches!(replies[1], Reply::Info { needed: true, .. }));
        assert_eq!(replies[2], bad);
        assert_eq!(replies[3], bad);
        assert_eq!(replies.len(), 4);
        assert!(meter.flash.borrow().iter().all(|byte| *byte == 0xff));
    }

    #[test]
    fn splits_a_one_step_file() {
        let upper = region(SECTOR + 12);
        let mut file = vec![0; link::CONTAINER_BYTES];
        assert_eq!(link::split(&file), Ok((&file[..], None)));
        file.extend_from_slice(&link::section_header(&upper));
        file.extend_from_slice(&upper);
        let (container, found) = link::split(&file).unwrap();
        assert_eq!(container.len(), link::CONTAINER_BYTES);
        assert_eq!(found, Some(&upper[..]));
        let last = file.len() - 1;
        file[last] ^= 1;
        assert!(link::split(&file).is_err());
        file[last] ^= 1;
        file.truncate(last);
        assert!(link::split(&file).is_err());
    }
}
