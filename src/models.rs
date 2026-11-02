//! Supported mice, transcribed from G-Helper's `app/Peripherals/Mouse/Models/`
//! and `PeripheralsProvider.cs` (https://github.com/seerge/g-helper, 912cde3;
//! settings from 54c5bd0), and keyboards, battery only, from its
//! `app/Peripherals/Keyboard/` (the 2026-10-24 tree). Models whose
//! `HasBattery()` is false there are left out.
//!
//! Each entry is one way a mouse or keyboard shows up on USB: its own receiver, a cable,
//! or a shared receiver (SpeedNova, Omni). Names drop G-Helper's
//! "(Wireless)"/"(Wired)"/"(OMNI)" suffix; `wireless` carries that instead.
//! The `Features` constants are named after the G-Helper class they come from.

use crate::device::{Kind, Match, Model};
use crate::protocol::{
    AngleTuning, Battery, Dpi, Features, Layout, LiftOffFormat, LightingZone, PollingRate,
    WarningRange,
};

/// ROG Omni receiver; the paired mouse and keyboard are found by asking it.
pub const OMNI_PID: u16 = 0x1ace;

/// An Omni receiver whose paired devices couldn't be asked for, usually for
/// lack of permission. Every Omni entry shares these query parameters, so the
/// battery read still works, or fails with the real reason. Not in `MODELS`.
pub static OMNI_RECEIVER: Model = omni("ROG Omni receiver", &[], BATTERY_ONLY);

// G-Helper's `AsusMouse` defaults, which each class overrides.
const DPI: Dpi = Dpi {
    stages: 4,
    min: 100,
    max: 2000,
    step: 50,
    xy: false,
    colors: false,
    variable_stages: false,
    switchable: true,
    active_byte: 12,
};
const BASE: Features = Features {
    dpi: DPI,
    layout: Layout::Standard,
    polling_rates: &[],
    debounce: false,
    angle_snapping: false,
    angle_tuning: None,
    motion_sync: false,
    lift_off: None,
    power_off: None,
    low_battery_warning: None,
    power_saving_rates: &[],
    lighting: None,
};
const TUNING: AngleTuning = AngleTuning {
    min: -20,
    max: 20,
    step: 1,
};
const TUNING_BY_5: AngleTuning = AngleTuning { step: 5, ..TUNING };
const POWER_OFF: Option<usize> = Some(6);
const WARNING: Option<WarningRange> = Some(WarningRange {
    step: 10,
    max: 50,
    quarters: false,
});
/// Mice that store the warning in quarters. G-Helper keeps its 10% step for
/// some of these, which truncates when divided; 25% sends the same values.
const fn warning_quarters(max: u8) -> Option<WarningRange> {
    Some(WarningRange {
        step: 25,
        max,
        quarters: true,
    })
}

const UP_TO_1000: &[PollingRate] = &[
    PollingRate::HZ_125,
    PollingRate::HZ_250,
    PollingRate::HZ_500,
    PollingRate::HZ_1000,
];
const UP_TO_8000: &[PollingRate] = &[
    PollingRate::HZ_125,
    PollingRate::HZ_250,
    PollingRate::HZ_500,
    PollingRate::HZ_1000,
    PollingRate::HZ_2000,
    PollingRate::HZ_4000,
    PollingRate::HZ_8000,
];
const FROM_250: &[PollingRate] = &[
    PollingRate::HZ_250,
    PollingRate::HZ_500,
    PollingRate::HZ_1000,
];

/// X/Y DPI with a colour per stage, as on most current mice.
const XY_COLORS: Dpi = Dpi {
    xy: true,
    colors: true,
    ..DPI
};

