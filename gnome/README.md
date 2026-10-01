# GNOME extension

Shows your mouse's battery level in the GNOME top bar. Needs GNOME 45 or newer and
[rogctl installed](../README.md#getting-started).

## Install

```sh
rogctl configure gnome
```

This packs and installs it with `gnome-extensions`.

Log out and back in, then turn on **ASUS Mouse Battery** in the Extensions app, or run:

```sh
gnome-extensions enable rog-mouse-battery@humblemonk.github.io
```

## Use

Its menu shows the details and has a Refresh item. The rogctl command, refresh interval and
low-battery threshold are in the extension's settings in the Extensions app.

## Update or remove

After updating rogctl, run `rogctl configure gnome` again, then log out and back in. To
remove it:

```sh
rogctl configure gnome --remove
```
