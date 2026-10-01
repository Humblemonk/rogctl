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
        alt="rogctl settings showing a ROG Harpe II Ace's DPI stages, polling rate and battery settings">
    </td>
  </tr>
  <tr>
    <td align="center">Panel widget</td>
    <td align="center"><code>rogctl settings</code></td>
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

5. **Optional: add a panel widget.** See [Panel widgets](#panel-widgets).

## Changing settings

```sh
rogctl settings
```

This shows the mouse's settings and lets you change them: DPI stages, polling rate, button
debounce, angle snapping, motion sync, lift-off distance, auto power-off, low-battery warning
and, on ROG Harpe II models, advanced power saving. It only shows the settings your mouse has.

| Key | Does |
| --- | --- |
| ↑ ↓ | Select a setting |
| ← → | Change the value (Shift for bigger steps), or type a DPI |
| Enter | Apply the change |
| Esc | Cancel the change |
| ? | All keys |
| q | Quit |

You can also click and scroll. Nothing is sent until you press Enter (or click a selected
setting). Applied changes are saved on the mouse, so they stay after unplugging it and apply
on other computers too. Changing the lift-off distance also resets the sensor calibration.

Want to look first? `rogctl settings --demo` shows the screen without a mouse and changes
nothing.

## Panel widgets

Each widget shows the mouse's battery level in your panel or bar. It turns red when the
battery is low (20% by default), dims while the mouse sleeps and hides while no mouse is
plugged in. All but Waybar also send a notification when the battery gets low.

Install the one for your desktop with `rogctl configure`, such as:

```sh
rogctl configure kde
```

| Desktop | Command | Details |
| --- | --- | --- |
| Noctalia | `rogctl configure noctalia` | [noctalia/](noctalia/README.md) |
| Waybar (Hyprland, Sway, niri, …) | `rogctl configure waybar` | [waybar/](waybar/README.md) |
| Quickshell config of your own | `rogctl configure quickshell` | [quickshell/](quickshell/README.md) |
| DankMaterialShell | `rogctl configure dms` | [dms/](dms/README.md) |
| KDE Plasma 6 | `rogctl configure kde` | [kde/](kde/README.md) |
| GNOME 45 or newer | `rogctl configure gnome` | [gnome/](gnome/README.md) |

It prints the step left to do by hand, such as adding the widget to your panel. Run it again
after updating rogctl to update the widget, or add `--remove` to remove it. Waybar's config is
yours, so for Waybar it prints the module to add rather than editing the config.

If a widget says rogctl wasn't found, your desktop's `PATH` probably doesn't include
`~/.cargo/bin`. Set the widget's **rogctl command** setting to the full path, such as
`/home/you/.cargo/bin/rogctl`.

## Troubleshooting

- **`No supported mouse found`**: the receiver or cable isn't plugged in, or the mouse isn't
  in [Supported mice](#supported-mice). `rogctl list` shows what was detected.
- **`Permission denied`**: the udev rule from step 3 isn't active yet. Unplug and replug the
  receiver or cable, or log out and back in.
- **`asleep or out of range`**: the mouse is off, asleep or too far from the receiver. Move it
  to wake it and try again.
- **Odd colours in `rogctl settings`**: it needs a terminal with 24-bit colour, which most
  current ones have.

Something else wrong, or a setting that reads or changes wrong?
[Report it](CONTRIBUTING.md#reporting-a-problem).

## Supported mice

Only the ROG Harpe II Ace (on the SpeedNova 8K receiver) has been tested so far. The others
should work; please [report](CONTRIBUTING.md#reporting-a-problem) whether yours does. Mice not
listed here aren't detected.

<details>
<summary>All 27 mice</summary>

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

</details>

## Scripting

`rogctl --json` and `rogctl watch --json` print the status as JSON for your own scripts and
bars. See [docs/cli.md](docs/cli.md) for every command, option, field and exit code.

## License

Everything in this repository is licensed under the GNU Affero General Public License v3.0 or
later (`AGPL-3.0-or-later`). See [LICENSE](LICENSE).
