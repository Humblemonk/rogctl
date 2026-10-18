//! Draws the TUI from `App`. Every model goes through the same code: rows a
//! model lacks are left out, never drawn differently. The colours follow ASUS
//! GearLink's ROG theme: near-black, white text, grey detail, one red accent.

use ratatui::Frame;
use std::ops::Range;

use ratatui::buffer::Buffer;
use ratatui::layout::{Constraint, Layout, Position, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::symbols::{border, line};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, BorderType, Padding, Paragraph, Widget, Wrap};

use crate::app::{App, Control, Edit, Link, Panel, Row, Target, Value};

/// Settings stay readable on a wide terminal; a full polling-rate scale and
/// its hint fit an 80-column one.
const MAX_WIDTH: u16 = 80;
const LABEL_WIDTH: usize = 24;
/// The widest DPI values, five digits (42000, 65000).
const DPI_WIDTH: usize = 5;

/// The smallest terminal, as (width, height), that shows every setting of
/// every model, a pending edit's hint and a message: the VT100's 80x24, still
/// the default terminal window and btop's minimum.
/// `every_screen_fits_the_min_size` checks it.
pub const MIN_SIZE: (u16, u16) = (80, 24);

/// The ROG palette. Only this block names colours; everything else uses the
/// styles below.
/// GearLink's red (#e60012) lightened so red text reads at 4.5:1 on the
/// background and on the focus band.
const RED: Color = Color::Rgb(0xff, 0x2d, 0x3d);
const RED_BAND: Color = Color::Rgb(0x3d, 0x0a, 0x10);
const BACKGROUND: Color = Color::Rgb(0x10, 0x10, 0x10);
const TEXT: Color = Color::Rgb(0xf2, 0xf2, 0xf2);
const GREY: Color = Color::Rgb(0x8c, 0x8c, 0x8c);
const LINE: Color = Color::Rgb(0x3a, 0x3a, 0x3a);

/// The palette's stand-ins among the xterm 256 colours, for terminals
/// without 24-bit colour. Picked so text still reads at 4.5:1, not as the
/// nearest: the nearest red is pinker and fails on the nearest band.
const INDEXED: [(Color, u8); 6] = [
    (RED, 203),
    (RED_BAND, 52),
    (BACKGROUND, 233),
    (TEXT, 255),
    (GREY, 247),
    (LINE, 237),
];

const BASE: Style = Style::new().fg(TEXT).bg(BACKGROUND);
const MUTED: Style = Style::new().fg(GREY);
const BOLD: Style = Style::new().fg(TEXT).add_modifier(Modifier::BOLD);
const ACCENT: Style = Style::new().fg(RED).add_modifier(Modifier::BOLD);
/// The focused row: a dark red band, as GearLink marks the selected item.
const FOCUSED: Style = Style::new().bg(RED_BAND);

/// The colours the terminal shows. Everything is drawn in 24-bit colour, then
/// converted for the others (`Downsample`), so the palette stays in one place.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ColorMode {
    /// `COLORTERM` says the terminal has 24-bit colour.
    TrueColor,
    /// The xterm 256 colours, which nearly every terminal has.
    Indexed,
    /// `NO_COLOR` is set: the terminal's own colours, with the focus band in
    /// reverse video. Bold, underline and the marks (▌, ◀ ▶, ●, ✓ ✗, 🔒)
    /// still tell every state apart.
    None,
}

impl ColorMode {
    pub fn detect() -> Self {
        Self::from_env(
            crate::no_color(),
            std::env::var("COLORTERM").ok().as_deref(),
        )
    }

    fn from_env(no_color: bool, colorterm: Option<&str>) -> Self {
        match (no_color, colorterm) {
            (true, _) => Self::None,
            (false, Some("truecolor" | "24bit")) => Self::TrueColor,
            (false, _) => Self::Indexed,
        }
    }
}

/// Converts what was drawn to `ColorMode`'s colours. Drawn last, over the
/// whole screen.
struct Downsample(ColorMode);

impl Widget for Downsample {
    fn render(self, area: Rect, buf: &mut Buffer) {
        for y in area.top()..area.bottom() {
            for x in area.left()..area.right() {
                let Some(cell) = buf.cell_mut((x, y)) else {
                    continue;
                };
                match self.0 {
                    ColorMode::TrueColor => return,
                    ColorMode::Indexed => {
                        cell.fg = indexed(cell.fg);
                        cell.bg = indexed(cell.bg);
                    }
                    ColorMode::None => {
                        if cell.bg == RED_BAND {
                            cell.modifier |= Modifier::REVERSED;
                        }
                        cell.fg = Color::Reset;
                        cell.bg = Color::Reset;
                    }
                }
            }
        }
    }
}

/// A colour as one of the 256: the palette's stand-in, or the nearest for
/// the mouse's own colours (the swatches).
fn indexed(color: Color) -> Color {
    let Color::Rgb(r, g, b) = color else {
        return color;
    };
    let index = INDEXED
        .iter()
        .find(|&&(palette, _)| palette == color)
        .map_or_else(|| nearest_indexed([r, g, b]), |&(_, index)| index);
    Color::Indexed(index)
}

/// The levels of the xterm colour cube (16-231) on each axis.
const CUBE: [u8; 6] = [0x00, 0x5f, 0x87, 0xaf, 0xd7, 0xff];

/// What colours 16-255 are in xterm; terminals set 0-15 themselves, so
/// those aren't used.
fn xterm_rgb(index: u8) -> [u8; 3] {
    let i = usize::from(index.saturating_sub(16));
    match index {
        16..=231 => [CUBE[i / 36], CUBE[i / 6 % 6], CUBE[i % 6]],
        232..=255 => [8 + 10 * (index - 232); 3],
        0..=15 => [0; 3],
    }
}

/// The nearest of colours 16-255: the closest cube colour or grey.
fn nearest_indexed(rgb: [u8; 3]) -> u8 {
    let level = |c: u8| {
        (0u8..6)
            .min_by_key(|&i| CUBE[usize::from(i)].abs_diff(c))
            .unwrap_or(0)
    };
    let [r, g, b] = rgb.map(level);
    let cube = 16 + 36 * r + 6 * g + b;
    let average = rgb.iter().map(|&c| u16::from(c)).sum::<u16>() / 3;
    let grey = 232 + u8::try_from((average.saturating_sub(3) / 10).min(23)).unwrap_or(23);
    let distance = |index: u8| -> u32 {
        xterm_rgb(index)
            .iter()
            .zip(rgb)
            .map(|(&a, b)| u32::from(a.abs_diff(b)).pow(2))
            .sum()
    };
    if distance(grey) < distance(cube) {
        grey
    } else {
        cube
    }
}

/// Where each clickable thing was drawn, for the mouse.
#[derive(Debug, Default)]
pub struct Hits(Vec<(Rect, Target)>);

impl Hits {
    /// Later regions win: a scale's values over their row.
    pub fn target_at(&self, column: u16, row: u16) -> Option<Target> {
        self.0
            .iter()
            .rev()
            .find(|(area, _)| area.contains(Position::new(column, row)))
            .map(|&(_, target)| target)
    }

    fn add(&mut self, area: Rect, target: Target) {
        if !area.is_empty() {
            self.0.push((area, target));
        }
    }
}

/// Draws the screen and returns where its rows and scale values are, or
/// `None` when the terminal is too small and only a notice is drawn: then keys
/// only quit and clicks are ignored, so nothing changes out of sight.
pub fn draw(frame: &mut Frame, app: &App, colors: ColorMode) -> Option<Hits> {
    frame.render_widget(Block::new().style(BASE), frame.area());
    let (min_width, min_height) = MIN_SIZE;
    let hits = if frame.area().width < min_width || frame.area().height < min_height {
        draw_too_small(frame, MIN_SIZE);
        None
    } else {
        draw_screen(frame, app)
    };
    frame.render_widget(Downsample(colors), frame.area());
    hits
}

