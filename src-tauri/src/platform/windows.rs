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

pub fn open_terminal(dir: &Path) -> Result<()> {
    if process::find_in_path("wt").is_some() {
        return process::spawn_detached("wt", ["-d".as_ref(), dir.as_os_str()], None);
    }
    process::spawn_detached("powershell", ["-NoExit"], Some(dir))
}
