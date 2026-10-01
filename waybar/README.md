# Waybar module

Shows your mouse's battery level in Waybar. Needs [rogctl installed](../README.md#getting-started)
and a Nerd Font for the icons.

## Install

`rogctl configure waybar` prints the module, the CSS and these steps:

1. Copy the `custom/rog-mouse` module from [`config.json`](config.json) into your Waybar
   config.
2. Add `"custom/rog-mouse"` to one of your `modules-left`, `modules-center` or
   `modules-right` lists.
3. Append [`style.css`](style.css) to your Waybar `style.css`.
4. Reload Waybar: `pkill -SIGUSR2 waybar`.

Change `--interval` (seconds between reads) and `--low-threshold` (percent) in the `exec`
line. If Waybar can't find rogctl, put its full path there, such as
`/home/you/.cargo/bin/rogctl`.

## Styling

The module has one CSS class for its state, `connected`, `asleep`, `error` or `disconnected`,
plus `charging` and `low`. `format-icons` uses the same names. It hides itself while no mouse
is plugged in.

Waybar can't send notifications, so this module only turns red when the battery is low.

## Remove

Take `"custom/rog-mouse"` out of your modules list, delete the module and the
`#custom-rog-mouse` rules from `style.css`, then reload Waybar.
