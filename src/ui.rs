//! Draws the TUI from `App`. Every model goes through the same code: rows a
//! model lacks are left out, never drawn differently. The colours follow ASUS
//! GearLink's ROG theme: near-black, white text, grey detail, one red accent.

use ratatui::Frame;
use std::ops::Range;

use ratatui::layout::{Constraint, Layout, Position, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, BorderType, Paragraph, Wrap};

use crate::app::{App, Control, Edit, Link, Row, Target, Value};

/// Settings stay readable on a wide terminal; a full polling-rate scale and
/// its hint fit an 80-column one.
const MAX_WIDTH: u16 = 80;
const LABEL_WIDTH: usize = 24;
/// The widest DPI value, 42000.
const DPI_WIDTH: usize = 5;

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

const BASE: Style = Style::new().fg(TEXT).bg(BACKGROUND);
const MUTED: Style = Style::new().fg(GREY);
const BOLD: Style = Style::new().fg(TEXT).add_modifier(Modifier::BOLD);
const ACCENT: Style = Style::new().fg(RED).add_modifier(Modifier::BOLD);
/// The focused row: a dark red band, as GearLink marks the selected item.
const FOCUSED: Style = Style::new().bg(RED_BAND);

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

/// Draws the screen and returns where its rows and scale values are.
pub fn draw(frame: &mut Frame, app: &App) -> Hits {
    let mut hits = Hits::default();
    frame.render_widget(Block::new().style(BASE), frame.area());
    let area = frame.area();
    let frame_width = area.width;
    let area = Rect {
        width: area.width.min(MAX_WIDTH),
        ..area
    };
    let [header, body, message, footer] = Layout::vertical([
        Constraint::Length(1),
        Constraint::Min(0),
        Constraint::Length(1),
        Constraint::Length(1),
    ])
    .areas(area);

    // The header and footer can outgrow MAX_WIDTH (a long name, Tab, the
    // version); they may use the terminal's whole width.
    let full_width = |r: Rect| Rect {
        width: frame_width,
        ..r
    };
    frame.render_widget(Paragraph::new(header_line(app)), full_width(header));
    match app.link {
        Link::Connected => draw_settings(frame, app, body, &mut hits),
        Link::Asleep | Link::Disconnected | Link::Error(_) => draw_link_notice(frame, app, body),
    }
    if let Some(m) = &app.message {
        // Red is for errors; ✓ and ✗ tell them apart without colour too.
        let line = if m.error {
            Line::styled(format!(" ✗ {}", m.text), ACCENT)
        } else {
            Line::styled(format!(" ✓ {}", m.text), BOLD)
        };
        frame.render_widget(Paragraph::new(line), message);
    }
    frame.render_widget(
        Paragraph::new(footer_line(app, frame_width)),
        full_width(footer),
    );
    if app.help {
        draw_help(frame, area);
    }
    hits
}

/// A bordered section with GearLink's thin grey lines and a white heading.
fn panel(title: &str) -> Block<'static> {
    Block::bordered()
        .border_type(BorderType::Plain)
        .border_style(Style::new().fg(LINE))
        .title(Span::styled(format!(" {title} "), BOLD))
}

fn header_line(app: &App) -> Line<'static> {
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
            format!("  Battery: {}%", r.battery),
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
/// then the keys least needed, so `?` and `q` always show.
fn footer_line(app: &App, width: u16) -> Line<'static> {
    // Only the keys that do something on this screen.
    let mut keys = match app.link {
        Link::Connected => vec![
            ("↑↓", "select"),
            ("←→", "change"),
            ("Enter", "apply"),
            ("Esc", "cancel"),
            ("r", "reload"),
        ],
        Link::Asleep | Link::Disconnected | Link::Error(_) => vec![("r", "retry")],
    };
    if app.devices > 1 {
        keys.push(("Tab", "next mouse"));
    }
    keys.extend([("?", "help"), ("q", "quit")]);
    let line = |keys: &[(&'static str, &'static str)], version: bool| {
        let mut spans = vec![Span::raw(" ")];
        for &(key, what) in keys {
            spans.push(Span::styled(key, BOLD));
            spans.push(Span::styled(format!(" {what}  "), MUTED));
        }
        if version {
            spans.push(Span::styled(concat!("v", env!("CARGO_PKG_VERSION")), MUTED));
        }
        Line::from(spans)
    };
    let fits = |line: &Line| line.width() <= usize::from(width);
    let full = line(&keys, true);
    if fits(&full) {
        return full;
    }
    for least_needed in ["Esc", "r", "←→", "↑↓", "Tab", "Enter"] {
        let shorter = line(&keys, false);
        if fits(&shorter) {
            return shorter;
        }
        keys.retain(|&(key, _)| key != least_needed);
    }
    line(&keys, false)
}

