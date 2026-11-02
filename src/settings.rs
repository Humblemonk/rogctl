//! `rogctl settings get`, `set`, `export` and `import`: the mouse's settings
//! from the command line. Every change is checked by `App::change()`, as the
//! settings screen's are, and sent by `Device::apply()`. `get` and `export`
//! only read.

use std::fs;
use std::io::{self, Read};
use std::path::Path;

use serde::de::{MapAccess, Visitor};
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use serde_json::Value as Json;

use crate::app::{self, App, Control, Link, Planned, Refusal, Value};
use crate::device::{self, ApplyError, Device, Outcome, QueryError};
use crate::protocol::{Effect, LiftOff, MouseSettings, PowerOff};

/// The `version` of the format `get --json` and `export` write and `import`
/// reads. Raise it only for a change older versions would misread.
const FORMAT_VERSION: u64 = 1;

const ACTIVE_STAGE: &str = "active-stage";

/// The exit codes, as `rogctl battery`'s.
const EXIT_ERROR: u8 = 1;
const EXIT_NO_MOUSE: u8 = 2;
const EXIT_ASLEEP: u8 = 3;

/// What follows `rogctl settings`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Command {
    Get {
        json: bool,
    },
    Set(Vec<String>),
    /// To the file, or stdout for None or `-`.
    Export(Option<String>),
    /// From the file, or stdin for `-`.
    Import(String),
}

/// `demo`: `Some` runs without a mouse, as the named model or the default.
/// `dry_run`: `set` and `import` say what they'd change and send nothing.
pub fn run(command: Command, demo: Option<Option<&str>>, dry_run: bool) -> u8 {
    let (mut app, device) = match load(demo) {
        Ok(loaded) => loaded,
        Err((code, text)) => {
            eprint!("{text}");
            return code;
        }
    };
    let result = match command {
        Command::Get { json: false } => {
            print!("{}", get_text(&app));
            Ok(0)
        }
        Command::Get { json: true } => to_json(&app, Form::Line).map(|text| {
            println!("{text}");
            0
        }),
        Command::Export(file) => {
            to_json(&app, Form::File).and_then(|text| export(&app, file, &text).map(|()| 0))
        }
        Command::Set(args) => parse_args(&app, &args)
            .map(|wanted| apply_all(&mut app, send(device.as_ref()), wanted, dry_run, Kind::Set)),
        Command::Import(file) => read_file(&file)
            .and_then(|text| parse_file(&app, &file, &text))
            .map(|(wanted, skipped)| {
                let kind = Kind::Import { skipped };
                apply_all(&mut app, send(device.as_ref()), wanted, dry_run, kind)
            }),
    };
    result.unwrap_or_else(|text| {
        eprintln!("{text}");
        EXIT_ERROR
    })
}

// ---------------------------------------------------------------------------
// Finding the mouse
// ---------------------------------------------------------------------------

/// The settings of the first mouse that answers, as the settings screen
/// picks it, and its device; no device in demo mode. Or the exit code and
/// message to stop with.
fn load(demo: Option<Option<&str>>) -> Result<(App, Option<Device>), (u8, String)> {
    if let Some(name) = demo {
        let model = app::demo_model(name).map_err(|e| (EXIT_ERROR, e))?;
        let (reading, settings) = app::demo_state(&model.features);
        let mut app = App::new(true);
        app.show(Some(model), Link::Connected, Some(reading), settings);
        return Ok((app, None));
    }
    let devices = device::discover_mice();
    let Some((i, outcome)) = device::first_answering(&devices, 0..devices.len()) else {
        return Err((EXIT_NO_MOUSE, "No supported mouse found\n".into()));
    };
    let device = devices
        .into_iter()
        .nth(i)
        .ok_or((EXIT_NO_MOUSE, String::new()))?;
    let name = device.model.name;
    match outcome {
        Ok(Outcome::Awake(reading, settings)) => {
            let mut app = App::new(false);
            app.show(Some(device.model), Link::Connected, Some(reading), settings);
            Ok((app, Some(device)))
        }
        Ok(Outcome::Asleep) => Err((EXIT_ASLEEP, format!("{name}: asleep or out of range\n"))),
        Err(e) if e.means_asleep(device.model.wireless) => {
            Err((EXIT_ASLEEP, format!("{name}: asleep or out of range\n")))
        }
        Err(e) => Err((
            EXIT_ERROR,
            format!("{name}: {}: {e}\n", device.hidraw.display()),
        )),
    }
}

// ---------------------------------------------------------------------------
// Settings and their names
// ---------------------------------------------------------------------------

/// One setting `get` shows and `set` takes: a row of the settings screen, or
/// which DPI stage is active.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Setting {
    Control(Control),
    ActiveStage,
}

impl Setting {
    fn key(self) -> String {
        match self {
            Self::Control(control) => control.key(),
            Self::ActiveStage => ACTIVE_STAGE.into(),
        }
    }
}

/// A value asked for, not yet checked against the mouse.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Wanted {
    Set(Control, Value),
    /// Index into the stages.
    Activate(usize),
}

/// The mouse's settings in row order, the active stage after the stages,
/// each with why it can't be changed now, if it can't.
fn settings_of(app: &App) -> Vec<(Setting, Option<&'static str>)> {
    let rows = app.rows();
    let stages = rows
        .iter()
        .take_while(|r| matches!(r.control, Control::Stage(_)))
        .count();
    let row = |r: &app::Row| (Setting::Control(r.control), r.locked);
    let mut list: Vec<_> = rows.iter().take(stages).map(row).collect();
    // Shown even when the reply named no stage, so one can still be picked.
    if app.settings.stages.len() > 1 {
        let switchable = app.model.is_some_and(|m| m.features.dpi.switchable);
        let locked = (!switchable).then_some("only the mouse's DPI button changes it");
        list.push((Setting::ActiveStage, locked));
    }
    list.extend(rows.iter().skip(stages).map(row));
    list
}

/// The setting called `key` on this mouse, or why there's none.
fn find(app: &App, key: &str) -> Result<Setting, String> {
    if let Some(setting) = lookup(app, key) {
        return Ok(setting);
    }
    let name = app.model.map_or("mouse", |m| m.name);
    Err(if model_has(app, key) {
        format!("The {name} didn't report {key} this time: try again")
    } else if is_known_key(key) {
        format!("This {name} doesn't have {key}")
    } else {
        format!("Unknown setting {key}: `rogctl settings get` lists this mouse's")
    })
}

