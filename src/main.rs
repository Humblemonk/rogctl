// Defined before the modules so they can use them too. Output that can't be
// written is dropped and the command carries on: `set` and `import` never stop
// between sending a change and reading it back. Where the output is the data
// (`battery`, `settings get`, `export`), the command checks its own writes.

/// `println!`, minus the panic when stdout is closed, as in
/// `rogctl configure waybar | head`.
macro_rules! say {
    ($($arg:tt)*) => {{
        use ::std::io::Write as _;
        let _ = writeln!(::std::io::stdout(), $($arg)*);
    }};
}

/// `eprintln!`, minus the panic when stderr is a closed pipe, as in
/// `rogctl bogus 2>&1 | true`.
macro_rules! complain {
    ($($arg:tt)*) => {{
        use ::std::io::Write as _;
        let _ = writeln!(::std::io::stderr(), $($arg)*);
    }};
}

mod app;
mod configure;
mod device;
mod models;
mod protocol;
mod settings;
mod tui;
mod ui;
mod waybar;

use std::collections::HashMap;
use std::ffi::OsString;
use std::fs::{self, OpenOptions};
use std::io::{self, Write};
use std::os::fd::{AsRawFd, FromRawFd, OwnedFd, RawFd};
use std::os::unix::ffi::OsStrExt;
use std::os::unix::fs::OpenOptionsExt;
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::sync::OnceLock;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

use device::{Device, Kind};
use protocol::{PowerOff, Warning};

/// Prints the usage, minus the panic when stdout is closed, as in
/// `rogctl --help | head -c1`.
fn print_usage() -> ExitCode {
    let _ = io::stdout().write_all(USAGE.as_bytes());
    ExitCode::SUCCESS
}

const USAGE: &str = "\
rogctl - battery levels for ASUS mice and keyboards, and settings for the mice

Examples:
  rogctl                                 Show the mouse's battery level (a
                                         keyboard's if there's no mouse)
  rogctl --device all                    Show every mouse's and keyboard's level
  rogctl settings                        View and change DPI, polling rate and more
  rogctl settings --demo                 Try the settings screen without a mouse
  rogctl settings set polling-rate=1000  Change a setting without the screen
  rogctl configure kde                   Add the battery widget to KDE Plasma

Usage:
  rogctl [battery] [OPTIONS]             Print the battery status once
  rogctl watch [--interval SECS] [OPTIONS]
                                         Print the status every SECS seconds (default 60)
  rogctl list                            List detected mice and keyboards and their
                                         hidraw nodes
  rogctl refresh PID                     Make the `rogctl watch` with this PID read now
  rogctl stop PID                        Stop the `rogctl watch` with this PID
  rogctl settings [--demo [MODEL]]       View and change the mouse's settings (DPI,
                                         polling rate, ...). --demo runs without a
                                         mouse, as MODEL (default ROG Harpe II Ace)
  rogctl settings get [--json]           Print the mouse's settings
  rogctl settings set KEY=VALUE... [--dry-run]
                                         Change settings; `get` lists the keys
  rogctl settings export [FILE]          Save the settings as JSON (stdout without FILE)
  rogctl settings import FILE [--dry-run]
                                         Change the settings to FILE's (- for stdin)
  rogctl configure [DESKTOP] [--remove] [--force]
                                         Install the panel widget for DESKTOP (noctalia,
                                         waybar, quickshell, dms, kde or gnome), or
                                         remove it. Without DESKTOP, lists them

Options:
  --device DEVICE        mouse, keyboard, or all (one status each). Without it:
                         the mouse, or a keyboard if there's no mouse
  --json                 Same as --format json
  --format FORMAT        text (default), json, or waybar (a Waybar custom module)
  --low-threshold PCT    Level at or below which the battery counts as low
                         (default 20, 0 turns it off)

Exit status of `battery` and `settings get|set|export|import`: 0 connected
(or done), 1 error, 2 no device, 3 asleep. With
--device all, the best of the devices': connected, then asleep, then error.
`watch` reads the devices right away when sent SIGUSR1, which `refresh` sends.
`refresh` and `stop` signal only a `rogctl watch`; widgets use them with the
`pid` from its output.
";

/// What the bar sees. `battery` stays at the last known level while the
/// device sleeps, since the receiver stops reporting it.
#[derive(Debug, Clone, PartialEq, Serialize)]
struct Status {
    state: State,
    #[serde(skip_serializing_if = "Option::is_none")]
    device: Option<&'static str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    kind: Option<Kind>,
    #[serde(skip_serializing_if = "Option::is_none")]
    wireless: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    battery: Option<u8>,
    charging: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    low_battery_warning: Option<u8>,
    #[serde(skip_serializing_if = "Option::is_none")]
    power_off_minutes: Option<u8>,
    #[serde(skip_serializing_if = "Option::is_none")]
    hidraw: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<String>,
    /// Unix seconds when `battery` was last read from the device.
    #[serde(skip_serializing_if = "Option::is_none")]
    battery_updated: Option<u64>,
    /// At or below `--low-threshold` and not charging.
    #[serde(skip_serializing_if = "is_false")]
    low: bool,
    /// Set by `watch` on the one line where a low-battery notification is due.
    #[serde(skip_serializing_if = "is_false")]
    notify_low: bool,
    /// The `watch` process, for `rogctl refresh` and `rogctl stop`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pid: Option<u32>,
    /// The smallest change in `battery` the device reports: 25 for mice that
    /// report quarters, else 1.
    #[serde(skip)]
    battery_step: u8,
}

// serde's skip_serializing_if passes a reference.
fn is_false(b: &bool) -> bool {
    !b
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
enum State {
    Connected,
    Asleep,
    Disconnected,
    Error,
}

impl Status {
    fn empty(state: State) -> Self {
        Self {
            state,
            device: None,
            kind: None,
            wireless: None,
            battery: None,
            charging: false,
            low_battery_warning: None,
            power_off_minutes: None,
            hidraw: None,
            error: None,
            battery_updated: None,
            low: false,
            notify_low: false,
            pid: None,
            battery_step: 1,
        }
    }

    fn for_device(state: State, dev: &Device) -> Self {
        Self {
            device: Some(dev.model.name),
            kind: Some(dev.model.kind),
            wireless: Some(dev.model.wireless),
            hidraw: Some(dev.hidraw.display().to_string()),
            battery_step: dev.model.battery.step(),
            ..Self::empty(state)
        }
    }
}

/// Which devices `battery` and `watch` report: `--device`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Selection {
    /// One device: a mouse, or a keyboard if no mouse is plugged in.
    First,
    Mouse,
    Keyboard,
    /// Every device, a status each.
    All,
}

impl Selection {
    fn parse(name: &str) -> Option<Self> {
        match name {
            "mouse" => Some(Self::Mouse),
            "keyboard" => Some(Self::Keyboard),
            "all" => Some(Self::All),
            _ => None,
        }
    }

    fn nothing_found(self) -> &'static str {
        match self {
            Self::First | Self::All => "No supported mouse or keyboard found",
            Self::Mouse => "No supported mouse found",
            Self::Keyboard => "No supported keyboard found",
        }
    }
}

/// Splits `devices` (in `discover` order) into the groups to report, one
/// status each. A group is the ways in to one device, or for `First`, `Mouse`
/// and `Keyboard` to any device of that kind: the first that answers is shown.
fn groups(selection: Selection, devices: &[Device]) -> Vec<Vec<&Device>> {
    let of_kind =
        |kind: Kind| -> Vec<&Device> { devices.iter().filter(|d| d.model.kind == kind).collect() };
    fn one(group: Vec<&Device>) -> Vec<Vec<&Device>> {
        if group.is_empty() {
            Vec::new()
        } else {
            vec![group]
        }
    }
    match selection {
        Selection::Mouse => one(of_kind(Kind::Mouse)),
        Selection::Keyboard => one(of_kind(Kind::Keyboard)),
        Selection::First => {
            let mice = of_kind(Kind::Mouse);
            one(if mice.is_empty() {
                of_kind(Kind::Keyboard)
            } else {
                mice
            })
        }
        // By name: a receiver and a cable for the same model are one device.
        // Two of the same model show as one, the first that answers.
        Selection::All => {
            let mut grouped: Vec<Vec<&Device>> = Vec::new();
            for dev in devices {
                // An Omni receiver that couldn't be asked is found as a mouse
                // and as a keyboard (`discover`): one device, listed as the
                // first, the mouse.
                let listed = || {
                    grouped
                        .iter()
                        .flatten()
                        .any(|d| d.model.is_stand_in() && d.hidraw == dev.hidraw)
                };
                if dev.model.is_stand_in() && listed() {
                    continue;
                }
                let same = |g: &&mut Vec<&Device>| {
                    g.first().is_some_and(|f| {
                        (f.model.kind, f.model.name) == (dev.model.kind, dev.model.name)
                    })
                };
                match grouped.iter_mut().find(same) {
                    Some(group) => group.push(dev),
                    None => grouped.push(vec![dev]),
                }
            }
            grouped
        }
    }
}

/// Reads the devices `selection` asks for.
fn poll(selection: Selection) -> Vec<Status> {
    let devices = device::discover();
    let statuses = groups(selection, &devices)
        .iter()
        .map(|group| read_first(group))
        .collect();
    or_disconnected(statuses, selection)
}

/// Not `All`: always one status, "disconnected" if there's nothing to read.
fn or_disconnected(mut statuses: Vec<Status>, selection: Selection) -> Vec<Status> {
    if statuses.is_empty() && selection != Selection::All {
        statuses.push(Status::empty(State::Disconnected));
    }
    statuses
}

/// Reads the first of `devices` that reports a battery level. When the cable
/// and receiver are both plugged in, only one of them usually has live data.
fn read_first(devices: &[&Device]) -> Status {
    let mut fallback: Option<Status> = None;
    for &dev in devices {
        let status = match dev.read_battery() {
            Ok(r) if !r.is_asleep() => {
                return Status {
                    battery: Some(r.battery),
                    charging: r.charging,
                    low_battery_warning: r.low_battery_warning.and_then(Warning::percent),
                    power_off_minutes: r.power_off.and_then(PowerOff::minutes),
                    battery_updated: Some(now()),
                    ..Status::for_device(State::Connected, dev)
                };
            }
            Ok(_) => Status::for_device(State::Asleep, dev),
            Err(e) if e.means_asleep(dev.model.wireless) => Status::for_device(State::Asleep, dev),
            Err(e) => Status {
                error: Some(format!("{}: {e}", dev.hidraw.display())),
                ..Status::for_device(State::Error, dev)
            },
        };
        // Prefer reporting "asleep" over an error from another node.
        if fallback.as_ref().is_none_or(|f| f.state == State::Error) {
            fallback = Some(status);
        }
    }
    fallback.unwrap_or_else(|| Status::empty(State::Disconnected))
}

fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs())
}

