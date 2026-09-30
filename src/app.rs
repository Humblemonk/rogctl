//! The TUI's state and key handling. Nothing here talks to the mouse: a key
//! that should change a setting returns an [`Action`], which
//! `tui::apply_action` carries out.

use std::fmt;

use ratatui::crossterm::event::{KeyCode, KeyModifiers, MouseButton, MouseEventKind};

use crate::device::Model;
use crate::models::MODELS;
use crate::protocol::{
    self, Change, Debounce, DpiStage, Features, LiftOff, MouseSettings, PollingRate, PowerOff,
    PowerSaving, Reading,
};

/// A row of the settings screen, in focus order. Models leave out the rows
/// they don't have, and never reorder them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Control {
    /// Index into the DPI stages.
    Stage(usize),
    PollingRate,
    Debounce,
    AngleSnapping,
    AngleTuning,
    MotionSync,
    LiftOff,
    PowerOff,
    LowBatteryWarning,
    PowerSaving,
    PowerSavingRate,
}

impl Control {
    pub fn label(self) -> String {
        match self {
            Self::Stage(i) => format!("DPI stage {}", i + 1),
            Self::PollingRate => "Polling rate".into(),
            Self::Debounce => "Button debounce".into(),
            Self::AngleSnapping => "Angle snapping".into(),
            Self::AngleTuning => "Angle tuning".into(),
            Self::MotionSync => "Motion sync".into(),
            Self::LiftOff => "Lift-off distance".into(),
            Self::PowerOff => "Auto power-off".into(),
            Self::LowBatteryWarning => "Low-battery warning".into(),
            Self::PowerSaving => "Advanced power saving".into(),
            Self::PowerSavingRate => "Power-saving rate".into(),
        }
    }

    /// Shown under "Battery" rather than "Performance".
    pub fn is_battery(self) -> bool {
        match self {
            Self::PowerOff
            | Self::LowBatteryWarning
            | Self::PowerSaving
            | Self::PowerSavingRate => true,
            Self::Stage(_)
            | Self::PollingRate
            | Self::Debounce
            | Self::AngleSnapping
            | Self::AngleTuning
            | Self::MotionSync
            | Self::LiftOff => false,
        }
    }
}

/// A setting's value, as shown and edited.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Value {
    Dpi(u32),
    PollingRate(PollingRate),
    Debounce(Debounce),
    Switch(bool),
    Degrees(i16),
    LiftOff(LiftOff),
    PowerOff(PowerOff),
    Percent(u8),
}

impl fmt::Display for Value {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match *self {
            Self::Dpi(dpi) => write!(f, "{dpi}"),
            Self::PollingRate(rate) => write!(f, "{} Hz", rate.hz()),
            Self::Debounce(d) => write!(f, "{} ms", d.ms()),
            Self::Switch(true) => write!(f, "On"),
            Self::Switch(false) => write!(f, "Off"),
            Self::Degrees(0) => write!(f, "0°"),
            Self::Degrees(d) => write!(f, "{d:+}°"),
            Self::LiftOff(LiftOff::Low) => write!(f, "Low"),
            Self::LiftOff(LiftOff::High) => write!(f, "High"),
            Self::LiftOff(LiftOff::Unknown(code)) => write!(f, "Unknown ({code:#04x})"),
            Self::PowerOff(p) => write!(f, "{p}"),
            Self::Percent(0) => write!(f, "Off"),
            Self::Percent(pct) => write!(f, "{pct}%"),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Link {
    Connected,
    Asleep,
    Disconnected,
    Error(String),
}

/// A value changed with the keys but not yet sent to the mouse.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Edit {
    pub control: Control,
    pub value: Value,
    /// The value is DPI digits being typed.
    pub typed: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Message {
    pub text: String,
    pub error: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Action {
    /// `done` is the message to show once it has worked.
    Apply {
        change: Change,
        done: String,
    },
    Refresh,
    NextDevice,
}

/// What the mouse pointer is over, from the last frame `ui::draw` drew.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Target {
    Row(Control),
    /// One value on a row's scale.
    Choice(Control, Value),
}

/// One row as drawn: `locked` explains why a supported setting can't be
/// changed right now. Locked rows are shown but not focusable.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Row {
    pub control: Control,
    pub locked: Option<&'static str>,
}

#[derive(Debug)]
pub struct App {
    pub model: Option<&'static Model>,
    pub link: Link,
    pub reading: Option<Reading>,
    pub settings: MouseSettings,
    focus: Option<Control>,
    pub edit: Option<Edit>,
    pub message: Option<Message>,
    pub help: bool,
    pub quit: bool,
    pub demo: bool,
    /// Mice found, for offering Tab.
    pub devices: usize,
}

impl App {
    pub fn new(demo: bool) -> Self {
        Self {
            model: None,
            link: Link::Disconnected,
            reading: None,
            settings: MouseSettings::default(),
            focus: None,
            edit: None,
            message: None,
            help: false,
            quit: false,
            demo,
            devices: 0,
        }
    }

    /// Shows what was just read. Keeps the focus, drops any unsent edit.
    pub fn show(
        &mut self,
        model: Option<&'static Model>,
        link: Link,
        reading: Option<Reading>,
        settings: MouseSettings,
    ) {
        self.model = model;
        self.link = link;
        self.reading = reading;
        self.settings = settings;
        self.edit = None;
    }