/// The setting called `key` among those the mouse reported.
fn lookup(app: &App, key: &str) -> Option<Setting> {
    settings_of(app)
        .into_iter()
        .map(|(s, _)| s)
        .find(|s| s.key() == key)
}

/// Whether the mouse's model has the setting `key`, read or not: its demo
/// settings show everything its `Features` have
/// (`demo_shows_every_setting_the_model_has`).
fn model_has(app: &App, key: &str) -> bool {
    let Some(model) = app.model else {
        return false;
    };
    let (reading, settings) = app::demo_state(&model.features);
    let mut full = App::new(true);
    full.show(Some(model), Link::Connected, Some(reading), settings);
    lookup(&full, key).is_some()
}

/// Whether `key` names a setting some mouse has.
fn is_known_key(key: &str) -> bool {
    key == ACTIVE_STAGE
        || Control::FIXED.iter().any(|c| c.key() == key)
        || key
            .strip_prefix("dpi-")
            .and_then(|n| n.parse::<usize>().ok())
            .is_some_and(|n| n >= 1)
}

/// What `get` shows for `setting`: as the settings screen does.
fn shown(app: &App, setting: Setting) -> Option<String> {
    match setting {
        Setting::Control(control) => app.value(control).map(|v| v.to_string()),
        Setting::ActiveStage => app.settings.active_stage.map(|i| (i + 1).to_string()),
    }
}

/// A value as `get --json` and `export` write it: a number, a switch's
/// boolean, or a lowercase name. None for a code rogctl can't name, which
/// `import` couldn't send back.
fn json_value(value: Value) -> Option<Json> {
    let name = |value: Value| {
        Some(Json::from(
            value.to_string().to_lowercase().replace(' ', "-"),
        ))
    };
    match value {
        Value::Dpi(dpi) => Some(dpi.into()),
        Value::PollingRate(rate) => Some(rate.hz().into()),
        Value::Debounce(d) => d.is_known().then(|| d.ms().into()),
        Value::Switch(on) => Some(on.into()),
        Value::Degrees(d) => Some(d.into()),
        Value::LiftOff(LiftOff::Unknown(_)) => None,
        Value::LiftOff(LiftOff::Low | LiftOff::High) => name(value),
        Value::PowerOff(p) if p == PowerOff::NEVER => Some("never".into()),
        Value::PowerOff(p) => p.minutes().map(Json::from),
        Value::Percent(pct) | Value::Brightness(pct) => Some(pct.into()),
        Value::Effect(Effect::Unknown(_)) => None,
        Value::Effect(_) | Value::Color(_) => name(value),
    }
}

/// The text form of a value in a file or `KEY=VALUE`: a number or name.
fn json_text(json: &Json) -> String {
    match json {
        Json::String(s) => s.clone(),
        other => other.to_string(),
    }
}

/// Lowercase without spaces, hyphens or underscores, to match names loosely:
/// `1000`, `1000hz` and `1000 Hz` alike.
fn loose(text: &str) -> String {
    text.chars()
        .filter(|c| !matches!(c, ' ' | '-' | '_'))
        .flat_map(char::to_lowercase)
        .collect()
}

/// `text` as a value for `setting`. Only its form is checked here; whether
/// the mouse takes it, `App::change()` decides.
fn parse_value(app: &App, setting: Setting, text: &str) -> Result<Wanted, String> {
    let key = setting.key();
    let text = text.trim();
    let control = match setting {
        Setting::ActiveStage => {
            return match text.parse::<usize>() {
                Ok(n) if n >= 1 => Ok(Wanted::Activate(n - 1)),
                _ => Err(format!("{key} takes a stage number, such as 2")),
            };
        }
        Setting::Control(control) => control,
    };
    let value = match control {
        Control::Stage(_) => text
            .parse()
            .map(Value::Dpi)
            .map_err(|_| format!("{key} takes a DPI, such as 800"))?,
        Control::AngleTuning => text
            .trim_end_matches('°')
            .parse()
            .map(Value::Degrees)
            .map_err(|_| format!("{key} takes a number of degrees, such as -5"))?,
        Control::LightingColor if text.starts_with('#') => {
            parse_hex(text).ok_or_else(|| format!("{key} takes a colour name or #rrggbb"))?
        }
        _ => {
            // The mouse's own value too, should another program have set one
            // outside the list: `App::change()` finds it unchanged.
            let mut named = app.named_values(control);
            named.extend(app.value(control).filter(|v| !named.contains(v)));
            let wanted = loose(text);
            named
                .iter()
                .copied()
                .find(|&v| {
                    loose(&v.to_string()) == wanted
                        || json_value(v).is_some_and(|j| loose(&json_text(&j)) == wanted)
                })
                .ok_or_else(|| {
                    // Only rogctl's choices: the mouse's own is no offer.
                    let names: Vec<String> = app
                        .named_values(control)
                        .iter()
                        .filter_map(|&v| json_value(v).map(|j| json_text(&j)))
                        .collect();
                    let or_hex = if control == Control::LightingColor {
                        ", or #rrggbb"
                    } else {
                        ""
                    };
                    format!("{key} takes one of: {}{or_hex}", names.join(", "))
                })?
        }
    };
    Ok(Wanted::Set(control, value))
}

fn parse_hex(text: &str) -> Option<Value> {
    let hex = text.strip_prefix('#').filter(|h| h.len() == 6)?;
    let byte = |i: usize| u8::from_str_radix(hex.get(i..i + 2)?, 16).ok();
    Some(Value::Color([byte(0)?, byte(2)?, byte(4)?]))
}

/// The values asked for, in the order they're applied: the settings
/// screen's row order, so turning a setting on comes before what it unlocks.
fn in_order(app: &App, wanted: Vec<(Setting, Wanted)>) -> Result<Vec<(Setting, Wanted)>, String> {
    let order: Vec<Setting> = settings_of(app).into_iter().map(|(s, _)| s).collect();
    let mut sorted: Vec<(usize, Setting, Wanted)> = Vec::new();
    for (setting, w) in wanted {
        if sorted.iter().any(|&(_, s, _)| s == setting) {
            return Err(format!("{} is given twice", setting.key()));
        }
        let at = order
            .iter()
            .position(|&s| s == setting)
            .unwrap_or(usize::MAX);
        sorted.push((at, setting, w));
    }
    sorted.sort_by_key(|&(at, _, _)| at);
    Ok(sorted.into_iter().map(|(_, s, w)| (s, w)).collect())
}

