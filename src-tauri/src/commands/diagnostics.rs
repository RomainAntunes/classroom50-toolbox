//! Settings > About > Copy diagnostics.

use tauri::{AppHandle, Manager};

use super::installed_editors;
use crate::platform;
use crate::projects::ProjectsState;
use crate::settings::SettingsStore;
use crate::tools;

/// The "Copy diagnostics" report (`diagnostics.rs`), masked, for the UI to preview and
/// copy. Reads only what the app already has; nothing is sent anywhere.
#[tauri::command]
pub async fn diagnostics_report(
    app: AppHandle,
    ui: crate::diagnostics::UiFacts,
) -> crate::diagnostics::Report {
    let (tools, auth, accounts, editors) = tokio::join!(
        tools::statuses(),
        tools::auth_status(),
        crate::accounts::list(),
        installed_editors(),
    );
    let state = app.state::<ProjectsState>();
    let mut stats = state.stats();
    stats.rate_limit = crate::github::rate_limit().or(stats.rate_limit);
    let log = app
        .path()
        .app_log_dir()
        .ok()
        .map(|dir| dir.join(format!("{}.log", app.package_info().name)));
    let log = match log {
        Some(path) => {
            tauri::async_runtime::spawn_blocking(move || crate::diagnostics::read_log(&path))
                .await
                .ok()
                .flatten()
        }
        None => None,
    };
    let facts = crate::diagnostics::Facts {
        now: chrono::Utc::now(),
        version: app.package_info().version.to_string(),
        debug: cfg!(debug_assertions),
        os: platform::os_version().unwrap_or_else(|| std::env::consts::OS.to_owned()),
        arch: std::env::consts::ARCH.to_owned(),
        ui,
        tools,
        auth,
        accounts: accounts.ok(),
        snapshot: state.snapshot(),
        discovery: state.discovery_names(),
        stats,
        settings: app.state::<SettingsStore>().get(),
        editors: editors.into_iter().map(|e| e.name.to_owned()).collect(),
        log,
        home: platform::home_dir().map(|h| h.to_string_lossy().into_owned()),
    };
    crate::diagnostics::report(&facts)
}