fn draw_settings(frame: &mut Frame, app: &App, area: Rect, hits: &mut Hits) {
    let (battery, performance): (Vec<Row>, Vec<Row>) =
        app.rows().into_iter().partition(|r| r.control.is_battery());
    let height = |rows: &[Row]| {
        if rows.is_empty() {
            0
        } else {
            rows.len() as u16 + 2
        }
    };
    // Cut-off panels could hide the focused row, and a change to it.
    let needed = height(&performance) + height(&battery);
    if needed > area.height {
        let rows = needed + 3;
        draw_notice(
            frame,
            area,
            vec![Line::styled(
                format!("Make the terminal taller: the settings need {rows} rows."),
                BOLD,
            )],
        );
        return;
    }
    let [top, bottom, _] = Layout::vertical([
        Constraint::Length(height(&performance)),
        Constraint::Length(height(&battery)),
        Constraint::Min(0),
    ])
    .areas(area);
    for (title, rows, area) in [
        ("Performance", performance, top),
        ("Battery", battery, bottom),
    ] {
        if rows.is_empty() {
            continue;
        }
        let block = panel(title);
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
    let label = format!("{:<LABEL_WIDTH$}", control.label());
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

/// A value drawn as text: pending edits between red arrows (or with a cursor
/// while DPI digits are typed), switches that are on in red.
fn value_spans(
    row: Row,
    focused: bool,
    edit: Option<Edit>,
    value: Option<Value>,
) -> Vec<Span<'static>> {
    match (edit, value) {
        (Some(Edit { value, typed, .. }), _) => {
            let value = Span::styled(value.to_string(), BOLD.add_modifier(Modifier::UNDERLINED));
            if typed {
                vec![value, Span::styled("▏", ACCENT)]
            } else {
                vec![
                    Span::styled("◀ ", ACCENT),
                    value,
                    Span::styled(" ▶", ACCENT),
                ]
            }
        }
        (None, Some(value)) => {
            let style = match (row.locked, value) {
                (Some(_), _) => MUTED,
                (None, Value::Switch(true)) => ACCENT,
                (None, _) if focused => BOLD,
                (None, _) => Style::new().fg(TEXT),
            };
            let text = match value {
                // Padded so the stages' swatches and "● active" line up.
                Value::Dpi(_) => format!("{:<DPI_WIDTH$}", value.to_string()),
                Value::PollingRate(_)
                | Value::Debounce(_)
                | Value::Switch(_)
                | Value::Degrees(_)
                | Value::LiftOff(_)
                | Value::PowerOff(_)
                | Value::Percent(_) => value.to_string(),
            };
            vec![Span::styled(text, style)]
        }
        (None, None) => vec![Span::styled("unknown", MUTED)],
    }
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
        Value::Percent(pct) => pct.to_string(),
        Value::Dpi(_) | Value::Switch(_) | Value::Degrees(_) | Value::LiftOff(_) => {
            value.to_string()
        }
    }
}