    pub fn ok(&mut self, text: impl Into<String>) {
        self.message = Some(Message {
            text: text.into(),
            error: false,
        });
    }

    pub fn error(&mut self, text: impl Into<String>) {
        self.message = Some(Message {
            text: text.into(),
            error: true,
        });
    }

    fn features(&self) -> Option<&'static Features> {
        self.model.map(|m| &m.features)
    }

    /// Why motion sync can't be changed right now, if it can't.
    fn motion_sync_lock(&self) -> Option<&'static str> {
        (self.settings.polling_rate == Some(PollingRate::HZ_8000)).then_some("not at 8000 Hz")
    }

    pub fn rows(&self) -> Vec<Row> {
        if self.link != Link::Connected {
            return Vec::new();
        }
        let s = &self.settings;
        let row = |control| Row {
            control,
            locked: None,
        };
        let mut rows: Vec<Row> = (0..s.stages.len())
            .map(|i| row(Control::Stage(i)))
            .collect();
        let optional = [
            (s.polling_rate.is_some(), Control::PollingRate),
            (s.debounce.is_some(), Control::Debounce),
            (s.angle_snapping.is_some(), Control::AngleSnapping),
            (s.angle_tuning.is_some(), Control::AngleTuning),
            (s.motion_sync.is_some(), Control::MotionSync),
            (s.lift_off.is_some(), Control::LiftOff),
            (s.power_off.is_some(), Control::PowerOff),
            (s.low_battery_warning.is_some(), Control::LowBatteryWarning),
            (s.power_saving.is_some(), Control::PowerSaving),
            (s.power_saving.is_some(), Control::PowerSavingRate),
        ];
        for (present, control) in optional {
            if present {
                rows.push(Row {
                    control,
                    locked: match control {
                        Control::MotionSync => self.motion_sync_lock(),
                        // As in ASUS's own software.
                        Control::PowerSavingRate => s
                            .power_saving
                            .is_some_and(|p| !p.on)
                            .then_some("only while power saving is on"),
                        Control::Stage(_)
                        | Control::PollingRate
                        | Control::Debounce
                        | Control::AngleSnapping
                        | Control::AngleTuning
                        | Control::LiftOff
                        | Control::PowerOff
                        | Control::LowBatteryWarning
                        | Control::PowerSaving => None,
                    },
                });
            }
        }
        rows
    }

    fn controls(&self) -> Vec<Control> {
        self.rows()
            .into_iter()
            .filter(|r| r.locked.is_none())
            .map(|r| r.control)
            .collect()
    }

    /// The focused control: the last one focused if it's still there, else
    /// the first.
    pub fn focused(&self) -> Option<Control> {
        let controls = self.controls();
        self.focus
            .filter(|c| controls.contains(c))
            .or_else(|| controls.first().copied())
    }

    /// Every value a setting with a short fixed list can take, in ←/→
    /// order, for drawing it as a scale. None for the others.
    pub fn choices(&self, control: Control) -> Option<Vec<Value>> {
        let f = self.features()?;
        let rates = |rates: &[PollingRate]| rates.iter().map(|&r| Value::PollingRate(r)).collect();
        let choices: Vec<Value> = match control {
            Control::PollingRate => rates(f.polling_rates),
            Control::PowerSavingRate => rates(f.power_saving_rates),
            Control::Debounce => (Debounce::MIN.code()..=Debounce::MAX.code())
                .map(|c| Value::Debounce(Debounce::from_code(c)))
                .collect(),
            Control::PowerOff => PowerOff::CHOICES.map(Value::PowerOff).to_vec(),
            Control::LowBatteryWarning => {
                let w = f.low_battery_warning?;
                (0..=w.max)
                    .step_by(usize::from(w.step.max(1)))
                    .map(Value::Percent)
                    .collect()
            }
            Control::Stage(_)
            | Control::AngleSnapping
            | Control::AngleTuning
            | Control::MotionSync
            | Control::LiftOff
            | Control::PowerSaving => return None,
        };
        (!choices.is_empty()).then_some(choices)
    }

    /// The value on the mouse.
    pub fn value(&self, control: Control) -> Option<Value> {
        let s = &self.settings;
        match control {
            Control::Stage(i) => s.stages.get(i).map(|st| Value::Dpi(st.dpi)),
            Control::PollingRate => s.polling_rate.map(Value::PollingRate),
            Control::Debounce => s.debounce.map(Value::Debounce),
            Control::AngleSnapping => s.angle_snapping.map(Value::Switch),
            Control::AngleTuning => s.angle_tuning.map(Value::Degrees),
            Control::MotionSync => s.motion_sync.map(Value::Switch),
            Control::LiftOff => s.lift_off.map(Value::LiftOff),
            Control::PowerOff => s.power_off.map(Value::PowerOff),
            Control::LowBatteryWarning => s.low_battery_warning.map(Value::Percent),
            Control::PowerSaving => s.power_saving.map(|p| Value::Switch(p.on)),
            Control::PowerSavingRate => s.power_saving.map(|p| Value::PollingRate(p.rate)),
        }
    }

    /// The value being edited, if any, else the one on the mouse.
    fn shown(&self, control: Control) -> Option<Value> {
        match self.edit {
            Some(edit) if edit.control == control => Some(edit.value),
            Some(_) | None => self.value(control),
        }
    }

    pub fn handle_key(&mut self, code: KeyCode, mods: KeyModifiers) -> Option<Action> {
        if mods.contains(KeyModifiers::CONTROL) && code == KeyCode::Char('c') {
            self.quit = true;
            return None;
        }
        if self.help {
            // Help stays keyboard-only and swallows keys until closed.
            if matches!(code, KeyCode::Char('?' | 'q') | KeyCode::Esc) {
                self.help = false;
            }
            return None;
        }
        let big = mods.contains(KeyModifiers::SHIFT);
        // KeyCode has many keys rogctl doesn't use; they do nothing.
        match code {
            KeyCode::Char('q') => self.quit = true,
            KeyCode::Char('?') => self.help = true,
            KeyCode::Char('r') => {
                self.edit = None;
                return Some(Action::Refresh);
            }
            KeyCode::Tab => {
                self.edit = None;
                return (self.devices > 1).then_some(Action::NextDevice);
            }
            KeyCode::Esc => self.edit = None,
            KeyCode::Up | KeyCode::Char('k') => self.move_focus(-1),
            KeyCode::Down | KeyCode::Char('j') => self.move_focus(1),
            KeyCode::Left | KeyCode::Char('h') => self.adjust(-1, big),
            KeyCode::Right | KeyCode::Char('l') => self.adjust(1, big),
            KeyCode::PageDown => self.adjust(-1, true),
            KeyCode::PageUp => self.adjust(1, true),
            KeyCode::Char(c @ '0'..='9') => self.type_digit(c),
            KeyCode::Backspace => self.backspace(),
            KeyCode::Enter | KeyCode::Char(' ') => return self.confirm(),
            _ => {}
        }
        None
    }

    /// Only Enter's path writes. A click selects a row, then applies like
    /// Enter; a click on a scale's value or the wheel (up is higher) only
    /// changes the pending edit. A click while help is open closes it.
    pub fn handle_mouse(&mut self, kind: MouseEventKind, target: Option<Target>) -> Option<Action> {
        let dir = match kind {
            MouseEventKind::Down(MouseButton::Left) => 0,
            MouseEventKind::ScrollUp => 1,
            MouseEventKind::ScrollDown => -1,
            // Other buttons, releases, drags and sideways scrolling do nothing.
            _ => return None,
        };
        let click = dir == 0;
        if self.help {
            self.help = !click;
            return None;
        }
        let (control, choice) = match target? {
            Target::Row(control) => (control, None),
            Target::Choice(control, value) => (control, Some(value)),
        };
        // Locked rows can't be focused, so they don't take clicks either.
        if !self.controls().contains(&control) {
            return None;
        }
        if self.focused() != Some(control) {
            self.focus = Some(control);
            self.edit = None;
            if click && choice.is_none() {
                return None;
            }
        }
        if !click {
            self.adjust(dir, false);
            return None;
        }
        match choice {
            Some(value) if self.shown(control) != Some(value) => {
                self.set_edit(control, value, false);
                None
            }
            Some(_) | None => self.confirm(),
        }
    }

    fn move_focus(&mut self, by: isize) {
        self.edit = None;
        let controls = self.controls();
        let Some(current) = self.focused() else {
            return;
        };
        let at = controls.iter().position(|&c| c == current).unwrap_or(0);
        let next = at
            .saturating_add_signed(by)
            .min(controls.len().saturating_sub(1));
        self.focus = controls.get(next).copied();
    }

    /// Stores an edit, or drops it if it's back to the mouse's value.
    fn set_edit(&mut self, control: Control, value: Value, typed: bool) {
        self.edit = (typed || self.value(control) != Some(value)).then_some(Edit {
            control,
            value,
            typed,
        });
    }

    fn adjust(&mut self, dir: i8, big: bool) {
        let (Some(control), Some(f)) = (self.focused(), self.features()) else {
            return;
        };
        let Some(current) = self.shown(control) else {
            return;
        };
        let rates = match control {
            Control::PowerSavingRate => f.power_saving_rates,
            Control::Stage(_)
            | Control::PollingRate
            | Control::Debounce
            | Control::AngleSnapping
            | Control::AngleTuning
            | Control::MotionSync
            | Control::LiftOff
            | Control::PowerOff
            | Control::LowBatteryWarning
            | Control::PowerSaving => f.polling_rates,
        };
        self.set_edit(control, step(f, rates, current, dir, big), false);
    }

    fn type_digit(&mut self, c: char) {
        let Some(control @ Control::Stage(_)) = self.focused() else {
            return;
        };
        let digit = c.to_digit(10).unwrap_or(0);
        let dpi = match self.edit {
            Some(Edit {
                control: edited,
                value: Value::Dpi(dpi),
                typed: true,
            }) if edited == control => dpi.saturating_mul(10).saturating_add(digit).min(999_999),
            Some(_) | None => digit,
        };
        self.set_edit(control, Value::Dpi(dpi), true);
    }

    fn backspace(&mut self) {
        if let Some(Edit {
            control,
            value: Value::Dpi(dpi),
            typed: true,
        }) = self.edit
        {
            self.edit = (dpi >= 10).then_some(Edit {
                control,
                value: Value::Dpi(dpi / 10),
                typed: true,
            });
        }
    }

    /// Enter: sends the edit; without one, turns a switch on or off, or makes
    /// a DPI stage the active one.
    fn confirm(&mut self) -> Option<Action> {
        let control = self.focused()?;
        let f = self.features()?;
        let edit = self.edit.take().filter(|e| e.control == control);
        let value = match edit {
            Some(Edit {
                value: Value::Dpi(dpi),
                typed: true,
                ..
            }) => {
                if !(f.dpi.min..=f.dpi.max).contains(&dpi) {
                    self.error(format!("DPI must be {} to {}", f.dpi.min, f.dpi.max));
                    self.edit = edit;
                    return None;
                }
                Value::Dpi(protocol::snap_dpi(&f.dpi, dpi))
            }
            Some(edit) => edit.value,
            None => match (control, self.value(control)?) {
                (Control::Stage(i), _) => {
                    if !f.dpi.switchable || self.settings.active_stage == Some(i) {
                        return None;
                    }
                    return Some(Action::Apply {
                        change: Change::ActiveStage(i),
                        done: format!("DPI stage {} is active", i + 1),
                    });
                }
                (_, Value::Switch(on)) => Value::Switch(!on),
                // Other values change with ←/→ first.
                (_, _) => return None,
            },
        };
        if self.value(control) == Some(value) {
            return None;
        }
        Some(Action::Apply {
            change: self.change_for(control, value)?,
            done: format!("{} set to {value}", control.label()),
        })
    }

    fn change_for(&self, control: Control, value: Value) -> Option<Change> {
        let s = &self.settings;
        // A value only ever belongs to its own control; other pairs don't occur.
        Some(match (control, value) {
            (Control::Stage(stage), Value::Dpi(dpi)) => Change::StageDpi {
                stage,
                dpi,
                color: s.stages.get(stage)?.color,
            },
            (Control::PollingRate, Value::PollingRate(rate)) => Change::PollingRate(rate),
            (Control::Debounce, Value::Debounce(d)) => Change::Debounce(d),
            (Control::AngleSnapping, Value::Switch(on)) => Change::AngleSnapping(on),
            (Control::AngleTuning, Value::Degrees(d)) => Change::AngleTuning(d),
            (Control::MotionSync, Value::Switch(on)) => Change::MotionSync(on),
            (Control::LiftOff, Value::LiftOff(l)) => Change::LiftOff(l),
            (Control::PowerOff, Value::PowerOff(power_off)) => Change::Energy {
                power_off,
                low_battery_warning: s.low_battery_warning.unwrap_or(0),
            },
            (Control::LowBatteryWarning, Value::Percent(low_battery_warning)) => Change::Energy {
                power_off: s.power_off.unwrap_or(PowerOff::NEVER),
                low_battery_warning,
            },
            (Control::PowerSaving, Value::Switch(on)) => Change::PowerSaving(PowerSaving {
                on,
                ..s.power_saving?
            }),
            (Control::PowerSavingRate, Value::PollingRate(rate)) => {
                Change::PowerSaving(PowerSaving {
                    rate,
                    ..s.power_saving?
                })
            }
            (_, _) => return None,
        })
    }

    /// In demo mode: what reading the mouse back after `change` would show.
    pub fn simulate(&mut self, change: &Change) {
        let Some(f) = self.features() else {
            return;
        };
        let s = &mut self.settings;
        match *change {
            Change::ActiveStage(i) => s.active_stage = Some(i),
            Change::StageDpi { stage, dpi, .. } => {
                if let Some(st) = s.stages.get_mut(stage) {
                    st.dpi = protocol::snap_dpi(&f.dpi, dpi);
                }
            }
            Change::PollingRate(rate) => s.polling_rate = Some(rate),
            Change::Debounce(d) => s.debounce = Some(d),
            Change::AngleSnapping(on) => s.angle_snapping = Some(on),
            Change::AngleTuning(d) => s.angle_tuning = Some(d),
            Change::MotionSync(on) => s.motion_sync = Some(on),
            Change::LiftOff(l) => s.lift_off = Some(l),
            Change::Energy {
                power_off,
                low_battery_warning,
            } => {
                s.power_off = s.power_off.map(|_| power_off);
                s.low_battery_warning = s.low_battery_warning.map(|_| low_battery_warning);
            }
            Change::PowerSaving(p) => s.power_saving = Some(p),
        }
    }
}