const HARPE_II_ACE: Features = Features {
    dpi: Dpi {
        max: 42_000,
        variable_stages: true,
        ..XY_COLORS
    },
    polling_rates: UP_TO_8000,
    angle_snapping: true,
    angle_tuning: Some(TUNING),
    motion_sync: true,
    lift_off: Some(LiftOffFormat::HarpeII),
    power_saving_rates: &[
        PollingRate::HZ_125,
        PollingRate::HZ_250,
        PollingRate::HZ_500,
        PollingRate::HZ_1000,
        PollingRate::HZ_2000,
    ],
    power_off: POWER_OFF,
    low_battery_warning: WARNING,
    // Zone 0, as ASUS GearLink sets it on this mouse.
    lighting: Some(LightingZone::First),
    ..BASE
};
const HARPE_II_EXTREME_EDITION_20: Features = Features {
    dpi: Dpi {
        max: 65_000,
        step: 1,
        ..HARPE_II_ACE.dpi
    },
    ..HARPE_II_ACE
};
const HARPE_ACE_AIM_LAB: Features = Features {
    dpi: Dpi {
        min: 50,
        max: 36_000,
        ..XY_COLORS
    },
    polling_rates: UP_TO_1000,
    debounce: true,
    angle_snapping: true,
    angle_tuning: Some(AngleTuning {
        min: -30,
        max: 30,
        step: 1,
    }),
    lift_off: Some(LiftOffFormat::Standard),
    power_off: POWER_OFF,
    low_battery_warning: WARNING,
    lighting: Some(LightingZone::All),
    ..BASE
};
const HARPE_ACE_EXTREME: Features = Features {
    dpi: Dpi {
        max: 42_000,
        ..HARPE_ACE_AIM_LAB.dpi
    },
    ..HARPE_ACE_AIM_LAB
};
const KERIS_II_ACE: Features = Features {
    dpi: Dpi {
        max: 42_000,
        ..XY_COLORS
    },
    polling_rates: UP_TO_1000,
    angle_snapping: true,
    angle_tuning: Some(TUNING_BY_5),
    lift_off: Some(LiftOffFormat::Standard),
    power_off: POWER_OFF,
    low_battery_warning: WARNING,
    lighting: Some(LightingZone::All),
    ..BASE
};
const KERIS_II_ORIGIN: Features = Features {
    dpi: Dpi {
        variable_stages: true,
        ..KERIS_II_ACE.dpi
    },
    ..KERIS_II_ACE
};
/// `HarpeAceMini` is the Keris II Origin under another name.
const HARPE_ACE_MINI: Features = KERIS_II_ORIGIN;
const STRIX_IMPACT_III_WIRELESS: Features = Features {
    dpi: Dpi {
        max: 36_000,
        ..XY_COLORS
    },
    ..KERIS_II_ACE
};
/// `GladiusIIIAimpoint` and `KerisWirelssAimpoint`.
const AIMPOINT: Features = Features {
    dpi: Dpi {
        max: 36_000,
        ..XY_COLORS
    },
    polling_rates: UP_TO_1000,
    debounce: true,
    angle_snapping: true,
    angle_tuning: Some(TUNING),
    lift_off: Some(LiftOffFormat::Standard),
    power_off: POWER_OFF,
    low_battery_warning: WARNING,
    lighting: Some(LightingZone::All),
    ..BASE
};
const CHAKRAM_X: Features = Features {
    polling_rates: FROM_250,
    ..AIMPOINT
};
const CHAKRAM_X_WIRED: Features = Features {
    polling_rates: &[
        PollingRate::HZ_250,
        PollingRate::HZ_500,
        PollingRate::HZ_1000,
        PollingRate::HZ_2000,
        PollingRate::HZ_4000,
        PollingRate::HZ_8000,
    ],
    ..CHAKRAM_X
};
const SPATHA_X: Features = Features {
    dpi: Dpi {
        max: 19_000,
        colors: true,
        ..DPI
    },
    polling_rates: FROM_250,
    angle_snapping: true,
    lift_off: Some(LiftOffFormat::Standard),
    power_off: POWER_OFF,
    low_battery_warning: WARNING,
    lighting: Some(LightingZone::All),
    ..BASE
};
const GLADIUS_III_WIRELESS: Features = Features {
    dpi: Dpi { max: 26_000, ..DPI },
    polling_rates: UP_TO_1000,
    debounce: true,
    angle_snapping: true,
    lift_off: Some(LiftOffFormat::Standard),
    power_off: POWER_OFF,
    low_battery_warning: WARNING,
    lighting: Some(LightingZone::All),
    ..BASE
};
const TUF_M4_WIRELESS: Features = Features {
    dpi: Dpi {
        max: 12_000,
        step: 100,
        ..DPI
    },
    polling_rates: UP_TO_1000,
    debounce: true,
    angle_snapping: true,
    power_off: POWER_OFF,
    low_battery_warning: WARNING,
    ..BASE
};
const TX_GAMING_MINI: Features = Features {
    dpi: Dpi {
        step: 50,
        xy: true,
        ..TUF_M4_WIRELESS.dpi
    },
    ..TUF_M4_WIRELESS
};
/// The only one of G-Helper's `TUFM4Wirelss` family with lighting.
const TUF_GAMING_MINI_MIKU: Features = Features {
    lighting: Some(LightingZone::All),
    ..TX_GAMING_MINI
};
const MD200: Features = Features {
    dpi: Dpi {
        stages: 2,
        max: 4_200,
        ..DPI
    },
    polling_rates: &[PollingRate::HZ_125, PollingRate::HZ_250],
    debounce: true,
    ..BASE
};
/// Older mice whose active DPI stage only the mouse's own button changes.
const FIXED_STAGE: Dpi = Dpi {
    max: 16_000,
    step: 100,
    switchable: false,
    ..DPI
};
/// `Chakram` and `PugioII`.
const CHAKRAM: Features = Features {
    dpi: FIXED_STAGE,
    polling_rates: UP_TO_1000,
    debounce: true,
    angle_snapping: true,
    lift_off: Some(LiftOffFormat::Standard),
    power_off: POWER_OFF,
    low_battery_warning: warning_quarters(100),
    ..BASE
};
/// `KerisWireless` and `StrixImpactIIWireless`.
const KERIS_WIRELESS: Features = Features {
    lift_off: Some(LiftOffFormat::Legacy),
    low_battery_warning: warning_quarters(50),
    ..CHAKRAM
};
const GLADIUS_II_WIRELESS: Features = Features {
    dpi: Dpi {
        stages: 2,
        max: 16_000,
        step: 100,
        ..DPI
    },
    layout: Layout::Legacy,
    polling_rates: UP_TO_1000,
    debounce: true,
    angle_snapping: true,
    angle_tuning: Some(TUNING),
    power_off: POWER_OFF,
    low_battery_warning: Some(WarningRange {
        step: 25,
        max: 50,
        quarters: false,
    }),
    ..BASE
};
const STRIX_CARRY: Features = Features {
    dpi: Dpi {
        stages: 2,
        min: 50,
        max: 7_200,
        switchable: false,
        active_byte: 11,
        ..DPI
    },
    layout: Layout::Legacy,
    polling_rates: UP_TO_1000,
    debounce: true,
    angle_snapping: true,
    // G-Helper: "Potentially does nothing", but the mouse stores it.
    lift_off: Some(LiftOffFormat::Standard),
    power_off: Some(5),
    ..BASE
};
/// No settings, so `rogctl settings` offers nothing to send: for an Omni
/// receiver whose mouse is unknown, and for keyboards.
const BATTERY_ONLY: Features = Features {
    dpi: Dpi { stages: 0, ..DPI },
    ..BASE
};

