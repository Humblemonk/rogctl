mod device;
mod models;

use std::io::{self, Write};
use std::process::ExitCode;
use std::thread;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde::Serialize;

use device::{Device, PowerOff, QueryError};

const USAGE: &str = "\
rogctl - battery status for ASUS mice

Usage:
  rogctl [battery] [--json]              Print the battery status once
  rogctl watch [--interval SECS] [--json]
                                         Print the status every SECS seconds (default 60)
  rogctl list                            List detected devices and their hidraw nodes

Exit status of `battery`: 0 connected, 1 error, 2 no device, 3 mouse asleep.
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
            Ok(r) if r.battery > 0 => {
                return Status {
                    battery: Some(r.battery),
                    charging: r.charging,
                    low_battery_warning: r.settings.map(|s| s.low_battery_warning),
                    power_off_minutes: r.settings.and_then(|s| match s.power_off {
                        PowerOff::Minutes(m) => Some(m),
                        PowerOff::Never | PowerOff::Unknown(_) => None,
                    }),
                    battery_updated: Some(now()),
                    ..Status::for_device(State::Connected, dev)
                };
            }
            Ok(_) | Err(QueryError::Empty | QueryError::Timeout) => {
                Status::for_device(State::Asleep, dev)
            }
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

fn print_human(s: &Status) {
    let name = s.device.unwrap_or("mouse");
    match s.state {
        State::Disconnected => println!("No supported mouse found"),
        State::Error => println!("{name}: {}", s.error.as_deref().unwrap_or("error")),
        State::Asleep => println!("{name}: asleep or out of range"),
        State::Connected => {
            let pct = s.battery.unwrap_or(0);
            let charging = if s.charging { " (charging)" } else { "" };
            println!("{name}: {pct}%{charging}");
        }
    }
}

fn print_json(s: &Status) -> io::Result<()> {
    let mut out = io::stdout().lock();
    serde_json::to_writer(&mut out, s)?;
    writeln!(out)?;
    out.flush()
}

fn cmd_battery(json: bool) -> ExitCode {
    let status = poll();
    if json {
        if print_json(&status).is_err() {
            return ExitCode::FAILURE;
        }
    } else {
        print_human(&status);
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

fn cmd_watch(interval: Duration, json: bool) -> ExitCode {
    let mut last_known: Option<Status> = None;
    loop {
        let mut status = poll();
        carry_over(&mut status, &mut last_known);
        let ok = if json {
            print_json(&status).is_ok()
        } else {
            print_human(&status);
            true
        };
        // stdout closed: the bar went away, so stop.
        if !ok {
            return ExitCode::SUCCESS;
        }
        thread::sleep(interval);
    }
}

fn cmd_list() -> ExitCode {
    let devices = device::discover();
    if devices.is_empty() {
        println!("No supported mouse found");
        return ExitCode::from(2);
    }
    for dev in devices {
        let link = if dev.model.wireless {
            "wireless"
        } else {
            "wired"
        };
        println!(
            "{}  {} ({link}, {:04x}:{:04x} interface {})",
            dev.hidraw.display(),
            dev.model.name,
            0x0b05,
            dev.model.pid,
            dev.model.interface
        );
    }
    ExitCode::SUCCESS
}

fn main() -> ExitCode {
    let mut args = std::env::args().skip(1).peekable();
    let command = match args.peek().map(String::as_str) {
        Some(c) if !c.starts_with('-') => args.next().unwrap(),
        _ => "battery".to_owned(),
    };

    let mut json = false;
    let mut interval = Duration::from_secs(60);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--json" => json = true,
            "--interval" => match args.next().and_then(|v| v.parse::<u64>().ok()) {
                Some(secs) if secs > 0 => interval = Duration::from_secs(secs),
                _ => {
                    eprintln!("--interval needs a positive number of seconds");
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
            other => {
                eprintln!("unknown argument: {other}\n\n{USAGE}");
                return ExitCode::FAILURE;
            }
        }
    }

    match command.as_str() {
        "battery" => cmd_battery(json),
        "watch" => cmd_watch(interval, json),
        "list" => cmd_list(),
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

    /// The Noctalia plugin reads these field names, expects snake_case states,
    /// and treats missing fields as nil, so `None` must be omitted, not null.
    #[test]
    fn json_matches_the_plugin_contract() {
        let json = serde_json::to_value(connected("Mouse", 80)).unwrap();
        assert_eq!(json["state"], "connected");
        assert_eq!(json["device"], "Mouse");
        assert_eq!(json["battery"], 80);
        assert_eq!(json["charging"], false);
        assert_eq!(json["battery_updated"], 1000);
        assert!(json.get("error").is_none());

        let json = serde_json::to_value(Status::empty(State::Disconnected)).unwrap();
        assert_eq!(json.as_object().unwrap().len(), 2, "{json}");
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
    fn does_not_carry_level_to_another_mouse() {
        let mut last = None;
        carry_over(&mut connected("Mouse A", 80), &mut last);

        let mut status = asleep("Mouse B");
        carry_over(&mut status, &mut last);
        assert_eq!(status.battery, None);
    }
}
