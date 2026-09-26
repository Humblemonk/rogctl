# Contributing to rogctl

Thanks for helping out. This covers setting up, making changes, and sending them in. For what
the project does and how to install it, see [README.md](README.md).

## Setup

You need a Rust toolchain (stable, edition 2024), and for plugin work, Noctalia 5.1 or newer.

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

Then check it: `cargo run -- list` should show the mouse, and `cargo run -- --json` should
report `"state":"connected"` with the right percentage. If the mouse overrides
`GetBatteryReportPacket`, or parses the battery some other way, it needs code changes; open an
issue first.

Only send read-only queries while testing. The commands that change DPI, lighting, polling rate
or power-off settings are stored on the mouse.

The protocol details are in [AGENTS.md](AGENTS.md#hardware-protocol).

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

The service and widget read the JSON line `rogctl watch --json` prints. If you add, rename or
remove a field in `Status` (`src/main.rs`), update `service.luau` and `widget.luau` in the same
change. New user-visible text goes in `translations/en.json`, and is read with
`noctalia.tr()`.

See the [Noctalia plugin docs](https://docs.noctalia.dev/noctalia/plugins/development/) for the
Luau API.

## Commits and pull requests

- Branch from `main`, and keep each pull request to one change.
- Write commit subjects in the imperative, under about 72 characters ("Fix Omni pairing
  lookup", not "Fixed Omni pairing"). Use the body to explain why.
- Update README.md when behavior users can see changes, such as commands, flags, output fields
  or supported mice.

## Reporting a problem

Include:

- the output of `rogctl list` and `rogctl --json`
- `lsusb | grep -i asus`
- your mouse model and whether it's on the receiver or a cable
- for widget problems, your Noctalia version (`noctalia --version`) and any lines mentioning
  `rog-mouse-battery` in `~/.cache/noctalia/noctalia.log`
