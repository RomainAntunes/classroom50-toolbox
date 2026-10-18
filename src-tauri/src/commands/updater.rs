//! Self-update and quitting.

use tauri::{AppHandle, State};

use crate::error::Result;
use crate::updater::{self, UpdateStatus, UpdaterState};

/// Where self-update stands; `update-state` follows every change.
#[tauri::command]
#[cfg_attr(test, specta::specta)]
pub fn update_status(state: State<'_, UpdaterState>) -> UpdateStatus {
    state.status()
}

/// Settings > About: checks now and starts the download when there is a newer version.
/// Unlike background checks, a failure rejects.
#[tauri::command]
#[cfg_attr(test, specta::specta)]
pub async fn check_update(app: AppHandle) -> Result<UpdateStatus> {
    updater::check(&app, false).await
}

/// "Restart now": installs the downloaded update and relaunches.
#[tauri::command]
#[cfg_attr(test, specta::specta)]
pub async fn install_update(app: AppHandle) -> Result<()> {
    updater::install_now(&app).await
}

/// "Later": keeps the banner without the question.
#[tauri::command]
#[cfg_attr(test, specta::specta)]
pub fn defer_update(app: AppHandle) {
    updater::defer(&app);
}

/// Quits the app, installing a downloaded update first where the platform allows it.
#[tauri::command]
#[cfg_attr(test, specta::specta)]
pub fn quit_app(app: AppHandle) {
    updater::quit(&app);
}
