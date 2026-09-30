---
paths:
  - "noctalia/**"
  - "waybar/**"
  - "quickshell/**"
  - "dms/**"
  - "kde/**"
  - "gnome/**"
  - "src/main.rs"
  - "src/waybar.rs"
---

# Panel widgets

Each widget is a thin frontend over `rogctl`'s JSON status line.

## The JSON contract

- The status line is the contract between Rust and every widget. `None` fields are omitted
  (not `null`); widgets treat a missing field as `nil`/`undefined`. If you add, rename or
  remove a field, update every widget in the same change (grep for it under
  `noctalia/ quickshell/ dms/ kde/ gnome/`) and `src/waybar.rs`.
- Shared logic lives in `rogctl`, not the frontends. `watch` keeps the last level while the
  mouse sleeps, sets `low` (and `notify_low` on the one line a notification is due: once per
  drop, re-armed by charging or rising 5% above the threshold), reports its `pid`, and
  re-reads the mouse on SIGUSR1.
- Widgets pass `--low-threshold`, restart `rogctl` when the binary, interval or threshold
  setting changes, and send SIGUSR1 to refresh. Noctalia has no settings-changed event and
  can't stop a stream, so its service polls the settings in `update()` and stops `rogctl` by
  its `pid`; the exit marker restarts it. The Plasma widget polls one-shot `rogctl battery`,
  so it keeps its own sleeping-level and notified-once state.

## Keep them alike

- The same settings (binary, interval, low threshold, show percent, hide when disconnected)
  with the same defaults, Solaar's wording ("Battery: 79% (discharging)"), red when `low`,
  dimmed while asleep, and restarting `rogctl` 10 s after it exits.
- Luau files start with `--!nonstrict`. User-visible strings go through `noctalia.tr()` with
  keys in `translations/en.json`; settings need `label_key` entries there too.
- The plugin declares `plugin_api = 24` (for `runAsync` with an argv array). Don't raise it
  without checking the user's installed Noctalia supports the new level.

## Checking without the desktops

`/usr/lib/qt6/bin/qmllint` for QML syntax (import warnings are expected without Quickshell,
DMS or Plasma; `/usr/bin/qmllint` is Qt 5 and can't parse `?.` or `??`),
`node --check --input-type=module < file.js` for the GNOME JS, and
`glib-compile-schemas --strict --dry-run gnome/*/schemas`.

Hardware-verified end-to-end: Noctalia only. The Waybar output is unit-tested and was run
against a real mouse; Quickshell, DMS, Plasma and GNOME are checked for syntax only. Say so
when a change depends on their runtime behaviour.

## Per widget

- **Noctalia**: with the user's permission, `~/.local/share/noctalia/plugins/rog-mouse-battery`
  is a symlink to this repo's plugin directory, so `.luau` edits hot-reload. `plugin.toml`
  changes need `noctalia msg config-reload`. Logs: `~/.cache/noctalia/noctalia.log`. Docs:
  <https://docs.noctalia.dev/noctalia/plugins/development/>.
- **Waybar**: `waybar/config.json` runs `rogctl watch --format waybar`. Waybar hides a custom
  module whose `text` is empty, which is how "disconnected" hides it.
- **Quickshell**: no plugin system; users copy the two `.qml` files into their config.
  Reference: <https://quickshell.org/docs/>.
- **DankMaterialShell**: plugin docs are in the DMS repository under
  `.agents/skills/dms-plugin-dev/` and `quickshell/PLUGINS/`
  (<https://github.com/AvengeMedia/DankMaterialShell>). The daemon is the only surface that
  runs `rogctl`; widgets read the `status` global var and ask for a re-read by setting
  `refresh`. Test with `dms ipc plugin-scan reload rogMouseBattery`.
- **KDE Plasma 6**: `plasmoidviewer -a kde/rog-mouse-battery` (from `plasma-sdk`) runs it
  without a panel. Notifications use the generic `plasma_workspace`/`notification` event,
  since a QML-only package can't install a `.notifyrc`.
- **GNOME**: every main-loop source and subprocess must be removed in `disable()`, or they
  leak each time the screen locks. The review guidelines
  (<https://gjs.guide/extensions/review-guidelines/review-guidelines.html>) are a good
  checklist. Test nested: `dbus-run-session gnome-shell --devkit` (GNOME 49+) or
  `--nested --wayland` (older).
