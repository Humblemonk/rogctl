//! Waybar's custom-module JSON (`"return-type": "json"`): Waybar reads fixed
//! field names, so it can't take `Status` as is.

use serde::Serialize;

use super::{State, Status};

#[derive(Debug, PartialEq, Serialize)]
pub struct Line {
    /// The level, e.g. "80%". Empty while nothing is plugged in, which
    /// makes Waybar hide the module.
    pub text: String,
    /// Picks the icon when `format-icons` is keyed by name.
    pub alt: &'static str,
    pub tooltip: String,
    /// CSS classes on the module: the state, plus `charging` and `low`.
    pub class: Vec<&'static str>,
    /// Picks the icon when `format-icons` is a list.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub percentage: Option<u8>,
}

/// `nothing_found` is the tooltip while nothing is plugged in.
pub fn line(s: &Status, nothing_found: &str) -> Line {
    let state = state_name(s.state);
    let mut class = vec![state];
    if s.charging {
        class.push("charging");
    }
    if s.low {
        class.push("low");
    }

    let text = match (s.state, s.battery) {
        (State::Disconnected, _) => String::new(),
        (_, Some(pct)) => format!("{pct}%"),
        (_, None) => "?".to_owned(),
    };

    let alt = if s.state == State::Connected && s.charging {
        "charging"
    } else {
        state
    };

    Line {
        text,
        alt,
        tooltip: tooltip(s, nothing_found),
        class,
        percentage: s.battery,
    }
}

fn state_name(state: State) -> &'static str {
    match state {
        State::Connected => "connected",
        State::Asleep => "asleep",
        State::Disconnected => "disconnected",
        State::Error => "error",
    }
}

/// The widgets' wording: "Battery: 79% (discharging)", with "(offline)"
/// while the device sleeps.
fn tooltip(s: &Status, nothing_found: &str) -> String {
    let device = escape(s.device.unwrap_or("Device"));
    let detail = match s.state {
        State::Disconnected => return nothing_found.to_owned(),
        State::Error => format!("Error: {}", escape(s.error.as_deref().unwrap_or("unknown"))),
        State::Asleep => match s.battery {
            Some(pct) => format!("Battery: {pct}% (offline)"),
            None => "Battery: offline".to_owned(),
        },
        State::Connected => format!(
            "Battery: {}% ({})",
            s.battery.unwrap_or(0),
            super::charge_word(s.charging, s.battery)
        ),
    };
    format!("{device}\n{detail}")
}

/// Waybar renders tooltips as Pango markup.
fn escape(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn status(state: State, battery: Option<u8>, charging: bool) -> Status {
        let mut s = Status {
            device: Some("ROG Harpe II Ace"),
            battery,
            charging,
            ..Status::empty(state)
        };
        s.low = crate::is_low(&s, 20);
        s
    }

    fn line_of(s: &Status) -> Line {
        line(s, "No supported mouse found")
    }

    #[test]
    fn connected() {
        let l = line_of(&status(State::Connected, Some(80), false));
        assert_eq!(l.text, "80%");
        assert_eq!(l.alt, "connected");
        assert_eq!(l.class, ["connected"]);
        assert_eq!(l.percentage, Some(80));
        assert_eq!(l.tooltip, "ROG Harpe II Ace\nBattery: 80% (discharging)");
    }

    #[test]
    fn low_until_charging() {
        let l = line_of(&status(State::Connected, Some(20), false));
        assert_eq!(l.class, ["connected", "low"]);

        let l = line_of(&status(State::Connected, Some(20), true));
        assert_eq!(l.alt, "charging");
        assert_eq!(l.class, ["connected", "charging"]);
        assert_eq!(l.tooltip, "ROG Harpe II Ace\nBattery: 20% (charging)");
    }

    #[test]
    fn asleep_keeps_last_level() {
        let l = line_of(&status(State::Asleep, Some(55), false));
        assert_eq!(l.text, "55%");
        assert_eq!(l.class, ["asleep"]);
        assert_eq!(l.tooltip, "ROG Harpe II Ace\nBattery: 55% (offline)");

        let l = line_of(&status(State::Asleep, None, false));
        assert_eq!(l.text, "?");
        assert_eq!(l.percentage, None);
    }

    #[test]
    fn disconnected_hides_the_module() {
        let l = line(
            &Status::empty(State::Disconnected),
            "No supported keyboard found",
        );
        assert_eq!(l.text, "");
        assert_eq!(l.tooltip, "No supported keyboard found");
    }

    #[test]
    fn error_is_escaped() {
        let s = Status {
            error: Some("<bad> & worse".to_owned()),
            ..status(State::Error, None, false)
        };
        let l = line_of(&s);
        assert_eq!(l.text, "?");
        assert_eq!(
            l.tooltip,
            "ROG Harpe II Ace\nError: &lt;bad&gt; &amp; worse"
        );
    }

    /// Waybar needs `text`; `percentage` is left out rather than null.
    #[test]
    fn json_shape() {
        let json = serde_json::to_value(line_of(&Status::empty(State::Disconnected))).unwrap();
        assert_eq!(json["text"], "");
        assert_eq!(json["class"], serde_json::json!(["disconnected"]));
        assert!(json.get("percentage").is_none());
    }
}
