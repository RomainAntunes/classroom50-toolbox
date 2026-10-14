//! Self-update from GitHub Releases (`latest.json`, signed with the Tauri updater key).

use serde::Serialize;
use tauri::{AppHandle, Emitter};
use tauri_plugin_updater::UpdaterExt;

use crate::error::Result;

pub const UPDATE_AVAILABLE_EVENT: &str = "update-available";

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateInfo {
    pub version: String,
    pub notes: Option<String>,
}

pub async fn check(app: &AppHandle) -> Result<Option<UpdateInfo>> {
    let update = app.updater()?.check().await?;
    Ok(update.map(|u| UpdateInfo {
        version: u.version,
        notes: u.body,
    }))
}

/// Background check at startup; the UI decides when to install.
pub fn check_in_background(app: AppHandle) {
    tauri::async_runtime::spawn(async move {
        match check(&app).await {
            Ok(Some(info)) => {
                let _ = app.emit(UPDATE_AVAILABLE_EVENT, info);
            }
            Ok(None) => {}
            Err(err) => log::warn!("update check failed: {err}"),
        }
    });
}

pub async fn install_and_restart(app: &AppHandle) -> Result<()> {
    if let Some(update) = app.updater()?.check().await? {
        update.download_and_install(|_, _| {}, || {}).await?;
        app.restart();
    }
    Ok(())
}
