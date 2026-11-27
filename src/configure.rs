//! `rogctl configure`: installs or removes a desktop's battery widget. The
//! widget files are built into the binary, so this needs no checkout of the
//! repository, and the widget always matches the binary's version.

use std::ffi::OsString;
use std::fs;
use std::io::{self, IsTerminal, Write};
use std::path::{Component, Path, PathBuf};
use std::process::{Command, ExitCode, Stdio};
use std::time::{SystemTime, UNIX_EPOCH};

/// What every widget is called in its desktop's widget picker.
const WIDGET_NAME: &str = "ASUS Peripheral Battery";
const NOCTALIA_ID: &str = "humblemonk/rog-mouse-battery";
const DMS_ID: &str = "rogMouseBattery";
const KDE_ID: &str = "com.github.humblemonk.rogmousebattery";
const GNOME_UUID: &str = "rog-mouse-battery@humblemonk.github.io";

/// A widget file, at its path inside the widget's directory.
struct File {
    path: &'static str,
    contents: &'static [u8],
}

/// A widget directory of the repository, built in.
struct Bundle {
    #[cfg_attr(not(test), allow(dead_code))]
    dir: &'static str,
    files: &'static [File],
    /// Files among `files` that hold the user's settings: updated and removed
    /// only while unchanged, and once edited kept, even with --force, rather
    /// than counted as edited.
    users: &'static [&'static str],
}

impl Bundle {
    fn is_users(&self, path: &str) -> bool {
        self.users.contains(&path)
    }
}

macro_rules! bundle {
    ($dir:literal, [$($path:literal),+ $(,)?]) => {
        bundle!($dir, [$($path),+], users: [])
    };
    ($dir:literal, [$($path:literal),+ $(,)?], users: [$($user:literal),* $(,)?]) => {
        Bundle {
            dir: $dir,
            files: &[$(File {
                path: $path,
                contents: include_bytes!(concat!("../", $dir, "/", $path)),
            }),+],
            users: &[$($user),*],
        }
    };
}

static NOCTALIA: Bundle = bundle!(
    "noctalia/rog-mouse-battery",
    [
        "battery.luau",
        "plugin.toml",
        "service.luau",
        "translations/en.json",
        "widget.luau",
    ]
);
static WAYBAR: Bundle = bundle!("waybar", ["config.json", "style.css"]);
static QUICKSHELL: Bundle = bundle!(
    "quickshell",
    ["RogMouse.qml", "RogMouseSettings.qml", "RogMouseWidget.qml"],
    users: ["RogMouseSettings.qml"]
);
static DMS: Bundle = bundle!(
    "dms/RogMouseBattery",
    [
        "MouseBatteryDaemon.qml",
        "MouseBatteryWidget.qml",
        "Settings.qml",
        "plugin.json",
    ]
);
static KDE: Bundle = bundle!(
    "kde/rog-mouse-battery",
    [
        "metadata.json",
        "contents/config/config.qml",
        "contents/config/main.xml",
        "contents/ui/configGeneral.qml",
        "contents/ui/main.qml",
    ]
);
static GNOME: Bundle = bundle!(
    "gnome/rog-mouse-battery@humblemonk.github.io",
    [
        "extension.js",
        "metadata.json",
        "prefs.js",
        "schemas/org.gnome.shell.extensions.rog-mouse-battery.gschema.xml",
        "stylesheet.css",
    ]
);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Desktop {
    Noctalia,
    Waybar,
    Quickshell,
    Dms,
    Kde,
    Gnome,
}

impl Desktop {
    const ALL: [Self; 6] = [
        Self::Noctalia,
        Self::Waybar,
        Self::Quickshell,
        Self::Dms,
        Self::Kde,
        Self::Gnome,
    ];

    fn parse(name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|d| d.name() == name)
    }

    fn name(self) -> &'static str {
        match self {
            Self::Noctalia => "noctalia",
            Self::Waybar => "waybar",
            Self::Quickshell => "quickshell",
            Self::Dms => "dms",
            Self::Kde => "kde",
            Self::Gnome => "gnome",
        }
    }

    fn title(self) -> &'static str {
        match self {
            Self::Noctalia => "Noctalia",
            Self::Waybar => "Waybar",
            Self::Quickshell => "Quickshell",
            Self::Dms => "DankMaterialShell",
            Self::Kde => "KDE Plasma",
            Self::Gnome => "GNOME",
        }
    }

    fn description(self) -> &'static str {
        match self {
            Self::Noctalia => "Noctalia",
            Self::Waybar => "Waybar (prints the module to add to your config)",
            Self::Quickshell => "a Quickshell config of your own",
            Self::Dms => "DankMaterialShell",
            Self::Kde => "KDE Plasma 6",
            Self::Gnome => "GNOME 45 or newer",
        }
    }
}

type Result<T> = std::result::Result<T, String>;

/// `desktop`: `None` lists the choices. `force` overwrites or deletes files the
/// user changed.
pub fn run(desktop: Option<&str>, remove: bool, force: bool) -> ExitCode {
    let Some(name) = desktop else {
        let _ = io::stdout().write_all(choices(color(&io::stdout())).as_bytes());
        return ExitCode::SUCCESS;
    };
    let Some(desktop) = Desktop::parse(name) else {
        let _ = write!(
            io::stderr(),
            "Unknown desktop {name:?}.\n\n{}",
            choices(color(&io::stderr()))
        );
        return ExitCode::FAILURE;
    };
    let done = if remove {
        uninstall(desktop, force)
    } else {
        install(desktop, force)
    };
    match done {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            complain!("rogctl configure: {e}");
            ExitCode::FAILURE
        }
    }
}

/// Whether to colour what goes to `stream`: only a terminal, and not with
/// `NO_COLOR` set (<https://no-color.org>).
fn color(stream: &impl IsTerminal) -> bool {
    stream.is_terminal() && !crate::no_color()
}

/// Whether a desktop's widget is installed, as the listing marks it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Status {
    Absent,
    Installed,
    Outdated,
}

impl Status {
    fn of(desktop: Desktop) -> Self {
        if is_outdated(desktop).unwrap_or(false) {
            Self::Outdated
        } else if is_installed(desktop).unwrap_or(false) {
            Self::Installed
        } else {
            Self::Absent
        }
    }

    fn label(self) -> &'static str {
        match self {
            Self::Absent => "",
            Self::Installed => "installed",
            Self::Outdated => "out of date",
        }
    }
}

fn choices(color: bool) -> String {
    listing(&Desktop::ALL.map(|d| (d, Status::of(d))), color)
}

fn listing(statuses: &[(Desktop, Status)], color: bool) -> String {
    let mut text = "\
Usage: rogctl configure DESKTOP [--remove] [--force]

Installs the battery widget for DESKTOP, or with --remove removes it:
"
    .to_owned();
    let width = statuses
        .iter()
        .map(|(d, _)| d.description().chars().count())
        .max()
        .unwrap_or(0);
    for &(desktop, status) in statuses {
        let line = format!(
            "  {:<12}{:<width$}  {}",
            desktop.name(),
            desktop.description(),
            status.label()
        );
        text += line.trim_end();
        text += "\n";
    }
    for &(desktop, status) in statuses {
        if status == Status::Outdated {
            text += &format!("\n{}\n", outdated_message(desktop, color));
        }
    }
    text
}

/// Said in the `rogctl configure` listing and the desktop notification.
/// `color` puts the warning in bold yellow and the command in bold.
fn outdated_message(desktop: Desktop, color: bool) -> String {
    let (warn, bold, reset) = if color {
        ("\x1b[1;33m", "\x1b[1m", "\x1b[0m")
    } else {
        ("", "", "")
    };
    format!(
        "{warn}The {} widget is out of date.{reset} Run: {bold}rogctl configure {}{reset}",
        desktop.title(),
        desktop.name()
    )
}

/// The installed widgets older than the ones built into this binary.
fn outdated() -> Vec<Desktop> {
    Desktop::ALL
        .into_iter()
        .filter(|&d| is_outdated(d).unwrap_or(false))
        .collect()
}

/// `false` when the widget isn't installed, or rogctl can't tell.
fn is_outdated(desktop: Desktop) -> Result<bool> {
    match desktop {
        Desktop::Noctalia => noctalia()?.is_outdated(),
        Desktop::Waybar => Ok(false),
        Desktop::Quickshell => quickshell()?.is_outdated(),
        Desktop::Dms => dms()?.is_outdated(),
        Desktop::Kde => Ok(package_outdated(&kde_dir()?, &KDE, "/KPlugin/Version")),
        Desktop::Gnome => Ok(package_outdated(&gnome_dir()?, &GNOME, "/version-name")),
    }
}

