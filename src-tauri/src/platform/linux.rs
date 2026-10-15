use std::path::{Path, PathBuf};

use super::{CommandSpec, Os, home_dir};
use crate::error::{Error, Result};
use crate::process;
use crate::tools::Tool;

pub const OS: Os = Os::Linux;

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

/// Package-manager installs need root; `pkexec` shows the desktop's own polkit prompt.
pub fn install_command(tool: Tool) -> Result<CommandSpec> {
    let package = match tool {
        Tool::Git => "git",
        Tool::Gh => "gh",
        Tool::GhStudent => {
            return Err(Error::Unsupported(
                "gh-student is installed through gh".into(),
            ));
        }
    };
    let (manager, install_args): (&str, &[&str]) = if process::find_in_path("apt-get").is_some() {
        ("apt-get", &["install", "-y"])
    } else if process::find_in_path("dnf").is_some() {
        ("dnf", &["install", "-y"])
    } else if process::find_in_path("pacman").is_some() {
        ("pacman", &["-S", "--noconfirm"])
    } else if process::find_in_path("zypper").is_some() {
        ("zypper", &["--non-interactive", "install"])
    } else {
        return Err(Error::Unsupported(
            "no supported package manager found (apt, dnf, pacman, zypper)".into(),
        ));
    };
    // Arch names the GitHub CLI package `github-cli`.
    let package = if manager == "pacman" && package == "gh" {
        "github-cli"
    } else {
        package
    };

    let mut args = vec![manager];
    args.extend_from_slice(install_args);
    args.push(package);
    Ok(CommandSpec::new("pkexec", &args))
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
    Err(Error::Unsupported("no terminal emulator found".into()))
}
