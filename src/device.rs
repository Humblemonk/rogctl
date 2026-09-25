//! Discovery and battery queries for ASUS mice over Linux hidraw.
//!
//! The protocol mirrors G-Helper's `AsusMouse`: send `[report_id, 0x12, 0x07]`
//! zero-padded to the model's packet size and read back a reply echoing that
//! header. In the reply, byte 10 is the charging flag and the battery level's
//! position depends on the model (see [`Battery`]). Most models also put the
//! auto power-off setting in byte 6 and the low-battery warning in byte 7.

use std::fs::{self, File, OpenOptions};
use std::io::{self, Read, Write};
use std::os::fd::AsRawFd;
use std::os::unix::fs::OpenOptionsExt;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use crate::models::{MODELS, OMNI_PID};

const ASUS_VID: u16 = 0x0b05;
const MAX_PACKET: usize = 65;
const REPLY_TIMEOUT: Duration = Duration::from_millis(500);
/// Reports the kernel buffers per open hidraw file (`HIDRAW_BUFFER_SIZE`).
const HIDRAW_QUEUE: usize = 64;

/// One way a supported mouse shows up on USB: a product ID and the HID
/// interface that answers vendor commands on it.
#[derive(Debug)]
pub struct Model {
    pub name: &'static str,
    pub pid: u16,
    pub interface: u8,
    pub report_id: u8,
    /// Bytes written per command, including the report ID byte.
    pub packet_size: usize,
    /// How the mouse is connected: receiver (true) or cable (false).
    pub wireless: bool,
    pub battery: Battery,
    /// Reply bytes 6 and 7 hold the power-off and low-battery settings.
    pub settings: bool,
    pub matches: Match,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Battery {
    /// Byte 5 is the percentage.
    Percent,
    /// `byte` is a 0-4 level, reported as multiples of 25%.
    Quarters { byte: usize },
}

/// How to tell apart models that share a product ID and interface.
#[derive(Debug)]
pub enum Match {
    Any,
    /// The receiver's USB product name contains this (case-insensitive).
    Product(&'static str),
    /// The Omni receiver reports one of these as its paired device.
    OmniPaired(&'static [u16]),
}

#[derive(Debug)]
pub struct Device {
    pub model: &'static Model,
    pub hidraw: PathBuf,
}

/// Finds connected supported mice, in `MODELS` order.
pub fn discover() -> Vec<Device> {
    let mut found: Vec<Device> = Vec::new();
    let Ok(entries) = fs::read_dir("/sys/class/hidraw") else {
        return found;
    };
    for entry in entries.flatten() {
        let sys = entry.path();
        let Some((vid, pid)) = hid_ids(&sys) else {
            continue;
        };
        if vid != ASUS_VID {
            continue;
        }
        let Some(interface) = interface_number(&sys) else {
            continue;
        };
        if !MODELS
            .iter()
            .any(|m| m.pid == pid && m.interface == interface)
        {
            continue;
        }
        let hidraw = Path::new("/dev").join(entry.file_name());
        let product = usb_product(&sys).unwrap_or_default();
        let paired = if pid == OMNI_PID {
            omni_paired(&hidraw).unwrap_or_default()
        } else {
            Vec::new()
        };
        if let Some(model) = select_model(pid, interface, &product, &paired) {
            found.push(Device { model, hidraw });
        }
    }
    found.sort_by_key(|d| MODELS.iter().position(|m| std::ptr::eq(m, d.model)));
    found
}

/// Picks the model for a USB interface. `product` is the device's USB product
/// name and `paired` the Omni receiver's paired product IDs, if any.
fn select_model(pid: u16, interface: u8, product: &str, paired: &[u16]) -> Option<&'static Model> {
    let product = product.to_uppercase();
    MODELS
        .iter()
        .filter(|m| m.pid == pid && m.interface == interface)
        .find(|m| match m.matches {
            Match::Any => true,
            Match::Product(s) => product.contains(s),
            Match::OmniPaired(pids) => pids.iter().any(|p| paired.contains(p)),
        })
}

/// Parses `HID_ID=0003:00000B05:00001AD0` from the hidraw node's uevent.
fn hid_ids(sys: &Path) -> Option<(u16, u16)> {
    let uevent = fs::read_to_string(sys.join("device/uevent")).ok()?;
    let id = uevent.lines().find_map(|l| l.strip_prefix("HID_ID="))?;
    let mut parts = id.split(':').skip(1);
    let vid = u32::from_str_radix(parts.next()?, 16).ok()?;
    let pid = u32::from_str_radix(parts.next()?, 16).ok()?;
    Some((vid as u16, pid as u16))
}

/// The HID device's parent is the USB interface, which knows its number.
fn interface_number(sys: &Path) -> Option<u8> {
    let raw = fs::read_to_string(sys.join("device/../bInterfaceNumber")).ok()?;
    u8::from_str_radix(raw.trim(), 16).ok()
}

/// The USB device's product string, e.g. "ROG SPEEDNOVA 8K RECEIVER".
fn usb_product(sys: &Path) -> Option<String> {
    let raw = fs::read_to_string(sys.join("device/../../product")).ok()?;
    Some(raw.trim().to_owned())
}

/// Asks an Omni receiver which devices are paired with it, as G-Helper's
/// `DedectOmniMouse` does: send `01 A0` and read product IDs, little-endian,
/// every 4 bytes from byte 5 until a zero.
fn omni_paired(hidraw: &Path) -> Result<Vec<u16>, QueryError> {
    let mut file = open(hidraw)?;
    drain(&mut file);
    let mut packet = [0u8; 64];
    packet[..2].copy_from_slice(&[0x01, 0xa0]);
    file.write_all(&packet)?;
    let reply = read_packet(&mut file, 0x01, Instant::now() + REPLY_TIMEOUT, |r| {
        r[0] == 0x01
    })?;
    Ok(parse_omni_paired(&reply))
}

fn parse_omni_paired(reply: &[u8]) -> Vec<u16> {
    reply[5..]
        .chunks_exact(4)
        .map(|c| u16::from_le_bytes([c[0], c[1]]))
        .take_while(|&pid| pid != 0)
        .collect()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Reading {
    /// Percent. 0 means the mouse is asleep or out of range: the receiver
    /// still answers, but with no data.
    pub battery: u8,
    pub charging: bool,
    pub settings: Option<Settings>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Settings {
    pub power_off: PowerOff,
    pub low_battery_warning: u8,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PowerOff {
    Minutes(u8),
    Never,
    Unknown(u8),
}

impl PowerOff {
    fn from_byte(b: u8) -> Self {
        match b {
            0 => Self::Minutes(1),
            1 => Self::Minutes(2),
            2 => Self::Minutes(3),
            3 => Self::Minutes(5),
            4 => Self::Minutes(10),
            0xff => Self::Never,
            other => Self::Unknown(other),
        }
    }
}

#[derive(Debug)]
pub enum QueryError {
    Io(io::Error),
    Timeout,
    /// The mouse answered with an error (`FF AA`).
    Rejected,
    /// The receiver answered with an all-zero packet; the mouse is unreachable.
    Empty,
}

impl std::fmt::Display for QueryError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Io(e) => write!(f, "{e}"),
            Self::Timeout => write!(f, "no reply from device"),
            Self::Rejected => write!(f, "device rejected the battery query"),
            Self::Empty => write!(f, "device returned an empty reply"),
        }
    }
}

impl From<io::Error> for QueryError {
    fn from(e: io::Error) -> Self {
        Self::Io(e)
    }
}

impl Device {
    pub fn read_battery(&self) -> Result<Reading, QueryError> {
        let model = self.model;
        let mut file = open(&self.hidraw)?;
        drain(&mut file);

        let mut packet = [0u8; MAX_PACKET];
        packet[..3].copy_from_slice(&[model.report_id, 0x12, 0x07]);
        file.write_all(&packet[..model.packet_size])?;

        let deadline = Instant::now() + REPLY_TIMEOUT;
        // Other input reports can share the interface; skip until ours or an error.
        let reply = read_packet(&mut file, model.report_id, deadline, |r| {
            r[..3] == packet[..3] || (r[1] == 0xff && r[2] == 0xaa) || r[1..4] == [0, 0, 0]
        })?;
        parse_reply(model, &reply)
    }
}

/// Decodes a battery reply whose byte 0 is the report ID.
pub fn parse_reply(model: &Model, reply: &[u8]) -> Result<Reading, QueryError> {
    if reply[1] == 0xff && reply[2] == 0xaa {
        return Err(QueryError::Rejected);
    }
    if reply[1..4].iter().all(|&b| b == 0) {
        return Err(QueryError::Empty);
    }
    let battery = match model.battery {
        Battery::Percent => reply[5],
        Battery::Quarters { byte } => reply[byte].saturating_mul(25),
    };
    Ok(Reading {
        battery: battery.min(100),
        charging: reply[10] > 0,
        settings: model.settings.then(|| Settings {
            power_off: PowerOff::from_byte(reply[6]),
            low_battery_warning: reply[7],
        }),
    })
}

fn open(hidraw: &Path) -> io::Result<File> {
    OpenOptions::new()
        .read(true)
        .write(true)
        .custom_flags(libc::O_NONBLOCK)
        .open(hidraw)
}

/// Discards input queued before our request so we don't match a stale reply.
/// Stops after one kernel queue's worth, so a device that never stops sending
/// can't stall us; anything later is filtered out by `read_packet`.
fn drain(file: &mut File) {
    let mut buf = [0u8; MAX_PACKET];
    for _ in 0..HIDRAW_QUEUE {
        if !matches!(file.read(&mut buf), Ok(n) if n > 0) {
            break;
        }
    }
}

/// Reads input reports until `accept` matches one, normalized so byte 0 is
/// always the report ID: hidraw omits it for devices without numbered reports.
fn read_packet(
    file: &mut File,
    report_id: u8,
    deadline: Instant,
    accept: impl Fn(&[u8]) -> bool,
) -> Result<[u8; MAX_PACKET + 1], QueryError> {
    let mut buf = [0u8; MAX_PACKET + 1];
    let offset = usize::from(report_id == 0);
    loop {
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() || !poll_readable(file, remaining)? {
            return Err(QueryError::Timeout);
        }
        buf.fill(0);
        match file.read(&mut buf[offset..]) {
            Ok(n) if n + offset >= 11 && accept(&buf) => return Ok(buf),
            Ok(_) => continue,
            Err(e) if e.kind() == io::ErrorKind::WouldBlock => continue,
            Err(e) => return Err(e.into()),
        }
    }
}

fn poll_readable(file: &File, timeout: Duration) -> io::Result<bool> {
    let mut pfd = libc::pollfd {
        fd: file.as_raw_fd(),
        events: libc::POLLIN,
        revents: 0,
    };
    let ms = timeout.as_millis().min(i32::MAX as u128) as i32;
    // SAFETY: `pfd` is a valid pollfd for the duration of the call.
    let n = unsafe { libc::poll(&mut pfd, 1, ms) };
    match n {
        -1 => {
            let e = io::Error::last_os_error();
            if e.kind() == io::ErrorKind::Interrupted {
                Ok(false)
            } else {
                Err(e)
            }
        }
        0 => Ok(false),
        _ if pfd.revents & (libc::POLLERR | libc::POLLHUP | libc::POLLNVAL) != 0 => {
            Err(io::Error::from(io::ErrorKind::BrokenPipe))
        }
        _ => Ok(true),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn model(name: &str) -> &'static Model {
        MODELS.iter().find(|m| m.name == name).unwrap()
    }

    fn reply(bytes: &[u8]) -> [u8; MAX_PACKET + 1] {
        let mut buf = [0u8; MAX_PACKET + 1];
        buf[..bytes.len()].copy_from_slice(bytes);
        buf
    }

    /// Captured from a Harpe II Ace on the SpeedNova receiver.
    #[test]
    fn parses_percent_reply() {
        let r = reply(&[
            0x03, 0x12, 0x07, 0, 0, 0x50, 0x02, 0x14, 0xd8, 0x0f, 0, 0, 0x01,
        ]);
        let reading = parse_reply(model("ROG Harpe II Ace"), &r).unwrap();
        assert_eq!(reading.battery, 80);
        assert!(!reading.charging);
        let settings = reading.settings.unwrap();
        assert_eq!(settings.power_off, PowerOff::Minutes(3));
        assert_eq!(settings.low_battery_warning, 20);
    }

    #[test]
    fn parses_quarters_reply() {
        let r = reply(&[0x00, 0x12, 0x07, 0, 0, 0x03, 0, 0, 0, 0, 0x01]);
        let reading = parse_reply(model("ROG Chakram"), &r).unwrap();
        assert_eq!(reading.battery, 75);
        assert!(reading.charging);
        assert_eq!(reading.settings, None);
    }

    #[test]
    fn parses_quarters_from_byte_7() {
        let r = reply(&[0x00, 0x12, 0x07, 0, 0, 0x09, 0, 0x02]);
        assert_eq!(
            parse_reply(model("ROG Strix Carry"), &r).unwrap().battery,
            50
        );
    }

    #[test]
    fn clamps_out_of_range_quarters() {
        let r = reply(&[0x00, 0x12, 0x07, 0, 0, 0x09]);
        assert_eq!(parse_reply(model("ROG Chakram"), &r).unwrap().battery, 100);
    }

    #[test]
    fn decodes_power_off_never() {
        let r = reply(&[0x03, 0x12, 0x07, 0, 0, 0x50, 0xff, 0x14]);
        let settings = parse_reply(model("ROG Harpe II Ace"), &r).unwrap().settings;
        assert_eq!(settings.unwrap().power_off, PowerOff::Never);
    }

    #[test]
    fn tells_speednova_mice_apart_by_product_name() {
        let ace = select_model(0x1ad0, 2, "ROG SPEEDNOVA 8K RECEIVER", &[]).unwrap();
        assert_eq!(ace.name, "ROG Harpe II Ace");
        let extreme = select_model(0x1ad0, 2, "ROG Harpe II Extreme Receiver", &[]).unwrap();
        assert_eq!(extreme.name, "Harpe II Extreme Edition 20");
    }

    #[test]
    fn identifies_omni_mouse_by_paired_id() {
        // A keyboard (unknown ID) paired first, then a Harpe Ace Mini.
        let m = select_model(OMNI_PID, 2, "", &[0x1234, 0x1b65]).unwrap();
        assert_eq!(m.name, "Harpe Ace Mini");
        assert!(select_model(OMNI_PID, 2, "", &[]).is_none());
    }

    #[test]
    fn ignores_unsupported_interfaces() {
        assert!(select_model(0x1ad0, 0, "", &[]).is_none());
        assert!(select_model(0xffff, 0, "", &[]).is_none());
    }

    #[test]
    fn parses_omni_pairing_reply() {
        let mut r = [0u8; 64];
        r[..2].copy_from_slice(&[0x01, 0xa0]);
        r[5..7].copy_from_slice(&0x1b65u16.to_le_bytes());
        r[9..11].copy_from_slice(&0x1b1au16.to_le_bytes());
        assert_eq!(parse_omni_paired(&r), vec![0x1b65, 0x1b1a]);
    }

    #[test]
    fn rejects_error_and_empty_replies() {
        let m = model("ROG Harpe II Ace");
        assert!(matches!(
            parse_reply(m, &reply(&[0x03, 0xff, 0xaa])),
            Err(QueryError::Rejected)
        ));
        assert!(matches!(
            parse_reply(m, &reply(&[0x03])),
            Err(QueryError::Empty)
        ));
    }
}