/// `false` for Waybar, whose config rogctl doesn't edit, and when rogctl
/// can't tell.
fn is_installed(desktop: Desktop) -> Result<bool> {
    match desktop {
        Desktop::Noctalia => noctalia()?.is_installed(),
        Desktop::Waybar => Ok(false),
        Desktop::Quickshell => quickshell()?.is_installed(),
        Desktop::Dms => dms()?.is_installed(),
        Desktop::Kde => kde_installed(),
        Desktop::Gnome => gnome_installed(),
    }
}

/// For a widget its desktop's tool installed, which keeps no record: whether
/// its files differ from the bundle and its version, at `pointer` in its
/// `metadata.json`, is older than this rogctl's.
fn package_outdated(dir: &Path, bundle: &Bundle, pointer: &str) -> bool {
    if !differs(dir, bundle) {
        return false;
    }
    let version = fs::read(dir.join("metadata.json"))
        .ok()
        .and_then(|bytes| serde_json::from_slice::<serde_json::Value>(&bytes).ok())
        .and_then(|json| json.pointer(pointer)?.as_str().map(str::to_owned));
    is_older(version.as_deref(), env!("CARGO_PKG_VERSION"))
}

/// Whether `installed` is an older version than `ours`. An unknown or
/// unreadable version counts as older. The same version with other files is
/// a build from a checkout, so it doesn't. If `ours` is unreadable, nothing
/// counts as older.
fn is_older(installed: Option<&str>, ours: &str) -> bool {
    let parse = |v: &str| {
        v.split('.')
            .map(|n| n.parse::<u32>().ok())
            .collect::<Option<Vec<_>>>()
    };
    let Some(ours) = parse(ours) else {
        return false;
    };
    installed
        .and_then(parse)
        .is_none_or(|installed| installed < ours)
}

/// Whether a widget is there, with one of the bundle's files missing or
/// different. Nobody edits the files of the widgets this checks, so going by
/// them is safe.
fn differs(dir: &Path, bundle: &Bundle) -> bool {
    dir.exists()
        && bundle
            .files
            .iter()
            .any(|f| fs::read(dir.join(f.path)).ok().as_deref() != Some(f.contents))
}

/// Once per rogctl version, tells the user in a desktop notification which
/// installed widgets to update with `rogctl configure`. Which widgets are out
/// of date only changes with rogctl's version, so it checks once per version
/// too. `battery` and `watch` call it, so it ignores every error and never
/// waits.
pub fn notify_if_outdated() {
    let Ok(marker) = state_dir().map(|d| d.join("notified")) else {
        return;
    };
    let version = env!("CARGO_PKG_VERSION");
    if fs::read_to_string(&marker).is_ok_and(|v| v.trim() == version) {
        return;
    }
    // Without the marker, every `battery` poll would notify again.
    if write_creating_dirs(&marker, version).is_err() {
        return;
    }
    let stale = outdated();
    if stale.is_empty() {
        return;
    }
    let body: Vec<String> = stale
        .into_iter()
        .map(|d| outdated_message(d, false))
        .collect();
    let Ok(mut child) = Command::new("notify-send")
        .args([WIDGET_NAME, &body.join("\n")])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
    else {
        return;
    };
    // Reaped on its own thread, so `watch` doesn't wait on it.
    let _ = std::thread::Builder::new().spawn(move || child.wait());
}

fn install(desktop: Desktop, force: bool) -> Result<()> {
    match desktop {
        Desktop::Noctalia => install_noctalia(force),
        Desktop::Waybar => {
            print_waybar_module();
            Ok(())
        }
        Desktop::Quickshell => install_quickshell(force),
        Desktop::Dms => install_dms(force),
        Desktop::Kde => install_kde(),
        Desktop::Gnome => install_gnome(),
    }?;
    print_binary_hint(desktop);
    Ok(())
}

fn uninstall(desktop: Desktop, force: bool) -> Result<()> {
    match desktop {
        Desktop::Noctalia => remove_noctalia(force),
        Desktop::Waybar => {
            say!(
                "rogctl doesn't edit your Waybar config. To remove the modules, take \
                 \"custom/rog-mouse\" and \"custom/rog-keyboard\" out of your modules list, \
                 delete the modules and their #custom-rog-mouse and #custom-rog-keyboard \
                 rules from style.css, then reload Waybar: pkill -SIGUSR2 waybar"
            );
            Ok(())
        }
        Desktop::Quickshell => remove_quickshell(force),
        Desktop::Dms => remove_dms(force),
        Desktop::Kde => remove_kde(),
        Desktop::Gnome => remove_gnome(),
    }
}

fn noctalia() -> Result<Copied> {
    Ok(Copied {
        dir: data_home()?.join("noctalia/plugins/rog-mouse-battery"),
        own_dir: true,
        bundle: &NOCTALIA,
        record: record_path(Desktop::Noctalia)?,
    })
}

fn install_noctalia(force: bool) -> Result<()> {
    find_program("noctalia")?;
    let noctalia = noctalia()?;
    // By the record, not the directory: a first install whose enable failed
    // isn't an update when run again.
    let previous = noctalia.previous()?;
    noctalia.install(force)?;
    say!("Copied the plugin to {}", noctalia.dir.display());
    run_tool(Command::new("noctalia").args(["msg", "plugins", "enable", NOCTALIA_ID]))?;
    match previous {
        Previous::Nothing => say!("Now add {WIDGET_NAME} from the bar's Add-widget picker."),
        Previous::ThisVersion => say!(
            "Reinstalled the {WIDGET_NAME} plugin. If it isn't in your bar, add it from the \
             bar's Add-widget picker."
        ),
        Previous::Other => {
            // Settings in plugin.toml only load with the config.
            run_tool(Command::new("noctalia").args(["msg", "config-reload"]))?;
            say!("Updated the {WIDGET_NAME} plugin.");
        }
    }
    Ok(())
}

fn remove_noctalia(force: bool) -> Result<()> {
    let noctalia = noctalia()?;
    if !exists(&noctalia.dir)? {
        return not_installed(Desktop::Noctalia);
    }
    // Before disabling it, so a refusal leaves the plugin as it was.
    noctalia.removable(force)?;
    // Noctalia may not be running; the files go either way.
    if let Err(e) =
        run_tool(Command::new("noctalia").args(["msg", "plugins", "disable", NOCTALIA_ID]))
    {
        complain!("{e}; removing the plugin anyway");
    }
    noctalia.remove(force)?;
    say!("Removed the plugin from {}", noctalia.dir.display());
    Ok(())
}

fn print_waybar_module() {
    let [config, style] = WAYBAR.files else {
        return;
    };
    say!(
        "rogctl doesn't edit your Waybar config. To add the modules:\n\n\
         1. Add these modules to your Waybar config, one for the mouse and one for the \
         keyboard (leave out either one):\n\n{}\n\
         2. Add \"custom/rog-mouse\" and \"custom/rog-keyboard\" to modules-left, \
         modules-center or modules-right. Each hides while its device isn't plugged in.\n\n\
         3. Add this to your Waybar style.css:\n\n{}\n\
         4. Reload Waybar: pkill -SIGUSR2 waybar\n\n\
         The icons need a Nerd Font.",
        String::from_utf8_lossy(config.contents),
        String::from_utf8_lossy(style.contents),
    );
}

/// Quickshell has no plugin directory: the files go next to the user's
/// `shell.qml`, among their own files.
fn quickshell() -> Result<Copied> {
    let dir = config_home()?.join("quickshell");
    if !dir.join("shell.qml").is_file() {
        return Err(format!(
            "there's no shell.qml in {}. If your config is elsewhere, copy RogMouse.qml, \
             RogMouseSettings.qml and RogMouseWidget.qml from \
             https://github.com/humblemonk/rogctl/tree/main/quickshell next to its shell.qml",
            dir.display()
        ));
    }
    Ok(Copied {
        dir,
        own_dir: false,
        bundle: &QUICKSHELL,
        record: record_path(Desktop::Quickshell)?,
    })
}

fn install_quickshell(force: bool) -> Result<()> {
    let quickshell = quickshell()?;
    let written = quickshell.install(force).map_err(|e| {
        // Before 2.6.0 the settings were at the top of RogMouse.qml, so an
        // edited one most likely holds them, and --force overwrites it.
        if e.starts_with("RogMouse.qml ") {
            format!(
                "{e}. Its settings now go in RogMouseSettings.qml: note yours at the top of \
                 RogMouse.qml first, then set them there"
            )
        } else {
            e
        }
    })?;
    say!(
        "Copied {} to {}\n\
         Now put RogMouseWidget {{}} in your bar. Its settings are in RogMouseSettings.qml \
         there; updates keep your changes to it.",
        and_list(&written),
        quickshell.dir.display()
    );
    Ok(())
}

