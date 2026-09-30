# KDE Plasma widget

Shows your mouse's battery level in a Plasma 6 panel. Needs
[rogctl installed](../README.md#getting-started).

## Install

From your rogctl checkout:

```sh
kpackagetool6 --type Plasma/Applet --install kde/rog-mouse-battery
```

Then add **ASUS Mouse Battery** to a panel from Add Widgets.

## Use

Click the widget for details; middle-click it to read the mouse right away. The rogctl
command, refresh interval and low-battery threshold are in its settings.

## Update or remove

After a `git pull`, update it with `--upgrade` in place of `--install`. To remove it:

```sh
kpackagetool6 --type Plasma/Applet --remove com.github.humblemonk.rogmousebattery
```
