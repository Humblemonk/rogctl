# Changelog

What changed in each rogctl release, newest first. Only changes you can see or feel are
listed; internal refactors, test work and CI changes are left out. To check which version you
have, run `rogctl --version`, or look at the end of the footer in `rogctl settings`.

Releases that need you to do something after upgrading have an **Upgrading** note.

## 2.6.0 — 2026-10-18

Lighting, and safer widgets.

### Added

- **Lighting** in `rogctl settings`: effect (Static, Breathing, Colour cycle, Reactive,
  Battery level), colour (seven standard colours) and brightness (Off to 100% in 10% steps).
  On mice with more than one lit zone (logo, scroll wheel, underglow) it sets them all
  together. Available on the ROG Harpe II Ace, Harpe II Extreme Edition 20, Harpe Ace Aim Lab
  Edition, Harpe Ace Extreme, Harpe Ace Mini, Keris II Ace, Keris II Origin (and KJP), Keris
  Wireless Aimpoint, Gladius III Aimpoint, Gladius III Eva 2, Gladius III Wireless, Chakram X,
  Spatha X, Strix Impact III Wireless and TUF Gaming Mini Miku Edition. Only the Harpe II Ace
  has been tested; please open an issue to say how it works on yours.
- `rogctl refresh PID` and `rogctl stop PID` ask a running `rogctl watch` to read the mouse
  now, or to stop. They refuse any process that isn't a `rogctl watch`.
- `rogctl settings` follows the mouse's DPI button: the active stage updates within 10
  seconds.

### Changed

- The selected setting shows ◀ and ▶ only for the directions ←/→ can still go, in grey,
  turning red once you've changed the value.
- The settings panels share their dividing lines, so every mouse, lighting included, still
  fits an 80×24 terminal.
- The widgets refresh and restart rogctl with `rogctl refresh` and `rogctl stop` instead of
  signalling the process ID `watch` prints. Under a container wrapper (distrobox, toolbox,
  `flatpak-spawn`) that ID could belong to an unrelated program on the host.
- **Quickshell:** the settings moved from the top of `RogMouse.qml` to
  `RogMouseSettings.qml`, which updates never overwrite, so editing them no longer blocks
  `rogctl configure quickshell`.
- For an Omni receiver whose mouse rogctl can't identify, the JSON output no longer includes
  `low_battery_warning` and `power_off_minutes`, which were read by guesswork, and
  `rogctl settings` offers no changes.

### Fixed

- **Harpe II Ace:** the first change after the mouse sat unused no longer says "not saved on
  the mouse" when it was saved. If the mouse really doesn't confirm, the message now says it
  may not last a power cycle.
- A change no longer reports "No change made" when another program using the mouse (a
  widget, ASUS GearLink) drew a stray error just before the mouse confirmed it.
- Mice that report the battery in 25% steps (Chakram, Gladius II Wireless, Keris Wireless and
  EVA Edition, Pugio II, Strix Carry, Strix Impact II Wireless) now count as low at 25% when
  the low-battery threshold is below 25%, so the widgets turn red and notify. Before, their
  next level down read as asleep and they never warned.
- Changing auto power-off no longer lowers a low-battery warning that another program set
  above rogctl's range. ← brings such a warning into range; → leaves it alone.
- A key or click that arrives as the terminal is resized can no longer change a setting on a
  screen you didn't see. A message too long for the space left ends in "…".
- **Waybar:** the module now hides when no mouse is found, as the readme says.
- **GNOME:** changing the settings no longer leaves extra copies of rogctl running. Spinning
  the interval restarts rogctl once, the command field applies on Apply rather than on every
  keystroke, and the charging icon shows at 95–99%.
- **Plasma:** a sleeping mouse keeps showing its last level after a failed read.
- **Noctalia:** if rogctl can't be restarted with new settings, the error is logged once
  instead of every 2 seconds.

### Upgrading

- Update your widget with `rogctl configure DESKTOP`; rogctl sends a notification naming the
  command when an installed widget is out of date.
- **Quickshell:** if you changed the settings at the top of `RogMouse.qml`, note them first.
  Updating needs `rogctl configure quickshell --force` once, which overwrites that file. Then
  set them again in `RogMouseSettings.qml`.
- **Waybar:** add `"hide-empty-text": true` to your `custom/rog-mouse` module, as in
  [waybar/config.json](../waybar/config.json).

## 2.5.0 — 2026-10-17

### Changed

- `rogctl settings` needs a terminal of at least 80×24. In a smaller one it shows the size it
  needs, and only `q` and `Ctrl+C` work, so nothing can change out of sight.
- Long messages wrap onto up to three rows instead of being cut off at 80 columns.

## 2.4.3 — 2026-10-14

