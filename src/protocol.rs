//! Every byte rogctl sends to a mouse or reads back: request builders, reply
//! parsers and the value types they use. Transcribed from G-Helper's
//! `app/Peripherals/Mouse/AsusMouse.cs` and its model overrides
//! (https://github.com/seerge/g-helper). What each model supports is in
//! `models.rs`; the I/O is in `device.rs`.
//!
//! A request is `[report_id, command, …]`, zero-padded to the model's packet
//! size. The reply echoes the request's first bytes, or is `FF AA` in bytes 1-2
//! when the mouse rejects it. Changes (`SET`) are stored on the mouse only after
//! a `SAVE`.

use crate::device::QueryError;

/// Byte 1: the command.
const GET: u8 = 0x12;
const SET: u8 = 0x51;
const PROFILE: u8 = 0x50;

/// Byte 2 of a `GET`: what to read.
const GET_PROFILE: u8 = 0x00;
const GET_CONFIG: u8 = 0x04;
const GET_LIFT_OFF: u8 = 0x06;
const GET_BATTERY: u8 = 0x07;
const GET_POWER_SAVING: u8 = 0x15;

/// Byte 3 of `GET_CONFIG`: which page.
const PAGE_MAIN: u8 = 0x00;
const PAGE_XY_DPI: u8 = 0x02;
const PAGE_DPI_COLORS: u8 = 0x03;
const PAGE_MOTION_SYNC: u8 = 0x04;

/// Byte 2 of a `SET`.
const SET_CONFIG: u8 = 0x31;
const SET_LIFT_OFF: u8 = 0x35;
const SET_ENERGY: u8 = 0x37;
const SET_POWER_SAVING: u8 = 0x43;

/// Byte 3 of `SET_CONFIG` when it isn't a DPI stage (0-3). The polling rate,
/// debounce and angle snapping fields depend on the [`Layout`].
const CONFIG_ACTIVE_STAGE: u8 = 0x09;
const CONFIG_ANGLE_TUNING: u8 = 0x0b;
const CONFIG_MOTION_SYNC: u8 = 0x12;

/// Byte 2 of `PROFILE`: write the changed settings to the mouse's memory.
const SAVE: u8 = 0x03;

/// Asks the Omni receiver what is paired with it. Report ID 1, whatever the
/// mouse uses.
pub const OMNI_PAIRED: [u8; 2] = [0x01, 0xa0];

/// Longest request built here: a DPI stage with its colour.
const REQUEST_MAX: usize = 10;

/// A request before padding, and how much of it the reply echoes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Request {
    bytes: [u8; REQUEST_MAX],
    len: usize,
    echo: usize,
}

impl Request {
    fn new(bytes: &[u8], echo: usize) -> Self {
        let len = bytes.len().min(REQUEST_MAX);
        let mut buf = [0u8; REQUEST_MAX];
        buf[..len].copy_from_slice(&bytes[..len]);
        Self {
            bytes: buf,
            len,
            echo: echo.min(len),
        }
    }

    pub fn bytes(&self) -> &[u8] {
        &self.bytes[..self.len]
    }

    /// Whether `reply` answers this request: it echoes the header, or is an
    /// error or empty packet. Other input reports can share the interface.
    pub fn answered_by(&self, reply: &[u8]) -> bool {
        reply.get(..self.echo) == Some(&self.bytes[..self.echo])
            || is_rejected(reply)
            || is_empty(reply)
    }
}

fn byte(reply: &[u8], i: usize) -> u8 {
    reply.get(i).copied().unwrap_or(0)
}

fn is_rejected(reply: &[u8]) -> bool {
    byte(reply, 1) == 0xff && byte(reply, 2) == 0xaa
}

fn is_empty(reply: &[u8]) -> bool {
    (1..4).all(|i| byte(reply, i) == 0)
}

/// Turns an error or empty reply into the matching error.
pub fn check_reply(reply: &[u8]) -> Result<(), QueryError> {
    if is_rejected(reply) {
        Err(QueryError::Rejected)
    } else if is_empty(reply) {
        Err(QueryError::Empty)
    } else {
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// What a model supports
// ---------------------------------------------------------------------------

/// Where the battery level sits in the battery reply.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Battery {
    /// Byte 5 is the percentage.
    Percent,
    /// `byte` is a 0-4 level, reported as multiples of 25%.
    Quarters { byte: usize },
}

/// Which settings a mouse has, and where they sit in its replies.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Features {
    pub dpi: Dpi,
    pub layout: Layout,
    /// Rates the mouse accepts, slowest first. Empty: not settable.
    pub polling_rates: &'static [PollingRate],
    pub debounce: bool,
    pub angle_snapping: bool,
    pub angle_tuning: Option<AngleTuning>,
    pub motion_sync: bool,
    pub lift_off: Option<LiftOffFormat>,
    /// Byte of the battery reply holding the auto power-off code.
    pub power_off: Option<usize>,
    pub low_battery_warning: Option<WarningRange>,
    /// Rates advanced power saving offers, slowest first. Empty: no such mode.
    /// Not in G-Helper; captured from ASUS GearLink on a Harpe II Ace.
    pub power_saving_rates: &'static [PollingRate],
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Dpi {
    /// Stages the mouse stores; 0 for a receiver with no known mouse.
    pub stages: u8,
    pub min: u32,
    pub max: u32,
    /// DPI values are multiples of this.
    pub step: u32,
    /// Stages are read from the X/Y page (4 bytes each) instead of the main one.
    pub xy: bool,
    /// Each stage has a colour, sent along with its DPI.
    pub colors: bool,
    /// The mouse uses 2-4 of its stages, and says how many in the profile reply.
    pub variable_stages: bool,
    /// rogctl can pick the active stage. Otherwise only the mouse's DPI button can.
    pub switchable: bool,
    /// Byte of the profile reply holding the active stage, counted from 1.
    pub active_byte: usize,
}

/// Where the main config page keeps the polling rate, debounce and angle
/// snapping, and the `SET_CONFIG` fields that change them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Layout {
    Standard,
    /// G-Helper's Gladius II and Strix Carry classes: 4 bytes earlier, and
    /// numbered differently.
    Legacy,
}

