# AGENTS.md

Guidance for AI coding agents working in this repository. Human-facing docs are in
[README.md](README.md).

## Project

`rogctl` reads the battery level of ASUS mice (every battery-powered mouse G-Helper supports)
on Linux over `/dev/hidraw`, and ships panel widgets that display it. Treat all
supported mice alike: don't special-case one model in docs, UI text or defaults. The CLI is
the only thing that talks to the mouse; each widget is a thin frontend over its JSON output.

| Path | What | Language |
|------|------|----------|
| `src/device.rs` | Device discovery (sysfs, Omni pairing) and the HID battery query | Rust |
| `src/models.rs` | `MODELS`: supported mice, transcribed from G-Helper | Rust |
| `src/main.rs` | CLI (`battery`, `watch`, `list`), JSON status output | Rust |
| `src/waybar.rs` | `--format waybar`: the status as Waybar custom-module JSON | Rust |
| `noctalia/rog-mouse-battery/` | Noctalia plugin: `service.luau` runs `rogctl watch --json`; `widget.luau` renders it | Luau |
| `waybar/` | Config and CSS snippets for a custom module running `rogctl watch --format waybar` | JSONC, CSS |
| `quickshell/` | `RogMouse.qml` singleton runs `rogctl watch --json`; `RogMouseWidget.qml` is an example bar item | QML |
| `dms/RogMouseBattery/` | DankMaterialShell composite plugin: daemon runs `rogctl watch --json`, widget reads its global var | QML, JS |
| `kde/rog-mouse-battery/` | Plasma 6 plasmoid; polls `rogctl battery --json` (the executable engine can't stream) | QML |
| `gnome/rog-mouse-battery@humblemonk.github.io/` | GNOME Shell extension (ESM, 45+): runs `rogctl watch --json` | GJS |
| `udev/70-rogctl.rules` | Grants the logged-in user hidraw access | udev |

## Commands

Run all of these before calling a change done; each must pass cleanly:

```sh
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo test
noctalia plugins lint noctalia/rog-mouse-battery
```

When you touch the other frontends, also check what you can without their desktops:
`/usr/lib/qt6/bin/qmllint` for QML syntax (import warnings are expected without Quickshell,
DMS or Plasma installed; `/usr/bin/qmllint` is Qt 5 and can't parse `?.` or `??`),
`node --check --input-type=module < file.js` for the GNOME JS, and
`glib-compile-schemas --strict --dry-run gnome/*/schemas`.

Try the CLI without installing: `cargo run -- --json`, `cargo run -- list`,
`cargo run -- watch --interval 2 --json`.

## Code conventions

- Rust edition 2024. Keep dependencies minimal (currently `libc`, `serde`, `serde_json`);
  no async runtime, no HID crate, no CLI-parsing crate. Ask before adding one.
- The hidraw I/O is raw `libc::poll` + non-blocking reads with a deadline. Nothing may block
  indefinitely: `watch` runs as a long-lived child of the shell and must keep emitting lines.
- The JSON status line is the contract between Rust and every widget. Fields that are `None`
  are omitted (not `null`); the widgets treat a missing field as `nil`/`undefined`. If you
  add, rename or remove a field, update all the widgets in the same change (grep for the
  field name under `noctalia/ quickshell/ dms/ kde/ gnome/`), and `src/waybar.rs`.
- Keep the widgets alike: the same settings (binary, interval, low threshold, show percent,
  hide when disconnected) with the same defaults, Solaar's battery wording
  ("Battery: 79% (discharging)"), red at the threshold, dimmed while asleep, a notification
  once per drop below the threshold (re-armed when charging or 5% above it), and restarting
  `rogctl` 10 s after it exits.
- `rogctl battery` exit codes are documented (0 connected, 1 error, 2 no device, 3 asleep);
  keep them stable.
- Luau files start with `--!nonstrict`. All user-visible strings go through `noctalia.tr()`
  with keys in `translations/en.json`; settings need `label_key` entries there too.
- The widget versions must equal `Cargo.toml`'s: `version` in `plugin.toml` and the DMS
  `plugin.json`, `KPlugin.Version` in the plasmoid's `metadata.json`, and `version-name` in the
  GNOME `metadata.json`. `cargo test` checks all four, so bump them together.
- The plugin declares `plugin_api = 24` (needed for `runAsync` with an argv array). Don't raise
  it without checking the user's installed Noctalia supports the new level.

## Hardware protocol

Derived from G-Helper's `app/Peripherals/Mouse/AsusMouse.cs`
(<https://github.com/seerge/g-helper>). That is the reference for adding models or commands.

- Request: `[report_id, 0x12, 0x07]` zero-padded to the model's `packet_size` (64 or 65
  bytes, including the report ID byte).
- Reply echoes the 3-byte header. Byte 10 = charging flag. The battery is byte 5 as a
  percentage, or a 0-4 level ×25 in byte 5 or 7 on older mice (`Battery::Quarters`). When
  `settings` is true, byte 6 = auto power-off code and 7 = low-battery warning %.
  `FF AA` at bytes 1-2 = rejected.
- Battery `0` or an all-zero reply means the mouse is asleep or out of range, not empty.
- `MODELS` in `src/models.rs` lists each way a mouse appears on USB (product ID, interface,
  report ID, packet size, battery format). Entries sharing a product ID are told apart by
  `Match`: the SpeedNova receiver (`1ad0`) by its USB product name, the Omni receiver
  (`1ace`) by asking it for the paired mouse (`01 A0`, a read-only query).
- Hardware verification so far covers one mouse on the SpeedNova receiver (`0b05:1ad0`).
  Everything else is transcribed from G-Helper and untested; say so when a change depends
  on it. hidraw omits the report-ID byte for unnumbered
  reports (report ID 0), which `read_packet` compensates for.
- `cargo test` checks that `udev/70-rogctl.rules` and the README's Supported mice list
  cover every entry in `MODELS`; update both when you add a model.
- README.md doesn't mention G-Helper; keep the attribution in `src/models.rs`, AGENTS.md
  and CONTRIBUTING.md.
- Tests that need a mouse cannot run in CI. Put protocol parsing in pure functions so it can
  be unit-tested with captured byte arrays. A real reply captured from a SpeedNova receiver:
  `03 12 07 00 00 50 02 14 d8 0f 00 00 01 …` → 80%, not charging, 3 min, warn at 20%.

## Boundaries

- **Only send read-only queries to the mouse.** Commands that change settings (G-Helper's
  `0x51 …` packets: DPI, power-off, lighting, polling rate) persist on the device. Never
  send one without the user explicitly asking for that exact change.
- **Don't change the user's system without asking first.** This includes `cargo install`,
  creating the plugin symlink, `noctalia msg plugins enable|disable`, editing anything under
  `~/.config/noctalia` or `~/.local/state/noctalia`, installing udev rules, installing or
  enabling any of the other widgets (`kpackagetool6`, `gnome-extensions`, DMS plugin links,
  Waybar or Quickshell config edits), installing a desktop or shell to test with, and anything
  needing `sudo`. Building and running from `target/` inside the repository is fine.
- Don't commit or push unless asked. Public repository: <https://github.com/humblemonk/rogctl>, branch `main`.

## Testing the Noctalia plugin

With the user's permission to set it up:
`~/.local/share/noctalia/plugins/rog-mouse-battery` is a symlink to this repo's plugin
directory, so `.luau` edits hot-reload in the running shell. Manifest (`plugin.toml`)
changes need `noctalia msg config-reload`. Logs are in `~/.cache/noctalia/noctalia.log`.
Noctalia plugin docs: <https://docs.noctalia.dev/noctalia/plugins/development/>.

## The other widgets

Hardware-verified end-to-end so far: Noctalia only. The Waybar output is unit-tested and was
run against a real mouse; the Quickshell, DMS, Plasma and GNOME frontends have been checked for
syntax only. Say so when a change depends on their runtime behavior.

- **Waybar**: `waybar/config.json` runs `rogctl watch --format waybar`. Waybar hides a custom
  module whose `text` is empty, which is how "disconnected" hides it.
- **Quickshell**: no plugin system; users copy the two `.qml` files into their config.
  Reference: <https://quickshell.org/docs/>.
- **DankMaterialShell**: plugin docs are in the DMS repository under
  `.agents/skills/dms-plugin-dev/` and `quickshell/PLUGINS/`
  (<https://github.com/AvengeMedia/DankMaterialShell>). The daemon is the only surface that runs
  `rogctl`; widgets on every bar read the `status` global var and ask for a re-read by setting
  `refresh`. Test with `dms ipc plugin-scan reload rogMouseBattery`.
- **KDE Plasma 6**: `plasmoidviewer -a kde/rog-mouse-battery` (from `plasma-sdk`) runs it
  without a panel. Notifications use the generic `plasma_workspace`/`notification` event, since
  a QML-only package can't install a `.notifyrc`.
- **GNOME**: every main-loop source and subprocess must be removed in `disable()`, or they leak
  each time the screen locks. The GNOME review guidelines
  (<https://gjs.guide/extensions/review-guidelines/review-guidelines.html>) are a good
  checklist. Test in a nested session: `dbus-run-session gnome-shell --devkit` (GNOME 49+) or
  `--nested --wayland` (older).
