# KDE Plasma widget

Shows your mouse's battery level in a Plasma 6 panel. Needs
[rogctl installed](../README.md#getting-started).

## Install

```sh
rogctl configure kde
```

This installs it with `kpackagetool6`.

Then add **ASUS Mouse Battery** to a panel from Add Widgets.

## Use

Click the widget for details; middle-click it to read the mouse right away. The rogctl
command, refresh interval and low-battery threshold are in its settings.

## Update or remove

After updating rogctl, run `rogctl configure kde` again, then log out and back in. To remove
it:

```sh
rogctl configure kde --remove
```
