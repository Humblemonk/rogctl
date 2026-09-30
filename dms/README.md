# DankMaterialShell plugin

Shows your mouse's battery level in the DankMaterialShell bar. Needs DankMaterialShell 1.5 or
newer and [rogctl installed](../README.md#getting-started).

## Install

From your rogctl checkout:

```sh
ln -s "$PWD/dms/RogMouseBattery" ~/.config/DankMaterialShell/plugins/RogMouseBattery
dms ipc plugin-scan scan
```

Enable **ASUS Mouse Battery** in Settings → Plugins, then add it to the bar.

## Use

Click the widget for details; right-click it to read the mouse right away. The rogctl command,
refresh interval and low-battery threshold are in the plugin's settings. Low-battery
notifications use `notify-send`.

Because the plugin is a link to your checkout, `git pull` updates it.

## Remove

Disable it in Settings → Plugins, then delete the link:

```sh
rm ~/.config/DankMaterialShell/plugins/RogMouseBattery
```