/// Mice on their own receiver or cable. G-Helper's defaults: report ID 0,
/// 65-byte packets, battery percentage in byte 5.
const fn usb(
    name: &'static str,
    pid: u16,
    interface: u8,
    wireless: bool,
    features: Features,
) -> Model {
    Model {
        name,
        kind: Kind::Mouse,
        pid,
        interface,
        report_id: 0x00,
        packet_size: 65,
        wireless,
        battery: Battery::Percent,
        features,
        matches: Match::Any,
    }
}

/// Older mice that report the battery as a 0-4 level.
const fn usb_quarters(
    name: &'static str,
    pid: u16,
    interface: u8,
    wireless: bool,
    features: Features,
) -> Model {
    Model {
        battery: Battery::Quarters { byte: 5 },
        ..usb(name, pid, interface, wireless, features)
    }
}

/// Newer mice that use 64-byte packets.
const fn usb64(
    name: &'static str,
    pid: u16,
    interface: u8,
    wireless: bool,
    features: Features,
) -> Model {
    Model {
        packet_size: 64,
        ..usb(name, pid, interface, wireless, features)
    }
}

/// Mice on the Omni receiver, identified by the paired device's product IDs.
const fn omni(name: &'static str, paired: &'static [u16], features: Features) -> Model {
    Model {
        name,
        kind: Kind::Mouse,
        pid: OMNI_PID,
        interface: 2,
        report_id: 0x03,
        packet_size: 64,
        wireless: true,
        battery: Battery::Percent,
        features,
        matches: Match::OmniPaired(paired),
    }
}

