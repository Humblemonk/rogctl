# DankMaterialShell plugin

Shows your ASUS mouse's and keyboard's battery levels in the DankMaterialShell bar. Needs DankMaterialShell 1.5 or
newer and [rogctl installed](../README.md#getting-started).

## Install

```sh
rogctl configure dms
```

This copies the plugin to `~/.config/DankMaterialShell/plugins/RogMouseBattery` and has
DankMaterialShell rescan its plugins. Then enable **ASUS Peripheral Battery** in Settings → Plugins
and add it to the bar.

## Use

Click the widget for details; right-click it to read the devices right away. Which devices to
show (mouse and keyboard, the default, or only one), the rogctl command, refresh interval,
low-battery threshold, percentage and hiding while nothing is connected are in the plugin's
settings. A device that isn't plugged in is left out. Low-battery notifications use `notify-send`.

## Update or remove

After updating rogctl, run `rogctl configure dms` again. To remove it, disable it in
Settings → Plugins, then:

```sh
rogctl configure dms --remove
```
