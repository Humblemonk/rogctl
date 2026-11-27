//! `rogctl settings get`, `set`, `export` and `import`: the mouse's settings
//! from the command line. Every change is checked by `App::change()`, as the
//! settings screen's are, and sent by `Device::apply()`. `get` and `export`
//! only read.

use std::fs;
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};

use serde::de::{MapAccess, Visitor};
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use serde_json::Value as Json;

use crate::app::{self, App, Control, Link, Planned, Refusal, Value};
use crate::device::{self, ApplyError, Device, Outcome, QueryError};
use crate::protocol::{Change, Effect, LiftOff, MouseSettings, PowerOff};

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
    /// To the file, or stdout for None or `-`. A path need not be UTF-8.
    Export(Option<PathBuf>),
    /// From the file, or stdin for `-`.
    Import(PathBuf),
}

/// `demo`: `Some` runs without a mouse, as the named model or the default.
/// `dry_run`: `set` and `import` say what they'd change and send nothing.
pub fn run(command: Command, demo: Option<Option<&str>>, dry_run: bool) -> u8 {
    let (mut app, device) = match load(demo) {
        Ok(loaded) => loaded,
        Err((code, text)) => {
            let _ = io::stderr().write_all(text.as_bytes());
            return code;
        }
    };
    let result = match command {
        Command::Get { json: false } => write_out(io::stdout(), &get_text(&app)).map(|()| 0),
        Command::Get { json: true } => to_json(&app, Form::Line)
            .and_then(|text| write_out(io::stdout(), &format!("{text}\n")))
            .map(|()| 0),
        Command::Export(file) => {
            to_json(&app, Form::File).and_then(|text| export(&app, file, &text).map(|()| 0))
        }
        Command::Set(args) => parse_args(&app, &args)
            .map(|wanted| apply_all(&mut app, send(device.as_ref()), wanted, dry_run, Kind::Set)),
        Command::Import(file) => read_file(&file)
            .and_then(|text| parse_file(&app, &file.display().to_string(), &text))
            .map(|(wanted, skipped)| {
                let kind = Kind::Import { skipped };
                apply_all(&mut app, send(device.as_ref()), wanted, dry_run, kind)
            }),
    };
    result.unwrap_or_else(|text| {
        complain!("{text}");
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
    Err(if let Some(why) = unreported(app, key) {
        format!("{key}: {why}")
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

/// Why the mouse doesn't show `key`, a setting its model has: a stage beyond
/// those it uses now, or one it didn't report (lighting that didn't read).
/// None if the model hasn't the setting.
fn unreported(app: &App, key: &str) -> Option<String> {
    let model = app.model.filter(|_| model_has(app, key))?;
    let name = model.name;
    let used = app.settings.stages.len();
    let stage = key
        .strip_prefix("dpi-")
        .and_then(|n| n.parse::<usize>().ok());
    Some(match stage {
        Some(n) if n > used && model.features.dpi.variable_stages => {
            format!("the {name} uses {used} DPI stages now")
        }
        Some(_) | None => format!("the {name} didn't report it this time"),
    })
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
        Value::PollingRate(rate) => rate.is_known().then(|| rate.hz().into()),
        Value::Debounce(d) => d.is_known().then(|| d.ms().into()),
        Value::Switch(on) => Some(on.into()),
        Value::Degrees(d) => Some(d.into()),
        Value::LiftOff(LiftOff::Unknown(_)) => None,
        Value::LiftOff(LiftOff::Low | LiftOff::High) => name(value),
        Value::PowerOff(p) if p == PowerOff::NEVER => Some("never".into()),
        Value::PowerOff(p) => p.minutes().map(Json::from),
        Value::Percent(pct) | Value::Brightness(pct) => Some(pct.into()),
        Value::Effect(Effect::Unknown(_)) | Value::UnknownPercent(_) => None,
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

/// How `value` may be written: as `get` and the settings screen show it
/// (`1000 Hz`, `Colour cycle`), without the space before a unit (`1000hz`),
/// and as `export` writes it (`1000`, `colour-cycle`). Matched ignoring case,
/// nothing else: a sign or separator could make another valid value.
fn spellings(value: Value) -> Vec<String> {
    let shown = value.to_string();
    let mut list = vec![shown.clone()];
    if let Some((number, unit)) = shown.split_once(' ')
        && is_digits(number)
    {
        list.push(format!("{number}{unit}"));
    }
    list.extend(json_value(value).map(|j| json_text(&j)));
    list
}

/// One or more ASCII digits and nothing else: no sign, space or separator,
/// which `str::parse` would take (`+800`) or which could hide a typo.
fn is_digits(text: &str) -> bool {
    !text.is_empty() && text.bytes().all(|b| b.is_ascii_digit())
}

/// `text` as a value for `setting`. Only its form is checked here; whether
/// the mouse takes it, `App::change()` decides.
fn parse_value(app: &App, setting: Setting, text: &str) -> Result<Wanted, String> {
    let key = setting.key();
    let text = text.trim();
    let control = match setting {
        Setting::ActiveStage => {
            return match is_digits(text).then(|| text.parse::<usize>()) {
                Some(Ok(n)) if n >= 1 => Ok(Wanted::Activate(n - 1)),
                Some(_) | None => Err(format!("{key} takes a stage number, such as 2")),
            };
        }
        Setting::Control(control) => control,
    };
    let value = match control {
        Control::Stage(_) => is_digits(text)
            .then(|| text.parse().ok())
            .flatten()
            .map(Value::Dpi)
            .ok_or_else(|| format!("{key} takes a DPI, such as 800"))?,
        // As the settings screen shows it (`+5°`, `-5°`) or exported (`-5`):
        // i16's own parse takes only an optional sign and digits.
        Control::AngleTuning => text
            .strip_suffix('°')
            .unwrap_or(text)
            .parse()
            .map(Value::Degrees)
            .map_err(|_| format!("{key} takes a number of degrees, such as -5"))?,
        Control::LightingColor if text.starts_with('#') => {
            parse_hex(text).ok_or_else(|| format!("{key} takes a colour name or #rrggbb"))?
        }
        Control::PollingRate
        | Control::Debounce
        | Control::AngleSnapping
        | Control::MotionSync
        | Control::LiftOff
        | Control::PowerOff
        | Control::LowBatteryWarning
        | Control::PowerSaving
        | Control::PowerSavingRate
        | Control::LightingEffect
        | Control::LightingColor
        | Control::Brightness => {
            // The mouse's own value too, should another program have set one
            // outside the list: `App::change()` finds it unchanged.
            let mut named = app.named_values(control);
            named.extend(app.value(control).filter(|v| !named.contains(v)));
            named
                .iter()
                .copied()
                .find(|&v| spellings(v).iter().any(|s| s.eq_ignore_ascii_case(text)))
                .ok_or_else(|| {
                    // Only rogctl's choices: the mouse's own is no offer.
                    let names: Vec<String> = app
                        .named_values(control)
                        .iter()
                        .filter_map(|&v| choice_name(v))
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

/// How a refused value's message lists `value`: as `export` writes it, but
/// a number or switch that `get` shows as a word (`Off`, `On`) as that word,
/// which is taken too.
fn choice_name(value: Value) -> Option<String> {
    let json = json_value(value)?;
    let shown = value.to_string();
    let word = !json.is_string() && !shown.starts_with(|c: char| c.is_ascii_digit());
    Some(if word {
        shown.to_lowercase()
    } else {
        json_text(&json)
    })
}

fn parse_hex(text: &str) -> Option<Value> {
    // Hex digits only: `from_str_radix` would take a sign (`#+f+f+f`).
    let hex = text
        .strip_prefix('#')
        .filter(|h| h.len() == 6 && h.bytes().all(|b| b.is_ascii_hexdigit()))?;
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

/// Writes what `get` or `export` prints to `out`. An error if it didn't all
/// get out, as when stdout is a closed pipe: the settings weren't printed.
fn write_out(mut out: impl Write, text: &str) -> Result<(), String> {
    out.write_all(text.as_bytes())
        .and_then(|()| out.flush())
        .map_err(|e| format!("Couldn't print the settings: {e}"))
}

fn export(app: &App, file: Option<PathBuf>, text: &str) -> Result<(), String> {
    match file.filter(|path| path.as_os_str() != "-") {
        None => write_out(io::stdout(), &format!("{text}\n")),
        Some(path) => {
            let path_text = path.display();
            fs::write(&path, format!("{text}\n"))
                .map_err(|e| format!("Couldn't write {path_text}: {e}"))?;
            let name = app.model.map_or("mouse", |m| m.name);
            say!("Saved the {name}'s settings to {path_text}");
            Ok(())
        }
    }
}

// ---------------------------------------------------------------------------
// import
// ---------------------------------------------------------------------------

fn read_file(path: &Path) -> Result<String, String> {
    if path.as_os_str() == "-" {
        let mut text = String::new();
        io::stdin()
            .read_to_string(&mut text)
            .map_err(|e| format!("Couldn't read stdin: {e}"))?;
        return Ok(text);
    }
    fs::read_to_string(path).map_err(|e| format!("Couldn't read {}: {e}", path.display()))
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
    let mut skipped: Vec<String> = Vec::new();
    for (key, value) in file.settings.ok_or_else(not_ours)?.0 {
        let setting = match find(app, &key) {
            Ok(setting) => setting,
            Err(e) => match unreported(app, &key) {
                // `in_order()` refuses the others given twice.
                Some(_) if skipped.contains(&key) => {
                    return Err(format!("{path}: {key} is given twice"));
                }
                Some(why) => {
                    complain!("Skipped: {key}: {why}");
                    skipped.push(key);
                    continue;
                }
                None => return Err(format!("{path}: {e}")),
            },
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
    Ok((wanted, !skipped.is_empty()))
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
                    say!("Would change {}: {before} → {after}", setting.key());
                }
                Check::Due
            }
            Err(Refusal::Unchanged(text)) => {
                if !import {
                    say!("{text}");
                }
                Check::AlreadySet
            }
            Err(Refusal::NotNow(text)) if import => {
                complain!("{}", skipped_note(&trial, setting, w, &text));
                Check::Skipped
            }
            Err(Refusal::NotNow(text) | Refusal::Invalid(text)) => {
                complain!("{text}\nNothing was changed.");
                return EXIT_ERROR;
            }
        });
    }
    if !checks.contains(&Check::Due) {
        if import && !skipped_before && !checks.contains(&Check::Skipped) {
            say!("Nothing to change: the mouse already has these settings");
        }
        return 0;
    }
    if dry_run {
        say!("Dry run: nothing was sent to the mouse.");
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
                complain!("Stopped: {why}");
                return stopped(made, code);
            }
            complain!("Couldn't check the rest again after the last change: {why}");
            return 0;
        }
        let key = setting.key();
        // The read-back may leave a setting out (lighting that didn't read).
        if !settings_of(app).iter().any(|&(s, _)| s == setting) {
            if check == Check::Due {
                complain!(
                    "Couldn't change {key}: the mouse didn't report it after the last change"
                );
                return stopped(made, EXIT_ERROR);
            }
            complain!(
                "Couldn't check {key} again: the mouse didn't report it after the last change"
            );
            continue;
        }
        let planned = match (plan(app, w), check) {
            (Ok(planned), _) => planned,
            (Err(Refusal::Unchanged(text)), Check::Due) if !import => {
                say!("{text}");
                continue;
            }
            (Err(Refusal::Unchanged(_)), _) => continue,
            (Err(Refusal::NotNow(text)), Check::Due)
            | (Err(Refusal::NotNow(text) | Refusal::Invalid(text)), Check::AlreadySet)
                if import =>
            {
                complain!("{}", skipped_note(app, setting, w, &text));
                continue;
            }
            // Refused only now: the mouse read back differs from what was
            // checked, or an earlier change altered a setting that was right.
            (Err(Refusal::NotNow(text) | Refusal::Invalid(text)), _) => {
                complain!("{text}");
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
            complain!("The mouse took the change{unconfirmed}, but {key} reads back {now}");
            return stopped(made, EXIT_ERROR);
        }
        let demo = if app.demo { "Demo: " } else { "" };
        match &unsaved {
            Some(warning) => complain!("{demo}{warning}"),
            None => say!("{demo}{}", planned.done),
        }
        // An unsaved warning already says the mouse went to sleep.
        if app.link != Link::Connected && !(unsaved.is_some() && app.link == Link::Asleep) {
            let (why, _) = lost(&app.link);
            complain!("Couldn't check {key} after the change: {why}");
        } else if app.link == Link::Connected && !reported {
            complain!("Couldn't check {key}: the mouse didn't report it after the change");
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

/// `import`'s note for a setting it skipped because `why`: the mouse keeps
/// its value, not the file's, so it won't match the file there.
fn skipped_note(app: &App, setting: Setting, wanted: Wanted, why: &str) -> String {
    let key = setting.key();
    let file = match wanted {
        Wanted::Set(_, value) => value.to_string(),
        Wanted::Activate(stage) => (stage + 1).to_string(),
    };
    let keeps = shown(app, setting).unwrap_or_else(|| "its value".to_owned());
    format!("Skipped: {key} keeps {keeps}, not the file's {file}, which wasn't restored: {why}")
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
        1 => complain!("Made 1 change before stopping."),
        n => complain!("Made {n} changes before stopping."),
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
    let before = app.clone();
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
    let held = read_back_holds(&before, &planned.change, app);
    match describe(
        &applied.result,
        &planned.done,
        wireless,
        &device.hidraw,
        asleep,
        held,
    ) {
        Report::Done(_) => Ok(None),
        Report::Unsaved(text) => Ok(Some(text)),
        Report::Failed { text, asleep } => {
            complain!("{text}");
            Err(if asleep { EXIT_ASLEEP } else { EXIT_ERROR })
        }
    }
}

/// How a change went, in words, for the settings screen and the CLI alike.
pub enum Report {
    Done(String),
    /// Made, but the mouse didn't confirm saving it, or didn't confirm the
    /// change itself, which it reads back.
    Unsaved(String),
    /// Not made; `asleep` when the mouse didn't answer.
    Failed {
        text: String,
        asleep: bool,
    },
}

/// Whether `after`, the mouse read back after `change`, has the change
/// made: every setting the change sets, those `before` (the screen the
/// change was made from) shows differently with it ([`App::simulate`]),
/// reads back as it would then. Others are left out, since they can have
/// changed since `before` was read (the DPI button, another program). Sends
/// nothing; tells [`describe`] that a change the mouse didn't confirm was
/// made all the same.
pub fn read_back_holds(before: &App, change: &Change, after: &App) -> bool {
    let mut expected = before.clone();
    expected.simulate(change);
    let changed: Vec<Setting> = settings_of(&expected)
        .into_iter()
        .map(|(setting, _)| setting)
        .filter(|&setting| shown(&expected, setting) != shown(before, setting))
        .collect();
    after.link == Link::Connected
        && !changed.is_empty()
        && changed
            .iter()
            .all(|&setting| shown(after, setting) == shown(&expected, setting))
}

/// `done` says what was changed; `asleep_after`, that the read-back after
/// it found the mouse asleep; `held`, that it found the change made
/// ([`read_back_holds`]).
pub fn describe(
    result: &Result<(), ApplyError>,
    done: &str,
    wireless: bool,
    hidraw: &Path,
    asleep_after: bool,
    held: bool,
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
        // The change's echo came late or not at all, so it wasn't saved,
        // but the mouse reads back that it took it.
        Err(ApplyError::NotChanged(
            QueryError::Timeout | QueryError::Rejected | QueryError::Empty,
        )) if held => Report::Unsaved(format!(
            "{done}, but the mouse didn't confirm it and it wasn't saved: it may not last a power cycle"
        )),
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

    /// A polling rate rogctl can't name is left out of an export, as other
    /// unknown codes are.
    #[test]
    fn an_unknown_rate_is_not_exported() {
        assert_eq!(
            json_value(Value::PollingRate(PollingRate::from_raw(0x0f))),
            None
        );
        assert!(json_value(Value::PollingRate(PollingRate::HZ_1000)).is_some());
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

    /// A refusal lists a value `get` shows as a word by that word, which is
    /// taken: brightness 0 is "Off", a switch "On" or "Off".
    #[test]
    fn a_refusal_lists_values_as_get_shows_them() {
        let app = demo("ROG Harpe II Ace");
        let err = |key, text| value(&app, key, text).unwrap_err();
        assert_eq!(
            err("brightness", "-50"),
            "brightness takes one of: off, 10, 20, 30, 40, 50, 60, 70, 80, 90, 100"
        );
        assert_eq!(
            err("motion-sync", "maybe"),
            "motion-sync takes one of: off, on"
        );
        assert!(err("power-off", "soon").ends_with(", never"));
        assert!(
            err("low-battery-warning", "-1").starts_with("low-battery-warning takes one of: off, ")
        );
        for (key, text) in [("brightness", "off"), ("motion-sync", "on")] {
            assert!(value(&app, key, text).is_ok(), "{key}={text}");
        }
    }

    /// A sign, separator or typo is refused, not read as another value
    /// (`brightness=-50` was 50%, `power-off=1-0` 10 min).
    #[test]
    fn values_rogctl_doesnt_write_are_refused() {
        let app = demo(app::DEMO_MODEL);
        for (key, text) in [
            ("brightness", "-50"),
            ("brightness", "1 0 0"),
            ("brightness", "50 %"),
            ("low-battery-warning", "-30"),
            ("low-battery-warning", "o-f-f"),
            ("power-off", "-5"),
            ("power-off", "1-0"),
            ("power-off", "1_0"),
            ("power-off", "ne ver"),
            ("polling-rate", "-1000"),
            ("polling-rate", "1_000"),
            ("polling-rate", "1000 h z"),
            ("power-saving-rate", "+250"),
            ("effect", "-static"),
            ("effect", "colourcycle"),
            ("effect", "colour_cycle"),
            ("motion-sync", "o-n"),
            ("motion-sync", "+on"),
            ("lift-off", "lo w"),
            ("colour", "re-d"),
            ("colour", "#+f+f+f"),
            ("colour", "#-1-1-1"),
            ("colour", "# ff ff"),
            ("dpi-1", "+800"),
            ("dpi-1", "-800"),
            ("dpi-1", "8 00"),
            ("dpi-1", "8_00"),
            ("angle-tuning", "+5°°"),
            ("angle-tuning", "- 5"),
            ("angle-tuning", "--5"),
            ("angle-tuning", "5-"),
            ("active-stage", "+2"),
            ("active-stage", "-2"),
        ] {
            assert!(value(&app, key, text).is_err(), "{key}={text}");
        }
        let file = r#"{"version":1,"model":"ROG Harpe II Ace","settings":{"brightness":-50}}"#;
        assert!(parse_file(&app, "f", file).is_err());
        assert_eq!(
            value(&app, "angle-tuning", "+5°"),
            Ok(Wanted::Set(Control::AngleTuning, Value::Degrees(5)))
        );
    }

    /// Every spelling rogctl itself writes, on every model, reads back as
    /// the value it was written for: `get`'s and the settings screen's
    /// (`Display`, with or without the space before a unit), and
    /// `export`'s, for each value the mouse has and each named choice.
    #[test]
    fn every_value_rogctl_writes_reads_back() {
        for model in MODELS {
            let app = demo(model.name);
            for (setting, _) in settings_of(&app) {
                let key = setting.key();
                let Some(shown) = shown(&app, setting) else {
                    continue;
                };
                let w = parse_value(&app, setting, &shown)
                    .unwrap_or_else(|e| panic!("{} {key}={shown}: {e}", model.name));
                assert!(
                    matches!(plan(&app, w), Err(Refusal::Unchanged(_))),
                    "{} {key}={shown}",
                    model.name
                );
                let Setting::Control(control) = setting else {
                    continue;
                };
                let mut values = app.named_values(control);
                values.extend(app.value(control));
                for v in values {
                    let display = v.to_string();
                    let mut texts = vec![display.clone(), display.to_uppercase()];
                    if let Some((n, unit)) = display.split_once(' ')
                        && n.parse::<u32>().is_ok()
                    {
                        texts.push(format!("{n}{unit}"));
                        texts.push(n.to_owned());
                    }
                    texts.extend(json_value(v).map(|j| json_text(&j)));
                    for text in texts {
                        assert_eq!(
                            parse_value(&app, setting, &text),
                            Ok(Wanted::Set(control, v)),
                            "{} {key}={text}",
                            model.name
                        );
                    }
                }
            }
        }
    }

    /// What docs/cli.md shows `set` taking.
    #[test]
    fn the_documented_values_parse() {
        let app = demo(app::DEMO_MODEL);
        let args = [
            "polling-rate=1000",
            "polling-rate=1000hz",
            "dpi-2=1600",
            "motion-sync=off",
            "angle-snapping=on",
            "power-saving=off",
            "angle-tuning=-5",
            "lift-off=high",
            "power-off=10",
            "power-off=never",
            "low-battery-warning=0",
            "low-battery-warning=off",
            "effect=battery-level",
            "effect=colour-cycle",
            "colour=magenta",
            "colour=#00ff80",
            "brightness=0",
            "active-stage=1",
        ];
        for arg in args {
            assert!(wanted(&app, &[arg]).is_ok(), "{arg}");
        }
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
            Err("colour: the ROG Harpe II Ace didn't report it this time".into())
        );
        let mut three = app.clone();
        three.settings.stages.truncate(3);
        assert_eq!(
            find(&three, "dpi-4"),
            Err("dpi-4: the ROG Harpe II Ace uses 3 DPI stages now".into())
        );
        assert_eq!(
            find(&three, "dpi-5"),
            Err("This ROG Harpe II Ace doesn't have dpi-5".into())
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
        let w = app.model.unwrap().features.low_battery_warning.unwrap();
        app.settings.low_battery_warning = Some(crate::protocol::Warning::from_percent(w, 90));
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
        let twice = r#"{"version":1,"model":"ROG Harpe II Ace","settings":{"colour":"red","colour":"blue"}}"#;
        assert_eq!(
            parse_file(&app, "f", twice).unwrap_err(),
            "f: colour is given twice"
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

    /// A file exported with power saving off still has its rate, which is
    /// locked then: the note says the file's value wasn't restored.
    #[test]
    fn a_skipped_setting_says_the_files_value_wasnt_restored() {
        let app = demo(app::DEMO_MODEL);
        let text = file_with(&app, &[("power-saving-rate", "250")]);
        let (list, _) = parse_file(&app, "f", &text).unwrap();
        let (setting, w) = list
            .into_iter()
            .find(|&(s, _)| s == Setting::Control(Control::PowerSavingRate))
            .unwrap();
        let Err(Refusal::NotNow(why)) = plan(&app, w) else {
            panic!("the rate should be locked");
        };
        assert_eq!(
            skipped_note(&app, setting, w, &why),
            "Skipped: power-saving-rate keeps 1000 Hz, not the file's 250 Hz, which wasn't \
             restored: Power-saving rate can't be changed now: only while power saving is on"
        );
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

    /// A warning byte past 100% shows as unknown and is left out of an
    /// export, as an unknown power-off code is, so importing it sends nothing.
    #[test]
    fn an_unknown_warning_is_shown_but_not_exported() {
        let mut app = demo(app::DEMO_MODEL);
        let model = app.model.unwrap();
        let mut reply = [0u8; 16];
        reply[..3].copy_from_slice(&[model.report_id, 0x12, 0x07]);
        reply[7] = 0xff;
        let r = crate::protocol::parse_battery(model.battery, &model.features, &reply).unwrap();
        app.settings.low_battery_warning = r.low_battery_warning;
        assert!(get_text(&app).contains("low-battery-warning  Unknown (0xff)\n"));
        let text = to_json(&app, Form::File).unwrap();
        assert!(!text.contains("low-battery-warning"), "{text}");
        let (list, _) = parse_file(&app, "f", &text).unwrap();
        for (setting, w) in list {
            assert!(
                matches!(plan(&app, w), Err(Refusal::Unchanged(_))),
                "{}",
                setting.key()
            );
        }
    }

    /// A brightness past 100% shows as unknown and is left out of an export,
    /// as an unknown warning is.
    #[test]
    fn an_unknown_brightness_is_shown_but_not_exported() {
        let mut app = demo(app::DEMO_MODEL);
        if let Some(l) = app.settings.lighting.as_mut() {
            l.brightness = 0xc8;
        }
        assert!(get_text(&app).contains("brightness           Unknown (0xc8)\n"));
        let text = to_json(&app, Form::File).unwrap();
        assert!(!text.contains("\"brightness\""), "{text}");
    }

    /// Settings that didn't get out (stdout a closed pipe) are an error, so
    /// `get` and `export` exit 1 rather than 0.
    #[test]
    fn settings_that_dont_get_out_are_an_error() {
        struct Closed;
        impl Write for Closed {
            fn write(&mut self, _: &[u8]) -> io::Result<usize> {
                Err(io::ErrorKind::BrokenPipe.into())
            }
            fn flush(&mut self) -> io::Result<()> {
                Ok(())
            }
        }
        assert!(write_out(Closed, "{}\n").is_err());
        let mut out = Vec::new();
        assert_eq!(write_out(&mut out, "{}\n"), Ok(()));
        assert_eq!(out, b"{}\n");
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
        let describe = |result, asleep| describe(&result, done, true, path, asleep, false);
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

    /// A change the mouse didn't confirm, but reads back: made, unsaved.
    #[test]
    fn describe_says_an_unconfirmed_change_read_back_was_made() {
        let path = Path::new("/dev/hidraw3");
        let done = "Polling rate set to 1000 Hz";
        let describe = |e, held| {
            describe(
                &Err(ApplyError::NotChanged(e)),
                done,
                true,
                path,
                false,
                held,
            )
        };
        for e in [QueryError::Timeout, QueryError::Rejected, QueryError::Empty] {
            assert!(matches!(
                describe(e, true),
                Report::Unsaved(t) if t == "Polling rate set to 1000 Hz, but the mouse didn't confirm it and it wasn't saved: it may not last a power cycle"
            ));
        }
        assert!(matches!(
            describe(QueryError::Timeout, false),
            Report::Failed { text, asleep: true } if text.starts_with("No change made")
        ));
        // Not read back: an empty reply means it's asleep.
        assert!(matches!(
            describe(QueryError::Empty, false),
            Report::Failed { asleep: true, .. }
        ));
    }

    /// The read-back has a change when it shows what the screen it was made
    /// from would with it, and that's a change.
    #[test]
    fn read_back_holds_only_the_change_asked() {
        let before = demo("ROG Harpe II Ace");
        let change = Change::MotionSync(before.settings.motion_sync != Some(true));
        let mut after = before.clone();
        assert!(!read_back_holds(&before, &change, &after));
        after.simulate(&change);
        assert!(read_back_holds(&before, &change, &after));
        let unchanged = Change::MotionSync(before.settings.motion_sync == Some(true));
        assert!(!read_back_holds(&before, &unchanged, &before));
        after.link = Link::Asleep;
        assert!(!read_back_holds(&before, &change, &after));
    }

    /// Another setting read back differently (the DPI button pressed since
    /// the screen was read) doesn't hide the change; the change's own value
    /// read back as it was does.
    #[test]
    fn read_back_holds_past_another_setting_gone_stale() {
        let before = demo("ROG Harpe II Ace");
        let change = Change::MotionSync(before.settings.motion_sync != Some(true));
        let mut after = before.clone();
        after.simulate(&change);
        let stage = before.settings.active_stage.unwrap_or(0);
        after.settings.active_stage = Some((stage + 1) % before.settings.stages.len());
        assert_ne!(after.settings.active_stage, before.settings.active_stage);
        assert!(read_back_holds(&before, &change, &after));
        after.settings.motion_sync = before.settings.motion_sync;
        assert!(!read_back_holds(&before, &change, &after));
    }
}
