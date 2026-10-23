//! The project list and the sync engine.

use chrono::{TimeDelta, Utc};
use tauri::{AppHandle, Manager, State};

use crate::cooldown;
use crate::projects::{self, ProjectsState, Snapshot, SyncProgress};

/// A refresh this soon after a full sync that worked gets that sync's list: clicking again
/// and again doesn't spend GitHub's budget.
const REFRESH_COOLDOWN: TimeDelta = TimeDelta::seconds(5);
/// Clearing the cache again this soon after a clear only shows the list it just rebuilt.
const CLEAR_COOLDOWN: TimeDelta = TimeDelta::seconds(10);

/// The last synced project list, without waiting for the network. Empty before the first
/// sync finishes; `projects-updated` follows.
#[tauri::command]
#[cfg_attr(test, specta::specta)]
pub fn list_projects(state: State<'_, ProjectsState>) -> Snapshot {
    state.snapshot()
}

/// Syncs GitHub, the local clones and the assignment manifests now.
#[tauri::command]
#[cfg_attr(test, specta::specta)]
pub async fn refresh_projects(app: AppHandle) -> Snapshot {
    let snapshot = app.state::<ProjectsState>().snapshot();
    if snapshot.issue.is_none()
        && cooldown::within(snapshot.synced_at, Utc::now(), REFRESH_COOLDOWN)
    {
        return snapshot;
    }
    projects::refresh(&app).await
}

/// Rescans the disk only (watched folders and editor projects), with no GitHub request.
#[tauri::command]
#[cfg_attr(test, specta::specta)]
pub async fn rescan_local(app: AppHandle) -> Snapshot {
    projects::refresh_local(&app, true).await
}

/// What the sync engine did lately, for Settings.
#[derive(serde::Serialize)]
#[cfg_attr(test, derive(specta::Type))]
#[serde(rename_all = "camelCase")]
pub struct SyncDetails {
    login: Option<String>,
    synced_at: Option<chrono::DateTime<chrono::Utc>>,
    checked_at: Option<chrono::DateTime<chrono::Utc>>,
    stats: projects::SyncStats,
    rate_limit: Option<crate::github::RateLimit>,
    issue: Option<projects::SyncIssue>,
    assignments: usize,
    clones: usize,
}

#[tauri::command]
#[cfg_attr(test, specta::specta)]
pub fn sync_details(state: State<'_, ProjectsState>) -> SyncDetails {
    let snapshot = state.snapshot();
    SyncDetails {
        login: snapshot.login,
        synced_at: snapshot.synced_at,
        checked_at: snapshot.checked_at,
        stats: state.stats(),
        rate_limit: crate::github::rate_limit(),
        issue: snapshot.issue,
        assignments: snapshot.projects.len(),
        clones: snapshot
            .projects
            .iter()
            .filter(|p| p.path.is_some())
            .count(),
    }
}

/// Drops the GitHub caches and syncs from scratch.
#[tauri::command]
#[cfg_attr(test, specta::specta)]
pub async fn clear_cache(app: AppHandle) -> Snapshot {
    let state = app.state::<ProjectsState>();
    if cooldown::within(state.cleared_at(), Utc::now(), CLEAR_COOLDOWN) {
        return state.snapshot();
    }
    state.clear_cache();
    let snapshot = projects::refresh(&app).await;
    state.set_cleared_at(Utc::now());
    snapshot
}

/// Ids of projects and unaccepted assignments with news not seen yet; `unseen-updated`
/// follows.
#[tauri::command]
#[cfg_attr(test, specta::specta)]
pub fn unseen(state: State<'_, crate::alerts::AlertsState>) -> Vec<String> {
    state.unseen()
}

/// Whether a sync is running and how far it is; `sync-progress` follows.
#[tauri::command]
#[cfg_attr(test, specta::specta)]
pub fn sync_progress(state: State<'_, ProjectsState>) -> SyncProgress {
    state.progress()
}