/// Returns the error `println!` would panic on when stdout is closed, as in
/// `rogctl watch | head -1`.
fn print_human(s: &Status, nothing_found: &str) -> io::Result<()> {
    let name = s.device.unwrap_or("device");
    let mut out = io::stdout().lock();
    match s.state {
        State::Disconnected => writeln!(out, "{nothing_found}"),
        State::Error => writeln!(out, "{name}: {}", s.error.as_deref().unwrap_or("error")),
        State::Asleep => writeln!(out, "{name}: asleep or out of range"),
        State::Connected => {
            let pct = s.battery.unwrap_or(0);
            let charging = if s.charging { " (charging)" } else { "" };
            writeln!(out, "{name}: {pct}%{charging}")
        }
    }
}

fn print_json<T: Serialize>(value: &T) -> io::Result<()> {
    let mut out = io::stdout().lock();
    serde_json::to_writer(&mut out, value)?;
    writeln!(out)?;
    out.flush()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Format {
    Text,
    Json,
    Waybar,
}

impl Format {
    fn parse(name: &str) -> Option<Self> {
        match name {
            "text" => Some(Self::Text),
            "json" => Some(Self::Json),
            "waybar" => Some(Self::Waybar),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy)]
struct Output {
    format: Format,
    low_threshold: u8,
    selection: Selection,
}

/// `--device all`'s JSON: every device's status, and `watch`'s pid once.
#[derive(Debug, Serialize)]
struct Report<'a> {
    devices: &'a [Status],
    #[serde(skip_serializing_if = "Option::is_none")]
    pid: Option<u32>,
}

impl Output {
    /// Prints `statuses` as `poll(self.selection)` returned them, with
    /// `watch`'s `pid`.
    fn print(self, statuses: &mut [Status], pid: Option<u32>) -> io::Result<()> {
        let nothing_found = self.selection.nothing_found();
        if self.selection == Selection::All {
            return match self.format {
                Format::Json => print_json(&Report {
                    devices: statuses,
                    pid,
                }),
                Format::Text if statuses.is_empty() => {
                    print_human(&Status::empty(State::Disconnected), nothing_found)
                }
                // Refused when parsing the arguments; one line per device.
                Format::Text | Format::Waybar => statuses
                    .iter()
                    .try_for_each(|s| print_human(s, nothing_found)),
            };
        }
        let Some(s) = statuses.first_mut() else {
            return print_human(&Status::empty(State::Disconnected), nothing_found);
        };
        s.pid = pid;
        match self.format {
            Format::Text => print_human(s, nothing_found),
            Format::Json => print_json(s),
            Format::Waybar => print_json(&waybar::line(s, nothing_found)),
        }
    }
}

/// `battery`'s exit code for the statuses read: the best state among them.
fn exit_code(statuses: &[Status]) -> u8 {
    let any = |state: State| statuses.iter().any(|s| s.state == state);
    if any(State::Connected) {
        0
    } else if any(State::Asleep) {
        3
    } else if any(State::Error) {
        1
    } else {
        2
    }
}

fn cmd_battery(output: Output) -> ExitCode {
    // The Plasma widget runs `battery`, not `watch`.
    configure::notify_if_outdated();
    let mut statuses = poll(output.selection);
    for status in &mut statuses {
        status.low = is_low(status, output.low_threshold);
    }
    if output.print(&mut statuses, None).is_err() {
        return ExitCode::FAILURE;
    }
    ExitCode::from(exit_code(&statuses))
}

/// Remembers the last reading, and fills it back in while the same device
/// sleeps so the bar keeps showing its level.
fn carry_over(status: &mut Status, last_known: &mut Option<Status>) {
    match status.state {
        State::Connected => *last_known = Some(status.clone()),
        State::Asleep => {
            if let Some(prev) = last_known.as_ref().filter(|p| p.device == status.device) {
                status.battery = prev.battery;
                status.low_battery_warning = prev.low_battery_warning;
                status.power_off_minutes = prev.power_off_minutes;
                status.battery_updated = prev.battery_updated;
            }
        }
        State::Disconnected | State::Error => {}
    }
}

/// Whether `NO_COLOR` asks for no colour (<https://no-color.org>): set and
/// not empty.
fn no_color() -> bool {
    std::env::var_os("NO_COLOR").is_some_and(|v| !v.is_empty())
}

/// The word for a device that's awake, as every widget and the TUI show it:
/// "Battery: 79% (discharging)". The kernel's and desktops' words, not
/// Solaar's "recharging".
fn charge_word(charging: bool, battery: Option<u8>) -> &'static str {
    match (charging, battery) {
        (true, Some(100)) => "full",
        (true, _) => "charging",
        (false, _) => "discharging",
    }
}

/// A mouse that reports quarters is low at 25% whenever the threshold is below
/// that: its next level, 0, reads as asleep, so it would never be low.
fn is_low(s: &Status, threshold: u8) -> bool {
    threshold > 0
        && !s.charging
        && s.battery
            .is_some_and(|b| b <= threshold.max(s.battery_step))
}

/// How far above the threshold, or above the level a drop was notified at,
/// the battery must climb to re-arm the low-battery notification.
const REARM_RISE: u8 = 5;

/// Decides when the low-battery notification is due: once per drop to the
/// threshold, and again only after charging or climbing `REARM_RISE` above it.
///
/// Widgets restart `watch` often (GNOME on every unlock, every widget on a
/// settings change), so a notified drop is also remembered in a file of its
/// own per device and threshold, which exists while the drop is notified and
/// holds the level
/// it was notified at. Creating it with `create_new` is the one check, so of
/// several watches on one device only one notifies. A new watch that reads
/// a level more than `REARM_RISE` above that one takes the device to have
/// charged where no watch saw it (on another computer, or with the threshold
/// at 0), and re-arms; a smaller rise is a reading wobbling, not a charge. A
/// file without a level (empty, being written) counts as notified. A watch
/// that found the file there keeps trying to make it while the device stays
/// low, so if the watch that made it gives it back (`Watch::unclaim`), it
/// notifies instead.
/// If the file can't be made or removed, this falls back to remembering in
/// memory only.
#[derive(Debug, Default)]
struct LowWarning {
    /// Whether this drop was notified; `None` until this process knows.
    notified: Option<bool>,
    /// Notified by another `watch`, whose file this one couldn't make.
    claimed_elsewhere: bool,
    /// The device's file, `None` to remember in memory only.
    file: Option<PathBuf>,
}

impl LowWarning {
    /// `dir`: where to remember notified drops, `None` in memory only.
    /// `threshold`: the `--low-threshold` this is for, as each has its own
    /// drops.
    fn new(dir: Option<&std::path::Path>, key: DeviceKey, threshold: u8) -> Self {
        Self {
            notified: None,
            claimed_elsewhere: false,
            file: dir
                .zip(low_file_name(key, threshold))
                .map(|(dir, name)| dir.join(name)),
        }
    }

    fn update(&mut self, s: &mut Status, threshold: u8) {
        s.low = is_low(s, threshold);
        let Some(level) = s
            .battery
            .filter(|_| threshold > 0 && s.state == State::Connected)
        else {
            return;
        };
        if self.notified.is_none() && self.charged_since(level) {
            self.notified = Some(false);
        }
        if s.charging || level > threshold.saturating_add(REARM_RISE) {
            if self.notified != Some(false) {
                self.forget();
            }
            self.notified = Some(false);
            self.claimed_elsewhere = false;
        } else if s.low && (self.notified != Some(true) || self.claimed_elsewhere) {
            s.notify_low = self.claim(level);
            self.notified = Some(true);
            self.claimed_elsewhere = !s.notify_low;
        }
    }

    /// Whether the device's file holds a level more than `REARM_RISE` below
    /// `level`, so it charged since that drop was notified; if so, removes
    /// the file. It's moved
    /// aside first, so of watches starting together only one removes it, and
    /// one another watch claimed in between goes back.
    fn charged_since(&self, level: u8) -> bool {
        let Some(file) = &self.file else {
            return false;
        };
        let below = |path: &std::path::Path| {
            fs::read_to_string(path)
                .ok()
                .and_then(|text| text.trim().parse::<u8>().ok())
                .is_some_and(|notified| level > notified.saturating_add(REARM_RISE))
        };
        if !below(file) {
            return false;
        }
        let aside = file.with_extension(std::process::id().to_string());
        if fs::rename(file, &aside).is_err() {
            return false;
        }
        let charged = below(&aside);
        if !charged {
            let _ = fs::hard_link(&aside, file);
        }
        let _ = fs::remove_file(&aside);
        charged
    }

    /// Records the drop as notified at `level`: true if no other `watch` had
    /// (its file is there).
    fn claim(&mut self, level: u8) -> bool {
        let Some(file) = &self.file else {
            return true;
        };
        if let Some(dir) = file.parent() {
            let _ = fs::create_dir_all(dir);
        }
        match OpenOptions::new().write(true).create_new(true).open(file) {
            // Until this is written, the file counts as notified anyway.
            Ok(mut f) => {
                let _ = writeln!(f, "{level}");
                true
            }
            Err(e) if e.kind() == io::ErrorKind::AlreadyExists => false,
            Err(_) => {
                self.file = None;
                true
            }
        }
    }

    /// Re-arms: the next drop notifies. Also undoes a [`claim`] whose
    /// notification couldn't be sent.
    ///
    /// [`claim`]: Self::claim
    fn forget(&mut self) {
        let Some(file) = &self.file else {
            return;
        };
        match fs::remove_file(file) {
            Ok(()) => {}
            Err(e) if e.kind() == io::ErrorKind::NotFound => {}
            Err(_) => self.file = None,
        }
    }
}

/// The file a device's notified drop at `threshold` is kept in, such as
/// `mouse-rog-harpe-ii-ace-20`: only letters, digits and `-`.
fn low_file_name(key: DeviceKey, threshold: u8) -> Option<String> {
    device_file_name(key).map(|name| format!("{name}-{threshold}"))
}

/// A device's name for its files in the state directory, such as
/// `mouse-rog-harpe-ii-ace`: only letters, digits and `-`.
fn device_file_name((kind, name): DeviceKey) -> Option<String> {
    let kind = match kind? {
        Kind::Mouse => "mouse",
        Kind::Keyboard => "keyboard",
    };
    let name: String = name?
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() {
                c.to_ascii_lowercase()
            } else {
                '-'
            }
        })
        .collect();
    Some(format!("{kind}-{name}"))
}

/// A device's last level read awake, which `watch` keeps in a file of
/// its own per device (`device_file_name`), so a new `watch` (a widget
/// restarts it on a screen unlock or a settings change) shows it while the
/// device sleeps, as the one before did. What `carry_over` fills in.
#[derive(Debug, PartialEq, Serialize, Deserialize)]
struct LastLevel {
    battery: u8,
    battery_updated: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    low_battery_warning: Option<u8>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    power_off_minutes: Option<u8>,
}