pub const MODELS: &[Model] = &[
    // SpeedNova 8K receiver, shared by the Harpe II models; G-Helper tells
    // them apart by the receiver's USB product name. The Extreme inherits the
    // Ace's battery format there.
    Model {
        report_id: 0x03,
        matches: Match::Product("EXTREME"),
        ..usb64(
            "Harpe II Extreme Edition 20",
            0x1ad0,
            2,
            true,
            HARPE_II_EXTREME_EDITION_20,
        )
    },
    Model {
        report_id: 0x03,
        ..usb64("ROG Harpe II Ace", 0x1ad0, 2, true, HARPE_II_ACE)
    },
    usb64("ROG Harpe II Ace", 0x1c69, 0, false, HARPE_II_ACE),
    // Omni receiver
    omni("Harpe Ace Mini", &[0x1b65], HARPE_ACE_MINI),
    omni(
        "ROG Harpe Ace Aim Lab Edition",
        &[0x1a94],
        HARPE_ACE_AIM_LAB,
    ),
    omni(
        "ROG Harpe Ace Extreme",
        &[0x1b68, 0x1b69],
        HARPE_ACE_EXTREME,
    ),
    omni("ROG Keris II Ace", &[0x1b1a, 0x1b18], KERIS_II_ACE),
    omni("ROG Keris II Origin", &[0x1c0e], KERIS_II_ORIGIN),
    omni("ROG Keris II Origin KJP", &[0x1d4e], KERIS_II_ORIGIN),
    omni("ROG Keris Wireless Aimpoint", &[0x1a68, 0x1a6a], AIMPOINT),
    omni("ROG Gladius III Aimpoint", &[0x1a72], AIMPOINT),
    omni(
        "Strix Impact III Wireless",
        &[0x1ad7],
        STRIX_IMPACT_III_WIRELESS,
    ),
    // Own receiver or cable
    usb("ROG Chakram X", 0x1a1a, 0, true, CHAKRAM_X),
    usb("ROG Chakram X", 0x1a18, 0, false, CHAKRAM_X_WIRED),
    usb_quarters("ROG Chakram", 0x18e5, 0, true, CHAKRAM),
    usb_quarters("ROG Chakram", 0x18e3, 0, false, CHAKRAM),
    usb("ROG Gladius III Aimpoint", 0x1a72, 0, true, AIMPOINT),
    usb("ROG Gladius III Aimpoint", 0x1a70, 0, false, AIMPOINT),
    usb("ROG Gladius III Eva 2", 0x1b0c, 0, true, AIMPOINT),
    usb("ROG Gladius III Eva 2", 0x1b0a, 0, false, AIMPOINT),
    usb(
        "ROG Gladius III Wireless",
        0x197f,
        0,
        true,
        GLADIUS_III_WIRELESS,
    ),
    usb(
        "ROG Gladius III Wireless",
        0x197d,
        0,
        false,
        GLADIUS_III_WIRELESS,
    ),
    usb_quarters("Gladius II Wireless", 0x18a0, 2, true, GLADIUS_II_WIRELESS),
    usb(
        "ROG Harpe Ace Aim Lab Edition",
        0x1a94,
        0,
        true,
        HARPE_ACE_AIM_LAB,
    ),
    usb(
        "ROG Harpe Ace Aim Lab Edition",
        0x1a92,
        0,
        false,
        HARPE_ACE_AIM_LAB,
    ),
    usb("ROG Harpe Ace Extreme", 0x1b67, 0, false, HARPE_ACE_EXTREME),
    usb64("Harpe Ace Mini", 0x1b63, 0, false, HARPE_ACE_MINI),
    usb("ROG Keris II Ace", 0x1b16, 0, false, KERIS_II_ACE),
    usb64("ROG Keris II Origin", 0x1c0c, 0, false, KERIS_II_ORIGIN),
    usb64("ROG Keris II Origin KJP", 0x1d4c, 0, false, KERIS_II_ORIGIN),
    usb_quarters("ROG Keris Wireless", 0x1960, 0, true, KERIS_WIRELESS),
    usb_quarters("ROG Keris Wireless", 0x195e, 0, false, KERIS_WIRELESS),
    usb_quarters("ROG Keris EVA Edition", 0x1a59, 0, true, KERIS_WIRELESS),
    usb_quarters("ROG Keris EVA Edition", 0x1a57, 0, false, KERIS_WIRELESS),
    usb("ROG Keris Wireless Aimpoint", 0x1a68, 0, true, AIMPOINT),
    usb("ROG Keris Wireless Aimpoint", 0x1a66, 0, false, AIMPOINT),
    usb("ASUS Mouse MD200", 0x1a24, 2, true, MD200),
    usb_quarters("ROG Pugio II", 0x1908, 0, true, CHAKRAM),
    usb_quarters("ROG Pugio II", 0x1906, 0, false, CHAKRAM),
    usb("ROG Spatha X", 0x1979, 0, true, SPATHA_X),
    usb("ROG Spatha X", 0x1977, 0, false, SPATHA_X),
    Model {
        battery: Battery::Quarters { byte: 7 },
        ..usb_quarters("ROG Strix Carry", 0x18b4, 1, true, STRIX_CARRY)
    },
    usb_quarters(
        "ROG Strix Impact II Wireless",
        0x1949,
        0,
        true,
        KERIS_WIRELESS,
    ),
    usb_quarters(
        "ROG Strix Impact II Wireless",
        0x1947,
        0,
        false,
        KERIS_WIRELESS,
    ),
    usb("TUF GAMING M4 Wireless", 0x19f4, 0, true, TUF_M4_WIRELESS),
    usb("TX GAMING MOUSE", 0x1a8d, 0, true, TUF_M4_WIRELESS),
    usb("TX GAMING MOUSE Mini", 0x1af5, 0, true, TX_GAMING_MINI),
    usb("TX GAMING MOUSE Mini", 0x1af3, 0, false, TX_GAMING_MINI),
    usb(
        "TUF GAMING Mini Miku Edition",
        0x1c57,
        0,
        true,
        TUF_GAMING_MINI_MIKU,
    ),
    usb(
        "TUF GAMING Mini Miku Edition",
        0x1c56,
        0,
        false,
        TUF_GAMING_MINI_MIKU,
    ),
];

