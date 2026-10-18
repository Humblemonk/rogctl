//! `rogctl settings`: the terminal, the event loop, and [`apply_action`], the only
//! place that changes a mouse's settings.

use std::io::{self, IsTerminal};
use std::path::PathBuf;
use std::process::ExitCode;
use std::time::{Duration, Instant};

use ratatui::DefaultTerminal;
use ratatui::crossterm::event::{self, DisableMouseCapture, Event, KeyEventKind};
use ratatui::crossterm::{Command, ExecutableCommand};
use ratatui::layout::Size;

use crate::ASLEEP_RETRY;
use crate::app::{self, Action, App, Link};
use crate::device::{self, ApplyError, Device, QueryError};
use crate::models::MODELS;
use crate::protocol::{MouseSettings, Reading};
use crate::ui::{self, ColorMode};

/// How long to wait for a key before checking on the mouse.
const TICK: Duration = Duration::from_millis(250);
/// How often to look for an unplugged or replugged mouse.
const PRESENCE_CHECK: Duration = Duration::from_secs(2);
/// How often to read the battery for the header, and the active DPI stage,
/// while the mouse is connected, so a cable being plugged in shows as charging
/// and a press of the mouse's DPI button shows too.
const BATTERY_POLL: Duration = Duration::from_secs(10);

/// `demo`: `Some` runs without a mouse, as the named model or the default.
pub fn run(demo: Option<Option<String>>) -> ExitCode {
    let demo_model = match demo {
        Some(name) => {
            let name = name.as_deref().unwrap_or(app::DEMO_MODEL);
            let Some(model) = app::find_model(name) else {
                let mut names: Vec<&str> = Vec::new();
                for m in MODELS {
                    if !names.contains(&m.name) {
                        names.push(m.name);
                    }
                }
                eprintln!("Unknown model {name:?}. --demo takes one of:");
                for name in names {
                    eprintln!("  {name}");
                }
                return ExitCode::FAILURE;
            };
            Some(model)
        }
        None => None,
    };
    if !io::stdin().is_terminal() || !io::stdout().is_terminal() {
        eprintln!("rogctl settings needs a terminal");
        return ExitCode::FAILURE;
    }

    let mut app = App::new(demo_model.is_some());
    let mut session = Session::default();
    match demo_model {
        Some(model) => {
            let (mut reading, settings) = app::demo_state(&model.features);
            // A mouse on its cable charges.
            reading.charging = !model.wireless;
            app.show(Some(model), Link::Connected, Some(reading), settings);
            app.devices = 1;
        }
        None => session.load(&mut app, &[]),
    }

    let mut terminal = match ratatui::try_init() {
        Ok(terminal) => terminal,
        Err(e) => {
            ratatui::restore();
            eprintln!("rogctl settings couldn't set up the terminal: {e}");
            return ExitCode::FAILURE;
        }
    };
    // try_init's panic hook restores the terminal; stop the mouse first.
    let restore_terminal = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        release_mouse();
        restore_terminal(info);
    }));
    let result = io::stdout()
        .execute(EnableClickCapture)
        .and_then(|_| event_loop(&mut terminal, &mut app, &mut session));
    release_mouse();
    ratatui::restore();
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("rogctl settings: {e}");
            ExitCode::FAILURE
        }
    }
}

/// Mouse reporting for clicks and the wheel only. crossterm's
/// `EnableMouseCapture` also reports every pointer move, each of which would
/// wake the loop for a redraw. `DisableMouseCapture` turns either off.
struct EnableClickCapture;

impl Command for EnableClickCapture {
    fn write_ansi(&self, f: &mut impl std::fmt::Write) -> std::fmt::Result {
        // Presses, releases and the wheel, with SGR coordinates so columns
        // past 223 report correctly.
        f.write_str("\x1b[?1000h\x1b[?1006h")
    }
}

/// Best effort: it runs on the way out, when there's nothing left to report
/// an error to.
fn release_mouse() {
    let _ = io::stdout().execute(DisableMouseCapture);
}

fn event_loop(
    terminal: &mut DefaultTerminal,
    app: &mut App,
    session: &mut Session,
) -> io::Result<()> {
    let colors = ColorMode::detect();
    loop {
        let (size, hits) = draw(terminal, app, colors)?;
        if event::poll(TICK)? {
            let event = event::read()?;
            // Every event acts on the screen as drawn, the one the user saw,
            // even if a resize comes next. A click's position is only good
            // on that screen, so it's dropped once the terminal was resized.
            let resized = matches!(event, Event::Mouse(_)) && terminal.size()? != size;
            let action = match event {
                Event::Key(key) if key.kind == KeyEventKind::Press => match hits {
                    Some(_) => app.handle_key(key.code, key.modifiers),
                    None => {
                        app.handle_key_too_small(key.code, key.modifiers);
                        None
                    }
                },
                // Nothing on the too-small notice is clickable, and a click
                // would still change focus or a pending edit.
                Event::Mouse(_) if resized => None,
                Event::Mouse(mouse) => hits.and_then(|hits| {
                    app.handle_mouse(mouse.kind, hits.target_at(mouse.column, mouse.row))
                }),
                // Key releases, focus changes, pastes; a resize redraws anyway.
                _ => None,
            };
            if let Some(action) = action {
                apply_action(app, session, action);
            }
        }
        if app.quit {
            return Ok(());
        }
        if !app.demo {
            session.tick(app);
        }
    }
}