impl Layout {
    /// (reply byte, `SET_CONFIG` field)
    fn polling_rate(self) -> (usize, u8) {
        match self {
            Self::Standard => (13, 0x04),
            Self::Legacy => (9, 0x02),
        }
    }

    fn debounce(self) -> (usize, u8) {
        match self {
            Self::Standard => (15, 0x05),
            Self::Legacy => (11, 0x03),
        }
    }

    fn angle_snapping(self) -> (usize, u8) {
        match self {
            Self::Standard => (17, 0x06),
            Self::Legacy => (13, 0x04),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AngleTuning {
    pub min: i16,
    pub max: i16,
    pub step: i16,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LiftOffFormat {
    /// Reply byte 8, High = 1. Setting it also resets the sensor calibration.
    Standard,
    /// Reply byte 5, High = 1, and a shorter set command.
    Legacy,
    /// Harpe II: like `Standard`, but High = 2 and the set command ends in
    /// `01`. G-Helper has `Standard` here, which the mouse ignores; these bytes
    /// were captured from ASUS GearLink on a Harpe II Ace (SpeedNova).
    HarpeII,
}

impl LiftOffFormat {
    /// Reply byte holding the distance.
    fn reply_byte(self) -> usize {
        match self {
            Self::Standard | Self::HarpeII => 8,
            Self::Legacy => 5,
        }
    }

    fn code(self, distance: LiftOff) -> u8 {
        match (self, distance) {
            (Self::Standard | Self::Legacy | Self::HarpeII, LiftOff::Low) => 0,
            (Self::Standard | Self::Legacy, LiftOff::High) => 1,
            (Self::HarpeII, LiftOff::High) => 2,
            (Self::Standard | Self::Legacy | Self::HarpeII, LiftOff::Unknown(code)) => code,
        }
    }

    fn decode(self, code: u8) -> LiftOff {
        [LiftOff::Low, LiftOff::High]
            .into_iter()
            .find(|&d| self.code(d) == code)
            .unwrap_or(LiftOff::Unknown(code))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WarningRange {
    pub step: u8,
    pub max: u8,
    /// The mouse stores the level in quarters (1 = 25%).
    pub quarters: bool,
}

// ---------------------------------------------------------------------------
// Values
// ---------------------------------------------------------------------------

/// A polling rate code: 125 Hz doubled `code` times.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct PollingRate(u8);

impl PollingRate {
    pub const HZ_125: Self = Self(0);
    pub const HZ_250: Self = Self(1);
    pub const HZ_500: Self = Self(2);
    pub const HZ_1000: Self = Self(3);
    pub const HZ_2000: Self = Self(4);
    pub const HZ_4000: Self = Self(5);
    pub const HZ_8000: Self = Self(6);

    fn from_byte(b: u8) -> Self {
        Self(b & 0x07)
    }

    pub fn hz(self) -> u32 {
        125 << self.0.min(7)
    }
}

/// A debounce code, 2 (12 ms) to 7 (32 ms).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct Debounce(u8);

impl Debounce {
    pub const MIN: Self = Self(2);
    pub const MAX: Self = Self(7);

    /// Clamps to the range G-Helper offers, as it does when reading.
    pub fn from_code(code: u8) -> Self {
        Self(code.clamp(Self::MIN.0, Self::MAX.0))
    }

    pub fn code(self) -> u8 {
        self.0
    }

    pub fn ms(self) -> u32 {
        4 * (u32::from(self.0) + 1)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LiftOff {
    Low,
    High,
    /// A code this model's format doesn't define. Shown, never sent.
    Unknown(u8),
}

/// An auto power-off code.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PowerOff(u8);

impl PowerOff {
    pub const NEVER: Self = Self(0xff);
    /// Every setting, shortest first.
    pub const CHOICES: [Self; 6] = [Self(0), Self(1), Self(2), Self(3), Self(4), Self::NEVER];

    pub fn from_code(code: u8) -> Self {
        Self(code)
    }

    /// None for "never" and codes G-Helper doesn't know.
    pub fn minutes(self) -> Option<u8> {
        match self.0 {
            0 => Some(1),
            1 => Some(2),
            2 => Some(3),
            3 => Some(5),
            4 => Some(10),
            _ => None,
        }
    }
}

impl std::fmt::Display for PowerOff {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match (self.minutes(), *self == Self::NEVER) {
            (Some(m), _) => write!(f, "{m} min"),
            (None, true) => write!(f, "Never"),
            (None, false) => write!(f, "Unknown ({:#04x})", self.0),
        }
    }
}

// ---------------------------------------------------------------------------
// Reading
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Reading {
    /// Percent. 0 means the mouse is asleep or out of range: the receiver
    /// still answers, but with no data.
    pub battery: u8,
    pub charging: bool,
    pub power_off: Option<PowerOff>,
    /// Percent.
    pub low_battery_warning: Option<u8>,
}

/// A mouse's settings as last read. `None` or empty: the mouse doesn't have it.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct MouseSettings {
    pub stages: Vec<DpiStage>,
    /// Index into `stages`.
    pub active_stage: Option<usize>,
    pub polling_rate: Option<PollingRate>,
    pub debounce: Option<Debounce>,
    pub angle_snapping: Option<bool>,
    pub angle_tuning: Option<i16>,
    pub motion_sync: Option<bool>,
    pub lift_off: Option<LiftOff>,
    pub power_off: Option<PowerOff>,
    pub low_battery_warning: Option<u8>,
    pub power_saving: Option<PowerSaving>,
}

/// Advanced power saving: a slower polling rate the mouse uses while it's on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PowerSaving {
    pub on: bool,
    pub rate: PollingRate,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DpiStage {
    pub dpi: u32,
    pub color: Option<[u8; 3]>,
}

/// The reads that make up a full settings readback, after the battery.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Query {
    Battery,
    /// Active DPI stage and, on some mice, how many stages are in use.
    Profile,
    /// DPI stages from the X/Y page; other mice have them in `Config`.
    XyDpi,
    DpiColors,
    /// DPI (unless X/Y), polling rate, debounce, angle snapping and tuning.
    Config,
    MotionSync,
    LiftOff,
    PowerSaving,
}

impl Query {
    pub fn request(self, report_id: u8) -> Request {
        match self {
            Self::Battery => Request::new(&[report_id, GET, GET_BATTERY], 3),
            Self::Profile => Request::new(&[report_id, GET, GET_PROFILE, 0], 4),
            Self::XyDpi => Request::new(&[report_id, GET, GET_CONFIG, PAGE_XY_DPI], 4),
            Self::DpiColors => Request::new(&[report_id, GET, GET_CONFIG, PAGE_DPI_COLORS], 4),
            Self::Config => Request::new(&[report_id, GET, GET_CONFIG, PAGE_MAIN], 4),
            Self::MotionSync => Request::new(&[report_id, GET, GET_CONFIG, PAGE_MOTION_SYNC], 4),
            Self::LiftOff => Request::new(&[report_id, GET, GET_LIFT_OFF], 3),
            Self::PowerSaving => Request::new(&[report_id, GET, GET_POWER_SAVING], 3),
        }
    }
}

/// The queries after `Battery` that read every setting `f` has, in order:
/// `Profile` first, since it says how many DPI stages there are.
pub fn settings_queries(f: &Features) -> Vec<Query> {
    let mut queries = Vec::new();
    if f.dpi.stages > 1 {
        queries.push(Query::Profile);
    }
    if f.dpi.stages > 0 && f.dpi.xy {
        queries.push(Query::XyDpi);
    }
    if f.dpi.stages > 0 && f.dpi.colors {
        queries.push(Query::DpiColors);
    }
    if !f.dpi.xy && f.dpi.stages > 0
        || !f.polling_rates.is_empty()
        || f.debounce
        || f.angle_snapping
    {
        queries.push(Query::Config);
    }
    if f.motion_sync {
        queries.push(Query::MotionSync);
    }
    if f.lift_off.is_some() {
        queries.push(Query::LiftOff);
    }
    if !f.power_saving_rates.is_empty() {
        queries.push(Query::PowerSaving);
    }
    queries
}

/// Decodes a battery reply whose byte 0 is the report ID.
pub fn parse_battery(battery: Battery, f: &Features, reply: &[u8]) -> Result<Reading, QueryError> {
    check_reply(reply)?;
    let level = match battery {
        Battery::Percent => byte(reply, 5),
        Battery::Quarters { byte: at } => byte(reply, at).saturating_mul(25),
    };
    Ok(Reading {
        battery: level.min(100),
        charging: byte(reply, 10) > 0,
        power_off: f.power_off.map(|at| PowerOff(byte(reply, at))),
        low_battery_warning: f.low_battery_warning.map(|w| {
            let raw = byte(reply, 7);
            if w.quarters {
                raw.saturating_mul(25)
            } else {
                raw
            }
        }),
    })
}

/// Stores a reply to `query` in `s`. `Profile` must come before the DPI
/// queries, since it sets how many stages there are.
pub fn parse_settings(query: Query, f: &Features, reply: &[u8], s: &mut MouseSettings) {
    let dpi = &f.dpi;
    match query {
        Query::Battery => {}
        Query::Profile => {
            let count = byte(reply, 19);
            let count = if dpi.variable_stages && (2..=dpi.stages).contains(&count) {
                count
            } else {
                dpi.stages
            };
            s.stages = vec![
                DpiStage {
                    dpi: 0,
                    color: None
                };
                usize::from(count)
            ];
            s.active_stage = usize::from(byte(reply, dpi.active_byte))
                .checked_sub(1)
                .filter(|&i| i < s.stages.len());
        }
        Query::XyDpi => parse_stages(dpi, reply, 4, s),
        Query::DpiColors => {
            for (i, stage) in s.stages.iter_mut().enumerate() {
                let at = 5 + 3 * i;
                stage.color = Some([byte(reply, at), byte(reply, at + 1), byte(reply, at + 2)]);
            }
        }
        Query::Config => {
            if !dpi.xy {
                parse_stages(dpi, reply, 2, s);
            }
            if !f.polling_rates.is_empty() {
                s.polling_rate = Some(PollingRate::from_byte(byte(
                    reply,
                    f.layout.polling_rate().0,
                )));
            }
            if f.debounce {
                s.debounce = Some(Debounce::from_code(byte(reply, f.layout.debounce().0)));
            }
            if f.angle_snapping {
                s.angle_snapping = Some(byte(reply, f.layout.angle_snapping().0) == 0x01);
            }
            if f.angle_tuning.is_some() {
                s.angle_tuning = Some(i16::from_le_bytes([byte(reply, 19), byte(reply, 20)]));
            }
        }
        Query::MotionSync => s.motion_sync = Some(byte(reply, 5) == 0x01),
        Query::LiftOff => {
            let Some(format) = f.lift_off else {
                return;
            };
            s.lift_off = Some(format.decode(byte(reply, format.reply_byte())));
        }
        Query::PowerSaving => {
            if !f.power_saving_rates.is_empty() {
                s.power_saving = Some(PowerSaving {
                    on: byte(reply, 5) == 0x01,
                    rate: PollingRate::from_byte(byte(reply, 6)),
                });
            }
        }
    }
}

/// DPI stages are 16-bit little-endian, `width` bytes apart from byte 5, in
/// multiples of `step` counted from `step`.
fn parse_stages(dpi: &Dpi, reply: &[u8], width: usize, s: &mut MouseSettings) {
    if s.stages.is_empty() {
        s.stages = vec![
            DpiStage {
                dpi: 0,
                color: None
            };
            usize::from(dpi.stages)
        ];
    }
    for (i, stage) in s.stages.iter_mut().enumerate() {
        let at = 5 + width * i;
        let raw = u16::from_le_bytes([byte(reply, at), byte(reply, at + 1)]);
        stage.dpi = (u32::from(raw) + 1) * dpi.step;
    }
}

/// Parses the product IDs an Omni receiver reports as paired: little-endian,
/// every 4 bytes from byte 5, until a zero.
pub fn parse_omni_paired(reply: &[u8]) -> Vec<u16> {
    let Some(rest) = reply.get(5..) else {
        return Vec::new();
    };
    let (entries, _) = rest.as_chunks::<4>();
    entries
        .iter()
        .map(|&[lo, hi, _, _]| u16::from_le_bytes([lo, hi]))
        .take_while(|&pid| pid != 0)
        .collect()
}

// ---------------------------------------------------------------------------
// Changing
// ---------------------------------------------------------------------------

/// One setting change. Built by the TUI, sent by `device::Connection::apply`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Change {
    /// Index into the stages.
    ActiveStage(usize),
    /// `color` is sent back unchanged on mice with stage colours.
    StageDpi {
        stage: usize,
        dpi: u32,
        color: Option<[u8; 3]>,
    },
    PollingRate(PollingRate),
    Debounce(Debounce),
    AngleSnapping(bool),
    AngleTuning(i16),
    MotionSync(bool),
    LiftOff(LiftOff),
    /// Power-off and warning are one command, so both are sent.
    Energy {
        power_off: PowerOff,
        low_battery_warning: u8,
    },
    /// The switch and its rate are one command, so both are sent.
    PowerSaving(PowerSaving),
}

/// Rounds to the nearest multiple of `step` within the model's range.
pub fn snap_dpi(dpi: &Dpi, value: u32) -> u32 {
    let step = dpi.step.max(1);
    let rounded = (value + step / 2) / step * step;
    rounded.clamp(dpi.min, dpi.max)
}

/// The request for `change`, with values clamped to what `f` allows, or None
/// if the mouse doesn't have that setting.
pub fn change_request(report_id: u8, f: &Features, change: &Change) -> Option<Request> {
    let config = |field: u8, value: &[u8]| {
        let mut bytes = vec![report_id, SET, SET_CONFIG, field, 0x00];
        bytes.extend_from_slice(value);
        Request::new(&bytes, 3)
    };
    match *change {
        Change::ActiveStage(stage) => {
            let n = u8::try_from(stage + 1).ok()?;
            (f.dpi.switchable && n <= f.dpi.stages).then(|| config(CONFIG_ACTIVE_STAGE, &[n]))
        }
        Change::StageDpi { stage, dpi, color } => {
            let index = u8::try_from(stage).ok().filter(|&i| i < f.dpi.stages)?;
            let step = f.dpi.step.max(1);
            let raw = u16::try_from(snap_dpi(&f.dpi, dpi) / step - 1).ok()?;
            let [lo, hi] = raw.to_le_bytes();
            match (f.dpi.colors, color) {
                (true, Some([r, g, b])) => Some(config(index, &[lo, hi, r, g, b])),
                (true, None) => None,
                (false, _) => Some(config(index, &[lo, hi])),
            }
        }
        Change::PollingRate(rate) => f
            .polling_rates
            .contains(&rate)
            .then(|| config(f.layout.polling_rate().1, &[rate.0])),
        Change::Debounce(d) => f
            .debounce
            .then(|| config(f.layout.debounce().1, &[Debounce::from_code(d.0).0])),
        Change::AngleSnapping(on) => f
            .angle_snapping
            .then(|| config(f.layout.angle_snapping().1, &[u8::from(on)])),
        Change::AngleTuning(degrees) => f.angle_tuning.map(|t| {
            let [lo, hi] = degrees.clamp(t.min, t.max).to_le_bytes();
            config(CONFIG_ANGLE_TUNING, &[lo, hi])
        }),
        Change::MotionSync(on) => f
            .motion_sync
            .then(|| config(CONFIG_MOTION_SYNC, &[u8::from(on)])),
        Change::LiftOff(LiftOff::Unknown(_)) => None,
        Change::LiftOff(distance) => {
            let format = f.lift_off?;
            let v = format.code(distance);
            let bytes: &[u8] = match format {
                LiftOffFormat::Standard => &[report_id, SET, SET_LIFT_OFF, 0xff, 0x00, 0xff, v],
                LiftOffFormat::Legacy => &[report_id, SET, SET_LIFT_OFF, 0x00, 0x00, v],
                LiftOffFormat::HarpeII => {
                    &[report_id, SET, SET_LIFT_OFF, 0xff, 0x00, 0xff, v, 0x01]
                }
            };
            Some(Request::new(bytes, 3))
        }
        Change::Energy {
            power_off,
            low_battery_warning,
        } => {
            if f.power_off.is_none() && f.low_battery_warning.is_none() {
                return None;
            }
            let warning = match f.low_battery_warning {
                Some(w) => {
                    let pct = low_battery_warning.min(w.max);
                    if w.quarters { pct / 25 } else { pct }
                }
                None => 0,
            };
            Some(Request::new(
                &[
                    report_id,
                    SET,
                    SET_ENERGY,
                    0x00,
                    0x00,
                    power_off.0,
                    0x00,
                    warning,
                ],
                3,
            ))
        }
        Change::PowerSaving(PowerSaving { on, rate }) => {
            f.power_saving_rates.contains(&rate).then(|| {
                Request::new(
                    &[
                        report_id,
                        SET,
                        SET_POWER_SAVING,
                        0x00,
                        0x00,
                        u8::from(on),
                        rate.0,
                    ],
                    3,
                )
            })
        }
    }
}

/// Stores the changes made so far on the mouse.
pub fn save_request(report_id: u8) -> Request {
    Request::new(&[report_id, PROFILE, SAVE], 3)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::MODELS;

    fn features(name: &str, wireless: bool) -> &'static Features {
        &MODELS
            .iter()
            .find(|m| m.name == name && m.wireless == wireless)
            .unwrap()
            .features
    }

    fn reply(bytes: &[u8]) -> [u8; 66] {
        let mut buf = [0u8; 66];
        buf[..bytes.len()].copy_from_slice(bytes);
        buf
    }

    /// Puts `values` at `at` in an otherwise empty reply to `query`.
    fn reply_with(query: Query, report_id: u8, fields: &[(usize, &[u8])]) -> [u8; 66] {
        let mut buf = reply(Query::request(query, report_id).bytes());
        for (at, values) in fields {
            buf[*at..*at + values.len()].copy_from_slice(values);
        }
        buf
    }

    /// Captured from a Harpe II Ace on the SpeedNova receiver.
    #[test]
    fn parses_percent_battery() {
        let r = reply(&[
            0x03, 0x12, 0x07, 0, 0, 0x50, 0x02, 0x14, 0xd8, 0x0f, 0, 0, 0x01,
        ]);
        let f = features("ROG Harpe II Ace", true);
        let reading = parse_battery(Battery::Percent, f, &r).unwrap();
        assert_eq!(reading.battery, 80);
        assert!(!reading.charging);
        assert_eq!(reading.power_off.and_then(PowerOff::minutes), Some(3));
        assert_eq!(reading.low_battery_warning, Some(20));
    }

    #[test]
    fn parses_quarters_battery_and_warning() {
        let r = reply(&[0x00, 0x12, 0x07, 0, 0, 0x03, 0x01, 0x01, 0, 0, 0x01]);
        let reading = parse_battery(
            Battery::Quarters { byte: 5 },
            features("ROG Chakram", true),
            &r,
        )
        .unwrap();
        assert_eq!(reading.battery, 75);
        assert!(reading.charging);
        assert_eq!(reading.low_battery_warning, Some(25));
    }

    /// The Strix Carry keeps its power-off code where others keep the level.
    #[test]
    fn strix_carry_power_off_is_byte_5() {
        let r = reply(&[0x00, 0x12, 0x07, 0, 0, 0x04, 0, 0x02]);
        let reading = parse_battery(
            Battery::Quarters { byte: 7 },
            features("ROG Strix Carry", true),
            &r,
        )
        .unwrap();
        assert_eq!(reading.battery, 50);
        assert_eq!(reading.power_off.and_then(PowerOff::minutes), Some(10));
        assert_eq!(reading.low_battery_warning, None);
    }

    #[test]
    fn clamps_out_of_range_quarters() {
        let r = reply(&[0x00, 0x12, 0x07, 0, 0, 0x09]);
        let f = features("ROG Chakram", true);
        let battery = parse_battery(Battery::Quarters { byte: 5 }, f, &r)
            .unwrap()
            .battery;
        assert_eq!(battery, 100);
    }

    #[test]
    fn rejects_error_and_empty_replies() {
        let f = features("ROG Harpe II Ace", true);
        assert!(matches!(
            parse_battery(Battery::Percent, f, &reply(&[0x03, 0xff, 0xaa])),
            Err(QueryError::Rejected)
        ));
        assert!(matches!(
            parse_battery(Battery::Percent, f, &reply(&[0x03])),
            Err(QueryError::Empty)
        ));
    }

    #[test]
    fn replies_are_matched_on_the_echoed_header() {
        let config = Query::Config.request(0x03);
        assert!(config.answered_by(&reply(&[0x03, 0x12, 0x04, 0x00, 0, 1])));
        assert!(!config.answered_by(&reply(&[0x03, 0x12, 0x04, 0x02, 0, 1])));
        assert!(!config.answered_by(&reply(&[0x03, 0x12, 0x07, 0x00, 0, 1])));
        assert!(config.answered_by(&reply(&[0x03, 0xff, 0xaa])));
        assert!(config.answered_by(&reply(&[0x03])));
    }

    /// A Harpe II Ace reads its settings from the profile, X/Y DPI, colour,
    /// main config, motion sync and lift-off pages.
    #[test]
    fn reads_every_harpe_ii_ace_setting() {
        let f = features("ROG Harpe II Ace", true);
        assert_eq!(
            settings_queries(f),
            [
                Query::Profile,
                Query::XyDpi,
                Query::DpiColors,
                Query::Config,
                Query::MotionSync,
                Query::LiftOff,
                Query::PowerSaving,
            ]
        );

        let mut s = MouseSettings::default();
        // 3 stages in use, the second active.
        let profile = reply_with(Query::Profile, 3, &[(12, &[2]), (19, &[3])]);
        parse_settings(Query::Profile, f, &profile, &mut s);
        // 400, 800, 1600 DPI: (raw + 1) * 50, X then Y.
        let xy = reply_with(
            Query::XyDpi,
            3,
            &[
                (5, &[7, 0, 7, 0]),
                (9, &[15, 0, 15, 0]),
                (13, &[31, 0, 31, 0]),
            ],
        );
        parse_settings(Query::XyDpi, f, &xy, &mut s);
        let colors = reply_with(Query::DpiColors, 3, &[(5, &[0xff, 0, 0, 0, 0xff, 0])]);
        parse_settings(Query::DpiColors, f, &colors, &mut s);
        let config = reply_with(
            Query::Config,
            3,
            &[(13, &[0x03]), (17, &[0x01]), (19, &(-5i16).to_le_bytes())],
        );
        parse_settings(Query::Config, f, &config, &mut s);
        parse_settings(
            Query::MotionSync,
            f,
            &reply_with(Query::MotionSync, 3, &[(5, &[1])]),
            &mut s,
        );
        parse_settings(
            Query::LiftOff,
            f,
            &reply_with(Query::LiftOff, 3, &[(8, &[2])]),
            &mut s,
        );

        let dpis: Vec<u32> = s.stages.iter().map(|st| st.dpi).collect();
        assert_eq!(dpis, [400, 800, 1600]);
        assert_eq!(s.stages[1].color, Some([0, 0xff, 0]));
        assert_eq!(s.active_stage, Some(1));
        assert_eq!(s.polling_rate.map(PollingRate::hz), Some(1000));
        assert_eq!(s.debounce, None, "no debounce setting");
        assert_eq!(s.angle_snapping, Some(true));
        assert_eq!(s.angle_tuning, Some(-5));
        assert_eq!(s.motion_sync, Some(true));
        assert_eq!(s.lift_off, Some(LiftOff::High));
    }

    /// Mice without X/Y DPI keep 2-byte stages in the main page, ahead of the
    /// other settings; two-stage legacy mice have those 4 bytes earlier.
    #[test]
    fn reads_main_page_in_both_layouts() {
        let f = features("ROG Chakram", true);
        assert_eq!(
            settings_queries(f),
            [Query::Profile, Query::Config, Query::LiftOff]
        );
        let mut s = MouseSettings::default();
        let config = reply_with(
            Query::Config,
            0,
            &[
                (5, &[7, 0, 15, 0, 31, 0, 63, 0]),
                (13, &[2]),
                (15, &[3]),
                (17, &[0]),
            ],
        );
        parse_settings(Query::Config, f, &config, &mut s);
        let dpis: Vec<u32> = s.stages.iter().map(|st| st.dpi).collect();
        assert_eq!(dpis, [800, 1600, 3200, 6400], "Chakram steps are 100");
        assert_eq!(s.polling_rate, Some(PollingRate::HZ_500));
        assert_eq!(s.debounce.map(Debounce::ms), Some(16));
        assert_eq!(s.angle_snapping, Some(false));

        let f = features("Gladius II Wireless", true);
        let mut s = MouseSettings::default();
        let config = reply_with(
            Query::Config,
            0,
            &[(5, &[7, 0, 15, 0]), (9, &[1]), (11, &[7]), (13, &[1])],
        );
        parse_settings(Query::Config, f, &config, &mut s);
        assert_eq!(s.stages.len(), 2);
        assert_eq!(s.polling_rate, Some(PollingRate::HZ_250));
        assert_eq!(s.debounce.map(Debounce::ms), Some(32));
        assert_eq!(s.angle_snapping, Some(true));
    }

    #[test]
    fn clamps_debounce_and_ignores_unknown_lift_off() {
        let f = features("ROG Keris Wireless", true);
        let mut s = MouseSettings::default();
        parse_settings(
            Query::Config,
            f,
            &reply_with(Query::Config, 0, &[(15, &[0])]),
            &mut s,
        );
        assert_eq!(s.debounce, Some(Debounce::MIN));
        parse_settings(
            Query::LiftOff,
            f,
            &reply_with(Query::LiftOff, 0, &[(5, &[1])]),
            &mut s,
        );
        assert_eq!(s.lift_off, Some(LiftOff::High), "legacy lift-off is byte 5");
        parse_settings(
            Query::LiftOff,
            f,
            &reply_with(Query::LiftOff, 0, &[(5, &[9])]),
            &mut s,
        );
        assert_eq!(s.lift_off, Some(LiftOff::Unknown(9)));
        assert_eq!(
            change_request(0, f, &Change::LiftOff(LiftOff::Unknown(9))),
            None,
            "an unknown code is never sent"
        );
    }

    #[test]
    fn stage_count_comes_from_the_profile_reply_when_variable() {
        let f = features("ROG Harpe II Ace", true);
        let mut s = MouseSettings::default();
        let profile = reply_with(Query::Profile, 3, &[(12, &[9]), (19, &[7])]);
        parse_settings(Query::Profile, f, &profile, &mut s);
        assert_eq!(s.stages.len(), 4, "7 stages is out of range");
        assert_eq!(s.active_stage, None, "stage 9 doesn't exist");
    }

    #[test]
    fn builds_harpe_ii_ace_changes() {
        let f = features("ROG Harpe II Ace", true);
        let bytes = |c: Change| change_request(0x03, f, &c).unwrap().bytes().to_vec();
        assert_eq!(
            bytes(Change::ActiveStage(1)),
            [0x03, 0x51, 0x31, 0x09, 0x00, 0x02]
        );
        // 1600 DPI = (31 + 1) * 50, with the stage colour sent back.
        assert_eq!(
            bytes(Change::StageDpi {
                stage: 2,
                dpi: 1600,
                color: Some([1, 2, 3])
            }),
            [0x03, 0x51, 0x31, 0x02, 0x00, 31, 0, 1, 2, 3]
        );
        assert_eq!(
            bytes(Change::PollingRate(PollingRate::HZ_8000)),
            [0x03, 0x51, 0x31, 0x04, 0x00, 0x06]
        );
        assert_eq!(
            bytes(Change::AngleSnapping(true)),
            [0x03, 0x51, 0x31, 0x06, 0x00, 1]
        );
        assert_eq!(
            bytes(Change::AngleTuning(-5)),
            [0x03, 0x51, 0x31, 0x0b, 0x00, 0xfb, 0xff]
        );
        assert_eq!(
            bytes(Change::MotionSync(true)),
            [0x03, 0x51, 0x31, 0x12, 0x00, 1]
        );
        assert_eq!(
            bytes(Change::LiftOff(LiftOff::Low)),
            [0x03, 0x51, 0x35, 0xff, 0x00, 0xff, 0x00, 0x01]
        );
        assert_eq!(
            bytes(Change::Energy {
                power_off: PowerOff::NEVER,
                low_battery_warning: 20
            }),
            [0x03, 0x51, 0x37, 0x00, 0x00, 0xff, 0x00, 20]
        );
        assert_eq!(save_request(0x03).bytes(), [0x03, 0x50, 0x03]);
    }

    /// Captured from ASUS GearLink on a Harpe II Ace (SpeedNova): the mouse's
    /// echo of each set command, and its `12 06` reply afterwards.
    #[test]
    fn harpe_ii_lift_off_matches_gearlink() {
        let f = features("ROG Harpe II Ace", true);
        let bytes = |d| {
            change_request(0x03, f, &Change::LiftOff(d))
                .unwrap()
                .bytes()
                .to_vec()
        };
        assert_eq!(
            bytes(LiftOff::High),
            [0x03, 0x51, 0x35, 0xff, 0x00, 0xff, 0x02, 0x01]
        );
        assert_eq!(
            bytes(LiftOff::Low),
            [0x03, 0x51, 0x35, 0xff, 0x00, 0xff, 0x00, 0x01]
        );

        let read = |reply: &[u8]| {
            let mut s = MouseSettings::default();
            parse_settings(Query::LiftOff, f, reply, &mut s);
            s.lift_off
        };
        let high = reply(&[0x03, 0x12, 0x06, 0, 0, 0x00, 0xff, 0xff, 0x02]);
        let low = reply(&[0x03, 0x12, 0x06, 0, 0, 0x00, 0xff, 0xff, 0x00]);
        assert_eq!(read(&high), Some(LiftOff::High));
        assert_eq!(read(&low), Some(LiftOff::Low));
        let g_helper_high = reply(&[0x03, 0x12, 0x06, 0, 0, 0x00, 0xff, 0xff, 0x01]);
        assert_eq!(
            read(&g_helper_high),
            Some(LiftOff::Unknown(1)),
            "not a value this mouse uses"
        );
    }

    /// Captured from ASUS GearLink on a Harpe II Ace (SpeedNova): the echo of
    /// each change (on at 1000, 250 and 2000 Hz, then off), and `12 15` while off.
    #[test]
    fn power_saving_matches_gearlink() {
        let f = features("ROG Harpe II Ace", true);
        let bytes = |on, rate| {
            let change = Change::PowerSaving(PowerSaving { on, rate });
            change_request(0x03, f, &change).unwrap().bytes().to_vec()
        };
        assert_eq!(
            bytes(true, PollingRate::HZ_1000),
            [0x03, 0x51, 0x43, 0, 0, 0x01, 0x03]
        );
        assert_eq!(
            bytes(true, PollingRate::HZ_250),
            [0x03, 0x51, 0x43, 0, 0, 0x01, 0x01]
        );
        assert_eq!(
            bytes(true, PollingRate::HZ_2000),
            [0x03, 0x51, 0x43, 0, 0, 0x01, 0x04]
        );
        assert_eq!(
            bytes(false, PollingRate::HZ_1000),
            [0x03, 0x51, 0x43, 0, 0, 0x00, 0x03]
        );

        let mut s = MouseSettings::default();
        let off = reply(&[0x03, 0x12, 0x15, 0, 0, 0x00, 0x03]);
        parse_settings(Query::PowerSaving, f, &off, &mut s);
        assert_eq!(
            s.power_saving,
            Some(PowerSaving {
                on: false,
                rate: PollingRate::HZ_1000
            })
        );

        let too_fast = Change::PowerSaving(PowerSaving {
            on: true,
            rate: PollingRate::HZ_4000,
        });
        assert_eq!(change_request(0x03, f, &too_fast), None, "up to 2000 Hz");
        let f = features("ROG Chakram X", true);
        let change = Change::PowerSaving(PowerSaving {
            on: true,
            rate: PollingRate::HZ_250,
        });
        assert_eq!(change_request(0, f, &change), None, "not on this mouse");
    }

    /// Other mice keep G-Helper's encoding: High = 1, no trailing byte.
    #[test]
    fn standard_lift_off_is_g_helpers() {
        let f = features("ROG Keris Wireless Aimpoint", true);
        let r = change_request(0, f, &Change::LiftOff(LiftOff::High)).unwrap();
        assert_eq!(r.bytes(), [0, 0x51, 0x35, 0xff, 0x00, 0xff, 0x01]);
        let mut s = MouseSettings::default();
        let high = reply_with(Query::LiftOff, 0, &[(8, &[1])]);
        parse_settings(Query::LiftOff, f, &high, &mut s);
        assert_eq!(s.lift_off, Some(LiftOff::High));
    }

    #[test]
    fn builds_legacy_changes() {
        let f = features("Gladius II Wireless", true);
        let bytes = |c: Change| change_request(0, f, &c).unwrap().bytes().to_vec();
        assert_eq!(
            bytes(Change::PollingRate(PollingRate::HZ_1000)),
            [0, 0x51, 0x31, 0x02, 0, 3]
        );
        assert_eq!(
            bytes(Change::Debounce(Debounce::MIN)),
            [0, 0x51, 0x31, 0x03, 0, 2]
        );
        assert_eq!(
            bytes(Change::AngleSnapping(false)),
            [0, 0x51, 0x31, 0x04, 0, 0]
        );

        let f = features("ROG Keris Wireless", true);
        let bytes = |c: Change| change_request(0, f, &c).unwrap().bytes().to_vec();
        assert_eq!(
            bytes(Change::LiftOff(LiftOff::High)),
            [0, 0x51, 0x35, 0, 0, 1]
        );
        // Warning levels are sent as quarters.
        assert_eq!(
            bytes(Change::Energy {
                power_off: PowerOff::from_code(2),
                low_battery_warning: 50
            }),
            [0, 0x51, 0x37, 0, 0, 2, 0, 2]
        );
    }

    #[test]
    fn clamps_values_and_refuses_unsupported_changes() {
        let f = features("ROG Harpe II Ace", true);
        let raw = |dpi| {
            let r = change_request(
                3,
                f,
                &Change::StageDpi {
                    stage: 0,
                    dpi,
                    color: Some([0; 3]),
                },
            );
            let b = r.unwrap().bytes().to_vec();
            u16::from_le_bytes([b[5], b[6]])
        };
        assert_eq!(raw(1), 1, "clamped to 100");
        assert_eq!(raw(1_000_000), 839, "clamped to 42000");
        assert_eq!(raw(1624), 31, "rounded to 1600");
        assert_eq!(raw(1625), 32, "rounded to 1650");

        let none = |c: Change| change_request(3, f, &c);
        assert_eq!(none(Change::Debounce(Debounce::MIN)), None, "no debounce");
        assert_eq!(none(Change::ActiveStage(4)), None, "4 stages");
        assert_eq!(
            none(Change::StageDpi {
                stage: 0,
                dpi: 800,
                color: None
            }),
            None,
            "the colour is needed"
        );

        let f = features("ROG Chakram X", true);
        assert_eq!(
            change_request(0, f, &Change::PollingRate(PollingRate::HZ_125)),
            None,
            "125 Hz isn't offered"
        );
        let f = features("ROG Chakram", true);
        assert_eq!(
            change_request(0, f, &Change::ActiveStage(0)),
            None,
            "not switchable"
        );
    }

    /// The Extreme counts DPI in steps of 1 up to 65000, which still fits.
    #[test]
    fn extreme_dpi_is_exact() {
        let f = features("Harpe II Extreme Edition 20", true);
        let r = change_request(
            3,
            f,
            &Change::StageDpi {
                stage: 3,
                dpi: 65_000,
                color: Some([0; 3]),
            },
        );
        let b = r.unwrap().bytes().to_vec();
        assert_eq!(u16::from_le_bytes([b[5], b[6]]), 64_999);
        assert_eq!(snap_dpi(&f.dpi, 1234), 1234);
    }

    #[test]
    fn power_off_codes() {
        let names: Vec<String> = PowerOff::CHOICES.iter().map(ToString::to_string).collect();
        assert_eq!(
            names,
            ["1 min", "2 min", "3 min", "5 min", "10 min", "Never"]
        );
        assert_eq!(PowerOff::from_code(9).to_string(), "Unknown (0x09)");
    }

    #[test]
    fn parses_omni_pairing_reply() {
        let mut r = [0u8; 64];
        r[..2].copy_from_slice(&OMNI_PAIRED);
        r[5..7].copy_from_slice(&0x1b65u16.to_le_bytes());
        r[9..11].copy_from_slice(&0x1b1au16.to_le_bytes());
        assert_eq!(parse_omni_paired(&r), vec![0x1b65, 0x1b1a]);
    }
}