/// "a", "a and b", "a, b and c".
fn and_list(items: &[&str]) -> String {
    match items {
        [rest @ .., last] if !rest.is_empty() => format!("{} and {last}", rest.join(", ")),
        _ => items.concat(),
    }
}

fn remove_quickshell(force: bool) -> Result<()> {
    let quickshell = quickshell()?;
    if !quickshell.remove(force)? {
        return not_installed(Desktop::Quickshell);
    }
    say!(
        "Removed the widget from {}\n\
         Take RogMouseWidget {{}} out of your bar too.",
        quickshell.dir.display()
    );
    Ok(())
}

fn dms() -> Result<Copied> {
    Ok(Copied {
        dir: dms_plugins(&config_home()?).join("RogMouseBattery"),
        own_dir: true,
        bundle: &DMS,
        record: record_path(Desktop::Dms)?,
    })
}

/// The plugin directory DankMaterialShell reads. 2.0 moved its config from
/// `DankMaterialShell` to `dms`, leaving a link at the old name; before 2.0
/// only the old directory is there, perhaps itself a link (dotfiles), which
/// 2.0 then moves only while there's no `dms`.
fn dms_plugins(config: &Path) -> PathBuf {
    let legacy = config.join("DankMaterialShell");
    let current = config.join("dms");
    let legacy_only =
        fs::metadata(&legacy).is_ok_and(|m| m.is_dir()) && fs::symlink_metadata(&current).is_err();
    if legacy_only { legacy } else { current }.join("plugins")
}

fn install_dms(force: bool) -> Result<()> {
    find_program("dms")?;
    let dms = dms()?;
    let previous = dms.previous()?;
    dms.install(force)?;
    say!("Copied the plugin to {}", dms.dir.display());
    run_tool(Command::new("dms").args(["ipc", "plugin-scan", "scan"]))?;
    match previous {
        Previous::Nothing => {
            say!("Now enable {WIDGET_NAME} in Settings → Plugins, then add it to the bar.")
        }
        Previous::ThisVersion => say!(
            "Reinstalled the {WIDGET_NAME} plugin. If it isn't in your bar, enable it in \
             Settings → Plugins, then add it to the bar."
        ),
        // Not reloaded here: reloading a disabled plugin would start it.
        Previous::Other => say!(
            "Updated the {WIDGET_NAME} plugin. If it's enabled, load the new version with \
             `dms ipc plugin-scan reload {DMS_ID}`, or restart DankMaterialShell."
        ),
    }
    Ok(())
}

fn remove_dms(force: bool) -> Result<()> {
    let dms = dms()?;
    if !dms.remove(force)? {
        return not_installed(Desktop::Dms);
    }
    say!("Removed the plugin from {}", dms.dir.display());
    // DankMaterialShell may not be running; the files are gone either way.
    if let Err(e) = run_tool(Command::new("dms").args(["ipc", "plugin-scan", "scan"])) {
        complain!("{e}");
    }
    Ok(())
}

/// Where kpackagetool6 installs a user's Plasma widget.
fn kde_dir() -> Result<PathBuf> {
    Ok(data_home()?.join("plasma/plasmoids").join(KDE_ID))
}

fn kde_installed() -> Result<bool> {
    exists(&kde_dir()?)
}

fn install_kde() -> Result<()> {
    find_program("kpackagetool6")?;
    let updating = kde_installed()?;
    let temp = TempDir::new()?;
    let package = temp.0.join("rog-mouse-battery");
    write_files(&package, &KDE)?;
    let mode = if updating { "--upgrade" } else { "--install" };
    run_tool(
        Command::new("kpackagetool6")
            .args(["--type", "Plasma/Applet", mode])
            .arg(&package),
    )?;
    if updating {
        say!("Updated {WIDGET_NAME}. Log out and back in to load the new version.");
    } else {
        say!("Now add {WIDGET_NAME} to a panel from Add Widgets.");
    }
    Ok(())
}

fn remove_kde() -> Result<()> {
    if !kde_installed()? {
        return not_installed(Desktop::Kde);
    }
    run_tool(Command::new("kpackagetool6").args(["--type", "Plasma/Applet", "--remove", KDE_ID]))
}

fn gnome_dir() -> Result<PathBuf> {
    Ok(data_home()?.join("gnome-shell/extensions").join(GNOME_UUID))
}

fn gnome_installed() -> Result<bool> {
    exists(&gnome_dir()?)
}

fn install_gnome() -> Result<()> {
    find_program("gnome-extensions")?;
    let updating = gnome_installed()?;
    let temp = TempDir::new()?;
    let source = temp.0.join(GNOME_UUID);
    write_files(&source, &GNOME)?;
    run_tool(
        Command::new("gnome-extensions")
            .args(["pack", "--force", "--out-dir"])
            .arg(&temp.0)
            .arg(&source),
    )?;
    run_tool(
        Command::new("gnome-extensions")
            .args(["install", "--force"])
            .arg(temp.0.join(format!("{GNOME_UUID}.shell-extension.zip"))),
    )?;
    if updating {
        say!("Updated {WIDGET_NAME}. Log out and back in to load the new version.");
    } else {
        say!(
            "Log out and back in, then turn on {WIDGET_NAME} in the Extensions app, or run:\n  \
             gnome-extensions enable {GNOME_UUID}"
        );
    }
    Ok(())
}

fn remove_gnome() -> Result<()> {
    if !gnome_installed()? {
        return not_installed(Desktop::Gnome);
    }
    // gnome-extensions uninstall asks GNOME Shell to do it, over D-Bus.
    run_tool(Command::new("gnome-extensions").args(["uninstall", GNOME_UUID])).map_err(|e| {
        format!(
            "{e}. It needs GNOME Shell running: run this again in your GNOME session, or \
             delete {} yourself",
            gnome_dir().map_or_else(|_| GNOME_UUID.to_owned(), |d| d.display().to_string())
        )
    })
}

fn not_installed(desktop: Desktop) -> Result<()> {
    say!(
        "{WIDGET_NAME} isn't installed for {}; nothing to remove.",
        desktop.title()
    );
    Ok(())
}

/// The desktop's `PATH` often lacks `~/.cargo/bin`, where `cargo install`
/// puts rogctl, so say where it is.
fn print_binary_hint(desktop: Desktop) {
    let Ok(exe) = std::env::current_exe() else {
        return;
    };
    let exe = exe.display();
    match desktop {
        Desktop::Noctalia | Desktop::Dms | Desktop::Kde | Desktop::Gnome => {
            say!("If the widget says rogctl wasn't found, set its rogctl command setting to {exe}")
        }
        Desktop::Quickshell => {
            say!("If it says rogctl wasn't found, set binary in RogMouseSettings.qml to {exe}")
        }
        Desktop::Waybar => say!("If Waybar can't find rogctl, put {exe} in each exec line."),
    }
}

/// `$XDG_CONFIG_HOME`, or `~/.config`.
fn config_home() -> Result<PathBuf> {
    xdg_dir(
        std::env::var_os("XDG_CONFIG_HOME"),
        std::env::var_os("HOME"),
        ".config",
    )
}

/// `$XDG_DATA_HOME`, or `~/.local/share`.
fn data_home() -> Result<PathBuf> {
    xdg_dir(
        std::env::var_os("XDG_DATA_HOME"),
        std::env::var_os("HOME"),
        ".local/share",
    )
}

/// The XDG base directory spec ignores a relative `$XDG_*` value.
fn xdg_dir(xdg: Option<OsString>, home: Option<OsString>, fallback: &str) -> Result<PathBuf> {
    if let Some(dir) = xdg.map(PathBuf::from).filter(|d| d.is_absolute()) {
        return Ok(dir);
    }
    home.map(PathBuf::from)
        .filter(|h| h.is_absolute())
        .map(|h| h.join(fallback))
        .ok_or_else(|| "HOME isn't set".to_owned())
}

/// Whether anything is at `path`, or why rogctl can't tell.
fn exists(path: &Path) -> Result<bool> {
    path.try_exists()
        .map_err(|e| format!("{}: {e}", path.display()))
}

fn write_files(dir: &Path, bundle: &Bundle) -> Result<()> {
    for file in bundle.files {
        let path = dir.join(file.path);
        write_creating_dirs(&path, file.contents)
            .map_err(|e| format!("{}: {e}", path.display()))?;
    }
    Ok(())
}

