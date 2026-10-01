# Quickshell widget

For a Quickshell config of your own. Needs [rogctl installed](../README.md#getting-started),
and a Nerd Font for the example widget's icons.

## Install

```sh
rogctl configure quickshell
```

This copies `RogMouse.qml` and `RogMouseWidget.qml` next to `~/.config/quickshell/shell.qml`.
It won't overwrite or remove copies you've changed unless you add `--force`; it can tell
them from an older version's copies, which it updates. If your config is
elsewhere, copy the two files next to its `shell.qml` yourself.

Then put `RogMouseWidget {}` in your bar.

## Customize

The settings (rogctl command, refresh interval, low-battery threshold) are at the top of
`RogMouse.qml`. Low-battery notifications use `notify-send`.

`RogMouseWidget.qml` is a small example to restyle. To build your own, read these from the
`RogMouse` singleton, which runs one rogctl however many bars use it:

| Property | What |
| --- | --- |
| `battery` | Level in percent, -1 while unknown |
| `charging` | Whether it's charging |
| `low` | At or below the threshold and not charging |
| `connected` | Whether the mouse answered the last read |
| `device` | The mouse's name |
| `status` | The whole [status line](../docs/cli.md#json-status) |

## Remove

Take `RogMouseWidget {}` out of your bar, then:

```sh
rogctl configure quickshell --remove
```
