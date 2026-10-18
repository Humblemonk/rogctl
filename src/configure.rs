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
const WIDGET_NAME: &str = "ASUS Mouse Battery";
const NOCTALIA_ID: &str = "humblemonk/rog-mouse-battery";
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

/// `println!`, minus the panic when stdout is closed, as in
/// `rogctl configure waybar | head`.
macro_rules! say {
    ($($arg:tt)*) => {{
        let _ = writeln!(io::stdout(), $($arg)*);
    }};
}

/// `desktop`: `None` lists the choices. `force` overwrites or deletes files the
/// user changed.
pub fn run(desktop: Option<&str>, remove: bool, force: bool) -> ExitCode {
    let Some(name) = desktop else {
        let _ = io::stdout().write_all(choices(color(&io::stdout())).as_bytes());
        return ExitCode::SUCCESS;
    };
    let Some(desktop) = Desktop::parse(name) else {
        eprint!(
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
            eprintln!("rogctl configure: {e}");
            ExitCode::FAILURE
        }
    }
}

/// Whether to colour what goes to `stream`: only a terminal, and not with
/// `NO_COLOR` set (<https://no-color.org>).
fn color(stream: &impl IsTerminal) -> bool {
    stream.is_terminal() && std::env::var_os("NO_COLOR").is_none_or(|v| v.is_empty())
}