/// Draws the screen: the terminal size it was drawn at, and `ui::draw()`'s
/// hits (`None` while the terminal is too small).
fn draw(
    terminal: &mut DefaultTerminal,
    app: &App,
    colors: ColorMode,
) -> io::Result<(Size, Option<ui::Hits>)> {
    let mut hits = None;
    let frame = terminal.draw(|frame| hits = ui::draw(frame, app, colors))?;
    Ok((frame.area.as_size(), hits))
}

/// Carries out what a key or click asked for. The only caller of
/// `Connection::apply`, so the only code that changes the mouse.
fn apply_action(app: &mut App, session: &mut Session, action: Action) {
    match action {
        Action::Refresh if app.demo => app.ok("Demo: nothing to read"),
        Action::Refresh => {
            let current = session.current_path();
            session.load(app, current.as_slice());
            if app.link == Link::Connected {
                app.ok("Read the settings again");
            }
        }
        Action::NextDevice => {
            if !session.devices.is_empty() {
                session.current = (session.current + 1) % session.devices.len();
                session.show_current(app);
            }
        }
        Action::Apply { change, done } if app.demo => {
            app.simulate(&change);
            app.ok(done);
        }
        Action::Apply { change, done } => {
            let Some(dev) = session.devices.get(session.current) else {
                app.error("No mouse connected");
                return;
            };
            let mut conn = match dev.connect() {
                Ok(conn) => conn,
                Err(e) => {
                    app.error(format!("Couldn't open {}: {e}", dev.hidraw.display()));
                    return;
                }
            };
            let wireless = dev.model.wireless;
            let applied = conn.apply(&change);
            // Read back either way, so the screen shows what the mouse has.
            let outcome = read(&mut conn);
            drop(conn);
            session.show_outcome(app, outcome);
            match applied {
                Ok(()) => app.ok(done),
                Err(ApplyError::NotChanged(e)) if e.means_asleep(wireless) => {
                    app.error(format!("No change made: the mouse didn't answer ({e})"));
                }
                Err(ApplyError::NotChanged(e)) => app.error(format!("No change made: {e}")),
                // The read-back found the mouse asleep: that, not the save,
                // is what the user can act on.
                Err(ApplyError::NotSaved(_)) if app.link == Link::Asleep => app.error(format!(
                    "{done}, but the mouse went to sleep or out of range before confirming \
                     the save"
                )),
                // A Harpe II Ace keeps such a change (protocol.md); another
                // mouse may not.
                Err(ApplyError::NotSaved(QueryError::Rejected)) => app.error(format!(
                    "{done}, but the mouse didn't confirm saving it: it may not last a power cycle"
                )),
                Err(ApplyError::NotSaved(e)) => {
                    app.error(format!("{done}, but not saved on the mouse: {e}"));
                }
            }
        }
    }
}

enum Outcome {
    Awake(Reading, MouseSettings),
    Asleep,
}

fn read(conn: &mut device::Connection) -> Result<Outcome, QueryError> {
    let reading = conn.read_battery()?;
    if reading.is_asleep() {
        return Ok(Outcome::Asleep);
    }
    let settings = conn.read_settings(&reading)?;
    Ok(Outcome::Awake(reading, settings))
}

#[derive(Default)]
struct Session {
    devices: Vec<Device>,
    /// `device::candidate_nodes()` as of the last load, to notice a change.
    nodes: Vec<PathBuf>,
    current: usize,
    /// When sysfs was last rescanned.
    last_check: Option<Instant>,
    /// When the mouse was last read (or a load found none).
    last_battery: Option<Instant>,
}

impl Session {
    fn current_path(&self) -> Option<PathBuf> {
        self.devices.get(self.current).map(|d| d.hidraw.clone())
    }

