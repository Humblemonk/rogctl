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
  the node (GearLink, a widget's `rogctl watch`) can cause a stray one. They do carry the
  request's report ID, rejections included (`03 ff aa` from the SpeedNova with the Harpe II
  Ace off, `02 ff aa` from the Omni receiver with the Falchion RX off), so `read_packet`
  ignores a reply on another report ID. The report ID of the mouse's own rejection of a
  first save (below) wasn't recorded.
- Battery (`12 07`): byte 10 = charging; the level is byte 5 as a percentage, or a 0-4 level
  ×25 in byte 5 or 7 on older mice (`Battery::Quarters`). `Features::power_off` names the
  power-off byte (usually 6); byte 7 is the low-battery warning.
- Battery `0` while not charging, an all-zero reply, no reply, or `FF AA` from a receiver
  means the device is asleep or out of range, not empty or broken.
- Settings reads: `12 00` (active DPI stage in byte 12, counted from 1; stage count in byte
  19 on `variable_stages` mice), `12 04 00` (DPI on non-X/Y mice, polling rate, debounce,
  angle snapping and tuning), `12 04 02` (X/Y DPI), `12 04 03` (stage colours, RGB),
  `12 04 04` (motion sync), `12 06` (lift-off), `12 15` (advanced power saving), `12 03 00`
  (lighting: effect in byte 5, brightness in byte 6, colour in bytes 7-9). DPI is stored as
  `dpi / step - 1`, 16-bit little-endian. The polling rate is the low 3 bits of its byte;
  the Harpe II Ace sets the high bits too (`0x63` at 1000 Hz), and masking them is correct.
- Changes: `51 31 <field>`, `51 35` lift-off (also resets sensor calibration), `51 37`
  power-off and warning together, `51 43` advanced power saving,
  `51 28 <zone> 00 <effect> <brightness> R G B` lighting; each followed by `50 03`, which
  stores it on the mouse.

## Where models differ

- `Layout::Legacy` (Gladius II Wireless, Strix Carry) moves the main-page fields and
  renumbers their `51 31` fields. `LiftOffFormat::Legacy` covers the Keris Wireless family
  and Strix Impact II Wireless, which store the warning in quarters (1 = 25%) as the Chakram
  and Pugio II do. The Strix Carry keeps power-off in battery byte 5 and its active
  stage in `12 00` byte 11.
- Captured from ASUS GearLink on a Harpe II Ace, not in G-Helper or different from it:
  - Lift-off (`LiftOffFormat::HarpeII`): High is `02` in both directions, and the set
    command is `51 35 FF 00 FF <v> 01`. G-Helper's encoding is ignored by this mouse.
  - Advanced power saving: `12 15` byte 5 on/off, byte 6 a polling-rate code;
    `51 43 00 00 <on> <rate>` sets both. 125-2000 Hz (`Features::power_saving_rates`).
  - Lighting (`LightingZone::First`): GearLink reads and sets zone `00`; the read's reply
    has 0 in byte 3 whatever zone is asked. Effects as in G-Helper (Static `00`, Breathing
    `01`, Colour cycle `02`, Reactive `04`, Battery `06`); GearLink has no Off effect but
    sets brightness 0, a percentage it moves in 10% steps. Battery mode's green/blue/red
    aren't in the packet. GearLink remembers each effect's colour and brightness itself and
    sends them on a switch; the mouse holds only the current ones. `12 03 01` answers too
    (effect `02`, 50%, black); what that slot is isn't known. Zone `03` (all, as the other
    mice get) works on it too: sent unsaved on the SpeedNova, it was echoed, lit the wheel
    and read back from zone `00`.
- Lighting on other mice is G-Helper's and untested (`LightingZone::All`): every mouse whose
  class has `HasRGB()` and the default `12 03`/`51 28` packets gets the same five effects,
  set on zone `03` (all zones at once) and read from zone `00`, as G-Helper's settings
  window does; it sends a single zone (`00` logo, `01` wheel, `02` underglow) only when one
  is picked or a profile imported. Rainbow (`03`), Comet (`05`) and Off (`F0`) are shown
  but not offered, as they carry a speed, direction or random colour rogctl doesn't send.
  The Chakram, Pugio II, Keris Wireless family, Strix Impact II Wireless and Gladius II
  Wireless override those packets, the brightness range (0-4) or the effect codes, and get
  no lighting.
