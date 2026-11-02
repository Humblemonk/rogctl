# rogctl command reference

For scripts, status bars and widgets of your own. The [readme](../README.md) covers
installing and everyday use.

## Commands

```sh
rogctl                        # "<device name>: 80%", plus "(charging)" when charging
rogctl --device all           # a line like that for every mouse and keyboard
rogctl --json                 # one JSON status line
rogctl watch --json           # a status line every 60 s (--interval SECS)
rogctl watch --format waybar  # the same, as Waybar custom-module JSON
rogctl list                   # detected mice and keyboards and their /dev/hidraw nodes
rogctl refresh PID            # make that `rogctl watch` read the devices now
rogctl stop PID               # stop that `rogctl watch`
rogctl settings               # view and change the mouse's settings
rogctl settings --demo        # the settings screen without a mouse
rogctl settings get           # the mouse's settings (--json for JSON)
rogctl settings set KEY=VALUE # change settings, such as polling-rate=1000
rogctl settings export [FILE] # save the settings to a file (stdout without one)
rogctl settings import FILE   # change the settings to a file's (- for stdin)
rogctl configure DESKTOP      # install a panel widget (--remove removes it)
```

`rogctl` on its own is short for `rogctl battery`. `rogctl settings --demo "ROG Chakram"`
shows the demo as another model from the [Supported mice](../README.md#supported-mice).

## Options

| Option | Does |
| --- | --- |
| `--device DEVICE` | `battery` and `watch`: `mouse`, `keyboard` or `all`. Without it, the mouse, or a keyboard if no mouse is plugged in |
| `--format FORMAT` | `text` (the default), `json` or `waybar` |
| `--json` | Same as `--format json` |
| `--low-threshold PCT` | Level at or below which the battery counts as low (default 20, 0 turns it off) |
| `--interval SECS` | `watch` only: seconds between reads while the device is awake, 1 to 86400 (default 60) |

## Settings from the command line

`rogctl settings get`, `set`, `export` and `import` read and change the same settings as the
settings screen, of the same mouse: the first one that answers. Keyboards are never changed.

```sh
rogctl settings get
rogctl settings set polling-rate=1000 dpi-2=1600
rogctl settings set motion-sync=off --dry-run
rogctl settings export ~/harpe.json
rogctl settings import ~/harpe.json
```

`get` lists each setting the mouse has by the name `set` takes, with its value as the
settings screen shows it, and says when one is locked (as the settings screen dims it). `set`
takes one or more `KEY=VALUE`. Values are numbers or names, and units are optional:
`polling-rate=1000` and `polling-rate=1000hz` are the same.

| Key | Values |
| --- | --- |
| `dpi-1`, `dpi-2`, … | DPI, within the mouse's range, rounded to its step |
| `active-stage` | The DPI stage in use, from 1, on mice where rogctl can pick it |
| `polling-rate`, `power-saving-rate` | Hz: the rates the mouse offers |
| `debounce` | ms, 12 to 32 |
| `angle-snapping`, `motion-sync`, `power-saving` | `on` or `off` |
| `angle-tuning` | Degrees, such as `-5`, in the mouse's range and step |
| `lift-off` | `low` or `high`. Changing it also resets the sensor calibration |
| `power-off` | Minutes (`1`, `2`, `3`, `5`, `10`) or `never` |
| `low-battery-warning` | Percent, in the mouse's steps; `0` or `off` turns it off |
| `effect` | `static`, `breathing`, `colour-cycle`, `reactive` or `battery-level` |
| `colour` | `red`, `orange`, `yellow`, `green`, `cyan`, `blue`, `magenta` or `#rrggbb` |
| `brightness` | Percent, in 10% steps; `0` is off |

Each change is applied in the order of the settings screen, whatever order you give them
in, so `power-saving=on` comes before the `power-saving-rate` it unlocks. A setting locked
when its turn comes is refused, as is a value the mouse doesn't take, and a setting that
already has its value is left alone. Every change is checked before the first is sent, so a
refused one means nothing was changed. `--dry-run` shows what would change and sends nothing.

After each change rogctl reads the mouse again and plans the rest from that, so a setting an
earlier change altered on the mouse is set back too. Only then can a setting be refused after
others were changed: `set` stops there, with 1, and says how many changes it made, and
`import` skips a locked one with a note.

`export` writes every setting `get` shows, as JSON, and `get --json` prints the same on one
line:

```json
{
  "version": 1,
  "model": "ROG Harpe II Ace",
  "settings": {
    "dpi-1": 400,
    "polling-rate": 1000,
    "motion-sync": true,
    "effect": "static",
    "colour": "red"
  }
}
```

Numbers are numbers, switches `true` or `false`, and the rest the names `set` takes. A value
rogctl can't name, such as an unknown code another program set, is left out.

`import` changes each setting in the file that differs from the mouse, so importing a file
the mouse already matches sends nothing. It only takes a file made from the same model, and
refuses one with a setting the mouse doesn't have or a value it doesn't take before changing
anything. It skips, with a note, a setting that can't be changed now: a locked one, or the
Rainbow, Comet or Off effect, which rogctl shows but doesn't send.

As with the settings screen, each change is saved on the mouse. If the mouse doesn't confirm
the save, rogctl says so and carries on: the change was made, but may not last a power
cycle.

`--demo [MODEL]` runs any of them without a mouse, changing nothing.

## Installing widgets

`rogctl configure DESKTOP` installs the panel widget for `noctalia`, `waybar`, `quickshell`,
`dms`, `kde` or `gnome`; without `DESKTOP` it lists them, marking each one whose widget is
`installed` or `out of date`. Waybar is never marked, since rogctl doesn't edit its config.
The widget files are built into rogctl, so this needs no checkout, and running it again after
updating rogctl updates the widget. `--remove` removes the widget.

| Desktop | Install | Remove |
| --- | --- | --- |
| `noctalia` | Copies the plugin to `$XDG_DATA_HOME/noctalia/plugins/rog-mouse-battery`, then enables it with `noctalia msg` (and reloads Noctalia's config on an update) | Disables it, then deletes its files |
| `waybar` | Prints the module, its CSS and the steps; doesn't edit the Waybar config | Prints the steps |
| `quickshell` | Copies three files next to `$XDG_CONFIG_HOME/quickshell/shell.qml` | Deletes them, keeping `RogMouseSettings.qml` if you changed it |
| `dms` | Copies the plugin to `$XDG_CONFIG_HOME/DankMaterialShell/plugins/RogMouseBattery`, then rescans with `dms ipc` | Deletes its files, then rescans |
| `kde` | `kpackagetool6 --install`, or `--upgrade` if installed | `kpackagetool6 --remove` |
| `gnome` | `gnome-extensions pack` and `install` | `gnome-extensions uninstall` |

`$XDG_DATA_HOME` defaults to `~/.local/share` and `$XDG_CONFIG_HOME` to `~/.config`.

For Noctalia, DMS and Quickshell, rogctl records each file it writes, with a checksum, in
`$XDG_STATE_HOME/rogctl/DESKTOP.files` (`~/.local/state` by default). Updating and removing
touch only those files, and only while they're unchanged. An update deletes the files the
new version no longer ships, and removing deletes a directory only once it's empty. Files you
add are never touched.

If you changed one of rogctl's files, rogctl leaves it alone and exits with 1. `--force`
overwrites or removes it anyway. `rogctl configure` exits with 0 on success and 1 on an
error.

After you update rogctl, `rogctl battery` and `rogctl watch` check whether an installed widget
is older than the one this rogctl ships: its files differ, and it came from an earlier rogctl
version. A widget from a newer rogctl, or from a build of this version, doesn't count. If one
is older, they send a desktop notification with `notify-send` naming the
`rogctl configure DESKTOP` command that updates it, once per rogctl version (they keep the
version they last checked with in `$XDG_STATE_HOME/rogctl/notified`). For Noctalia, DMS and
Quickshell this goes by the record, which also holds the version, so your own edits don't
count; for KDE and GNOME, by the installed files and the version in their `metadata.json`.
`rogctl configure` with no desktop names the out-of-date ones too.

## Exit codes

`rogctl battery` exits with:

| Code | Means |
| --- | --- |
| 0 | Connected |
| 1 | Error |
| 2 | No device |
| 3 | Asleep or out of range |

With `--device all`, the code is the best of the devices': 0 if any is connected, then 3 if
any is asleep, then 1 on an error, otherwise 2.

`rogctl list` exits with 0 when it finds a mouse or keyboard and 2 when it finds none.

`rogctl settings get`, `set`, `export` and `import` exit with the same codes as `battery`
for the mouse: 0 done, 1 an error (a refused setting or value included), 2 no mouse, 3 asleep
or out of range. A change whose save the mouse didn't confirm still exits with 0. If `set` or
`import` stops partway, it says how many changes it made. It stops when a change fails, when
the mouse goes to sleep (3) or can't be read back (1) with a change still to make, and when
the mouse leaves a setting still to change out of what it reads back (1), as can happen to
the lighting on a mouse whose lighting doesn't read. If the mouse goes to sleep or can't be
read back after the last change it had to make, rogctl says it couldn't check the other
settings again, and exits with 0.

## JSON status

Each status is one line of JSON, for example:

```json
{"state":"connected","device":"ROG Harpe II Ace","kind":"mouse","wireless":true,"battery":53,"charging":false,"low_battery_warning":20,"power_off_minutes":3,"hidraw":"/dev/hidraw8","battery_updated":1790809563}
```

Fields that don't apply are left out rather than set to `null`.

| Field | What |
| --- | --- |
| `state` | `connected`, `asleep`, `disconnected` or `error` |
| `device` | The mouse's or keyboard's name |
| `kind` | `mouse` or `keyboard` |
| `wireless` | `true` through a receiver, `false` on a cable |
| `battery` | Level in percent |
| `charging` | Whether it's charging |
| `low` | At or below `--low-threshold` and not charging. Mice that report the level in quarters count as low at 25% when the threshold is lower |
| `low_battery_warning` | The mouse's own low-battery warning level, in percent (mice only) |
| `power_off_minutes` | The mouse's auto power-off time (mice only) |
| `hidraw` | The `/dev/hidraw` node it was read through |
| `battery_updated` | Unix time when `battery` was last read from the device |
| `error` | What went wrong, when `state` is `error` |
| `notify_low` | `watch` only: `true` on the one line where a low-battery notification is due |
| `pid` | `watch` only: its process ID |

A device stops answering while it sleeps. `watch` then keeps reporting the last level read,
with `"state":"asleep"`, and reads it every 10 seconds whatever `--interval` is (or every
`--interval` if that's shorter), so one that wakes shows up within 10 seconds. After an
error it tries again in 5 seconds, then waits twice as long each time, up to `--interval`.

### Every device

`--device all` prints one object instead, with a status for each device, mice first, and
`watch`'s `pid` once:

```json
{"devices":[{"state":"connected","device":"ROG Harpe II Ace","kind":"mouse",…},{"state":"asleep","device":"ROG Falchion RX Low Profile","kind":"keyboard",…}],"pid":4242}
```

`devices` is empty while nothing is plugged in. A receiver and a cable for the same model count
as one device, as without `--device all`. `watch` keeps each device's last level while it
sleeps and decides each one's `notify_low` separately, and reads each on its own schedule: a
sleeping keyboard is read every 10 seconds without reading an awake mouse that often. Each line
has every device, the ones not read that time with their last status (and no `notify_low`).

`notify_low` is set once when the battery drops to the threshold. It's set again only after
the device has charged or risen 5% above the threshold, so a notification on it won't repeat.

To read the devices right away instead of waiting for the next interval, run
`rogctl refresh PID` with the `pid` from its output; `rogctl stop PID` stops it. Both signal
the process only if it's a `rogctl watch` (`refresh` sends `SIGUSR1`, `stop` `SIGTERM`) and
exit 1 otherwise. Run them through the same command as `watch`: under a wrapper, such as a
container's, the `pid` is only right there. `kill -USR1 PID` works too where the `pid` is.

## Waybar format

`--format waybar` prints Waybar's custom-module JSON: `text`, `tooltip`, `percentage`, `alt`
(the state, or `charging`, for `format-icons`) and `class` (the state plus `charging` and
`low`). The `text` is empty while nothing is plugged in, which hides the module. A module shows
one device, so it doesn't take `--device all`: use a module with `--device mouse` and one with
`--device keyboard`. See
[waybar/](../waybar/README.md).