/// A keyboard on its own receiver or cable: G-Helper's `AsusKeyboard`
/// defaults, interface 1 (`mi_01`), report ID 0, 65-byte packets. `byte` holds
/// the battery percentage.
const fn keyboard(name: &'static str, pid: u16, wireless: bool, byte: usize) -> Model {
    Model {
        name,
        kind: Kind::Keyboard,
        pid,
        interface: 1,
        report_id: 0x00,
        packet_size: 65,
        wireless,
        battery: Battery::Keyboard { byte },
        features: BATTERY_ONLY,
        matches: Match::Any,
    }
}

/// A keyboard on the Omni receiver: the same node as a mouse would use
/// (`mi_02`), on its own report ID (`col02`).
const fn omni_keyboard(name: &'static str, paired: &'static [u16], byte: usize) -> Model {
    Model {
        pid: OMNI_PID,
        interface: 2,
        report_id: 0x02,
        packet_size: 64,
        matches: Match::OmniPaired(paired),
        ..keyboard(name, OMNI_PID, true, byte)
    }
}

/// Where G-Helper's `ParseBattery` finds the percentage: byte 6 by default,
/// 11 on the Falchion family (whose byte 6 G-Helper calls a 0-10 gauge).
const PERCENT: usize = 6;
const FALCHION_PERCENT: usize = 11;

