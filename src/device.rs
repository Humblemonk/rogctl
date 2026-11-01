//! Finding supported mice and keyboards over Linux hidraw, and talking to
//! them. Each
//! exchange writes one request and waits, with a deadline, for the reply that
//! echoes it. The bytes are in `protocol.rs`.

use std::fs::{self, File, OpenOptions};
use std::io::{self, Read, Write};
use std::os::fd::AsRawFd;
use std::os::unix::fs::OpenOptionsExt;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use crate::models::{self, OMNI_PID, OMNI_RECEIVER};
use crate::protocol::{
    self, Battery, Change, Features, MouseSettings, OMNI_REPORT_ID, Query, Reading, Request,
};

pub const ASUS_VID: u16 = 0x0b05;
const MAX_PACKET: usize = 65;
const REPLY_TIMEOUT: Duration = Duration::from_millis(500);
/// How long a write still waits for its echo after an `FF AA`. A Harpe II Ace
/// on the SpeedNova echoes about 1 ms after one, and answers a read in 5 ms,
/// or about 70 ms after sitting unused.
const ECHO_AFTER_REJECTION: Duration = Duration::from_millis(150);
/// Reports the kernel buffers per open hidraw file (`HIDRAW_BUFFER_SIZE`).
const HIDRAW_QUEUE: usize = 64;

/// A reply, normalized so byte 0 is the report ID.
type Reply = [u8; MAX_PACKET + 1];

/// What a device is. Keyboards are only read: their battery level.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Kind {
    Mouse,
    Keyboard,
}

/// One way a supported mouse or keyboard shows up on USB: a product ID and
/// the HID interface that answers vendor commands on it.
#[derive(Debug)]
pub struct Model {
    pub name: &'static str,
    pub kind: Kind,
    pub pid: u16,
    pub interface: u8,
    pub report_id: u8,
    /// Bytes written per command, including the report ID byte.
    pub packet_size: usize,
    /// How the device is connected: receiver (true) or cable (false).
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

/// A hidraw node whose product ID and interface are in `models::all()`.
struct Candidate {
    sys: PathBuf,
    hidraw: PathBuf,
    pid: u16,
    interface: u8,
}

/// Reads sysfs only; sends nothing.
fn candidates() -> Vec<Candidate> {
    let Ok(entries) = fs::read_dir("/sys/class/hidraw") else {
        return Vec::new();
    };
    let mut found: Vec<Candidate> = entries
        .flatten()
        .filter_map(|entry| {
            let sys = entry.path();
            let (_, pid) = hid_ids(&sys).filter(|&(vid, _)| vid == ASUS_VID)?;
            let interface = interface_number(&sys)?;
            let known = models::all().any(|m| m.pid == pid && m.interface == interface);
            known.then(|| Candidate {
                sys,
                hidraw: Path::new("/dev").join(entry.file_name()),
                pid,
                interface,
            })
        })
        .collect();
    found.sort_by(|a, b| a.hidraw.cmp(&b.hidraw));
    found
}

/// The nodes `discover` looks at, found from sysfs alone: cheap enough to
/// poll for devices being plugged in or out.
pub fn candidate_nodes() -> Vec<PathBuf> {
    candidates().into_iter().map(|c| c.hidraw).collect()
}

/// Finds connected supported mice and keyboards, in `models::all()` order:
/// every mouse before any keyboard. An Omni receiver can yield one of each,
/// on the same node.
pub fn discover() -> Vec<Device> {
    let mut found: Vec<Device> = Vec::new();
    for c in candidates() {
        for model in identify(&c) {
            found.push(Device {
                model,
                hidraw: c.hidraw.clone(),
            });
        }
    }
    found.sort_by_key(|d| rank(d.model));
    found
}

/// The connected mice alone: all `rogctl settings` may open.
pub fn discover_mice() -> Vec<Device> {
    let mut found = discover();
    found.retain(|d| d.model.kind == Kind::Mouse);
    found
}

/// Which models a candidate node holds, asking an Omni receiver what is
/// paired (read-only): at most one of each kind.
fn identify(c: &Candidate) -> Vec<&'static Model> {
    if c.pid == OMNI_PID {
        omni_models(omni_paired(&c.hidraw))
    } else {
        let product = usb_product(&c.sys).unwrap_or_default();
        select_models(c.pid, c.interface, &product, &[])
    }
}

