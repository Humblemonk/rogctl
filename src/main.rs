mod app;
mod configure;
mod device;
mod models;
mod protocol;
mod tui;
mod ui;
mod waybar;

use std::io::{self, Write};
use std::process::ExitCode;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use serde::Serialize;

use device::Device;
use protocol::PowerOff;

const USAGE: &str = "\
rogctl - battery status and settings for ASUS mice

Examples:
  rogctl                                 Show the mouse's battery level
  rogctl settings                        View and change DPI, polling rate and more
  rogctl settings --demo                 Try the settings screen without a mouse
  rogctl configure kde                   Add the battery widget to KDE Plasma

Usage:
  rogctl [battery] [OPTIONS]             Print the battery status once
  rogctl watch [--interval SECS] [OPTIONS]
                                         Print the status every SECS seconds (default 60)
  rogctl list                            List detected devices and their hidraw nodes
  rogctl settings [--demo [MODEL]]       View and change the mouse's settings (DPI,
                                         polling rate, ...). --demo runs without a
                                         mouse, as MODEL (default ROG Harpe II Ace)
  rogctl configure [DESKTOP] [--remove] [--force]
                                         Install the panel widget for DESKTOP (noctalia,
                                         waybar, quickshell, dms, kde or gnome), or
                                         remove it. Without DESKTOP, lists them

Options:
  --json                 Same as --format json
  --format FORMAT        text (default), json, or waybar (a Waybar custom module)
  --low-threshold PCT    Level at or below which the battery counts as low
                         (default 20, 0 turns it off)

Exit status of `battery`: 0 connected, 1 error, 2 no device, 3 mouse asleep.
`watch` reads the mouse right away when sent SIGUSR1 (`kill -USR1 PID`).
";

/// What the bar sees. `battery` stays at the last known level while the mouse
/// sleeps, since the receiver stops reporting it.
#[derive(Debug, Clone, PartialEq, Serialize)]
struct Status {
    state: State,
    #[serde(skip_serializing_if = "Option::is_none")]
    device: Option<&'static str>,
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
    /// Unix seconds when `battery` was last read from the mouse.
    #[serde(skip_serializing_if = "Option::is_none")]
    battery_updated: Option<u64>,
    /// At or below `--low-threshold` and not charging.
    #[serde(skip_serializing_if = "is_false")]
    low: bool,
    /// Set by `watch` on the one line where a low-battery notification is due.
    #[serde(skip_serializing_if = "is_false")]
    notify_low: bool,
    /// The `watch` process, for sending it SIGUSR1.
    #[serde(skip_serializing_if = "Option::is_none")]
    pid: Option<u32>,
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
        }
    }

    fn for_device(state: State, dev: &Device) -> Self {
        Self {
            device: Some(dev.model.name),
            wireless: Some(dev.model.wireless),
            hidraw: Some(dev.hidraw.display().to_string()),
            ..Self::empty(state)
        }
    }
}

