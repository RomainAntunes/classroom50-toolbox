use std::path::{Path, PathBuf};

use super::{CommandSpec, Os};
use crate::error::{Error, Result};
use crate::process;
use crate::tools::Tool;

pub const OS: Os = Os::Windows;

/// winget updates PATH in the registry, which an already-running process never sees, so
/// the default install locations are added explicitly.
pub fn extra_path_dirs() -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    for var in ["ProgramFiles", "ProgramW6432"] {
        if let Some(base) = std::env::var_os(var).map(PathBuf::from) {
            dirs.push(base.join("GitHub CLI"));
            dirs.push(base.join("Git").join("cmd"));
        }
    }
    if let Some(local) = std::env::var_os("LOCALAPPDATA").map(PathBuf::from) {
        dirs.push(local.join("Microsoft").join("WindowsApps"));
    }
    dirs
}

pub fn install_command(tool: Tool) -> Result<CommandSpec> {
    let id = match tool {
        Tool::Git => "Git.Git",
        Tool::Gh => "GitHub.cli",
        Tool::GhStudent => {
            return Err(Error::Unsupported(
                "gh-student is installed through gh".into(),
            ));
        }
    };
    if process::find_in_path("winget").is_none() {
        return Err(Error::Unsupported(
            "winget (App Installer) is required; install it from the Microsoft Store".into(),
        ));
    }
    Ok(CommandSpec::new(
        "winget",
        &[
            "install",
            "--id",
            id,
            "--exact",
            "--source",
            "winget",
            "--accept-package-agreements",
            "--accept-source-agreements",
        ],
    ))
}

/// Where editors keep their settings (JetBrains, VS Code and its forks all use it).
pub fn editor_config_dirs() -> Vec<PathBuf> {
    std::env::var_os("APPDATA")
        .map(PathBuf::from)
        .into_iter()
        .collect()
}

/// Executables per editor, relative to `%LOCALAPPDATA%\Programs` or Program Files.
fn editor_exes(id: &str) -> &'static [&'static str] {
    match id {
        "vscode" => &["Microsoft VS Code\\Code.exe"],
        "vscode-insiders" => &["Microsoft VS Code Insiders\\Code - Insiders.exe"],
        "vscodium" => &["VSCodium\\VSCodium.exe"],
        "cursor" => &["cursor\\Cursor.exe"],
        "windsurf" => &["Windsurf\\Windsurf.exe"],
        "kiro" => &["Kiro\\Kiro.exe"],
        "trae" => &["Trae\\Trae.exe"],
        "positron" => &["Positron\\Positron.exe"],
        "void" => &["Void\\Void.exe"],
        "antigravity" => &["Antigravity\\Antigravity.exe"],
        "zed" => &["Zed\\Zed.exe"],
        "sublime" => &[
            "Sublime Text\\sublime_text.exe",
            "Sublime Text 3\\sublime_text.exe",
        ],
        "android-studio" => &["Android\\Android Studio\\bin\\studio64.exe"],
        _ => &[],
    }
}

/// JetBrains installs outside the Toolbox: `Program Files\JetBrains\<folder> <version>\bin`.
fn jetbrains_exe(id: &str) -> Option<(&'static str, &'static str)> {
    Some(match id {
        "idea" => ("IntelliJ IDEA", "idea64.exe"),
        "idea-ce" => ("IntelliJ IDEA Community", "idea64.exe"),
        "clion" => ("CLion", "clion64.exe"),
        "pycharm" => ("PyCharm", "pycharm64.exe"),
        "pycharm-ce" => ("PyCharm Community", "pycharm64.exe"),
        "rustrover" => ("RustRover", "rustrover64.exe"),
        "webstorm" => ("WebStorm", "webstorm64.exe"),
        "goland" => ("GoLand", "goland64.exe"),
        "phpstorm" => ("PhpStorm", "phpstorm64.exe"),
        "rider" => ("JetBrains Rider", "rider64.exe"),
        "rubymine" => ("RubyMine", "rubymine64.exe"),
        "datagrip" => ("DataGrip", "datagrip64.exe"),
        "dataspell" => ("DataSpell", "dataspell64.exe"),
        _ => return None,
    })
}