/// What tells models apart: each entry's product ID, interface and name
/// together are unique (`entries_are_unambiguous`).
fn key(m: &Model) -> (u16, u8, &'static str) {
    (m.pid, m.interface, m.name)
}

/// Where `model` is in `models::all()`. OMNI_RECEIVER isn't there; it goes
/// last. Found by value (`key`): a `const` has no one address to compare.
fn rank(model: &Model) -> usize {
    models::all()
        .position(|m| key(m) == key(model))
        .unwrap_or(usize::MAX)
}

/// Picks the models for an Omni receiver from its pairing reply. If the
/// receiver couldn't be opened or written to, keep it so the battery read
/// reports why (e.g. no udev rule) instead of it looking unplugged. One that
/// didn't answer is left out, as one with nothing supported paired is: a
/// receiver answers within milliseconds even then (protocol.md).
fn omni_models(paired: Result<Vec<u16>, QueryError>) -> Vec<&'static Model> {
    match paired {
        Ok(paired) => select_models(OMNI_PID, OMNI_RECEIVER.interface, "", &paired),
        Err(QueryError::Io(_)) => vec![&OMNI_RECEIVER],
        Err(
            QueryError::Timeout
            | QueryError::Rejected
            | QueryError::Empty
            | QueryError::Unsupported,
        ) => Vec::new(),
    }
}

