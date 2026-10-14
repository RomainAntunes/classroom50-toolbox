//! IPC surface for the Svelte UI. Keep these thin: the logic lives in the domain modules.
//! Every command here has a typed wrapper in `src/lib/api.ts`.

use std::path::PathBuf;

use serde::Serialize;
use tauri::{AppHandle, State};

use crate::classroom50::{self, Assignment};
use crate::discovery::{self, LocalProject};
use crate::error::Result;
use crate::platform::{self, CommandSpec, Os};
use crate::tools::{self, AuthStatus, Tool, ToolStatus};
use crate::updater::{self, UpdateInfo};

pub struct Http(pub reqwest::Client);

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AppInfo {
    os: Os,
    version: String,
}

#[tauri::command]
pub fn app_info(app: AppHandle) -> AppInfo {
    AppInfo {
        os: platform::os(),
        version: app.package_info().version.to_string(),
    }
}

#[tauri::command]
pub async fn tool_statuses() -> Vec<ToolStatus> {
    tools::statuses().await
}

/// What `install_tool` will run, so the UI can show it before asking for elevation.
#[tauri::command]
pub fn install_command(tool: Tool) -> Result<CommandSpec> {
    tools::install_command(tool)
}

#[tauri::command]
pub async fn install_tool(tool: Tool) -> Result<ToolStatus> {
    tools::install(tool).await?;
    Ok(tools::status(tool).await)
}

#[tauri::command]
pub async fn auth_status() -> AuthStatus {
    tools::auth_status().await
}

#[tauri::command]
pub async fn scan_folders(paths: Vec<PathBuf>) -> Result<Vec<LocalProject>> {
    let projects = tauri::async_runtime::spawn_blocking(move || {
        paths
            .iter()
            .flat_map(|root| discovery::scan(root, discovery::DEFAULT_MAX_DEPTH))
            .collect()
    })
    .await?;
    Ok(projects)
}

#[tauri::command]
pub async fn fetch_assignments(
    http: State<'_, Http>,
    org: String,
    classroom: String,
    secret: Option<String>,
) -> Result<Vec<Assignment>> {
    classroom50::fetch_assignments(&http.0, &org, &classroom, secret.as_deref()).await
}

#[tauri::command]
pub fn open_terminal(path: PathBuf) -> Result<()> {
    platform::open_terminal(&path)
}

#[tauri::command]
pub async fn check_update(app: AppHandle) -> Result<Option<UpdateInfo>> {
    updater::check(&app).await
}

#[tauri::command]
pub async fn install_update(app: AppHandle) -> Result<()> {
    updater::install_and_restart(&app).await
}