/// `draw()` on a terminal of at least `MIN_SIZE`.
fn draw_screen(frame: &mut Frame, app: &App) -> Option<Hits> {
    let area = frame.area();
    let frame_width = area.width;
    let panels = match app.link {
        Link::Connected => Some(panels(app)),
        Link::Asleep | Link::Disconnected | Link::Error(_) => None,
    };
    let needed = panels.as_deref().map_or(0, panels_height);
    // The message gets the rows the header, panels and footer leave, at
    // least one and at most MESSAGE_ROWS, so a long one shortens before the
    // settings give way.
    let message_room = usize::from(area.height.saturating_sub(needed + 2)).clamp(1, MESSAGE_ROWS);
    // Red is for errors; ✓ and ✗ tell them apart without colour too. A long
    // error ("…, but not saved on the mouse: …") wraps across the terminal's
    // width, each line indented by one column.
    let message_lines: Vec<Line> = app.message.as_ref().map_or_else(Vec::new, |m| {
        let (mark, style) = if m.error {
            ("✗", ACCENT)
        } else {
            ("✓", BOLD)
        };
        let text = format!("{mark} {}", m.text);
        wrap_words(&text, usize::from(frame_width) - 1, message_room)
            .into_iter()
            .map(|line| Line::styled(format!(" {line}"), style))
            .collect()
    });
    let message_rows = message_lines.len().max(1) as u16;
    let area = Rect {
        width: area.width.min(MAX_WIDTH),
        ..area
    };
    let [header, body, message, footer] = Layout::vertical([
        Constraint::Length(1),
        Constraint::Min(0),
        Constraint::Length(message_rows),
        Constraint::Length(1),
    ])
    .areas(area);
    // Cut-off panels could hide the focused row, and a change to it. MIN_SIZE
    // fits every model (`every_screen_fits_the_min_size`); this catches what
    // that test doesn't.
    if needed > body.height {
        draw_too_small(frame, (MIN_SIZE.0, area.height - body.height + needed));
        return None;
    }

    // The header, message and footer can outgrow MAX_WIDTH (a long name or
    // message, Tab, the version); they may use the terminal's whole width.
    let full_width = |r: Rect| Rect {
        width: frame_width,
        ..r
    };
    let mut hits = Hits::default();
    frame.render_widget(
        Paragraph::new(header_line(app, frame_width)),
        full_width(header),
    );
    match panels {
        Some(panels) => draw_settings(frame, app, panels, body, &mut hits),
        None => draw_link_notice(frame, app, body),
    }
    frame.render_widget(Paragraph::new(message_lines), full_width(message));
    frame.render_widget(
        Paragraph::new(footer_line(app, frame_width)),
        full_width(footer),
    );
    if app.help {
        draw_help(frame, area);
    }
    Some(hits)
}

/// rogctl's longest message takes two rows, which the tallest model leaves
/// at 80x24; a longer one gets a third row where there's room.
const MESSAGE_ROWS: usize = 3;

/// A bordered section with GearLink's thin grey lines and a white heading.
/// One `below` another shares the line between them, so its top corners
/// join that line to the sides.
fn panel(title: &str, below: bool) -> Block<'static> {
    let block = Block::bordered()
        .border_type(BorderType::Plain)
        .border_style(Style::new().fg(LINE))
        .title(Span::styled(format!(" {title} "), BOLD));
    if below {
        block.border_set(border::Set {
            top_left: line::NORMAL.vertical_right,
            top_right: line::NORMAL.vertical_left,
            ..border::PLAIN
        })
    } else {
        block
    }
}

/// The mouse, its link and battery, and `[demo]`. A header too long for
/// `width` (a long name in the demo) drops the word "Battery:".
fn header_line(app: &App, width: u16) -> Line<'static> {
    let line = header_spans(app, "Battery: ");
    if line.width() <= usize::from(width) {
        line
    } else {
        header_spans(app, "")
    }
}

fn header_spans(app: &App, battery_label: &str) -> Line<'static> {
    // GearLink puts red stripes before the mouse's name.
    let mut spans = vec![Span::styled(" /// ", ACCENT)];
    match app.model {
        Some(model) => {
            let link = if model.wireless { "wireless" } else { "wired" };
            spans.push(Span::styled(model.name, BOLD));
            spans.push(Span::styled(format!(" ({link})"), MUTED));
        }
        None => spans.push(Span::styled("rogctl", BOLD)),
    }
    if let (Link::Connected, Some(r)) = (&app.link, app.reading) {
        let word = crate::charge_word(r.charging, Some(r.battery));
        // Red at or below the mouse's own warning level (0 is off), as the
        // widgets are at theirs.
        let low = !r.charging
            && app
                .settings
                .low_battery_warning
                .is_some_and(|warn| warn > 0 && r.battery <= warn);
        spans.push(Span::styled(
            format!("  {battery_label}{}%", r.battery),
            if low { ACCENT } else { Style::new().fg(TEXT) },
        ));
        // Detail in parentheses is grey, like "(wireless)".
        spans.push(Span::styled(format!(" ({word})"), MUTED));
    }
    if app.demo {
        spans.push(Span::styled("  [demo]", BOLD));
    }
    Line::from(spans)
}

/// The keys, then the version. A narrow terminal drops the version first,
/// then the keys least needed; outside help, `?` and `q` are never dropped.
fn footer_line(app: &App, width: u16) -> Line<'static> {
    const ALWAYS: u8 = u8::MAX;
    // Only the keys that do something on this screen, each with how long it
    // stays as the terminal narrows: the lowest goes first.
    let mut keys = if app.help {
        // Help swallows every key until Esc, ?, q or a click closes it.
        vec![("Esc", "close help", ALWAYS)]
    } else {
        let mut keys = match app.link {
            Link::Connected => vec![
                ("↑↓", "select", 2),
                ("←→", "change", 3),
                // The last action key: nothing else says Enter switches or makes
                // a stage active.
                ("Enter", "apply", 4),
                ("Esc", "cancel", 0),
                ("r", "reload", 1),
            ],
            // Retrying is what this screen is for.
            Link::Asleep | Link::Disconnected | Link::Error(_) => vec![("r", "retry", 6)],
        };
        if app.devices > 1 {
            keys.push(("Tab", "next mouse", 5));
        }
        keys.extend([("?", "help", ALWAYS), ("q", "quit", ALWAYS)]);
        keys
    };

    // " key what", two spaces between keys and before the version.
    let used = |keys: &[(&str, &str, u8)]| {
        let items: usize = keys
            .iter()
            .map(|&(key, what, _)| Span::raw(key).width() + 1 + Span::raw(what).width())
            .sum();
        1 + items + 2 * keys.len().saturating_sub(1)
    };
    let width = usize::from(width);
    let version = concat!("v", env!("CARGO_PKG_VERSION"));
    let show_version = used(&keys) + 2 + version.len() <= width;
    while used(&keys) > width {
        let least_needed = keys
            .iter()
            .enumerate()
            .filter(|&(_, &(_, _, keep))| keep != ALWAYS)
            .min_by_key(|&(_, &(_, _, keep))| keep)
            .map(|(at, _)| at);
        let Some(at) = least_needed else {
            break;
        };
        keys.remove(at);
    }

    let mut spans = vec![Span::raw(" ")];
    for (i, (key, what, _)) in keys.into_iter().enumerate() {
        if i > 0 {
            spans.push(Span::raw("  "));
        }
        spans.push(Span::styled(key, BOLD));
        spans.push(Span::styled(format!(" {what}"), MUTED));
    }
    if show_version {
        spans.push(Span::styled(format!("  {version}"), MUTED));
    }
    Line::from(spans)
}

/// The settings' panels that have rows, top to bottom, each with its title.
fn panels(app: &App) -> Vec<(&'static str, Vec<Row>)> {
    let rows = app.rows();
    Panel::ALL
        .into_iter()
        .map(|panel| {
            let rows: Vec<Row> = rows
                .iter()
                .copied()
                .filter(|r| r.control.panel() == panel)
                .collect();
            (panel.title(), rows)
        })
        .filter(|(_, rows)| !rows.is_empty())
        .collect()
}

