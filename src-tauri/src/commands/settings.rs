//! Reading and saving the settings, with what a change applies at once.

use tauri::{AppHandle, State};
use tauri_plugin_autostart::ManagerExt;

use crate::error::Result;
use crate::platform;
use crate::popover;
use crate::projects;
use crate::settings::{Settings, SettingsStore};
use crate::updater;

#[tauri::command]
#[cfg_attr(test, specta::specta)]
pub fn get_settings(app: AppHandle, settings: State<'_, SettingsStore>) -> Settings {
    reported_settings(&app, settings.get())
}

/// `settings` as the UI gets them: the OS setting wins, since the user may have removed
/// the login item outside the app.
pub fn reported_settings(app: &AppHandle, mut settings: Settings) -> Settings {
    if let Some(enabled) = platform::login_item_enabled() {
        settings.launch_at_login = enabled;
    } else if let Ok(enabled) = app.autolaunch().is_enabled() {
        settings.launch_at_login = enabled;
    }
    settings
}

#[tauri::command]
#[cfg_attr(test, specta::specta)]
pub fn update_settings(
    app: AppHandle,
    settings: State<'_, SettingsStore>,
    next: Settings,
) -> Result<Settings> {
    let previous = settings.get();
    // First, so a shortcut the OS refuses saves nothing.
    if next.global_shortcut != previous.global_shortcut {
        crate::shortcut::apply(&app, next.global_shortcut.as_deref())?;
    }
    let shortcut_changed = next.global_shortcut != previous.global_shortcut;
    // Any later failure saves nothing, so the OS gets the saved shortcut back.
    let restore_shortcut = || {
        if shortcut_changed {
            let _ = crate::shortcut::apply(&app, previous.global_shortcut.as_deref());
        }
    };
    if next.launch_at_login != previous.launch_at_login
        && let Err(err) = apply_launch_at_login(&app, next.launch_at_login)
    {
        restore_shortcut();
        return Err(err);
    }
    let saved = match settings.set(next) {
        Ok(saved) => saved,
        Err(err) => {
            restore_shortcut();
            return Err(err);
        }
    };
    // What the calendar holds depends on these.
    if saved.hidden != previous.hidden
        || saved.hidden_classes != previous.hidden_classes
        || saved.notify_hours_before != previous.notify_hours_before
    {
        crate::calendar::sync(&app);
    }
    if saved.appearance != previous.appearance {
        popover::apply_appearance(&app, saved.appearance);
    }
    if saved.update_channel != previous.update_channel {
        updater::channel_changed(&app);
    }
    if saved.watched_folders != previous.watched_folders
        || saved.scan_editor_projects != previous.scan_editor_projects
    {
        let app = app.clone();
        tauri::async_runtime::spawn(async move {
            projects::refresh_local(&app, false).await;
        });
    }
    Ok(saved)
}

/// Moves a login item from the plugin's Launch Agent to the OS's own API where there is
/// one, so an update keeps opening at login, now under the app's name and icon.
pub fn migrate_launch_at_login(app: &AppHandle) {
    if cfg!(debug_assertions) || platform::login_item_enabled().is_none() {
        return;
    }
    if app.autolaunch().is_enabled().unwrap_or(false)
        && let Err(err) = apply_launch_at_login(app, true)
    {
        log::warn!("moving the login item failed: {err}");
    }
}

/// Registers or removes the OS login item. Skipped in debug builds so `tauri dev` never
/// registers a throwaway binary.
pub fn apply_launch_at_login(app: &AppHandle, enabled: bool) -> Result<()> {
    if cfg!(debug_assertions) {
        return Ok(());
    }
    let autolaunch = app.autolaunch();
    if let Some(result) = platform::set_login_item(enabled) {
        result?;
        // The plugin's Launch Agent, which earlier versions wrote: it would open the
        // app a second time and stays listed as an unidentified binary.
        autolaunch.disable()?;
        return Ok(());
    }
    if enabled {
        autolaunch.enable()?;
    } else {
        autolaunch.disable()?;
    }
    Ok(())
}