/// The next value ←/→ gives. `big` (Shift, PgUp/PgDn) jumps further.
/// `rates` are the polling rates the control offers.
fn step(f: &Features, rates: &[PollingRate], current: Value, dir: i8, big: bool) -> Value {
    let up = dir > 0;
    match current {
        Value::Dpi(dpi) => {
            let by = f.dpi.step.max(50) * if big { 10 } else { 1 };
            let next = if up {
                dpi.saturating_add(by)
            } else {
                dpi.saturating_sub(by)
            };
            Value::Dpi(protocol::snap_dpi(&f.dpi, next))
        }
        Value::PollingRate(rate) => {
            let rates = rates.iter().copied();
            let next = match (up, big) {
                (true, true) => rates.last(),
                (false, true) => rates.min(),
                (true, false) => rates.filter(|&r| r > rate).min(),
                (false, false) => rates.filter(|&r| r < rate).max(),
            };
            Value::PollingRate(next.unwrap_or(rate))
        }
        Value::Debounce(d) => Value::Debounce(match (up, big) {
            (true, true) => Debounce::MAX,
            (false, true) => Debounce::MIN,
            (true, false) => Debounce::from_code(d.code().saturating_add(1)),
            (false, false) => Debounce::from_code(d.code().saturating_sub(1)),
        }),
        Value::Switch(on) => Value::Switch(!on),
        Value::Degrees(d) => {
            let Some(t) = f.angle_tuning else {
                return current;
            };
            let by = t.step * if big { 5 } else { 1 };
            let next = if up {
                d.saturating_add(by)
            } else {
                d.saturating_sub(by)
            };
            Value::Degrees(next.clamp(t.min, t.max))
        }
        Value::LiftOff(LiftOff::Low) => Value::LiftOff(LiftOff::High),
        Value::LiftOff(LiftOff::High) => Value::LiftOff(LiftOff::Low),
        Value::LiftOff(LiftOff::Unknown(_)) => {
            Value::LiftOff(if up { LiftOff::High } else { LiftOff::Low })
        }
        Value::PowerOff(p) => {
            let choices = PowerOff::CHOICES;
            let at = choices.iter().position(|&c| c == p);
            let last = choices.len() - 1;
            let next = match (at, up, big) {
                (_, true, true) | (None, true, false) => last,
                (_, false, true) | (None, false, false) => 0,
                (Some(i), true, false) => (i + 1).min(last),
                (Some(i), false, false) => i.saturating_sub(1),
            };
            Value::PowerOff(choices[next])
        }
        Value::Percent(pct) => {
            let Some(w) = f.low_battery_warning else {
                return current;
            };
            let by = if big { w.max } else { w.step };
            let next = if up {
                pct.saturating_add(by)
            } else {
                pct.saturating_sub(by)
            };
            Value::Percent((next / w.step * w.step).min(w.max))
        }
    }
}

