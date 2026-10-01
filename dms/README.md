# DankMaterialShell plugin

Shows your mouse's battery level in the DankMaterialShell bar. Needs DankMaterialShell 1.5 or
newer and [rogctl installed](../README.md#getting-started).

## Install

```sh
rogctl configure dms
```

This copies the plugin to `~/.config/DankMaterialShell/plugins/RogMouseBattery`. Then enable **ASUS Mouse Battery** in Settings → Plugins, then add it to the bar.

## Use

Click the widget for details; right-click it to read the mouse right away. The rogctl command,
refresh interval and low-battery threshold are in the plugin's settings. Low-battery
notifications use `notify-send`.

## Update or remove

After updating rogctl, run `rogctl configure dms` again. To remove it, disable it in
Settings → Plugins, then:

```sh
rogctl configure dms --remove
```
