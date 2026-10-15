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
