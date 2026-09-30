# Noctalia widget

Shows your mouse's battery level in the Noctalia bar. Needs Noctalia 5.1 or newer and
[rogctl installed](../README.md#getting-started).

## Install

From your rogctl checkout:

```sh
ln -s "$PWD/noctalia/rog-mouse-battery" ~/.local/share/noctalia/plugins/rog-mouse-battery
noctalia msg plugins enable humblemonk/rog-mouse-battery
```

Then add **ASUS Mouse Battery** from the bar's Add-widget picker, or in your config:

```toml
[widget.mouse-battery]
type = "humblemonk/rog-mouse-battery:battery"
```

## Use

Click the widget to read the mouse right away. The refresh interval, low-battery threshold,
icon and rogctl command are in the plugin and widget settings.

Because the plugin is a link to your checkout, `git pull` updates it.

## Remove

```sh
noctalia msg plugins disable humblemonk/rog-mouse-battery
rm ~/.local/share/noctalia/plugins/rog-mouse-battery
```