/// `set`'s `KEY=VALUE` arguments.
fn parse_args(app: &App, args: &[String]) -> Result<Vec<(Setting, Wanted)>, String> {
    if args.is_empty() {
        return Err("rogctl settings set takes KEY=VALUE, such as polling-rate=1000".into());
    }
    let mut wanted = Vec::new();
    for arg in args {
        let (key, text) = arg.split_once('=').ok_or_else(|| {
            format!("{arg}: rogctl settings set takes KEY=VALUE, such as polling-rate=1000")
        })?;
        let setting = find(app, key.trim())?;
        wanted.push((setting, parse_value(app, setting, text)?));
    }
    in_order(app, wanted)
}

// ---------------------------------------------------------------------------
// get and export
// ---------------------------------------------------------------------------

fn get_text(app: &App) -> String {
    let name = app.model.map_or("mouse", |m| m.name);
    let demo = if app.demo { " [demo]" } else { "" };
    let mut text = format!("{name}{demo}\n");
    let list = settings_of(app);
    let width = list.iter().map(|(s, _)| s.key().len()).max().unwrap_or(0);
    for (setting, locked) in list {
        let Some(value) = shown(app, setting) else {
            continue;
        };
        let key = setting.key();
        match locked {
            Some(why) => text.push_str(&format!("{key:width$}  {value} (locked: {why})\n")),
            None => text.push_str(&format!("{key:width$}  {value}\n")),
        }
    }
    text
}

/// Keeps the keys in row order; serde_json's own map sorts them.
struct Ordered(Vec<(String, Json)>);

impl Serialize for Ordered {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.collect_map(self.0.iter().map(|(k, v)| (k, v)))
    }
}

#[derive(Serialize)]
struct File {
    version: u64,
    model: &'static str,
    settings: Ordered,
}

/// What [`to_json`] writes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Form {
    /// `get --json`: one line, every setting.
    Line,
    /// `export`: indented, and without the active DPI stage. The mouse's
    /// DPI button changes that all the time, so importing it would write to
    /// the mouse at every import.
    File,
}

/// Every setting rogctl can name, locked ones included, as JSON.
fn to_json(app: &App, form: Form) -> Result<String, String> {
    let settings = settings_of(app)
        .into_iter()
        .filter_map(|(setting, _)| {
            let json = match setting {
                Setting::Control(control) => app.value(control).and_then(json_value),
                Setting::ActiveStage if form == Form::Line => {
                    app.settings.active_stage.map(|i| (i + 1).into())
                }
                Setting::ActiveStage => None,
            }?;
            Some((setting.key(), json))
        })
        .collect();
    let file = File {
        version: FORMAT_VERSION,
        model: app.model.map_or("", |m| m.name),
        settings: Ordered(settings),
    };
    let text = match form {
        Form::File => serde_json::to_string_pretty(&file),
        Form::Line => serde_json::to_string(&file),
    };
    text.map_err(|e| e.to_string())
}

fn export(app: &App, file: Option<String>, text: &str) -> Result<(), String> {
    match file.as_deref() {
        None | Some("-") => {
            println!("{text}");
            Ok(())
        }
        Some(path) => {
            fs::write(path, format!("{text}\n"))
                .map_err(|e| format!("Couldn't write {path}: {e}"))?;
            let name = app.model.map_or("mouse", |m| m.name);
            println!("Saved the {name}'s settings to {path}");
            Ok(())
        }
    }
}

// ---------------------------------------------------------------------------
// import
// ---------------------------------------------------------------------------

fn read_file(path: &str) -> Result<String, String> {
    if path == "-" {
        let mut text = String::new();
        io::stdin()
            .read_to_string(&mut text)
            .map_err(|e| format!("Couldn't read stdin: {e}"))?;
        return Ok(text);
    }
    fs::read_to_string(path).map_err(|e| format!("Couldn't read {path}: {e}"))
}

/// A file's `settings` in the order written, with any key given twice, which
/// serde_json's own map would quietly drop.
struct Entries(Vec<(String, Json)>);

impl<'de> Deserialize<'de> for Entries {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct Visit;
        impl<'de> Visitor<'de> for Visit {
            type Value = Entries;
            fn expecting(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                f.write_str("an object of settings")
            }
            fn visit_map<M: MapAccess<'de>>(self, mut map: M) -> Result<Entries, M::Error> {
                let mut entries = Vec::new();
                while let Some(entry) = map.next_entry()? {
                    entries.push(entry);
                }
                Ok(Entries(entries))
            }
        }
        deserializer.deserialize_map(Visit)
    }
}

/// What `import` reads; anything else in the file is ignored.
#[derive(Deserialize)]
struct Imported {
    version: Option<u64>,
    model: Option<String>,
    settings: Option<Entries>,
}

/// An exported file's settings, checked for form before anything is sent:
/// the same model, and only settings it has, as `set` would take them. A
/// setting this model has but the mouse didn't report now (lighting that
/// didn't read) is skipped with a note: true if one was.
fn parse_file(app: &App, path: &str, text: &str) -> Result<(Vec<(Setting, Wanted)>, bool), String> {
    let path = if path == "-" { "stdin" } else { path };
    let not_ours = || format!("{path} isn't a file from `rogctl settings export`");
    let file: Imported = serde_json::from_str(text).map_err(|e| format!("{path}: {e}"))?;
    match file.version {
        Some(FORMAT_VERSION) => {}
        Some(version) if version > FORMAT_VERSION => {
            return Err(format!(
                "{path} is in format {version}, from a newer rogctl; this one reads \
                 {FORMAT_VERSION}"
            ));
        }
        Some(_) | None => return Err(not_ours()),
    }
    let model = file.model.ok_or_else(not_ours)?;
    let name = app.model.map_or("", |m| m.name);
    if model != name {
        return Err(format!(
            "{path} has a {model}'s settings, and this is a {name}: rogctl only imports \
             settings onto the same model"
        ));
    }
    let mut wanted = Vec::new();
    let mut skipped = false;
    for (key, value) in file.settings.ok_or_else(not_ours)?.0 {
        let setting = match find(app, &key) {
            Ok(setting) => setting,
            // The model has it, so the mouse just didn't report it.
            Err(_) if model_has(app, &key) => {
                eprintln!("Skipped: {key}: the {name} didn't report it this time");
                skipped = true;
                continue;
            }
            Err(e) => return Err(format!("{path}: {e}")),
        };
        let text = match &value {
            Json::String(_) | Json::Number(_) | Json::Bool(_) => json_text(&value),
            Json::Null | Json::Array(_) | Json::Object(_) => {
                return Err(format!("{path}: {key} isn't a number or name"));
            }
        };
        let value = parse_value(app, setting, &text).map_err(|e| format!("{path}: {e}"))?;
        wanted.push((setting, value));
    }
    let wanted = in_order(app, wanted).map_err(|e| format!("{path}: {e}"))?;
    Ok((wanted, skipped))
}