/// Reads the first device that reports a battery level. When the cable and
/// receiver are both plugged in, only one of them usually has live data.
fn poll() -> Status {
    let devices = device::discover();
    let mut fallback: Option<Status> = None;
    for dev in &devices {
        let status = match dev.read_battery() {
            Ok(r) if !r.is_asleep() => {
                return Status {
                    battery: Some(r.battery),
                    charging: r.charging,
                    low_battery_warning: r.low_battery_warning,
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
fn print_human(s: &Status) -> io::Result<()> {
    let name = s.device.unwrap_or("mouse");
    let mut out = io::stdout().lock();
    match s.state {
        State::Disconnected => writeln!(out, "No supported mouse found"),
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
}

impl Output {
    fn print(self, s: &Status) -> io::Result<()> {
        match self.format {
            Format::Text => print_human(s),
            Format::Json => print_json(s),
            Format::Waybar => print_json(&waybar::line(s)),
        }
    }
}

fn cmd_battery(output: Output) -> ExitCode {
    let mut status = poll();
    status.low = is_low(&status, output.low_threshold);
    if output.print(&status).is_err() {
        return ExitCode::FAILURE;
    }
    ExitCode::from(match status.state {
        State::Connected => 0,
        State::Error => 1,
        State::Disconnected => 2,
        State::Asleep => 3,
    })
}

/// Remembers the last reading, and fills it back in while the same mouse
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

/// The word for a mouse that's awake, as every widget and the TUI show it:
/// "Battery: 79% (discharging)". The kernel's and desktops' words, not
/// Solaar's "recharging".
fn charge_word(charging: bool, battery: Option<u8>) -> &'static str {
    match (charging, battery) {
        (true, Some(100)) => "full",
        (true, _) => "charging",
        (false, _) => "discharging",
    }
}

fn is_low(s: &Status, threshold: u8) -> bool {
    threshold > 0 && !s.charging && s.battery.is_some_and(|b| b <= threshold)
}

/// Decides when the low-battery notification is due: once per drop to the
/// threshold, and again only after charging or climbing 5% above it.
#[derive(Debug, Default)]
struct LowWarning {
    notified: bool,
}

impl LowWarning {
    fn update(&mut self, s: &mut Status, threshold: u8) {
        s.low = is_low(s, threshold);
        let Some(level) = s
            .battery
            .filter(|_| threshold > 0 && s.state == State::Connected)
        else {
            return;
        };
        if s.charging || level > threshold.saturating_add(5) {
            self.notified = false;
        } else if s.low && !self.notified {
            self.notified = true;
            s.notify_low = true;
        }
    }
}

/// Blocks SIGUSR1 so it can be waited for in `wait_or_refresh` rather than
/// killing the process. Nothing else in rogctl uses signals, so blocking it for
/// the whole (single-threaded) process is safe.
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

/// Sleeps for `wait`, returning early if SIGUSR1 arrives: a widget asking to
/// read the mouse now. Several signals during one wait count as one.
fn wait_or_refresh(set: &libc::sigset_t, wait: Duration) {
    let deadline = Instant::now() + wait;
    loop {
        let left = deadline.saturating_duration_since(Instant::now());
        if left.is_zero() {
            return;
        }
        let timeout = libc::timespec {
            tv_sec: libc::time_t::try_from(left.as_secs()).unwrap_or(libc::time_t::MAX),
            tv_nsec: libc::c_long::from(left.subsec_nanos()),
        };
        // SAFETY: `set` is a valid sigset_t and `timeout` outlives the call.
        let got = unsafe { libc::sigtimedwait(set, std::ptr::null_mut(), &timeout) };
        if got == libc::SIGUSR1 {
            return;
        }
        // EAGAIN means the time ran out; EINTR (another signal, e.g. SIGCONT)
        // means keep waiting.
        if io::Error::last_os_error().raw_os_error() != Some(libc::EINTR) {
            return;
        }
    }
}

/// First retry after an error or while asleep. The receiver can reject a query
/// for a moment, e.g. right after login, and a mouse that just woke shouldn't
/// wait a whole interval to show its level.
const ERROR_RETRY: Duration = Duration::from_secs(5);

/// A day. Far larger values would overflow the deadline in `wait_or_refresh`.
const MAX_INTERVAL_SECS: u64 = 86_400;

/// How long to wait before the next poll: the interval, or after errors and
/// while asleep a retry that doubles each time up to the interval.
fn next_wait(state: State, interval: Duration, retry: &mut Duration) -> Duration {
    if !matches!(state, State::Error | State::Asleep) {
        *retry = ERROR_RETRY;
        return interval;
    }
    let wait = (*retry).min(interval);
    *retry = retry.saturating_mul(2);
    wait
}

fn cmd_watch(interval: Duration, output: Output) -> ExitCode {
    // Before the first line: a widget only learns the pid from that, and
    // SIGUSR1 would kill the process until it's blocked.
    let refresh = block_refresh_signal();
    let mut last_known: Option<Status> = None;
    let mut low_warning = LowWarning::default();
    let mut retry = ERROR_RETRY;
    loop {
        let mut status = poll();
        carry_over(&mut status, &mut last_known);
        low_warning.update(&mut status, output.low_threshold);
        status.pid = Some(std::process::id());
        // stdout closed: the bar (or pipe reader) went away, so stop.
        if output.print(&status).is_err() {
            return ExitCode::SUCCESS;
        }
        wait_or_refresh(&refresh, next_wait(status.state, interval, &mut retry));
    }
}

fn cmd_list() -> ExitCode {
    let devices = device::discover();
    let mut out = io::stdout().lock();
    if devices.is_empty() {
        let _ = writeln!(out, "No supported mouse found");
        return ExitCode::from(2);
    }
    for dev in devices {
        let link = if dev.model.wireless {
            "wireless"
        } else {
            "wired"
        };
        let printed = writeln!(
            out,
            "{}  {} ({link}, {:04x}:{:04x} interface {})",
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

fn main() -> ExitCode {
    let mut args = std::env::args().skip(1).peekable();
    let command = args
        .next_if(|c| !c.starts_with('-'))
        .unwrap_or_else(|| "battery".to_owned());

    let mut output = Output {
        format: Format::Text,
        low_threshold: 20,
    };
    let mut interval = Duration::from_secs(60);
    // `Some(None)`: --demo without a model name.
    let mut demo: Option<Option<String>> = None;
    let mut desktop: Option<String> = None;
    let mut remove = false;
    let mut force = false;
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--demo" => demo = Some(args.next_if(|v| !v.starts_with('-'))),
            "--json" => output.format = Format::Json,
            "--remove" => remove = true,
            "--force" => force = true,
            "--format" => match args.next().as_deref().and_then(Format::parse) {
                Some(format) => output.format = format,
                None => {
                    eprintln!("--format needs one of: text, json, waybar");
                    return ExitCode::FAILURE;
                }
            },
            "--low-threshold" => match args.next().and_then(|v| v.parse::<u8>().ok()) {
                Some(pct) if pct <= 100 => output.low_threshold = pct,
                _ => {
                    eprintln!("--low-threshold needs a percentage from 0 to 100");
                    return ExitCode::FAILURE;
                }
            },
            "--interval" => match args.next().and_then(|v| v.parse::<u64>().ok()) {
                Some(secs) if (1..=MAX_INTERVAL_SECS).contains(&secs) => {
                    interval = Duration::from_secs(secs);
                }
                _ => {
                    eprintln!("--interval needs a number of seconds from 1 to {MAX_INTERVAL_SECS}");
                    return ExitCode::FAILURE;
                }
            },
            "-h" | "--help" => {
                print!("{USAGE}");
                return ExitCode::SUCCESS;
            }
            "-V" | "--version" => {
                println!("rogctl {}", env!("CARGO_PKG_VERSION"));
                return ExitCode::SUCCESS;
            }
            other if command == "configure" && desktop.is_none() && !other.starts_with('-') => {
                desktop = Some(other.to_owned());
            }
            other => {
                eprintln!("unknown argument: {other}\n\n{USAGE}");
                return ExitCode::FAILURE;
            }
        }
    }

    // `tui` was the command's first name; it still works, undocumented.
    let settings = matches!(command.as_str(), "settings" | "tui");
    if demo.is_some() && !settings {
        eprintln!("--demo only works with `rogctl settings`");
        return ExitCode::FAILURE;
    }
    if (remove || force) && command != "configure" {
        eprintln!("--remove and --force only work with `rogctl configure`");
        return ExitCode::FAILURE;
    }

    match command.as_str() {
        "settings" | "tui" => tui::run(demo),
        "battery" => cmd_battery(output),
        "watch" => cmd_watch(interval, output),
        "list" => cmd_list(),
        "configure" => configure::run(desktop.as_deref(), remove, force),
        "help" => {
            print!("{USAGE}");
            ExitCode::SUCCESS
        }
        other => {
            eprintln!("unknown command: {other}\n\n{USAGE}");
            ExitCode::FAILURE
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
        let start = Instant::now();
        wait_or_refresh(&set, Duration::from_millis(50));
        assert!(start.elapsed() >= Duration::from_millis(50));

        // SAFETY: signals this thread, which has SIGUSR1 blocked.
        unsafe { libc::pthread_kill(libc::pthread_self(), libc::SIGUSR1) };
        let start = Instant::now();
        wait_or_refresh(&set, Duration::from_secs(10));
        assert!(start.elapsed() < Duration::from_secs(1));
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
        assert_eq!(next_wait(State::Asleep, interval, &mut retry), ERROR_RETRY);
        assert_eq!(
            next_wait(State::Error, interval, &mut retry),
            2 * ERROR_RETRY
        );
        assert_eq!(
            next_wait(State::Disconnected, interval, &mut retry),
            interval
        );

        // Never waits longer than a short interval.
        let short = Duration::from_secs(2);
        assert_eq!(next_wait(State::Error, short, &mut retry), short);
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
