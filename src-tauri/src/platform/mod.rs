//! Everything that differs per OS. Each submodule exposes the same functions; the rest of
//! the app calls `platform::*` and never branches on the OS itself.

use std::path::{Path, PathBuf};

use serde::Serialize;

use crate::error::Result;
use crate::tools::Tool;

#[cfg(target_os = "linux")]
mod linux;
#[cfg(target_os = "macos")]
mod macos;
#[cfg(target_os = "windows")]
mod windows;

#[cfg(target_os = "linux")]
use linux as imp;
#[cfg(target_os = "macos")]
use macos as imp;
#[cfg(target_os = "windows")]
use windows as imp;

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Os {
    Macos,
    Windows,
    Linux,
}

/// A command line to run, possibly with elevation (shown to the user before it runs).
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct CommandSpec {
    pub program: String,
    pub args: Vec<String>,
}

impl CommandSpec {
    pub fn new(program: &str, args: &[&str]) -> Self {
        Self {
            program: program.to_owned(),
            args: args.iter().map(|a| (*a).to_owned()).collect(),
        }
    }
}

pub fn os() -> Os {
    imp::OS
}

/// Directories appended to PATH for child processes (see `process::search_path`).
pub fn extra_path_dirs() -> Vec<PathBuf> {
    imp::extra_path_dirs()
}

/// How to install `tool` with this OS's package manager. `GhStudent` is the same everywhere
/// and is handled by the caller.
pub fn install_command(tool: Tool) -> Result<CommandSpec> {
    imp::install_command(tool)
}

pub fn open_terminal(dir: &Path) -> Result<()> {
    imp::open_terminal(dir)
}

/// Config dirs that may hold editor state (recent projects), most likely first.
pub fn editor_config_dirs() -> Vec<PathBuf> {
    imp::editor_config_dirs()
}

pub fn home_dir() -> Option<PathBuf> {
    std::env::var_os(if cfg!(windows) { "USERPROFILE" } else { "HOME" }).map(PathBuf::from)
}
