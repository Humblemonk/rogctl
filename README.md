# rogctl

Battery status and settings for ASUS mice on Linux, plus panel widgets for Noctalia, Waybar,
Quickshell, DankMaterialShell, KDE Plasma and GNOME.

rogctl supports 27 battery-powered ROG, TUF and ASUS mice, connected through their own
receiver, a USB cable, the ROG SpeedNova 8K receiver or the ROG Omni receiver. See
[Supported mice](#supported-mice) for the list.

<table align="center">
  <tr>
    <td align="center" valign="middle">
      <img src="images/rogctl.png" width="304"
        alt="Panel widget showing the mouse at 79%, with a tooltip saying Battery 79% (discharging), Device ROG Harpe II Ace">
    </td>
    <td align="center" valign="middle">
      <img src="images/rogctl_tui.png" width="500"
        alt="rogctl tui showing a ROG Harpe II Ace's DPI stages, polling rate and battery settings">
    </td>
  </tr>
  <tr>
    <td align="center">Panel widget</td>
    <td align="center"><code>rogctl tui</code></td>
  </tr>
</table>

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

5. **Optional: add a panel widget.** See [Desktop widgets](#desktop-widgets).

## Usage

```sh
rogctl                      # "<mouse name>: 80%", plus "(charging)" when charging
rogctl --json               # one JSON status line
rogctl watch --json         # a status line every 60 s (--interval SECS)
rogctl watch --format waybar  # the same, as Waybar custom-module JSON
rogctl list                 # detected devices and their /dev/hidraw nodes
rogctl tui                  # view and change the mouse's settings
```

`--format` takes `text` (the default), `json` or `waybar`. `--low-threshold PCT` (default 20,
0 to turn off) sets the level at which the battery counts as low: JSON gets `"low":true` and
Waybar the `low` CSS class.

`rogctl` exits 0 connected, 1 error, 2 no device, 3 mouse asleep. The mouse stops
answering when it sleeps; `watch` keeps reporting the last level with `"state":"asleep"`.

`watch --json` also adds `"notify_low":true` to the one line where the battery first drops to
the threshold (again only after charging or rising 5% above it), and its `"pid"`. Send that
process `SIGUSR1` (`kill -USR1 PID`) to read the mouse right away.

## Settings

`rogctl tui` shows the mouse's settings in the terminal and lets you change them:

- **DPI stages**: the DPI of each stage, and which stage is active
- **Polling rate**
- **Button debounce**
- **Angle snapping** and **angle tuning**
- **Motion sync** (not available at 8000 Hz)
- **Lift-off distance**
- **Auto power-off** and **low-battery warning**
- **Advanced power saving**, and the polling rate it uses (ROG Harpe II models)

It only shows the settings your mouse has, with the values it supports. The mouse has to be
awake: move it if rogctl says it's asleep.

| Key | Does |
| --- | --- |
| ↑ ↓ (or k j) | Select a setting |
| ← → (or h l) | Change the value; Shift, PgUp or PgDn for bigger steps |
| 0-9 | Type a DPI value |
| Enter, Space | Apply the change, switch a setting on or off, or make a DPI stage active |
| Esc | Cancel the change |
| r | Read the settings from the mouse again |
| Tab | Next mouse, when several are plugged in |
| ? | Help |
| q | Quit |

The mouse works too: click a setting to select it and click it again to apply, like Enter.
Scrolling over a setting, or clicking a value on a scale, changes it without sending it yet.
Most terminals let you hold Shift to select text while rogctl uses the mouse.

Nothing is sent until you press Enter. Applied changes are stored on the mouse, so they stay
after unplugging it and apply on other computers too. Changing the lift-off distance also
resets the sensor calibration.

Reading and changing settings has been tested on a ROG Harpe II Ace on the SpeedNova 8K
receiver; other mice are untested. If something reads wrong or doesn't
change, please [report it](CONTRIBUTING.md#reporting-a-problem).

The screen uses ASUS's ROG colours and needs a terminal with 24-bit colour, which most
current ones have.

To look around without a mouse, run `rogctl tui --demo`, or `rogctl tui --demo "ROG Chakram"`
for another model from [Supported mice](#supported-mice). Demo changes aren't sent anywhere.

## Troubleshooting

- **`No supported mouse found`**: the receiver or cable isn't plugged in, or the mouse isn't
  in [Supported mice](#supported-mice). `rogctl list` shows what was detected.
- **`Permission denied`**: the udev rule from step 3 isn't active yet. Unplug and replug the
  receiver or cable, or log out and back in.
- **`asleep or out of range`**: the mouse is off, asleep or too far from the receiver. Move it
  to wake it and run `rogctl` again.

## Desktop widgets

Each widget runs `rogctl` and shows the mouse's battery level in your panel or bar. It turns
red at the low threshold (default 20%), is dimmed while the mouse sleeps (showing the last
level read), and hides while no mouse is plugged in. All but Waybar also send a notification
when the battery drops to the threshold. Run the commands below from your rogctl checkout.

Widgets find `rogctl` on your desktop session's `PATH`, which may not include `~/.cargo/bin`.
If a widget says rogctl wasn't found, set its **rogctl command** setting to the full path, such
as `/home/you/.cargo/bin/rogctl`. For Waybar and Quickshell, edit the command in the config.

### Noctalia

```sh
ln -s "$PWD/noctalia/rog-mouse-battery" ~/.local/share/noctalia/plugins/rog-mouse-battery
noctalia msg plugins enable humblemonk/rog-mouse-battery
```

Then add **ASUS Mouse Battery** from the bar's Add-widget picker, or in config:

```toml
[widget.mouse-battery]
type = "humblemonk/rog-mouse-battery:battery"
```

Click the widget to read the mouse right away. The refresh interval, threshold, icon and
binary path are in the plugin and widget settings.

### Waybar

Copy the `custom/rog-mouse` module from [`waybar/config.json`](waybar/config.json) into your
Waybar config, add `"custom/rog-mouse"` to one of your `modules-*` lists, and append
[`waybar/style.css`](waybar/style.css) to your `style.css`. The icons need a Nerd Font.
`restart-interval` starts `rogctl` again 10 seconds after it exits, such as when it wasn't on
`PATH` yet.

The module gets a CSS class for the state (`connected`, `asleep`, `error`, `disconnected`),
plus `charging` and `low`; `format-icons` is keyed by the same names. Waybar can't send
notifications, so it only turns red.

### Quickshell

For a Quickshell config of your own. Copy both files next to your `shell.qml`:

```sh
cp quickshell/RogMouse.qml quickshell/RogMouseWidget.qml ~/.config/quickshell/
```

Then put `RogMouseWidget {}` in your bar. `RogMouse` is a singleton that runs one `rogctl` for
every bar; its settings are at the top of `RogMouse.qml`. `RogMouseWidget.qml` is a minimal
example (it needs a Nerd Font) to restyle, or build your own from `RogMouse.battery`,
`RogMouse.charging`, `RogMouse.low` and `RogMouse.status`. Notifications use `notify-send`.

### DankMaterialShell

```sh
ln -s "$PWD/dms/RogMouseBattery" ~/.config/DankMaterialShell/plugins/RogMouseBattery
dms ipc plugin-scan scan
```

Enable **ASUS Mouse Battery** in Settings → Plugins, then add it to the bar. Click it for
details, right-click to read the mouse right away. Notifications use `notify-send`.

### KDE Plasma 6

```sh
kpackagetool6 --type Plasma/Applet --install kde/rog-mouse-battery
```

Use `--upgrade` instead of `--install` to update it. Then add **ASUS Mouse Battery** to a panel
from Add Widgets. Click it for details, middle-click to read the mouse right away. Plasma
widgets can't stream a command's output, so this one runs `rogctl battery` every refresh
interval instead of `rogctl watch`.

### GNOME

```sh
gnome-extensions pack --force gnome/rog-mouse-battery@humblemonk.github.io
gnome-extensions install --force rog-mouse-battery@humblemonk.github.io.shell-extension.zip
```

Log out and back in, then turn on **ASUS Mouse Battery** in the Extensions app (or run
`gnome-extensions enable rog-mouse-battery@humblemonk.github.io`). Its menu shows the details
and has a Refresh item.

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