impl LastLevel {
    /// The level kept in `dir` for `key`, as the status it was read in.
    /// Opened non-blocking, so something other than a file there can't hang
    /// `watch`.
    fn load(dir: &Path, key: DeviceKey) -> Option<Status> {
        let mut file = OpenOptions::new()
            .read(true)
            .custom_flags(libc::O_NONBLOCK)
            .open(dir.join(device_file_name(key)?))
            .ok()?;
        let mut text = String::new();
        io::Read::read_to_string(&mut file, &mut text).ok()?;
        let kept: Self = serde_json::from_str(&text).ok()?;
        (kept.battery <= 100).then(|| Status {
            kind: key.0,
            device: key.1,
            battery: Some(kept.battery),
            battery_updated: Some(kept.battery_updated),
            low_battery_warning: kept.low_battery_warning,
            power_off_minutes: kept.power_off_minutes,
            ..Status::empty(State::Connected)
        })
    }

    /// Keeps `s`'s level in `dir` if it was read awake. Written aside and
    /// renamed, so a `watch` starting never reads half of it; a failure is
    /// ignored, leaving the level in memory only.
    fn save(dir: &Path, s: &Status) {
        let (State::Connected, Some(name), Some(battery), Some(battery_updated)) = (
            s.state,
            device_file_name((s.kind, s.device)),
            s.battery,
            s.battery_updated,
        ) else {
            return;
        };
        let kept = Self {
            battery,
            battery_updated,
            low_battery_warning: s.low_battery_warning,
            power_off_minutes: s.power_off_minutes,
        };
        let Ok(text) = serde_json::to_string(&kept) else {
            return;
        };
        let _ = fs::create_dir_all(dir);
        // No `.` in a device's name, so this is no other device's file.
        let aside = dir.join(format!("{name}.{}", std::process::id()));
        let _ = fs::remove_file(&aside);
        let written = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&aside)
            .and_then(|mut f| writeln!(f, "{text}"))
            .and_then(|()| fs::rename(&aside, dir.join(name)));
        if written.is_err() {
            let _ = fs::remove_file(&aside);
        }
    }
}

/// Blocks SIGUSR1 so it can be waited for in `wait_or_refresh` rather than
/// killing the process. Nothing else in rogctl uses SIGUSR1, and `watch` is
/// single-threaded, so blocking it for the whole process is safe.
fn block_refresh_signal() -> libc::sigset_t {
    // SAFETY: plain libc calls on a local, zero-initialized sigset_t.
    unsafe {
        let mut set: libc::sigset_t = std::mem::zeroed();
        libc::sigemptyset(&mut set);
        libc::sigaddset(&mut set, libc::SIGUSR1);
        libc::pthread_sigmask(libc::SIG_BLOCK, &set, std::ptr::null_mut());
        set
    }
}

/// A file descriptor that becomes readable when SIGUSR1, blocked in `set`, is
/// pending, so `wait_or_refresh` can wait for it and for stdout at once.
/// `None` if the kernel refuses one; the wait then only watches the signal.
fn refresh_signal_fd(set: &libc::sigset_t) -> Option<OwnedFd> {
    // SAFETY: `set` is a valid sigset_t; the call creates a new descriptor.
    let fd = unsafe { libc::signalfd(-1, set, libc::SFD_NONBLOCK | libc::SFD_CLOEXEC) };
    // SAFETY: a descriptor signalfd just returned, owned by nothing else.
    (fd >= 0).then(|| unsafe { OwnedFd::from_raw_fd(fd) })
}

/// What ended a wait in `wait_or_refresh`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Woken {
    /// The time ran out.
    TimedOut,
    /// SIGUSR1: a widget asking to read the devices now.
    Refresh,
    /// stdout's reader went away (a pipe's read end closed, a terminal hung
    /// up), so nothing will read another line.
    ReaderGone,
}

/// Sleeps for `wait`, returning early on SIGUSR1 (`signals`, from
/// `refresh_signal_fd`) or when `out`'s reader goes away. Several signals
/// during one wait count as one. A terminal or a file never reports its
/// reader gone, and isn't asked whether it's readable or writable (which it
/// always is), so the wait can't spin on one.
fn wait_or_refresh(
    set: &libc::sigset_t,
    signals: Option<&OwnedFd>,
    out: RawFd,
    wait: Duration,
) -> Woken {
    let Some(signals) = signals else {
        return wait_for_signal(set, wait);
    };
    let deadline = Instant::now() + wait;
    loop {
        let left = deadline.saturating_duration_since(Instant::now());
        if left.is_zero() {
            return Woken::TimedOut;
        }
        // Rounded up, so it doesn't spin through the last millisecond.
        let millis = left.as_micros().div_ceil(1000);
        let timeout = libc::c_int::try_from(millis).unwrap_or(libc::c_int::MAX);
        let mut fds = [
            libc::pollfd {
                fd: signals.as_raw_fd(),
                events: libc::POLLIN,
                revents: 0,
            },
            // No events asked for: POLLERR, POLLHUP and POLLNVAL come anyway.
            libc::pollfd {
                fd: out,
                events: 0,
                revents: 0,
            },
        ];
        // SAFETY: `fds` is a valid array of two pollfds for the whole call.
        let ready = unsafe { libc::poll(fds.as_mut_ptr(), 2, timeout) };
        if ready < 0 {
            if io::Error::last_os_error().raw_os_error() == Some(libc::EINTR) {
                continue;
            }
            // Can't poll (ENOMEM): still wait out the time, for the signal.
            return wait_for_signal(set, left);
        }
        if fds[1].revents & (libc::POLLERR | libc::POLLHUP | libc::POLLNVAL) != 0 {
            return Woken::ReaderGone;
        }
        if fds[0].revents & libc::POLLIN != 0 {
            drain(signals);
            return Woken::Refresh;
        }
        if fds[0].revents != 0 {
            // Never seen; waiting the time out beats waking again at once.
            return wait_for_signal(set, left);
        }
    }
}

/// Reads every pending signal off `signals`, which doesn't block.
fn drain(signals: &OwnedFd) {
    let mut info = std::mem::MaybeUninit::<libc::signalfd_siginfo>::uninit();
    let size = std::mem::size_of::<libc::signalfd_siginfo>();
    // SAFETY: reads at most `size` bytes into `info`, which has that many.
    while unsafe { libc::read(signals.as_raw_fd(), info.as_mut_ptr().cast(), size) } > 0 {}
}

/// `wait_or_refresh` without a signal descriptor: waits for SIGUSR1 alone.
fn wait_for_signal(set: &libc::sigset_t, wait: Duration) -> Woken {
    let deadline = Instant::now() + wait;
    loop {
        let left = deadline.saturating_duration_since(Instant::now());
        if left.is_zero() {
            return Woken::TimedOut;
        }
        let timeout = libc::timespec {
            tv_sec: libc::time_t::try_from(left.as_secs()).unwrap_or(libc::time_t::MAX),
            tv_nsec: libc::c_long::from(left.subsec_nanos()),
        };
        // SAFETY: `set` is a valid sigset_t and `timeout` outlives the call.
        let got = unsafe { libc::sigtimedwait(set, std::ptr::null_mut(), &timeout) };
        if got == libc::SIGUSR1 {
            return Woken::Refresh;
        }
        // EAGAIN means the time ran out; EINTR (another signal, e.g. SIGCONT)
        // means keep waiting.
        if io::Error::last_os_error().raw_os_error() != Some(libc::EINTR) {
            return Woken::TimedOut;
        }
    }
}

/// First retry after an error, which is often brief: a device on a cable
/// rejecting a query, or its node going away for a moment. (A receiver
/// rejecting a query means the device is asleep; see `ASLEEP_RETRY`.)
const ERROR_RETRY: Duration = Duration::from_secs(5);

/// How often to read a sleeping device (in the TUI, a failed mouse too), so one
/// that wakes shows its level within about 10 s rather than a whole interval
/// later. OpenRazer's battery notifier (`battery_notifier.py`) retries as often.
const ASLEEP_RETRY: Duration = Duration::from_secs(10);

/// A day. Far larger values would overflow the deadline in `wait_or_refresh`.
const MAX_INTERVAL_SECS: u64 = 86_400;

/// How long until a device is read again: the interval; after an error a
/// retry that doubles each time up to the interval; while asleep
/// `ASLEEP_RETRY`.
fn next_wait(state: State, interval: Duration, retry: &mut Duration) -> Duration {
    match state {
        State::Error => {
            let wait = interval.min(*retry);
            *retry = retry.saturating_mul(2);
            wait
        }
        // Sleeping leaves the error backoff alone, so errors between sleeps
        // still back off.
        State::Asleep => interval.min(ASLEEP_RETRY),
        State::Connected | State::Disconnected => {
            *retry = ERROR_RETRY;
            interval
        }
    }
}