A mouse that wakes up shows up within about 10 seconds.

### Fixed

- The widgets no longer stay dimmed for up to a whole refresh interval after the mouse wakes.
  A sleeping mouse is now checked every 10 seconds, including by the Plasma widget.
- `rogctl settings` notices a mouse plugged in or out while the shown mouse sleeps, retries
  a mouse that failed every 10 seconds instead of waiting for `r`, and no longer jumps
  straight back after Tab moves to a sleeping mouse.

### Upgrading

- Update your widget with `rogctl configure DESKTOP`.

## 2.4.1 — 2026-10-13

### Changed

- `rogctl configure` highlights out-of-date widgets in bold yellow, with the command that
  updates them.

## 2.4.0 — 2026-10-13

### Added

- When an installed widget is older than rogctl, `rogctl battery` and `rogctl watch` send one
  desktop notification per rogctl version naming the `rogctl configure` command that updates
  it. `rogctl configure` marks out-of-date widgets in its list too.

### Changed

- **Noctalia:** the plugin is always a copy installed by `rogctl configure`; a plugin folder
  linked to a source checkout is no longer supported.

### Upgrading

- **Noctalia:** if you linked the plugin folder to a checkout, delete the link
  (`rm ~/.local/share/noctalia/plugins/rog-mouse-battery`, no trailing slash), then run
  `rogctl configure noctalia`.

## 2.3.1 — 2026-10-13

### Changed

- Updated dependencies.

## 2.3.0 — 2026-10-13

Polish for `rogctl settings`.

### Changed

- Red text is brighter, so every label reads clearly on the dark background.
- Typing a DPI shows the mouse's range straight away, and a pending lift-off change warns that
  it also resets the sensor calibration.
- DPI values line up with their colour swatches, and the footer shows only the keys that work
  on the screen shown.

### Fixed

- The help box no longer cuts off its longest line.
- A terminal too short for the settings shows a notice instead of cutting them off.
- Shift+h and Shift+l step in big jumps, as the help says.
- Esc clears a message along with the pending change, and messages clear when the mouse is
  plugged in, unplugged or goes to sleep.

## 2.2.1 — 2026-10-11

### Added

- rogctl is in the AUR: `paru -S rogctl` or `yay -S rogctl`, udev rule included.

### Fixed

- Resizing the terminal while `rogctl settings` was reading the mouse could show an awake
  mouse as asleep for a few seconds, or report a change as not made when it was.
- **Noctalia and DMS:** refresh can no longer reach a rogctl that is being restarted.

## 2.2.0 — 2026-10-01

### Added

- `rogctl configure DESKTOP` installs the battery widget for Noctalia, Waybar, Quickshell,
  DMS, KDE Plasma or GNOME, and `--remove` removes it. The widgets are built into rogctl, so
  no source checkout is needed, and running it again after upgrading updates the widget.

## 2.1.0 — 2026-09-30

### Added

- `rogctl settings` notices mice plugged in or out and switches to the one just plugged in,
  such as the cable in wired mode. It shows charging when the cable goes in.
- The battery level in the settings header turns red at the mouse's own low-battery warning.
- [docs/cli.md](cli.md) describes every command, option, exit code and JSON field.

### Changed

- `rogctl tui` is now `rogctl settings`; the old name still works.
- "charging" instead of "recharging", everywhere.

### Fixed

- A mouse charging at 0% is no longer shown as asleep.
- A change that was made but not saved on the mouse is reported as such, not as "No change
  made".
- A huge `--interval` no longer crashes `watch`; it accepts 1 to 86400 seconds.

## 2.0.0 — 2026-09-30

### Added

- `rogctl tui` (renamed `rogctl settings` in 2.1.0) views and changes the mouse's settings:
  DPI stages and the active stage, polling rate, button debounce, angle snapping and tuning,
  motion sync, lift-off distance, auto power-off, low-battery warning and advanced power
  saving. Nothing is sent until you press Enter. `--demo [MODEL]` tries it without a mouse.
- Older mice also report `low_battery_warning` and `power_off_minutes` in the JSON output.

## 1.3.1 — 2026-09-29

First release.

- Reads the battery level of 27 battery-powered ROG, TUF and ASUS mice over their own
  receiver, a USB cable, the ROG SpeedNova 8K receiver or the ROG Omni receiver, with no
  driver or background service.
- `rogctl battery`, `rogctl watch` (a JSON line per reading, with low-battery and
  notify-once fields) and `rogctl list`.
- Panel widgets for Noctalia, Waybar, Quickshell, DankMaterialShell, KDE Plasma and GNOME:
  the level, dimmed while the mouse sleeps, red at a threshold, with one notification per
  drop below it.
- A udev rule for access without root.
