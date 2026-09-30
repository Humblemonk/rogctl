//! Finding supported mice over Linux hidraw, and talking to them. Each
//! exchange writes one request and waits, with a deadline, for the reply that
//! echoes it. The bytes are in `protocol.rs`.

use std::fs::{self, File, OpenOptions};
use std::io::{self, Read, Write};
use std::os::fd::AsRawFd;
use std::os::unix::fs::OpenOptionsExt;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use crate::models::{MODELS, OMNI_PID, OMNI_RECEIVER};
use crate::protocol::{
    self, Battery, Change, Features, MouseSettings, OMNI_PAIRED, Query, Reading, Request,
};

pub const ASUS_VID: u16 = 0x0b05;
const MAX_PACKET: usize = 65;
const REPLY_TIMEOUT: Duration = Duration::from_millis(500);
/// Reports the kernel buffers per open hidraw file (`HIDRAW_BUFFER_SIZE`).
const HIDRAW_QUEUE: usize = 64;

/// A reply, normalized so byte 0 is the report ID.
type Reply = [u8; MAX_PACKET + 1];

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
    pub features: Features,
    pub matches: Match,
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

#[derive(Debug, Clone)]
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
        let model = if pid == OMNI_PID {
            omni_model(omni_paired(&hidraw))
        } else {
            select_model(pid, interface, &usb_product(&sys).unwrap_or_default(), &[])
        };
        if let Some(model) = model {
            found.push(Device { model, hidraw });
        }
    }
    // OMNI_RECEIVER isn't in MODELS; it goes last.
    found.sort_by_key(|d| {
        MODELS
            .iter()
            .position(|m| std::ptr::eq(m, d.model))
            .unwrap_or(usize::MAX)
    });
    found
}

/// Picks the model for an Omni receiver from its pairing reply. If the
/// receiver couldn't be opened or written to, keep it so the battery read
/// reports why (e.g. no udev rule) instead of it looking unplugged.
fn omni_model(paired: Result<Vec<u16>, QueryError>) -> Option<&'static Model> {
    match paired {
        Ok(paired) => select_model(OMNI_PID, OMNI_RECEIVER.interface, "", &paired),
        Err(QueryError::Io(_)) => Some(&OMNI_RECEIVER),
        Err(
            QueryError::Timeout
            | QueryError::Rejected
            | QueryError::Empty
            | QueryError::Unsupported,
        ) => None,
    }
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
/// `DedectOmniMouse` does.
fn omni_paired(hidraw: &Path) -> Result<Vec<u16>, QueryError> {
    let mut file = open(hidraw)?;
    drain(&mut file);
    let mut packet = [0u8; 64];
    packet[..OMNI_PAIRED.len()].copy_from_slice(&OMNI_PAIRED);
    file.write_all(&packet)?;
    let report_id = OMNI_PAIRED[0];
    let reply = read_packet(&mut file, report_id, Instant::now() + REPLY_TIMEOUT, |r| {
        r[0] == report_id
    })?;
    Ok(protocol::parse_omni_paired(&reply))
}

#[derive(Debug)]
pub enum QueryError {
    Io(io::Error),
    Timeout,
    /// The mouse answered with an error (`FF AA`).
    Rejected,
    /// The receiver answered with an all-zero packet; the mouse is unreachable.
    Empty,
    /// The model doesn't have the setting a change was for.
    Unsupported,
}

impl QueryError {
    /// Whether this means the mouse is asleep or out of range rather than
    /// broken. A receiver answers for a sleeping mouse with an empty reply, no
    /// reply, or a rejection (`FF AA`); over a cable a rejection is a real error.
    pub fn means_asleep(&self, wireless: bool) -> bool {
        match self {
            Self::Empty | Self::Timeout => true,
            Self::Rejected => wireless,
            Self::Io(_) | Self::Unsupported => false,
        }
    }
}

impl std::fmt::Display for QueryError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Io(e) => write!(f, "{e}"),
            Self::Timeout => write!(f, "no reply from device"),
            Self::Rejected => write!(f, "device rejected the request"),
            Self::Empty => write!(f, "device returned an empty reply"),
            Self::Unsupported => write!(f, "this mouse doesn't have that setting"),
        }
    }
}

impl From<io::Error> for QueryError {
    fn from(e: io::Error) -> Self {
        Self::Io(e)
    }
}

impl Device {
    pub fn connect(&self) -> io::Result<Connection> {
        Ok(Connection {
            model: self.model,
            file: open(&self.hidraw)?,
        })
    }

    pub fn read_battery(&self) -> Result<Reading, QueryError> {
        self.connect()?.read_battery()
    }
}

/// An open hidraw node, for several exchanges in a row.
pub struct Connection {
    model: &'static Model,
    file: File,
}