/// `fs::write`, creating the directories above `path` first.
fn write_creating_dirs(path: &Path, contents: impl AsRef<[u8]>) -> io::Result<()> {
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir)?;
    }
    fs::write(path, contents)
}

/// A widget rogctl copies into place itself, rather than handing it to the
/// desktop's own tool. Updating and removing it touch only the files rogctl
/// wrote, and only while they're as rogctl left them, going by its record of
/// what it wrote.
struct Copied {
    dir: PathBuf,
    /// `dir` is the widget's alone, so it may be deleted once empty. Not so
    /// for Quickshell, whose files go in the user's own config directory.
    own_dir: bool,
    bundle: &'static Bundle,
    /// Each file written, relative to `dir`, with the checksum of what was
    /// written.
    record: PathBuf,
}

/// A file rogctl wrote, relative to the widget's directory, and the checksum
/// of what it wrote.
type Entry = (String, u64);

/// What a widget's directory held before an install.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Previous {
    /// None of the widget's files.
    Nothing,
    /// This version's files, as rogctl recorded writing them.
    ThisVersion,
    /// Another version's, or files rogctl has no record of.
    Other,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Found {
    Missing,
    /// What rogctl wrote there.
    Ours,
    /// Something else: the user changed it, or it isn't rogctl's.
    Edited,
}

impl Copied {
    /// Writes the bundle, and deletes files an earlier version wrote that this
    /// one doesn't have. Returns the files written: all but the user's edited
    /// settings.
    fn install(&self, force: bool) -> Result<Vec<&'static str>> {
        let previous_record = self.read_record()?;
        let record = previous_record.clone().unwrap_or_default();
        let recorded = |path: &str| record.iter().find(|(p, _)| p == path).map(|&(_, sum)| sum);
        self.refuse_links(
            (self.bundle.files.iter().map(|f| f.path))
                .chain(record.iter().map(|(p, _)| p.as_str())),
        )?;

        let mut edited = Vec::new();
        for file in self
            .bundle
            .files
            .iter()
            .filter(|f| !self.bundle.is_users(f.path))
        {
            let ours = [Some(checksum(file.contents)), recorded(file.path)];
            if self.find(file.path, &ours)? == Found::Edited {
                edited.push(file.path.to_owned());
            }
        }
        // Files an earlier version wrote and this one dropped go, the ones the
        // user changed only with --force.
        let mut obsolete = Vec::new();
        let mut dropped = Vec::new();
        for (path, sum) in &record {
            if self.shipped(path).is_some() {
                continue;
            }
            match self.find(path, &[Some(*sum)])? {
                Found::Missing => {}
                Found::Ours => obsolete.push(path.clone()),
                Found::Edited => dropped.push(path.clone()),
            }
        }
        if !force {
            self.refuse(&edited, &dropped)?;
        }
        obsolete.append(&mut dropped);

        let mut entries = Vec::new();
        let mut written = Vec::new();
        let done = self
            .write_bundle(&recorded, &mut entries, &mut written)
            .and_then(|()| self.delete(&obsolete));
        if let Err(e) = done {
            // What's there now: the files written, and the rest as before,
            // which with no record is what `removable()` and
            // `is_installed()` go by without one: this version's files.
            // Without a version the record reads as out of date, and a rerun
            // or a removal still knows every file rogctl wrote.
            let before = previous_record.unwrap_or_else(|| self.bundle_entries());
            for entry in &before {
                if !entries.iter().any(|(p, _)| *p == entry.0) {
                    entries.push(entry.clone());
                }
            }
            let _ = self.write_record(&entries, false);
            return Err(e);
        }
        self.write_record(&entries, true)?;
        Ok(written)
    }

    /// Writes the bundle's files, adding each to `entries` and, unless it's
    /// the user's edited settings, kept, to `written`.
    fn write_bundle(
        &self,
        recorded: &dyn Fn(&str) -> Option<u64>,
        entries: &mut Vec<Entry>,
        written: &mut Vec<&'static str>,
    ) -> Result<()> {
        for file in self.bundle.files {
            let ours = [Some(checksum(file.contents)), recorded(file.path)];
            if self.bundle.is_users(file.path) && self.find(file.path, &ours)? == Found::Edited {
                // The user's settings stay. Recorded as this version's, they
                // still read as edited, so nothing removes them, and they
                // don't make the widget look out of date.
                entries.push((file.path.to_owned(), checksum(file.contents)));
                continue;
            }
            let path = self.dir.join(file.path);
            write_creating_dirs(&path, file.contents)
                .map_err(|e| format!("{}: {e}", path.display()))?;
            entries.push((file.path.to_owned(), checksum(file.contents)));
            written.push(file.path);
        }
        Ok(())
    }

    /// The checksum of this version's file at `path`, if it ships one.
    fn shipped(&self, path: &str) -> Option<u64> {
        self.bundle
            .files
            .iter()
            .find(|f| f.path == path)
            .map(|f| checksum(f.contents))
    }

    /// What was installed before, for what to say after installing.
    fn previous(&self) -> Result<Previous> {
        if !self.is_installed()? {
            return Ok(Previous::Nothing);
        }
        let Some(mut record) = self.read_record()? else {
            return Ok(Previous::Other);
        };
        let mut current = self.bundle_entries();
        record.sort();
        current.sort();
        Ok(if record == current {
            Previous::ThisVersion
        } else {
            Previous::Other
        })
    }

    /// Errs if a symbolic link is at one of `paths`, at a directory between
    /// the widget's directory and it, or at the widget's directory when it's
    /// the widget's alone. rogctl doesn't write or delete through a link, so
    /// a widget that used to be linked to a checkout is left as it is.
    fn refuse_links<'a>(&self, paths: impl IntoIterator<Item = &'a str>) -> Result<()> {
        let mut checked = Vec::new();
        if self.own_dir {
            checked.push(self.dir.clone());
        }
        for path in paths {
            let mut full = self.dir.clone();
            for component in Path::new(path).components() {
                full.push(component);
                checked.push(full.clone());
            }
        }
        for full in checked {
            match fs::symlink_metadata(&full) {
                Ok(meta) if meta.file_type().is_symlink() => {
                    return Err(format!(
                        "{} is a symbolic link, and rogctl doesn't write or delete through one, \
                         so it changed nothing. Delete the link (not what it points to), then run \
                         this again",
                        full.display()
                    ));
                }
                Ok(_) => {}
                Err(e) if e.kind() == io::ErrorKind::NotFound => {}
                Err(e) => return Err(format!("{}: {e}", full.display())),
            }
        }
        Ok(())
    }

    /// The files `remove` deletes: those rogctl wrote. Errs if the user changed
    /// any, unless `force`.
    fn removable(&self, force: bool) -> Result<Vec<String>> {
        // With no record, the files this version ships are what rogctl wrote.
        let entries = self.read_record()?.unwrap_or_else(|| self.bundle_entries());
        self.refuse_links(entries.iter().map(|(p, _)| p.as_str()))?;
        let mut ours = Vec::new();
        let mut edited = Vec::new();
        let mut settings = Vec::new();
        for (path, sum) in entries {
            // As install counts them: what rogctl recorded, or this version's.
            match self.find(&path, &[Some(sum), self.shipped(&path)])? {
                Found::Missing => {}
                Found::Ours => ours.push(path),
                // The user's settings stay, even with --force.
                Found::Edited if self.bundle.is_users(&path) => settings.push(path),
                Found::Edited => edited.push(path),
            }
        }
        if !force {
            self.refuse(&[], &edited)?;
        }
        for path in settings {
            say!(
                "Left {}: it has your settings.",
                self.dir.join(path).display()
            );
        }
        ours.append(&mut edited);
        Ok(ours)
    }

    /// Whether rogctl installed this widget from an older version's bundle.
    /// Goes by the record, not the files, so the user's own edits don't count.
    /// `false` with no record, or once every recorded file but the user's
    /// settings is gone.
    fn is_outdated(&self) -> Result<bool> {
        let Some(text) = self.read_record_text()? else {
            return Ok(false);
        };
        let mut record = parse_record(&text);
        if !self.any_present(&record) {
            return Ok(false);
        }
        let mut current = self.bundle_entries();
        record.sort();
        current.sort();
        // Records written before rogctl recorded its version count as older.
        let version = text.lines().find_map(|l| l.strip_prefix("version "));
        Ok(record != current && is_older(version, env!("CARGO_PKG_VERSION")))
    }

    /// Whether any of the widget's files is there, by the record or, with
    /// none, the bundle. The user's settings don't count: Quickshell's stay
    /// after the widget is removed.
    fn is_installed(&self) -> Result<bool> {
        let entries = self.read_record()?.unwrap_or_else(|| self.bundle_entries());
        Ok(self.any_present(&entries))
    }

    /// Whether any of `entries` but the user's settings is on disk.
    fn any_present(&self, entries: &[Entry]) -> bool {
        entries
            .iter()
            .any(|(path, _)| !self.bundle.is_users(path) && self.dir.join(path).exists())
    }

    fn bundle_entries(&self) -> Vec<Entry> {
        self.bundle
            .files
            .iter()
            .map(|f| (f.path.to_owned(), checksum(f.contents)))
            .collect()
    }

    /// Deletes the files rogctl wrote, then the directories they leave empty.
    /// Returns whether there was anything to delete.
    fn remove(&self, force: bool) -> Result<bool> {
        let files = self.removable(force)?;
        self.delete(&files)?;
        self.delete_record()?;
        if self.own_dir && self.dir.exists() {
            say!(
                "Left {}: it has files rogctl didn't write.",
                self.dir.display()
            );
        }
        Ok(!files.is_empty())
    }

    /// What's at `path` in the widget's directory, where `ours` are the
    /// checksums of what rogctl may have written there.
    fn find(&self, path: &str, ours: &[Option<u64>]) -> Result<Found> {
        let full = self.dir.join(path);
        match fs::read(&full) {
            Ok(bytes) if ours.contains(&Some(checksum(&bytes))) => Ok(Found::Ours),
            Ok(_) => Ok(Found::Edited),
            Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(Found::Missing),
            Err(e) => Err(format!("{}: {e}", full.display())),
        }
    }

    /// Errs if the user changed any of the files rogctl would `overwrite` or
    /// `remove`.
    fn refuse(&self, overwrite: &[String], remove: &[String]) -> Result<()> {
        let names =
            |paths: &[String]| and_list(&paths.iter().map(String::as_str).collect::<Vec<_>>());
        let edited = [overwrite, remove].concat();
        if edited.is_empty() {
            return Ok(());
        }
        let (is, them) = if edited.len() == 1 {
            ("is", "it")
        } else {
            ("are", "them")
        };
        let action = match (overwrite.is_empty(), remove.is_empty()) {
            (false, false) => format!(
                "overwrite {} and remove {}",
                names(overwrite),
                names(remove)
            ),
            (false, true) => format!("overwrite {them}"),
            (true, _) => format!("remove {them}"),
        };
        Err(format!(
            "{} in {} {is} different from what rogctl wrote. Pass --force to {action} anyway",
            names(&edited),
            self.dir.display()
        ))
    }

    /// Deletes `paths`, then each directory above them that's left empty, up to
    /// the widget's directory, included only if it's the widget's alone.
    fn delete(&self, paths: &[String]) -> Result<()> {
        for path in paths {
            let mut full = self.dir.join(path);
            fs::remove_file(&full).map_err(|e| format!("{}: {e}", full.display()))?;
            while full.pop() && full.starts_with(&self.dir) {
                let keep = full == self.dir && !self.own_dir;
                // remove_dir fails on a directory that isn't empty.
                if keep || fs::remove_dir(&full).is_err() {
                    break;
                }
            }
        }
        Ok(())
    }

    /// `None` when there's no record: nothing installed yet, or installed by
    /// hand.
    fn read_record(&self) -> Result<Option<Vec<Entry>>> {
        Ok(self.read_record_text()?.map(|text| parse_record(&text)))
    }

    fn read_record_text(&self) -> Result<Option<String>> {
        match fs::read_to_string(&self.record) {
            Ok(text) => Ok(Some(text)),
            Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(None),
            Err(e) => Err(format!("{}: {e}", self.record.display())),
        }
    }

    /// `versioned`: with this rogctl's version. A record without one counts
    /// as an older version's.
    fn write_record(&self, written: &[Entry], versioned: bool) -> Result<()> {
        let mut text = format!(
            "# Files `rogctl configure` wrote to {}, with their checksums.\n",
            self.dir.display()
        );
        for (path, sum) in written {
            text += &format!("{sum:016x} {path}\n");
        }
        if versioned {
            text += &format!("version {}\n", env!("CARGO_PKG_VERSION"));
        }
        write_creating_dirs(&self.record, text)
            .map_err(|e| format!("{}: {e}", self.record.display()))
    }

    fn delete_record(&self) -> Result<()> {
        match fs::remove_file(&self.record) {
            Err(e) if e.kind() != io::ErrorKind::NotFound => {
                Err(format!("{}: {e}", self.record.display()))
            }
            _ => Ok(()),
        }
    }
}

