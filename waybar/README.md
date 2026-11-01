# Waybar module

Shows your ASUS mouse's and keyboard's battery levels in Waybar, one module each. Needs [rogctl installed](../README.md#getting-started)
and a Nerd Font for the icons.

## Install

`rogctl configure waybar` prints the module, the CSS and these steps:

1. Copy the `custom/rog-mouse` and `custom/rog-keyboard` modules from
   [`config.json`](config.json) into your Waybar config. Leave out the one you don't need.
2. Add `"custom/rog-mouse"` and `"custom/rog-keyboard"` to one of your `modules-left`,
   `modules-center` or `modules-right` lists.
3. Append [`style.css`](style.css) to your Waybar `style.css`.
4. Reload Waybar: `pkill -SIGUSR2 waybar`.

Change `--interval` (seconds between reads) and `--low-threshold` (percent) in each `exec`
line. `--device` picks the device a module shows. If Waybar can't find rogctl, put its full path there, such as
`/home/you/.cargo/bin/rogctl`.

## Styling

Each module has one CSS class for its state, `connected`, `asleep`, `error` or `disconnected`,
plus `charging` and `low`. `format-icons` is keyed by the state, or `charging` while the
device charges. A module hides itself while its device isn't plugged in.

Waybar can't send notifications, so these modules only turn red when the battery is low.

## Remove

Take `"custom/rog-mouse"` and `"custom/rog-keyboard"` out of your modules list, delete the
modules and their `#custom-rog-mouse` and `#custom-rog-keyboard` rules from `style.css`, then
reload Waybar.
