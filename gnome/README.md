# GNOME extension

Shows your mouse's battery level in the GNOME top bar. Needs GNOME 45 or newer and
[rogctl installed](../README.md#getting-started).

## Install

From your rogctl checkout:

```sh
gnome-extensions pack --force gnome/rog-mouse-battery@humblemonk.github.io
gnome-extensions install --force rog-mouse-battery@humblemonk.github.io.shell-extension.zip
```

Log out and back in, then turn on **ASUS Mouse Battery** in the Extensions app, or run:

```sh
gnome-extensions enable rog-mouse-battery@humblemonk.github.io
```

## Use

Its menu shows the details and has a Refresh item. The rogctl command, refresh interval and
low-battery threshold are in the extension's settings in the Extensions app.

## Update or remove

After a `git pull`, run the two install commands again and log out and back in. To remove it:

```sh
gnome-extensions uninstall rog-mouse-battery@humblemonk.github.io
```