/// Picks the models for a USB interface: the first match of each kind, mouse
/// first. `product` is the device's USB product name and `paired` the Omni
/// receiver's paired product IDs, if any.
fn select_models(pid: u16, interface: u8, product: &str, paired: &[u16]) -> Vec<&'static Model> {
    let product = product.to_uppercase();
    let matching = || {
        models::all()
            .filter(|m| m.pid == pid && m.interface == interface)
            .filter(|m| match m.matches {
                Match::Any => true,
                Match::Product(s) => product.contains(s),
                Match::OmniPaired(pids) => pids.iter().any(|p| paired.contains(p)),
            })
    };
    [Kind::Mouse, Kind::Keyboard]
        .into_iter()
        .filter_map(|kind| matching().find(|m| m.kind == kind))
        .collect()
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
    let request = protocol::omni_paired_request();
    let mut file = open(hidraw)?;
    drain(&mut file);
    let mut packet = [0u8; 64];
    let bytes = request.bytes();
    packet[..bytes.len()].copy_from_slice(bytes);
    file.write_all(&packet)?;
    let deadline = Instant::now() + REPLY_TIMEOUT;
    // The echo already has report ID 1; a rejection or empty packet doesn't,
    // and another report on the interface could look empty.
    let reply = read_packet(&mut file, OMNI_REPORT_ID, deadline, |r| {
        r[0] == OMNI_REPORT_ID && request.answered_by(r)
    })?;
    protocol::parse_omni_paired(&reply)
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
    /// Whether the node still holds the model it was found as. Another mouse
    /// can take it over between reads without the node's path changing: a
    /// receiver swapped for one with the same product ID, or another mouse
    /// paired to the Omni receiver. Reads sysfs, and asks an Omni receiver
    /// which mouse is paired; sends nothing that changes the mouse.
    pub fn is_unchanged(&self) -> bool {
        candidates()
            .iter()
            .find(|c| c.hidraw == self.hidraw)
            .is_some_and(|c| identify(c).iter().any(|m| key(m) == key(self.model)))
    }

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
    fn send(&mut self, request: &Request) -> Result<(), QueryError> {
        drain(&mut self.file);
        let mut packet = [0u8; MAX_PACKET];
        let bytes = request.bytes();
        packet[..bytes.len()].copy_from_slice(bytes);
        self.file.write_all(&packet[..self.model.packet_size])?;
        Ok(())
    }

    fn exchange(&mut self, request: &Request) -> Result<Reply, QueryError> {
        self.send(request)?;
        let deadline = Instant::now() + REPLY_TIMEOUT;
        let reply = read_packet(&mut self.file, self.model.report_id, deadline, |r| {
            request.answered_by(r)
        })?;
        protocol::check_reply(&reply)?;
        Ok(reply)
    }

    /// Sends a change or a save. An `FF AA` doesn't end the wait for its
    /// echo: the Harpe II Ace can send one just before a save's (protocol.md),
    /// and another process's request can draw one too. Only if no echo comes
    /// within `ECHO_AFTER_REJECTION` does the rejection count.
    fn write(&mut self, request: &Request) -> Result<(), QueryError> {
        self.send(request)?;
        let mut deadline = Instant::now() + REPLY_TIMEOUT;
        let mut refused = None;
        loop {
            match read_packet(&mut self.file, self.model.report_id, deadline, |r| {
                request.answered_by(r)
            }) {
                Ok(reply) => match protocol::check_reply(&reply) {
                    Ok(()) => return Ok(()),
                    Err(e) => {
                        refused = Some(e);
                        deadline = deadline.min(Instant::now() + ECHO_AFTER_REJECTION);
                    }
                },
                Err(QueryError::Timeout) => return Err(refused.unwrap_or(QueryError::Timeout)),
                Err(e) => return Err(e),
            }
        }
    }

    pub fn read_battery(&mut self) -> Result<Reading, QueryError> {
        let model = self.model;
        let reply = self.exchange(&model.battery.request(model.report_id))?;
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
            let reply = match self.exchange(&query.request(model.report_id)) {
                Ok(reply) => reply,
                // Lighting is untested on most mice that have it
                // (protocol.md): one that won't read leaves its rows out,
                // not the other settings. G-Helper carries on likewise.
                Err(QueryError::Rejected | QueryError::Timeout | QueryError::Empty)
                    if query == Query::Lighting =>
                {
                    continue;
                }
                Err(e) => return Err(e),
            };
            protocol::parse_settings(query, &model.features, &reply, &mut settings);
        }
        Ok(settings)
    }

    /// The active DPI stage alone, which the mouse's DPI button changes.
    /// `None` on a mouse with one stage, or a reply that doesn't name one.
    pub fn read_active_stage(&mut self) -> Result<Option<usize>, QueryError> {
        let model = self.model;
        // As `settings_queries()` decides whether to ask.
        if model.features.dpi.stages < 2 {
            return Ok(None);
        }
        let reply = self.exchange(&Query::Profile.request(model.report_id))?;
        let mut settings = MouseSettings::default();
        protocol::parse_settings(Query::Profile, &model.features, &reply, &mut settings);
        Ok(settings.active_stage)
    }

    /// Changes one setting and saves it on the mouse. The only code that
    /// writes settings; only `tui::apply_action` calls it. Never writes to a
    /// keyboard.
    pub fn apply(&mut self, change: &Change) -> Result<(), ApplyError> {
        let model = self.model;
        match model.kind {
            Kind::Mouse => {}
            Kind::Keyboard => return Err(ApplyError::NotChanged(QueryError::Unsupported)),
        }
        let request = protocol::change_request(model.report_id, &model.features, change)
            .ok_or(ApplyError::NotChanged(QueryError::Unsupported))?;
        self.write(&request).map_err(ApplyError::NotChanged)?;
        // A Harpe II Ace left idle can reject a save with no echo, yet keep
        // the change (protocol.md); ask once more.
        let save = protocol::save_request(model.report_id);
        match self.write(&save) {
            Err(QueryError::Rejected) => self.write(&save),
            other => other,
        }
        .map_err(ApplyError::NotSaved)?;
        Ok(())
    }
}