/// The editor's executable from its default install folder (JetBrains: the Toolbox's
/// `%LOCALAPPDATA%\\Programs\\<product>` or `Program Files\\JetBrains\\<product> <version>`),
/// then the JetBrains Toolbox scripts and PATH.
pub fn editor_launcher(id: &str) -> Option<CommandSpec> {
    let local = std::env::var_os("LOCALAPPDATA").map(PathBuf::from);
    let program_files: Vec<PathBuf> = ["ProgramFiles", "ProgramW6432", "ProgramFiles(x86)"]
        .iter()
        .filter_map(|var| std::env::var_os(var).map(PathBuf::from))
        .collect();
    let found = |path: PathBuf| Some(CommandSpec::new(&path.to_string_lossy(), &[]));
    let programs: Vec<PathBuf> = local.iter().map(|local| local.join("Programs")).collect();

    let bases: Vec<PathBuf> = programs.iter().chain(&program_files).cloned().collect();
    for exe in editor_exes(id) {
        if let Some(path) = bases
            .iter()
            .map(|base| base.join(exe))
            .find(|p| p.is_file())
        {
            return found(path);
        }
    }
    if let Some((folder, exe)) = jetbrains_exe(id) {
        let parents = programs
            .iter()
            .cloned()
            .chain(program_files.iter().map(|base| base.join("JetBrains")));
        for parent in parents {
            if let Some(path) = newest_child(&parent, folder)
                .map(|dir| dir.join("bin").join(exe))
                .filter(|path| path.is_file())
            {
                return found(path);
            }
        }
        let scripts = local
            .iter()
            .map(|local| local.join("JetBrains\\Toolbox\\scripts"));
        for dir in scripts {
            for name in super::editor_commands(id) {
                let script = dir.join(format!("{name}.cmd"));
                if script.is_file() {
                    return found(script);
                }
            }
        }
    }
    if id == "visual-studio" {
        return visual_studio(&program_files).and_then(found);
    }
    super::editor_commands(id)
        .iter()
        .find_map(|name| process::find_in_path(name))
        .and_then(found)
}

/// The child of `parent` named `<prefix>` or `<prefix> <version>`, newest version first.
fn newest_child(parent: &Path, prefix: &str) -> Option<PathBuf> {
    let entries = std::fs::read_dir(parent).ok()?;
    entries
        .flatten()
        .map(|entry| entry.path())
        .filter(|dir| {
            dir.file_name()
                .and_then(|name| name.to_str())
                .and_then(|name| name.strip_prefix(prefix))
                .is_some_and(|rest| rest.is_empty() || rest.starts_with(' '))
        })
        .max()
}

/// `devenv.exe` of the newest Visual Studio:
/// `Microsoft Visual Studio\\<year or version>\\<edition>\\Common7\\IDE\\devenv.exe`.
fn visual_studio(program_files: &[PathBuf]) -> Option<PathBuf> {
    let mut found: Vec<PathBuf> = program_files
        .iter()
        .filter_map(|base| std::fs::read_dir(base.join("Microsoft Visual Studio")).ok())
        .flat_map(|years| years.flatten().map(|entry| entry.path()))
        .filter_map(|year| std::fs::read_dir(year).ok())
        .flat_map(|editions| editions.flatten().map(|entry| entry.path()))
        .map(|edition| edition.join("Common7\\IDE\\devenv.exe"))
        .filter(|path| path.is_file())
        .collect();
    found.sort();
    found.pop()
}

/// `%LOCALAPPDATA%`: Zed and Visual Studio keep their state there.
pub fn editor_data_dirs() -> Vec<PathBuf> {
    std::env::var_os("LOCALAPPDATA")
        .map(PathBuf::from)
        .into_iter()
        .collect()
}

pub fn open_terminal(dir: &Path) -> Result<()> {
    if process::find_in_path("wt").is_some() {
        return process::spawn_detached("wt", ["-d".as_ref(), dir.as_os_str()], None);
    }
    process::spawn_detached("powershell", ["-NoExit"], Some(dir))
}