/// A device, as `watch` keeps track of it: its kind and model name.
type DeviceKey = (Option<Kind>, Option<&'static str>);

/// What `watch` remembers about one device between reads.
#[derive(Debug, Default)]
struct Tracked {
    last_known: Option<Status>,
    low_warning: LowWarning,
}

/// Carries each device's level over while it sleeps and decides its
/// low-battery notification, keyed by device so each has its own. `low_dir`
/// is where notified drops are remembered (`LowWarning`), and `level_dir`
/// where each device's last level is (`LastLevel`): a device first seen
/// starts from the one kept for it.
fn track(
    statuses: &mut [Status],
    tracked: &mut HashMap<DeviceKey, Tracked>,
    low_threshold: u8,
    low_dir: Option<&Path>,
    level_dir: Option<&Path>,
) {
    for status in statuses {
        let key = (status.kind, status.device);
        let t = tracked.entry(key).or_insert_with(|| Tracked {
            last_known: level_dir.and_then(|dir| LastLevel::load(dir, key)),
            low_warning: LowWarning::new(low_dir, key, low_threshold),
        });
        carry_over(status, &mut t.last_known);
        if let Some(dir) = level_dir {
            LastLevel::save(dir, status);
        }
        t.low_warning.update(status, low_threshold);
    }
}

/// Devices due within this of each other are read together, so they share a
/// line rather than printing two a moment apart.
const DUE_SLACK: Duration = Duration::from_secs(1);

/// When `watch` next reads one device (`boot_time`), and what it read last.
#[derive(Debug)]
struct Due {
    at: Duration,
    retry: Duration,
    status: Status,
}

/// `watch`'s state: each device is read on its own schedule, so one that's
/// asleep or failing is retried often without reading the others as often.
struct Watch {
    interval: Duration,
    low_threshold: u8,
    /// Where notified drops are remembered across restarts, `None` in memory
    /// only (`LowWarning`).
    low_dir: Option<PathBuf>,
    /// Where each device's last level is kept across restarts (`LastLevel`).
    level_dir: Option<PathBuf>,
    tracked: HashMap<DeviceKey, Tracked>,
    due: HashMap<DeviceKey, Due>,
}

impl Watch {
    fn new(interval: Duration, low_threshold: u8) -> Self {
        Self {
            interval,
            low_threshold,
            low_dir: None,
            level_dir: None,
            tracked: HashMap::new(),
            due: HashMap::new(),
        }
    }

    /// Re-arms the drops `statuses` would have notified, for a line that
    /// couldn't be written: no one was notified, so the next `watch` must.
    fn unclaim(&mut self, statuses: &[Status]) {
        for s in statuses.iter().filter(|s| s.notify_low) {
            if let Some(t) = self.tracked.get_mut(&(s.kind, s.device)) {
                t.low_warning.forget();
            }
        }
    }

    /// One pass over the devices `keys` names: reads each one that's due, or
    /// every one on a refresh, with `read(index)`, and repeats the others'
    /// last status (without its notification). A device not seen on the last
    /// pass is read at once. Returns the statuses and the wait until the next
    /// one is due. With no devices that's `ASLEEP_RETRY` (or the interval if
    /// shorter), though `cmd_watch` then waits an interval. `now` is
    /// `boot_time()`.
    fn step(
        &mut self,
        keys: &[DeviceKey],
        mut read: impl FnMut(usize) -> Status,
        now: Duration,
        refresh: bool,
    ) -> (Vec<Status>, Duration) {
        self.due.retain(|key, _| keys.contains(key));
        let mut statuses = Vec::with_capacity(keys.len());
        for (index, key) in keys.iter().enumerate() {
            let waiting = self
                .due
                .get(key)
                .filter(|due| !refresh && due.at > now.saturating_add(DUE_SLACK));
            if let Some(due) = waiting {
                statuses.push(Status {
                    notify_low: false,
                    ..due.status.clone()
                });
                continue;
            }
            let mut status = read(index);
            track(
                std::slice::from_mut(&mut status),
                &mut self.tracked,
                self.low_threshold,
                self.low_dir.as_deref(),
                self.level_dir.as_deref(),
            );
            let mut retry = self.due.get(key).map_or(ERROR_RETRY, |due| due.retry);
            let wait = next_wait(status.state, self.interval, &mut retry);
            self.due.insert(
                *key,
                Due {
                    at: now.saturating_add(wait),
                    retry,
                    status: status.clone(),
                },
            );
            statuses.push(status);
        }
        let wait = self
            .due
            .values()
            .map(|due| due.at.saturating_sub(now))
            .min()
            .unwrap_or(self.interval.min(ASLEEP_RETRY));
        (statuses, wait)
    }
}

fn cmd_watch(interval: Duration, output: Output) -> ExitCode {
    // Before the first line: a widget only learns the pid from that, and
    // SIGUSR1 would kill the process until it's blocked.
    let signals = block_refresh_signal();
    let signal_fd = refresh_signal_fd(&signals);
    configure::notify_if_outdated();
    let mut watch = Watch::new(interval, output.low_threshold);
    // A sleeping device's last level is kept across restarts for every
    // format (Waybar too). Only JSON carries `notify_low`: a watch in a
    // terminal or Waybar, which notifies no one, mustn't use up a widget's
    // notification.
    let state = configure::state_dir().ok();
    watch.level_dir = state.as_ref().map(|d| d.join("last-level"));
    if output.format == Format::Json {
        watch.low_dir = state.map(|d| d.join("low-battery"));
    }
    let mut refresh = false;
    // The nodes the last pass found, and until when to look for devices
    // again every `ASLEEP_RETRY` since they changed.
    let mut last_nodes: Option<Vec<PathBuf>> = None;
    let mut rescan_until = Duration::ZERO;
    loop {
        // Nodes first: one plugged in between the two is caught next pass.
        let nodes = device::candidate_nodes();
        let devices = device::discover();
        let now = boot_time();
        if last_nodes.as_ref().is_some_and(|last| *last != nodes) {
            // Plugged in or out: read every device now, and keep looking a
            // while for one that doesn't answer yet.
            refresh = true;
            rescan_until = now.saturating_add(RESCAN_AFTER_CHANGE);
        }
        let groups = groups(output.selection, &devices);
        let keys: Vec<DeviceKey> = groups
            .iter()
            .map(|group| {
                let model = group.first().map(|d| d.model);
                (model.map(|m| m.kind), model.map(|m| m.name))
            })
            .collect();
        let read = |i: usize| {
            groups.get(i).map_or_else(
                || Status::empty(State::Disconnected),
                |group| read_first(group),
            )
        };
        let (statuses, wait) = watch.step(&keys, read, now, refresh);
        let mut statuses = or_disconnected(statuses, output.selection);
        // stdout closed: the bar (or pipe reader) went away, so stop.
        if output
            .print(&mut statuses, Some(std::process::id()))
            .is_err()
        {
            watch.unclaim(&statuses);
            return ExitCode::SUCCESS;
        }
        // With nothing to read, look again once the interval is up.
        let wait = if keys.is_empty() { interval } else { wait };
        // A receiver kept unidentified is asked again as often as a sleeping
        // device is read, so the mouse on it soon shows under its own name.
        let rescan = now < rescan_until || devices.iter().any(|d| d.model.is_stand_in());
        let woken = wait_for_next(
            now.saturating_add(wait),
            rescan,
            boot_time,
            |wait| wait_or_refresh(&signals, signal_fd.as_ref(), libc::STDOUT_FILENO, wait),
            || device::candidate_nodes() != nodes,
        );
        last_nodes = Some(nodes);
        match woken {
            Woken::TimedOut => refresh = false,
            Woken::Refresh => refresh = true,
            // As above, but noticed while waiting rather than up to an
            // interval later, so a widget's next watch isn't left beside it.
            Woken::ReaderGone => return ExitCode::SUCCESS,
        }
    }
}

/// How long after its nodes change `watch` keeps looking for devices every
/// `ASLEEP_RETRY`: an Omni receiver just plugged in may not say yet what's
/// paired with it, and is left out until it does.
const RESCAN_AFTER_CHANGE: Duration = Duration::from_secs(60);

/// `watch`'s wait for its next pass, until `deadline` on `clock`: `wait` for
/// `ASLEEP_RETRY` at a time, and after each, end the wait early if `rescan`,
/// to look for devices again, or if `nodes_changed()` (sysfs alone, no HID
/// traffic). So a device plugged in or out shows within 10 s, while an Omni
/// receiver or another device's node isn't asked what it is (`discover()`,
/// the pairing query) unless something changed or a device is due, perhaps
/// beside a `rogctl settings`. `clock` is `boot_time`, which counts time
/// suspended, so a device due during a suspend is read within 10 s of the
/// resume.
fn wait_for_next(
    deadline: Duration,
    rescan: bool,
    mut clock: impl FnMut() -> Duration,
    mut wait: impl FnMut(Duration) -> Woken,
    mut nodes_changed: impl FnMut() -> bool,
) -> Woken {
    loop {
        let left = deadline.saturating_sub(clock());
        if left.is_zero() {
            return Woken::TimedOut;
        }
        match wait(left.min(ASLEEP_RETRY)) {
            Woken::TimedOut if !rescan && !nodes_changed() => {}
            woken => return woken,
        }
    }
}

/// Time since boot, time suspended included (`CLOCK_BOOTTIME`), which
/// `watch`'s due times are on: `Instant` stops while suspended, so a device
/// due during a suspend would wait out the rest of its interval after the
/// resume. Without that clock, the time since the first call, on `Instant`.
fn boot_time() -> Duration {
    let mut t = libc::timespec {
        tv_sec: 0,
        tv_nsec: 0,
    };
    // SAFETY: clock_gettime fills the timespec it's given.
    let got = unsafe { libc::clock_gettime(libc::CLOCK_BOOTTIME, &mut t) } == 0;
    let since_boot = u64::try_from(t.tv_sec)
        .ok()
        .zip(u64::try_from(t.tv_nsec).ok())
        .filter(|_| got);
    match since_boot {
        Some((secs, nanos)) => {
            Duration::from_secs(secs).saturating_add(Duration::from_nanos(nanos))
        }
        None => {
            static START: OnceLock<Instant> = OnceLock::new();
            START.get_or_init(Instant::now).elapsed()
        }
    }
}

/// Sends `signal` to `pid` only if it's a `rogctl watch`. Widgets refresh and
/// stop their watcher this way rather than with `kill`: the pid comes from its
/// output, and under a wrapper (a container's, say) it may name another
/// process to them, while this, run through the same wrapper, sees the right
/// one.
fn cmd_signal(pid: Option<&str>, signal: libc::c_int) -> ExitCode {
    let Some(pid) = pid
        .and_then(|p| p.parse::<libc::pid_t>().ok())
        .filter(|&p| p > 0)
    else {
        complain!("needs the process ID of a `rogctl watch`");
        return ExitCode::FAILURE;
    };
    let proc = |file: &str| std::fs::read(format!("/proc/{pid}/{file}")).unwrap_or_default();
    let own_name = std::fs::read("/proc/self/comm").unwrap_or_default();
    if !is_watch(&proc("comm"), &own_name, &proc("cmdline")) {
        complain!("{pid} isn't a `rogctl watch`; not signalled");
        return ExitCode::FAILURE;
    }
    // SAFETY: kill() takes plain integers and has no memory effects.
    if unsafe { libc::kill(pid, signal) } != 0 {
        complain!("couldn't signal {pid}: {}", io::Error::last_os_error());
        return ExitCode::FAILURE;
    }
    ExitCode::SUCCESS
}

/// Whether a process with this `/proc` `comm` and `cmdline` is a `rogctl
/// watch`: named like this binary, with `watch` as its first argument.
fn is_watch(comm: &[u8], own_comm: &[u8], cmdline: &[u8]) -> bool {
    !comm.is_empty() && comm == own_comm && cmdline.split(|&b| b == 0).nth(1) == Some(b"watch")
}

fn cmd_list() -> ExitCode {
    let devices = device::discover();
    let mut out = io::stdout().lock();
    if devices.is_empty() {
        let _ = writeln!(out, "{}", Selection::All.nothing_found());
        return ExitCode::from(2);
    }
    for dev in devices {
        let link = if dev.model.wireless {
            "wireless"
        } else {
            "wired"
        };
        let kind = match dev.model.kind {
            Kind::Mouse => "mouse",
            Kind::Keyboard => "keyboard",
        };
        let printed = writeln!(
            out,
            "{}  {} ({kind}, {link}, {:04x}:{:04x} interface {})",
            dev.hidraw.display(),
            dev.model.name,
            device::ASUS_VID,
            dev.model.pid,
            dev.model.interface
        );
        if printed.is_err() {
            return ExitCode::FAILURE;
        }
    }
    ExitCode::SUCCESS
}

/// An argument's text, `None` if it isn't UTF-8.
fn text(arg: Option<Result<String, OsString>>) -> Option<String> {
    arg.and_then(Result::ok)
}

/// Whether an argument, UTF-8 or not, starts with `-`.
fn starts_with_dash(arg: &Result<String, OsString>) -> bool {
    match arg {
        Ok(text) => text.starts_with('-'),
        Err(raw) => raw.as_bytes().first() == Some(&b'-'),
    }
}

fn main() -> ExitCode {
    // Only a file name may be other than UTF-8; anything else is refused.
    let mut args = std::env::args_os()
        .skip(1)
        .map(OsString::into_string)
        .peekable();
    let command = match args.next_if(|c| !starts_with_dash(c)) {
        Some(Ok(command)) => command,
        Some(Err(other)) => {
            complain!("unknown command: {}\n\n{USAGE}", other.to_string_lossy());
            return ExitCode::FAILURE;
        }
        None => "battery".to_owned(),
    };

    let mut output = Output {
        format: Format::Text,
        low_threshold: 20,
        selection: Selection::First,
    };
    let mut device_given = false;
    let mut interval = Duration::from_secs(60);
    let mut interval_given = false;
    // `Some(None)`: --demo without a model name.
    let mut demo: Option<Option<String>> = None;
    let mut desktop: Option<String> = None;
    let mut pid: Option<String> = None;
    let mut remove = false;
    let mut force = false;
    // `rogctl settings get|set|export|import`, and their arguments.
    let settings = matches!(command.as_str(), "settings" | "tui");
    let is_settings_command =
        |a: &str| settings && matches!(a, "get" | "set" | "export" | "import");
    let mut settings_command: Option<String> = None;
    let mut settings_args: Vec<OsString> = Vec::new();
    let mut dry_run = false;
    while let Some(arg) = args.next() {
        let arg = match arg {
            Ok(arg) => arg,
            Err(path)
                if matches!(settings_command.as_deref(), Some("export" | "import"))
                    && path.as_bytes().first() != Some(&b'-') =>
            {
                settings_args.push(path);
                continue;
            }
            Err(other) => {
                complain!("unknown argument: {}\n\n{USAGE}", other.to_string_lossy());
                return ExitCode::FAILURE;
            }
        };
        match arg.as_str() {
            // After `settings get` and the like, only a model's name: a
            // `KEY=VALUE` or file name there is the command's.
            "--demo" => {
                demo = Some(
                    args.next_if(|v| {
                        v.as_ref().is_ok_and(|v| {
                            !v.starts_with('-')
                                && !is_settings_command(v)
                                && (settings_command.is_none() || app::find_model(v).is_some())
                        })
                    })
                    .and_then(Result::ok),
                );
            }
            "--dry-run" => dry_run = true,
            "--json" => output.format = Format::Json,
            "--remove" => remove = true,
            "--force" => force = true,
            "--format" => match text(args.next()).as_deref().and_then(Format::parse) {
                Some(format) => output.format = format,
                None => {
                    complain!("--format needs one of: text, json, waybar");
                    return ExitCode::FAILURE;
                }
            },
            "--device" => match text(args.next()).as_deref().and_then(Selection::parse) {
                Some(selection) => {
                    output.selection = selection;
                    device_given = true;
                }
                None => {
                    complain!("--device needs one of: mouse, keyboard, all");
                    return ExitCode::FAILURE;
                }
            },
            "--low-threshold" => match text(args.next()).and_then(|v| v.parse::<u8>().ok()) {
                Some(pct) if pct <= 100 => output.low_threshold = pct,
                _ => {
                    complain!("--low-threshold needs a percentage from 0 to 100");
                    return ExitCode::FAILURE;
                }
            },
            "--interval" => match text(args.next()).and_then(|v| v.parse::<u64>().ok()) {
                Some(secs) if (1..=MAX_INTERVAL_SECS).contains(&secs) => {
                    interval = Duration::from_secs(secs);
                    interval_given = true;
                }
                _ => {
                    complain!("--interval needs a number of seconds from 1 to {MAX_INTERVAL_SECS}");
                    return ExitCode::FAILURE;
                }
            },
            "-h" | "--help" => return print_usage(),
            "-V" | "--version" => {
                let _ = writeln!(io::stdout(), "rogctl {}", env!("CARGO_PKG_VERSION"));
                return ExitCode::SUCCESS;
            }
            other if command == "configure" && desktop.is_none() && !other.starts_with('-') => {
                desktop = Some(other.to_owned());
            }
            other if matches!(command.as_str(), "refresh" | "stop") && pid.is_none() => {
                pid = Some(other.to_owned());
            }
            other if settings_command.is_none() && is_settings_command(other) => {
                settings_command = Some(other.to_owned());
            }
            // `-` is stdin or stdout. `angle-tuning=-5` starts with a key;
            // `--demo=X` is no argument.
            other if settings_command.is_some() && (other == "-" || !other.starts_with('-')) => {
                settings_args.push(other.into());
            }
            other => {
                complain!("unknown argument: {other}\n\n{USAGE}");
                return ExitCode::FAILURE;
            }
        }
    }

    // `tui` was the command's first name; it still works, undocumented.
    if demo.is_some() && !settings {
        complain!("--demo only works with `rogctl settings`");
        return ExitCode::FAILURE;
    }
    if interval_given && command != "watch" {
        complain!("--interval only works with `rogctl watch`");
        return ExitCode::FAILURE;
    }
    if device_given && !matches!(command.as_str(), "battery" | "watch") {
        complain!("--device only works with `rogctl battery` and `rogctl watch`");
        return ExitCode::FAILURE;
    }
    if output.selection == Selection::All && output.format == Format::Waybar {
        complain!(
            "Waybar shows one device per module: use a module with --device mouse \
             and one with --device keyboard"
        );
        return ExitCode::FAILURE;
    }
    if (remove || force) && command != "configure" {
        complain!("--remove and --force only work with `rogctl configure`");
        return ExitCode::FAILURE;
    }

    if dry_run && !matches!(settings_command.as_deref(), Some("set" | "import")) {
        complain!("--dry-run only works with `rogctl settings set` and `import`");
        return ExitCode::FAILURE;
    }
    if let Some(name) = settings_command {
        let json = output.format == Format::Json;
        let command = match (name.as_str(), settings_args.len()) {
            (_, _) if output.format == Format::Waybar => None,
            ("get", 0) => Some(settings::Command::Get { json }),
            (_, _) if json => None,
            ("set", _) => settings_args
                .into_iter()
                .map(OsString::into_string)
                .collect::<Result<_, _>>()
                .ok()
                .map(settings::Command::Set),
            ("export", 0 | 1) => Some(settings::Command::Export(
                settings_args.pop().map(PathBuf::from),
            )),
            ("import", 1) => settings_args
                .pop()
                .map(|file| settings::Command::Import(PathBuf::from(file))),
            _ => None,
        };
        let Some(command) = command else {
            complain!("unknown or missing arguments for `rogctl settings {name}`\n\n{USAGE}");
            return ExitCode::FAILURE;
        };
        let demo = demo.as_ref().map(|name| name.as_deref());
        return ExitCode::from(settings::run(command, demo, dry_run));
    }

    match command.as_str() {
        "settings" | "tui" => tui::run(demo),
        "battery" => cmd_battery(output),
        "watch" => cmd_watch(interval, output),
        "list" => cmd_list(),
        "refresh" => cmd_signal(pid.as_deref(), libc::SIGUSR1),
        "stop" => cmd_signal(pid.as_deref(), libc::SIGTERM),
        "configure" => configure::run(desktop.as_deref(), remove, force),
        "help" => print_usage(),
        other => {
            complain!("unknown command: {other}\n\n{USAGE}");
            ExitCode::FAILURE
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn signals_only_a_rogctl_watch() {
        let watch = b"rogctl\0watch\0--json\0--interval\x0060\0";
        assert!(is_watch(b"rogctl\n", b"rogctl\n", watch));
        assert!(
            is_watch(b"rogctl\n", b"rogctl\n", b"/usr/bin/rogctl\0watch\0"),
            "any path"
        );
        assert!(!is_watch(b"rogctl\n", b"rogctl\n", b"rogctl\0settings\0"));
        assert!(!is_watch(b"rogctl\n", b"rogctl\n", b"rogctl\0"));
        assert!(!is_watch(b"firefox\n", b"rogctl\n", b"firefox\0watch\0"));
        assert!(!is_watch(b"", b"", watch), "gone, or /proc unreadable");
    }

    #[test]
    fn signal_refuses_what_isnt_a_watch() {
        assert_eq!(cmd_signal(None, libc::SIGUSR1), ExitCode::FAILURE);
        assert_eq!(cmd_signal(Some("0"), libc::SIGUSR1), ExitCode::FAILURE);
        assert_eq!(cmd_signal(Some("-1"), libc::SIGUSR1), ExitCode::FAILURE);
        assert_eq!(cmd_signal(Some("x"), libc::SIGUSR1), ExitCode::FAILURE);
        // This test binary is no `rogctl watch`, so it isn't signalled.
        let own = std::process::id().to_string();
        assert_eq!(cmd_signal(Some(&own), libc::SIGTERM), ExitCode::FAILURE);
    }

    fn connected(device: &'static str, battery: u8) -> Status {
        Status {
            device: Some(device),
            battery: Some(battery),
            battery_updated: Some(1000),
            ..Status::empty(State::Connected)
        }
    }

    fn asleep(device: &'static str) -> Status {
        Status {
            device: Some(device),
            ..Status::empty(State::Asleep)
        }
    }

    /// The desktop widgets read these field names, expect snake_case states,
    /// and treat missing fields as nil/undefined, so `None` must be omitted,
    /// not null.
    #[test]
    fn json_matches_the_plugin_contract() {
        let json = serde_json::to_value(connected("Mouse", 80)).unwrap();
        assert_eq!(json["state"], "connected");
        assert_eq!(json["device"], "Mouse");
        assert_eq!(json["battery"], 80);
        assert_eq!(json["charging"], false);
        assert_eq!(json["battery_updated"], 1000);
        assert!(json.get("error").is_none());

        assert!(json.get("low").is_none(), "false flags are left out");
        assert!(json.get("notify_low").is_none());

        let json = serde_json::to_value(Status {
            low: true,
            notify_low: true,
            pid: Some(42),
            ..connected("Mouse", 15)
        })
        .unwrap();
        assert_eq!(json["low"], true);
        assert_eq!(json["notify_low"], true);
        assert_eq!(json["pid"], 42);

        let json = serde_json::to_value(Status::empty(State::Disconnected)).unwrap();
        assert_eq!(json.as_object().unwrap().len(), 2, "{json}");
    }

    fn with_charging(mut s: Status, charging: bool) -> Status {
        s.charging = charging;
        s
    }

    /// The (low, notify_low) pair `LowWarning` sets for each reading in turn.
    fn low_warnings(
        readings: impl IntoIterator<Item = Status>,
        threshold: u8,
    ) -> Vec<(bool, bool)> {
        let mut warning = LowWarning::default();
        readings
            .into_iter()
            .map(|mut s| {
                warning.update(&mut s, threshold);
                (s.low, s.notify_low)
            })
            .collect()
    }

    #[test]
    fn notifies_once_per_drop() {
        let levels = [30, 20, 19, 22, 18, 26, 20];
        let got = low_warnings(levels.map(|b| connected("Mouse", b)), 20);
        assert_eq!(
            got,
            [
                (false, false),
                (true, true),   // reached the threshold
                (true, false),  // still low: no repeat
                (false, false), // 22 is within 5% of 20, still armed off
                (true, false),
                (false, false), // 26 re-arms
                (true, true),
            ]
        );
    }

    #[test]
    fn charging_rearms_and_is_never_low() {
        let readings = [
            connected("Mouse", 15),
            with_charging(connected("Mouse", 16), true),
            connected("Mouse", 16),
        ];
        assert_eq!(
            low_warnings(readings, 20),
            [(true, true), (false, false), (true, true)]
        );
    }

    /// Quarters: 25% is the lowest level reported awake, so it's low at a
    /// threshold below 25, and climbing back to 50% re-arms.
    #[test]
    fn quarters_are_low_at_their_lowest_level() {
        let quarters = |battery| Status {
            battery_step: 25,
            ..connected("Mouse", battery)
        };
        let readings = [50, 25, 50, 25].map(quarters);
        assert_eq!(
            low_warnings(readings.clone(), 20),
            [(false, false), (true, true), (false, false), (true, true)]
        );
        assert_eq!(
            low_warnings(readings.clone(), 50),
            [(true, true), (true, false), (true, false), (true, false)],
            "a threshold above 25 is used as is"
        );
        assert_eq!(low_warnings(readings, 0), [(false, false); 4], "0 is off");
        assert!(
            !is_low(&connected("Mouse", 25), 20),
            "percent mice as before"
        );
    }

    #[test]
    fn asleep_is_low_but_never_notifies() {
        let mut sleeping = asleep("Mouse");
        sleeping.battery = Some(10);
        assert_eq!(low_warnings([sleeping.clone()], 20), [(true, false)]);
        assert_eq!(
            low_warnings([sleeping], 0),
            [(false, false)],
            "0 turns it off"
        );
    }

    /// SIGUSR1 cuts the wait short. The signal goes to this thread only, so the
    /// other test threads (which don't block it) can't be killed by it.
    #[test]
    fn sigusr1_ends_the_wait() {
        let set = block_refresh_signal();
        let signals = refresh_signal_fd(&set);
        assert!(signals.is_some());
        let (_reader, out) = pipe();
        for signals in [signals.as_ref(), None] {
            let wait = |time| wait_or_refresh(&set, signals, out.as_raw_fd(), time);
            let start = Instant::now();
            assert_eq!(wait(Duration::from_millis(50)), Woken::TimedOut);
            assert!(start.elapsed() >= Duration::from_millis(50));

            // SAFETY: signals this thread, which has SIGUSR1 blocked.
            unsafe { libc::pthread_kill(libc::pthread_self(), libc::SIGUSR1) };
            let start = Instant::now();
            assert_eq!(wait(Duration::from_secs(10)), Woken::Refresh);
            assert!(start.elapsed() < Duration::from_secs(1));
            // It counted once: the next wait runs its time out.
            assert_eq!(wait(Duration::from_millis(20)), Woken::TimedOut);
        }
    }

    /// A pipe: (read end, write end).
    fn pipe() -> (OwnedFd, OwnedFd) {
        let mut fds = [0; 2];
        // SAFETY: `fds` has room for the two descriptors pipe() returns.
        assert_eq!(unsafe { libc::pipe(fds.as_mut_ptr()) }, 0);
        // SAFETY: both were just made, and are owned by nothing else.
        unsafe { (OwnedFd::from_raw_fd(fds[0]), OwnedFd::from_raw_fd(fds[1])) }
    }

    /// CPU time this thread has used.
    fn thread_cpu() -> Duration {
        // SAFETY: getrusage fills the zeroed struct it's given.
        let usage = unsafe {
            let mut usage: libc::rusage = std::mem::zeroed();
            assert_eq!(libc::getrusage(libc::RUSAGE_THREAD, &mut usage), 0);
            usage
        };
        let time = |t: libc::timeval| {
            Duration::from_secs(t.tv_sec as u64) + Duration::from_micros(t.tv_usec as u64)
        };
        time(usage.ru_utime) + time(usage.ru_stime)
    }

    /// A widget that closes its end of the pipe without killing `watch` ends
    /// the wait at once, rather than an interval later at the next write.
    #[test]
    fn a_closed_pipe_ends_the_wait() {
        let set = block_refresh_signal();
        let signals = refresh_signal_fd(&set);
        let (reader, out) = pipe();
        drop(reader);
        let start = Instant::now();
        let woken = wait_or_refresh(
            &set,
            signals.as_ref(),
            out.as_raw_fd(),
            Duration::from_secs(10),
        );
        assert_eq!(woken, Woken::ReaderGone);
        assert!(start.elapsed() < Duration::from_secs(1));
    }

    /// A file or `/dev/null` as stdout is always writable and never hangs up:
    /// the wait runs its time out without spinning.
    #[test]
    fn a_file_as_stdout_waits_without_spinning() {
        let set = block_refresh_signal();
        let signals = refresh_signal_fd(&set);
        let path = std::env::temp_dir().join(format!("rogctl-out-{}", std::process::id()));
        let file = fs::File::create(&path).unwrap();
        let null = fs::OpenOptions::new()
            .write(true)
            .open("/dev/null")
            .unwrap();
        for out in [file.as_raw_fd(), null.as_raw_fd()] {
            let (start, cpu) = (Instant::now(), thread_cpu());
            let woken = wait_or_refresh(&set, signals.as_ref(), out, Duration::from_millis(300));
            assert_eq!(woken, Woken::TimedOut);
            assert!(start.elapsed() >= Duration::from_millis(300));
            assert!(thread_cpu() - cpu < Duration::from_millis(100), "spun");
        }
        let _ = fs::remove_file(path);
    }

    /// The widgets ship with this binary, so their versions move together.
    #[test]
    fn plugin_version_matches_crate() {
        let manifest = include_str!("../noctalia/rog-mouse-battery/plugin.toml");
        let version = manifest
            .lines()
            .find_map(|l| l.strip_prefix("version"))
            .and_then(|l| l.trim_start().strip_prefix('='))
            .map(|v| v.trim().trim_matches('"'))
            .expect("plugin.toml has no version");
        assert_eq!(version, env!("CARGO_PKG_VERSION"), "update plugin.toml");

        let json_manifests: [(&str, &str, &[&str]); 3] = [
            (
                "dms/RogMouseBattery/plugin.json",
                include_str!("../dms/RogMouseBattery/plugin.json"),
                &["version"],
            ),
            (
                "kde/rog-mouse-battery/metadata.json",
                include_str!("../kde/rog-mouse-battery/metadata.json"),
                &["KPlugin", "Version"],
            ),
            (
                "gnome/rog-mouse-battery@humblemonk.github.io/metadata.json",
                include_str!("../gnome/rog-mouse-battery@humblemonk.github.io/metadata.json"),
                &["version-name"],
            ),
        ];
        for (path, text, keys) in json_manifests {
            let json: serde_json::Value = serde_json::from_str(text).expect(path);
            let version = keys.iter().fold(&json, |v, k| &v[k]);
            assert_eq!(version, env!("CARGO_PKG_VERSION"), "update {path}");
        }
    }

    #[test]
    fn keeps_last_level_while_asleep() {
        let mut last = None;
        carry_over(&mut connected("Mouse", 80), &mut last);

        let mut status = asleep("Mouse");
        carry_over(&mut status, &mut last);
        assert_eq!(status.state, State::Asleep);
        assert_eq!(status.battery, Some(80));
        assert_eq!(status.battery_updated, Some(1000));
    }

    #[test]
    fn retries_errors_sooner_with_backoff() {
        let interval = Duration::from_secs(60);
        let mut retry = ERROR_RETRY;
        let waits: Vec<u64> = (0..6)
            .map(|_| next_wait(State::Error, interval, &mut retry).as_secs())
            .collect();
        assert_eq!(waits, [5, 10, 20, 40, 60, 60]);

        assert_eq!(next_wait(State::Connected, interval, &mut retry), interval);
        assert_eq!(next_wait(State::Error, interval, &mut retry), ERROR_RETRY);
        assert_eq!(
            next_wait(State::Error, interval, &mut retry),
            2 * ERROR_RETRY
        );

        // Asleep never backs off, and doesn't reset the error backoff.
        for _ in 0..6 {
            assert_eq!(next_wait(State::Asleep, interval, &mut retry), ASLEEP_RETRY);
        }
        assert_eq!(
            next_wait(State::Error, interval, &mut retry),
            4 * ERROR_RETRY
        );
        assert_eq!(
            next_wait(State::Disconnected, interval, &mut retry),
            interval
        );

        // Never waits longer than a short interval.
        let short = Duration::from_secs(2);
        assert_eq!(next_wait(State::Error, short, &mut retry), short);
        assert_eq!(next_wait(State::Asleep, short, &mut retry), short);
    }

    /// A model as `discover` would find it on `hidraw`.
    fn found(name: &str, pid: u16, hidraw: &str) -> Device {
        Device {
            model: models::all()
                .find(|m| m.name == name && m.pid == pid)
                .unwrap(),
            hidraw: hidraw.into(),
        }
    }

    fn names(groups: &[Vec<&Device>]) -> Vec<Vec<String>> {
        groups
            .iter()
            .map(|g| {
                g.iter()
                    .map(|d| format!("{} {}", d.model.name, d.hidraw.display()))
                    .collect()
            })
            .collect()
    }

    /// A Harpe II Ace on its receiver and cable, and a Falchion RX Low
    /// Profile on an Omni receiver and its cable, in `discover` order.
    fn mouse_and_keyboard() -> Vec<Device> {
        vec![
            found("ROG Harpe II Ace", 0x1ad0, "/dev/hidraw8"),
            found("ROG Harpe II Ace", 0x1c69, "/dev/hidraw3"),
            found("ROG Falchion RX Low Profile", 0x1b04, "/dev/hidraw19"),
            found(
                "ROG Falchion RX Low Profile",
                models::OMNI_PID,
                "/dev/hidraw15",
            ),
        ]
    }

    #[test]
    fn groups_each_device_by_name() {
        let devices = mouse_and_keyboard();
        assert_eq!(
            names(&groups(Selection::All, &devices)),
            [
                vec![
                    "ROG Harpe II Ace /dev/hidraw8",
                    "ROG Harpe II Ace /dev/hidraw3"
                ],
                vec![
                    "ROG Falchion RX Low Profile /dev/hidraw19",
                    "ROG Falchion RX Low Profile /dev/hidraw15"
                ],
            ]
        );
        assert!(groups(Selection::All, &[]).is_empty());
    }

    /// Without `--device`, a keyboard is shown only when there's no mouse.
    #[test]
    fn shows_a_mouse_before_a_keyboard() {
        let devices = mouse_and_keyboard();
        let mice = names(&groups(Selection::Mouse, &devices));
        assert_eq!(names(&groups(Selection::First, &devices)), mice);
        assert_eq!(mice.len(), 1);
        assert!(mice[0].iter().all(|n| n.starts_with("ROG Harpe II Ace")));

        let keyboards = names(&groups(Selection::Keyboard, &devices));
        assert_eq!(keyboards.len(), 1);
        assert_eq!(names(&groups(Selection::First, &devices[2..])), keyboards);
        assert!(groups(Selection::Keyboard, &devices[..2]).is_empty());
        assert!(groups(Selection::Mouse, &devices[2..]).is_empty());
    }

    #[test]
    fn exit_code_is_the_best_state() {
        let error = Status::empty(State::Error);
        assert_eq!(exit_code(&[asleep("Mouse"), connected("Keyboard", 52)]), 0);
        assert_eq!(exit_code(&[error.clone(), asleep("Keyboard")]), 3);
        assert_eq!(exit_code(&[error]), 1);
        assert_eq!(exit_code(&[Status::empty(State::Disconnected)]), 2);
        assert_eq!(exit_code(&[]), 2);
    }

    /// `--device all`: one object holding each status, the pid once.
    #[test]
    fn all_devices_json() {
        let mut keyboard = connected("ROG Falchion RX Low Profile", 52);
        keyboard.kind = Some(Kind::Keyboard);
        let devices = [connected("ROG Harpe II Ace", 80), keyboard];
        let json = serde_json::to_value(Report {
            devices: &devices,
            pid: Some(42),
        })
        .unwrap();
        assert_eq!(json["pid"], 42);
        assert_eq!(json["devices"][1]["kind"], "keyboard");
        assert_eq!(json["devices"][1]["battery"], 52);
        assert!(json["devices"][0].get("pid").is_none());
        let json = serde_json::to_value(Report {
            devices: &[],
            pid: None,
        })
        .unwrap();
        assert_eq!(json.to_string(), r#"{"devices":[]}"#);
    }

    /// Each device keeps its own level while asleep and gets its own
    /// low-battery notification.
    #[test]
    fn tracks_each_device() {
        let mut tracked = HashMap::new();
        let mut first = [connected("Mouse", 80), connected("Keyboard", 15)];
        track(&mut first, &mut tracked, 20, None, None);
        assert!(!first[0].notify_low && first[1].notify_low);

        let mut next = [connected("Mouse", 18), asleep("Keyboard")];
        track(&mut next, &mut tracked, 20, None, None);
        assert!(next[0].notify_low);
        assert_eq!(next[1].battery, Some(15));
        assert!(next[1].low && !next[1].notify_low);

        let mut again = [connected("Mouse", 17), connected("Keyboard", 14)];
        track(&mut again, &mut tracked, 20, None, None);
        assert!(
            !again[0].notify_low && !again[1].notify_low,
            "once per drop"
        );
    }

    const MOUSE: DeviceKey = (Some(Kind::Mouse), Some("Mouse"));
    const KEYBOARD: DeviceKey = (Some(Kind::Keyboard), Some("Keyboard"));

    /// Runs one `Watch::step` at `at` seconds, reading device i as
    /// `reads[i]`, and returns which devices it read, the levels shown and the
    /// wait in seconds.
    fn step(
        watch: &mut Watch,
        start: Duration,
        at: u64,
        keys: &[DeviceKey],
        reads: &[Status],
        refresh: bool,
    ) -> (Vec<usize>, Vec<Option<u8>>, u64) {
        let mut read = Vec::new();
        let (statuses, wait) = watch.step(
            keys,
            |i| {
                read.push(i);
                reads[i].clone()
            },
            start + Duration::from_secs(at),
            refresh,
        );
        let levels = statuses.iter().map(|s| s.battery).collect();
        (read, levels, wait.as_secs())
    }

    /// Waits for the next pass as `wait_for_next` would on a clock that
    /// advances by each wait, plus `suspended` seconds during the first:
    /// whether the wait ended and the waits it took. The third wait ends with
    /// `last`, and the nodes change on check `changes_at`.
    fn wait_for(
        deadline: u64,
        rescan: bool,
        changes_at: Option<usize>,
        last: Woken,
        suspended: u64,
    ) -> (Woken, Vec<u64>) {
        let clock = std::cell::Cell::new(Duration::ZERO);
        let mut waits = Vec::new();
        let mut checks = 0;
        let woken = wait_for_next(
            Duration::from_secs(deadline),
            rescan,
            || clock.get(),
            |d| {
                waits.push(d.as_secs());
                let slept = if waits.len() == 1 { suspended } else { 0 };
                clock.set(clock.get() + d + Duration::from_secs(slept));
                if waits.len() == 3 {
                    last
                } else {
                    Woken::TimedOut
                }
            },
            || {
                checks += 1;
                Some(checks) == changes_at
            },
        );
        (woken, waits)
    }

    /// Between passes, sysfs is checked every 10 s, but the devices are
    /// looked for again only when its nodes change, a device is due, or while
    /// rescanning.
    #[test]
    fn looks_for_devices_when_a_node_changes_or_one_is_due() {
        use Woken::{ReaderGone, Refresh, TimedOut};
        let ten = |n| vec![10; n];
        assert_eq!(wait_for(60, false, None, TimedOut, 0), (TimedOut, ten(6)));
        assert_eq!(
            wait_for(60, false, Some(2), TimedOut, 0),
            (TimedOut, ten(2)),
            "plugged in or out"
        );
        assert_eq!(
            wait_for(25, false, None, TimedOut, 0),
            (TimedOut, vec![10, 10, 5])
        );
        assert_eq!(wait_for(4, false, None, TimedOut, 0), (TimedOut, vec![4]));
        assert_eq!(wait_for(60, false, None, Refresh, 0), (Refresh, ten(3)));
        assert_eq!(
            wait_for(60, false, None, ReaderGone, 0),
            (ReaderGone, ten(3))
        );
        assert_eq!(
            wait_for(60, true, None, TimedOut, 0),
            (TimedOut, ten(1)),
            "rescanning"
        );
        assert_eq!(wait_for(0, false, None, TimedOut, 0), (TimedOut, vec![]));
    }

    /// A device due during a suspend is read within 10 s of the resume, not
    /// the rest of its interval later.
    #[test]
    fn time_suspended_counts() {
        assert_eq!(
            wait_for(600, false, None, Woken::TimedOut, 3600),
            (Woken::TimedOut, vec![10])
        );
        let before = boot_time();
        assert!(boot_time() >= before);
    }

    /// A sleeping keyboard is read every 10 s without reading the awake mouse
    /// that often: each device has its own schedule.
    #[test]
    fn reads_each_device_when_it_is_due() {
        let mut watch = Watch::new(Duration::from_secs(60), 20);
        let start = boot_time();
        let keys = [MOUSE, KEYBOARD];
        let reads = [connected("Mouse", 80), asleep("Keyboard")];
        assert_eq!(
            step(&mut watch, start, 0, &keys, &reads, false),
            (vec![0, 1], vec![Some(80), None], 10)
        );
        for at in [10, 20, 30, 40, 50] {
            let (read, levels, wait) = step(&mut watch, start, at, &keys, &reads, false);
            assert_eq!((read, wait), (vec![1], 10), "at {at} s");
            assert_eq!(levels, [Some(80), None], "the mouse's last level");
        }
        let (read, _, _) = step(&mut watch, start, 60, &keys, &reads, false);
        assert_eq!(read, [0, 1], "both due at 60 s");

        // A refresh reads both at once.
        let (read, _, _) = step(&mut watch, start, 61, &keys, &reads, true);
        assert_eq!(read, [0, 1]);
    }

    /// Each device backs off its own errors, and with nothing plugged in
    /// `watch` looks again as often as it reads a sleeping device, so a device
    /// plugged in shows up within 10 s, whatever `--device`.
    #[test]
    fn backs_off_each_device_on_its_own() {
        let mut watch = Watch::new(Duration::from_secs(60), 20);
        let start = boot_time();
        let keys = [MOUSE, KEYBOARD];
        let failing = Status {
            device: Some("Keyboard"),
            ..Status::empty(State::Error)
        };
        let reads = [connected("Mouse", 80), failing];
        let waits: Vec<u64> = [0, 5, 15]
            .into_iter()
            .map(|at| step(&mut watch, start, at, &keys, &reads, false).2)
            .collect();
        assert_eq!(waits, [5, 10, 20]);
        assert_eq!(step(&mut watch, start, 100, &[], &[], false).2, 10);
        let mut short = Watch::new(Duration::from_secs(3), 20);
        assert_eq!(step(&mut short, start, 0, &[], &[], false).2, 3);
    }

    /// A scratch state directory, removed when dropped.
    struct ScratchDir(PathBuf);

    impl ScratchDir {
        fn new(name: &str) -> Self {
            let nanos = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos();
            let dir =
                std::env::temp_dir().join(format!("rogctl-{name}-{}-{nanos}", std::process::id()));
            Self(dir)
        }
    }

    impl Drop for ScratchDir {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
            let _ = fs::remove_file(&self.0);
        }
    }

    /// (low, notify_low) for one reading by a new `LowWarning`, as a `watch`
    /// just restarted would make, remembering in `dir`.
    fn after_restart(dir: &std::path::Path, reading: Status) -> (bool, bool) {
        let key = (Some(Kind::Mouse), reading.device);
        let mut s = reading;
        LowWarning::new(Some(dir), key, 20).update(&mut s, 20);
        (s.low, s.notify_low)
    }

    /// A restarted `watch` (GNOME restarts it on every unlock) doesn't notify
    /// the same drop again, and still notifies the next one.
    #[test]
    fn remembers_a_notified_drop_across_restarts() {
        let scratch = ScratchDir::new("low");
        let dir = scratch.0.join("low-battery");
        let mouse = |b| connected("ROG Harpe II Ace", b);
        assert_eq!(after_restart(&dir, mouse(18)), (true, true));
        assert!(dir.join("mouse-rog-harpe-ii-ace-20").is_file());
        assert_eq!(after_restart(&dir, mouse(17)), (true, false), "same drop");
        assert_eq!(
            after_restart(&dir, connected("Other", 17)),
            (true, true),
            "another device has its own"
        );
        assert_eq!(after_restart(&dir, mouse(26)), (false, false));
        assert!(
            !dir.join("mouse-rog-harpe-ii-ace-20").exists(),
            "26 re-arms"
        );
        assert_eq!(after_restart(&dir, mouse(20)), (true, true));
        assert_eq!(
            after_restart(&dir, with_charging(mouse(20), true)),
            (false, false)
        );
        assert_eq!(
            after_restart(&dir, mouse(19)),
            (true, true),
            "charging re-arms"
        );
    }

    /// A device that charged and drained again where no watch saw it (used on
    /// another computer, the widget off) notifies its next drop, read from a
    /// level more than 5% above the one notified, and not one at, below or
    /// just above it.
    #[test]
    fn a_higher_level_after_a_restart_re_arms() {
        let scratch = ScratchDir::new("low-charged");
        let dir = scratch.0.join("low-battery");
        let file = dir.join("mouse-rog-harpe-ii-ace-20");
        let mouse = |b| connected("ROG Harpe II Ace", b);
        assert_eq!(after_restart(&dir, mouse(10)), (true, true));
        assert_eq!(fs::read_to_string(&file).unwrap(), "10\n");
        assert_eq!(after_restart(&dir, mouse(10)), (true, false), "same level");
        assert_eq!(after_restart(&dir, mouse(8)), (true, false), "lower");
        assert_eq!(
            fs::read_to_string(&file).unwrap(),
            "10\n",
            "kept as notified"
        );
        assert_eq!(after_restart(&dir, mouse(15)), (true, false), "a wobble");
        assert_eq!(after_restart(&dir, mouse(16)), (true, true), "charged");
        assert_eq!(fs::read_to_string(&file).unwrap(), "16\n");
        assert_eq!(after_restart(&dir, mouse(16)), (true, false));

        // Not yet low, but above the level notified: the next drop notifies.
        // Within one watch, only charging or 5% above the threshold re-arms.
        let mut warning = LowWarning::new(
            Some(&dir),
            (Some(Kind::Mouse), Some("ROG Harpe II Ace")),
            20,
        );
        let got: Vec<bool> = [23, 20, 22, 19]
            .map(|b| {
                let mut s = mouse(b);
                warning.update(&mut s, 20);
                s.notify_low
            })
            .into();
        assert_eq!(got, [false, true, false, false]);

        // Empty, as while another watch writes it: notified, level unknown.
        fs::write(&file, "").unwrap();
        assert_eq!(after_restart(&dir, mouse(19)), (true, false));
        assert_eq!(fs::read_dir(&dir).unwrap().count(), 1, "nothing left aside");
    }

    /// Two watches on one device notify a drop once, a re-armed one included.
    #[test]
    fn concurrent_watches_notify_once() {
        let scratch = ScratchDir::new("low-two");
        let dir = scratch.0.join("low-battery");
        let key = (Some(Kind::Mouse), Some("Mouse"));
        let notified = |dir: &std::path::Path, level| {
            let mut a = LowWarning::new(Some(dir), key, 20);
            let mut b = LowWarning::new(Some(dir), key, 20);
            let mut sa = connected("Mouse", level);
            let mut sb = connected("Mouse", level);
            a.update(&mut sa, 20);
            b.update(&mut sb, 20);
            [sa.notify_low, sb.notify_low]
        };
        assert_eq!(notified(&dir, 10), [true, false]);
        assert_eq!(notified(&dir, 10), [false, false], "same drop");
        assert_eq!(notified(&dir, 11), [false, false], "a wobble");
        assert_eq!(notified(&dir, 16), [true, false], "charged, once");
    }

    /// A new `watch` shows a sleeping device's last level, as the one
    /// before it did, from the file that one kept.
    #[test]
    fn a_restarted_watch_keeps_the_last_level() {
        let scratch = ScratchDir::new("last-level");
        let dir = scratch.0.join("last-level");
        let mouse = |state| Status {
            kind: Some(Kind::Mouse),
            ..Status::empty(state)
        };
        let awake = Status {
            device: Some("ROG Harpe II Ace"),
            battery: Some(53),
            battery_updated: Some(1000),
            low_battery_warning: Some(20),
            power_off_minutes: Some(3),
            charging: true,
            ..mouse(State::Connected)
        };
        let sleeping = Status {
            device: Some("ROG Harpe II Ace"),
            ..mouse(State::Asleep)
        };
        let run = |status: &Status| {
            let mut s = [status.clone()];
            track(&mut s, &mut HashMap::new(), 20, None, Some(&dir));
            let [s] = s;
            s
        };
        assert_eq!(run(&sleeping).battery, None, "nothing kept yet");
        run(&awake);
        assert!(dir.join("mouse-rog-harpe-ii-ace").is_file());
        assert_eq!(fs::read_dir(&dir).unwrap().count(), 1, "nothing left aside");
        let shown = run(&sleeping);
        assert_eq!(shown.state, State::Asleep);
        assert_eq!(
            (shown.battery, shown.battery_updated),
            (Some(53), Some(1000))
        );
        assert_eq!(
            (shown.low_battery_warning, shown.power_off_minutes),
            (Some(20), Some(3))
        );
        assert!(!shown.charging, "as within one watch");
        let other = Status {
            device: Some("ROG Keris II Ace"),
            ..sleeping.clone()
        };
        assert_eq!(run(&other).battery, None, "another device's own");
        // Asleep or failing doesn't replace it.
        run(&Status {
            device: Some("ROG Harpe II Ace"),
            ..mouse(State::Error)
        });
        assert_eq!(run(&sleeping).battery, Some(53));
        // A broken file is ignored.
        fs::write(dir.join("mouse-rog-harpe-ii-ace"), "{\"battery\":250}").unwrap();
        assert_eq!(run(&sleeping).battery, None);
    }

    /// A watch that found the drop notified by another notifies it after all
    /// if that one gives it back, its line unwritten, and only once.
    #[test]
    fn a_drop_given_back_by_another_watch_is_notified() {
        let scratch = ScratchDir::new("low-given-back");
        let dir = scratch.0.join("low-battery");
        let key = (Some(Kind::Mouse), Some("Mouse"));
        let mut a = LowWarning::new(Some(&dir), key, 20);
        let mut b = LowWarning::new(Some(&dir), key, 20);
        let update = |w: &mut LowWarning, level| {
            let mut s = connected("Mouse", level);
            w.update(&mut s, 20);
            s.notify_low
        };
        assert!(update(&mut a, 10));
        assert!(!update(&mut b, 10), "a has notified it");
        assert!(!update(&mut b, 10), "still a's");
        a.forget();
        assert!(update(&mut b, 10), "given back");
        assert!(!update(&mut b, 10) && !update(&mut a, 10), "once");
    }

    /// Watches with different thresholds keep apart: one at 20% reading 30%
    /// doesn't re-arm the drop one at 30% notified, so that one doesn't
    /// notify it again after a restart.
    #[test]
    fn each_threshold_has_its_own_drop() {
        let scratch = ScratchDir::new("low-thresholds");
        let dir = scratch.0.join("low-battery");
        let key = (Some(Kind::Mouse), Some("Mouse"));
        let notified = |level, threshold| {
            let mut s = connected("Mouse", level);
            LowWarning::new(Some(&dir), key, threshold).update(&mut s, threshold);
            s.notify_low
        };
        assert!(notified(30, 30));
        assert!(!notified(30, 20), "not low at 20%");
        assert!(!notified(29, 30), "same drop, after a restart");
        assert!(notified(20, 20));
        assert!(!notified(20, 20));
    }

    /// A line whose notification couldn't be written (the reader gone) leaves
    /// the drop for the next `watch` to notify.
    #[test]
    fn an_unwritten_notification_is_given_back() {
        let scratch = ScratchDir::new("low-unwritten");
        let dir = scratch.0.join("low-battery");
        let read = |_| Status {
            kind: Some(Kind::Mouse),
            ..connected("Mouse", 10)
        };
        let mut watch = Watch::new(Duration::from_secs(60), 20);
        watch.low_dir = Some(dir.clone());
        let (statuses, _) = watch.step(&[MOUSE], read, boot_time(), false);
        assert!(statuses[0].notify_low);
        assert!(dir.join("mouse-mouse-20").is_file());
        watch.unclaim(&statuses);
        assert!(!dir.join("mouse-mouse-20").exists());
        let mut next = Watch::new(Duration::from_secs(60), 20);
        next.low_dir = Some(dir);
        let (statuses, _) = next.step(&[MOUSE], read, boot_time(), false);
        assert!(statuses[0].notify_low, "notified by the next watch");
    }

    /// With the state directory unusable, the drop is remembered in memory, as
    /// before: once per drop for this process.
    #[test]
    fn falls_back_to_memory_without_a_state_dir() {
        let scratch = ScratchDir::new("low-file");
        // A file where the directory should be: nothing can be made in it.
        fs::write(&scratch.0, "").unwrap();
        let dir = scratch.0.join("low-battery");
        let mut warning = LowWarning::new(Some(&dir), (Some(Kind::Mouse), Some("Mouse")), 20);
        let got: Vec<bool> = [18, 17, 26, 19]
            .map(|b| {
                let mut s = connected("Mouse", b);
                warning.update(&mut s, 20);
                s.notify_low
            })
            .into();
        assert_eq!(got, [true, false, false, true]);
        assert_eq!(
            low_file_name(
                (Some(Kind::Keyboard), Some("ROG Falchion RX Low Profile")),
                20
            ),
            Some("keyboard-rog-falchion-rx-low-profile-20".to_owned())
        );
        assert_eq!(low_file_name((None, Some("x/../y")), 20), None);
        assert_eq!(
            low_file_name((Some(Kind::Mouse), Some("../x")), 5).as_deref(),
            Some("mouse----x-5"),
            "never leaves the directory"
        );
    }

    /// An Omni receiver that can't be opened is listed as a mouse and as a
    /// keyboard, so `--device keyboard` reports its error too, as a keyboard,
    /// and `--device all` once, as the mouse.
    #[test]
    fn an_unreadable_omni_receiver_is_reported_for_either_kind() {
        let devices = [
            Device {
                model: &models::OMNI_RECEIVER,
                hidraw: "/dev/hidraw15".into(),
            },
            Device {
                model: &models::OMNI_KEYBOARD_RECEIVER,
                hidraw: "/dev/hidraw15".into(),
            },
        ];
        for (selection, kind) in [
            (Selection::Mouse, Kind::Mouse),
            (Selection::Keyboard, Kind::Keyboard),
            (Selection::First, Kind::Mouse),
        ] {
            let found = groups(selection, &devices);
            assert_eq!(found.len(), 1);
            assert!(found[0].iter().all(|d| d.model.kind == kind));
        }
        let all = groups(Selection::All, &devices);
        assert_eq!(all.len(), 1);
        assert_eq!(all[0].len(), 1);
        assert_eq!(all[0][0].model.kind, Kind::Mouse);
        // Another receiver's node is another device.
        let two = [
            devices[0].clone(),
            devices[1].clone(),
            Device {
                model: &models::OMNI_RECEIVER,
                hidraw: "/dev/hidraw16".into(),
            },
            Device {
                model: &models::OMNI_KEYBOARD_RECEIVER,
                hidraw: "/dev/hidraw16".into(),
            },
        ];
        let all = groups(Selection::All, &two);
        assert_eq!(names(&all).concat().len(), 2);
    }

    /// A device that appears is read at once, and the repeated status of one
    /// not yet due doesn't repeat its low-battery notification.
    #[test]
    fn notifies_once_and_reads_new_devices_at_once() {
        let mut watch = Watch::new(Duration::from_secs(60), 20);
        let start = boot_time();
        let mut low = connected("Mouse", 15);
        low.kind = Some(Kind::Mouse);
        let (statuses, _) = watch.step(&[MOUSE], |_| low.clone(), start, false);
        assert!(statuses[0].notify_low);

        let mut keyboard = connected("Keyboard", 52);
        keyboard.kind = Some(Kind::Keyboard);
        let reads = [low, keyboard];
        let mut read = Vec::new();
        let (statuses, _) = watch.step(
            &[MOUSE, KEYBOARD],
            |i| {
                read.push(i);
                reads[i].clone()
            },
            start + Duration::from_secs(5),
            false,
        );
        assert_eq!(read, [1], "only the new keyboard");
        assert!(statuses[0].low && !statuses[0].notify_low);
    }

    #[test]
    fn does_not_carry_level_to_another_mouse() {
        let mut last = None;
        carry_over(&mut connected("Mouse A", 80), &mut last);

        let mut status = asleep("Mouse B");
        carry_over(&mut status, &mut last);
        assert_eq!(status.battery, None);
    }
}
