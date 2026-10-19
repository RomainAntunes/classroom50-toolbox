use std::path::{Path, PathBuf};

use super::{CommandSpec, InstallPlan, Os, home_dir, install};
use crate::error::{Error, Result};
use crate::process;
use crate::tools::Tool;

pub const OS: Os = Os::Linux;

pub fn os_version() -> Option<String> {
    ["/etc/os-release", "/usr/lib/os-release"]
        .iter()
        .find_map(|path| std::fs::read_to_string(path).ok())
        .and_then(|text| super::os_version::linux(&text))
}

pub fn extra_path_dirs() -> Vec<PathBuf> {
    let mut dirs = vec![
        PathBuf::from("/usr/local/bin"),
        PathBuf::from("/home/linuxbrew/.linuxbrew/bin"),
    ];
    if let Some(home) = home_dir() {
        dirs.push(home.join(".local/bin"));
    }
    dirs
}

/// Lines apt, dnf, pacman and zypper print at each step (in English: `pkexec` drops the
/// locale only on some systems, so elsewhere the bar stays indeterminate).
pub const INSTALL_STEPS: &[(&str, f32)] = &[
    ("Reading package lists", 0.05),
    ("Hit:", 0.1),
    ("Get:", 0.2),
    ("Downloading Packages", 0.2),
    ("downloading ", 0.2),
    ("Retrieving: ", 0.2),
    ("Unpacking", 0.6),
    ("Running transaction", 0.6),
    ("installing ", 0.7),
    ("Installing", 0.7),
    ("Setting up", 0.8),
    ("Processing triggers", 0.9),
    ("Complete!", 0.95),
];

pub fn install_plan(tool: Tool) -> Result<InstallPlan> {
    let has = |program: &str| process::find_in_path(program).is_some();
    // An ostree deployment (Fedora Atomic, Bazzite, Bluefin) or NixOS: the package manager
    // can't add to the running system.
    let immutable = Path::new("/run/ostree-booted").exists() || Path::new("/etc/NIXOS").exists();
    install::linux(
        tool,
        &install::Probe {
            has: &has,
            immutable,
        },
    )
}

pub fn is_stub(_program: &str, _path: &Path) -> bool {
    false
}

/// Terminals open in the working directory they are spawned from.
/// Where editors keep their settings: the XDG config dir, plus the Flatpak sandboxes of
/// VS Code and VSCodium.
pub fn editor_config_dirs() -> Vec<PathBuf> {
    let home = home_dir();
    let xdg = std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .filter(|dir| dir.is_absolute())
        .or_else(|| home.as_ref().map(|home| home.join(".config")));
    let flatpaks = home.iter().flat_map(|home| {
        ["com.visualstudio.code", "com.vscodium.codium"]
            .map(|app| home.join(".var/app").join(app).join("config"))
    });
    xdg.into_iter().chain(flatpaks).collect()
}

/// Flatpak ids per editor, run with `flatpak run`.
fn editor_flatpaks(id: &str) -> &'static [&'static str] {
    match id {
        "idea" => &["com.jetbrains.IntelliJ-IDEA-Ultimate"],
        "idea-ce" => &["com.jetbrains.IntelliJ-IDEA-Community"],
        "clion" => &["com.jetbrains.CLion"],
        "pycharm" => &["com.jetbrains.PyCharm-Professional"],
        "pycharm-ce" => &["com.jetbrains.PyCharm-Community"],
        "webstorm" => &["com.jetbrains.WebStorm"],
        "goland" => &["com.jetbrains.GoLand"],
        "phpstorm" => &["com.jetbrains.PhpStorm"],
        "rider" => &["com.jetbrains.Rider"],
        "rubymine" => &["com.jetbrains.RubyMine"],
        "datagrip" => &["com.jetbrains.DataGrip"],
        "android-studio" => &["com.google.AndroidStudio"],
        "vscode" => &["com.visualstudio.code"],
        "vscodium" => &["com.vscodium.codium"],
        "zed" => &["dev.zed.Zed"],
        "sublime" => &["com.sublimetext.three"],
        "netbeans" => &["org.apache.netbeans"],
        _ => &[],
    }
}

/// The editor's CLI from the JetBrains Toolbox scripts, PATH or `/snap/bin`, else its
/// Flatpak (system-wide or per user).
pub fn editor_launcher(id: &str) -> Option<CommandSpec> {
    let home = home_dir();
    let mut dirs = vec![PathBuf::from("/snap/bin")];
    if let Some(home) = &home {
        dirs.insert(0, home.join(".local/share/JetBrains/Toolbox/scripts"));
    }
    for name in super::editor_commands(id) {
        if let Some(path) = dirs.iter().map(|dir| dir.join(name)).find(|p| p.is_file()) {
            return Some(CommandSpec::new(&path.to_string_lossy(), &[]));
        }
        if let Some(path) = process::find_in_path(name) {
            return Some(CommandSpec::new(&path.to_string_lossy(), &[]));
        }
    }
    let mut installs = vec![PathBuf::from("/var/lib/flatpak/app")];
    if let Some(home) = &home {
        installs.push(home.join(".local/share/flatpak/app"));
    }
    let app = editor_flatpaks(id)
        .iter()
        .find(|app| installs.iter().any(|dir| dir.join(app).is_dir()))?;
    Some(CommandSpec::new("flatpak", &["run", app]))
}

/// The XDG data dir (Zed, NetBeans), plus Zed's Flatpak sandbox.
pub fn editor_data_dirs() -> Vec<PathBuf> {
    let home = home_dir();
    let xdg = std::env::var_os("XDG_DATA_HOME")
        .map(PathBuf::from)
        .filter(|dir| dir.is_absolute())
        .or_else(|| home.as_ref().map(|home| home.join(".local/share")));
    let flatpak = home.map(|home| home.join(".var/app/dev.zed.Zed/data"));
    xdg.into_iter().chain(flatpak).collect()
}

pub fn open_terminal(dir: &Path) -> Result<()> {
    for terminal in [
        "x-terminal-emulator",
        "gnome-terminal",
        "konsole",
        "kgx",
        "xfce4-terminal",
        "xterm",
    ] {
        if process::find_in_path(terminal).is_some() {
            return process::spawn_detached::<[&str; 0], &str>(terminal, [], Some(dir));
        }
    }
    Err(Error::NoTerminal)
}

pub fn global_shortcuts_supported() -> bool {
    let var = |name: &str| std::env::var(name).ok();
    !super::is_wayland(
        var("XDG_SESSION_TYPE").as_deref(),
        var("WAYLAND_DISPLAY").as_deref(),
    )
}

mod calendar;

pub use calendar::{has_native_calendar, native_calendars, write_calendar};

pub fn set_login_item(_enabled: bool) -> Option<Result<()>> {
    None
}

pub fn login_item_enabled() -> Option<bool> {
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::editors::ALL;

    #[test]
    fn every_editor_but_eclipse_and_visual_studio_can_be_found() {
        for editor in ALL {
            let found = !super::super::editor_commands(editor.id).is_empty()
                || !editor_flatpaks(editor.id).is_empty();
            let expected = !matches!(editor.id, "eclipse" | "visual-studio");
            assert_eq!(found, expected, "{}", editor.id);
        }
    }
}
