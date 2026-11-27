# DankMaterialShell plugin

Shows your ASUS mouse's and keyboard's battery levels in the DankMaterialShell bar. Needs DankMaterialShell 1.5 or
newer and [rogctl installed](../README.md#getting-started).

## Install

```sh
rogctl configure dms
```

This copies the plugin to `~/.config/dms/plugins/RogMouseBattery`, or to
`~/.config/DankMaterialShell/plugins/RogMouseBattery` when `~/.config/DankMaterialShell` is a
directory (or a link to one) and there's no `~/.config/dms`, as before DankMaterialShell 2.0,
and has DankMaterialShell rescan its plugins. Then enable **ASUS Peripheral Battery** in Settings → Plugins
and add it to the bar.

## Use

Click the widget for details; right-click it to read the devices right away. Which devices to
show (mouse and keyboard, the default, or only one), the rogctl command, refresh interval,
low-battery threshold, percentage and hiding while nothing is connected are in the plugin's
settings. A device that isn't plugged in is left out. Low-battery notifications use `notify-send`.

## Update or remove

After updating rogctl, run `rogctl configure dms` again. If the plugin is enabled, load the
new version with `dms ipc plugin-scan reload rogMouseBattery` (or restart DankMaterialShell);
reloading a disabled plugin starts it. To remove it,
disable it in
Settings → Plugins, then:

```sh
rogctl configure dms --remove
```