- Not implemented: lighting on the mice above with their own encodings, single lighting
  zones, Rainbow and Comet, button bindings, onboard profiles, DPI stage count, zone mode
  (`12 14`/`51 44`), acceleration/deceleration, the Omni "booster" polling-rate encoding.

## Models

- `MODELS` lists each way a mouse appears on USB (product ID, interface, report ID, packet
  size, battery format, `Features`). Entries sharing a product ID are told apart by `Match`:
  the SpeedNova receiver (`1ad0`) by its USB product name, the Omni receiver (`1ace`) by
  asking it for the paired devices (`01 A0`, read-only).
- The Omni reply (captured on interface 2 with only a Falchion RX Low Profile keyboard
  paired, `parses_captured_omni_reply_with_a_keyboard`) echoes `01 A0` and lists 4-byte
  slots from byte 5: product ID (little-endian), then two bytes, `02 04` for that keyboard,
  meaning unknown; byte 3 may be the count. It answers in 1-2 ms with nothing connected, and
  the same with the keyboard on: it lists every device paired, not which is connected. So
  with two supported mice paired, rogctl takes the first in `MODELS` order (G-Helper the
  first slot) and can't tell which is on. A pairing query that times out, or gets `FF AA`
  or an all-zero reply, leaves the receiver out, as one with nothing supported paired is; only one
  that can't be opened is kept, as `OMNI_RECEIVER`. Keeping it on a miss was tried and
  undone: a keyboard-only receiver then showed as a mouse, and an awake mouse as "ROG Omni
  receiver" with no settings, which the TUI didn't retry.
- `Features` constants are named after the G-Helper class they come from. Code asks the
  features; never match on a model's name.
- `cargo test` checks that `udev/70-rogctl.rules` has exactly one line per entry's product ID
  and interface, and that the readme's Supported mice and Supported keyboards lists cover
  every entry; update both when you add a model. The rule matches the USB interface's
  `modalias` (`usb:v0B05p1ACE*in02`), the one attribute holding vendor, product and
  interface, so it opens only the interface rogctl uses: receivers and keyboards also have a
  keyboard interface (usage page 7) that carries every keystroke.
- Report descriptors read from sysfs (`/sys/class/hidraw/*/device/report_descriptor`,
  world-readable) on 2026-11-01: rogctl's interface has only vendor pages on the SpeedNova
  (`1ad0` interface 2: `FF00`-`FF02`), the Omni receiver (`1ace` interface 2: the same), the
  Harpe II Ace cable (`1c69` interface 0: `FF01`) and the Falchion RX Low Profile wired
  (`1b04` interface 1: `FF00`). Their other interfaces carry keystrokes (page 7), the
  pointer or media keys, and the installed rule leaves them closed. Other models' interfaces
  are unchecked.
- README.md doesn't mention G-Helper; the attribution lives in `src/models.rs`, AGENTS.md,
  this file and CONTRIBUTING.md.

## Keyboards

- Battery only, from G-Helper's `app/Peripherals/Keyboard/AsusKeyboard.cs` and
  `Models/`: `12 01` (its "power" page) on interface 1 (`mi_01`), report ID 0, 65 bytes; on
  the Omni receiver interface 2, report ID 2 (`col02`, page `FF00`), 64 bytes, beside a
  mouse's report 3 on the same node. The percentage is byte 6, or 11 on the Falchion family
  (`Battery::Keyboard { byte }`); byte 9 is 1 while charging. G-Helper's comments say byte 7
  is the sleep timeout; bytes 7 and 10 aren't read (10 was `14`, perhaps a 20% warning).