    /// Finds the mice and shows the first one that answers, trying `prefer`
    /// first. With both the cable and the receiver plugged in, usually only
    /// one has live data.
    fn load(&mut self, app: &mut App, prefer: &[PathBuf]) {
        // Nodes first: one plugged in between the two is caught next tick.
        self.nodes = device::candidate_nodes();
        self.devices = device::discover();
        self.last_check = Some(Instant::now());
        self.last_battery = self.last_check;
        app.devices = self.devices.len();
        if self.devices.is_empty() {
            app.show(None, Link::Disconnected, None, MouseSettings::default());
            return;
        }
        // `prefer` first, in its order; the rest keep theirs.
        let mut order: Vec<usize> = (0..self.devices.len()).collect();
        order.sort_by_key(|&i| {
            prefer
                .iter()
                .position(|p| *p == self.devices[i].hidraw)
                .unwrap_or(usize::MAX)
        });
        let mut fallback: Option<(usize, Result<Outcome, QueryError>)> = None;
        for i in order {
            let outcome = self.devices[i]
                .connect()
                .map_err(QueryError::from)
                .and_then(|mut conn| read(&mut conn));
            if matches!(outcome, Ok(Outcome::Awake(..))) {
                self.current = i;
                self.show_outcome(app, outcome);
                return;
            }
            // Prefer showing a sleeping mouse over another node's error.
            if fallback.as_ref().is_none_or(|(_, f)| f.is_err()) {
                fallback = Some((i, outcome));
            }
        }
        if let Some((i, outcome)) = fallback {
            self.current = i;
            self.show_outcome(app, outcome);
        }
    }

    /// Reads and shows the current device only, for Tab.
    fn show_current(&mut self, app: &mut App) {
        let Some(dev) = self.devices.get(self.current) else {
            return;
        };
        let outcome = dev
            .connect()
            .map_err(QueryError::from)
            .and_then(|mut conn| read(&mut conn));
        self.show_outcome(app, outcome);
    }

    /// Shows what was just read from the current mouse.
    fn show_outcome(&mut self, app: &mut App, outcome: Result<Outcome, QueryError>) {
        self.last_battery = Some(Instant::now());
        let Some(dev) = self.devices.get(self.current) else {
            return;
        };
        let model = Some(dev.model);
        let empty = MouseSettings::default;
        match outcome {
            Ok(Outcome::Awake(reading, settings)) => {
                app.show(model, Link::Connected, Some(reading), settings);
            }
            Ok(Outcome::Asleep) => app.show(model, Link::Asleep, None, empty()),
            Err(e) if e.means_asleep(dev.model.wireless) => {
                app.show(model, Link::Asleep, None, empty());
            }
            Err(e) => {
                let error = format!("{}: {e}", dev.hidraw.display());
                app.show(model, Link::Error(error), None, empty());
            }
        }
    }

    /// Reads the battery for the header and the active DPI stage, which the
    /// mouse's DPI button changes without telling rogctl, keeping the other
    /// settings and any pending edit.
    fn poll_mouse(&mut self, app: &mut App) {
        self.last_battery = Some(Instant::now());
        let Some(dev) = self.devices.get(self.current) else {
            return;
        };
        let read = dev
            .connect()
            .map_err(QueryError::from)
            .and_then(|mut conn| {
                let reading = conn.read_battery()?;
                // A missed stage read leaves the stage shown; the next retries.
                let stage = if reading.is_asleep() {
                    None
                } else {
                    conn.read_active_stage().ok().flatten()
                };
                Ok((reading, stage))
            });
        match read {
            Ok((reading, stage)) if !reading.is_asleep() => {
                app.reading = Some(reading);
                if let Some(stage) = stage.filter(|&s| s < app.settings.stages.len()) {
                    app.settings.active_stage = Some(stage);
                }
            }
            Ok(_) => self.show_outcome(app, Ok(Outcome::Asleep)),
            Err(e) if e.means_asleep(dev.model.wireless) => self.show_outcome(app, Err(e)),
            // A missed read isn't worth losing the screen over; the next retries.
            Err(_) => {}
        }
    }

    /// Keeps the battery current and notices a mouse being unplugged, plugged
    /// in or waking up. Besides the battery and active stage, only looks at
    /// sysfs, except to read the mice again when that changes, or when the
    /// mouse was missing, asleep or failed. That shows whichever mouse
    /// answers, even after Tab.
    fn tick(&mut self, app: &mut App) {
        if app.link == Link::Connected
            && self
                .last_battery
                .is_none_or(|t| t.elapsed() >= BATTERY_POLL)
        {
            self.poll_mouse(app);
        }
        if self
            .last_check
            .is_some_and(|t| t.elapsed() < PRESENCE_CHECK)
        {
            return;
        }
        self.last_check = Some(Instant::now());
        let nodes = device::candidate_nodes();
        let changed = nodes != self.nodes;
        // Show what was just plugged in (the cable, say) if it answers, then
        // the mouse already shown.
        let mut prefer: Vec<PathBuf> = nodes
            .into_iter()
            .filter(|n| !self.nodes.contains(n))
            .collect();
        prefer.extend(self.current_path());
        let reload = changed
            || match app.link {
                Link::Disconnected => true,
                // Rescans every PRESENCE_CHECK, but reads a sleeping mouse, or
                // one that failed, less often.
                Link::Asleep | Link::Error(_) => self
                    .last_battery
                    .is_none_or(|t| t.elapsed() >= ASLEEP_RETRY),
                Link::Connected => false,
            };
        if reload {
            self.load(app, &prefer);
        }
    }
}