/// Which step of [`Connection::apply`] failed.
#[derive(Debug)]
pub enum ApplyError {
    /// The mouse didn't take the change.
    NotChanged(QueryError),
    /// The mouse took the change, but the save failed, so it may be lost when
    /// the mouse turns off. The Harpe II Ace keeps changes it rejects saving.
    NotSaved(QueryError),
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

/// Reads input reports until `accept` matches one on `report_id`, normalized
/// so byte 0 is always the report ID: hidraw omits it for devices without
/// numbered reports. The report ID tells apart the devices sharing an Omni
/// receiver's node (mouse 3, keyboard 2): every open file gets a copy of each
/// report, and a rejection (`FF AA`) or empty reply carries no other sign of
/// which request it answers.
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
        if remaining.is_zero() {
            return Err(QueryError::Timeout);
        }
        match poll_readable(file, remaining) {
            Ok(true) => {}
            Ok(false) => return Err(QueryError::Timeout),
            // A signal (the TUI's resize, say) cut the wait short.
            Err(e) if e.kind() == io::ErrorKind::Interrupted => continue,
            Err(e) => return Err(e.into()),
        }
        buf.fill(0);
        match file.read(&mut buf[offset..]) {
            Ok(n) if n + offset >= 11 && buf[0] == report_id && accept(&buf) => return Ok(buf),
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
        -1 => Err(io::Error::last_os_error()),
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

    /// The first model `select_models` finds: the mouse, if there is one.
    fn one(pid: u16, interface: u8, product: &str, paired: &[u16]) -> Option<&'static Model> {
        select_models(pid, interface, product, paired)
            .first()
            .copied()
    }

    #[test]
    fn tells_speednova_mice_apart_by_product_name() {
        let ace = one(0x1ad0, 2, "ROG SPEEDNOVA 8K RECEIVER", &[]).unwrap();
        assert_eq!(ace.name, "ROG Harpe II Ace");
        let extreme = one(0x1ad0, 2, "ROG Harpe II Extreme Receiver", &[]).unwrap();
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
        let ace = one(0x1ad0, 2, "ROG SPEEDNOVA 8K RECEIVER", &[]).unwrap();
        let extreme = one(0x1ad0, 2, "EXTREME", &[]).unwrap();
        assert_eq!(parse(extreme), parse(ace));
    }

    /// `discover` lists the receiver before the cable, as `MODELS` does.
    #[test]
    fn devices_sort_in_models_order() {
        let receiver = one(0x1ad0, 2, "ROG SPEEDNOVA 8K RECEIVER", &[]).unwrap();
        let cable = one(0x1c69, 0, "", &[]).unwrap();
        assert!(rank(receiver) < rank(cable));
        assert!(rank(cable) < rank(&OMNI_RECEIVER));
    }

    /// `Device::is_unchanged` tells apart the models that can take over a node
    /// without its path changing.
    #[test]
    fn tells_apart_models_sharing_a_node() {
        let ace = one(0x1ad0, 2, "ROG SPEEDNOVA 8K RECEIVER", &[]).unwrap();
        let extreme = one(0x1ad0, 2, "EXTREME", &[]).unwrap();
        assert_ne!(key(ace), key(extreme));
        let mini = one(OMNI_PID, 2, "", &[0x1b65]).unwrap();
        let keris = one(OMNI_PID, 2, "", &[0x1b1a]).unwrap();
        assert_ne!(key(mini), key(keris));
        assert_eq!(key(ace), key(one(0x1ad0, 2, "SPEEDNOVA", &[]).unwrap()));
    }

