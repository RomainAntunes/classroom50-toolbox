//! Everything that differs per OS. Each submodule exposes the same functions; the rest of
//! the app calls `platform::*` and never branches on the OS itself.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::error::Result;
use crate::tools::Tool;

mod install;
#[cfg(target_os = "linux")]
mod linux;
// Pure (paths, file text, D-Bus command lines), so it is tested on every OS.
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
mod linux_calendar;
#[cfg(target_os = "macos")]
mod macos;
mod os_version;
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

/// The OS release as people name it ("macOS 26.5", "Windows 11 24H2 (build 26100)",
/// "Ubuntu 24.04.1 LTS"), read from a file or the registry, never by running a program.
pub fn os_version() -> Option<String> {
    imp::os_version()
}

/// Directories appended to PATH for child processes (see `process::search_path`).
pub fn extra_path_dirs() -> Vec<PathBuf> {
    imp::extra_path_dirs()
}

pub use install::{InstallPlan, interpret_failure};

/// How to install `tool` with this OS's package manager (see `install`). `GhStudent` is the
/// same everywhere and is handled by the caller.
pub fn install_plan(tool: Tool) -> Result<InstallPlan> {
    imp::install_plan(tool)
}

/// Whether `path`, found on the search path for `program`, is a stand-in that would ask to
/// install the real program instead of running it (macOS's `/usr/bin/git` without the
/// Command Line Tools opens Apple's install dialog). Such a program counts as missing.
pub fn is_stub(program: &str, path: &Path) -> bool {
    imp::is_stub(program, path)
}

/// Whether `program` has such a stand-in on this OS, so running it by name needs a lookup
/// first (cheap file checks, but git runs often).
pub fn may_be_stubbed(program: &str) -> bool {
    cfg!(target_os = "macos") && program == "git"
}

/// How far along an install is (0 to 1) when `line` is one of the package manager's step
/// markers, else `None`.
pub fn install_progress(line: &str) -> Option<f32> {
    imp::INSTALL_STEPS
        .iter()
        .find(|(marker, _)| line.contains(marker))
        .map(|&(_, progress)| progress)
}

pub fn open_terminal(dir: &Path) -> Result<()> {
    imp::open_terminal(dir)
}

/// Config dirs that may hold editor state (recent projects), most likely first.
pub fn editor_config_dirs() -> Vec<PathBuf> {
    imp::editor_config_dirs()
}

/// How to open a folder in the editor `id` (see `editors::ALL`), the folder being the last
/// argument; `None` when it isn't installed or can't open folders.
pub fn editor_launcher(id: &str) -> Option<CommandSpec> {
    imp::editor_launcher(id)
}

/// Command-line launchers of each editor, shared by Windows and Linux: the editor's own CLI
/// and the JetBrains Toolbox script names (the Toolbox's `scripts` folder or PATH).
#[cfg_attr(target_os = "macos", allow(dead_code))]
fn editor_commands(id: &str) -> &'static [&'static str] {
    match id {
        "idea" => &["idea", "intellij-idea-ultimate"],
        "idea-ce" => &["idea-ce", "idea", "intellij-idea-community"],
        "clion" => &["clion"],
        "pycharm" => &["pycharm", "pycharm-professional"],
        "pycharm-ce" => &["pycharm-ce", "pycharm", "pycharm-community"],
        "rustrover" => &["rustrover"],
        "webstorm" => &["webstorm"],
        "goland" => &["goland"],
        "phpstorm" => &["phpstorm"],
        "rider" => &["rider"],
        "rubymine" => &["rubymine"],
        "datagrip" => &["datagrip"],
        "dataspell" => &["dataspell"],
        "android-studio" => &["studio", "android-studio"],
        "vscode" => &["code"],
        "vscode-insiders" => &["code-insiders"],
        "vscodium" => &["codium"],
        "code-oss" => &["code-oss"],
        "cursor" => &["cursor"],
        "windsurf" => &["windsurf"],
        "kiro" => &["kiro"],
        "trae" => &["trae"],
        "positron" => &["positron"],
        "void" => &["void"],
        "antigravity" => &["antigravity"],
        "zed" => &["zed", "zeditor", "zedit"],
        "sublime" => &["subl"],
        "netbeans" => &["netbeans"],
        _ => &[],
    }
}

/// Data dirs that may hold editor state (Zed's database, NetBeans, Visual Studio). Same as
/// the config dirs on macOS.
pub fn editor_data_dirs() -> Vec<PathBuf> {
    imp::editor_data_dirs()
}

pub fn home_dir() -> Option<PathBuf> {
    std::env::var_os(if cfg!(windows) { "USERPROFILE" } else { "HOME" }).map(PathBuf::from)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::editors::ALL;

    /// Eclipse and Visual Studio have no command that opens a plain folder.
    #[test]
    fn every_editor_has_a_command_line_name() {
        for editor in ALL {
            let names = editor_commands(editor.id);
            let expected = !matches!(editor.id, "eclipse" | "visual-studio");
            assert_eq!(!names.is_empty(), expected, "{}", editor.id);
        }
    }
}

/// A deadline written into the user's own calendar app.
#[derive(Debug, Clone)]
pub struct CalendarEvent {
    /// The list id; the key in `WrittenEvents`.
    pub uid: String,
    pub title: String,
    pub notes: Option<String>,
    pub url: Option<String>,
    /// The event is a block ending at the deadline, so it has room to be clicked.
    pub start: DateTime<Utc>,
    pub due: DateTime<Utc>,
    /// Hours before the deadline (the event's end).
    pub alarm_hours: Vec<u32>,
    /// Changes when any field above does, so unchanged events are left alone.
    pub fingerprint: u64,
}

/// A calendar of the OS's calendar app the user can write to.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NativeCalendar {
    pub id: String,
    pub title: String,
    /// The account holding it (iCloud, Google, On My Mac...).
    pub account: String,
    /// The store the UI names itself instead of `account` (Linux: `gnome`, `kde`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub kind: Option<&'static str>,
}

/// An event the app wrote, by `CalendarEvent::uid`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Written {
    pub event_id: String,
    pub calendar_id: String,
    pub fingerprint: u64,
}

pub type WrittenEvents = BTreeMap<String, Written>;

/// Whether this OS has a calendar store the app can write to (else: the `.ics` file only).
/// On Linux, whether Evolution Data Server or Akonadi is installed.
pub fn has_native_calendar() -> bool {
    imp::has_native_calendar()
}

/// The writable calendars. With `ask`, asks for access first when it was never asked;
/// without, fails rather than prompting (background syncs).
pub fn native_calendars(ask: bool) -> Result<Vec<NativeCalendar>> {
    imp::native_calendars(ask)
}

/// Makes `calendar_id` hold exactly `events`: adds and updates what changed, removes events
/// the app wrote earlier that are gone (or sit in another calendar). `written` is updated
/// as it goes, so a failure halfway loses nothing. `dir` (the app data dir) holds what the
/// store reads, if anything: Linux's calendars read an `.ics` file there.
pub fn write_calendar(
    calendar_id: &str,
    events: &[CalendarEvent],
    written: &mut WrittenEvents,
    dir: &Path,
) -> Result<()> {
    imp::write_calendar(calendar_id, events, written, dir)
}

/// Removes every event the app wrote (the user turned the sync off).
pub fn clear_calendar(written: &mut WrittenEvents, dir: &Path) -> Result<()> {
    imp::write_calendar("", &[], written, dir)
}
