//! Waybar's custom-module JSON (`"return-type": "json"`): Waybar reads fixed
//! field names, so it can't take `Status` as is.

use serde::Serialize;

use super::{State, Status};

#[derive(Debug, PartialEq, Serialize)]
pub struct Line {
    /// The level, e.g. "80%". Empty while no mouse is plugged in, which
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

pub fn line(s: &Status, low_threshold: u8) -> Line {
    let state = state_name(s.state);
    let low = low_threshold > 0 && !s.charging && s.battery.is_some_and(|b| b <= low_threshold);

    let mut class = vec![state];
    if s.charging {
        class.push("charging");
    }
    if low {
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
        tooltip: tooltip(s),
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

/// Solaar's wording, as in the Noctalia widget: "Battery: 79% (discharging)",
/// with "(offline)" while the mouse sleeps.
fn tooltip(s: &Status) -> String {
    let device = escape(s.device.unwrap_or("Mouse"));
    let detail = match s.state {
        State::Disconnected => return "No supported mouse found".to_owned(),
        State::Error => format!("Error: {}", escape(s.error.as_deref().unwrap_or("unknown"))),
        State::Asleep => match s.battery {
            Some(pct) => format!("Battery: {pct}% (offline)"),
            None => "Battery: offline".to_owned(),
        },
        State::Connected => {
            let word = match (s.charging, s.battery) {
                (true, Some(100)) => "full",
                (true, _) => "recharging",
                (false, _) => "discharging",
            };
            format!("Battery: {}% ({word})", s.battery.unwrap_or(0))
        }
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
        Status {
            device: Some("ROG Harpe II Ace"),
            battery,
            charging,
            ..Status::empty(state)
        }
    }

    #[test]
    fn connected() {
        let l = line(&status(State::Connected, Some(80), false), 20);
        assert_eq!(l.text, "80%");
        assert_eq!(l.alt, "connected");
        assert_eq!(l.class, ["connected"]);
        assert_eq!(l.percentage, Some(80));
        assert_eq!(l.tooltip, "ROG Harpe II Ace\nBattery: 80% (discharging)");
    }

    #[test]
    fn low_until_charging() {
        let l = line(&status(State::Connected, Some(20), false), 20);
        assert_eq!(l.class, ["connected", "low"]);

        let l = line(&status(State::Connected, Some(20), true), 20);
        assert_eq!(l.alt, "charging");
        assert_eq!(l.class, ["connected", "charging"]);
        assert_eq!(l.tooltip, "ROG Harpe II Ace\nBattery: 20% (recharging)");

        let l = line(&status(State::Connected, Some(20), false), 0);
        assert_eq!(l.class, ["connected"], "0 turns the warning off");
    }

    #[test]
    fn asleep_keeps_last_level() {
        let l = line(&status(State::Asleep, Some(55), false), 20);
        assert_eq!(l.text, "55%");
        assert_eq!(l.class, ["asleep"]);
        assert_eq!(l.tooltip, "ROG Harpe II Ace\nBattery: 55% (offline)");

        let l = line(&status(State::Asleep, None, false), 20);
        assert_eq!(l.text, "?");
        assert_eq!(l.percentage, None);
    }

    #[test]
    fn disconnected_hides_the_module() {
        let l = line(&Status::empty(State::Disconnected), 20);
        assert_eq!(l.text, "");
        assert_eq!(l.tooltip, "No supported mouse found");
    }

    #[test]
    fn error_is_escaped() {
        let s = Status {
            error: Some("<bad> & worse".to_owned()),
            ..status(State::Error, None, false)
        };
        let l = line(&s, 20);
        assert_eq!(l.text, "?");
        assert_eq!(
            l.tooltip,
            "ROG Harpe II Ace\nError: &lt;bad&gt; &amp; worse"
        );
    }

    /// Waybar needs `text`; `percentage` is left out rather than null.
    #[test]
    fn json_shape() {
        let json = serde_json::to_value(line(&Status::empty(State::Disconnected), 20)).unwrap();
        assert_eq!(json["text"], "");
        assert_eq!(json["class"], serde_json::json!(["disconnected"]));
        assert!(json.get("percentage").is_none());
    }
}