    #[test]
    fn identifies_omni_mouse_by_paired_id() {
        // A keyboard (unknown ID) paired first, then a Harpe Ace Mini.
        let m = one(OMNI_PID, 2, "", &[0x1234, 0x1b65]).unwrap();
        assert_eq!(m.name, "Harpe Ace Mini");
        assert!(one(OMNI_PID, 2, "", &[]).is_none());
    }

    #[test]
    fn keeps_omni_receiver_it_cannot_query() {
        let denied = io::Error::from(io::ErrorKind::PermissionDenied);
        let found = omni_models(Err(QueryError::Io(denied)));
        assert_eq!(found.len(), 1);
        let m = found[0];
        assert!(std::ptr::eq(m, &OMNI_RECEIVER));
        // Battery only: nothing to change on a mouse it couldn't identify.
        let f = m.features;
        assert_eq!(f.dpi.stages, 0);
        assert_eq!((f.power_off, f.low_battery_warning), (None, None));
        // No answer, or nothing supported paired: nothing to report.
        for e in [QueryError::Timeout, QueryError::Rejected, QueryError::Empty] {
            assert!(omni_models(Err(e)).is_empty());
        }
        assert!(omni_models(Ok(vec![0x1234])).is_empty());
        let names = |paired: Vec<u16>| -> Vec<&str> {
            omni_models(Ok(paired)).iter().map(|m| m.name).collect()
        };
        assert_eq!(names(vec![0x1b65]), ["Harpe Ace Mini"]);
    }

    /// Captured from a Falchion RX Low Profile, the only device paired with
    /// its Omni receiver: `01 a0 00 01 00 06 1b 02 04`.
    #[test]
    fn finds_omni_keyboard() {
        let found = omni_models(Ok(vec![0x1b06]));
        assert_eq!(found.len(), 1);
        let kb = found[0];
        assert_eq!(kb.name, "ROG Falchion RX Low Profile");
        assert_eq!(kb.kind, Kind::Keyboard);
        assert_eq!((kb.interface, kb.report_id, kb.packet_size), (2, 0x02, 64));
    }

    /// A mouse and a keyboard paired with one receiver share its node, on
    /// report IDs 3 and 2; the mouse comes first whatever the slot order.
    #[test]
    fn finds_omni_mouse_and_keyboard() {
        let found = omni_models(Ok(vec![0x1b06, 0x1b65]));
        let found: Vec<(&str, Kind, u8)> = found
            .iter()
            .map(|m| (m.name, m.kind, m.report_id))
            .collect();
        assert_eq!(
            found,
            [
                ("Harpe Ace Mini", Kind::Mouse, 0x03),
                ("ROG Falchion RX Low Profile", Kind::Keyboard, 0x02),
            ]
        );
    }

    /// In wired mode the Falchion RX Low Profile answers on interface 1 of
    /// `0b05:1b04` (captured), the only one on the vendor page `FF00`.
    #[test]
    fn finds_wired_keyboard() {
        let found = select_models(0x1b04, 1, "ROG FALCHION RX LOW PROFILE", &[]);
        let names: Vec<&str> = found.iter().map(|m| m.name).collect();
        assert_eq!(names, ["ROG Falchion RX Low Profile"]);
        assert!(!found[0].wireless);
        assert!(select_models(0x1b04, 0, "", &[]).is_empty());
    }

    /// Every mouse ranks before every keyboard, so a mouse is reported first.
    #[test]
    fn mice_rank_before_keyboards() {
        let last_mouse = models::all()
            .filter(|m| m.kind == Kind::Mouse)
            .map(rank)
            .max()
            .unwrap();
        assert!(
            models::all()
                .filter(|m| m.kind == Kind::Keyboard)
                .all(|m| rank(m) > last_mouse)
        );
    }

