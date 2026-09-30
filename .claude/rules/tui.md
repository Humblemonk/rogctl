---
paths:
  - "src/app.rs"
  - "src/ui.rs"
  - "src/tui.rs"
---

# TUI

## Behaviour

- Nothing is written until Enter, Space or a click on the focused row. ←/→ and typed digits
  only change a pending `Edit`; Esc, moving focus or `r` drop it. Every change is also saved
  to the mouse's flash, so never write on each key press.
- The mouse goes through the same paths: `ui::draw()` returns `Hits` (each selectable row,
  and each value on its scale), `App::handle_mouse()` maps a click on the focused row to
  `confirm()` (Enter) and everything else to focus or a pending `Edit`: a first click
  selects, the wheel steps like ←/→, a scale value sets the edit. Locked rows get no hit.
  Only clicks and the wheel are captured (`EnableClickCapture`), so pointer moves don't
  redraw.
- After every change `apply_action()` reads the settings back, so the screen shows what the
  mouse has, not what was asked for.
- `--demo` runs with no device: `App::simulate()` stands in for the write and read-back.
  Demo mode must stay fully navigable for every model in `MODELS`.
- `Session::tick()` notices a mouse plugged in or out every 2 s by rescanning sysfs
  (`device::candidate_nodes()`, no HID traffic) and reading the mice again only when that
  changes. It then shows what was just plugged in (the cable, so the header says wired) if
  it answers, else the mouse it was showing. It looks for a mouse every 2 s while none is
  found, and retries a sleeping one every 5 s. While connected it reads the battery (`12 07`
  only) every 10 s for the header, without dropping a pending edit; a mouse that went to
  sleep shows as asleep.
- `ratatui::try_init()` installs a panic hook that restores the terminal (release builds
  abort on panic); always pair it with `ratatui::restore()`, after `release_mouse()` (the
  TUI's own hook does that too). Once the terminal is up, nothing prints: messages go to the
  status line through `App::ok()` and `App::error()`.

## Same UI for every mouse

A user owns one mouse but reads one readme and one help screen, so a setting that exists on
several models looks and behaves the same on all of them.

- **One row order:** DPI stages → polling rate → button debounce → angle snapping → angle
  tuning → motion sync → lift-off distance → auto power-off → low-battery warning →
  advanced power saving → power-saving rate. Models leave out rows; never reorder them.
- **Same labels, units, formatting and keys.** Labels come from `Control::label()`, values
  from `Value`'s `Display` (a scale's `ui::tick()` drops the unit, shown once at its end).
  No key that only one model responds to. The UI uses rogctl's names, not a vendor's.
- **Hide or lock:** a setting the mouse doesn't have is hidden. One it has but can't change
  now is shown dimmed with 🔒 and the reason (`Row::locked`: motion sync at 8000 Hz, the
  power-saving rate while power saving is off) and can't be focused. Never leave a row
  visible and focusable but inert.
- **Unknown values show:** a code rogctl can't decode reads "Unknown (0x..)" (`PowerOff`,
  `LiftOff::Unknown`), ←/→ still pick a valid value, and it's never sent back. Whether a row
  shows depends on the model, not the reply.
- **Scales:** settings with a short fixed list (`App::choices()`: polling rates, debounce,
  power-off, warning) draw as a GearLink-style slider, red up to a ● on the value. ←/→ step
  through exactly that list (`arrows_walk_exactly_the_choices`). Two-way settings, DPI, angle
  tuning, locked rows and values outside the list stay text. No sidebar or pages while
  everything fits one screen.
- **Colours** follow GearLink's ROG theme: near-black, white text, grey detail, one red
  accent (focus band, active stage, pending edit, switches that are on, errors, the header's
  battery at or below the mouse's low-battery warning). They're named once in the palette at
  the top of `ui.rs`; use its styles (`BASE`, `MUTED`, `BOLD`, `ACCENT`, `FOCUSED`). The DPI
  swatches, read from the mouse, are the only other colours. The footer ends with the version.

## Adding a setting

1. `protocol.rs`: named constants, a `Features` field, a `Query` (or a field in an existing
   reply) and `parse_settings()` branch, a `Change` variant and its `change_request()` arm.
   Clamp to the model's range; never panic on a bad value.
2. `protocol.rs` tests: parse a hand-made or captured reply; check the exact request bytes.
3. `models.rs`: fill the field for every G-Helper class that has it; extend
   `features_are_consistent()` if it has a range.
4. `app.rs`: a `Control` (in its place in the row order) and `Value` variant, then `label()`,
   `value()`, `rows()`, `step()`, `change_for()`, `choices()` if it's a short list, plus
   `simulate()` and `demo_state()`. The compiler lists the matches to update.
5. `ui.rs`: usually nothing; rows draw through `row_line()`.
6. README.md: the settings list, and the key table if a main key changes (the full list is
   the `?` help). Say which mice have it.
