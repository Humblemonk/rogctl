# AGENTS.md

Guidance for AI coding agents working in this repository. Human-facing docs are in
[README.md](README.md).

Topic detail lives in `.claude/rules/`. Read the matching file before editing (Claude Code
loads them automatically): `protocol.md` for `protocol.rs`/`device.rs`/`models.rs` (packets,
per-model differences, capturing unknown commands), `tui.md` for `app.rs`/`ui.rs`/`tui.rs`
(behaviour, same UI for every mouse, adding a setting), `widgets.md` for the panel widgets,
`main.rs` and `waybar.rs` (the JSON contract, keeping widgets alike).

## Project

`rogctl` reads the battery level of ASUS mice (every battery-powered mouse G-Helper supports)
on Linux over `/dev/hidraw`, and ships panel widgets that display it. `rogctl settings`
reads and changes the mouse's settings. Treat all supported mice alike: don't special-case
one model in docs, UI text or defaults. The CLI is the only thing that talks to the mouse;
each widget is a thin frontend over its JSON output. Prefer the simple, obvious solution.

| Path | What |
|------|------|
| `src/device.rs` | Discovery (sysfs, Omni pairing) and hidraw I/O: `Connection` sends a request, waits for its reply |
| `src/protocol.rs` | Every byte sent or parsed: requests, reply parsers, `Features` (what a model has) |
| `src/models.rs` | `MODELS`: supported mice and their `Features`, transcribed from G-Helper |
| `src/main.rs` | CLI (`battery`, `watch`, `list`, `settings`), JSON status output |
| `src/waybar.rs` | `--format waybar`: the status as Waybar custom-module JSON |
| `src/app.rs` | TUI state and key handling (`Control`, `Value`, `Action`); no I/O |
| `src/ui.rs` | TUI rendering with ratatui |
| `src/tui.rs` | TUI terminal setup, event loop, and `apply_action()`, the only writer |
| `noctalia/`, `waybar/`, `quickshell/`, `dms/`, `kde/`, `gnome/` | Panel widgets over `rogctl watch --json` (Plasma polls `rogctl battery --json`) |
| `udev/70-rogctl.rules` | Grants the logged-in user hidraw access |
| `README.md`, `docs/cli.md` | User docs; `docs/cli.md` has the commands, exit codes and JSON fields |

## Commands

Run all of these before calling a change done; each must pass cleanly:

```sh
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo test
noctalia plugins lint noctalia/rog-mouse-battery
```

CI also runs super-linter on changed files. jscpd allows 3% duplication, SonarQube's default
quality gate (`.github/linters/.jscpd.json`): fix a clone with a shared helper rather than
raising it.
textlint checks Markdown wording: its terminology rule wants a lowercase "readme" outside
filenames.

Try it without installing: `cargo run -- --json`, `cargo run -- list`,
`cargo run -- watch --interval 2 --json`, and without a mouse
`cargo run -- settings --demo` (or `--demo "ROG Chakram"`, any name in `MODELS`).

## Conventions

- Rust edition 2024, MSRV 1.88 (ratatui's). Keep dependencies minimal (`libc`, `ratatui`,
  `serde`, `serde_json`): no async runtime, HID crate, CLI-parsing crate or `anyhow`. Ask
  before adding one. crossterm comes through `ratatui::crossterm`; don't depend on it directly.
- hidraw I/O is raw `libc::poll` + non-blocking reads with a deadline. Nothing may block
  indefinitely: `watch` runs as a long-lived child of the shell and must keep emitting lines.
- `rogctl battery` exit codes are documented (0 connected, 1 error, 2 no device, 3 asleep);
  keep them stable.
- The widget versions equal `Cargo.toml`'s: `version` in `plugin.toml` and the DMS
  `plugin.json`, `KPlugin.Version` in the plasmoid's `metadata.json`, `version-name` in the
  GNOME `metadata.json`. `cargo test` checks all four; bump them together.

## Layering

Control flow for a change: key → `App::handle_key()` → `Action` → `tui::apply_action()` →
`Connection::apply()` → `protocol::change_request()` + `save_request()`.

- `tui::apply_action()` is the only caller of `Connection::apply()`, the only code that sends
  a change. `app.rs` and `ui.rs` never use `device`.
- Raw protocol bytes live only in `protocol.rs`, as named constants. `device.rs` moves bytes
  without knowing what they mean.
- What a mouse supports lives in its `Features`. Code asks the features (or the settings read
  back); never match on a model's name.

## Rust rules

- No `unwrap()`/`expect()` outside tests; no `panic!()`, `todo!()` or `unimplemented!()`.
  Read replies with bounds-checked access (`protocol::byte()`).
- Exhaustive matches on rogctl's own enums: no `_` arm that would swallow a new variant. `_`
  is fine for foreign enums (`KeyCode`, crossterm `Event`) with a comment.
- Validate packet arguments before encoding: clamp or refuse (`None`), don't panic.
- crossterm: act on `KeyEventKind::Press` only. ratatui: draw with `Frame::render_widget()`,
  not direct buffer writes.
- Meaningful names; delete code you replace rather than keeping versioned copies.

## Testing

| Situation | Approach |
| --- | --- |
| New request or reply field | `protocol.rs` test: exact request bytes, and a reply parsed |
| Captured reply from real hardware | Add it as a test, with a comment saying where it came from |
| New model or `Features` change | `features_are_consistent()`; the demo tests cover every model |
| Key handling, focus, edits | `app.rs` tests on a demo `App` |
| Rendering | `ui.rs` tests with ratatui's `TestBackend` |
| Terminal setup, `main()` argument parsing | No tests; try `cargo run -- settings --demo` |

Tests that need a mouse can't run in CI; keep protocol parsing in pure functions.

## Workflow

- For non-trivial features, read the relevant code and G-Helper's, and confirm a plan with
  the user before implementing.
- If a byte offset or command value is uncertain, say so and capture it rather than guess
  (`protocol.md` explains how, read-only, with ASUS GearLink).
- Say in the commit or PR body which mice a change depends on, and whether and how it was
  tested on hardware. Only one mouse has been: see `protocol.md`.

## Boundaries

- **Never change the user's mouse without being asked for that exact change.** Settings
  changes (`51 …` and `50 …` packets) persist on the device. Reads (`12 …`, `01 A0`) are
  fine. `rogctl settings` against the real mouse only reads until Enter or Space is pressed
  or a selected row is clicked, so an agent driving it may send navigation keys and `q` but
  never Enter, Space or mouse clicks. Test changes with `--demo` and unit tests.
- **Don't change the user's system without asking first.** This includes `cargo install`,
  creating the plugin symlink, `noctalia msg plugins enable|disable`, editing anything under
  `~/.config/noctalia` or `~/.local/state/noctalia`, installing udev rules, installing or
  enabling any of the other widgets (`kpackagetool6`, `gnome-extensions`, DMS plugin links,
  Waybar or Quickshell config edits), installing a desktop or shell to test with, and anything
  needing `sudo`. Building and running from `target/` inside the repository is fine.
- Don't commit or push unless asked. Public repository: <https://github.com/humblemonk/rogctl>,
  branch `main`.
