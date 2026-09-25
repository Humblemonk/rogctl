# AGENTS.md

Guidance for AI coding agents working in this repository. Human-facing docs are in
[README.md](README.md).

## Project

`rogctl` reads the battery level of ASUS mice (every battery-powered mouse G-Helper supports)
on Linux over `/dev/hidraw`, and ships a Noctalia bar widget that displays it. Treat all
supported mice alike: don't special-case one model in docs, UI text or defaults. There are two parts:

| Path | What | Language |
|------|------|----------|
| `src/device.rs` | Device discovery (sysfs, Omni pairing) and the HID battery query | Rust |
| `src/models.rs` | `MODELS`: supported mice, transcribed from G-Helper | Rust |
| `src/main.rs` | CLI (`battery`, `watch`, `list`), JSON status output | Rust |
| `noctalia/rog-mouse-battery/` | Noctalia plugin: `service.luau` runs `rogctl watch --json`; `widget.luau` renders it | Luau |
| `udev/70-rogctl.rules` | Grants the logged-in user hidraw access | udev |

## Commands

Run all of these before calling a change done; each must pass cleanly:

```sh
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo test
noctalia plugins lint noctalia/rog-mouse-battery
```

Try the CLI without installing: `cargo run -- --json`, `cargo run -- list`,
`cargo run -- watch --interval 2 --json`.

## Code conventions

- Rust edition 2024. Keep dependencies minimal (currently `libc`, `serde`, `serde_json`);
  no async runtime, no HID crate, no CLI-parsing crate. Ask before adding one.
- The hidraw I/O is raw `libc::poll` + non-blocking reads with a deadline. Nothing may block
  indefinitely: `watch` runs as a long-lived child of the shell and must keep emitting lines.
- The JSON status line is the contract between Rust and Luau. Fields that are `None` are
  omitted (not `null`); the Luau side treats a missing field as `nil`. If you add, rename or
  remove a field, update `service.luau` and `widget.luau` in the same change.
- `rogctl battery` exit codes are documented (0 connected, 1 error, 2 no device, 3 asleep);
  keep them stable.
- Luau files start with `--!nonstrict`. All user-visible strings go through `noctalia.tr()`
  with keys in `translations/en.json`; settings need `label_key` entries there too.
- `version` in `plugin.toml` must equal `Cargo.toml`'s; `cargo test` checks it, so bump both.
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
- `cargo test` checks that `udev/70-rogctl.rules` and the README's Supported mice table
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
  `~/.config/noctalia` or `~/.local/state/noctalia`, installing udev rules, and anything
  needing `sudo`. Building and running from `target/` inside the repository is fine.
- Don't commit or push unless asked. Public repository: <https://github.com/humblemonk/rogctl>, branch `main`.

## Testing the Noctalia plugin

With the user's permission to set it up:
`~/.local/share/noctalia/plugins/rog-mouse-battery` is a symlink to this repo's plugin
directory, so `.luau` edits hot-reload in the running shell. Manifest (`plugin.toml`)
changes need `noctalia msg config-reload`. Logs are in `~/.cache/noctalia/noctalia.log`.
Noctalia plugin docs: <https://docs.noctalia.dev/noctalia/plugins/development/>.
