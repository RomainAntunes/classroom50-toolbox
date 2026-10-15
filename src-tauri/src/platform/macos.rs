use std::path::{Path, PathBuf};

use super::{CommandSpec, Os, home_dir};
use crate::error::{Error, Result};
use crate::process;
use crate::tools::Tool;

pub const OS: Os = Os::Macos;

pub fn extra_path_dirs() -> Vec<PathBuf> {
    let mut dirs = vec![
        PathBuf::from("/opt/homebrew/bin"),
        PathBuf::from("/usr/local/bin"),
    ];
    if let Some(home) = home_dir() {
        dirs.push(home.join(".local/bin"));
    }
    dirs
}

pub fn install_command(tool: Tool) -> Result<CommandSpec> {
    if process::find_in_path("brew").is_none() {
        return Err(Error::Unsupported(
            "Homebrew is required to install tools on macOS: https://brew.sh".into(),
        ));
    }
    match tool {
        Tool::Git => Ok(CommandSpec::new("brew", &["install", "git"])),
        Tool::Gh => Ok(CommandSpec::new("brew", &["install", "gh"])),
        Tool::GhStudent => Err(Error::Unsupported(
            "gh-student is installed through gh".into(),
        )),
    }
}

/// Where editors keep their settings (JetBrains, VS Code and its forks all use it).
pub fn editor_config_dirs() -> Vec<PathBuf> {
    home_dir()
        .map(|home| home.join("Library/Application Support"))
        .into_iter()
        .collect()
}

/// App bundle names per editor. Eclipse, NetBeans and Visual Studio don't open a plain
/// folder handed to them, so they aren't offered.
fn editor_apps(id: &str) -> &'static [&'static str] {
    match id {
        "idea" => &["IntelliJ IDEA.app", "IntelliJ IDEA Ultimate.app"],
        "idea-ce" => &[
            "IntelliJ IDEA CE.app",
            "IntelliJ IDEA Community Edition.app",
        ],
        "clion" => &["CLion.app"],
        "pycharm" => &["PyCharm.app", "PyCharm Professional Edition.app"],
        "pycharm-ce" => &["PyCharm CE.app", "PyCharm Community Edition.app"],
        "rustrover" => &["RustRover.app"],
        "webstorm" => &["WebStorm.app"],
        "goland" => &["GoLand.app"],
        "phpstorm" => &["PhpStorm.app"],
        "rider" => &["Rider.app"],
        "rubymine" => &["RubyMine.app"],
        "datagrip" => &["DataGrip.app"],
        "dataspell" => &["DataSpell.app"],
        "android-studio" => &["Android Studio.app"],
        "vscode" => &["Visual Studio Code.app"],
        "vscode-insiders" => &["Visual Studio Code - Insiders.app"],
        "vscodium" => &["VSCodium.app"],
        "code-oss" => &["Code - OSS.app"],
        "cursor" => &["Cursor.app"],
        "windsurf" => &["Windsurf.app"],
        "kiro" => &["Kiro.app"],
        "trae" => &["Trae.app"],
        "positron" => &["Positron.app"],
        "void" => &["Void.app"],
        "antigravity" => &["Antigravity.app"],
        "zed" => &["Zed.app", "Zed Preview.app"],
        "sublime" => &["Sublime Text.app"],
        _ => &[],
    }
}

/// `open -a <app>`, with the app found in `/Applications` or `~/Applications` (where the
/// JetBrains Toolbox installs).
pub fn editor_launcher(id: &str) -> Option<CommandSpec> {
    let mut dirs = vec![PathBuf::from("/Applications")];
    if let Some(home) = home_dir() {
        dirs.push(home.join("Applications"));
        dirs.push(home.join("Applications/JetBrains Toolbox"));
    }
    editor_apps(id).iter().find_map(|app| {
        let path = dirs
            .iter()
            .map(|dir| dir.join(app))
            .find(|path| path.is_dir())?;
        Some(CommandSpec::new("open", &["-a", &path.to_string_lossy()]))
    })
}

pub fn editor_data_dirs() -> Vec<PathBuf> {
    editor_config_dirs()
}

pub fn open_terminal(dir: &Path) -> Result<()> {
    process::spawn_detached(
        "open",
        ["-a".as_ref(), "Terminal".as_ref(), dir.as_os_str()],
        None,
    )
}
