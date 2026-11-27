# Changelog

What changed in each rogctl release, newest first. Only changes you can see or feel are
listed; internal refactors, test work and CI changes are left out. To check which version you
have, run `rogctl --version`, or look at the end of the footer in `rogctl settings`.

Releases that need you to do something after upgrading have an **Upgrading** note.

## 2.9.0 — 2026-11-26

A bug hunt across rogctl and its widgets: settings changes only ever send what you asked
for, and the widgets keep up with devices coming, going and sleeping.

### Added

- Prebuilt binaries for x86_64 and aarch64 Linux (glibc 2.35 or newer) on each
  [GitHub release](https://github.com/humblemonk/rogctl/releases), so you don't need a Rust
  toolchain outside Arch. Each archive has the udev rule and comes with a SHA-256 checksum
  file; the readme explains how to install it.

### Changed

- The Noctalia widget draws a sleeping device in Noctalia's `outline_variant` colour, which
  replaced the deprecated `outline` that made Noctalia log a warning.
- `rogctl settings set` and `import` take a value only as `get` shows it or `export` writes
  it, in any case and with or without its unit. Before, they ignored spaces, `-` and `_`,
  so `brightness=-50` set 50%, `power-off=1-0` set 10 minutes and `dpi-1=+800` set 800 DPI;
  such values are now refused and nothing is sent.
- When `import` skips a setting that can't be changed now (the power-saving rate while power
  saving is off, the colour while the effect sets its own), it says the file's value wasn't
  restored and names the value the mouse keeps.
- `rogctl watch` notices a mouse or keyboard plugged in or unplugged within 10 seconds,
  whatever `--interval` is, and reads it at once. An ROG Omni receiver that doesn't answer
  just after it's plugged in, or that couldn't be asked what's paired with it, is asked again
  every 10 seconds.
- `rogctl configure gnome --remove` says it needs GNOME Shell running, and how to remove the
  extension without it.
- The Waybar readme explains that a failing `rogctl` hides the module, and how to see why.

### Fixed

- In `rogctl settings`, pasting text no longer acts as key presses. Before, a space or a
  newline in the paste applied and saved a change. Terminals without bracketed paste still
  send a paste as keys.
- `rogctl configure` no longer writes or deletes through a symbolic link. Before,
  `--remove` with the Noctalia plugin directory still a link to a checkout deleted the
  checkout's files. Now a link at a widget's directory or files makes it change nothing and
  exit with 1, `--force` included.
- A battery or settings read no longer takes a stray rejection, drawn by another program's
  save on the same receiver (`rogctl settings`, GearLink), as its answer. The mouse showed
  as asleep for up to 10 seconds, `rogctl battery` exited with 3, and an `import` could stop
  as if the mouse had gone to sleep.
- When the mouse doesn't confirm a change but reads it back changed, `rogctl settings` and
  `settings set`/`import` say it was made but not saved and may not last a power cycle,
  instead of "No change made"; `set` and `import` exit with 0.
- Changing auto power-off sends the low-battery warning back exactly as the mouse had it, and
  the other way round. A warning set by other software outside rogctl's range was rewritten:
  to 100% on the ROG Chakram, Pugio II, Keris Wireless family and Strix Impact II Wireless,
  and to at most 100% on the others. Such a warning now shows as "Unknown (0x..)" rather than
  as a level like 255%, and `get --json`, `export` and the status line leave it out.
- ROG Harpe II Ace and Harpe II Extreme Edition 20: advanced power saving turns on and off
  when the mouse holds a rate outside 125-2000 Hz, and sends that rate back as the mouse has
  it. Before, it did nothing and said the mouse didn't have the setting. A rate rogctl can't
  name shows as "Unknown (0x..)", and `get --json` and `export` leave it out.
- The low-battery notification no longer repeats each time a widget restarts `rogctl watch`
  (GNOME on every screen unlock, any widget after a settings change). rogctl remembers it in
  `~/.local/state/rogctl/low-battery`.
- `rogctl watch` exits as soon as the widget reading it goes away. Before, a restarted widget
  could have an extra `watch` running beside it for up to an interval.
- An ROG Omni receiver rogctl can't open shows its error for a keyboard too. Before,
  `--device keyboard` said no keyboard was found, and the error showed only as a mouse.
  `--device all` lists it once, with the mice; before, it came after the keyboards.
