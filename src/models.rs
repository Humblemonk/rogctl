//! Supported mice, transcribed from G-Helper's `app/Peripherals/Mouse/Models/`
//! and `PeripheralsProvider.cs` (https://github.com/seerge/g-helper, 912cde3).
//! Models whose `HasBattery()` is false there are left out.
//!
//! Each entry is one way a mouse shows up on USB: its own receiver, a cable,
//! or a shared receiver (SpeedNova, Omni). Names drop G-Helper's
//! "(Wireless)"/"(Wired)"/"(OMNI)" suffix; `wireless` carries that instead.

use crate::device::{Battery, Match, Model};

/// ROG Omni receiver; the paired mouse is found by asking the receiver.
pub const OMNI_PID: u16 = 0x1ace;

/// Mice on their own receiver or cable. G-Helper's defaults: report ID 0,
/// 65-byte packets, battery percentage in byte 5.
const fn usb(name: &'static str, pid: u16, interface: u8, wireless: bool) -> Model {
    Model {
        name,
        pid,
        interface,
        report_id: 0x00,
        packet_size: 65,
        wireless,
        battery: Battery::Percent,
        settings: true,
        matches: Match::Any,
    }
}

/// Older mice that report the battery as a 0-4 level.
const fn usb_quarters(name: &'static str, pid: u16, interface: u8, wireless: bool) -> Model {
    Model {
        battery: Battery::Quarters { byte: 5 },
        settings: false,
        ..usb(name, pid, interface, wireless)
    }
}

/// Newer mice that use 64-byte packets.
const fn usb64(name: &'static str, pid: u16, interface: u8, wireless: bool) -> Model {
    Model {
        packet_size: 64,
        ..usb(name, pid, interface, wireless)
    }
}

/// Mice on the Omni receiver, identified by the paired device's product IDs.
const fn omni(name: &'static str, paired: &'static [u16]) -> Model {
    Model {
        name,
        pid: OMNI_PID,
        interface: 2,
        report_id: 0x03,
        packet_size: 64,
        wireless: true,
        battery: Battery::Percent,
        settings: true,
        matches: Match::OmniPaired(paired),
    }
}

pub const MODELS: &[Model] = &[
    // SpeedNova 8K receiver, shared by the Harpe II models; G-Helper tells
    // them apart by the receiver's USB product name.
    Model {
        report_id: 0x03,
        battery: Battery::Quarters { byte: 7 },
        settings: false,
        matches: Match::Product("EXTREME"),
        ..usb64("Harpe II Extreme Edition 20", 0x1ad0, 2, true)
    },
    Model {
        report_id: 0x03,
        ..usb64("ROG Harpe II Ace", 0x1ad0, 2, true)
    },
    usb64("ROG Harpe II Ace", 0x1c69, 0, false),
    // Omni receiver
    omni("Harpe Ace Mini", &[0x1b65]),
    omni("ROG Harpe Ace Aim Lab Edition", &[0x1a94]),
    omni("ROG Harpe Ace Extreme", &[0x1b68, 0x1b69]),
    omni("ROG Keris II Ace", &[0x1b1a, 0x1b18]),
    omni("ROG Keris II Origin", &[0x1c0e]),
    omni("ROG Keris II Origin KJP", &[0x1d4e]),
    omni("ROG Keris Wireless Aimpoint", &[0x1a68, 0x1a6a]),
    omni("ROG Gladius III Aimpoint", &[0x1a72]),
    omni("Strix Impact III Wireless", &[0x1ad7]),
    // Own receiver or cable
    usb("ROG Chakram X", 0x1a1a, 0, true),
    usb("ROG Chakram X", 0x1a18, 0, false),
    usb_quarters("ROG Chakram", 0x18e5, 0, true),
    usb_quarters("ROG Chakram", 0x18e3, 0, false),
    usb("ROG Gladius III Aimpoint", 0x1a72, 0, true),
    usb("ROG Gladius III Aimpoint", 0x1a70, 0, false),
    usb("ROG Gladius III Eva 2", 0x1b0c, 0, true),
    usb("ROG Gladius III Eva 2", 0x1b0a, 0, false),
    usb("ROG Gladius III Wireless", 0x197f, 0, true),
    usb("ROG Gladius III Wireless", 0x197d, 0, false),
    usb_quarters("Gladius II Wireless", 0x18a0, 2, true),
    usb("ROG Harpe Ace Aim Lab Edition", 0x1a94, 0, true),
    usb("ROG Harpe Ace Aim Lab Edition", 0x1a92, 0, false),
    usb("ROG Harpe Ace Extreme", 0x1b67, 0, false),
    usb64("Harpe Ace Mini", 0x1b63, 0, false),
    usb("ROG Keris II Ace", 0x1b16, 0, false),
    usb64("ROG Keris II Origin", 0x1c0c, 0, false),
    usb64("ROG Keris II Origin KJP", 0x1d4c, 0, false),
    usb_quarters("ROG Keris Wireless", 0x1960, 0, true),
    usb_quarters("ROG Keris Wireless", 0x195e, 0, false),
    usb_quarters("ROG Keris EVA Edition", 0x1a59, 0, true),
    usb_quarters("ROG Keris EVA Edition", 0x1a57, 0, false),
    usb("ROG Keris Wireless Aimpoint", 0x1a68, 0, true),
    usb("ROG Keris Wireless Aimpoint", 0x1a66, 0, false),
    Model {
        settings: false,
        ..usb("ASUS Mouse MD200", 0x1a24, 2, true)
    },
    usb_quarters("ROG Pugio II", 0x1908, 0, true),
    usb_quarters("ROG Pugio II", 0x1906, 0, false),
    usb("ROG Spatha X", 0x1979, 0, true),
    usb("ROG Spatha X", 0x1977, 0, false),
    Model {
        battery: Battery::Quarters { byte: 7 },
        ..usb_quarters("ROG Strix Carry", 0x18b4, 1, true)
    },
    usb_quarters("ROG Strix Impact II Wireless", 0x1949, 0, true),
    usb_quarters("ROG Strix Impact II Wireless", 0x1947, 0, false),
    usb("TUF GAMING M4 Wireless", 0x19f4, 0, true),
    usb("TX GAMING MOUSE", 0x1a8d, 0, true),
    usb("TX GAMING MOUSE Mini", 0x1af5, 0, true),
    usb("TX GAMING MOUSE Mini", 0x1af3, 0, false),
    usb("TUF GAMING Mini Miku Edition", 0x1c57, 0, true),
    usb("TUF GAMING Mini Miku Edition", 0x1c56, 0, false),
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
}
