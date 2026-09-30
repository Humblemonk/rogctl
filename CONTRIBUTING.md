# Contributing to rogctl

Thanks for helping out. This covers setting up, making changes, and sending them in. For what
the project does and how to install it, see [README.md](README.md).

## Setup

You need a Rust toolchain (stable, edition 2024). For widget work you need the desktop the
widget is for: Noctalia 5.1 or newer, Waybar, Quickshell, DankMaterialShell 1.5 or newer,
KDE Plasma 6, or GNOME 45 or newer.

```sh
git clone https://github.com/humblemonk/rogctl.git
cd rogctl
cargo build
```

To talk to a mouse without root, install the udev rule once (see
[Getting started](README.md#getting-started), step 3), then run from the checkout:

```sh
cargo run -- list        # which devices were found, and on which /dev/hidraw node
cargo run -- --json      # read the battery once
cargo run -- settings       # read the settings (nothing is changed until you press Enter)
cargo run -- settings --demo  # the settings screen without a mouse
```

## Before you open a pull request

All of these must pass:

```sh
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo test
noctalia plugins lint noctalia/rog-mouse-battery
```

CI can't reach a mouse, so also say in the pull request which hardware you tested on and how
it was connected (receiver or cable). If you couldn't test on hardware, say so.

## Adding a mouse

`src/models.rs` already covers every battery-powered mouse in G-Helper. When G-Helper adds
one, add it to `MODELS` using the values from its file in
[`app/Peripherals/Mouse/Models/`](https://github.com/seerge/g-helper/tree/main/app/Peripherals/Mouse/Models).
A constructor such as

```csharp
base(0x0B05, 0x1A1A, "mi_00", true)
```

maps to `usb("Name", 0x1a1a, 0, true)`: product ID, interface (from `mi_00`) and whether
that product ID is a receiver. Use `usb64` if the class overrides `USBPacketSize()` to 64,
and `usb_quarters` if `ParseBattery` multiplies by 25. A mouse on the Omni receiver
(`0x1ACE`) goes in with `omni()` and the product IDs from `MouseFromOmniPid` in
`PeripheralsProvider.cs`. Add the product ID to `udev/70-rogctl.rules` and the mouse to the
Supported mice list in README.md too; `cargo test` fails until you do.

The last argument is the mouse's `Features`: which settings it has and their ranges. They
map one to one onto the class's overrides (`DPIProfileCount`, `MaxDPI`,
`SupportedPollingrates`, `HasDebounceSetting`, `HasAngleTuning`, `HasLowBatteryWarning` and
so on). Reuse the constant of the class it inherits from if it has one, or add a constant
named after the class. A class that overrides a `Parse…` or `Get…Packet` method stores that
setting somewhere else; check it matches a `Layout`, `LiftOffFormat` or `WarningRange` that
already exists, and open an issue if it doesn't.

Then check it: `cargo run -- list` should show the mouse, `cargo run -- --json` should
report `"state":"connected"` with the right percentage, and `cargo run -- settings` should
show the settings the mouse's own software shows. If the mouse overrides
`GetBatteryReportPacket`, or parses the battery some other way, it needs code changes; open
an issue first.

Changes you apply in `rogctl settings` are stored on the mouse. Test the screens with
`--demo`, and only press Enter against a real mouse when you mean to change it.

The protocol details are in [`.claude/rules/protocol.md`](.claude/rules/protocol.md).

## Working on the Noctalia plugin

Link your checkout into Noctalia's local plugin folder and enable it:

```sh
ln -s "$PWD/noctalia/rog-mouse-battery" ~/.local/share/noctalia/plugins/rog-mouse-battery
noctalia msg plugins enable humblemonk/rog-mouse-battery
```

- `.luau` edits reload in the running shell automatically.
- Changes to `plugin.toml` need `noctalia msg config-reload`.
- Script errors appear in `~/.cache/noctalia/noctalia.log`.
- The service runs whichever `rogctl` is first on Noctalia's `PATH`. To test a local build, set
  the plugin's **rogctl command** setting to `/path/to/rogctl/target/debug/rogctl`.

New user-visible text goes in `translations/en.json`, and is read with `noctalia.tr()`.

See the [Noctalia plugin docs](https://docs.noctalia.dev/noctalia/plugins/development/) for the
Luau API.

## Working on the other widgets

Install the widget from your checkout as described in its readme (linked from
[Panel widgets](README.md#panel-widgets)), and point its **rogctl command** setting at
`target/debug/rogctl` to test a local build. Then:

- **Waybar**: `pkill -SIGUSR2 waybar` reloads the config.
- **Quickshell**: Quickshell reloads `.qml` files when they change.
- **DankMaterialShell**: `dms ipc plugin-scan reload rogMouseBattery` after an edit.
- **KDE Plasma**: `kpackagetool6 --type Plasma/Applet --upgrade kde/rog-mouse-battery`, or run
  it on its own with `plasmoidviewer -a kde/rog-mouse-battery` (from `plasma-sdk`).
- **GNOME**: pack and install again, then test in a nested shell,
  `dbus-run-session gnome-shell --devkit` (GNOME 49+), so you don't have to log out. Follow the
  [review guidelines](https://gjs.guide/extensions/review-guidelines/review-guidelines.html).

Every widget reads the JSON line `rogctl watch --json` (or `rogctl battery --json`) prints. If
you add, rename or remove a field in `Status` (`src/main.rs`), update all of them in the same
change, and `src/waybar.rs`. Keep them behaving alike: the same settings and defaults, the same
wording, and the same low-battery notification rules.
[`.claude/rules/widgets.md`](.claude/rules/widgets.md) lists the details.

The widgets share `Cargo.toml`'s version; `cargo test` fails until every manifest matches.

## Commits and pull requests

- Branch from `main`, and keep each pull request to one change.
- Write commit subjects in the imperative, under about 72 characters ("Fix Omni pairing
  lookup", not "Fixed Omni pairing"). Use the body to explain why.
- Update the docs when behavior users can see changes: README.md for installing, settings and
  supported mice, [docs/cli.md](docs/cli.md) for commands, flags, exit codes and JSON fields,
  and the widget's own readme for widget setup.

## Reporting a problem

Include:

- the output of `rogctl list` and `rogctl --json`
- for a settings problem, what `rogctl settings` shows next to what the mouse's own
  software shows
- `lsusb | grep -i asus`
- your mouse model and whether it's on the receiver or a cable
- for widget problems, your Noctalia version (`noctalia --version`) and any lines mentioning
  `rog-mouse-battery` in `~/.cache/noctalia/noctalia.log`
