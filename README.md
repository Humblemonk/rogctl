# rogctl

Battery status for ASUS mice on Linux, plus a Noctalia bar widget.

rogctl supports 27 battery-powered ROG, TUF and ASUS mice, connected through their own
receiver, a USB cable, the ROG SpeedNova 8K receiver or the ROG Omni receiver. See
[Supported mice](#supported-mice) for the list.

<p align="center">
  <img src="images/rogctl.png" alt="Project Example Screenshot">
</p>

## Getting started

You need a Rust toolchain (`cargo`); most distributions package it as `rust` or `rustup`.

1. **Check your mouse is supported.** Find it in [Supported mice](#supported-mice).

2. **Install rogctl.** This puts `rogctl` in `~/.cargo/bin`, which needs to be on your `PATH`.

   ```sh
   git clone https://github.com/humblemonk/rogctl.git
   cd rogctl
   cargo install --path .
   ```

3. **Allow access to the mouse without root.** This installs a udev rule for the supported
   USB IDs:

   ```sh
   sudo cp udev/70-rogctl.rules /etc/udev/rules.d/
   sudo udevadm control --reload && sudo udevadm trigger
   ```

4. **Read the battery.**

   ```sh
   rogctl
   ```

   You should see the mouse's name and level, such as `ROG Chakram X: 80%`.

5. **Optional: add the bar widget** if you use Noctalia. See [Noctalia widget](#noctalia-widget).

## Usage

```sh
rogctl                      # "<mouse name>: 80%", plus "(charging)" when charging
rogctl --json               # one JSON status line
rogctl watch --json         # a status line every 60 s (--interval SECS)
rogctl list                 # detected devices and their /dev/hidraw nodes
```

`rogctl` exits 0 connected, 1 error, 2 no device, 3 mouse asleep. The mouse stops
answering when it sleeps; `watch` keeps reporting the last level with `"state":"asleep"`.

## Troubleshooting

- **`No supported mouse found`**: the receiver or cable isn't plugged in, or the mouse isn't
  in [Supported mice](#supported-mice). `rogctl list` shows what was detected.
- **`Permission denied`**: the udev rule from step 3 isn't active yet. Unplug and replug the
  receiver or cable, or log out and back in.
- **`asleep or out of range`**: the mouse is off, asleep or too far from the receiver. Move it
  to wake it and run `rogctl` again.

## Noctalia widget

```sh
ln -s "$PWD/noctalia/rog-mouse-battery" ~/.local/share/noctalia/plugins/rog-mouse-battery
noctalia msg plugins enable humblemonk/rog-mouse-battery
```

Then add **ASUS Mouse Battery** from the bar's Add-widget picker, or in config:

```toml
[widget.mouse-battery]
type = "humblemonk/rog-mouse-battery:battery"
```

A background service runs `rogctl watch --json` and sends a notification when the battery
drops to the low threshold (default 20%). The widget turns red at that level, is dimmed
while the mouse sleeps, and re-reads the mouse when clicked. The refresh interval, threshold,
icon and binary path are in the plugin and widget settings.

## Supported mice

Mice not listed here aren't detected.

- ROG Chakram
- ROG Chakram X
- Gladius II Wireless
- ROG Gladius III Aimpoint
- ROG Gladius III Eva 2
- ROG Gladius III Wireless
- ROG Harpe Ace Aim Lab Edition
- ROG Harpe Ace Extreme
- Harpe Ace Mini
- ROG Harpe II Ace
- Harpe II Extreme Edition 20
- ROG Keris EVA Edition
- ROG Keris II Ace
- ROG Keris II Origin
- ROG Keris II Origin KJP
- ROG Keris Wireless
- ROG Keris Wireless Aimpoint
- ASUS Mouse MD200
- ROG Pugio II
- ROG Spatha X
- ROG Strix Carry
- ROG Strix Impact II Wireless
- Strix Impact III Wireless
- TUF GAMING M4 Wireless
- TUF GAMING Mini Miku Edition
- TX GAMING MOUSE
- TX GAMING MOUSE Mini

Tested on hardware so far: ROG Harpe II Ace on the SpeedNova 8K receiver. The rest are
untested, so please [report](CONTRIBUTING.md#reporting-a-problem) whether yours works.

## License

rogctl and the Noctalia plugin are licensed under the GNU Affero General Public License v3.0
or later (`AGPL-3.0-or-later`). See [LICENSE](LICENSE).
