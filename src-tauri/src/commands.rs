//! IPC surface for the Svelte UI. Keep these thin: the logic lives in the domain modules.
//! Every command here has a typed wrapper in `src/lib/api.ts`.

use std::path::PathBuf;

use serde::Serialize;
use tauri::{AppHandle, State};
use tauri_plugin_autostart::ManagerExt;

use crate::classroom50::{self, Assignment};
use crate::discovery::{self, LocalProject};
use crate::error::Result;
use crate::login::{self, LoginState};
use crate::platform::{self, CommandSpec, Os};
use crate::popover::PopoverState;
use crate::settings::{Settings, SettingsStore};
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

/// Keeps the popover open while the UI shows a native dialog.
#[tauri::command]
pub fn set_popover_pinned(state: State<'_, PopoverState>, pinned: bool) {
    state.set_pinned(pinned);
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

/// Scans every watched folder from the settings.
#[tauri::command]
pub async fn scan_projects(settings: State<'_, SettingsStore>) -> Result<Vec<LocalProject>> {
    let folders = settings.get().watched_folders;
    let projects = tauri::async_runtime::spawn_blocking(move || {
        folders
            .iter()
            .flat_map(|root| discovery::scan(root, discovery::DEFAULT_MAX_DEPTH))
            .collect()
    })
    .await?;
    Ok(projects)
}

#[tauri::command]
pub fn get_settings(app: AppHandle, settings: State<'_, SettingsStore>) -> Settings {
    let mut current = settings.get();
    // The OS setting wins: the user may have removed the login item outside the app.
    if let Ok(enabled) = app.autolaunch().is_enabled() {
        current.launch_at_login = enabled;
    }
    current
}

#[tauri::command]
pub fn update_settings(
    app: AppHandle,
    settings: State<'_, SettingsStore>,
    next: Settings,
) -> Result<Settings> {
    if next.launch_at_login != settings.get().launch_at_login {
        apply_launch_at_login(&app, next.launch_at_login)?;
    }
    settings.set(next)
}

/// Registers or removes the OS login item. Skipped in debug builds so `tauri dev` never
/// registers a throwaway binary.
pub fn apply_launch_at_login(app: &AppHandle, enabled: bool) -> Result<()> {
    if cfg!(debug_assertions) {
        return Ok(());
    }
    let autolaunch = app.autolaunch();
    if enabled {
        autolaunch.enable()?;
    } else {
        autolaunch.disable()?;
    }
    Ok(())
}

/// Runs gh's device flow; the code arrives through the `login-device-code` event.
#[tauri::command]
pub async fn login(app: AppHandle, state: State<'_, LoginState>) -> Result<AuthStatus> {
    login::login(&app, &state).await
}

#[tauri::command]
pub fn cancel_login(state: State<'_, LoginState>) {
    state.cancel();
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
