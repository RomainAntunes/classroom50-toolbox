use std::path::{Path, PathBuf};

use super::{CommandSpec, InstallPlan, Os, home_dir, install};
use crate::error::Result;
use crate::process;
use crate::tools::Tool;

pub const OS: Os = Os::Macos;

pub fn os_version() -> Option<String> {
    let plist = std::fs::read_to_string("/System/Library/CoreServices/SystemVersion.plist").ok()?;
    super::os_version::macos(&plist)
}

pub const HOUR_CYCLE_IN_PROCESS: bool = true;

pub async fn hour_cycle() -> Option<super::HourCycle> {
    icu_hour_cycle()
}

/// The pattern the user's locale gives the `j` template (its preferred hour), which honors
/// System Settings' "24-hour time" switch as well as the region's default.
fn icu_hour_cycle() -> Option<super::HourCycle> {
    use objc2_foundation::{NSDateFormatter, NSLocale, NSString};
    let locale = NSLocale::autoupdatingCurrentLocale();
    let pattern = NSDateFormatter::dateFormatFromTemplate_options_locale(
        &NSString::from_str("j"),
        0,
        Some(&locale),
    )?;
    super::hour_cycle::from_icu_pattern(&pattern.to_string())
}

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

/// Lines `brew install` prints at each step (no TTY, so no progress bar), in order, then
/// the Command Line Tools installer's (`xcode-select --install` prints one line and returns).
pub const INSTALL_STEPS: &[(&str, f32)] = &[
    ("==> Fetching", 0.1),
    ("==> Downloading", 0.15),
    ("Bottle ", 0.65),
    ("==> Installing", 0.7),
    ("==> Reinstalling", 0.7),
    ("==> Upgrading", 0.7),
    ("==> Pouring", 0.8),
    ("\u{1f37a}", 0.95),
    ("==> Caveats", 0.95),
    ("==> Summary", 0.95),
];

pub fn install_plan(tool: Tool) -> Result<InstallPlan> {
    let has = |program: &str| process::find_in_path(program).is_some();
    install::macos(
        tool,
        &install::Probe {
            has: &has,
            immutable: false,
        },
    )
}

/// The xcrun stand-ins in `/usr/bin` (git among them) open Apple's "install the command line
/// developer tools" dialog when neither the Command Line Tools nor Xcode are installed, on
/// every run. A status check must not do that.
pub fn is_stub(program: &str, path: &Path) -> bool {
    program == "git" && path == Path::new("/usr/bin/git") && !developer_tools_installed()
}

/// Whether the developer directory `xcode-select` points at (or its defaults) has git: what
/// `/usr/bin/git` runs. Read from the file system, no process: it runs before every git call.
fn developer_tools_installed() -> bool {
    let chosen = std::env::var_os("DEVELOPER_DIR")
        .map(PathBuf::from)
        .or_else(|| std::fs::read_link("/var/db/xcode_select_link").ok());
    let defaults = [
        PathBuf::from("/Applications/Xcode.app/Contents/Developer"),
        PathBuf::from("/Library/Developer/CommandLineTools"),
    ];
    match chosen {
        Some(dir) => dir.join("usr/bin/git").is_file(),
        None => defaults.iter().any(|dir| dir.join("usr/bin/git").is_file()),
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::editors::ALL;

    #[test]
    fn only_apples_git_can_be_a_stub() {
        assert!(!is_stub("gh", Path::new("/usr/bin/git")));
        assert!(!is_stub("git", Path::new("/opt/homebrew/bin/git")));
        // With the tools installed (this machine builds the app), the real git is found.
        if developer_tools_installed() {
            assert!(!is_stub("git", Path::new("/usr/bin/git")));
            assert!(crate::process::find_in_path("git").is_some());
        }
    }

    #[test]
    fn every_folder_opening_editor_has_an_app() {
        for editor in ALL {
            let apps = editor_apps(editor.id);
            let expected = !matches!(editor.id, "eclipse" | "netbeans" | "visual-studio");
            assert_eq!(!apps.is_empty(), expected, "{}", editor.id);
            assert!(
                apps.iter().all(|app| app.ends_with(".app")),
                "{}",
                editor.id
            );
        }
    }
}

mod calendar;
mod login_item;

pub fn global_shortcuts_supported() -> bool {
    true
}

pub fn has_native_calendar() -> bool {
    true
}

pub fn calendar_privacy_url() -> Option<&'static str> {
    Some("x-apple.systempreferences:com.apple.preference.security?Privacy_Calendars")
}

pub use calendar::{native_calendars, write_calendar};
pub use login_item::{login_item_enabled, set_login_item};