impl Connection {
    fn exchange(&mut self, request: &Request) -> Result<Reply, QueryError> {
        let model = self.model;
        drain(&mut self.file);

        let mut packet = [0u8; MAX_PACKET];
        let bytes = request.bytes();
        packet[..bytes.len()].copy_from_slice(bytes);
        self.file.write_all(&packet[..model.packet_size])?;

        let deadline = Instant::now() + REPLY_TIMEOUT;
        let reply = read_packet(&mut self.file, model.report_id, deadline, |r| {
            request.answered_by(r)
        })?;
        protocol::check_reply(&reply)?;
        Ok(reply)
    }

    pub fn read_battery(&mut self) -> Result<Reading, QueryError> {
        let model = self.model;
        let reply = self.exchange(&Query::Battery.request(model.report_id))?;
        protocol::parse_battery(model.battery, &model.features, &reply)
    }

    /// Reads every setting the model has. `reading` supplies the ones that
    /// come with the battery level.
    pub fn read_settings(&mut self, reading: &Reading) -> Result<MouseSettings, QueryError> {
        let model = self.model;
        let mut settings = MouseSettings {
            power_off: reading.power_off,
            low_battery_warning: reading.low_battery_warning,
            ..MouseSettings::default()
        };
        for query in protocol::settings_queries(&model.features) {
            let reply = self.exchange(&query.request(model.report_id))?;
            protocol::parse_settings(query, &model.features, &reply, &mut settings);
        }
        Ok(settings)
    }

    /// Changes one setting and saves it on the mouse. The only code that
    /// writes settings; only `tui::apply_action` calls it.
    pub fn apply(&mut self, change: &Change) -> Result<(), QueryError> {
        let model = self.model;
        let request = protocol::change_request(model.report_id, &model.features, change)
            .ok_or(QueryError::Unsupported)?;
        self.exchange(&request)?;
        self.exchange(&protocol::save_request(model.report_id))?;
        Ok(())
    }
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
) -> Result<Reply, QueryError> {
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

    #[test]
    fn tells_speednova_mice_apart_by_product_name() {
        let ace = select_model(0x1ad0, 2, "ROG SPEEDNOVA 8K RECEIVER", &[]).unwrap();
        assert_eq!(ace.name, "ROG Harpe II Ace");
        let extreme = select_model(0x1ad0, 2, "ROG Harpe II Extreme Receiver", &[]).unwrap();
        assert_eq!(extreme.name, "Harpe II Extreme Edition 20");
    }

    /// G-Helper's Extreme subclasses the Ace without changing the battery
    /// format, so byte 7 is the warning level here, not the battery.
    #[test]
    fn parses_extreme_like_ace() {
        let mut r = [0u8; MAX_PACKET + 1];
        r[..13].copy_from_slice(&[
            0x03, 0x12, 0x07, 0, 0, 0x50, 0x02, 0x14, 0xd8, 0x0f, 0, 0, 0x01,
        ]);
        let parse = |m: &Model| protocol::parse_battery(m.battery, &m.features, &r).unwrap();
        let ace = select_model(0x1ad0, 2, "ROG SPEEDNOVA 8K RECEIVER", &[]).unwrap();
        let extreme = select_model(0x1ad0, 2, "EXTREME", &[]).unwrap();
        assert_eq!(parse(extreme), parse(ace));
    }

    #[test]
    fn identifies_omni_mouse_by_paired_id() {
        // A keyboard (unknown ID) paired first, then a Harpe Ace Mini.
        let m = select_model(OMNI_PID, 2, "", &[0x1234, 0x1b65]).unwrap();
        assert_eq!(m.name, "Harpe Ace Mini");
        assert!(select_model(OMNI_PID, 2, "", &[]).is_none());
    }

    #[test]
    fn keeps_omni_receiver_it_cannot_query() {
        let denied = io::Error::from(io::ErrorKind::PermissionDenied);
        let m = omni_model(Err(QueryError::Io(denied))).unwrap();
        assert!(std::ptr::eq(m, &OMNI_RECEIVER));
        // No answer or no supported mouse paired: nothing to report.
        assert!(omni_model(Err(QueryError::Timeout)).is_none());
        assert!(omni_model(Ok(vec![0x1234])).is_none());
        assert_eq!(omni_model(Ok(vec![0x1b65])).unwrap().name, "Harpe Ace Mini");
    }

    #[test]
    fn ignores_unsupported_interfaces() {
        assert!(select_model(0x1ad0, 0, "", &[]).is_none());
        assert!(select_model(0xffff, 0, "", &[]).is_none());
    }

    #[test]
    fn receiver_rejection_means_asleep() {
        assert!(QueryError::Rejected.means_asleep(true));
        assert!(!QueryError::Rejected.means_asleep(false));
        assert!(QueryError::Empty.means_asleep(false));
        assert!(QueryError::Timeout.means_asleep(true));
        let io = io::Error::from(io::ErrorKind::BrokenPipe);
        assert!(!QueryError::Io(io).means_asleep(true));
    }
}