- `KEYBOARDS` follows `MODELS`, so every mouse ranks first. `select_models()` takes the first
  match of each kind, so one Omni receiver can yield a mouse and a keyboard: they share the
  node, `discover()` lists both, and `rogctl battery --device all` groups entries by kind and
  name, as it does a mouse's receiver and cable.
- Left out: G-Helper's keyboards without an `mi_` path (it picks the interface from the
  report descriptor: a vendor `FF00` page with 64-byte output reports), the Claymore II
  (`196b`) and Strix Scope RX TKL Wireless (`1a07`). The Falchion RX (`1b04`) is one of those
  in G-Helper but was captured on interface 1.
- Captured 2026-11-01 from a Falchion RX Low Profile, read-only (`12 01`, `01 A0`):
  - Omni receiver: pairing reply `01 a0 00 01 00 06 1b 02 04` (`1b06`); battery
    `02 12 01 00 00 00 34 02 01 00 14 34 ff 0e`: 52% in bytes 6 and 11 (not a 0-10 gauge on
    this one), not charging. On the cable in 2.4 GHz mode byte 9 became `01` and bytes 12-13
    `65 0f` (millivolts, perhaps); no `1b04` node appears then. Switched off: `02 ff aa` in
    1-2 ms, which `means_asleep` reads as asleep, and the pairing reply is unchanged.
  - Wired mode (`0b05:1b04`, "ROG FALCHION RX LOW PROFILE"): five interfaces; interface 1 is
    the only `FF00` page, unnumbered, 64-byte input and output. Same reply after the report ID
    (`12 01 00 00 00 34 02 01 01 14 34 84 0f`), charging.
- The reporter of issue #34 read a Falchion (`193e`) the same way: interface 1, bytes 11 and 9.

## Verification and captures

- Hardware-verified: one mouse, a Harpe II Ace, and one keyboard, a Falchion RX Low Profile
  (battery only, above). On the SpeedNova receiver (`0b05:1ad0`):
  battery and settings reads, and the user's own tests of every change in
  `rogctl settings`. On its cable in wired mode (`0b05:1c69` interface 0, report ID 0,
  64-byte packets): found and read in `rogctl settings` (battery and settings), and
  active-stage and polling-rate changes, each echoed and saved without `FF AA`, kept through
  a replug. With the receiver plugged in, the cable only charges until the
  mouse's mode button is set to wired; then the receiver answers `FF AA` (so, asleep) and the
  cable's replies match the receiver's byte for byte after the report ID
  (`reads_harpe_ii_ace_over_its_cable`).
- The Harpe II Ace on the SpeedNova answers the first save (`50 03`) after it sat unused or
  was just turned on with `FF AA`, for active-stage and polling-rate changes alike, and
  sometimes sends the save's echo 1 ms later; the change survives a power cycle either way
  (captured read-only, with only rogctl and its widget on the node). G-Helper logs such
  errors and carries on. `Connection::write()` keeps waiting up to 150 ms for the echo of a
  change or save after an `FF AA` (this mouse answers a read in about 5 ms, or 70 ms after
  sitting unused); with none, `Connection::apply()` asks for the save once more, which the
  mouse then echoes, and reports the save as unconfirmed only if that's rejected too. Its
  DPI button changes the active stage without any input report on this node, so the TUI
  polls `12 00`.
- Everything else is transcribed and untested; say so when a change depends on it. G-Helper
  was wrong about the Harpe II lift-off, so its values for similar mice may be too.
- Put parsing in pure functions and test it with captured byte arrays. A real SpeedNova
  battery reply: `03 12 07 00 00 50 02 14 d8 0f 00 00 01 …` → 80%, not charging, 3 min,
  warn at 20%.
- If a byte offset or command value is uncertain, say so and capture rather than guess.
  GearLink runs in the browser over the same hidraw node: open the node read-only
  (`O_RDONLY`, send nothing) while the user changes the setting in GearLink, and every reply,
  including the mouse's echo of each command, arrives on your file too. Then send a
  read-only `12` query to see where the value is stored. usbmon (needs root) or G-Helper's
  packet logger on Windows also work.
