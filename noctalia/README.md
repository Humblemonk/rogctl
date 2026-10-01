# Noctalia widget

Shows your mouse's battery level in the Noctalia bar. Needs Noctalia 5.1 or newer and
[rogctl installed](../README.md#getting-started).

## Install

```sh
rogctl configure noctalia
```

This copies the plugin to `~/.local/share/noctalia/plugins/rog-mouse-battery` and enables it.
Then add **ASUS Mouse Battery** from the bar's Add-widget picker, or in your config:

```toml
[widget.mouse-battery]
type = "humblemonk/rog-mouse-battery:battery"
```

## Use

Click the widget to read the mouse right away. The refresh interval, low-battery threshold,
icon and rogctl command are in the plugin and widget settings.

## Update or remove

After updating rogctl, run `rogctl configure noctalia` again. To remove it:

```sh
rogctl configure noctalia --remove
```

## Developing it

To try edits from your rogctl checkout as you make them, link the plugin directory instead:

```sh
ln -s "$PWD/noctalia/rog-mouse-battery" ~/.local/share/noctalia/plugins/rog-mouse-battery
noctalia msg plugins enable humblemonk/rog-mouse-battery
```

`rogctl configure noctalia` won't replace the link unless you add `--force`, and
`rogctl configure noctalia --remove` deletes only the link.
