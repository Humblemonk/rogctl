---
paths:
  - "src/protocol.rs"
  - "src/device.rs"
  - "src/models.rs"
---

# Hardware protocol

Derived from G-Helper's `app/Peripherals/Mouse/AsusMouse.cs` and its model overrides in
`app/Peripherals/Mouse/Models/` (<https://github.com/seerge/g-helper>), the reference for
adding models or commands. `protocol.rs` has every byte; `device.rs` only moves them.

## Packets

- A request is `[report_id, command, …]` zero-padded to the model's `packet_size` (64 or 65
  bytes, including the report ID). hidraw omits the report-ID byte for unnumbered reports
  (report ID 0); `read_packet` compensates.
- Replies echo the request's header: 3 bytes, or 4 for the `12 00` and `12 04 xx` reads.
  `FF AA` in bytes 1-2 means rejected. Replies carry no request ID, so another process on
  the node (GearLink, a widget's `rogctl watch`) can cause a stray one.
- Battery (`12 07`): byte 10 = charging; the level is byte 5 as a percentage, or a 0-4 level
  ×25 in byte 5 or 7 on older mice (`Battery::Quarters`). `Features::power_off` names the
  power-off byte (usually 6); byte 7 is the low-battery warning.
- Battery `0`, an all-zero reply, or `FF AA` from a receiver means the mouse is asleep or out
  of range, not empty or broken.
- Settings reads: `12 00` (active DPI stage in byte 12, counted from 1; stage count in byte 19
  on `variable_stages` mice), `12 04 00` (DPI on non-X/Y mice, polling rate, debounce, angle
  snapping and tuning), `12 04 02` (X/Y DPI), `12 04 03` (stage colours, RGB), `12 04 04`
  (motion sync), `12 06` (lift-off), `12 15` (advanced power saving). DPI is stored as
  `dpi / step - 1`, 16-bit little-endian. The polling rate is the low 3 bits of its byte; the
  Harpe II Ace sets the high bits too (`0x63` at 1000 Hz), and masking them is correct.
- Changes: `51 31 <field>`, `51 35` lift-off (also resets sensor calibration), `51 37`
  power-off and warning together, `51 43` advanced power saving; each followed by `50 03`,
  which stores it on the mouse.

## Where models differ

- `Layout::Legacy` (Gladius II Wireless, Strix Carry) moves the main-page fields and
  renumbers their `51 31` fields. `LiftOffFormat::Legacy` and quarter-step warnings cover the
  Keris Wireless family. The Strix Carry keeps power-off in battery byte 5 and its active
  stage in `12 00` byte 11.
- Captured from ASUS GearLink on a Harpe II Ace, not in G-Helper or different from it:
  - Lift-off (`LiftOffFormat::HarpeII`): High is `02` in both directions, and the set
    command is `51 35 FF 00 FF <v> 01`. G-Helper's encoding is ignored by this mouse.
  - Advanced power saving: `12 15` byte 5 on/off, byte 6 a polling-rate code;
    `51 43 00 00 <on> <rate>` sets both. 125-2000 Hz (`Features::power_saving_rates`).
- Not implemented: lighting, button bindings, onboard profiles, DPI stage count, zone mode
  (`12 14`/`51 44`), acceleration/deceleration, the Omni "booster" polling-rate encoding.

## Models

- `MODELS` lists each way a mouse appears on USB (product ID, interface, report ID, packet
  size, battery format, `Features`). Entries sharing a product ID are told apart by `Match`:
  the SpeedNova receiver (`1ad0`) by its USB product name, the Omni receiver (`1ace`) by
  asking it for the paired mouse (`01 A0`, read-only).
- `Features` constants are named after the G-Helper class they come from. Code asks the
  features; never match on a model's name.
- `cargo test` checks that `udev/70-rogctl.rules` and the README's Supported mice list cover
  every entry; update both when you add a model.
- README.md doesn't mention G-Helper; the attribution lives in `src/models.rs`, AGENTS.md,
  this file and CONTRIBUTING.md.

## Verification and captures

- Hardware-verified: one mouse on the SpeedNova receiver (`0b05:1ad0`): battery and settings
  reads, and the user's own tests of every change in `rogctl tui`. Everything else is
  transcribed and untested; say so when a change depends on it. G-Helper was wrong about
  the Harpe II lift-off, so its values for similar mice may be too.
- Put parsing in pure functions and test it with captured byte arrays. A real SpeedNova
  battery reply: `03 12 07 00 00 50 02 14 d8 0f 00 00 01 …` → 80%, not charging, 3 min,
  warn at 20%.
- If a byte offset or command value is uncertain, say so and capture rather than guess.
  GearLink runs in the browser over the same hidraw node: open the node read-only
  (`O_RDONLY`, send nothing) while the user changes the setting in GearLink, and every reply,
  including the mouse's echo of each command, arrives on your file too. Then send a
  read-only `12` query to see where the value is stored. usbmon (needs root) or G-Helper's
  packet logger on Windows also work.