- After a widget restarts `rogctl watch` (GNOME on every screen unlock, any widget after a
  settings change), a sleeping mouse or keyboard shows its last level. Before, it showed none
  until it woke. rogctl keeps it in `~/.local/state/rogctl/last-level`.
- After the computer resumes from suspend, `rogctl watch` reads a device that was due within
  10 seconds. Before, it waited out the rest of `--interval`, showing the level from before
  the suspend.
- rogctl no longer crashes on an argument that isn't valid UTF-8, and `settings export` and
  `import` take any file name. It no longer crashes when its output goes to a program that
  has exited (`rogctl --help | head -1`); `settings get` and `export` then exit with 1.
- `rogctl settings` restores the terminal when it's closed or killed.
- `rogctl settings` ignores keys pressed while it waits on the mouse (a change, Tab, reading
  the mice again). Before, an Enter meant to apply a typed DPI could make that stage the
  active one instead, and a second Space turned a switch back.
- `rogctl settings` keeps auto power-off and the low-battery warning current while open, so
  changing one no longer puts back the other as it was before other software changed it.
- Changing the lighting colour or effect sends a brightness other software set above 100%
  back as it was, instead of 100%. It shows as "Unknown (0x..)", and `export` leaves it out.
- In `rogctl settings`, an ROG Omni receiver that couldn't be asked which mouse it holds
  (just after it's plugged in) is asked again every 10 seconds, instead of showing as
  "ROG Omni receiver" with no settings until `r`.
- `rogctl settings` moves on from a mouse that stops answering but stays plugged in (a cable
  left in when the mouse goes back to its receiver), instead of showing its last battery
  level for good.
- `rogctl configure`:
  - `--remove` counts a file holding this version's contents as rogctl's, as installing
    does, and an install that fails partway records what it wrote.
  - A file you changed that a new version no longer ships stops the update with exit 1, as
    documented; `--force` removes it.
  - `configure dms` installs to `~/.config/dms/plugins`, where DankMaterialShell 2 reads
    plugins (unless only the older `~/.config/DankMaterialShell` is there), and on an update
    says how to load the new version if the plugin is enabled.
  - Running it again with nothing changed says the widget was reinstalled. Before, Noctalia
    said "Updated" and DMS asked you to enable the plugin again.
- Widgets:
  - Quickshell and DankMaterialShell show "rogctl not found on PATH" when rogctl can't be
    started, instead of staying blank or "Waiting for rogctl…"; GNOME uses the same
    wording.
  - Noctalia, GNOME, Quickshell and DankMaterialShell name the `rogctl` that exited in their
    error, not one just entered in their settings.
  - Plasma reads at once when its refresh interval changes, and keeps checking a sleeping
    device every 10 seconds; a sleeping device's red low warning follows a changed
    threshold. A read started before a settings change no longer overwrites the one after it.
  - DankMaterialShell and Noctalia dim a sleeping device that's low as well as showing it
    red, as the others do. DankMaterialShell shows the mouse icon while waiting for rogctl,
    and dragging its interval or threshold slider no longer restarts rogctl over and over.

### Upgrading

- The Noctalia widget needs Noctalia 5.4 or newer (plugin API 43), for the colour role that
  replaced the one it used for sleeping devices. Upgrade Noctalia first if yours is older,
  then run `rogctl configure noctalia` again.
- If your Quickshell `RogMouse*.qml` files are symbolic links (from a dotfiles manager, say),
  `rogctl configure quickshell` now refuses them. Replace them with copies, then run it
  again.
- Scripts that pass `settings set` a value spelled differently from `get` and `export`
  (`colourcycle`, `1_000`) now get an error; use those spellings.

## 2.8.0 — 2026-11-02

Settings from the command line.

### Added

- `rogctl settings get`, `set`, `export` and `import`: read and change the mouse's settings
  without the settings screen, such as `rogctl settings set polling-rate=1000 dpi-2=1600`
  from a script or a keybinding. `export` saves them to a JSON file (all but the active DPI
  stage, which the DPI button changes), and `import` puts them back on the same model,
  changing only what differs, so importing at every login doesn't write to the mouse each
  time. They check each value as the settings screen does, refuse a value the mouse doesn't
  take before changing anything, and check the mouse reads each change back. `--dry-run`
  shows what would change. Keys and values are in the
  [command reference](cli.md#settings-from-the-command-line). Tested on a ROG Harpe II Ace.

### Changed

- In `rogctl settings`, angle tuning steps to multiples of the mouse's step (5° on some mice)
  when another program set a value between them.

### Upgrading

- rogctl may say your widget is out of date, though it hasn't changed apart from its version
  number. Run `rogctl configure` for your desktop again to clear it.

## 2.7.1 — 2026-11-02

### Added

- `rogctl configure` on its own marks each desktop whose widget is installed, and says when
  one is out of date. Waybar is never marked, since rogctl doesn't edit its config.

### Upgrading

- rogctl may say your widget is out of date, though it hasn't changed apart from its version
  number. Run `rogctl configure` for your desktop again to clear it.

## 2.7.0 — 2026-11-01

Keyboard battery levels.

### Added

- **Keyboards**: the battery level of the ROG Azoth, Azoth Extreme, Azoth Extreme SE, Azoth X,
  Falchion, Falchion RX Low Profile, Strix Scope II 96 and Strix Scope II 96 RX, on their own
  receiver, a cable or the ROG Omni receiver. rogctl only reads keyboards; `rogctl settings`
  stays for mice. Tested on a Falchion RX Low Profile.
- `--device mouse|keyboard|all` for `rogctl battery` and `rogctl watch`. With `all`, the JSON
  is one object with a status for each device (`devices`), and `watch` keeps each device's
  last level and low-battery notification separately and reads each on its own schedule, so
  a sleeping keyboard doesn't make it read an awake mouse every 10 seconds. Each status has a
  new `kind` field, `mouse` or `keyboard`.
- A **Show** setting in every widget but Waybar: mouse and keyboard (the default), only the mouse, or
  only the keyboard. With both, the widget shows an icon and level for each device that's
  plugged in, and names the device in its low-battery notification. The Waybar config has a
  module for each.

### Changed

- The widgets are now called **ASUS Peripheral Battery** (was ASUS Mouse Battery). Their
  settings carry over.
- Without `--device`, `rogctl battery` and `rogctl watch` show a keyboard when no mouse is
  plugged in, and say "No supported mouse or keyboard found" when there's neither.
- `rogctl list` shows keyboards, and says whether each device is a mouse or a keyboard.

### Fixed

- The udev rule opens only the interface rogctl reads on each receiver, cable and keyboard.
  Before, it opened all of them, including the keyboard interface that most receivers have,
  so any program you ran could read what you typed on a keyboard paired to the ROG Omni
  receiver, or the keys a mouse sends, without root.

### Upgrading

- Install the udev rule again (step 3 of [Getting started](../README.md#getting-started)),
  then unplug and replug each receiver and cable, or restart, to drop the access the old rule
  gave. Until then a keyboard on its own receiver or cable shows "Permission denied". The AUR
  package installs the new rule itself; replug after upgrading.
- Run `rogctl configure` for your desktop again to update the widget. In Waybar, add
  `--device mouse` to your `custom/rog-mouse` module's `exec`, or it shows the keyboard when
  no mouse is plugged in, and add the `custom/rog-keyboard` module that
  `rogctl configure waybar` prints.

## 2.6.1 — 2026-11-01

### Fixed

- `rogctl settings` checks which mouse is connected before every change. Before, swapping a
  receiver for another with the same USB ID, or pairing a different mouse to the ROG Omni
  receiver in place of the first, while it was open could send the change in the format of
  the mouse shown first. Now it makes no change and reads the mouse again instead.
- The Noctalia widget runs `rogctl` when its "rogctl command" setting is cleared, as the
  other widgets do.
- In `rogctl settings`, a button debounce code outside the 12-32 ms range shows as
  "Unknown" instead of as 12 or 32 ms; ←/→ pick a value as usual.
- In `rogctl settings`, when the selected setting locks (motion sync at 8000 Hz, say), the
  selection moves to the next setting rather than back to the top.

### Changed

- `--interval` only works with `rogctl watch`; with another command it's an error (exit 1)
  instead of being ignored.

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
- `rogctl settings` works in terminals without 24-bit colour: unless `COLORTERM` says the
  terminal has it, it uses the 256 standard colours, chosen so text stays easy to read.
  `NO_COLOR` turns colour off, with the selected row in reverse video.
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
  the mouse" when it was saved, and the screen pauses for about 0.15 s there instead of
  0.5 s. If the mouse really doesn't confirm, the message now says it may not last a power
  cycle, or that the mouse went to sleep or out of range if it did.
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
- **Quickshell:** refresh goes through the rogctl that's running, even after the rogctl
  command setting changed.
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
