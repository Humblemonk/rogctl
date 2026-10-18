---
paths:
  - "src/app.rs"
  - "src/ui.rs"
  - "src/tui.rs"
---

# TUI

## Behaviour

- Nothing is written until Enter, Space or a click on the focused row. ←/→ and typed digits
  only change a pending `Edit`; Esc, moving focus, `r` or Tab drop it (Esc the status
  message too). Every change is also saved to the mouse's flash, so never write on each key press.
- The mouse goes through the same paths: `ui::draw()` returns `Hits` (each selectable row,
  and each value on its scale; `None` when only the too-small notice was drawn),
  `App::handle_mouse()` maps a click on the focused row to
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
  found, and retries a sleeping one, or one that failed, every 10 s. While connected it
  reads the battery (`12 07`) for the header and the active DPI stage (`12 00`), which the
  mouse's DPI button changes without telling rogctl, every 10 s, without dropping a pending
  edit; a mouse that went to sleep shows as asleep.
- A save answered `FF AA` with no echo within 150 ms is asked for once more
  (`Connection::apply()`); rejected again, the message says the mouse didn't confirm saving
  it and that it may not last a power cycle, since the Harpe II Ace keeps such changes but
  other mice may not (`protocol.md`). If the read-back that follows finds the mouse asleep,
  the message says it went to sleep or out of range before confirming the save instead.
- `ratatui::try_init()` installs a panic hook that restores the terminal (release builds
  abort on panic); always pair it with `ratatui::restore()`, after `release_mouse()` (the
  TUI's own hook does that too). Once the terminal is up, nothing prints: messages go to the
  status line through `App::ok()` and `App::error()`.

## Same UI for every mouse

A user owns one mouse but reads one readme and one help screen, so a setting that exists on
several models looks and behaves the same on all of them.

- **One row order:** DPI stages → polling rate → button debounce → angle snapping → angle
  tuning → motion sync → lift-off distance → auto power-off → low-battery warning →
  advanced power saving → power-saving rate → lighting effect → colour → brightness, in
  three panels (Performance, Battery, Lighting; `Control::panel()`) that share the line
  between them. Models leave out rows; never reorder them.
- **Same labels, units, formatting and keys.** Labels come from `Control::label()`, values
  from `Value`'s `Display` (a scale's `ui::tick()` drops the unit, shown once at its end).
  No key that only one model responds to. The UI uses rogctl's names, not a vendor's.
- **Hide or lock:** a setting the mouse doesn't have is hidden. One it has but can't change
  now is shown dimmed with 🔒 and the reason (`Row::locked`: motion sync at 8000 Hz, the
  power-saving rate while power saving is off, the lighting colour while the effect sets its
  own, the colour and brightness while the effect is one rogctl doesn't send) and can't be
  focused. Never leave a row visible and focusable but inert.
- **Arrows:** the focused row's text value has ◀ and ▶ only for the ways ←/→ can still go
  (`App::can_step()`), grey before an edit and red during one; none on scales or while DPI
  digits are typed. The ◀ sits in the label's last two columns and DPI values keep a slot
  for the ▶, so values and the stages' swatches stay in their columns.
- **Unknown values show:** a code rogctl can't decode reads "Unknown (0x..)" (`PowerOff`,
  `LiftOff::Unknown`), ←/→ still pick a valid value, and it's never sent back. Whether a row
  shows depends on the model, not the reply, with one exception: lighting, untested on most
  mice that have it, is left out if its read fails (`Connection::read_settings()`), so the
  other settings still show.
- **Scales:** settings with a short fixed list (`App::choices()`: polling rates, debounce,
  power-off, warning, brightness) draw as a GearLink-style slider, red up to a ● on the
  value. ←/→ step through exactly that list (`arrows_walk_exactly_the_choices`). Two-way
  settings, DPI, angle tuning, the lighting effect (too wide as a scale) and colour
  (GearLink's 7 standard colours, or the mouse's own as `#rrggbb`), locked rows and values
  outside the list stay text. No sidebar or pages: every model fits one screen.
- **Size:** every screen of every model fits 80x24 (`ui::MIN_SIZE`, btop's minimum): all
  rows, a pending edit's hint, the header and a message, which wraps at spaces across the
  terminal (breaking a word wider than it) onto up to three rows, fewer if the panels need
  them, ending in "…" if it's longer (`wrap_words()`). The tallest model (Harpe II Ace) uses
  22 of the 24 rows, 23 with rogctl's longest (two-row) message, and the widest rows with
  their hint (polling rate, brightness) 77 of a panel's 78 columns. Below 80x24, or if the
  panels don't fit beside a one-row message, `draw()` shows only btop's "Terminal size too
  small" notice with the size needed and returns `None`: keys only quit
  (`App::handle_key_too_small()`: q, Ctrl+C) and clicks are ignored, so nothing changes out
  of sight. Each event acts on the screen as last drawn, the one the user saw; a click that
  comes with a resize is dropped. Only the footer may drop keys at 80 columns; a demo header
  too long for 80 drops "Battery:". `every_screen_fits_the_min_size` checks every model,
  focus, edit, message and notice.
- **Footer keys** are only the ones that do something on the screen shown: with no mouse
  connected, just `r`, `?`, `q` (and Tab with several mice); with help open, just Esc. When
  they don't fit, the version goes first, then the least needed keys (`footer_line()`); at
  80 columns that's only Esc, when Tab shows. Outside help, `?` and `q` are never dropped.
- **Colours** follow GearLink's ROG theme: near-black, white text, grey detail, one red
  accent (the header's `///`, focus band, active stage, pending edit, switches that are on,
  errors, the header's battery at or below the mouse's low-battery warning). Nothing else is
  red: the ✓ of a success and `[demo]` are white. They're named once in the palette at the
  top of `ui.rs`; use its styles (`BASE`, `MUTED`, `BOLD`, `ACCENT`, `FOCUSED`). Text needs
  4.5:1 contrast on the background and the focus band (`text_has_enough_contrast` checks
  every model and state), which is why `RED` is GearLink's #e60012 lightened. The DPI and
  lighting swatches, read from the mouse, are the only other colours. The footer ends with
  the version when it fits.
- **Colour modes** (`ColorMode::detect()`): everything is drawn in 24-bit colour, then
  `Downsample`, drawn last, converts it. `COLORTERM` of `truecolor` or `24bit` keeps 24-bit;
  otherwise each palette colour becomes its stand-in in `INDEXED` (picked to keep 4.5:1, which
  `text_has_enough_contrast` checks too) and a swatch the nearest of the 256. `NO_COLOR`
  drops every colour and shows the focus band in reverse video, so no state may be told by
  colour alone: bold, underline and the marks (▌, ◀ ▶, ●, ✓ ✗, 🔒) carry it.

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
5. `ui.rs`: usually nothing; rows draw through `row_line()`. If applying it does more than
   the value shows (lift-off also resets the sensor calibration), say so in the pending
   edit's hint there.
6. README.md: the settings list, and the key table if a main key changes (the full list is
   the `?` help). Say which mice have it.
