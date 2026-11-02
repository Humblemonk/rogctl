# KDE Plasma widget

Shows your ASUS mouse's and keyboard's battery levels in a Plasma 6 panel. Needs
[rogctl installed](../README.md#getting-started).

## Install

```sh
rogctl configure kde
```

This installs it with `kpackagetool6`. Then add **ASUS Peripheral Battery** to a panel from Add
Widgets.

## Use

Click the widget for details; middle-click it to read the devices right away. Which devices to
show (mouse and keyboard, the default, or only one), the rogctl command, refresh interval,
low-battery threshold, percentage and hiding while nothing is connected are in its settings. A
device that isn't plugged in is left out.

## Update or remove

After updating rogctl, run `rogctl configure kde` again, then log out and back in. To remove
it:

```sh
rogctl configure kde --remove
```