    /// The last guard on the write path: nothing reaches a keyboard, even if
    /// a change were asked for.
    #[test]
    fn never_writes_to_a_keyboard() {
        use std::os::unix::net::UnixDatagram;

        let (keyboard, ours) = UnixDatagram::pair().unwrap();
        keyboard.set_nonblocking(true).unwrap();
        let mut conn = Connection {
            model: one(0x1b04, 1, "", &[]).unwrap(),
            file: File::from(std::os::fd::OwnedFd::from(ours)),
        };
        let result = conn.apply(&Change::ActiveStage(0));
        assert!(matches!(
            result,
            Err(ApplyError::NotChanged(QueryError::Unsupported))
        ));
        let mut buf = [0u8; 128];
        assert!(keyboard.recv(&mut buf).is_err(), "something was sent");
    }

    #[test]
    fn ignores_unsupported_interfaces() {
        assert!(one(0x1ad0, 0, "", &[]).is_none());
        assert!(one(0xffff, 0, "", &[]).is_none());
    }

    /// A datagram socket keeps packet boundaries, like hidraw. Most mice use
    /// report ID 0, which hidraw leaves out of the replies.
    #[test]
    fn read_packet_skips_other_reports_and_never_blocks() {
        use std::os::unix::net::UnixDatagram;

        let (mouse, ours) = UnixDatagram::pair().unwrap();
        ours.set_nonblocking(true).unwrap();
        let mut file = File::from(std::os::fd::OwnedFd::from(ours));
        let request = Battery::Percent.request(0);
        let read = |file: &mut File| {
            let deadline = Instant::now() + Duration::from_millis(50);
            read_packet(file, 0, deadline, |r| request.answered_by(r))
        };

        let mut stray = [0u8; 64];
        stray[..3].copy_from_slice(&[0x12, 0x04, 0x01]);
        let mut battery = [0u8; 64];
        battery[..5].copy_from_slice(&[0x12, 0x07, 0, 0, 0x50]);
        mouse.send(&stray).unwrap();
        mouse.send(&[0x12, 0x07]).unwrap(); // too short to be a reply
        mouse.send(&battery).unwrap();
        let reply = read(&mut file).unwrap();
        assert_eq!(reply[..6], [0, 0x12, 0x07, 0, 0, 0x50]);

        let start = Instant::now();
        assert!(matches!(read(&mut file), Err(QueryError::Timeout)));
        assert!(start.elapsed() < Duration::from_secs(1));
    }

    /// On an Omni receiver's node, another process reading the keyboard (report
    /// 2) can draw its rejection, captured as `02 ff aa` with the keyboard off,
    /// while a mouse read (report 3) waits: it must not count as the mouse's.
    #[test]
    fn read_packet_ignores_another_report_ids_rejection() {
        use std::os::unix::net::UnixDatagram;

        let (receiver, ours) = UnixDatagram::pair().unwrap();
        ours.set_nonblocking(true).unwrap();
        let mut file = File::from(std::os::fd::OwnedFd::from(ours));
        let request = Battery::Percent.request(0x03);
        let mut keyboard_off = [0u8; 64];
        keyboard_off[..3].copy_from_slice(&[0x02, 0xff, 0xaa]);
        let mut mouse_empty = [0u8; 64];
        mouse_empty[0] = 0x02;
        let mut battery = [0u8; 64];
        battery[..6].copy_from_slice(&[0x03, 0x12, 0x07, 0, 0, 0x50]);
        receiver.send(&keyboard_off).unwrap();
        receiver.send(&mouse_empty).unwrap();
        receiver.send(&battery).unwrap();
        let deadline = Instant::now() + Duration::from_millis(50);
        let reply = read_packet(&mut file, 0x03, deadline, |r| request.answered_by(r)).unwrap();
        assert_eq!(reply[..6], [0x03, 0x12, 0x07, 0, 0, 0x50]);

        // Its own report ID's rejection still counts.
        let mut rejected = [0u8; 64];
        rejected[..3].copy_from_slice(&[0x03, 0xff, 0xaa]);
        receiver.send(&rejected).unwrap();
        let deadline = Instant::now() + Duration::from_millis(50);
        let reply = read_packet(&mut file, 0x03, deadline, |r| request.answered_by(r)).unwrap();
        assert!(matches!(
            protocol::check_reply(&reply),
            Err(QueryError::Rejected)
        ));
    }

