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

pub fn open_terminal(dir: &Path) -> Result<()> {
    process::spawn_detached(
        "open",
        ["-a".as_ref(), "Terminal".as_ref(), dir.as_os_str()],
        None,
    )
}
