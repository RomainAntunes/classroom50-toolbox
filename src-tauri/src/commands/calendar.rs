//! Due dates in a calendar: the `.ics` export and the OS calendar stores.

use std::path::PathBuf;

use tauri::{AppHandle, Manager, State};

use crate::error::{Error, Result};
use crate::platform;
use crate::projects::ProjectsState;
use crate::settings::{Settings, SettingsStore};

/// Writes the due dates as an `.ics` file at `path` (chosen in a save dialog), keeps it
/// updated after each sync from now on, and returns how many events it holds.
#[tauri::command]
#[cfg_attr(test, specta::specta)]
pub fn export_calendar(
    app: AppHandle,
    path: PathBuf,
    state: State<'_, ProjectsState>,
    settings: State<'_, SettingsStore>,
) -> Result<usize> {
    let current = settings.get();
    let labels = app.state::<crate::calendar::CalendarState>().labels();
    let count = crate::calendar::export(&path, &state.snapshot(), &current, &labels)?;
    settings.set(Settings {
        calendar_file: Some(path),
        ..current
    })?;
    Ok(count)
}

/// Opens the exported `.ics` file with the default app (Calendar, Outlook...), which
/// imports it; importing again updates the events (stable UIDs). Only that file: the path
/// comes from the settings, not the UI.
#[tauri::command]
#[cfg_attr(test, specta::specta)]
pub fn open_calendar_file(app: AppHandle, settings: State<'_, SettingsStore>) -> Result<()> {
    use tauri_plugin_opener::OpenerExt;
    let path = settings
        .get()
        .calendar_file
        .ok_or_else(|| Error::Unsupported("no calendar file exported yet".into()))?;
    app.opener()
        .open_path(path.to_string_lossy(), None::<&str>)
        .map_err(|err| Error::Unsupported(err.to_string()))
}

/// The words of calendar event notes, from the UI's i18n.
#[tauri::command]
#[cfg_attr(test, specta::specta)]
pub fn set_calendar_labels(
    app: AppHandle,
    state: State<'_, crate::calendar::CalendarState>,
    projects: State<'_, ProjectsState>,
    labels: crate::calendar::Labels,
) {
    // Another language: rewrite the notes now rather than after the next sync. Not before
    // the first sync, when the list is still empty.
    if state.set_labels(labels) && projects.snapshot().synced_at.is_some() {
        crate::calendar::sync(&app);
    }
}

/// The OS calendars the user can write to; asks for access the first time.
#[tauri::command]
#[cfg_attr(test, specta::specta)]
pub async fn native_calendars() -> Result<Vec<platform::NativeCalendar>> {
    tauri::async_runtime::spawn_blocking(|| platform::native_calendars(true))
        .await
        .map_err(|err| Error::Unsupported(err.to_string()))?
}

/// Picks the OS calendar to keep the due dates in (writing them now), or with `None` stops
/// and removes the events the app wrote.
#[tauri::command]
#[cfg_attr(test, specta::specta)]
pub async fn set_native_calendar(
    app: AppHandle,
    settings: State<'_, SettingsStore>,
    id: Option<String>,
) -> Result<Settings> {
    let current = settings.get();
    // Writing to a new calendar also removes what sits in the old one (best effort on
    // Linux, where a store may have gone). Turning the sync off always works: what
    // couldn't be removed is logged, and the setting is saved anyway.
    if id.is_some() || current.native_calendar.is_some() {
        let written = crate::calendar::write_native(&app, id.clone()).await;
        match written {
            Err(err) if id.is_none() => log::warn!("could not clear the calendar: {err}"),
            other => other?,
        }
    }
    settings.set(Settings {
        native_calendar: id,
        ..current
    })
}