/// Battery-powered keyboards, after every mouse so `rogctl battery` shows a
/// mouse first. Names drop G-Helper's "(Wireless)"/"(OMNI)"; its classes
/// without that suffix are taken as the cable. Left out until their interface
/// is known (G-Helper finds it from the report descriptor): the Claymore II
/// (`196b`) and Strix Scope RX TKL Wireless (`1a07`).
pub const KEYBOARDS: &[Model] = &[
    keyboard("ROG Azoth", 0x1a83, false, PERCENT),
    keyboard("ROG Azoth", 0x1a85, true, PERCENT),
    omni_keyboard("ROG Azoth", &[0x1a85], PERCENT),
    keyboard("ROG Azoth Extreme", 0x1b3f, false, PERCENT),
    omni_keyboard("ROG Azoth Extreme", &[0x1b42], PERCENT),
    keyboard("ROG Azoth Extreme SE", 0x1cef, false, PERCENT),
    omni_keyboard("ROG Azoth Extreme SE", &[0x1cf1], PERCENT),
    keyboard("ROG Azoth X", 0x1c24, false, PERCENT),
    keyboard("ROG Azoth X", 0x1c25, true, PERCENT),
    keyboard("ROG Strix Scope II 96", 0x1aae, false, PERCENT),
    omni_keyboard("ROG Strix Scope II 96", &[0x1ab0], PERCENT),
    keyboard("ROG Strix Scope II 96 RX", 0x1b78, false, PERCENT),
    omni_keyboard("ROG Strix Scope II 96 RX", &[0x1b7a], PERCENT),
    keyboard("ROG Falchion", 0x193c, false, FALCHION_PERCENT),
    keyboard("ROG Falchion", 0x193e, true, FALCHION_PERCENT),
    // G-Helper's "Falchion RX" (`1b04`) and "Falchion RX LP (OMNI)": the cable
    // names itself "ROG FALCHION RX LOW PROFILE" (captured).
    keyboard(
        "ROG Falchion RX Low Profile",
        0x1b04,
        false,
        FALCHION_PERCENT,
    ),
    omni_keyboard("ROG Falchion RX Low Profile", &[0x1b06], FALCHION_PERCENT),
];

