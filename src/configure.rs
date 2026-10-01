//! `rogctl configure`: installs or removes a desktop's battery widget. The
//! widget files are built into the binary, so this needs no checkout of the
//! repository, and the widget always matches the binary's version.

use std::ffi::OsString;
use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
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
/// directory goes, and Quickshell files that differ from rogctl's.
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
        Desktop::Noctalia => remove_noctalia(),
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
        Desktop::Dms => remove_dms(),
        Desktop::Kde => remove_kde(),
        Desktop::Gnome => remove_gnome(),
    }
}

fn noctalia_dir() -> Result<PathBuf> {
    Ok(data_home()?.join("noctalia/plugins/rog-mouse-battery"))
}

fn install_noctalia(force: bool) -> Result<()> {
    find_program("noctalia")?;
    let dir = noctalia_dir()?;
    let updating = exists(&dir)?;
    replace_dir(&dir, &NOCTALIA, force)?;
    say!("Copied the plugin to {}", dir.display());
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

fn remove_noctalia() -> Result<()> {
    let dir = noctalia_dir()?;
    if !exists(&dir)? {
        return not_installed(Desktop::Noctalia);
    }
    // Noctalia may not be running; the files go either way.
    if let Err(e) =
        run_tool(Command::new("noctalia").args(["msg", "plugins", "disable", NOCTALIA_ID]))
    {
        eprintln!("{e}; removing the plugin anyway");
    }
    remove_path(&dir)?;
    say!("Removed {}", dir.display());
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
fn quickshell_dir() -> Result<PathBuf> {
    let dir = config_home()?.join("quickshell");
    if dir.join("shell.qml").is_file() {
        Ok(dir)
    } else {
        Err(format!(
            "there's no shell.qml in {}. If your config is elsewhere, copy RogMouse.qml and \
             RogMouseWidget.qml from https://github.com/humblemonk/rogctl/tree/main/quickshell \
             next to its shell.qml",
            dir.display()
        ))
    }
}

fn install_quickshell(force: bool) -> Result<()> {
    let dir = quickshell_dir()?;
    refuse_edited(&dir, &QUICKSHELL, force, "overwrite")?;
    write_files(&dir, &QUICKSHELL)?;
    say!(
        "Copied RogMouse.qml and RogMouseWidget.qml to {}\n\
         Now put RogMouseWidget {{}} in your bar. Its settings are at the top of RogMouse.qml.",
        dir.display()
    );
    Ok(())
}

fn remove_quickshell(force: bool) -> Result<()> {
    let dir = quickshell_dir()?;
    refuse_edited(&dir, &QUICKSHELL, force, "remove")?;
    let mut removed = false;
    for file in QUICKSHELL.files {
        removed |= remove_path(&dir.join(file.path))?;
    }
    if !removed {
        return not_installed(Desktop::Quickshell);
    }
    say!(
        "Removed RogMouse.qml and RogMouseWidget.qml from {}\n\
         Take RogMouseWidget {{}} out of your bar too.",
        dir.display()
    );
    Ok(())
}

/// Errs if any of `bundle`'s files in `dir` differ from rogctl's: the user may
/// have changed them.
fn refuse_edited(dir: &Path, bundle: &Bundle, force: bool, verb: &str) -> Result<()> {
    if force {
        return Ok(());
    }
    let edited: Vec<&str> = bundle
        .files
        .iter()
        .filter(|f| fs::read(dir.join(f.path)).is_ok_and(|ours| ours != f.contents))
        .map(|f| f.path)
        .collect();
    if edited.is_empty() {
        return Ok(());
    }
    let (is, them) = if edited.len() == 1 {
        ("is", "it")
    } else {
        ("are", "them")
    };
    Err(format!(
        "{} in {} {is} different from rogctl's copy. Pass --force to {verb} {them} anyway",
        edited.join(" and "),
        dir.display()
    ))
}

fn dms_dir() -> Result<PathBuf> {
    Ok(config_home()?.join("DankMaterialShell/plugins/RogMouseBattery"))
}

fn install_dms(force: bool) -> Result<()> {
    find_program("dms")?;
    let dir = dms_dir()?;
    replace_dir(&dir, &DMS, force)?;
    say!("Copied the plugin to {}", dir.display());
    run_tool(Command::new("dms").args(["ipc", "plugin-scan", "scan"]))?;
    say!("Now enable {WIDGET_NAME} in Settings → Plugins, then add it to the bar.");
    Ok(())
}

fn remove_dms() -> Result<()> {
    let dir = dms_dir()?;
    if !remove_path(&dir)? {
        return not_installed(Desktop::Dms);
    }
    say!("Removed {}", dir.display());
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

/// Fills `dir` with `bundle`'s files and nothing else, so files an older
/// version had don't linger. Won't replace a link without `force`: it's likely
/// a developer's link into a checkout, as the widget readmes once suggested.
fn replace_dir(dir: &Path, bundle: &Bundle, force: bool) -> Result<()> {
    if let Ok(meta) = fs::symlink_metadata(dir)
        && meta.is_symlink()
        && !force
    {
        return Err(format!(
            "{} is a link, perhaps to a rogctl checkout. Pass --force to replace it with a copy",
            dir.display()
        ));
    }
    remove_path(dir)?;
    write_files(dir, bundle)
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

/// Deletes a file or directory; a link, not what it points to. Returns whether
/// there was anything to delete.
fn remove_path(path: &Path) -> Result<bool> {
    let removed = match fs::symlink_metadata(path) {
        Ok(meta) if meta.is_dir() => fs::remove_dir_all(path),
        Ok(_) => fs::remove_file(path),
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(false),
        Err(e) => Err(e),
    };
    removed
        .map(|()| true)
        .map_err(|e| format!("{}: {e}", path.display()))
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

    fn read(path: PathBuf) -> String {
        fs::read_to_string(path).unwrap()
    }

    #[test]
    fn replace_dir_drops_old_files() {
        let temp = TempDir::new().unwrap();
        let dir = temp.0.join("plugin");
        fs::create_dir_all(dir.join("translations")).unwrap();
        fs::write(dir.join("old.luau"), "from an older version").unwrap();
        fs::write(dir.join("plugin.toml"), "edited").unwrap();

        replace_dir(&dir, &NOCTALIA, false).unwrap();
        assert!(!dir.join("old.luau").exists());
        assert_eq!(
            read(dir.join("plugin.toml")).as_bytes(),
            NOCTALIA.files[1].contents
        );
        assert!(dir.join("translations/en.json").is_file());
    }

    /// Writing through a link would overwrite the checkout it points to.
    #[test]
    fn replace_dir_leaves_a_linked_checkout_alone() {
        let temp = TempDir::new().unwrap();
        let checkout = temp.0.join("checkout");
        fs::create_dir(&checkout).unwrap();
        fs::write(checkout.join("widget.luau"), "work in progress").unwrap();
        let link = temp.0.join("plugin");
        std::os::unix::fs::symlink(&checkout, &link).unwrap();

        assert!(replace_dir(&link, &NOCTALIA, false).is_err());
        assert!(link.is_symlink());

        replace_dir(&link, &NOCTALIA, true).unwrap();
        assert!(!link.is_symlink() && link.join("plugin.toml").is_file());
        assert_eq!(read(checkout.join("widget.luau")), "work in progress");
        assert!(!checkout.join("plugin.toml").exists());

        assert_eq!(remove_path(&link), Ok(true));
        assert_eq!(remove_path(&link), Ok(false));
    }

    #[test]
    fn refuses_to_touch_edited_files() {
        let temp = TempDir::new().unwrap();
        assert!(refuse_edited(&temp.0, &QUICKSHELL, false, "overwrite").is_ok());

        write_files(&temp.0, &QUICKSHELL).unwrap();
        assert!(
            refuse_edited(&temp.0, &QUICKSHELL, false, "overwrite").is_ok(),
            "unchanged copies can go"
        );

        fs::write(temp.0.join("RogMouse.qml"), "edited").unwrap();
        let err = refuse_edited(&temp.0, &QUICKSHELL, false, "remove").unwrap_err();
        assert!(err.starts_with("RogMouse.qml in"), "{err}");
        assert!(refuse_edited(&temp.0, &QUICKSHELL, true, "remove").is_ok());
    }
}