    /// A signal during the wait, as crossterm's resize handler sends in the
    /// TUI, isn't a timeout: the reply that follows it is still read.
    #[test]
    fn read_packet_waits_through_a_signal() {
        use std::os::unix::net::UnixDatagram;

        extern "C" fn ignore(_: libc::c_int) {}
        // SAFETY: an empty handler for SIGUSR2, which nothing else uses.
        unsafe { libc::signal(libc::SIGUSR2, ignore as *const () as libc::sighandler_t) };

        let (mouse, ours) = UnixDatagram::pair().unwrap();
        ours.set_nonblocking(true).unwrap();
        let mut file = File::from(std::os::fd::OwnedFd::from(ours));
        // SAFETY: returns this thread's own handle.
        let reader = unsafe { libc::pthread_self() };
        let fake = std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(50));
            // SAFETY: the reader thread is alive, blocked in read_packet.
            unsafe { libc::pthread_kill(reader, libc::SIGUSR2) };
            std::thread::sleep(Duration::from_millis(50));
            let mut battery = [0u8; 64];
            battery[..5].copy_from_slice(&[0x12, 0x07, 0, 0, 0x50]);
            mouse.send(&battery).unwrap();
            mouse
        });
        let request = Battery::Percent.request(0);
        let deadline = Instant::now() + Duration::from_secs(2);
        let reply = read_packet(&mut file, 0, deadline, |r| request.answered_by(r));
        fake.join().unwrap();
        assert_eq!(reply.unwrap()[..6], [0, 0x12, 0x07, 0, 0, 0x50]);
    }

    /// What a fake mouse sends back for a packet it gets.
    enum Answer {
        Echo,
        Rejected,
    }

    /// Runs `act` against a fake SpeedNova mouse that answers the n-th packet
    /// it gets with `answers[n]`, then stays silent. Returns what `act` did
    /// and the packets the mouse got.
    fn on_fake<T>(
        answers: Vec<Vec<Answer>>,
        act: impl FnOnce(&mut Connection) -> T,
    ) -> (T, Vec<Vec<u8>>) {
        use std::os::unix::net::UnixDatagram;

        let (mouse, ours) = UnixDatagram::pair().unwrap();
        ours.set_nonblocking(true).unwrap();
        mouse
            .set_read_timeout(Some(Duration::from_secs(2)))
            .unwrap();
        let model = one(0x1ad0, 2, "ROG SPEEDNOVA 8K RECEIVER", &[]).unwrap();
        let mut conn = Connection {
            model,
            file: File::from(std::os::fd::OwnedFd::from(ours)),
        };
        let fake = std::thread::spawn(move || {
            let mut got = Vec::new();
            let mut packet = [0u8; MAX_PACKET];
            for answer in answers {
                let Ok(n) = mouse.recv(&mut packet) else {
                    break;
                };
                got.push(packet[..n].to_vec());
                for reply in answer {
                    let reply = match reply {
                        Answer::Echo => packet[..n].to_vec(),
                        Answer::Rejected => {
                            let mut r = vec![0u8; n];
                            r[..3].copy_from_slice(&[0x03, 0xff, 0xaa]);
                            r
                        }
                    };
                    mouse.send(&reply).unwrap();
                }
            }
            // Kept open until now, so a read times out rather than fails.
            (mouse, got)
        });
        let result = act(&mut conn);
        let (_, got) = fake.join().unwrap();
        (result, got)
    }

    fn apply_to_fake(answers: Vec<Vec<Answer>>) -> (Result<(), ApplyError>, Vec<Vec<u8>>) {
        on_fake(answers, |conn| conn.apply(&Change::MotionSync(true)))
    }

    const SAVE: &[u8] = &[0x03, 0x50, 0x03];

    /// A mouse that echoes the change but never answers the save: the change
    /// went through, so the TUI mustn't say none was made.
    #[test]
    fn apply_reports_a_change_that_was_not_saved() {
        let (result, got) = apply_to_fake(vec![vec![Answer::Echo], vec![]]);
        assert!(
            matches!(result, Err(ApplyError::NotSaved(QueryError::Timeout))),
            "{result:?}"
        );
        assert_eq!(got.len(), 2, "no second save for a timeout");
    }

    /// Captured from a Harpe II Ace left idle: `FF AA`, then the save's echo
    /// 1 ms later. That's a save, sent once.
    #[test]
    fn apply_waits_past_a_rejection_for_the_echo() {
        let (result, got) = apply_to_fake(vec![
            vec![Answer::Echo],
            vec![Answer::Rejected, Answer::Echo],
        ]);
        assert!(result.is_ok(), "{result:?}");
        assert_eq!(got.len(), 2);
    }

    /// Another process on the node can draw an `FF AA` just before the
    /// change's echo; the change was made, so it's saved and reported so.
    #[test]
    fn a_stray_rejection_before_the_change_echo_is_no_failure() {
        let (result, got) = apply_to_fake(vec![
            vec![Answer::Rejected, Answer::Echo],
            vec![Answer::Echo],
        ]);
        assert!(result.is_ok(), "{result:?}");
        assert_eq!(got.len(), 2, "the change, then one save");
        assert!(got[1].starts_with(SAVE));
    }

    /// A mouse that won't read its lighting still shows its other settings;
    /// the lighting rows are left out.
    #[test]
    fn a_failed_lighting_read_keeps_the_other_settings() {
        let model = one(0x1ad0, 2, "ROG SPEEDNOVA 8K RECEIVER", &[]).unwrap();
        let queries = protocol::settings_queries(&model.features);
        assert_eq!(queries.last(), Some(&Query::Lighting));
        let mut answers: Vec<Vec<Answer>> = queries.iter().map(|_| vec![Answer::Echo]).collect();
        answers.pop();
        answers.push(vec![Answer::Rejected]);
        let reading = Reading {
            battery: 80,
            charging: false,
            power_off: None,
            low_battery_warning: None,
        };
        let (settings, got) = on_fake(answers, |conn| conn.read_settings(&reading));
        let settings = settings.unwrap();
        assert_eq!(got.len(), queries.len());
        assert!(settings.polling_rate.is_some());
        assert_eq!(settings.lighting, None);
    }

    /// After an `FF AA` with no echo, the save is asked again well before
    /// the full reply timeout, so the screen barely pauses.
    #[test]
    fn a_rejection_with_no_echo_is_not_waited_out() {
        let start = Instant::now();
        let (result, _) = apply_to_fake(vec![
            vec![Answer::Echo],
            vec![Answer::Rejected],
            vec![Answer::Echo],
        ]);
        assert!(result.is_ok(), "{result:?}");
        assert!(start.elapsed() < REPLY_TIMEOUT, "{:?}", start.elapsed());
    }

    /// A rejection with no echo is asked once more; rejected again, it's
    /// reported.
    #[test]
    fn apply_asks_once_more_after_a_rejection() {
        let (result, got) = apply_to_fake(vec![
            vec![Answer::Echo],
            vec![Answer::Rejected],
            vec![Answer::Echo],
        ]);
        assert!(result.is_ok(), "{result:?}");
        assert_eq!(got.len(), 3);
        assert!(got[1].starts_with(SAVE) && got[2].starts_with(SAVE));

        let (result, got) = apply_to_fake(vec![
            vec![Answer::Echo],
            vec![Answer::Rejected],
            vec![Answer::Rejected],
            vec![],
        ]);
        assert!(
            matches!(result, Err(ApplyError::NotSaved(QueryError::Rejected))),
            "{result:?}"
        );
        assert_eq!(got.len(), 3, "asked once more, not again");
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