/// Every supported model: the mice, then the keyboards.
pub fn all() -> impl Iterator<Item = &'static Model> {
    MODELS.iter().chain(KEYBOARDS)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every product ID in the table needs a udev rule, or the device is
    /// found but can't be opened without root. Each rule opens only the
    /// interface rogctl uses: the others carry keystrokes and pointer input.
    #[test]
    fn udev_rules_match_every_model_interface() {
        use std::collections::BTreeSet;
        let rules: BTreeSet<&str> = include_str!("../udev/70-rogctl.rules")
            .lines()
            .filter(|l| !l.is_empty() && !l.starts_with('#'))
            .collect();
        let wanted: BTreeSet<String> = all()
            .map(|m| {
                format!(
                    "KERNEL==\"hidraw*\", ATTRS{{modalias}}==\"usb:v0B05p{:04X}*in{:02X}\", TAG+=\"uaccess\"",
                    m.pid, m.interface
                )
            })
            .collect();
        let wanted: BTreeSet<&str> = wanted.iter().map(String::as_str).collect();
        assert_eq!(rules, wanted);
    }

    /// The README's Supported mice and Supported keyboards lists name every
    /// model.
    #[test]
    fn readme_lists_every_model() {
        let readme = include_str!("../README.md");
        for (heading, models) in [
            ("## Supported mice", MODELS),
            ("## Supported keyboards", KEYBOARDS),
        ] {
            let start = readme.find(heading).expect("section missing") + heading.len();
            let section = &readme[start..];
            let section = &section[..section.find("\n## ").unwrap_or(section.len())];
            for m in models {
                let item = format!("- {}", m.name);
                assert!(
                    section.lines().any(|l| l == item),
                    "README's {heading} doesn't list {}",
                    m.name
                );
            }
        }
    }

    /// Keyboards are only read: no settings, the keyboard power read, and
    /// G-Helper's interfaces.
    #[test]
    fn keyboards_are_battery_only() {
        assert!(MODELS.iter().all(|m| m.kind == Kind::Mouse));
        for m in KEYBOARDS {
            assert_eq!(m.kind, Kind::Keyboard, "{}", m.name);
            assert_eq!(m.features, BATTERY_ONLY, "{}", m.name);
            assert!(crate::protocol::settings_queries(&m.features).is_empty());
            assert!(matches!(m.battery, Battery::Keyboard { .. }), "{}", m.name);
            let omni = (OMNI_PID, 2, 0x02, 64, true);
            let usb = (m.pid, 1, 0x00, 65);
            if m.pid == OMNI_PID {
                assert_eq!(
                    (m.pid, m.interface, m.report_id, m.packet_size, m.wireless),
                    omni
                );
            } else {
                assert_eq!((m.pid, m.interface, m.report_id, m.packet_size), usb);
            }
        }
    }

    /// Two entries for the same USB interface must be told apart by `matches`.
    #[test]
    fn entries_are_unambiguous() {
        let every: Vec<&Model> = all().collect();
        for (i, a) in every.iter().enumerate() {
            for b in &every[i + 1..] {
                if a.pid == b.pid && a.interface == b.interface {
                    assert!(
                        !matches!(a.matches, Match::Any),
                        "{} shadows {} ({:04x})",
                        a.name,
                        b.name,
                        a.pid
                    );
                    // `device::rank` tells entries apart by these three.
                    assert_ne!(a.name, b.name, "{:04x}", a.pid);
                }
            }
        }
    }

    /// Lighting follows G-Helper's `HasRGB()`, left out where it uses its
    /// own packets or a 0-4 brightness (Chakram, Pugio II, Keris Wireless,
    /// Strix Impact II Wireless, Gladius II Wireless). Only the Harpe II Ace
    /// was tested, with zone 0 as GearLink sends it.
    #[test]
    fn lighting_is_where_g_helper_has_it() {
        let mut lit: Vec<(&str, Option<LightingZone>)> = MODELS
            .iter()
            .filter(|m| m.features.lighting.is_some())
            .map(|m| (m.name, m.features.lighting))
            .collect();
        lit.dedup();
        let all = Some(LightingZone::All);
        assert_eq!(
            lit,
            [
                ("Harpe II Extreme Edition 20", Some(LightingZone::First)),
                ("ROG Harpe II Ace", Some(LightingZone::First)),
                ("Harpe Ace Mini", all),
                ("ROG Harpe Ace Aim Lab Edition", all),
                ("ROG Harpe Ace Extreme", all),
                ("ROG Keris II Ace", all),
                ("ROG Keris II Origin", all),
                ("ROG Keris II Origin KJP", all),
                ("ROG Keris Wireless Aimpoint", all),
                ("ROG Gladius III Aimpoint", all),
                ("Strix Impact III Wireless", all),
                ("ROG Chakram X", all),
                ("ROG Gladius III Aimpoint", all),
                ("ROG Gladius III Eva 2", all),
                ("ROG Gladius III Wireless", all),
                ("ROG Harpe Ace Aim Lab Edition", all),
                ("ROG Harpe Ace Extreme", all),
                ("Harpe Ace Mini", all),
                ("ROG Keris II Ace", all),
                ("ROG Keris II Origin", all),
                ("ROG Keris II Origin KJP", all),
                ("ROG Keris Wireless Aimpoint", all),
                ("ROG Spatha X", all),
                ("TUF GAMING Mini Miku Edition", all),
            ]
        );
    }

    /// Ranges the TUI steps through must be consistent, or it could offer a
    /// value the mouse can't store.
    #[test]
    fn features_are_consistent() {
        for m in MODELS {
            let f = &m.features;
            let d = &f.dpi;
            assert!((1..=4).contains(&d.stages), "{}", m.name);
            assert!(
                d.step > 0 && d.min % d.step == 0 && d.max % d.step == 0,
                "{}",
                m.name
            );
            assert!(u16::try_from(d.max / d.step - 1).is_ok(), "{}", m.name);
            assert!(!d.switchable || d.stages > 1, "{}", m.name);
            assert!(!f.polling_rates.is_empty(), "{}", m.name);
            assert!(f.polling_rates.is_sorted(), "{}", m.name);
            assert!(f.power_saving_rates.is_sorted(), "{}", m.name);
            if let Some(t) = f.angle_tuning {
                assert!(f.angle_snapping && t.min < 0 && t.max > 0, "{}", m.name);
                assert_eq!(t.max % t.step, 0, "{}", m.name);
            }
            if let Some(w) = f.low_battery_warning {
                // Both go in one command: without a power-off byte to read
                // back, changing the warning would send "never".
                assert!(f.power_off.is_some(), "{}", m.name);
                assert_eq!(w.max % w.step, 0, "{}", m.name);
                assert!(!w.quarters || w.step % 25 == 0, "{}", m.name);
            }
        }
    }
}