/// Skips lines that aren't a checksum and a plain relative path, so a damaged
/// record can't point outside the widget's directory.
fn parse_record(text: &str) -> Vec<Entry> {
    text.lines()
        .filter_map(|line| {
            let (sum, path) = line.split_once(' ')?;
            let sum = u64::from_str_radix(sum, 16).ok()?;
            let plain = !path.is_empty()
                && Path::new(path)
                    .components()
                    .all(|c| matches!(c, Component::Normal(_)));
            plain.then(|| (path.to_owned(), sum))
        })
        .collect()
}

/// `$XDG_STATE_HOME/rogctl`, or `~/.local/state/rogctl`.
pub fn state_dir() -> Result<PathBuf> {
    let state = xdg_dir(
        std::env::var_os("XDG_STATE_HOME"),
        std::env::var_os("HOME"),
        ".local/state",
    )?;
    Ok(state.join("rogctl"))
}

/// `$XDG_STATE_HOME/rogctl/DESKTOP.files`.
fn record_path(desktop: Desktop) -> Result<PathBuf> {
    Ok(state_dir()?.join(format!("{}.files", desktop.name())))
}

/// 64-bit FNV-1a. Unlike std's hasher it's the same in every Rust version,
/// which a record kept across rogctl updates needs. It only has to notice
/// edits, not resist them.
fn checksum(bytes: &[u8]) -> u64 {
    bytes.iter().fold(0xcbf2_9ce4_8422_2325, |hash, &b| {
        (hash ^ u64::from(b)).wrapping_mul(0x0100_0000_01b3)
    })
}

/// Checked before writing anything, so a missing tool doesn't leave a widget
/// half installed.
fn find_program(name: &str) -> Result<()> {
    let path = std::env::var_os("PATH").unwrap_or_default();
    if std::env::split_paths(&path).any(|dir| dir.join(name).is_file()) {
        Ok(())
    } else {
        Err(format!("{name} isn't installed, or isn't on PATH"))
    }
}

/// Runs a desktop's own tool, with its output going to the terminal.
fn run_tool(command: &mut Command) -> Result<()> {
    let shown: Vec<String> = std::iter::once(command.get_program())
        .chain(command.get_args())
        .map(|a| a.to_string_lossy().into_owned())
        .collect();
    let shown = shown.join(" ");
    match command.status() {
        Ok(status) if status.success() => Ok(()),
        Ok(status) => Err(format!("`{shown}` failed ({status})")),
        Err(e) => Err(format!("couldn't run `{shown}`: {e}")),
    }
}

/// A new directory under the system's temporary directory, deleted on drop.
struct TempDir(PathBuf);

