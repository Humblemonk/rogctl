# rogctl command reference

For scripts, status bars and widgets of your own. The [readme](../README.md) covers
installing and everyday use.

## Commands

```sh
rogctl                        # "<mouse name>: 80%", plus "(charging)" when charging
rogctl --json                 # one JSON status line
rogctl watch --json           # a status line every 60 s (--interval SECS)
rogctl watch --format waybar  # the same, as Waybar custom-module JSON
rogctl list                   # detected devices and their /dev/hidraw nodes
rogctl settings               # view and change the mouse's settings
rogctl settings --demo        # the settings screen without a mouse
rogctl configure DESKTOP      # install a panel widget (--remove removes it)
```

`rogctl` on its own is short for `rogctl battery`. `rogctl settings --demo "ROG Chakram"`
shows the demo as another model from the [Supported mice](../README.md#supported-mice).

## Options

| Option | Does |
| --- | --- |
| `--format FORMAT` | `text` (the default), `json` or `waybar` |
| `--json` | Same as `--format json` |
| `--low-threshold PCT` | Level at or below which the battery counts as low (default 20, 0 turns it off) |
| `--interval SECS` | `watch` only: seconds between reads (default 60) |

## Installing widgets

`rogctl configure DESKTOP` installs the panel widget for `noctalia`, `waybar`, `quickshell`,
`dms`, `kde` or `gnome`; without `DESKTOP` it lists them. The widget files are built into
rogctl, so this needs no checkout, and running it again after updating rogctl updates the
widget. `--remove` removes the widget.

| Desktop | Install | Remove |
| --- | --- | --- |
| `noctalia` | Copies the plugin to `$XDG_DATA_HOME/noctalia/plugins/rog-mouse-battery`, then enables it with `noctalia msg` | Disables it, then deletes the directory |
| `waybar` | Prints the module, its CSS and the steps; doesn't edit the Waybar config | Prints the steps |
| `quickshell` | Copies two files next to `$XDG_CONFIG_HOME/quickshell/shell.qml` | Deletes them |
| `dms` | Copies the plugin to `$XDG_CONFIG_HOME/DankMaterialShell/plugins/RogMouseBattery`, then rescans with `dms ipc` | Deletes the directory, then rescans |
| `kde` | `kpackagetool6 --install`, or `--upgrade` if installed | `kpackagetool6 --remove` |
| `gnome` | `gnome-extensions pack` and `install` | `gnome-extensions uninstall` |

`$XDG_DATA_HOME` defaults to `~/.local/share` and `$XDG_CONFIG_HOME` to `~/.config`. `--force`
replaces a link where a plugin directory goes, and overwrites or removes Quickshell files
that differ from rogctl's copies; without it, rogctl leaves them alone and exits with 1.
`rogctl configure` exits with 0 on success and 1 on an error.

## Exit codes

`rogctl battery` exits with:

| Code | Means |
| --- | --- |
| 0 | Connected |
| 1 | Error |
| 2 | No device |
| 3 | Mouse asleep or out of range |

## JSON status

Each status is one line of JSON, for example:

```json
{"state":"connected","device":"ROG Harpe II Ace","wireless":true,"battery":53,"charging":false,"low_battery_warning":20,"power_off_minutes":3,"hidraw":"/dev/hidraw8","battery_updated":1790809563}
```

Fields that don't apply are left out rather than set to `null`.

| Field | What |
| --- | --- |
| `state` | `connected`, `asleep`, `disconnected` or `error` |
| `device` | The mouse's name |
| `wireless` | `true` through a receiver, `false` on a cable |
| `battery` | Level in percent |
| `charging` | Whether it's charging |
| `low` | At or below `--low-threshold` and not charging |
| `low_battery_warning` | The mouse's own low-battery warning level, in percent |
| `power_off_minutes` | The mouse's auto power-off time |
| `hidraw` | The `/dev/hidraw` node it was read through |
| `battery_updated` | Unix time when `battery` was last read from the mouse |
| `error` | What went wrong, when `state` is `error` |
| `notify_low` | `watch` only: `true` on the one line where a low-battery notification is due |
| `pid` | `watch` only: its process ID |

The mouse stops answering while it sleeps. `watch` then keeps reporting the last level read,
with `"state":"asleep"`.

`notify_low` is set once when the battery drops to the threshold. It's set again only after
the mouse has charged or risen 5% above the threshold, so a notification on it won't repeat.

To read the mouse right away instead of waiting for the next interval, send `watch` the
`SIGUSR1` signal: `kill -USR1 PID`, with the `pid` from its output.

## Waybar format

`--format waybar` prints Waybar's custom-module JSON: `text`, `tooltip`, `percentage`, `alt`
(the state, or `charging`, for `format-icons`) and `class` (the state plus `charging` and
`low`). The `text` is empty while no mouse is plugged in, which hides the module. See
[waybar/](../waybar/README.md).
