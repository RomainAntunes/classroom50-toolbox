//! The app itself: what it runs on, the popover, the global shortcut, languages, the tray
//! menu's words.

use serde::Serialize;
use tauri::{AppHandle, Manager, State};

use crate::error::Result;
use crate::platform::{self, Os};
use crate::popover::{self, PopoverState};

#[derive(Serialize)]
#[cfg_attr(test, derive(specta::Type))]
#[serde(rename_all = "camelCase")]
pub struct AppInfo {
    os: Os,
    version: String,
    /// The OS has a calendar store the app can write into (macOS; Linux with Evolution Data
    /// Server or Akonadi).
    native_calendar: bool,
    /// A system-wide shortcut can be registered (not in a Wayland session).
    global_shortcuts: bool,
}

/// Async, so detecting a calendar store (Linux reads the XDG data dirs and PATH) runs off
/// the main thread.
#[tauri::command]
#[cfg_attr(test, specta::specta)]
pub async fn app_info(app: AppHandle) -> AppInfo {
    let native_calendar = tauri::async_runtime::spawn_blocking(platform::has_native_calendar)
        .await
        .unwrap_or(false);
    AppInfo {
        os: platform::os(),
        version: app.package_info().version.to_string(),
        native_calendar,
        global_shortcuts: crate::shortcut::available(&app),
    }
}

/// Keeps the popover open while the UI shows a native dialog.
#[tauri::command]
#[cfg_attr(test, specta::specta)]
pub fn set_popover_pinned(state: State<'_, PopoverState>, pinned: bool) {
    state.set_pinned(pinned);
}

/// The system-wide shortcut registered right now (`None` when the saved one couldn't be).
/// Sync, like every caller of `shortcut::apply`: it runs on the main thread.
#[tauri::command]
#[cfg_attr(test, specta::specta)]
pub fn active_global_shortcut(app: AppHandle) -> Option<String> {
    app.state::<crate::shortcut::ShortcutRegistration>()
        .active()
}

/// Lets the shortcut recorder hear the saved shortcut: unregistered while `paused`.
#[tauri::command]
#[cfg_attr(test, specta::specta)]
pub fn pause_global_shortcut(app: AppHandle, paused: bool) -> Result<()> {
    crate::shortcut::pause(&app, paused)
}

/// Hides the popover, as a click outside it would (Escape with nothing else to close).
#[tauri::command]
#[cfg_attr(test, specta::specta)]
pub fn hide_popover(app: AppHandle) {
    popover::hide(&app);
}

/// The OS's preferred languages, most wanted first, as BCP 47 tags (`fr-CH`). The UI's
/// `navigator.languages` can't stand in: WKWebView reports the bundle's localizations
/// there, not the user's.
#[tauri::command]
#[cfg_attr(test, specta::specta)]
pub fn system_languages() -> Vec<String> {
    sys_locale::get_locales().collect()
}

/// The tray menu, computed and worded by the UI; rebuilt only when it changed.
#[tauri::command]
#[cfg_attr(test, specta::specta)]
pub fn set_tray_menu(app: AppHandle, menu: crate::tray::TrayMenu) {
    crate::tray::set_menu(&app, menu);
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Whatever this machine prefers comes out as BCP 47 tags, ready for `resolveLocale`.
    #[test]
    fn system_languages_are_language_tags() {
        let languages = system_languages();
        println!("system languages: {languages:?}");
        for tag in languages {
            assert!(!tag.is_empty() && !tag.contains('_'), "{tag:?}");
        }
    }
}