// ---------------------------------------------------------------------------
// Applying
// ---------------------------------------------------------------------------

fn plan(app: &App, wanted: Wanted) -> Result<Planned, Refusal> {
    match wanted {
        Wanted::Set(control, value) => app.change(control, value),
        Wanted::Activate(stage) => app.activate(stage),
    }
}

/// Makes each change in turn, after checking all of them against a copy of
/// the settings first, so a value the mouse won't take stops `set` or
/// `import` before anything is sent. `import` skips settings that can't be
/// changed now (a locked row, an effect rogctl doesn't send) and keeps
/// quiet about those already right; `set` refuses the first and says the
/// second. Each setting is planned again from what the change before it read
/// back, so one an earlier change altered is set back. A setting refused only
/// then stops `set` (exit 1) and is skipped by `import`; one the read-back
/// left out (lighting that didn't read) stops either if it was due, else is
/// noted. After each change the mouse must read back what was asked, or it
/// stops with 1. If the mouse stops answering, a change still due stops it (3
/// asleep, 1 a read error); with only settings already right left, it notes
/// they couldn't be checked again and exits 0. Exits as `rogctl battery`
/// does; a change made but whose save the mouse didn't confirm still exits
/// 0, with a warning. `send` makes one change: `send()` below.
fn apply_all(
    app: &mut App,
    mut send: impl FnMut(&mut App, &Planned) -> Result<Option<String>, u8>,
    wanted: Vec<(Setting, Wanted)>,
    dry_run: bool,
    kind: Kind,
) -> u8 {
    let (import, skipped_before) = match kind {
        Kind::Set => (false, false),
        Kind::Import { skipped } => (true, skipped),
    };
    let mut trial = app.clone();
    let mut checks = Vec::new();
    for &(setting, w) in &wanted {
        let planned = plan(&trial, w);
        checks.push(match planned {
            Ok(planned) => {
                let before = shown(&trial, setting).unwrap_or_default();
                trial.simulate(&planned.change);
                if dry_run {
                    let after = shown(&trial, setting).unwrap_or_default();
                    println!("Would change {}: {before} → {after}", setting.key());
                }
                Check::Due
            }
            Err(Refusal::Unchanged(text)) => {
                if !import {
                    println!("{text}");
                }
                Check::AlreadySet
            }
            Err(Refusal::NotNow(text)) if import => {
                eprintln!("Skipped: {text}");
                Check::Skipped
            }
            Err(Refusal::NotNow(text) | Refusal::Invalid(text)) => {
                eprintln!("{text}\nNothing was changed.");
                return EXIT_ERROR;
            }
        });
    }
    if !checks.contains(&Check::Due) {
        if import && !skipped_before && !checks.contains(&Check::Skipped) {
            println!("Nothing to change: the mouse already has these settings");
        }
        return 0;
    }
    if dry_run {
        println!("Dry run: nothing was sent to the mouse.");
        return 0;
    }
    let mut made = 0;
    for (i, &(setting, w)) in wanted.iter().enumerate() {
        let check = checks[i];
        // Noted in the check above, and left so.
        if check == Check::Skipped {
            continue;
        }
        // Each is planned again from what the last change read back, which
        // needs the mouse: a change can change another setting with it.
        if app.link != Link::Connected {
            let (why, code) = lost(&app.link);
            if checks[i..].contains(&Check::Due) {
                eprintln!("Stopped: {why}");
                return stopped(made, code);
            }
            eprintln!("Couldn't check the rest again after the last change: {why}");
            return 0;
        }
        let key = setting.key();
        // The read-back may leave a setting out (lighting that didn't read).
        if !settings_of(app).iter().any(|&(s, _)| s == setting) {
            if check == Check::Due {
                eprintln!(
                    "Couldn't change {key}: the mouse didn't report it after the last change"
                );
                return stopped(made, EXIT_ERROR);
            }
            eprintln!(
                "Couldn't check {key} again: the mouse didn't report it after the last change"
            );
            continue;
        }
        let planned = match (plan(app, w), check) {
            (Ok(planned), _) => planned,
            (Err(Refusal::Unchanged(text)), Check::Due) if !import => {
                println!("{text}");
                continue;
            }
            (Err(Refusal::Unchanged(_)), _) => continue,
            (Err(Refusal::NotNow(text)), Check::Due)
            | (Err(Refusal::NotNow(text) | Refusal::Invalid(text)), Check::AlreadySet)
                if import =>
            {
                eprintln!("Skipped: {text}");
                continue;
            }
            // Refused only now: the mouse read back differs from what was
            // checked, or an earlier change altered a setting that was right.
            (Err(Refusal::NotNow(text) | Refusal::Invalid(text)), _) => {
                eprintln!("{text}");
                return stopped(made, EXIT_ERROR);
            }
        };
        let unsaved = match send(app, &planned) {
            Ok(unsaved) => unsaved,
            Err(code) => return stopped(made, code),
        };
        // The mouse echoed the change; check it has it, as the settings
        // screen shows what's read back. An encoding rogctl has wrong for a
        // mouse (untested ones) shows up here.
        // Has a value for it: the active stage can be unknown.
        let reported = shown(app, setting).is_some();
        made += 1;
        if app.link == Link::Connected
            && reported
            && !matches!(plan(app, w), Err(Refusal::Unchanged(_)))
        {
            let now = shown(app, setting).unwrap_or_default();
            let unconfirmed = if unsaved.is_some() {
                " (and didn't confirm saving it)"
            } else {
                ""
            };
            eprintln!("The mouse took the change{unconfirmed}, but {key} reads back {now}");
            return stopped(made, EXIT_ERROR);
        }
        let demo = if app.demo { "Demo: " } else { "" };
        match &unsaved {
            Some(warning) => eprintln!("{demo}{warning}"),
            None => println!("{demo}{}", planned.done),
        }
        // An unsaved warning already says the mouse went to sleep.
        if app.link != Link::Connected && !(unsaved.is_some() && app.link == Link::Asleep) {
            let (why, _) = lost(&app.link);
            eprintln!("Couldn't check {key} after the change: {why}");
        } else if app.link == Link::Connected && !reported {
            eprintln!("Couldn't check {key}: the mouse didn't report it after the change");
        }
    }
    0
}