/// Finds a model by name for `--demo`, ignoring case; the wireless entry if
/// there are both.
pub fn find_model(name: &str) -> Option<&'static Model> {
    let named = || MODELS.iter().filter(|m| m.name.eq_ignore_ascii_case(name));
    named().find(|m| m.wireless).or_else(|| named().next())
}

pub const DEMO_MODEL: &str = "ROG Harpe II Ace";

/// Plausible settings for `--demo`, within what `f` allows.
pub fn demo_state(f: &Features) -> (Reading, MouseSettings) {
    const COLORS: [[u8; 3]; 4] = [[0xff, 0, 0], [0, 0xff, 0], [0, 0x80, 0xff], [0xff, 0xff, 0]];
    let d = &f.dpi;
    let stages = [400, 800, 1600, 3200]
        .into_iter()
        .zip(COLORS)
        .take(usize::from(d.stages))
        .map(|(dpi, color)| DpiStage {
            dpi: protocol::snap_dpi(d, dpi),
            color: d.colors.then_some(color),
        })
        .collect();
    let power_off = f.power_off.map(|_| PowerOff::from_code(4));
    let low_battery_warning = f.low_battery_warning.map(|w| (20 / w.step).max(1) * w.step);
    let reading = Reading {
        battery: 80,
        charging: false,
        power_off,
        low_battery_warning,
    };
    let settings = MouseSettings {
        stages,
        active_stage: (d.stages > 1).then_some(1),
        polling_rate: f
            .polling_rates
            .iter()
            .copied()
            .filter(|r| r.hz() <= 1000)
            .max()
            .or_else(|| f.polling_rates.first().copied()),
        debounce: f.debounce.then_some(Debounce::MIN),
        angle_snapping: f.angle_snapping.then_some(false),
        angle_tuning: f.angle_tuning.map(|_| 0),
        motion_sync: f.motion_sync.then_some(true),
        lift_off: f.lift_off.map(|_| LiftOff::Low),
        power_off,
        low_battery_warning,
        power_saving: f
            .power_saving_rates
            .iter()
            .copied()
            .filter(|r| r.hz() <= 1000)
            .max()
            .map(|rate| PowerSaving { on: false, rate }),
    };
    (reading, settings)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn demo(name: &str) -> App {
        let model = find_model(name).unwrap();
        let (reading, settings) = demo_state(&model.features);
        let mut app = App::new(true);
        app.show(Some(model), Link::Connected, Some(reading), settings);
        app
    }

    fn press(app: &mut App, code: KeyCode) -> Option<Action> {
        app.handle_key(code, KeyModifiers::NONE)
    }

    fn focus(app: &mut App, control: Control) {
        for _ in 0..app.controls().len() {
            press(app, KeyCode::Up);
        }
        while app.focused() != Some(control) {
            assert!(
                app.controls().last() != app.focused().as_ref(),
                "{control:?} not reachable"
            );
            press(app, KeyCode::Down);
        }
    }

    fn applied(action: Option<Action>) -> Change {
        match action {
            Some(Action::Apply { change, .. }) => change,
            other => panic!("expected a change, got {other:?}"),
        }
    }

    #[test]
    fn rows_follow_one_order_and_skip_what_a_model_lacks() {
        let controls = |name| demo(name).controls();
        assert_eq!(
            controls("ROG Harpe II Ace"),
            [
                Control::Stage(0),
                Control::Stage(1),
                Control::Stage(2),
                Control::Stage(3),
                Control::PollingRate,
                Control::AngleSnapping,
                Control::AngleTuning,
                Control::MotionSync,
                Control::LiftOff,
                Control::PowerOff,
                Control::LowBatteryWarning,
                Control::PowerSaving,
            ]
        );
        assert_eq!(
            controls("ASUS Mouse MD200"),
            [
                Control::Stage(0),
                Control::Stage(1),
                Control::PollingRate,
                Control::Debounce,
            ]
        );
    }

    #[test]
    fn every_model_has_a_usable_demo() {
        for m in MODELS {
            let mut app = demo(m.name);
            assert!(!app.controls().is_empty(), "{}", m.name);
            for control in app.controls() {
                assert!(app.value(control).is_some(), "{} {control:?}", m.name);
            }
            // Walking every row and nudging it must never panic or lose focus.
            for _ in 0..app.controls().len() {
                press(&mut app, KeyCode::Right);
                press(&mut app, KeyCode::Left);
                press(&mut app, KeyCode::Down);
                assert!(app.focused().is_some(), "{}", m.name);
            }
        }
    }

    #[test]
    fn nothing_is_sent_until_enter() {
        let mut app = demo("ROG Harpe II Ace");
        focus(&mut app, Control::PollingRate);
        assert_eq!(press(&mut app, KeyCode::Right), None);
        assert_eq!(press(&mut app, KeyCode::Right), None);
        assert_eq!(
            app.edit.map(|e| e.value),
            Some(Value::PollingRate(PollingRate::HZ_4000))
        );
        assert_eq!(
            applied(press(&mut app, KeyCode::Enter)),
            Change::PollingRate(PollingRate::HZ_4000)
        );
        assert_eq!(app.edit, None);
    }

    #[test]
    fn esc_and_moving_away_cancel_an_edit() {
        let mut app = demo("ROG Harpe II Ace");
        focus(&mut app, Control::PollingRate);
        press(&mut app, KeyCode::Left);
        press(&mut app, KeyCode::Esc);
        assert_eq!(app.edit, None);
        press(&mut app, KeyCode::Left);
        press(&mut app, KeyCode::Down);
        assert_eq!(app.edit, None);
        assert_eq!(press(&mut app, KeyCode::Up), None);
        assert_eq!(press(&mut app, KeyCode::Enter), None, "no edit to send");
    }

    #[test]
    fn returning_to_the_mouse_value_is_no_edit() {
        let mut app = demo("ROG Harpe II Ace");
        focus(&mut app, Control::PollingRate);
        press(&mut app, KeyCode::Right);
        press(&mut app, KeyCode::Left);
        assert_eq!(app.edit, None);
    }

    #[test]
    fn dpi_steps_clamp_and_accept_typed_values() {
        let mut app = demo("ROG Harpe II Ace");
        // Stage 1 is 400; 50 per step, 500 with Shift, never under 100.
        press(&mut app, KeyCode::Right);
        assert_eq!(app.edit.map(|e| e.value), Some(Value::Dpi(450)));
        app.handle_key(KeyCode::Left, KeyModifiers::SHIFT);
        assert_eq!(app.edit.map(|e| e.value), Some(Value::Dpi(100)));

        for c in "1625".chars() {
            press(&mut app, KeyCode::Char(c));
        }
        assert_eq!(
            applied(press(&mut app, KeyCode::Enter)),
            Change::StageDpi {
                stage: 0,
                dpi: 1650,
                color: Some([0xff, 0, 0])
            },
            "rounded to the nearest 50, with the stage colour"
        );

        for c in "99".chars() {
            press(&mut app, KeyCode::Char(c));
        }
        assert_eq!(press(&mut app, KeyCode::Enter), None, "under 100");
        assert!(app.message.as_ref().is_some_and(|m| m.error));
        press(&mut app, KeyCode::Backspace);
        press(&mut app, KeyCode::Backspace);
        assert_eq!(app.edit, None);
    }

    #[test]
    fn enter_makes_a_stage_active_when_the_mouse_allows_it() {
        let mut app = demo("ROG Harpe II Ace");
        assert_eq!(
            applied(press(&mut app, KeyCode::Enter)),
            Change::ActiveStage(0)
        );
        focus(&mut app, Control::Stage(1));
        assert_eq!(press(&mut app, KeyCode::Enter), None, "already active");

        let mut app = demo("ROG Chakram");
        assert_eq!(press(&mut app, KeyCode::Enter), None, "not switchable");
    }

    #[test]
    fn switches_toggle_on_enter() {
        let mut app = demo("ROG Harpe II Ace");
        focus(&mut app, Control::AngleSnapping);
        assert_eq!(
            applied(press(&mut app, KeyCode::Char(' '))),
            Change::AngleSnapping(true)
        );
    }

    #[test]
    fn motion_sync_locks_at_8000_hz() {
        let mut app = demo("ROG Harpe II Ace");
        assert!(app.controls().contains(&Control::MotionSync));
        app.simulate(&Change::PollingRate(PollingRate::HZ_8000));
        assert!(!app.controls().contains(&Control::MotionSync));
        let row = app
            .rows()
            .into_iter()
            .find(|r| r.control == Control::MotionSync);
        assert!(row.unwrap().locked.is_some(), "still shown");
    }

    #[test]
    fn energy_changes_keep_the_other_value() {
        let mut app = demo("ROG Harpe II Ace");
        focus(&mut app, Control::LowBatteryWarning);
        press(&mut app, KeyCode::Right);
        assert_eq!(
            applied(press(&mut app, KeyCode::Enter)),
            Change::Energy {
                power_off: PowerOff::from_code(4),
                low_battery_warning: 30
            }
        );

        focus(&mut app, Control::PowerOff);
        press(&mut app, KeyCode::Right);
        assert_eq!(
            applied(press(&mut app, KeyCode::Enter)),
            Change::Energy {
                power_off: PowerOff::NEVER,
                low_battery_warning: 20
            }
        );
    }

    #[test]
    fn warning_steps_in_quarters_where_the_mouse_needs_it() {
        let mut app = demo("ROG Keris Wireless");
        focus(&mut app, Control::LowBatteryWarning);
        assert_eq!(
            app.value(Control::LowBatteryWarning),
            Some(Value::Percent(25))
        );
        press(&mut app, KeyCode::Right);
        press(&mut app, KeyCode::Right);
        assert_eq!(
            app.edit.map(|e| e.value),
            Some(Value::Percent(50)),
            "max 50"
        );
    }

    #[test]
    fn simulated_changes_show_up() {
        let mut app = demo("ROG Harpe II Ace");
        focus(&mut app, Control::Stage(2));
        press(&mut app, KeyCode::Right);
        let change = applied(press(&mut app, KeyCode::Enter));
        app.simulate(&change);
        assert_eq!(app.value(Control::Stage(2)), Some(Value::Dpi(1650)));
        assert_eq!(app.focused(), Some(Control::Stage(2)), "focus kept");
    }

    #[test]
    fn help_swallows_keys_until_closed() {
        let mut app = demo("ROG Harpe II Ace");
        press(&mut app, KeyCode::Char('?'));
        assert_eq!(press(&mut app, KeyCode::Enter), None);
        assert_eq!(press(&mut app, KeyCode::Char('r')), None);
        press(&mut app, KeyCode::Esc);
        assert!(!app.help);
        assert_eq!(press(&mut app, KeyCode::Char('r')), Some(Action::Refresh));
    }

    /// The rate is locked while power saving is off, offers only its own
    /// rates, and every change sends the switch and the rate together.
    #[test]
    fn power_saving_sends_switch_and_rate_together() {
        let mut app = demo("ROG Harpe II Ace");
        let row = app
            .rows()
            .into_iter()
            .find(|r| r.control == Control::PowerSavingRate);
        assert!(row.unwrap().locked.is_some(), "off in the demo");

        focus(&mut app, Control::PowerSaving);
        let on = PowerSaving {
            on: true,
            rate: PollingRate::HZ_1000,
        };
        assert_eq!(
            applied(press(&mut app, KeyCode::Enter)),
            Change::PowerSaving(on)
        );
        app.simulate(&Change::PowerSaving(on));

        focus(&mut app, Control::PowerSavingRate);
        app.handle_key(KeyCode::Right, KeyModifiers::SHIFT);
        assert_eq!(
            app.edit.map(|e| e.value),
            Some(Value::PollingRate(PollingRate::HZ_2000)),
            "tops out at 2000 Hz, unlike the polling rate"
        );
        assert_eq!(
            applied(press(&mut app, KeyCode::Enter)),
            Change::PowerSaving(PowerSaving {
                on: true,
                rate: PollingRate::HZ_2000
            })
        );
    }

    /// The scale a row draws and the values ←/→ step through are the same
    /// list, for every model.
    #[test]
    fn arrows_walk_exactly_the_choices() {
        for m in MODELS {
            let app = demo(m.name);
            for control in app.controls() {
                let Some(choices) = app.choices(control) else {
                    continue;
                };
                let first = choices[0];
                let (Some(f), Some(current)) = (app.features(), app.value(control)) else {
                    panic!("{} {control:?}", m.name);
                };
                assert!(choices.contains(&current), "{} {control:?}", m.name);
                let rates = match control {
                    Control::PowerSavingRate => f.power_saving_rates,
                    _ => f.polling_rates,
                };
                let mut walked = vec![first];
                while walked.len() <= choices.len() {
                    let next = step(f, rates, *walked.last().unwrap(), 1, false);
                    if Some(&next) == walked.last() {
                        break;
                    }
                    walked.push(next);
                }
                assert_eq!(walked, choices, "{} {control:?}", m.name);
            }
        }
    }

    /// A value rogctl can't decode still shows, and ←/→ replace it.
    #[test]
    fn unknown_lift_off_stays_visible_and_settable() {
        let mut app = demo("ROG Harpe II Ace");
        app.settings.lift_off = Some(LiftOff::Unknown(1));
        focus(&mut app, Control::LiftOff);
        assert_eq!(
            app.value(Control::LiftOff)
                .map(|v| v.to_string())
                .as_deref(),
            Some("Unknown (0x01)")
        );
        assert_eq!(press(&mut app, KeyCode::Enter), None, "nothing to send yet");
        press(&mut app, KeyCode::Right);
        assert_eq!(
            applied(press(&mut app, KeyCode::Enter)),
            Change::LiftOff(LiftOff::High)
        );
    }

    const CLICK: MouseEventKind = MouseEventKind::Down(MouseButton::Left);

    fn click(app: &mut App, target: Target) -> Option<Action> {
        app.handle_mouse(CLICK, Some(target))
    }

    #[test]
    fn first_click_selects_and_second_applies_like_enter() {
        let mut app = demo("ROG Harpe II Ace");
        let snapping = Target::Row(Control::AngleSnapping);
        assert_eq!(click(&mut app, snapping), None);
        assert_eq!(app.focused(), Some(Control::AngleSnapping));
        assert_eq!(
            applied(click(&mut app, snapping)),
            Change::AngleSnapping(true)
        );

        // A pending edit is dropped by selecting another row, applied by a
        // click on its own.
        focus(&mut app, Control::Stage(0));
        press(&mut app, KeyCode::Right);
        assert_eq!(click(&mut app, Target::Row(Control::Stage(1))), None);
        assert_eq!(app.edit, None);
        press(&mut app, KeyCode::Right);
        assert_eq!(
            applied(click(&mut app, Target::Row(Control::Stage(1)))),
            Change::StageDpi {
                stage: 1,
                dpi: 850,
                color: Some([0, 0xff, 0])
            }
        );
    }

    #[test]
    fn scale_clicks_and_the_wheel_only_edit() {
        let mut app = demo("ROG Harpe II Ace");
        let rate = |rate| Target::Choice(Control::PollingRate, Value::PollingRate(rate));
        let pending = |app: &App| app.edit.map(|e| e.value);
        assert_eq!(click(&mut app, rate(PollingRate::HZ_4000)), None);
        assert_eq!(app.focused(), Some(Control::PollingRate), "selected");
        assert_eq!(
            pending(&app),
            Some(Value::PollingRate(PollingRate::HZ_4000))
        );
        assert_eq!(click(&mut app, rate(PollingRate::HZ_1000)), None);
        assert_eq!(app.edit, None, "back to the mouse's value");
        assert_eq!(click(&mut app, rate(PollingRate::HZ_1000)), None);
        assert_eq!(click(&mut app, rate(PollingRate::HZ_250)), None);
        assert_eq!(
            applied(click(&mut app, rate(PollingRate::HZ_250))),
            Change::PollingRate(PollingRate::HZ_250),
            "the pending value again applies it"
        );

        let warning = Some(Target::Row(Control::LowBatteryWarning));
        for (kind, pct) in [
            (MouseEventKind::ScrollUp, 30),
            (MouseEventKind::ScrollUp, 40),
            (MouseEventKind::ScrollDown, 30),
        ] {
            assert_eq!(app.handle_mouse(kind, warning), None);
            assert_eq!(pending(&app), Some(Value::Percent(pct)));
        }
    }

    #[test]
    fn locked_rows_help_and_other_buttons_ignore_the_mouse() {
        let mut app = demo("ROG Harpe II Ace");
        let locked = Target::Row(Control::PowerSavingRate);
        assert_eq!(click(&mut app, locked), None);
        assert_eq!(app.focused(), Some(Control::Stage(0)));

        let stage = Some(Target::Row(Control::Stage(0)));
        for kind in [
            MouseEventKind::Down(MouseButton::Right),
            MouseEventKind::Up(MouseButton::Left),
            MouseEventKind::Drag(MouseButton::Left),
            MouseEventKind::Moved,
        ] {
            assert_eq!(app.handle_mouse(kind, stage), None);
        }
        assert_eq!(app.handle_mouse(CLICK, None), None);
        assert_eq!(app.edit, None);

        press(&mut app, KeyCode::Char('?'));
        assert_eq!(app.handle_mouse(MouseEventKind::ScrollUp, stage), None);
        assert!(app.help, "only a click closes help");
        assert_eq!(app.handle_mouse(CLICK, stage), None);
        assert!(!app.help);
        assert_eq!(app.edit, None, "the click did nothing else");
    }

    #[test]
    fn values_read_like_the_widgets() {
        assert_eq!(
            Value::PollingRate(PollingRate::HZ_1000).to_string(),
            "1000 Hz"
        );
        assert_eq!(Value::Debounce(Debounce::MIN).to_string(), "12 ms");
        assert_eq!(Value::Degrees(-5).to_string(), "-5°");
        assert_eq!(Value::Degrees(5).to_string(), "+5°");
        assert_eq!(Value::Percent(0).to_string(), "Off");
    }
}
