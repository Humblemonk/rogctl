//! `rogctl settings`: the terminal, the event loop, and [`apply_action`], which
//! changes a mouse's settings on Enter, Space or a click.

use std::io::{self, IsTerminal, Write};
use std::path::PathBuf;
use std::process::ExitCode;
use std::time::{Duration, Instant};

use ratatui::DefaultTerminal;
use ratatui::crossterm::event::{
    self, DisableBracketedPaste, DisableMouseCapture, EnableBracketedPaste, Event, KeyEventKind,
};
use ratatui::crossterm::{Command, ExecutableCommand};
use ratatui::layout::Size;

use crate::ASLEEP_RETRY;
use crate::app::{self, Action, App, Link};
use crate::device::{self, ApplyError, Device, Model, Outcome, QueryError};
use crate::protocol::MouseSettings;
use crate::settings::{self, Report};
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
    let demo_model = match demo.map(|name| app::demo_model(name.as_deref())) {
        Some(Ok(model)) => Some(model),
        Some(Err(e)) => {
            let _ = io::stderr().write_all(e.as_bytes());
            return ExitCode::FAILURE;
        }
        None => None,
    };
    if !io::stdin().is_terminal() || !io::stdout().is_terminal() {
        complain!("rogctl settings needs a terminal");
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

    // From here on a SIGTERM, SIGHUP or SIGINT waits for the event loop,
    // which leaves the terminal as it found it.
    let exit_signals = block_exit_signals();
    let mut terminal = match ratatui::try_init() {
        Ok(terminal) => terminal,
        Err(e) => {
            ratatui::restore();
            complain!("rogctl settings couldn't set up the terminal: {e}");
            return ExitCode::FAILURE;
        }
    };
    // try_init's panic hook restores the terminal; stop the mouse and
    // bracketed paste first.
    let restore_terminal = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        release_input();
        restore_terminal(info);
    }));
    // A paste arrives as one `Event::Paste`, which does nothing, rather than
    // as key presses: in "hello world" h and l would step a value and the
    // space apply it.
    let result = io::stdout()
        .execute(EnableClickCapture)
        .and_then(|out| out.execute(EnableBracketedPaste))
        .and_then(|_| event_loop(&mut terminal, &mut app, &mut session, &exit_signals));
    release_input();
    ratatui::restore();
    match result {
        Ok(None) => ExitCode::SUCCESS,
        // As a shell reports a process the signal ended.
        Ok(Some(signal)) => u8::try_from(128 + signal).map_or(ExitCode::FAILURE, ExitCode::from),
        Err(e) => {
            complain!("rogctl settings: {e}");
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

/// Stops mouse reporting and bracketed paste. Best effort: it runs on the way
/// out, when there's nothing left to report an error to.
fn release_input() {
    let _ = io::stdout().execute(DisableMouseCapture);
    let _ = io::stdout().execute(DisableBracketedPaste);
}

/// The signals that end `rogctl settings` from outside (a closed terminal
/// window, `kill`), blocked so they wait for the event loop, which restores
/// the terminal. In raw mode Ctrl+C is a key, not SIGINT.
fn block_exit_signals() -> libc::sigset_t {
    // SAFETY: plain libc calls on a local, zero-initialized sigset_t. The TUI
    // is single-threaded, so blocking them for this thread is for the process.
    unsafe {
        let mut set: libc::sigset_t = std::mem::zeroed();
        libc::sigemptyset(&mut set);
        for signal in [libc::SIGTERM, libc::SIGHUP, libc::SIGINT] {
            libc::sigaddset(&mut set, signal);
        }
        libc::pthread_sigmask(libc::SIG_BLOCK, &set, std::ptr::null_mut());
        set
    }
}

/// One of `set` if it has arrived, without waiting.
fn pending_signal(set: &libc::sigset_t) -> Option<libc::c_int> {
    let now = libc::timespec {
        tv_sec: 0,
        tv_nsec: 0,
    };
    // SAFETY: `set` is a valid sigset_t and `now` outlives the call.
    let got = unsafe { libc::sigtimedwait(set, std::ptr::null_mut(), &now) };
    // -1: none (EAGAIN), or EINTR, which the next check retries.
    (got > 0).then_some(got)
}

/// Runs until `q` or Ctrl+C (None) or one of `exit_signals` (Some, the
/// signal).
fn event_loop(
    terminal: &mut DefaultTerminal,
    app: &mut App,
    session: &mut Session,
    exit_signals: &libc::sigset_t,
) -> io::Result<Option<libc::c_int>> {
    let colors = ColorMode::detect();
    // Keys typed while the mice were first read (an extra Enter after the
    // command, say) were typed at no screen at all.
    let mut waited = !app.demo;
    loop {
        if waited {
            discard_input()?;
        }
        let (size, hits) = draw(terminal, app, colors)?;
        let ready = event::poll(TICK)?;
        // Before acting on a key, so nothing is sent on the way out.
        if let Some(signal) = pending_signal(exit_signals) {
            return Ok(Some(signal));
        }
        waited = false;
        if ready {
            let event = event::read()?;
            // Every event acts on the screen as drawn, the one the user saw,
            // even if a resize comes next. A click's position is only good
            // on that screen, so it's dropped once the terminal was resized.
            let resized = matches!(event, Event::Mouse(_)) && terminal.size()? != size;
            if let Some(action) = action_for(app, event, hits.as_ref(), resized) {
                waited = apply_action(app, session, action);
            }
        }
        if app.quit {
            return Ok(None);
        }
        if !app.demo {
            waited |= session.tick(app);
        }
    }
}

/// Drops the input that came in while the loop waited on the mouse and the
/// wait replaced the screen. It was typed at the screen as drawn before: an
/// Enter meant for a DPI edit a reload dropped would make the stage active
/// instead, a second Space while a switch is applied would turn it back, and
/// a key after Tab would act on the next mouse before it's shown. The user
/// sees nothing happen and presses again. A resize is dropped too, which
/// does no harm: every draw fits the terminal's size at the time.
fn discard_input() -> io::Result<()> {
    while event::poll(Duration::ZERO)? {
        event::read()?;
    }
    Ok(())
}

/// What `event` asks for on the screen as drawn: `hits` is None on the
/// too-small notice, and `resized` says a click came after a resize.
fn action_for(
    app: &mut App,
    event: Event,
    hits: Option<&ui::Hits>,
    resized: bool,
) -> Option<Action> {
    match event {
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
        Event::Mouse(mouse) => hits
            .and_then(|hits| app.handle_mouse(mouse.kind, hits.target_at(mouse.column, mouse.row))),
        // Pasted text is never taken as keys: it would step and apply
        // values, and a newline is Enter.
        Event::Paste(_) => None,
        // Key releases, focus changes; a resize redraws anyway.
        _ => None,
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

/// Carries out what a key or click asked for. A change goes through
/// `Device::apply`, the only code that changes the mouse. True if it waited
/// on the mouse, so the input that came in meanwhile is dropped.
fn apply_action(app: &mut App, session: &mut Session, action: Action) -> bool {
    match action {
        Action::Refresh if app.demo => app.ok("Demo: nothing to read"),
        Action::Refresh => {
            let current = session.current_path();
            session.load(app, current.as_slice());
            if app.link == Link::Connected {
                app.ok("Read the settings again");
            }
            return true;
        }
        Action::NextDevice => {
            if session.devices.is_empty() {
                return false;
            }
            session.current = (session.current + 1) % session.devices.len();
            session.show_current(app);
            return true;
        }
        Action::Apply { change, done } if app.demo => {
            app.simulate(&change);
            app.ok(done);
        }
        Action::Apply { change, done } => {
            let Some(dev) = session.devices.get(session.current) else {
                app.error("No mouse connected");
                return false;
            };
            let (wireless, hidraw) = (dev.model.wireless, dev.hidraw.clone());
            let before = app.clone();
            let applied = dev.apply(&change);
            if matches!(applied.result, Err(ApplyError::Replaced)) {
                let current = session.current_path();
                session.load(app, current.as_slice());
                app.error(
                    "No change made: the mouse on this port changed or didn't answer, so it was read again",
                );
                return true;
            }
            if let Some(outcome) = applied.read_back {
                session.show_outcome(app, outcome);
            }
            let asleep = app.link == Link::Asleep;
            let held = settings::read_back_holds(&before, &change, app);
            match settings::describe(&applied.result, &done, wireless, &hidraw, asleep, held) {
                Report::Done(text) => app.ok(text),
                Report::Unsaved(text) | Report::Failed { text, .. } => app.error(text),
            }
            return true;
        }
    }
    false
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
    /// The last poll of the mouse shown failed: one more failure shows the
    /// error, and the retry reads the mice again.
    missed_poll: bool,
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
        self.devices = device::discover_mice();
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
        if let Some((i, outcome)) = device::first_answering(&self.devices, order) {
            self.current = i;
            self.show_outcome(app, outcome);
        }
    }

    /// Reads and shows the current device only, for Tab.
    fn show_current(&mut self, app: &mut App) {
        let Some(dev) = self.devices.get(self.current) else {
            return;
        };
        let outcome = dev.read();
        self.show_outcome(app, outcome);
    }

    /// Shows what was just read from the current mouse.
    fn show_outcome(&mut self, app: &mut App, outcome: Result<Outcome, QueryError>) {
        self.last_battery = Some(Instant::now());
        self.missed_poll = false;
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
    ///
    /// True if the poll replaced the screen (the mouse went to sleep or
    /// failed): only then do keys typed during it no longer mean what they
    /// meant. A poll that keeps the screen keeps them too.
    fn poll_mouse(&mut self, app: &mut App) -> bool {
        self.last_battery = Some(Instant::now());
        let Some(dev) = self.devices.get(self.current) else {
            return false;
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
                self.missed_poll = false;
                app.polled(reading, stage);
                return false;
            }
            Ok(_) => self.show_outcome(app, Ok(Outcome::Asleep)),
            Err(e) if e.means_asleep(dev.model.wireless) => self.show_outcome(app, Err(e)),
            // Twice in a row (a cable that answers `FF AA` or EIO once the
            // mouse is back on its receiver), as a failed read: shown, and
            // read again on the retry, which finds the node that answers.
            Err(e) if self.missed_poll => self.show_outcome(app, Err(e)),
            // Once isn't worth losing the screen and a pending edit over.
            Err(_) => {
                self.missed_poll = true;
                return false;
            }
        }
        true
    }

    /// Keeps the battery current and notices a mouse being unplugged, plugged
    /// in or waking up. Besides the battery and active stage, only looks at
    /// sysfs, except to read the mice again when that changes, or when the
    /// mouse was missing, asleep or failed, or is a receiver rogctl couldn't
    /// ask which mouse it holds. That shows whichever mouse answers, even
    /// after Tab. True if it replaced the screen, so keys typed meanwhile
    /// are dropped (`discard_input()`).
    fn tick(&mut self, app: &mut App) -> bool {
        let stand_in = app.link == Link::Connected && app.model.is_some_and(Model::is_stand_in);
        let replaced = app.link == Link::Connected
            && !stand_in
            && self
                .last_battery
                .is_none_or(|t| t.elapsed() >= BATTERY_POLL)
            && self.poll_mouse(app);
        if self
            .last_check
            .is_some_and(|t| t.elapsed() < PRESENCE_CHECK)
        {
            return replaced;
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
                // The receiver's stand-in: its pairing query failed (as it
                // can right after it's plugged in), so ask it again.
                Link::Connected => {
                    stand_in
                        && self
                            .last_battery
                            .is_none_or(|t| t.elapsed() >= ASLEEP_RETRY)
                }
            };
        if reload {
            self.load(app, &prefer);
        }
        replaced || reload
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::protocol;
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;
    use ratatui::crossterm::event::{KeyCode, KeyEvent};

    fn demo() -> App {
        let model = app::demo_model(None).unwrap();
        let (reading, settings) = app::demo_state(&model.features);
        let mut app = App::new(true);
        app.show(Some(model), Link::Connected, Some(reading), settings);
        app
    }

    fn hits(app: &App) -> ui::Hits {
        let mut terminal = Terminal::new(TestBackend::new(80, 24)).unwrap();
        let mut hits = None;
        terminal
            .draw(|frame| hits = ui::draw(frame, app, ColorMode::TrueColor))
            .unwrap();
        hits.unwrap()
    }

    /// Typed, "hello world" steps the DPI with h and l and applies it with
    /// the space. Pasted, it does nothing at all.
    #[test]
    fn a_paste_is_not_taken_as_keys() {
        let text = "hello world\n";
        let mut typed = demo();
        let hits = hits(&typed);
        let actions: Vec<Action> = text
            .chars()
            .map(|c| match c {
                '\n' => KeyCode::Enter,
                c => KeyCode::Char(c),
            })
            .filter_map(|code| {
                let key = Event::Key(KeyEvent::from(code));
                action_for(&mut typed, key, Some(&hits), false)
            })
            .collect();
        assert!(
            actions.iter().any(|a| matches!(a, Action::Apply { .. })),
            "typed: {actions:?}"
        );

        let mut pasted = demo();
        let before = (pasted.focused(), pasted.edit, pasted.message.clone());
        let paste = Event::Paste(text.into());
        assert_eq!(action_for(&mut pasted, paste, Some(&hits), false), None);
        assert_eq!(
            (pasted.focused(), pasted.edit, pasted.message.clone()),
            before
        );
        assert!(!pasted.quit);
    }

    /// Why input that came in while the loop waited on the mouse is
    /// dropped: a reload (`App::show()`) drops the DPI being typed but keeps
    /// the focus, so the Enter meant to apply 1600 to stage 1 would make
    /// stage 1 the active one instead.
    #[test]
    fn a_reload_changes_what_a_queued_enter_means() {
        let mut app = demo();
        let press = |app: &mut App, code| {
            let key = Event::Key(KeyEvent::from(code));
            let hits = hits(app);
            action_for(app, key, Some(&hits), false)
        };
        assert_eq!(app.settings.active_stage, Some(1));
        for c in "1600".chars() {
            press(&mut app, KeyCode::Char(c));
        }
        assert_eq!(app.edit.map(|e| e.value), Some(app::Value::Dpi(1600)));
        let (model, reading, settings) = (app.model, app.reading, app.settings.clone());
        app.show(model, Link::Connected, reading, settings);
        assert_eq!(
            press(&mut app, KeyCode::Enter),
            Some(Action::Apply {
                change: protocol::Change::ActiveStage(0),
                done: "DPI stage 1 is active".into()
            })
        );
    }

    /// Nothing waits on a mouse in demo mode, or without one, so keys typed
    /// ahead there are kept.
    #[test]
    fn only_waiting_on_the_mouse_drops_input() {
        let mut app = demo();
        let mut session = Session::default();
        let change = protocol::Change::AngleSnapping(true);
        for action in [
            Action::Refresh,
            Action::NextDevice,
            Action::Apply {
                change,
                done: String::new(),
            },
        ] {
            assert!(
                !apply_action(&mut app, &mut session, action.clone()),
                "{action:?}"
            );
        }
        app.demo = false;
        assert!(!apply_action(&mut app, &mut session, Action::NextDevice));
        let apply = Action::Apply {
            change,
            done: String::new(),
        };
        assert!(!apply_action(&mut app, &mut session, apply));
        assert_eq!(
            app.message.map(|m| m.text).as_deref(),
            Some("No mouse connected")
        );
    }

    /// A poll that fails once keeps the screen and its pending edit; twice
    /// in a row, it shows the error, so the retry reads the mice again and
    /// finds the node that answers now.
    #[test]
    fn a_poll_failing_twice_shows_the_error() {
        let mut app = demo();
        let model = app.model.unwrap();
        let mut session = Session {
            devices: vec![Device {
                model,
                // Not there: opening it fails with ENOENT, a read error.
                hidraw: PathBuf::from("/nonexistent/rogctl-test/hidraw0"),
            }],
            ..Session::default()
        };
        app.handle_key(
            KeyCode::Right,
            ratatui::crossterm::event::KeyModifiers::NONE,
        );
        let edit = app.edit;
        assert!(edit.is_some());
        // Kept: keys typed during it still mean what they meant.
        assert!(!session.poll_mouse(&mut app), "screen kept");
        assert_eq!((&app.link, app.edit), (&Link::Connected, edit));
        // Replaced: keys typed during it are dropped.
        assert!(session.poll_mouse(&mut app), "screen replaced");
        assert!(matches!(app.link, Link::Error(_)), "{:?}", app.link);
    }

    /// A SIGHUP (the terminal closed) or SIGTERM waits, blocked, until the
    /// event loop asks for it.
    #[test]
    fn exit_signals_wait_to_be_asked_for() {
        let set = block_exit_signals();
        assert_eq!(pending_signal(&set), None);
        // SAFETY: signals this thread, which has SIGHUP and SIGTERM blocked.
        unsafe { libc::pthread_kill(libc::pthread_self(), libc::SIGHUP) };
        assert_eq!(pending_signal(&set), Some(libc::SIGHUP));
        assert_eq!(pending_signal(&set), None, "taken once");
        // SAFETY: as above.
        unsafe { libc::pthread_kill(libc::pthread_self(), libc::SIGTERM) };
        assert_eq!(pending_signal(&set), Some(libc::SIGTERM));
    }
}
