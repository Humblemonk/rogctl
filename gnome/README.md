# GNOME extension

Shows your ASUS mouse's and keyboard's battery levels in the GNOME top bar. Needs GNOME 45 or newer and
[rogctl installed](../README.md#getting-started).

## Install

```sh
rogctl configure gnome
```

This packs and installs it with `gnome-extensions`.

Log out and back in, then turn on **ASUS Peripheral Battery** in the Extensions app, or run:

```sh
gnome-extensions enable rog-mouse-battery@humblemonk.github.io
```

## Use

Its menu shows the details and has a Refresh item. Which devices to show (mouse and keyboard,
the default, or only one), the rogctl command, refresh interval, low-battery threshold,
percentage and hiding while nothing is connected are in the extension's settings in the
Extensions app. A device that isn't plugged in is left out.

## Update or remove

After updating rogctl, run `rogctl configure gnome` again, then log out and back in. To
remove it:

```sh
rogctl configure gnome --remove
```