/// The panels' rows, the line above each, and the bottom border.
fn panels_height(panels: &[(&'static str, Vec<Row>)]) -> u16 {
    let lines: usize = panels.iter().map(|(_, rows)| rows.len() + 1).sum();
    u16::try_from(lines + usize::from(!panels.is_empty())).unwrap_or(u16::MAX)
}

fn draw_settings(
    frame: &mut Frame,
    app: &App,
    panels: Vec<(&'static str, Vec<Row>)>,
    area: Rect,
    hits: &mut Hits,
) {
    let mut top = area.y;
    for (i, (title, rows)) in panels.into_iter().enumerate() {
        // Each panel's top border is drawn over the bottom one above it.
        let height = u16::try_from(rows.len() + 2).unwrap_or(u16::MAX);
        let area = Rect {
            y: top,
            height,
            ..area
        }
        .intersection(area);
        top = top.saturating_add(height - 1);
        let block = panel(title, i > 0);
        let inner = block.inner(area);
        let mut lines = Vec::new();
        for (y, row) in (inner.y..inner.bottom()).zip(rows) {
            let line_area = Rect {
                y,
                height: 1,
                ..inner
            };
            // The band goes under the text, across the panel's whole width.
            if app.focused() == Some(row.control) {
                frame.render_widget(Block::new().style(FOCUSED), line_area);
            }
            let (line, choices) = row_line(app, row);
            if row.locked.is_none() {
                hits.add(line_area, Target::Row(row.control));
                for (columns, value) in choices {
                    let choice = Rect {
                        x: inner.x.saturating_add(columns.start),
                        width: columns.end - columns.start,
                        ..line_area
                    };
                    hits.add(
                        choice.intersection(line_area),
                        Target::Choice(row.control, value),
                    );
                }
            }
            lines.push(line);
        }
        frame.render_widget(Paragraph::new(lines).block(block), area);
    }
}

/// A row, and the columns each value on its scale covers, if it has one.
fn row_line(app: &App, row: Row) -> (Line<'static>, Vec<(Range<u16>, Value)>) {
    let control = row.control;
    let focused = app.focused() == Some(control);
    // The label's last two columns hold the focused row's ◀, so every value
    // starts in the same column.
    let label = format!("{:<width$}", control.label(), width = LABEL_WIDTH - 2);
    let mut spans = match (focused, row.locked) {
        (true, _) => vec![
            Span::styled("▌ ", Style::new().fg(RED)),
            Span::styled(label, BOLD),
        ],
        (false, Some(_)) => vec![Span::styled(format!("  {label}"), MUTED)],
        (false, None) => vec![Span::styled(format!("  {label}"), Style::new().fg(TEXT))],
    };

    let edit = app.edit.filter(|e| e.control == control);
    let shown = edit.map(|e| e.value).or_else(|| app.value(control));
    // A value that isn't one of the choices (unknown to rogctl) shows as text.
    let on_scale = app
        .choices(control)
        .filter(|_| row.locked.is_none())
        .zip(shown)
        .filter(|(choices, value)| choices.contains(value));
    // A focused text value gets ◀ and ▶ for the ways ←/→ can still go: grey,
    // red once they've made an edit. Typed DPI digits get a cursor instead.
    let typed = edit.is_some_and(|e| e.typed);
    let (left, right) = if focused && on_scale.is_none() && !typed {
        app.can_step(control)
    } else {
        (false, false)
    };
    let arrow = if edit.is_some() { ACCENT } else { MUTED };
    spans.push(if left {
        Span::styled("◀ ", arrow)
    } else {
        Span::raw("  ")
    });
    let mut columns = Vec::new();
    if let Some((choices, value)) = on_scale {
        let mut x = width(&spans);
        for (choice, choice_spans) in scale(&choices, value, edit.is_some()) {
            let end = x.saturating_add(width(&choice_spans));
            columns.push((x..end, choice));
            spans.extend(choice_spans);
            x = end;
        }
        if let Some(unit) = unit(value) {
            spans.push(Span::styled(format!(" {unit}"), MUTED));
        }
    } else {
        spans.extend(value_spans(row, focused, edit, app.value(control)));
        if right {
            spans.push(Span::styled(" ▶", arrow));
        } else if matches!(shown, Some(Value::Dpi(_))) && !typed {
            // Keeps the stages' swatches lined up with a focused stage's ▶.
            spans.push(Span::raw("  "));
        }
    }

    // A locked colour isn't the one lit, so it gets no swatch.
    if let (Some(Value::Color([r, g, b])), None) = (shown, row.locked) {
        spans.push(Span::styled(" ■", Style::new().fg(Color::Rgb(r, g, b))));
    }
    if let Control::Stage(i) = control {
        if let Some([r, g, b]) = app.settings.stages.get(i).and_then(|s| s.color) {
            spans.push(Span::styled(" ■", Style::new().fg(Color::Rgb(r, g, b))));
        }
        if app.settings.active_stage == Some(i) {
            spans.push(Span::styled("  ● active", ACCENT));
        }
    }
    if let Some(why) = row.locked {
        spans.push(Span::styled(format!("  🔒 {why}"), MUTED));
    }
    if let Some(edit) = edit {
        let dpi = app.model.map(|m| m.features.dpi);
        let hint = match (edit.typed, dpi) {
            // The range up front, rather than as an error after Enter.
            (true, Some(dpi)) => format!("  {}–{}, Enter to apply", dpi.min, dpi.max),
            // Said before the change, as changing it back doesn't undo it.
            _ if control == Control::LiftOff => {
                "  Enter to apply; resets sensor calibration".into()
            }
            _ => "  Enter to apply".into(),
        };
        spans.push(Span::styled(hint, MUTED));
    }
    (Line::from(spans), columns)
}

fn width(spans: &[Span]) -> u16 {
    let width: usize = spans.iter().map(Span::width).sum();
    u16::try_from(width).unwrap_or(u16::MAX)
}

/// A value drawn as text, without its arrows: pending edits underlined (with
/// a cursor while DPI digits are typed), switches that are on in red.
fn value_spans(
    row: Row,
    focused: bool,
    edit: Option<Edit>,
    value: Option<Value>,
) -> Vec<Span<'static>> {
    let (value, style) = match (edit, value) {
        (Some(Edit { value, typed, .. }), _) => {
            let edited = BOLD.add_modifier(Modifier::UNDERLINED);
            if typed {
                return vec![
                    Span::styled(value.to_string(), edited),
                    Span::styled("▏", ACCENT),
                ];
            }
            (value, edited)
        }
        (None, Some(value)) => {
            let style = match (row.locked, value) {
                (Some(_), _) => MUTED,
                (None, Value::Switch(true)) => ACCENT,
                (None, _) if focused => BOLD,
                (None, _) => Style::new().fg(TEXT),
            };
            (value, style)
        }
        (None, None) => return vec![Span::styled("unknown", MUTED)],
    };
    let text = value.to_string();
    // Padded so the stages' swatches and "● active" line up.
    let pad = match value {
        Value::Dpi(_) => DPI_WIDTH.saturating_sub(text.len()),
        Value::PollingRate(_)
        | Value::Debounce(_)
        | Value::Switch(_)
        | Value::Degrees(_)
        | Value::LiftOff(_)
        | Value::PowerOff(_)
        | Value::Percent(_)
        | Value::Effect(_)
        | Value::Color(_)
        | Value::Brightness(_) => 0,
    };
    vec![Span::styled(text, style), Span::raw(" ".repeat(pad))]
}

/// A row of choices drawn like GearLink's sliders: the track is red up to the
/// knob (●) on `value`, grey after it. `pending`: the value isn't sent yet.
/// Each choice comes with its spans, the track leading up to it included.
fn scale(choices: &[Value], value: Value, pending: bool) -> Vec<(Value, Vec<Span<'static>>)> {
    let at = choices.iter().position(|&c| c == value).unwrap_or(0);
    let mut drawn = Vec::new();
    for (i, &choice) in choices.iter().enumerate() {
        let mut spans = Vec::new();
        if i > 0 {
            let track = if i <= at {
                Style::new().fg(RED)
            } else {
                Style::new().fg(LINE)
            };
            spans.push(Span::styled("─", track));
        }
        if i == at {
            let label = if pending {
                BOLD.add_modifier(Modifier::UNDERLINED)
            } else {
                BOLD
            };
            spans.push(Span::styled("●", Style::new().fg(RED)));
            spans.push(Span::styled(tick(choice), label));
        } else {
            spans.push(Span::styled(tick(choice), MUTED));
        }
        drawn.push((choice, spans));
    }
    drawn
}

/// A choice's label on a scale: `Value`'s text without the unit, which the
/// scale shows once at the end.
fn tick(value: Value) -> String {
    match value {
        Value::PollingRate(rate) => rate.hz().to_string(),
        Value::Debounce(d) => d.ms().to_string(),
        Value::PowerOff(p) => p
            .minutes()
            .map_or_else(|| value.to_string(), |m| format!("{m}m")),
        Value::Percent(0) => "Off".into(),
        // Brightness 0 is "0" rather than "Off": the full scale and its hint
        // then fit.
        Value::Percent(pct) | Value::Brightness(pct) => pct.to_string(),
        Value::Dpi(_)
        | Value::Switch(_)
        | Value::Degrees(_)
        | Value::LiftOff(_)
        | Value::Effect(_)
        | Value::Color(_) => value.to_string(),
    }
}

fn unit(value: Value) -> Option<&'static str> {
    match value {
        Value::PollingRate(_) => Some("Hz"),
        Value::Debounce(_) => Some("ms"),
        Value::Percent(_) | Value::Brightness(_) => Some("%"),
        Value::Dpi(_)
        | Value::Switch(_)
        | Value::Degrees(_)
        | Value::LiftOff(_)
        | Value::PowerOff(_)
        | Value::Effect(_)
        | Value::Color(_) => None,
    }
}

fn draw_link_notice(frame: &mut Frame, app: &App, area: Rect) {
    let name = app.model.map_or("The mouse", |m| m.name);
    let (title, hint) = match &app.link {
        Link::Disconnected => (
            "No supported mouse found.".to_owned(),
            "Plug in the receiver or cable; rogctl keeps looking.".to_owned(),
        ),
        Link::Asleep => (
            format!("{name} is asleep or out of range."),
            "Move the mouse to wake it; rogctl reads it again shortly, or press r.".to_owned(),
        ),
        Link::Error(e) => {
            let hint = if e.contains("ermission denied") {
                "Install the udev rule from the README, then replug the receiver or cable."
            } else {
                "Press r to try again."
            };
            (format!("{name}: {e}"), hint.to_owned())
        }
        Link::Connected => return,
    };
    let title_style = match app.link {
        Link::Error(_) => ACCENT,
        Link::Connected | Link::Asleep | Link::Disconnected => BOLD,
    };
    let text = vec![
        Line::styled(title, title_style),
        Line::raw(""),
        Line::styled(hint, MUTED),
    ];
    draw_notice(frame, area, text);
}

/// `text` broken at spaces into lines at most `width` columns wide; a word
/// wider than that (a long path) is broken across lines. Past `max_lines` the
/// text is cut, and the last line ends in "…" to show it.
fn wrap_words(text: &str, width: usize, max_lines: usize) -> Vec<String> {
    let mut lines: Vec<String> = Vec::new();
    for word in text.split_whitespace().flat_map(|w| split_wide(w, width)) {
        match lines.last_mut() {
            Some(line) if Span::raw(&*line).width() + 1 + Span::raw(&*word).width() <= width => {
                line.push(' ');
                line.push_str(&word);
            }
            Some(_) | None => lines.push(word),
        }
    }
    if lines.len() > max_lines {
        lines.truncate(max_lines);
        if let Some(last) = lines.last_mut() {
            while Span::raw(&*last).width() + 1 > width && last.pop().is_some() {}
            last.push('…');
        }
    }
    lines
}

/// `word` in pieces at most `width` columns wide.
fn split_wide(word: &str, width: usize) -> Vec<String> {
    let mut pieces: Vec<String> = Vec::new();
    let mut buf = [0; 4];
    for c in word.chars() {
        let c = &*c.encode_utf8(&mut buf);
        match pieces.last_mut() {
            Some(piece) if Span::raw(&*piece).width() + Span::raw(c).width() <= width => {
                piece.push_str(c);
            }
            Some(_) | None => pieces.push(c.to_owned()),
        }
    }
    pieces
}

/// btop's notice in place of everything: the terminal's size, each side red
/// while it is too small, and the size rogctl needs.
fn draw_too_small(frame: &mut Frame, (min_width, min_height): (u16, u16)) {
    let area = frame.area();
    let side = |have: u16, need: u16| {
        Span::styled(have.to_string(), if have < need { ACCENT } else { BOLD })
    };
    let lines = vec![
        Line::styled("Terminal size too small:", BOLD),
        Line::from(vec![
            Span::raw("Width = "),
            side(area.width, min_width),
            Span::raw(" Height = "),
            side(area.height, min_height),
        ]),
        Line::raw(""),
        Line::styled("Needed for rogctl:", BOLD),
        Line::raw(format!("Width = {min_width} Height = {min_height}")),
    ];
    let top = area.height.saturating_sub(lines.len() as u16) / 2;
    frame.render_widget(
        Paragraph::new(lines).centered(),
        Rect {
            y: area.y + top,
            height: area.height - top,
            ..area
        },
    );
}

/// Text in place of the settings. No frame: an empty box the height of the
/// screen would dwarf it.
fn draw_notice(frame: &mut Frame, area: Rect, text: Vec<Line>) {
    let block = Block::new().padding(Padding::new(2, 2, 1, 0));
    frame.render_widget(
        Paragraph::new(text).block(block).wrap(Wrap { trim: true }),
        area,
    );
}

const HELP: &[(&str, &str)] = &[
    ("↑ ↓  k j", "Select a setting"),
    ("← → h l", "Change the value"),
    ("Shift, PgUp PgDn", "Change it in bigger steps"),
    ("0-9, Backspace", "Type a DPI value"),
    ("Enter, Space", "Apply the change, switch on or off,"),
    ("", "or make a DPI stage the active one"),
    ("Esc", "Cancel the change"),
    ("r", "Read the settings from the mouse again"),
    ("Tab", "Next mouse, when several are plugged in"),
    ("Mouse", "Click a setting to select it, again to apply;"),
    ("", "click a scale or scroll to change the value"),
    ("q", "Quit"),
];

fn draw_help(frame: &mut Frame, area: Rect) {
    let mut lines: Vec<Line> = HELP
        .iter()
        .map(|(keys, what)| {
            Line::from(vec![
                Span::styled(format!(" {keys:<18}"), BOLD),
                Span::styled(*what, Style::new().fg(TEXT)),
            ])
        })
        .collect();
    lines.push(Line::raw(""));
    lines.push(Line::styled(
        " Applied changes are stored on the mouse.",
        MUTED,
    ));
    lines.push(Line::styled(" Esc, ? or q closes this.", MUTED));
    // Wide enough for the longest line and the border.
    let width = lines
        .iter()
        .map(|l| l.width() + 3)
        .max()
        .map_or(area.width, |w| u16::try_from(w).unwrap_or(u16::MAX))
        .min(area.width);
    let height = (lines.len() as u16 + 2).min(area.height);
    let popup = Rect {
        x: area.x + (area.width - width) / 2,
        y: area.y + (area.height - height) / 2,
        width,
        height,
    };
    // GearLink outlines what's selected in red; the help box is on top of all.
    let block = panel("Keys", false)
        .border_style(Style::new().fg(RED))
        .style(BASE);
    frame.render_widget(ratatui::widgets::Clear, popup);
    frame.render_widget(Paragraph::new(lines).block(block), popup);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::{self, Action};
    use crate::models::MODELS;
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;
    use ratatui::buffer::Buffer;
    use ratatui::crossterm::event::{KeyCode, KeyModifiers};

    /// Draws `app` on a test terminal and returns its cells.
    fn draw_to_buffer(app: &App, width: u16, height: u16) -> Buffer {
        draw_in(app, width, height, ColorMode::TrueColor)
    }

    fn draw_in(app: &App, width: u16, height: u16, colors: ColorMode) -> Buffer {
        let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
        terminal
            .draw(|frame| {
                draw(frame, app, colors);
            })
            .unwrap();
        terminal.backend().buffer().clone()
    }

    /// The screen as text, one line per row, trailing spaces trimmed.
    fn render(app: &App, width: u16, height: u16) -> String {
        screen_text(&draw_to_buffer(app, width, height))
    }

    fn screen_text(buffer: &Buffer) -> String {
        let Rect { width, height, .. } = buffer.area;
        (0..height)
            .map(|y| {
                (0..width)
                    .map(|x| buffer[(x, y)].symbol())
                    .collect::<String>()
                    .trim_end()
                    .to_owned()
            })
            .collect::<Vec<_>>()
            .join("\n")
    }

    fn demo(model: &'static crate::device::Model) -> App {
        let (reading, settings) = app::demo_state(&model.features);
        let mut app = App::new(true);
        app.show(Some(model), Link::Connected, Some(reading), settings);
        app
    }

    /// Every model draws, from a tiny terminal to a wide one, in every state.
    #[test]
    fn every_model_renders_at_any_size() {
        for model in MODELS {
            let mut app = demo(model);
            for (w, h) in [(1, 1), (20, 5), (80, 24), (200, 60)] {
                render(&app, w, h);
            }
            app.help = true;
            render(&app, 80, 24);
            render(&app, 10, 3);
            for link in [Link::Asleep, Link::Disconnected, Link::Error("x".into())] {
                app.link = link;
                render(&app, 80, 24);
            }
        }
    }

    #[test]
    fn shows_harpe_ii_ace_settings() {
        let model = app::find_model(app::DEMO_MODEL).unwrap();
        let mut app = demo(model);
        let screen = render(&app, 80, 24);
        for text in [
            "ROG Harpe II Ace (wireless)",
            "Battery: 80% (discharging)",
            "[demo]",
            "▌ DPI stage 1",
            "1600",
            "● active",
            "Polling rate",
            "125─250─500─●1000─2000─4000─8000 Hz",
            "Motion sync",
            "Lift-off distance",
            "Auto power-off",
            "1m─2m─3m─5m─●10m─Never",
            "Low-battery warning",
            "Off─10─●20─30─40─50 %",
        ] {
            assert!(screen.contains(text), "{text:?} missing:\n{screen}");
        }
        assert!(!screen.contains("debounce"), "not on this mouse:\n{screen}");

        app.handle_key(KeyCode::Char('9'), KeyModifiers::NONE);
        let screen = render(&app, 80, 24);
        assert!(
            screen.contains("9▏ ■  100–42000, Enter to apply"),
            "{screen}"
        );
        app.handle_key(KeyCode::Esc, KeyModifiers::NONE);

        app.handle_key(KeyCode::Right, KeyModifiers::NONE);
        let screen = render(&app, 80, 24);
        assert!(
            screen.contains("◀ 450   ▶ ■"),
            "padded like the stages\n{screen}"
        );
        assert!(screen.contains("Enter to apply"), "{screen}");
        let action = app.handle_key(KeyCode::Enter, KeyModifiers::NONE);
        let Some(Action::Apply { change, done }) = action else {
            panic!("{action:?}");
        };
        app.simulate(&change);
        app.ok(done);
        let screen = render(&app, 80, 24);
        assert!(screen.contains("DPI stage 1 set to 450"), "{screen}");
    }

    /// The lighting gets its own panel, joined to the one above, all within
    /// 80x24; the colour has a swatch unless the effect sets its own.
    #[test]
    fn shows_harpe_ii_ace_lighting() {
        let model = app::find_model(app::DEMO_MODEL).unwrap();
        let mut app = demo(model);
        let screen = render(&app, 80, 24);
        for text in [
            "├ Battery ─",
            "├ Lighting ─",
            "Effect                  Static",
            "Colour                  Red ■",
            "Brightness              0─10─20─30─40─●50─60─70─80─90─100 %",
        ] {
            assert!(screen.contains(text), "{text:?} missing:\n{screen}");
        }
        let lines: Vec<&str> = screen.lines().collect();
        assert!(lines[20].starts_with('└'), "one bottom border\n{screen}");

        while app.focused() != Some(Control::LightingEffect) {
            app.handle_key(KeyCode::Down, KeyModifiers::NONE);
        }
        app.handle_key(KeyCode::Right, KeyModifiers::NONE);
        app.handle_key(KeyCode::Right, KeyModifiers::NONE);
        let screen = render(&app, 80, 24);
        assert!(
            screen.contains("◀ Colour cycle ▶  Enter to apply"),
            "{screen}"
        );
        let Some(Action::Apply { change, .. }) = app.handle_key(KeyCode::Enter, KeyModifiers::NONE)
        else {
            panic!("no change");
        };
        app.simulate(&change);
        let screen = render(&app, 80, 24);
        let colour = screen.lines().find(|l| l.contains("Colour   "));
        assert!(
            colour.is_some_and(|l| in_order(l, "Red 🔒 the effect sets its own")),
            "{screen}"
        );
        assert!(!screen.contains("Red ■"), "no swatch\n{screen}");
    }

    /// A focused text value shows ◀ and ▶ only for the ways ←/→ can still go,
    /// grey until they make an edit, then red; other rows show none.
    #[test]
    fn arrows_show_only_where_the_value_can_go() {
        let model = app::find_model(app::DEMO_MODEL).unwrap();
        let mut app = demo(model);
        let effect_row = |app: &App| {
            let row = app
                .rows()
                .into_iter()
                .find(|r| r.control == Control::LightingEffect);
            row_line(app, row.unwrap()).0
        };
        let arrows = |line: &Line| {
            line.spans
                .iter()
                .filter(|s| s.content.contains(['◀', '▶']))
                .map(|s| (s.content.trim().to_owned(), s.style.fg))
                .collect::<Vec<_>>()
        };
        assert_eq!(arrows(&effect_row(&app)), [], "not focused");

        while app.focused() != Some(Control::LightingEffect) {
            app.handle_key(KeyCode::Down, KeyModifiers::NONE);
        }
        assert_eq!(
            arrows(&effect_row(&app)),
            [("▶".to_owned(), Some(GREY))],
            "Static is first"
        );
        app.handle_key(KeyCode::Right, KeyModifiers::NONE);
        assert_eq!(
            arrows(&effect_row(&app)),
            [("◀".to_owned(), Some(RED)), ("▶".to_owned(), Some(RED))],
            "an edit"
        );
        app.handle_key(KeyCode::Right, KeyModifiers::SHIFT);
        assert_eq!(
            arrows(&effect_row(&app)),
            [("◀".to_owned(), Some(RED))],
            "Battery level is last"
        );
        let screen = render(&app, 80, 24);
        assert!(
            screen.contains("Effect                ◀ Battery level  Enter to apply"),
            "the value stays in its column\n{screen}"
        );
    }

    /// The knob follows a pending edit; the track is red up to it; a locked
    /// row and a value that isn't a choice fall back to text.
    #[test]
    fn scales_follow_edits_and_fall_back_to_text() {
        let model = app::find_model(app::DEMO_MODEL).unwrap();
        let mut app = demo(model);
        for _ in 0..4 {
            app.handle_key(KeyCode::Down, KeyModifiers::NONE);
        }
        app.handle_key(KeyCode::Right, KeyModifiers::NONE);
        let screen = render(&app, 80, 24);
        let row = screen.lines().find(|l| l.contains("Polling rate")).unwrap();
        assert!(
            row.contains("1000─●2000") && row.contains("Enter to apply"),
            "{screen}"
        );
        let power_saving = screen
            .lines()
            .find(|l| l.contains("Power-saving rate"))
            .unwrap();
        assert!(
            power_saving.contains("1000 Hz") && !power_saving.contains('●'),
            "locked"
        );

        let buffer = draw_to_buffer(&app, 80, 24);
        let y = 6;
        let x = (0..80).find(|&x| buffer[(x, y)].symbol() == "●").unwrap();
        assert_eq!(buffer[(x - 1, y)].fg, RED, "track before the knob");
        assert_eq!(buffer[(x + 5, y)].fg, LINE, "track after it");

        app.settings.power_off = Some(crate::protocol::PowerOff::from_code(9));
        let screen = render(&app, 80, 24);
        assert!(screen.contains("Unknown (0x09)"), "{screen}");
    }

    /// The header's battery is white, in the widgets' words; the word in
    /// parentheses is grey, like "(wireless)".
    #[test]
    fn header_battery_reads_like_the_widgets() {
        let model = app::find_model(app::DEMO_MODEL).unwrap();
        let mut app = demo(model);
        let header = |app: &App| render(app, 80, 24).lines().next().unwrap().to_owned();
        let buffer = draw_to_buffer(&app, 80, 24);
        let x = (0..80).find(|&x| buffer[(x, 0)].symbol() == "B").unwrap();
        assert_eq!(buffer[(x, 0)].fg, TEXT);
        let parens: Vec<u16> = (0..80)
            .filter(|&x| buffer[(x, 0)].symbol() == "(")
            .collect();
        assert_eq!(parens.len(), 2, "(wireless) and (discharging)");
        for x in parens {
            assert_eq!(buffer[(x, 0)].fg, GREY);
        }

        for (battery, word) in [(80, "charging"), (100, "full")] {
            app.reading = app.reading.map(|r| crate::protocol::Reading {
                battery,
                charging: true,
                ..r
            });
            let expected = format!("Battery: {battery}% ({word})");
            assert!(header(&app).contains(&expected), "{}", header(&app));
        }
    }

    /// The version ends the footer when it fits (a narrower terminal drops it,
    /// `narrow_footer_keeps_help_and_quit`); with Tab too, the footer uses a
    /// wider terminal rather than being cut at MAX_WIDTH.
    #[test]
    fn footer_ends_with_the_version() {
        let model = app::find_model(app::DEMO_MODEL).unwrap();
        let mut app = demo(model);
        let version = concat!("v", env!("CARGO_PKG_VERSION"));
        let footer = |app: &App, width| render(app, width, 24).lines().last().unwrap().to_owned();
        assert!(
            footer(&app, 100).ends_with(version),
            "{}",
            footer(&app, 100)
        );
        app.devices = 2;
        let wide = footer(&app, 120);
        assert!(
            wide.contains("Tab next mouse") && wide.ends_with(version),
            "{wide}"
        );
    }

    /// Red at or below the mouse's warning level, unless charging or the
    /// warning is off. The demo warns at 20%.
    #[test]
    fn header_battery_is_red_when_low() {
        let model = app::find_model(app::DEMO_MODEL).unwrap();
        let mut app = demo(model);
        let color = |app: &App, battery, charging| {
            let mut app_at = App::new(true);
            let reading = app.reading.map(|r| crate::protocol::Reading {
                battery,
                charging,
                ..r
            });
            app_at.show(app.model, Link::Connected, reading, app.settings.clone());
            let buffer = draw_to_buffer(&app_at, 80, 24);
            let x = (0..80).find(|&x| buffer[(x, 0)].symbol() == "B").unwrap();
            buffer[(x, 0)].fg
        };
        assert_eq!(color(&app, 21, false), TEXT);
        assert_eq!(color(&app, 20, false), RED);
        assert_eq!(color(&app, 20, true), TEXT, "charging");
        app.settings.low_battery_warning = Some(0);
        assert_eq!(color(&app, 5, false), TEXT, "warning off");
    }

    /// The focused row is a red band across the whole panel; others aren't.
    #[test]
    fn focused_row_is_a_red_band() {
        let model = app::find_model(app::DEMO_MODEL).unwrap();
        let app = demo(model);
        let buffer = draw_to_buffer(&app, 80, 24);
        // Row 2 is DPI stage 1 (focused), row 3 DPI stage 2; x = 60 is past the text.
        assert_eq!(buffer[(60, 2)].bg, RED_BAND);
        assert_eq!(buffer[(60, 3)].bg, BACKGROUND);
        assert_eq!(buffer[(1, 2)].symbol(), "▌");
        assert_eq!(buffer[(1, 2)].fg, RED);
        assert_eq!(
            buffer[(79, 23)].bg,
            BACKGROUND,
            "the whole screen is themed"
        );
    }

    /// Every row that can be selected, and every value on its scale, can be
    /// clicked where it's drawn; locked rows can't. Regions stay on screen.
    #[test]
    fn hits_match_what_is_drawn() {
        let text = |buffer: &Buffer, area: Rect| {
            area.positions()
                .map(|p| buffer[p].symbol().to_owned())
                .collect::<String>()
        };
        for model in MODELS {
            let app = demo(model);
            let mut terminal = Terminal::new(TestBackend::new(MIN_SIZE.0, MIN_SIZE.1)).unwrap();
            let mut hits = None;
            terminal
                .draw(|frame| hits = draw(frame, &app, ColorMode::TrueColor))
                .unwrap();
            let hits = hits.unwrap();
            let buffer = terminal.backend().buffer();
            for row in app.rows() {
                let control = row.control;
                let area = hits.0.iter().find(|(_, t)| *t == Target::Row(control));
                let Some(&(area, _)) = area else {
                    assert!(row.locked.is_some(), "{} {control:?}", model.name);
                    continue;
                };
                assert!(row.locked.is_none(), "{} {control:?}", model.name);
                assert!(text(buffer, area).contains(&control.label()));
                for value in app.choices(control).unwrap_or_default() {
                    let target = Target::Choice(control, value);
                    let Some(&(area, _)) = hits.0.iter().find(|(_, t)| *t == target) else {
                        panic!("{} {target:?} not clickable", model.name);
                    };
                    assert!(
                        text(buffer, area).ends_with(&tick(value)),
                        "{} {target:?}: {:?}",
                        model.name,
                        text(buffer, area)
                    );
                    for p in area.positions() {
                        assert_eq!(hits.target_at(p.x, p.y), Some(target));
                    }
                }
            }
            for (w, h) in [(80, 24), (120, 40)] {
                let mut terminal = Terminal::new(TestBackend::new(w, h)).unwrap();
                let mut hits = None;
                terminal
                    .draw(|frame| hits = draw(frame, &app, ColorMode::TrueColor))
                    .unwrap();
                for (area, target) in &hits.unwrap().0 {
                    assert!(area.right() <= w && area.bottom() <= h, "{target:?}");
                }
            }
        }
    }

    /// The help box is as wide as its longest line, so nothing is cut off.
    #[test]
    fn help_fits_its_lines() {
        let model = app::find_model(app::DEMO_MODEL).unwrap();
        let mut app = demo(model);
        app.help = true;
        let screen = render(&app, 80, 24);
        assert!(screen.contains("again to apply;"), "{screen}");
    }

    /// A line's text, without its styles.
    fn text(line: &Line) -> String {
        line.spans.iter().map(|s| s.content.as_ref()).collect()
    }

    /// At 80 columns, the narrowest the footer is drawn, every key fits but
    /// with Tab, which costs Esc; `?` and `q` stay.
    #[test]
    fn narrow_footer_keeps_help_and_quit() {
        let model = app::find_model(app::DEMO_MODEL).unwrap();
        let mut app = demo(model);
        let width = MIN_SIZE.0;
        let footer = |app: &App| text(&footer_line(app, width));
        assert!(
            footer(&app).starts_with(
                " ↑↓ select  ←→ change  Enter apply  Esc cancel  r reload  ? help  q quit"
            ),
            "{:?}",
            footer(&app)
        );
        app.devices = 2;
        assert_eq!(
            footer(&app),
            " ↑↓ select  ←→ change  Enter apply  r reload  Tab next mouse  ? help  q quit"
        );
        app.help = true;
        let version = concat!("v", env!("CARGO_PKG_VERSION"));
        assert_eq!(
            footer(&app),
            format!(" Esc close help  {version}"),
            "only Esc works"
        );

        let mut app = App::new(false);
        app.devices = 2;
        assert!(
            footer(&app).starts_with(" r retry  Tab next mouse  ? help  q quit"),
            "{:?}",
            footer(&app)
        );
    }

    /// Below 80x24 only btop's notice draws: the terminal's size, the side
    /// that is too small in red, the size needed, and nothing to click.
    #[test]
    fn too_small_terminal_shows_the_size_needed() {
        let model = app::find_model(app::DEMO_MODEL).unwrap();
        let app = demo(model);
        assert!(!render(&app, 80, 24).contains("too small"));
        let screen = render(&app, 79, 24);
        for text in [
            "Terminal size too small:",
            "Width = 79 Height = 24",
            "Needed for rogctl:",
            "Width = 80 Height = 24",
        ] {
            assert!(screen.contains(text), "{text:?}\n{screen}");
        }
        assert!(!screen.contains("DPI"), "{screen}");

        let buffer = draw_to_buffer(&app, 79, 24);
        let (y, line) = screen
            .lines()
            .enumerate()
            .find(|(_, l)| l.contains("Width = 79"))
            .unwrap();
        // The column, not the byte offset: they differ past a non-ASCII
        // character.
        let at = |number: &str| {
            let x = Span::raw(&line[..line.find(number).unwrap()]).width();
            buffer[(x as u16, y as u16)].fg
        };
        assert_eq!(at("79"), RED, "too narrow");
        assert_eq!(at("24"), TEXT, "tall enough");

        for (w, h) in [(79, 24), (80, 23), (1, 1)] {
            let mut terminal = Terminal::new(TestBackend::new(w, h)).unwrap();
            let mut hits = Some(Hits::default());
            terminal
                .draw(|frame| hits = draw(frame, &app, ColorMode::TrueColor))
                .unwrap();
            assert!(hits.is_none(), "{w}x{h}");
        }
    }

    /// Panels taller than the space left for them aren't drawn cut off, which
    /// could hide the focused row: the notice says how many rows they need.
    /// No model is that tall at `MIN_SIZE`, so this draws below it.
    #[test]
    fn panels_that_do_not_fit_show_the_size_needed() {
        let model = app::find_model(app::DEMO_MODEL).unwrap();
        let mut app = demo(model);
        app.error("x");
        let panels = panels_height(&panels(&app));
        // The header, a one-row message and the footer.
        let needed = panels + 3;
        let draw_at = |app: &App, height| {
            let mut terminal = Terminal::new(TestBackend::new(MIN_SIZE.0, height)).unwrap();
            let mut hits = None;
            terminal
                .draw(|frame| hits = draw_screen(frame, app))
                .unwrap();
            (hits, screen_text(terminal.backend().buffer()))
        };
        let (hits, screen) = draw_at(&app, needed - 1);
        assert!(hits.is_none(), "{screen}");
        let size = format!("Width = 80 Height = {needed}");
        assert!(screen.contains(&size), "{size:?}\n{screen}");
        assert!(!screen.contains("DPI"), "{screen}");
        let (hits, screen) = draw_at(&app, needed);
        assert!(hits.is_some(), "{screen}");

        // A long message shortens to one row first, so the settings and Esc
        // stay.
        app.error("word ".repeat(100));
        let (hits, screen) = draw_at(&app, needed);
        assert!(hits.is_some(), "{screen}");
        let lines: Vec<&str> = screen.lines().collect();
        let message = lines[lines.len() - 2];
        assert!(
            message.starts_with(" ✗ word") && message.ends_with('…'),
            "{screen}"
        );
    }

    /// A message too long for its rows ends in "…" rather than being cut
    /// without a sign: two rows under the tallest model at 80x24, three on a
    /// taller terminal. On a wide terminal it uses the whole width.
    #[test]
    fn messages_wrap_across_the_terminal() {
        let model = app::find_model(app::DEMO_MODEL).unwrap();
        let mut app = demo(model);
        app.error("word ".repeat(100));
        let screen = render(&app, 80, 24);
        let lines: Vec<&str> = screen.lines().collect();
        assert!(lines[22].ends_with('…'), "{screen}");
        assert!(Span::raw(lines[22]).width() <= 80, "{screen}");
        assert!(lines[21].starts_with(" ✗ word"), "two rows\n{screen}");
        let screen = render(&app, 80, 25);
        let lines: Vec<&str> = screen.lines().collect();
        assert!(lines[23].ends_with('…'), "{screen}");
        assert!(lines[21].starts_with(" ✗ word"), "three rows\n{screen}");

        app.error(
            "Power-saving rate set to 1000 Hz, but not saved on the mouse: \
             this mouse doesn't have that setting",
        );
        let screen = render(&app, 120, 24);
        let lines: Vec<&str> = screen.lines().collect();
        assert!(lines[22].ends_with("that setting"), "one row\n{screen}");
    }

    #[test]
    fn wrap_words_marks_a_cut() {
        assert_eq!(wrap_words("aa bb cc", 5, 2), ["aa bb", "cc"]);
        assert_eq!(wrap_words("aa bb cc dd", 5, 1), ["aa b…"]);
        assert_eq!(wrap_words("aa bb cc dd", 6, 1), ["aa bb…"]);
        assert!(wrap_words("", 5, 1).is_empty());
        assert_eq!(wrap_words("abcdefgh", 3, 3), ["abc", "def", "gh"], "broken");
        assert_eq!(wrap_words("abcdefgh x", 3, 2), ["abc", "de…"]);
    }

    /// Whether every word of `text` is in `line`, in order.
    fn in_order(line: &str, text: &str) -> bool {
        let mut rest = line;
        text.split_whitespace().all(|word| match rest.find(word) {
            Some(at) => {
                rest = &rest[at + word.len()..];
                true
            }
            None => false,
        })
    }

    /// Every model, focused row, pending edit, message, help and notice fits
    /// 80x24 (`MIN_SIZE`): each row shows whole on its own line, with its
    /// hint, and the header and messages whole. Only the footer may drop
    /// keys and the version.
    #[test]
    fn every_screen_fits_the_min_size() {
        let (w, h) = MIN_SIZE;
        let fits = |app: &App| {
            let name = app.model.map(|m| m.name);
            let screen = render(app, w, h);
            let lines: Vec<&str> = screen.lines().collect();
            let header = text(&header_line(app, w));
            assert!(in_order(lines[0], &header), "{name:?}: header\n{screen}");
            for row in app.rows() {
                let label = row.control.label();
                let row_text = text(&row_line(app, row).0);
                let line = lines.iter().find(|l| l.contains(&label));
                assert!(
                    line.is_some_and(|l| in_order(l, &row_text)),
                    "{name:?}: {row_text:?} is cut\n{screen}"
                );
            }
            if let Some(m) = &app.message {
                // The rows between the panels and the footer, so the
                // panels' own text can't stand in for a cut message.
                let panels_end = lines.iter().rposition(|l| l.starts_with('└'));
                let below = lines[panels_end.unwrap() + 1..lines.len() - 1].join(" ");
                assert!(
                    in_order(&below, &m.text),
                    "{name:?}: {:?} is cut\n{screen}",
                    m.text
                );
            }
        };
        let typed_dpi = [KeyCode::Char('1'), KeyCode::Char('2'), KeyCode::Char('3')];
        for model in MODELS {
            let mut app = demo(model);
            // The longest battery text: full, but not charging.
            app.reading = app.reading.map(|r| crate::protocol::Reading {
                battery: 100,
                charging: false,
                ..r
            });
            assert!(
                header_line(&app, w).width() <= usize::from(w),
                "{}",
                model.name
            );
            for _ in app.rows() {
                for keys in [&[][..], &[KeyCode::Right], &typed_dpi] {
                    for &key in keys {
                        app.handle_key(key, KeyModifiers::NONE);
                    }
                    for row in app.rows() {
                        let (line, _) = row_line(&app, row);
                        assert!(
                            line.width() <= usize::from(MAX_WIDTH - 2),
                            "{}: {:?} is wider than its panel",
                            model.name,
                            text(&line)
                        );
                    }
                    fits(&app);
                    app.handle_key(KeyCode::Esc, KeyModifiers::NONE);
                }
                app.handle_key(KeyCode::Down, KeyModifiers::NONE);
            }
            // The longest message rogctl writes takes two rows, and an
            // unusually long OS error a third.
            app.error(
                "Power-saving rate set to 1000 Hz, but the mouse went to sleep or out \
                 of range before confirming the save",
            );
            fits(&app);
            // An unusually long OS error takes a third row where the panels
            // leave one, else ends in "…".
            app.error(format!("{} (os error 5)", "Input/output error ".repeat(9)));
            if h - panels_height(&panels(&app)) - 2 >= 3 {
                fits(&app);
            } else {
                let screen = render(&app, w, h);
                assert!(
                    screen.contains(" ✗ Input/output error") && screen.contains('…'),
                    "{}\n{screen}",
                    model.name
                );
            }
            app.message = None;

            app.help = true;
            let screen = render(&app, w, h);
            for (keys, what) in HELP {
                let line = screen.lines().find(|l| l.contains(what));
                assert!(
                    line.is_some_and(|l| l.contains(keys)),
                    "{keys:?} {what:?}\n{screen}"
                );
            }
            app.help = false;

            for (link, hint) in [
                (Link::Asleep, "or press r."),
                (Link::Disconnected, "rogctl keeps looking."),
                (
                    Link::Error("Permission denied".into()),
                    "replug the receiver or cable.",
                ),
            ] {
                app.link = link;
                let screen = render(&app, w, h);
                assert!(screen.contains(hint), "{hint:?}\n{screen}");
            }
        }
    }

    /// Without a mouse the footer offers only the keys that do something.
    #[test]
    fn footer_keys_fit_the_screen() {
        let mut app = App::new(false);
        let footer = render(&app, 80, 24).lines().last().unwrap().to_owned();
        assert!(footer.starts_with(" r retry  ? help  q quit"), "{footer}");
        app.devices = 2;
        let footer = render(&app, 80, 24).lines().last().unwrap().to_owned();
        assert!(footer.contains("Tab next mouse"), "{footer}");
    }

    /// WCAG contrast of two colours of the palette or the mouse's.
    fn contrast(a: Color, b: Color) -> f64 {
        let luminance = |c: Color| {
            let [r, g, b] = match c {
                Color::Rgb(r, g, b) => [r, g, b],
                Color::Indexed(i) if i >= 16 => xterm_rgb(i),
                _ => panic!("not a palette colour: {c:?}"),
            };
            let channel = |c: u8| {
                let c = f64::from(c) / 255.0;
                if c <= 0.04045 {
                    c / 12.92
                } else {
                    ((c + 0.055) / 1.055).powf(2.4)
                }
            };
            0.2126 * channel(r) + 0.7152 * channel(g) + 0.0722 * channel(b)
        };
        let (a, b) = (luminance(a), luminance(b));
        (a.max(b) + 0.05) / (a.min(b) + 0.05)
    }

    /// All text reads at 4.5:1, on every model, focused, edited, applied, in
    /// help and on every notice, in 24-bit and in 256 colours. Lines,
    /// swatches and the focus bar aren't text.
    #[test]
    fn text_has_enough_contrast() {
        let check = |app: &App| {
            for colors in [ColorMode::TrueColor, ColorMode::Indexed] {
                let buffer = draw_in(app, 80, 24, colors);
                for cell in buffer.content() {
                    let symbol = cell.symbol();
                    if symbol.trim().is_empty() || "─│┌┐└┘├┤■▌".contains(symbol)
                    {
                        continue;
                    }
                    let ratio = contrast(cell.fg, cell.bg);
                    assert!(
                        ratio >= 4.5,
                        "{colors:?}: {symbol:?} at {ratio:.2}: {cell:?}"
                    );
                }
            }
        };
        let mut seen = Vec::new();
        for model in MODELS {
            // Models with the same features draw alike.
            if seen.contains(&model.features) {
                continue;
            }
            seen.push(model.features);
            let mut app = demo(model);
            // Each row edited, then applied (focused, with the ✓ message).
            for _ in app.rows() {
                app.handle_key(KeyCode::Right, KeyModifiers::NONE);
                check(&app);
                if let Some(Action::Apply { change, done }) =
                    app.handle_key(KeyCode::Enter, KeyModifiers::NONE)
                {
                    app.simulate(&change);
                    app.ok(done);
                }
                check(&app);
                app.handle_key(KeyCode::Down, KeyModifiers::NONE);
            }
            app.error("x");
            check(&app);
            app.help = true;
            check(&app);
            for link in [Link::Asleep, Link::Disconnected, Link::Error("x".into())] {
                app.show(Some(model), link, None, Default::default());
                check(&app);
            }
        }
    }

    /// `NO_COLOR` wins, then `COLORTERM`; without either, 256 colours.
    #[test]
    fn color_mode_follows_the_environment() {
        assert_eq!(
            ColorMode::from_env(true, Some("truecolor")),
            ColorMode::None
        );
        assert_eq!(
            ColorMode::from_env(false, Some("truecolor")),
            ColorMode::TrueColor
        );
        assert_eq!(
            ColorMode::from_env(false, Some("24bit")),
            ColorMode::TrueColor
        );
        assert_eq!(ColorMode::from_env(false, Some("yes")), ColorMode::Indexed);
        assert_eq!(ColorMode::from_env(false, None), ColorMode::Indexed);
    }

    /// The mouse's colours (the swatches) go to the nearest of the 256.
    #[test]
    fn swatches_take_the_nearest_of_256() {
        assert_eq!(nearest_indexed([0xff, 0, 0]), 196);
        assert_eq!(nearest_indexed([0, 0xff, 0]), 46);
        assert_eq!(nearest_indexed([0xff, 0x80, 0]), 208);
        assert_eq!(nearest_indexed([0xff; 3]), 231);
        assert_eq!(nearest_indexed([0x80; 3]), 244);
        assert_eq!(xterm_rgb(196), [0xff, 0, 0]);
        assert_eq!(xterm_rgb(244), [0x80; 3]);
        assert_eq!(indexed(RED), Color::Indexed(203), "the palette's stand-in");
    }

    /// Under `NO_COLOR` nothing is coloured; the focused row shows in
    /// reverse video instead of the red band.
    #[test]
    fn no_color_draws_no_colours() {
        let model = app::find_model(app::DEMO_MODEL).unwrap();
        let app = demo(model);
        let buffer = draw_in(&app, 80, 24, ColorMode::None);
        assert!(
            buffer
                .content()
                .iter()
                .all(|c| c.fg == Color::Reset && c.bg == Color::Reset)
        );
        let focused = screen_text(&buffer)
            .lines()
            .position(|l| l.contains("▌ DPI stage 1"))
            .unwrap();
        let row = u16::try_from(focused).unwrap();
        assert!(buffer[(5, row)].modifier.contains(Modifier::REVERSED));
        assert!(!buffer[(5, row + 1)].modifier.contains(Modifier::REVERSED));
    }

    /// A lift-off change also resets the sensor calibration; say so first.
    #[test]
    fn lift_off_edit_warns_of_calibration_reset() {
        let model = app::find_model(app::DEMO_MODEL).unwrap();
        let mut app = demo(model);
        while app.focused() != Some(Control::LiftOff) {
            app.handle_key(KeyCode::Down, KeyModifiers::NONE);
        }
        app.handle_key(KeyCode::Right, KeyModifiers::NONE);
        let screen = render(&app, 80, 24);
        let row = screen.lines().find(|l| l.contains("Lift-off")).unwrap();
        assert!(
            row.contains("◀ High ▶  Enter to apply; resets sensor calibration"),
            "{row}"
        );
    }

    #[test]
    fn locked_motion_sync_says_why() {
        let model = app::find_model(app::DEMO_MODEL).unwrap();
        let mut app = demo(model);
        app.simulate(&crate::protocol::Change::PollingRate(
            crate::protocol::PollingRate::HZ_8000,
        ));
        let screen = render(&app, 80, 24);
        let row = screen.lines().find(|l| l.contains("Motion sync")).unwrap();
        assert!(
            row.contains('🔒') && row.contains("not at 8000 Hz"),
            "{screen}"
        );
        let buffer = draw_to_buffer(&app, 80, 24);
        let y = (0..24).find(|&y| buffer[(3, y)].symbol() == "M").unwrap();
        assert_eq!(buffer[(3, y)].fg, GREY, "the label is dimmed too");
    }

    /// The stages' swatches line up whatever the DPI's digits.
    #[test]
    fn stage_swatches_line_up() {
        let model = app::find_model(app::DEMO_MODEL).unwrap();
        let screen = render(&demo(model), 80, 24);
        let columns: Vec<usize> = screen
            .lines()
            .filter(|l| l.contains("DPI stage"))
            .map(|l| l.chars().position(|c| c == '■').unwrap())
            .collect();
        assert!(columns.windows(2).all(|w| w[0] == w[1]), "{screen}");
    }
}