impl TempDir {
    fn new() -> Result<Self> {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |d| d.subsec_nanos());
        let path = std::env::temp_dir().join(format!("rogctl-{}-{nanos}", std::process::id()));
        // create_dir, not create_dir_all: fails rather than reuse a directory
        // someone else made.
        fs::create_dir(&path).map_err(|e| format!("{}: {e}", path.display()))?;
        Ok(Self(path))
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bundle(desktop: Desktop) -> &'static Bundle {
        match desktop {
            Desktop::Noctalia => &NOCTALIA,
            Desktop::Waybar => &WAYBAR,
            Desktop::Quickshell => &QUICKSHELL,
            Desktop::Dms => &DMS,
            Desktop::Kde => &KDE,
            Desktop::Gnome => &GNOME,
        }
    }

    /// Every file under `dir` but the readmes, as paths relative to `root`.
    fn files_on_disk(root: &Path, dir: &Path, found: &mut Vec<String>) {
        for entry in fs::read_dir(dir).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                files_on_disk(root, &path, found);
            } else if path.file_name().is_some_and(|n| n != "README.md") {
                let relative = path.strip_prefix(root).unwrap();
                found.push(relative.to_str().unwrap().to_owned());
            }
        }
    }

    /// A file added to a widget must be added to its bundle too.
    #[test]
    fn bundles_hold_every_widget_file() {
        for desktop in Desktop::ALL {
            let bundle = bundle(desktop);
            let root = Path::new(env!("CARGO_MANIFEST_DIR")).join(bundle.dir);
            let mut on_disk = Vec::new();
            files_on_disk(&root, &root, &mut on_disk);
            on_disk.sort();
            let mut bundled: Vec<&str> = bundle.files.iter().map(|f| f.path).collect();
            bundled.sort_unstable();
            assert_eq!(bundled, on_disk, "update the bundle for {}", bundle.dir);
        }
    }

    #[test]
    fn desktop_names_round_trip() {
        for desktop in Desktop::ALL {
            assert_eq!(Desktop::parse(desktop.name()), Some(desktop));
        }
        assert_eq!(Desktop::parse("plasma"), None);
    }

    #[test]
    fn xdg_dir_falls_back_to_home() {
        let some = |s: &str| Some(OsString::from(s));
        assert_eq!(
            xdg_dir(some("/xdg"), some("/home/u"), ".config"),
            Ok(PathBuf::from("/xdg"))
        );
        assert_eq!(
            xdg_dir(some("relative"), some("/home/u"), ".config"),
            Ok(PathBuf::from("/home/u/.config")),
            "the spec ignores relative paths"
        );
        assert_eq!(
            xdg_dir(None, some("/home/u"), ".local/share"),
            Ok(PathBuf::from("/home/u/.local/share"))
        );
        assert!(xdg_dir(None, None, ".config").is_err());
    }

    /// A widget in a fresh temporary directory, as Noctalia's.
    fn copied(temp: &TempDir, own_dir: bool) -> Copied {
        Copied {
            dir: temp.0.join("plugin"),
            own_dir,
            bundle: &NOCTALIA,
            record: temp.0.join("state/noctalia.files"),
        }
    }

    fn write(path: PathBuf, text: &str) {
        write_creating_dirs(&path, text).unwrap();
    }

    #[test]
    fn and_list_joins_names() {
        assert_eq!(and_list(&[]), "");
        assert_eq!(and_list(&["a"]), "a");
        assert_eq!(and_list(&["a", "b"]), "a and b");
        assert_eq!(and_list(&["a", "b", "c"]), "a, b and c");
    }

    #[test]
    fn checksum_is_fnv1a() {
        assert_eq!(checksum(b""), 0xcbf2_9ce4_8422_2325);
        assert_eq!(checksum(b"a"), 0xaf63_dc4c_8601_ec8c);
    }

    #[test]
    fn records_and_removes_its_files() {
        let temp = TempDir::new().unwrap();
        let widget = copied(&temp, true);
        widget.install(false).unwrap();
        assert!(widget.dir.join("translations/en.json").is_file());
        let record = widget.read_record().unwrap().unwrap();
        assert_eq!(record.len(), NOCTALIA.files.len());

        assert_eq!(widget.remove(false), Ok(true));
        assert!(!widget.dir.exists(), "empty directories go too");
        assert!(!widget.record.exists());
        assert_eq!(widget.remove(false), Ok(false));
    }

    /// An update replaces what the old version wrote, without --force, and
    /// deletes the files the new version dropped.
    #[test]
    fn updates_an_older_version() {
        let temp = TempDir::new().unwrap();
        let widget = copied(&temp, true);
        write(widget.dir.join("plugin.toml"), "version 1");
        write(widget.dir.join("old/dropped.luau"), "version 1");
        let sum = checksum(b"version 1");
        write(
            widget.record.clone(),
            &format!("# header\n{sum:016x} plugin.toml\n{sum:016x} old/dropped.luau\n"),
        );

        widget.install(false).unwrap();
        assert_eq!(
            fs::read(widget.dir.join("plugin.toml")).unwrap(),
            NOCTALIA.files[1].contents
        );
        assert!(!widget.dir.join("old").exists());
        assert!(
            widget
                .read_record()
                .unwrap()
                .unwrap()
                .iter()
                .all(|(p, _)| p != "old/dropped.luau")
        );
    }

    #[test]
    fn leaves_edited_and_unknown_files_alone() {
        let temp = TempDir::new().unwrap();
        let widget = copied(&temp, true);
        widget.install(false).unwrap();
        write(widget.dir.join("widget.luau"), "edited");
        write(widget.dir.join("notes.txt"), "the user's");

        let err = widget.install(false).unwrap_err();
        assert!(err.starts_with("widget.luau in"), "{err}");
        assert!(widget.remove(false).is_err());
        assert_eq!(
            fs::read_to_string(widget.dir.join("widget.luau")).unwrap(),
            "edited"
        );

        assert_eq!(widget.remove(true), Ok(true));
        assert!(!widget.dir.join("widget.luau").exists());
        assert!(widget.dir.join("notes.txt").is_file(), "never rogctl's");
    }

    /// Without a record, only files matching this version's count as rogctl's.
    #[test]
    fn without_a_record_trusts_only_identical_files() {
        let temp = TempDir::new().unwrap();
        let widget = copied(&temp, true);
        write_files(&widget.dir, &NOCTALIA).unwrap();
        assert!(widget.install(false).is_ok());

        fs::remove_file(&widget.record).unwrap();
        write(widget.dir.join("plugin.toml"), "from somewhere else");
        assert!(widget.install(false).is_err());
        assert!(widget.remove(false).is_err());
    }

    /// Quickshell's files share the user's config directory, which stays.
    #[test]
    fn keeps_a_directory_it_doesnt_own() {
        let temp = TempDir::new().unwrap();
        let widget = copied(&temp, false);
        widget.install(false).unwrap();
        assert_eq!(widget.remove(false), Ok(true));
        assert!(widget.dir.is_dir());
    }

    /// Quickshell's settings file is the user's: written when missing, kept
    /// through updates and --force, removed only while unchanged.
    #[test]
    fn keeps_the_users_settings() {
        let temp = TempDir::new().unwrap();
        let widget = Copied {
            bundle: &QUICKSHELL,
            ..copied(&temp, false)
        };
        let settings = widget.dir.join("RogMouseSettings.qml");
        let default = QUICKSHELL
            .files
            .iter()
            .find(|f| f.path == "RogMouseSettings.qml");
        widget.install(false).unwrap();
        assert_eq!(
            fs::read(&settings).ok().as_deref(),
            default.map(|f| f.contents)
        );
        assert_eq!(widget.remove(false), Ok(true));
        assert!(!settings.exists(), "unchanged, so rogctl's to remove");

        // An older version's settings, unchanged: updated.
        write(settings.clone(), "older default");
        write(
            widget.record.clone(),
            &format!("{:016x} RogMouseSettings.qml\n", checksum(b"older default")),
        );
        let written = widget.install(false).unwrap();
        assert_eq!(
            fs::read(&settings).ok().as_deref(),
            default.map(|f| f.contents)
        );
        assert!(written.contains(&"RogMouseSettings.qml"));

        write(settings.clone(), "edited");
        assert_eq!(
            widget.install(false).unwrap(),
            ["RogMouse.qml", "RogMouseWidget.qml"]
        );
        widget.install(true).unwrap();
        assert_eq!(fs::read_to_string(&settings).unwrap(), "edited");
        assert_eq!(widget.is_outdated(), Ok(false));
        assert_eq!(widget.remove(true), Ok(true));
        assert_eq!(
            fs::read_to_string(&settings).unwrap(),
            "edited",
            "even with --force"
        );
        assert!(!widget.dir.join("RogMouse.qml").exists());

        // A settings file rogctl didn't write is kept, and stays through
        // removal too.
        assert_eq!(
            widget.install(false).unwrap(),
            ["RogMouse.qml", "RogMouseWidget.qml"]
        );
        assert_eq!(widget.is_outdated(), Ok(false));
        assert_eq!(widget.remove(false), Ok(true));
        assert!(settings.is_file());
    }

    /// A settings file that changed between versions is an update to make:
    /// an unchanged copy gets it.
    #[test]
    fn outdated_counts_the_users_settings() {
        let temp = TempDir::new().unwrap();
        let widget = Copied {
            bundle: &QUICKSHELL,
            ..copied(&temp, false)
        };
        widget.install(false).unwrap();
        let current = fs::read_to_string(&widget.record).unwrap();
        let ours = format!("version {}", env!("CARGO_PKG_VERSION"));
        let older = current.replace(&ours, "version 2.5.0");
        let changed = older
            .lines()
            .map(|l| match l.split_once(' ') {
                Some((sum, "RogMouseSettings.qml")) => {
                    let sum = u64::from_str_radix(sum, 16).unwrap() ^ 1;
                    format!("{sum:016x} RogMouseSettings.qml")
                }
                _ => l.to_owned(),
            })
            .collect::<Vec<_>>()
            .join("\n");
        write(widget.record.clone(), &changed);
        assert_eq!(widget.is_outdated(), Ok(true), "only the settings differ");
    }

    /// Only a record from another bundle counts, not the user's edits.
    #[test]
    fn outdated_goes_by_the_record() {
        let temp = TempDir::new().unwrap();
        let widget = copied(&temp, false);
        assert_eq!(widget.is_outdated(), Ok(false), "not installed");

        widget.install(false).unwrap();
        assert_eq!(widget.is_outdated(), Ok(false));
        write(widget.dir.join("plugin.toml"), "edited");
        assert_eq!(widget.is_outdated(), Ok(false), "edits don't count");

        let current = fs::read_to_string(&widget.record).unwrap();
        let ours = format!("version {}", env!("CARGO_PKG_VERSION"));
        write(
            widget.record.clone(),
            &current
                .replacen("widget.luau", "new.luau", 1)
                .replace(&ours, "version 99.0.0"),
        );
        assert_eq!(widget.is_outdated(), Ok(false), "a newer version");
        write(
            widget.record.clone(),
            &current.replacen("widget.luau", "new.luau", 1),
        );
        assert_eq!(
            widget.is_outdated(),
            Ok(false),
            "a checkout's build of this version"
        );

        let record = current.replace(&ours, "version 2.3.1");
        write(
            widget.record.clone(),
            &record.replacen("widget.luau", "old.luau", 1),
        );
        assert_eq!(widget.is_outdated(), Ok(true), "a file renamed since");
        write(widget.record.clone(), &format!("{record}0a dropped.luau\n"));
        assert_eq!(widget.is_outdated(), Ok(true), "a file dropped since");
        let (sum, rest) = record.split_once("\n").unwrap().1.split_once(' ').unwrap();
        let changed = format!("{:016x} {rest}", u64::from_str_radix(sum, 16).unwrap() ^ 1);
        write(widget.record.clone(), &changed);
        assert_eq!(widget.is_outdated(), Ok(true), "a file changed since");

        let unversioned = current.replace(&format!("{ours}\n"), "");
        write(
            widget.record.clone(),
            &format!("{unversioned}0a dropped.luau\n"),
        );
        assert_eq!(
            widget.is_outdated(),
            Ok(true),
            "a record from before versions"
        );

        fs::remove_dir_all(&widget.dir).unwrap();
        assert_eq!(widget.is_outdated(), Ok(false), "deleted by hand");
    }

    #[test]
    fn outdated_message_colours_only_when_asked() {
        let plain = "The Noctalia widget is out of date. Run: rogctl configure noctalia";
        assert_eq!(outdated_message(Desktop::Noctalia, false), plain);
        let colored = outdated_message(Desktop::Noctalia, true);
        assert!(colored.starts_with("\x1b[1;33mThe Noctalia"));
        let stripped = ["\x1b[1;33m", "\x1b[1m", "\x1b[0m"]
            .iter()
            .fold(colored, |text, code| text.replace(code, ""));
        assert_eq!(stripped, plain);
    }

    #[test]
    fn listing_marks_installed_widgets() {
        let mut statuses = Desktop::ALL.map(|d| (d, Status::Absent));
        let plain = listing(&statuses, false);
        assert!(
            plain.lines().all(|l| l == l.trim_end()),
            "no trailing spaces"
        );
        assert!(!plain.contains("installed\n"));

        statuses[0].1 = Status::Installed;
        statuses[2].1 = Status::Outdated;
        let text = listing(&statuses, false);
        let lines: Vec<&str> = text.lines().collect();
        let noctalia = lines.iter().find(|l| l.starts_with("  noctalia"));
        let quickshell = lines.iter().find(|l| l.starts_with("  quickshell"));
        assert!(noctalia.is_some_and(|l| l.ends_with("  installed")));
        assert!(quickshell.is_some_and(|l| l.ends_with("  out of date")));
        assert!(lines.contains(&"  dms         DankMaterialShell"));
        assert_eq!(
            noctalia.map(|l| l.rfind("  installed")),
            quickshell.map(|l| l.rfind("  out of date")),
            "the marks line up"
        );
        assert!(text.ends_with(&format!(
            "\n\n{}\n",
            outdated_message(Desktop::Quickshell, false)
        )));
        assert_eq!(text.matches("out of date. Run").count(), 1);
    }

    #[test]
    fn installed_goes_by_the_widgets_own_files() {
        let temp = TempDir::new().unwrap();
        let widget = copied(&temp, true);
        assert_eq!(widget.is_installed(), Ok(false));
        widget.install(false).unwrap();
        assert_eq!(widget.is_installed(), Ok(true));
        widget.remove(false).unwrap();
        assert_eq!(widget.is_installed(), Ok(false));

        // Quickshell's settings stay after a removal; they don't count.
        let quickshell = Copied {
            bundle: &QUICKSHELL,
            ..copied(&temp, false)
        };
        write(quickshell.dir.join("RogMouseSettings.qml"), "edited");
        assert_eq!(quickshell.is_installed(), Ok(false));
        write(
            quickshell.record.clone(),
            &format!(
                "version 2.6.0\n{:016x} RogMouse.qml\n{:016x} RogMouseSettings.qml\n",
                checksum(b"old"),
                checksum(b"old settings")
            ),
        );
        assert_eq!(quickshell.is_installed(), Ok(false), "removed by hand");
        assert_eq!(quickshell.is_outdated(), Ok(false), "removed by hand");
        fs::remove_file(&quickshell.record).unwrap();
        write(quickshell.dir.join("RogMouse.qml"), "by hand");
        assert_eq!(quickshell.is_installed(), Ok(true), "no record");
    }

    #[test]
    fn compares_versions_by_number() {
        assert!(is_older(Some("2.3.1"), "2.4.0"));
        assert!(!is_older(Some("2.10.0"), "2.4.0"));
        assert!(!is_older(Some("2.4.0"), "2.4.0"));
        assert!(is_older(None, "2.4.0"));
        assert!(
            is_older(Some("2.4.0-rc1"), "2.4.0"),
            "unreadable counts as older"
        );
        assert!(!is_older(Some("2.3.1"), "2.5.0-rc.1"), "unless it's ours");
    }

    #[test]
    fn package_outdated_needs_older_files() {
        let temp = TempDir::new().unwrap();
        let dir = temp.0.join("plasmoid");
        let pointer = "/KPlugin/Version";
        write_files(&dir, &KDE).unwrap();
        assert!(!package_outdated(&dir, &KDE, pointer));
        write(dir.join("contents/ui/main.qml"), "changed");
        assert!(
            !package_outdated(&dir, &KDE, pointer),
            "this version, from a checkout"
        );
        write(
            dir.join("metadata.json"),
            r#"{"KPlugin": {"Version": "99.0.0"}}"#,
        );
        assert!(!package_outdated(&dir, &KDE, pointer), "newer");
        write(
            dir.join("metadata.json"),
            r#"{"KPlugin": {"Version": "2.3.1"}}"#,
        );
        assert!(package_outdated(&dir, &KDE, pointer));
    }

    #[test]
    fn differs_compares_installed_files() {
        let temp = TempDir::new().unwrap();
        let dir = temp.0.join("plasmoid");
        assert!(!differs(&dir, &KDE), "not installed");
        write_files(&dir, &KDE).unwrap();
        assert!(!differs(&dir, &KDE));
        write(dir.join("contents/ui/main.qml"), "older");
        assert!(differs(&dir, &KDE));
        fs::remove_file(dir.join("contents/ui/main.qml")).unwrap();
        assert!(differs(&dir, &KDE), "a file added since");
    }

    /// A plugin directory still linked to a checkout: nothing in the checkout
    /// is written or deleted, --force included.
    #[test]
    fn refuses_a_linked_widget_directory() {
        let temp = TempDir::new().unwrap();
        let widget = copied(&temp, true);
        let checkout = temp.0.join("checkout");
        write_files(&checkout, &NOCTALIA).unwrap();
        write(checkout.join("widget.luau"), "edited in the checkout");
        std::os::unix::fs::symlink(&checkout, &widget.dir).unwrap();

        for force in [false, true] {
            let err = widget.remove(force).unwrap_err();
            assert!(err.contains("symbolic link"), "{err}");
            let err = widget.install(force).unwrap_err();
            assert!(err.contains("symbolic link"), "{err}");
        }
        assert!(checkout.join("plugin.toml").is_file());
        assert_eq!(
            fs::read_to_string(checkout.join("widget.luau")).unwrap(),
            "edited in the checkout"
        );
        assert!(!widget.record.exists());
    }

    /// A link at a file, or at a directory below the widget's, dangling or
    /// not, is neither written through nor deleted.
    #[test]
    fn refuses_a_link_at_a_file() {
        let temp = TempDir::new().unwrap();
        let widget = copied(&temp, false);
        let target = temp.0.join("elsewhere/target.luau");
        fs::create_dir_all(&widget.dir).unwrap();
        std::os::unix::fs::symlink(&target, widget.dir.join("widget.luau")).unwrap();
        for force in [false, true] {
            assert!(widget.install(force).unwrap_err().contains("symbolic link"));
        }
        assert!(!target.exists(), "not created through the link");
        assert!(!widget.dir.join("plugin.toml").exists(), "nothing written");

        fs::remove_file(widget.dir.join("widget.luau")).unwrap();
        widget.install(false).unwrap();
        fs::remove_file(widget.dir.join("widget.luau")).unwrap();
        write(target.clone(), "elsewhere");
        std::os::unix::fs::symlink(&target, widget.dir.join("widget.luau")).unwrap();
        for force in [false, true] {
            assert!(widget.remove(force).unwrap_err().contains("symbolic link"));
        }
        assert_eq!(fs::read_to_string(&target).unwrap(), "elsewhere");
        assert!(widget.dir.join("plugin.toml").is_file(), "nothing deleted");

        fs::remove_file(widget.dir.join("widget.luau")).unwrap();
        let translations = widget.dir.join("translations");
        fs::remove_dir_all(&translations).unwrap();
        fs::create_dir_all(temp.0.join("elsewhere/translations")).unwrap();
        std::os::unix::fs::symlink(temp.0.join("elsewhere/translations"), &translations).unwrap();
        assert!(widget.install(true).unwrap_err().contains("symbolic link"));
        assert!(!temp.0.join("elsewhere/translations/en.json").exists());
    }

    /// Removing counts a file as rogctl's by the same rule as installing:
    /// this version's contents, or what the record says.
    #[test]
    fn removes_this_versions_files_whatever_the_record_says() {
        let temp = TempDir::new().unwrap();
        let widget = copied(&temp, true);
        widget.install(false).unwrap();
        let old = checksum(b"older");
        write(
            widget.record.clone(),
            &NOCTALIA
                .files
                .iter()
                .map(|f| format!("{old:016x} {}\n", f.path))
                .collect::<String>(),
        );
        assert_eq!(widget.remove(false), Ok(true));
        assert!(!widget.dir.exists());
    }

    /// An install that fails partway records what it wrote, so removing it
    /// afterwards deletes those files too.
    #[test]
    fn a_failed_install_records_what_it_wrote() {
        let temp = TempDir::new().unwrap();
        let widget = copied(&temp, true);
        let old = checksum(b"version 1");
        write(widget.dir.join("battery.luau"), "version 1");
        write(
            widget.record.clone(),
            &format!("{old:016x} battery.luau\nversion 2.0.0\n"),
        );
        // translations/en.json can't be written.
        let translations = widget.dir.join("translations");
        fs::create_dir_all(&translations).unwrap();
        let mode = |m| {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&translations, fs::Permissions::from_mode(m)).unwrap();
        };
        mode(0o555);
        if fs::write(translations.join("probe"), "").is_ok() {
            // Running as root, which permissions don't stop.
            mode(0o755);
            return;
        }

        assert!(widget.install(false).is_err());
        mode(0o755);
        let record = widget.read_record().unwrap().unwrap();
        for path in ["battery.luau", "plugin.toml", "service.luau"] {
            assert!(
                record.contains(&(path.to_owned(), widget.shipped(path).unwrap())),
                "{path} is recorded"
            );
        }
        assert_eq!(widget.is_outdated(), Ok(true), "half updated");
        assert_eq!(widget.remove(false), Ok(true));
        for path in ["battery.luau", "plugin.toml", "service.luau"] {
            assert!(!widget.dir.join(path).exists(), "{path} is removed");
        }
    }

    /// An install over files copied by hand (no record) that fails partway
    /// leaves a record that still covers every file of the widget, so it
    /// still reads as installed and removing it deletes them all.
    #[test]
    fn a_failed_install_without_a_record_keeps_every_file() {
        use std::os::unix::fs::PermissionsExt;
        for stuck in ["translations/en.json", "battery.luau"] {
            let temp = TempDir::new().unwrap();
            let widget = copied(&temp, true);
            for file in NOCTALIA.files {
                write_creating_dirs(&widget.dir.join(file.path), file.contents).unwrap();
            }
            let path = widget.dir.join(stuck);
            fs::set_permissions(&path, fs::Permissions::from_mode(0o444)).unwrap();
            if fs::OpenOptions::new().write(true).open(&path).is_ok() {
                // Running as root, which permissions don't stop.
                return;
            }

            assert!(widget.install(false).is_err(), "{stuck}");
            assert_eq!(widget.is_installed(), Ok(true), "{stuck}");
            assert_eq!(widget.remove(false), Ok(true), "{stuck}");
            assert!(!widget.dir.exists(), "{stuck}: every file removed");
        }
    }

    /// An edited file a new version no longer ships is left, and kept in the
    /// record, unless --force removes it.
    #[test]
    fn removes_an_edited_dropped_file_only_with_force() {
        let temp = TempDir::new().unwrap();
        let widget = copied(&temp, true);
        write(widget.dir.join("kept.luau"), "edited");
        write(
            widget.record.clone(),
            &format!("{:016x} kept.luau\n", checksum(b"version 1")),
        );

        let err = widget.install(false).unwrap_err();
        assert!(err.starts_with("kept.luau in"), "{err}");
        assert!(err.contains("--force to remove it"), "{err}");
        assert!(widget.dir.join("kept.luau").is_file());
        assert!(!widget.dir.join("plugin.toml").exists(), "nothing written");

        widget.install(true).unwrap();
        assert!(!widget.dir.join("kept.luau").exists());
        assert!(widget.dir.join("plugin.toml").is_file());
    }

    #[test]
    fn tells_a_fresh_install_from_an_update() {
        let temp = TempDir::new().unwrap();
        let widget = copied(&temp, true);
        assert_eq!(widget.previous(), Ok(Previous::Nothing));
        widget.install(false).unwrap();
        assert_eq!(widget.previous(), Ok(Previous::ThisVersion));

        let current = fs::read_to_string(&widget.record).unwrap();
        write(
            widget.record.clone(),
            &current.replacen("widget.luau", "old.luau", 1),
        );
        assert_eq!(widget.previous(), Ok(Previous::Other));
        fs::remove_file(&widget.record).unwrap();
        assert_eq!(widget.previous(), Ok(Previous::Other), "no record");
        fs::remove_dir_all(&widget.dir).unwrap();
        assert_eq!(widget.previous(), Ok(Previous::Nothing));
    }

    /// DankMaterialShell 2.0 reads `dms`, and leaves a link at the old name.
    #[test]
    fn dms_plugins_goes_where_dms_reads() {
        let temp = TempDir::new().unwrap();
        let config = temp.0.join("config");
        assert_eq!(dms_plugins(&config), config.join("dms/plugins"));
        fs::create_dir_all(config.join("DankMaterialShell/plugins")).unwrap();
        assert_eq!(
            dms_plugins(&config),
            config.join("DankMaterialShell/plugins"),
            "before 2.0"
        );
        fs::rename(config.join("DankMaterialShell"), config.join("dms")).unwrap();
        std::os::unix::fs::symlink("dms", config.join("DankMaterialShell")).unwrap();
        assert_eq!(dms_plugins(&config), config.join("dms/plugins"));
    }

    /// Before 2.0, a `DankMaterialShell` linked from dotfiles is still the
    /// directory DMS reads; making a `dms` beside it would hide it from 2.0's
    /// move as well.
    #[test]
    fn dms_plugins_follows_a_linked_legacy_dir() {
        let temp = TempDir::new().unwrap();
        let config = temp.0.join("config");
        fs::create_dir_all(temp.0.join("dotfiles/dank")).unwrap();
        fs::create_dir_all(&config).unwrap();
        std::os::unix::fs::symlink("../dotfiles/dank", config.join("DankMaterialShell")).unwrap();
        assert_eq!(
            dms_plugins(&config),
            config.join("DankMaterialShell/plugins")
        );
        // A link to nothing isn't a directory DMS reads.
        fs::remove_dir(temp.0.join("dotfiles/dank")).unwrap();
        assert_eq!(dms_plugins(&config), config.join("dms/plugins"));
    }

    #[test]
    fn record_rejects_paths_outside_the_directory() {
        let entries = parse_record("# header\n0a ok.qml\n0b ../escape\n0c /abs\nzz bad.qml\n");
        assert_eq!(entries, [("ok.qml".to_owned(), 10)]);
    }
}