fn unit(value: Value) -> Option<&'static str> {
    match value {
        Value::PollingRate(_) => Some("Hz"),
        Value::Debounce(_) => Some("ms"),
        Value::Percent(_) => Some("%"),
        Value::Dpi(_)
        | Value::Switch(_)
        | Value::Degrees(_)
        | Value::LiftOff(_)
        | Value::PowerOff(_) => None,
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

/// Text in place of the settings. No frame: an empty box the height of the
/// screen would dwarf it.
fn draw_notice(frame: &mut Frame, area: Rect, text: Vec<Line>) {
    let block = Block::new().padding(ratatui::widgets::Padding::new(2, 2, 1, 0));
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
    lines.push(Line::styled(" ? or Esc closes this.", MUTED));
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
    let block = panel("Keys").border_style(Style::new().fg(RED)).style(BASE);
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
        let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
        terminal
            .draw(|frame| {
                draw(frame, app);
            })
            .unwrap();
        terminal.backend().buffer().clone()
    }

    /// The screen as text, one line per row, trailing spaces trimmed.
    fn render(app: &App, width: u16, height: u16) -> String {
        let buffer = draw_to_buffer(app, width, height);
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
        assert!(screen.contains("◀ 450 ▶"), "{screen}");
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

    /// The version ends the footer and fits 80 columns; with Tab too, the
    /// footer uses a wider terminal rather than being cut at MAX_WIDTH.
    #[test]
    fn footer_ends_with_the_version() {
        let model = app::find_model(app::DEMO_MODEL).unwrap();
        let mut app = demo(model);
        let version = concat!("v", env!("CARGO_PKG_VERSION"));
        let footer = |app: &App, width| render(app, width, 24).lines().last().unwrap().to_owned();
        assert!(footer(&app, 80).ends_with(version), "{}", footer(&app, 80));
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
            let mut terminal = Terminal::new(TestBackend::new(80, 40)).unwrap();
            let mut hits = Hits::default();
            terminal.draw(|frame| hits = draw(frame, &app)).unwrap();
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
            for (w, h) in [(1, 1), (20, 5), (80, 24)] {
                let mut terminal = Terminal::new(TestBackend::new(w, h)).unwrap();
                let mut hits = Hits::default();
                terminal.draw(|frame| hits = draw(frame, &app)).unwrap();
                for (area, target) in &hits.0 {
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

    /// A narrow terminal drops the version, then the least needed keys;
    /// `?` and `q` stay.
    #[test]
    fn narrow_footer_keeps_help_and_quit() {
        let model = app::find_model(app::DEMO_MODEL).unwrap();
        let mut app = demo(model);
        app.devices = 2;
        let footer = |app: &App, width| render(app, width, 24).lines().last().unwrap().to_owned();
        for width in [20, 40, 60, 79] {
            let footer = footer(&app, width);
            assert!(footer.ends_with("? help  q quit"), "{width}: {footer:?}");
        }
        assert_eq!(
            footer(&app, 60),
            " ↑↓ select  Enter apply  Tab next mouse  ? help  q quit"
        );
        assert_eq!(footer(&app, 40), " Enter apply  ? help  q quit");
    }

    /// Rather than cut a panel off, and maybe the focused row with it.
    #[test]
    fn short_terminal_asks_for_more_rows() {
        let model = app::find_model(app::DEMO_MODEL).unwrap();
        let app = demo(model);
        assert!(!render(&app, 80, 20).contains("taller"));
        let screen = render(&app, 80, 19);
        assert!(
            screen.contains("need 20 rows") && !screen.contains("DPI"),
            "{screen}"
        );
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
            let Color::Rgb(r, g, b) = c else {
                panic!("not a palette colour: {c:?}");
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
    /// help and on every notice. Lines, swatches and the focus bar aren't text.
    #[test]
    fn text_has_enough_contrast() {
        let check = |app: &App| {
            let buffer = draw_to_buffer(app, 80, 24);
            for cell in buffer.content() {
                let symbol = cell.symbol();
                if symbol.trim().is_empty() || "─│┌┐└┘■▌".contains(symbol) {
                    continue;
                }
                let ratio = contrast(cell.fg, cell.bg);
                assert!(ratio >= 4.5, "{symbol:?} at {ratio:.2}: {cell:?}");
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
            row.contains("◀ High ▶  Enter to apply; resets sensor calibration │"),
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
