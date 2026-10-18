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

Click the widget to read the mouse right away. The rogctl command, refresh interval and
low-battery threshold are in the plugin's settings; the icon, percentage and hiding while no
mouse is connected are in the widget's.

## Update or remove

After updating rogctl, run `rogctl configure noctalia` again. To remove it:

```sh
rogctl configure noctalia --remove
```