/// Which command `apply_all` runs for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Kind {
    Set,
    /// `skipped`: reading the file skipped a setting, and said so.
    Import {
        skipped: bool,
    },
}

/// What checking a wanted value against a copy of the settings found.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Check {
    Due,
    AlreadySet,
    /// `import` only: it can't be changed now, and said so.
    Skipped,
}

/// Why the mouse can't be read, and the exit code for it.
fn lost(link: &Link) -> (String, u8) {
    match link {
        Link::Error(e) => (format!("couldn't read the mouse back: {e}"), EXIT_ERROR),
        Link::Asleep | Link::Disconnected | Link::Connected => (
            "the mouse went to sleep or out of range".to_owned(),
            EXIT_ASLEEP,
        ),
    }
}

fn stopped(made: usize, code: u8) -> u8 {
    match made {
        0 => {}
        1 => eprintln!("Made 1 change before stopping."),
        n => eprintln!("Made {n} changes before stopping."),
    }
    code
}

/// How `apply_all` makes a change: on the mouse, or in demo mode (no
/// device) only in `app`. Ok with the warning to give if the mouse didn't
/// confirm saving it; Err with the exit code if it wasn't made.
fn send(
    device: Option<&Device>,
) -> impl FnMut(&mut App, &Planned) -> Result<Option<String>, u8> + '_ {
    move |app, planned| match device {
        Some(device) => apply_one(app, device, planned),
        None => {
            app.simulate(&planned.change);
            Ok(None)
        }
    }
}

/// Sends one change, then shows what the mouse read back in `app`. As
/// [`send`] returns.
fn apply_one(app: &mut App, device: &Device, planned: &Planned) -> Result<Option<String>, u8> {
    let wireless = device.model.wireless;
    let applied = device.apply(&planned.change);
    let model = Some(device.model);
    match applied.read_back {
        Some(Ok(Outcome::Awake(reading, settings))) => {
            app.show(model, Link::Connected, Some(reading), settings);
        }
        Some(Ok(Outcome::Asleep)) => app.show(model, Link::Asleep, None, MouseSettings::default()),
        Some(Err(e)) if e.means_asleep(wireless) => {
            app.show(model, Link::Asleep, None, MouseSettings::default());
        }
        Some(Err(e)) => {
            let error = format!("{}: {e}", device.hidraw.display());
            app.show(model, Link::Error(error), None, MouseSettings::default());
        }
        None => {}
    }
    let asleep = app.link == Link::Asleep;
    match describe(
        &applied.result,
        &planned.done,
        wireless,
        &device.hidraw,
        asleep,
    ) {
        Report::Done(_) => Ok(None),
        Report::Unsaved(text) => Ok(Some(text)),
        Report::Failed { text, asleep } => {
            eprintln!("{text}");
            Err(if asleep { EXIT_ASLEEP } else { EXIT_ERROR })
        }
    }
}

/// How a change went, in words, for the settings screen and the CLI alike.
pub enum Report {
    Done(String),
    /// Made, but the mouse didn't confirm saving it.
    Unsaved(String),
    /// Not made; `asleep` when the mouse didn't answer.
    Failed {
        text: String,
        asleep: bool,
    },
}

