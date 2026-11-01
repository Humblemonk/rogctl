# Quickshell widget

For a Quickshell config of your own. Needs [rogctl installed](../README.md#getting-started),
and a Nerd Font for the example widget's icons.

## Install

```sh
rogctl configure quickshell
```

This copies `RogMouse.qml`, `RogMouseSettings.qml` and `RogMouseWidget.qml` next to
`~/.config/quickshell/shell.qml`. It won't overwrite or remove copies you've changed unless you
add `--force`; it can tell them from an older version's copies, which it updates. Once you've
changed `RogMouseSettings.qml`, it never overwrites it, even with `--force`. If your config is
elsewhere, copy the three files next to its `shell.qml` yourself.

Then put `RogMouseWidget {}` in your bar.

## Customize

The settings (which devices to show, rogctl command, refresh interval, low-battery threshold)
are in `RogMouseSettings.qml`; updates keep your changes to it. `device` is `"all"` (the
default), `"mouse"` or `"keyboard"`; a settings file from before 2.7.0 has none, which means
all. Low-battery notifications use
`notify-send`.

Before 2.6.0 the settings were at the top of `RogMouse.qml`. If you changed them there, note
them first: updating needs `--force` once, which overwrites `RogMouse.qml`. Then set them again
in `RogMouseSettings.qml`.

`RogMouseWidget.qml` is a small example to restyle: an icon and level for each device. To build
your own, read these from the `RogMouse` singleton, which runs one rogctl however many bars use
it:

| Property | What |
| --- | --- |
| `devices` | A [status](../docs/cli.md#json-status) for each device, mice first; empty while none is plugged in |
| `error` | Why rogctl couldn't run, or `""` |
| `battery` | The first device's level in percent, -1 while unknown |
| `charging` | Whether the first device is charging |
| `low` | Whether the first device is at or below the threshold and not charging |
| `connected` | Whether the first device answered the last read |
| `device` | The first device's name |
| `status` | The first device's status |

Its `lowBattery(device)` signal passes the status of the device that ran low.

## Remove

Take `RogMouseWidget {}` out of your bar, then:

```sh
rogctl configure quickshell --remove
```

It leaves `RogMouseSettings.qml` if you changed it; delete it yourself if you're done with it.
