# Noctalia widget

Shows your ASUS mouse's and keyboard's battery levels in the Noctalia bar. Needs Noctalia 5.4 or newer and
[rogctl installed](../README.md#getting-started).

## Install

```sh
rogctl configure noctalia
```

This copies the plugin to `~/.local/share/noctalia/plugins/rog-mouse-battery` and enables it.
Then add **ASUS Peripheral Battery** from the bar's Add-widget picker, or in your config:

```toml
[widget.mouse-battery]
type = "humblemonk/rog-mouse-battery:battery"
```

## Use

Click the widget to read the devices right away. Which devices to show (mouse and keyboard,
the default, or only one), the rogctl command, refresh interval and low-battery threshold are in
the plugin's settings; the icon, percentage and hiding while nothing is connected are in the
widget's. A device that isn't plugged in is left out.

## Update or remove

After updating rogctl, run `rogctl configure noctalia` again. If
`~/.local/share/noctalia/plugins/rog-mouse-battery` is still a symbolic link to a checkout,
from an old setup, rogctl leaves it alone: delete the link first. To remove it:

```sh
rogctl configure noctalia --remove
```