fn choices(color: bool) -> String {
    let mut text = "\
Usage: rogctl configure DESKTOP [--remove] [--force]

Installs the battery widget for DESKTOP, or with --remove removes it:
"
    .to_owned();
    for desktop in Desktop::ALL {
        text += &format!("  {:<12}{}\n", desktop.name(), desktop.description());
    }
    for desktop in outdated() {
        text += &format!("\n{}\n", outdated_message(desktop, color));
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
                "rogctl doesn't edit your Waybar config. To remove the module, take \
                 \"custom/rog-mouse\" out of your modules list, delete the module and the \
                 #custom-rog-mouse rules from style.css, then reload Waybar: \
                 pkill -SIGUSR2 waybar"
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
    let updating = exists(&noctalia.dir)?;
    noctalia.install(force)?;
    say!("Copied the plugin to {}", noctalia.dir.display());
    run_tool(Command::new("noctalia").args(["msg", "plugins", "enable", NOCTALIA_ID]))?;
    if updating {
        // Settings in plugin.toml only load with the config.
        run_tool(Command::new("noctalia").args(["msg", "config-reload"]))?;
        say!("Updated the {WIDGET_NAME} plugin.");
    } else {
        say!("Now add {WIDGET_NAME} from the bar's Add-widget picker.");
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
        eprintln!("{e}; removing the plugin anyway");
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
        "rogctl doesn't edit your Waybar config. To add the module:\n\n\
         1. Add this module to your Waybar config:\n\n{}\n\
         2. Add \"custom/rog-mouse\" to modules-left, modules-center or modules-right.\n\n\
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
        "Removed RogMouse.qml and RogMouseWidget.qml from {}\n\
         Take RogMouseWidget {{}} out of your bar too.",
        quickshell.dir.display()
    );
    Ok(())
}

fn dms() -> Result<Copied> {
    Ok(Copied {
        dir: config_home()?.join("DankMaterialShell/plugins/RogMouseBattery"),
        own_dir: true,
        bundle: &DMS,
        record: record_path(Desktop::Dms)?,
    })
}

fn install_dms(force: bool) -> Result<()> {
    find_program("dms")?;
    let dms = dms()?;
    dms.install(force)?;
    say!("Copied the plugin to {}", dms.dir.display());
    run_tool(Command::new("dms").args(["ipc", "plugin-scan", "scan"]))?;
    say!("Now enable {WIDGET_NAME} in Settings → Plugins, then add it to the bar.");
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
        eprintln!("{e}");
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
    run_tool(Command::new("gnome-extensions").args(["uninstall", GNOME_UUID]))
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
        Desktop::Waybar => say!("If Waybar can't find rogctl, put {exe} in the exec line."),
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
        let record = self.read_record()?.unwrap_or_default();
        let recorded = |path: &str| record.iter().find(|(p, _)| p == path).map(|&(_, sum)| sum);

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
        if !force {
            self.refuse(&edited, "overwrite")?;
        }
        // Files an earlier version wrote and this one dropped go, unless the
        // user changed them.
        let mut obsolete = Vec::new();
        for (path, sum) in &record {
            if self.bundle.files.iter().any(|f| f.path == path) {
                continue;
            }
            match self.find(path, &[Some(*sum)])? {
                Found::Missing => {}
                Found::Ours => obsolete.push(path.clone()),
                Found::Edited => say!(
                    "Left {}: you changed it, and this version doesn't use it.",
                    self.dir.join(path).display()
                ),
            }
        }

        let mut entries = Vec::new();
        let mut written = Vec::new();
        for file in self.bundle.files {
            let ours = [Some(checksum(file.contents)), recorded(file.path)];
            if self.bundle.is_users(file.path) && self.find(file.path, &ours)? == Found::Edited {
                // The user's settings stay; the record keeps what rogctl
                // wrote there, if it wrote them.
                if let Some(sum) = recorded(file.path) {
                    entries.push((file.path.to_owned(), sum));
                }
                continue;
            }
            let path = self.dir.join(file.path);
            write_creating_dirs(&path, file.contents)
                .map_err(|e| format!("{}: {e}", path.display()))?;
            entries.push((file.path.to_owned(), checksum(file.contents)));
            written.push(file.path);
        }
        self.delete(&obsolete)?;
        self.write_record(&entries)?;
        Ok(written)
    }

    /// The files `remove` deletes: those rogctl wrote. Errs if the user changed
    /// any, unless `force`.
    fn removable(&self, force: bool) -> Result<Vec<String>> {
        // With no record, the files this version ships are what rogctl wrote.
        let entries = self.read_record()?.unwrap_or_else(|| self.bundle_entries());
        let mut ours = Vec::new();
        let mut edited = Vec::new();
        let mut settings = Vec::new();
        for (path, sum) in entries {
            match self.find(&path, &[Some(sum)])? {
                Found::Missing => {}
                Found::Ours => ours.push(path),
                // The user's settings stay, even with --force.
                Found::Edited if self.bundle.is_users(&path) => settings.push(path),
                Found::Edited => edited.push(path),
            }
        }
        if !force {
            self.refuse(&edited, "remove")?;
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
    /// `false` with no record, or once every recorded file is gone.
    fn is_outdated(&self) -> Result<bool> {
        let Some(text) = self.read_record_text()? else {
            return Ok(false);
        };
        // The user's settings are never updated, so they don't count.
        let theirs = |entries: Vec<Entry>| -> Vec<Entry> {
            entries
                .into_iter()
                .filter(|(path, _)| !self.bundle.is_users(path))
                .collect()
        };
        let mut record = theirs(parse_record(&text));
        if !record.iter().any(|(path, _)| self.dir.join(path).exists()) {
            return Ok(false);
        }
        let mut current = theirs(self.bundle_entries());
        record.sort();
        current.sort();
        // Records written before rogctl recorded its version count as older.
        let version = text.lines().find_map(|l| l.strip_prefix("version "));
        Ok(record != current && is_older(version, env!("CARGO_PKG_VERSION")))
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

    fn refuse(&self, edited: &[String], verb: &str) -> Result<()> {
        if edited.is_empty() {
            return Ok(());
        }
        let (is, them) = if edited.len() == 1 {
            ("is", "it")
        } else {
            ("are", "them")
        };
        Err(format!(
            "{} in {} {is} different from what rogctl wrote. Pass --force to {verb} {them} \
             anyway",
            edited.join(" and "),
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

    fn write_record(&self, written: &[Entry]) -> Result<()> {
        let mut text = format!(
            "# Files `rogctl configure` wrote to {}, with their checksums.\n",
            self.dir.display()
        );
        for (path, sum) in written {
            text += &format!("{sum:016x} {path}\n");
        }
        text += &format!("version {}\n", env!("CARGO_PKG_VERSION"));
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
fn state_dir() -> Result<PathBuf> {
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
        write(widget.dir.join("kept.luau"), "edited");
        let sum = checksum(b"version 1");
        write(
            widget.record.clone(),
            &format!(
                "# header\n{sum:016x} plugin.toml\n{sum:016x} old/dropped.luau\n\
                 {sum:016x} kept.luau\n"
            ),
        );

        widget.install(false).unwrap();
        assert_eq!(
            fs::read(widget.dir.join("plugin.toml")).unwrap(),
            NOCTALIA.files[1].contents
        );
        assert!(!widget.dir.join("old").exists());
        assert!(
            widget.dir.join("kept.luau").is_file(),
            "dropped, but edited"
        );
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

        // A settings file rogctl didn't write is never recorded, so stays.
        widget.install(false).unwrap();
        assert!(
            widget
                .read_record()
                .unwrap()
                .unwrap()
                .iter()
                .all(|(p, _)| p != "RogMouseSettings.qml")
        );
        assert_eq!(widget.remove(false), Ok(true));
        assert!(settings.is_file());
    }

    /// A settings file that changed between versions isn't an update to make.
    #[test]
    fn outdated_ignores_the_users_settings() {
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
        assert_eq!(widget.is_outdated(), Ok(false), "only the settings differ");
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

    #[test]
    fn record_rejects_paths_outside_the_directory() {
        let entries = parse_record("# header\n0a ok.qml\n0b ../escape\n0c /abs\nzz bad.qml\n");
        assert_eq!(entries, [("ok.qml".to_owned(), 10)]);
    }
}
