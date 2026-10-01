//! `rogctl configure`: installs or removes a desktop's battery widget. The
//! widget files are built into the binary, so this needs no checkout of the
//! repository, and the widget always matches the binary's version.

use std::ffi::OsString;
use std::fs;
use std::io::{self, Write};
use std::path::{Component, Path, PathBuf};
use std::process::{Command, ExitCode};
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
}

macro_rules! bundle {
    ($dir:literal, [$($path:literal),+ $(,)?]) => {
        Bundle {
            dir: $dir,
            files: &[$(File {
                path: $path,
                contents: include_bytes!(concat!("../", $dir, "/", $path)),
            }),+],
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
static QUICKSHELL: Bundle = bundle!("quickshell", ["RogMouse.qml", "RogMouseWidget.qml"]);
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

/// `desktop`: `None` lists the choices. `force` replaces a link where a plugin
/// directory goes, and overwrites or deletes files the user changed.
pub fn run(desktop: Option<&str>, remove: bool, force: bool) -> ExitCode {
    let Some(name) = desktop else {
        let _ = io::stdout().write_all(choices().as_bytes());
        return ExitCode::SUCCESS;
    };
    let Some(desktop) = Desktop::parse(name) else {
        eprint!("Unknown desktop {name:?}.\n\n{}", choices());
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

fn choices() -> String {
    let mut text = "\
Usage: rogctl configure DESKTOP [--remove] [--force]

Installs the battery widget for DESKTOP, or with --remove removes it:
"
    .to_owned();
    for desktop in Desktop::ALL {
        text += &format!("  {:<12}{}\n", desktop.name(), desktop.description());
    }
    text
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
            "there's no shell.qml in {}. If your config is elsewhere, copy RogMouse.qml and \
             RogMouseWidget.qml from https://github.com/humblemonk/rogctl/tree/main/quickshell \
             next to its shell.qml",
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
    quickshell.install(force)?;
    say!(
        "Copied RogMouse.qml and RogMouseWidget.qml to {}\n\
         Now put RogMouseWidget {{}} in your bar. Its settings are at the top of RogMouse.qml.",
        quickshell.dir.display()
    );
    Ok(())
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

/// Where kpackagetool6 installs a user's Plasma widgets.
fn kde_installed() -> Result<bool> {
    exists(&data_home()?.join("plasma/plasmoids").join(KDE_ID))
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

fn gnome_installed() -> Result<bool> {
    exists(&data_home()?.join("gnome-shell/extensions").join(GNOME_UUID))
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
            say!("If it says rogctl wasn't found, set binary at the top of RogMouse.qml to {exe}")
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

/// Whether anything is at `path`, a dangling link included.
fn exists(path: &Path) -> Result<bool> {
    match fs::symlink_metadata(path) {
        Ok(_) => Ok(true),
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(false),
        Err(e) => Err(format!("{}: {e}", path.display())),
    }
}

fn write_files(dir: &Path, bundle: &Bundle) -> Result<()> {
    for file in bundle.files {
        let path = dir.join(file.path);
        let parent = path.parent().unwrap_or(dir);
        fs::create_dir_all(parent)
            .and_then(|()| fs::write(&path, file.contents))
            .map_err(|e| format!("{}: {e}", path.display()))?;
    }
    Ok(())
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
    /// one doesn't have.
    fn install(&self, force: bool) -> Result<()> {
        if self.own_dir && is_link(&self.dir) {
            if !force {
                return Err(format!(
                    "{} is a link, perhaps to a rogctl checkout. Pass --force to replace it \
                     with a copy",
                    self.dir.display()
                ));
            }
            // Only the link goes, never what it points to.
            fs::remove_file(&self.dir).map_err(|e| format!("{}: {e}", self.dir.display()))?;
        }
        let record = self.read_record()?.unwrap_or_default();
        let recorded = |path: &str| record.iter().find(|(p, _)| p == path).map(|&(_, sum)| sum);

        let mut edited = Vec::new();
        for file in self.bundle.files {
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

        write_files(&self.dir, self.bundle)?;
        self.delete(&obsolete)?;
        self.write_record()
    }

    /// The files `remove` deletes: those rogctl wrote. Errs if the user changed
    /// any, unless `force`. Empty for a link, which `remove` deletes instead.
    fn removable(&self, force: bool) -> Result<Vec<String>> {
        if self.own_dir && is_link(&self.dir) {
            return Ok(Vec::new());
        }
        // With no record, the files this version ships are what rogctl wrote.
        let entries = self.read_record()?.unwrap_or_else(|| {
            self.bundle
                .files
                .iter()
                .map(|f| (f.path.to_owned(), checksum(f.contents)))
                .collect()
        });
        let mut ours = Vec::new();
        let mut edited = Vec::new();
        for (path, sum) in entries {
            match self.find(&path, &[Some(sum)])? {
                Found::Missing => {}
                Found::Ours => ours.push(path),
                Found::Edited => edited.push(path),
            }
        }
        if !force {
            self.refuse(&edited, "remove")?;
        }
        ours.append(&mut edited);
        Ok(ours)
    }

    /// Deletes the files rogctl wrote, then the directories they leave empty.
    /// Returns whether there was anything to delete.
    fn remove(&self, force: bool) -> Result<bool> {
        if self.own_dir && is_link(&self.dir) {
            // Only the link goes, never what it points to.
            fs::remove_file(&self.dir).map_err(|e| format!("{}: {e}", self.dir.display()))?;
            self.delete_record()?;
            return Ok(true);
        }
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
        match fs::read_to_string(&self.record) {
            Ok(text) => Ok(Some(parse_record(&text))),
            Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(None),
            Err(e) => Err(format!("{}: {e}", self.record.display())),
        }
    }

    fn write_record(&self) -> Result<()> {
        let mut text = format!(
            "# Files `rogctl configure` wrote to {}, with their checksums.\n",
            self.dir.display()
        );
        for file in self.bundle.files {
            text += &format!("{:016x} {}\n", checksum(file.contents), file.path);
        }
        let parent = self.record.parent().unwrap_or(&self.dir);
        fs::create_dir_all(parent)
            .and_then(|()| fs::write(&self.record, text))
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

/// `$XDG_STATE_HOME/rogctl/DESKTOP.files`.
fn record_path(desktop: Desktop) -> Result<PathBuf> {
    let state = xdg_dir(
        std::env::var_os("XDG_STATE_HOME"),
        std::env::var_os("HOME"),
        ".local/state",
    )?;
    Ok(state.join(format!("rogctl/{}.files", desktop.name())))
}

/// 64-bit FNV-1a. Unlike std's hasher it's the same in every Rust version,
/// which a record kept across rogctl updates needs. It only has to notice
/// edits, not resist them.
fn checksum(bytes: &[u8]) -> u64 {
    bytes.iter().fold(0xcbf2_9ce4_8422_2325, |hash, &b| {
        (hash ^ u64::from(b)).wrapping_mul(0x0100_0000_01b3)
    })
}

fn is_link(path: &Path) -> bool {
    fs::symlink_metadata(path).is_ok_and(|m| m.is_symlink())
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
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, text).unwrap();
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

    /// Writing through a link would overwrite the checkout it points to.
    #[test]
    fn leaves_a_linked_checkout_alone() {
        let temp = TempDir::new().unwrap();
        let checkout = temp.0.join("checkout");
        write(checkout.join("widget.luau"), "work in progress");
        let widget = copied(&temp, true);
        std::os::unix::fs::symlink(&checkout, &widget.dir).unwrap();

        assert!(widget.install(false).is_err());
        assert!(is_link(&widget.dir));

        widget.install(true).unwrap();
        assert!(!is_link(&widget.dir) && widget.dir.join("plugin.toml").is_file());
        assert_eq!(
            fs::read_to_string(checkout.join("widget.luau")).unwrap(),
            "work in progress"
        );
        assert!(!checkout.join("plugin.toml").exists());

        fs::remove_dir_all(&widget.dir).unwrap();
        std::os::unix::fs::symlink(&checkout, &widget.dir).unwrap();
        assert_eq!(widget.remove(false), Ok(true));
        assert!(!widget.dir.exists() && checkout.join("widget.luau").is_file());
    }

    #[test]
    fn record_rejects_paths_outside_the_directory() {
        let entries = parse_record("# header\n0a ok.qml\n0b ../escape\n0c /abs\nzz bad.qml\n");
        assert_eq!(entries, [("ok.qml".to_owned(), 10)]);
    }
}
