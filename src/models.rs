//! Supported mice, transcribed from G-Helper's `app/Peripherals/Mouse/Models/`
//! and `PeripheralsProvider.cs` (https://github.com/seerge/g-helper, 912cde3;
//! settings from 54c5bd0). Models whose `HasBattery()` is false there are left
//! out.
//!
//! Each entry is one way a mouse shows up on USB: its own receiver, a cable,
//! or a shared receiver (SpeedNova, Omni). Names drop G-Helper's
//! "(Wireless)"/"(Wired)"/"(OMNI)" suffix; `wireless` carries that instead.
//! The `Features` constants are named after the G-Helper class they come from.

use crate::device::{Match, Model};
use crate::protocol::{
    AngleTuning, Battery, Dpi, Features, Layout, LiftOffFormat, PollingRate, WarningRange,
};

/// ROG Omni receiver; the paired mouse is found by asking the receiver.
pub const OMNI_PID: u16 = 0x1ace;

/// An Omni receiver whose paired mouse couldn't be asked for, usually for
/// lack of permission. Every Omni entry shares these query parameters, so the
/// battery read still works, or fails with the real reason. Not in `MODELS`.
pub static OMNI_RECEIVER: Model = omni("ROG Omni receiver", &[], OMNI_UNKNOWN);

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
/// `TXGamingMini` and `TUFGamingMiniMiku`.
const TX_GAMING_MINI: Features = Features {
    dpi: Dpi {
        step: 50,
        xy: true,
        ..TUF_M4_WIRELESS.dpi
    },
    ..TUF_M4_WIRELESS
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
/// An Omni receiver whose mouse is unknown: battery only.
const OMNI_UNKNOWN: Features = Features {
    dpi: Dpi { stages: 0, ..DPI },
    power_off: POWER_OFF,
    low_battery_warning: WARNING,
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
        TX_GAMING_MINI,
    ),
    usb(
        "TUF GAMING Mini Miku Edition",
        0x1c56,
        0,
        false,
        TX_GAMING_MINI,
    ),
];

#[cfg(test)]
mod tests {
    use super::*;

    /// Every product ID in the table needs a udev rule, or the mouse is
    /// found but can't be opened without root.
    #[test]
    fn udev_rules_cover_every_model() {
        let rules = include_str!("../udev/70-rogctl.rules");
        for m in MODELS {
            let needle = format!("ATTRS{{idProduct}}==\"{:04x}\"", m.pid);
            assert!(
                rules.contains(&needle),
                "udev rule missing for {} ({:04x})",
                m.name,
                m.pid
            );
        }
    }

    /// The README's Supported mice list names every mouse.
    #[test]
    fn readme_lists_every_model() {
        let readme = include_str!("../README.md");
        let section = &readme[readme.find("## Supported mice").expect("section missing")..];
        for m in MODELS {
            let item = format!("- {}", m.name);
            assert!(
                section.lines().any(|l| l == item),
                "README doesn't list {}",
                m.name
            );
        }
    }

    /// Two entries for the same USB interface must be told apart by `matches`.
    #[test]
    fn entries_are_unambiguous() {
        for (i, a) in MODELS.iter().enumerate() {
            for b in &MODELS[i + 1..] {
                if a.pid == b.pid && a.interface == b.interface {
                    assert!(
                        !matches!(a.matches, Match::Any),
                        "{} shadows {} ({:04x})",
                        a.name,
                        b.name,
                        a.pid
                    );
                }
            }
        }
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
                assert_eq!(w.max % w.step, 0, "{}", m.name);
                assert!(!w.quarters || w.step % 25 == 0, "{}", m.name);
            }
        }
    }
}