/// `done` says what was changed; `asleep_after`, that the read-back after
/// it found the mouse asleep.
pub fn describe(
    result: &Result<(), ApplyError>,
    done: &str,
    wireless: bool,
    hidraw: &Path,
    asleep_after: bool,
) -> Report {
    let failed = |text: String, asleep: bool| Report::Failed { text, asleep };
    match result {
        Ok(()) => Report::Done(done.to_owned()),
        Err(ApplyError::Replaced) => failed(
            "No change made: the mouse on this port changed or didn't answer".into(),
            false,
        ),
        Err(ApplyError::Open(e)) => {
            failed(format!("Couldn't open {}: {e}", hidraw.display()), false)
        }
        Err(ApplyError::NotChanged(e)) if e.means_asleep(wireless) => failed(
            format!("No change made: the mouse didn't answer ({e})"),
            true,
        ),
        Err(ApplyError::NotChanged(e)) => failed(format!("No change made: {e}"), false),
        // The read-back found the mouse asleep: that, not the save, is what
        // the user can act on.
        Err(ApplyError::NotSaved(_)) if asleep_after => Report::Unsaved(format!(
            "{done}, but the mouse went to sleep or out of range before confirming the save"
        )),
        // A Harpe II Ace keeps such a change (protocol.md); another mouse may
        // not.
        Err(ApplyError::NotSaved(QueryError::Rejected)) => Report::Unsaved(format!(
            "{done}, but the mouse didn't confirm saving it: it may not last a power cycle"
        )),
        Err(ApplyError::NotSaved(e)) => {
            Report::Unsaved(format!("{done}, but not saved on the mouse: {e}"))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::MODELS;
    use crate::protocol::PollingRate;

    fn demo(name: &str) -> App {
        load(Some(Some(name))).unwrap().0
    }

    fn wanted(app: &App, args: &[&str]) -> Result<Vec<(Setting, Wanted)>, String> {
        let args: Vec<String> = args.iter().map(|&a| a.to_owned()).collect();
        parse_args(app, &args)
    }

    fn value(app: &App, key: &str, text: &str) -> Result<Wanted, String> {
        parse_value(app, find(app, key)?, text)
    }

    #[test]
    fn values_are_taken_in_several_forms() {
        let app = demo(app::DEMO_MODEL);
        let rate = Ok(Wanted::Set(
            Control::PollingRate,
            Value::PollingRate(PollingRate::HZ_1000),
        ));
        for text in ["1000", "1000hz", "1000 Hz", " 1000 "] {
            assert_eq!(value(&app, "polling-rate", text), rate, "{text}");
        }
        let on = Ok(Wanted::Set(Control::MotionSync, Value::Switch(true)));
        assert_eq!(value(&app, "motion-sync", "on"), on);
        assert_eq!(value(&app, "motion-sync", "true"), on);
        assert_eq!(
            value(&app, "power-off", "never"),
            Ok(Wanted::Set(
                Control::PowerOff,
                Value::PowerOff(PowerOff::NEVER)
            ))
        );
        let cycle = Ok(Wanted::Set(
            Control::LightingEffect,
            Value::Effect(Effect::ColorCycle),
        ));
        assert_eq!(value(&app, "effect", "colour-cycle"), cycle);
        assert_eq!(value(&app, "effect", "Colour cycle"), cycle);
        assert_eq!(
            value(&app, "colour", "#00FF80"),
            Ok(Wanted::Set(
                Control::LightingColor,
                Value::Color([0, 0xff, 0x80])
            ))
        );
        assert_eq!(
            value(&app, "colour", "blue"),
            Ok(Wanted::Set(
                Control::LightingColor,
                Value::Color([0, 0, 0xff])
            ))
        );
        assert_eq!(
            value(&app, "angle-tuning", "-5"),
            Ok(Wanted::Set(Control::AngleTuning, Value::Degrees(-5)))
        );
        assert_eq!(
            value(&app, "low-battery-warning", "off"),
            Ok(Wanted::Set(Control::LowBatteryWarning, Value::Percent(0)))
        );
        assert_eq!(value(&app, "active-stage", "3"), Ok(Wanted::Activate(2)));
        assert_eq!(
            value(&app, "polling-rate", "fast"),
            Err("polling-rate takes one of: 125, 250, 500, 1000, 2000, 4000, 8000".into())
        );
        assert!(value(&app, "colour", "#12345").is_err());
        assert!(value(&app, "active-stage", "0").is_err());
        assert!(value(&app, "dpi-1", "lots").is_err());
    }

    #[test]
    fn keys_name_this_mouses_settings() {
        let app = demo(app::DEMO_MODEL);
        assert_eq!(
            find(&app, "debounce"),
            Err("This ROG Harpe II Ace doesn't have debounce".into())
        );
        assert_eq!(
            find(&app, "dpi-5"),
            Err("This ROG Harpe II Ace doesn't have dpi-5".into())
        );
        assert!(
            find(&app, "speed")
                .unwrap_err()
                .starts_with("Unknown setting speed")
        );
        let mut unlit = app.clone();
        unlit.settings.lighting = None;
        assert_eq!(
            find(&unlit, "colour"),
            Err("The ROG Harpe II Ace didn't report colour this time: try again".into())
        );
        let keys: Vec<String> = settings_of(&app).iter().map(|(s, _)| s.key()).collect();
        assert_eq!(
            keys[..6],
            [
                "dpi-1",
                "dpi-2",
                "dpi-3",
                "dpi-4",
                "active-stage",
                "polling-rate"
            ]
        );
    }

    #[test]
    fn set_applies_in_row_order_and_once_per_key() {
        let app = demo(app::DEMO_MODEL);
        let order: Vec<String> = wanted(&app, &["power-saving-rate=250", "power-saving=on"])
            .unwrap()
            .iter()
            .map(|(s, _)| s.key())
            .collect();
        assert_eq!(order, ["power-saving", "power-saving-rate"]);
        assert_eq!(
            wanted(&app, &["dpi-1=800", "dpi-1=900"]),
            Err("dpi-1 is given twice".into())
        );
        assert!(wanted(&app, &["dpi-1"]).is_err());
        assert!(wanted(&app, &[]).is_err());
    }

    #[test]
    fn a_refused_value_stops_set_before_anything_is_sent() {
        let mut app = demo(app::DEMO_MODEL);
        let before = app.settings.clone();
        let list = wanted(&app, &["dpi-1=800", "dpi-2=99999"]).unwrap();
        assert_eq!(
            apply_all(&mut app, send(None), list, false, Kind::Set),
            EXIT_ERROR
        );
        assert_eq!(app.settings, before);

        let list = wanted(&app, &["power-saving-rate=250", "power-saving=on"]).unwrap();
        assert_eq!(apply_all(&mut app, send(None), list, false, Kind::Set), 0);
        let saving = app.settings.power_saving.unwrap();
        assert!(saving.on);
        assert_eq!(saving.rate, PollingRate::HZ_250);
    }

    #[test]
    fn a_dry_run_changes_nothing() {
        let mut app = demo(app::DEMO_MODEL);
        let before = app.settings.clone();
        let list = wanted(&app, &["dpi-1=800", "polling-rate=500"]).unwrap();
        assert_eq!(apply_all(&mut app, send(None), list, true, Kind::Set), 0);
        assert_eq!(app.settings, before);
    }

    /// What every model exports, it imports again without a change.
    #[test]
    fn every_model_imports_its_own_export() {
        for model in MODELS {
            let app = demo(model.name);
            let text = to_json(&app, Form::File).unwrap();
            let (list, skipped) = parse_file(&app, "file", &text).unwrap();
            assert!(!skipped);
            let exported = settings_of(&app)
                .iter()
                .filter(|&&(s, _)| s != Setting::ActiveStage)
                .count();
            assert_eq!(list.len(), exported, "{}", model.name);
            for (setting, w) in list {
                assert!(
                    matches!(plan(&app, w), Err(Refusal::Unchanged(_))),
                    "{} {}",
                    model.name,
                    setting.key()
                );
            }
        }
    }

    #[test]
    fn import_takes_only_this_models_files() {
        let app = demo(app::DEMO_MODEL);
        let file = |model: &str, version: u64, settings: &str| {
            format!(r#"{{"version":{version},"model":"{model}","settings":{{{settings}}}}}"#)
        };
        let harpe = app::DEMO_MODEL;
        assert!(parse_file(&app, "f", &file(harpe, 1, r#""dpi-1":800"#)).is_ok());
        assert!(
            parse_file(&app, "f", &file("ROG Chakram", 1, ""))
                .unwrap_err()
                .contains("only imports settings onto the same model")
        );
        assert!(
            parse_file(&app, "f", &file(harpe, 2, ""))
                .unwrap_err()
                .contains("newer rogctl")
        );
        assert!(parse_file(&app, "f", &file(harpe, 1, r#""speed":1"#)).is_err());
        assert!(
            parse_file(&app, "f", &file(harpe, 1, r#""debounce":12"#))
                .unwrap_err()
                .contains("doesn't have debounce"),
            "a setting the model lacks is refused, not skipped"
        );
        assert!(parse_file(&app, "f", &file(harpe, 1, r#""dpi-7":800"#)).is_err());
        assert!(parse_file(&app, "f", &file(harpe, 1, r#""dpi-1":[800]"#)).is_err());
        assert!(parse_file(&app, "f", r#"{"settings":{}}"#).is_err());
        assert!(
            parse_file(&app, "f", &file(harpe, 0, ""))
                .unwrap_err()
                .contains("isn't a file from")
        );
        assert_eq!(
            parse_file(&app, "f", &file(harpe, 1, r#""dpi-1":800,"dpi-1":900"#)).unwrap_err(),
            "f: dpi-1 is given twice"
        );
        assert!(parse_file(&app, "f", "not json").is_err());
    }

    #[test]
    fn import_skips_what_cant_change_now() {
        let mut app = demo(app::DEMO_MODEL);
        let text = file_with(&app, &[("effect", "\"rainbow\""), ("polling-rate", "500")]);
        let (list, _) = parse_file(&app, "f", &text).unwrap();
        assert_eq!(
            apply_all(
                &mut app,
                send(None),
                list,
                false,
                Kind::Import { skipped: false }
            ),
            0
        );
        assert_eq!(app.settings.polling_rate, Some(PollingRate::HZ_500));
    }

    /// Values another program set, outside rogctl's choices, export and
    /// import back without a change.
    #[test]
    fn values_outside_the_choices_import_unchanged() {
        let mut app = demo(app::DEMO_MODEL);
        if let Some(lighting) = app.settings.lighting.as_mut() {
            lighting.brightness = 35;
        }
        app.settings.low_battery_warning = Some(90);
        let text = to_json(&app, Form::File).unwrap();
        let (list, _) = parse_file(&app, "f", &text).unwrap();
        for (setting, w) in list {
            assert!(
                matches!(plan(&app, w), Err(Refusal::Unchanged(_))),
                "{}",
                setting.key()
            );
        }
        assert!(plan(&app, value(&app, "brightness", "40").unwrap()).is_ok());
    }

    /// A reply that names no active stage still lets one be picked.
    #[test]
    fn an_unknown_active_stage_can_be_set() {
        let mut app = demo(app::DEMO_MODEL);
        app.settings.active_stage = None;
        assert!(!get_text(&app).contains(ACTIVE_STAGE));
        let w = value(&app, ACTIVE_STAGE, "1").unwrap();
        assert_eq!(
            plan(&app, w).unwrap().change,
            crate::protocol::Change::ActiveStage(0)
        );
    }

    /// A mouse that does what's sent, plus `side_effect` after each change.
    fn mouse(
        side_effect: impl Fn(&mut App, &Planned),
    ) -> impl FnMut(&mut App, &Planned) -> Result<Option<String>, u8> {
        move |app, planned| {
            app.simulate(&planned.change);
            side_effect(app, planned);
            Ok(None)
        }
    }

    /// A mouse that echoes a change but doesn't make it: an encoding rogctl
    /// has wrong. `set` says so and fails, and stops there.
    #[test]
    fn a_change_the_mouse_doesnt_read_back_fails() {
        let mut app = demo(app::DEMO_MODEL);
        let list = wanted(&app, &["polling-rate=500", "angle-snapping=on"]).unwrap();
        let mut sent = 0;
        let ignores = |_: &mut App, _: &Planned| {
            sent += 1;
            Ok(None)
        };
        assert_eq!(
            apply_all(&mut app, ignores, list, false, Kind::Set),
            EXIT_ERROR
        );
        assert_eq!(sent, 1, "stopped after the first");
        assert_eq!(app.settings.polling_rate, Some(PollingRate::HZ_1000));
    }

    /// A reply naming no active stage, before and after picking one, can't
    /// check the change: that's noted, not called a failure.
    #[test]
    fn an_active_stage_that_doesnt_read_back_is_noted() {
        let mut app = demo(app::DEMO_MODEL);
        app.settings.active_stage = None;
        let list = wanted(&app, &["active-stage=2"]).unwrap();
        let unread = mouse(|app, _| app.settings.active_stage = None);
        assert_eq!(apply_all(&mut app, unread, list, false, Kind::Set), 0);
    }

    /// The DPI button changes the active stage, so `get --json` shows it but
    /// `export` doesn't; a file with it imports it.
    #[test]
    fn the_active_stage_is_set_but_not_exported() {
        let app = demo(app::DEMO_MODEL);
        assert!(!to_json(&app, Form::File).unwrap().contains(ACTIVE_STAGE));
        assert!(
            to_json(&app, Form::Line)
                .unwrap()
                .contains(r#""active-stage":2"#)
        );
        assert!(get_text(&app).contains(ACTIVE_STAGE));
        let text = file_with(&app, &[(ACTIVE_STAGE, "1")]);
        let (list, _) = parse_file(&app, "f", &text).unwrap();
        assert!(list.iter().any(|&(s, _)| s == Setting::ActiveStage));
    }

    /// Lighting that didn't read this time is skipped, not the whole file.
    #[test]
    fn import_skips_a_setting_the_mouse_didnt_report() {
        let mut app = demo(app::DEMO_MODEL);
        let text = to_json(&app, Form::File).unwrap();
        app.settings.lighting = None;
        let (list, skipped) = parse_file(&app, "f", &text).unwrap();
        assert!(skipped);
        assert!(
            !list
                .iter()
                .any(|&(s, _)| s == Setting::Control(Control::LightingEffect))
        );
    }

    fn on_rate(planned: &Planned) -> bool {
        matches!(planned.change, crate::protocol::Change::PollingRate(_))
    }

    /// A setting a change altered on the mouse is set back.
    #[test]
    fn a_setting_an_earlier_change_altered_is_set_back() {
        let mut app = demo(app::DEMO_MODEL);
        let list = wanted(&app, &["polling-rate=500", "angle-snapping=off"]).unwrap();
        let flips = mouse(|app, p| {
            if on_rate(p) {
                app.settings.angle_snapping = Some(true);
            }
        });
        assert_eq!(apply_all(&mut app, flips, list, false, Kind::Set), 0);
        assert_eq!(app.settings.angle_snapping, Some(false));
    }

    /// Altered to a value it can't be set back from now: `set` fails,
    /// `import` skips it.
    #[test]
    fn a_setting_altered_and_locked_fails_set_and_is_skipped_by_import() {
        // Motion sync locks at 8000 Hz; this mouse turns it off then.
        let syncless = || {
            mouse(|app, p| {
                if on_rate(p) {
                    app.settings.motion_sync = Some(false);
                }
            })
        };
        let mut app = demo(app::DEMO_MODEL);
        let list = wanted(&app, &["polling-rate=8000", "motion-sync=on"]).unwrap();
        assert_eq!(
            apply_all(&mut app, syncless(), list, false, Kind::Set),
            EXIT_ERROR
        );
        assert_eq!(app.settings.polling_rate, Some(PollingRate::HZ_8000));

        let mut app = demo(app::DEMO_MODEL);
        let text = file_with(&app, &[("polling-rate", "8000")]);
        let (list, _) = parse_file(&app, "f", &text).unwrap();
        assert_eq!(
            apply_all(
                &mut app,
                syncless(),
                list,
                false,
                Kind::Import { skipped: false }
            ),
            0
        );
    }

    /// Asleep after a change: stops if a change is still due, else says
    /// the rest couldn't be checked again.
    #[test]
    fn a_mouse_asleep_after_a_change_stops_only_with_changes_left() {
        let sleeps = || {
            mouse(|app, _| {
                app.show(app.model, Link::Asleep, None, MouseSettings::default());
            })
        };
        let mut app = demo(app::DEMO_MODEL);
        let list = wanted(&app, &["polling-rate=500", "angle-snapping=off"]).unwrap();
        assert_eq!(apply_all(&mut app, sleeps(), list, false, Kind::Set), 0);

        let mut app = demo(app::DEMO_MODEL);
        let list = wanted(&app, &["polling-rate=500", "motion-sync=off"]).unwrap();
        assert_eq!(
            apply_all(&mut app, sleeps(), list, false, Kind::Set),
            EXIT_ASLEEP
        );
    }

    /// A read error after a change, as asleep: stops with 1 if a change is
    /// still due, else exits 0.
    #[test]
    fn a_read_error_after_a_change_stops_only_with_changes_left() {
        let fails = || {
            mouse(|app, _| {
                let error = Link::Error("/dev/hidraw3: no reply from device".into());
                app.show(app.model, error, None, MouseSettings::default());
            })
        };
        let mut app = demo(app::DEMO_MODEL);
        let list = wanted(&app, &["polling-rate=500", "angle-snapping=off"]).unwrap();
        assert_eq!(apply_all(&mut app, fails(), list, false, Kind::Set), 0);

        let mut app = demo(app::DEMO_MODEL);
        let list = wanted(&app, &["polling-rate=500", "motion-sync=off"]).unwrap();
        assert_eq!(
            apply_all(&mut app, fails(), list, false, Kind::Set),
            EXIT_ERROR
        );
    }

    /// Lighting that didn't read back can't be changed or checked.
    #[test]
    fn a_setting_missing_from_the_read_back_is_not_sent() {
        let unlit = || {
            mouse(|app, p| {
                if on_rate(p) {
                    app.settings.lighting = None;
                }
            })
        };
        let mut app = demo(app::DEMO_MODEL);
        let list = wanted(&app, &["polling-rate=500", "colour=red"]).unwrap();
        assert_eq!(apply_all(&mut app, unlit(), list, false, Kind::Set), 0);

        let mut app = demo(app::DEMO_MODEL);
        let list = wanted(&app, &["polling-rate=500", "colour=blue"]).unwrap();
        assert_eq!(
            apply_all(&mut app, unlit(), list, false, Kind::Set),
            EXIT_ERROR
        );
        assert_eq!(app.settings.lighting, None);
    }

    /// An item `import` skipped stays skipped, even once it unlocks.
    #[test]
    fn an_item_import_skipped_stays_skipped() {
        let mut app = demo(app::DEMO_MODEL);
        let text = file_with(
            &app,
            &[("power-saving-rate", "250"), ("polling-rate", "500")],
        );
        let (list, _) = parse_file(&app, "f", &text).unwrap();
        let unlocks = mouse(|app, p| {
            if on_rate(p)
                && let Some(saving) = app.settings.power_saving.as_mut()
            {
                saving.on = true;
            }
        });
        assert_eq!(
            apply_all(
                &mut app,
                unlocks,
                list,
                false,
                Kind::Import { skipped: false }
            ),
            0
        );
        let saving = app.settings.power_saving.unwrap();
        assert_ne!(saving.rate, PollingRate::HZ_250);
    }

    /// The app's own export with some settings changed.
    fn file_with(app: &App, changes: &[(&str, &str)]) -> String {
        let mut json: Json = serde_json::from_str(&to_json(app, Form::File).unwrap()).unwrap();
        for &(key, value) in changes {
            json["settings"][key] = serde_json::from_str(value).unwrap();
        }
        json.to_string()
    }

    #[test]
    fn get_lists_keys_values_and_locks() {
        let text = get_text(&demo(app::DEMO_MODEL));
        assert!(text.starts_with("ROG Harpe II Ace [demo]\ndpi-1                400\n"));
        assert!(
            text.contains("power-saving-rate    1000 Hz (locked: only while power saving is on)\n")
        );
    }

    #[test]
    fn describe_says_what_happened() {
        let path = Path::new("/dev/hidraw3");
        let done = "Polling rate set to 1000 Hz";
        let describe = |result, asleep| describe(&result, done, true, path, asleep);
        assert!(matches!(describe(Ok(()), false), Report::Done(t) if t == done));
        assert!(matches!(
            describe(Err(ApplyError::NotSaved(QueryError::Rejected)), false),
            Report::Unsaved(t) if t.ends_with("it may not last a power cycle")
        ));
        assert!(matches!(
            describe(Err(ApplyError::NotSaved(QueryError::Timeout)), true),
            Report::Unsaved(t) if t.contains("went to sleep")
        ));
        assert!(matches!(
            describe(Err(ApplyError::NotChanged(QueryError::Timeout)), false),
            Report::Failed { asleep: true, .. }
        ));
        assert!(matches!(
            describe(Err(ApplyError::Replaced), false),
            Report::Failed { asleep: false, .. }
        ));
    }
}
