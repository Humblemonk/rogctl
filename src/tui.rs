//! `rogctl settings`: the terminal, the event loop, and [`apply_action`], the only
//! place that changes a mouse's settings.

use std::io::{self, IsTerminal};
use std::path::PathBuf;
use std::process::ExitCode;
use std::time::{Duration, Instant};

use ratatui::DefaultTerminal;
use ratatui::crossterm::event::{self, DisableMouseCapture, Event, KeyEventKind};
use ratatui::crossterm::{Command, ExecutableCommand};

use crate::app::{self, Action, App, Link};
use crate::device::{self, ApplyError, Device, QueryError};
use crate::models::MODELS;
use crate::protocol::{MouseSettings, Reading};
use crate::ui;

/// How long to wait for a key before checking on the mouse.
const TICK: Duration = Duration::from_millis(250);
/// How often to look for an unplugged or replugged mouse.
const PRESENCE_CHECK: Duration = Duration::from_secs(2);
/// How often to try a sleeping mouse again, as `watch` does after an error.
const ASLEEP_RETRY: Duration = Duration::from_secs(5);
/// How often to read the battery for the header while the mouse is connected,
/// so a cable being plugged in shows as charging.
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
            let (reading, settings) = app::demo_state(&model.features);
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
    loop {
        let mut hits = ui::Hits::default();
        terminal.draw(|frame| hits = ui::draw(frame, app))?;
        if event::poll(TICK)? {
            let action = match event::read()? {
                Event::Key(key) if key.kind == KeyEventKind::Press => {
                    app.handle_key(key.code, key.modifiers)
                }
                Event::Mouse(mouse) => {
                    app.handle_mouse(mouse.kind, hits.target_at(mouse.column, mouse.row))
                }
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

/// Carries out what a key asked for. The only caller of
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
            let applied = conn.apply(&change);
            // Read back either way, so the screen shows what the mouse has.
            let outcome = read(&mut conn);
            drop(conn);
            session.show_outcome(app, outcome);
            match applied {
                Ok(()) => app.ok(done),
                Err(ApplyError::NotChanged(e)) if e.means_asleep(dev.model.wireless) => {
                    app.error(format!("No change made: the mouse didn't answer ({e})"));
                }
                Err(ApplyError::NotChanged(e)) => app.error(format!("No change made: {e}")),
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
    last_check: Option<Instant>,
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

    fn show_outcome(&self, app: &mut App, outcome: Result<Outcome, QueryError>) {
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

    /// Reads the battery again for the header, keeping the settings and any
    /// pending edit.
    fn read_battery(&mut self, app: &mut App) {
        self.last_battery = Some(Instant::now());
        let Some(dev) = self.devices.get(self.current) else {
            return;
        };
        match dev.read_battery() {
            Ok(reading) if !reading.is_asleep() => app.reading = Some(reading),
            Ok(_) => self.show_outcome(app, Ok(Outcome::Asleep)),
            Err(e) if e.means_asleep(dev.model.wireless) => self.show_outcome(app, Err(e)),
            // A missed read isn't worth losing the screen over; the next retries.
            Err(_) => {}
        }
    }

    /// Keeps the battery current and notices a mouse being unplugged, plugged
    /// in or waking up. Besides the battery, only looks at sysfs, except to
    /// read the mice again when that changes, or when the mouse was missing or
    /// asleep.
    fn tick(&mut self, app: &mut App) {
        if app.link == Link::Connected
            && self
                .last_battery
                .is_none_or(|t| t.elapsed() >= BATTERY_POLL)
        {
            self.read_battery(app);
        }
        let wait = match app.link {
            Link::Asleep => ASLEEP_RETRY,
            Link::Connected | Link::Disconnected | Link::Error(_) => PRESENCE_CHECK,
        };
        if self.last_check.is_some_and(|t| t.elapsed() < wait) {
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
        match app.link {
            Link::Disconnected | Link::Asleep => self.load(app, &prefer),
            Link::Connected | Link::Error(_) => {
                if changed {
                    self.load(app, &prefer);
                }
            }
        }
    }
}
