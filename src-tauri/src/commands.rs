//! IPC surface for the Svelte UI. Keep these thin: the logic lives in the domain modules.
//! Every command here has a typed wrapper in `src/lib/api.ts`.

use std::path::PathBuf;

use serde::Serialize;
use tauri::{AppHandle, Manager, State};
use tauri_plugin_autostart::ManagerExt;

use crate::clones::{self, CloneStore};
use crate::error::{Error, Result};
use crate::login::{self, LoginState};
use crate::platform::{self, CommandSpec, Os};
use crate::popover::{self, PopoverState};
use crate::projects::{self, ProjectsState, Snapshot, SyncProgress};
use crate::pull;
use crate::settings::{Settings, SettingsStore};
use crate::simulate;
use crate::student;
use crate::tools::{self, AuthStatus, Tool, ToolJob, ToolJobs, ToolStatus};
use crate::updater::{self, UpdateInfo};

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AppInfo {
    os: Os,
    version: String,
    /// The OS has a calendar store the app can write into (macOS).
    native_calendar: bool,
}

#[tauri::command]
pub fn app_info(app: AppHandle) -> AppInfo {
    AppInfo {
        os: platform::os(),
        version: app.package_info().version.to_string(),
        native_calendar: platform::has_native_calendar(),
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

/// Installs `tool` (`tool-jobs` events report its output) and returns every tool's status:
/// gh-student depends on gh, so installing gh changes what the other rows can do.
#[tauri::command]
pub async fn install_tool(app: AppHandle, tool: Tool) -> Result<Vec<ToolStatus>> {
    tools::install(&app, tool).await?;
    Ok(tools::statuses().await)
}

/// The installs and updates still running, for a view opened after they started.
#[tauri::command]
pub fn tool_jobs(state: State<'_, ToolJobs>) -> Vec<ToolJob> {
    state.list()
}

/// Stops the install or update of `tool`; its command then fails with `cancelled`.
#[tauri::command]
pub fn cancel_tool_job(state: State<'_, ToolJobs>, tool: Tool) {
    state.cancel(tool);
}

/// The newer version `upgrade_tool` would install, if any.
#[tauri::command]
pub async fn tool_update(tool: Tool) -> Result<Option<String>> {
    tools::available_update(tool).await
}

/// Updates gh-student and returns its new status.
#[tauri::command]
pub async fn upgrade_tool(app: AppHandle, tool: Tool) -> Result<ToolStatus> {
    tools::upgrade(&app, tool).await?;
    Ok(tools::status(tool).await)
}

#[tauri::command]
pub async fn auth_status() -> AuthStatus {
    tools::auth_status().await
}

/// The last synced project list, without waiting for the network. Empty before the first
/// sync finishes; `projects-updated` follows.
#[tauri::command]
pub fn list_projects(state: State<'_, ProjectsState>) -> Snapshot {
    state.snapshot()
}

/// Syncs GitHub, the local clones and the assignment manifests now.
#[tauri::command]
pub async fn refresh_projects(app: AppHandle) -> Snapshot {
    projects::refresh(&app).await
}

/// Rescans the disk only (watched folders and editor projects), with no GitHub request.
#[tauri::command]
pub async fn rescan_local(app: AppHandle) -> Snapshot {
    projects::refresh_local(&app, true).await
}

/// What the sync engine did lately, for Settings.
#[derive(serde::Serialize)]
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

/// Writes the due dates as an `.ics` file at `path` (chosen in a save dialog), keeps it
/// updated after each sync from now on, and returns how many events it holds.
#[tauri::command]
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

/// The words of calendar event notes, from the UI's i18n.
#[tauri::command]
pub fn set_calendar_labels(
    state: State<'_, crate::calendar::CalendarState>,
    labels: crate::calendar::Labels,
) {
    state.set_labels(labels);
}

/// The OS calendars the user can write to; asks for access the first time.
#[tauri::command]
pub async fn native_calendars() -> Result<Vec<platform::NativeCalendar>> {
    tauri::async_runtime::spawn_blocking(|| platform::native_calendars(true))
        .await
        .map_err(|err| Error::Unsupported(err.to_string()))?
}

/// Picks the OS calendar to keep the due dates in (writing them now), or with `None` stops
/// and removes the events the app wrote.
#[tauri::command]
pub async fn set_native_calendar(
    app: AppHandle,
    settings: State<'_, SettingsStore>,
    id: Option<String>,
) -> Result<Settings> {
    let current = settings.get();
    // Writing to a new calendar also removes what sits in the old one.
    if id.is_some() || current.native_calendar.is_some() {
        crate::calendar::write_native(&app, id.clone()).await?;
    }
    settings.set(Settings {
        native_calendar: id,
        ..current
    })
}

#[tauri::command]
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
pub async fn clear_cache(app: AppHandle) -> Snapshot {
    app.state::<ProjectsState>().clear_cache();
    projects::refresh(&app).await
}

/// Ids of projects and unaccepted assignments with news not seen yet; `unseen-updated`
/// follows.
#[tauri::command]
pub fn unseen(state: State<'_, crate::alerts::AlertsState>) -> Vec<String> {
    state.unseen()
}

/// Whether a sync is running and how far it is; `sync-progress` follows.
#[tauri::command]
pub fn sync_progress(state: State<'_, ProjectsState>) -> SyncProgress {
    state.progress()
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
    let previous = settings.get();
    if next.launch_at_login != previous.launch_at_login {
        apply_launch_at_login(&app, next.launch_at_login)?;
    }
    let saved = settings.set(next)?;
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

/// The github.com accounts gh is signed in to, the active one first.
#[tauri::command]
pub async fn list_accounts() -> Result<Vec<crate::accounts::Account>> {
    crate::accounts::list().await
}

/// Switches gh to `login`, then syncs that account's projects.
#[tauri::command]
pub async fn switch_account(app: AppHandle, login: String) -> Result<AuthStatus> {
    crate::accounts::switch(&login).await?;
    let status = tools::auth_status().await;
    tauri::async_runtime::spawn(async move {
        projects::refresh(&app).await;
    });
    Ok(status)
}

/// The account's avatar as a `data:` URL (cached for a week).
#[tauri::command]
pub async fn account_avatar(app: AppHandle, login: String) -> Result<String> {
    let http = app.state::<crate::github::Http>().0.clone();
    let cache_dir = app.path().app_cache_dir()?;
    crate::accounts::avatar(&http, &cache_dir, &login).await
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
pub fn open_terminal(path: PathBuf) -> Result<()> {
    platform::open_terminal(&path)
}

#[derive(Debug, Serialize)]
pub struct EditorInfo {
    id: &'static str,
    name: &'static str,
}

/// The editors installed here that can open a folder, in a stable order.
#[tauri::command]
pub async fn installed_editors() -> Vec<EditorInfo> {
    tauri::async_runtime::spawn_blocking(|| {
        crate::editors::installed()
            .into_iter()
            .map(|editor| EditorInfo {
                id: editor.id,
                name: editor.name,
            })
            .collect()
    })
    .await
    .unwrap_or_default()
}

#[tauri::command]
pub async fn open_in_editor(path: PathBuf, editor: String) -> Result<()> {
    tauri::async_runtime::spawn_blocking(move || crate::editors::open(&editor, &path))
        .await
        .map_err(|err| Error::Unsupported(err.to_string()))?
}

/// Clones a project that has no local copy into `parent/<repo>`, registers the clone and
/// rescans the disk. Returns the new folder; `projects-updated` follows.
#[tauri::command]
pub async fn clone_project(
    app: AppHandle,
    state: State<'_, ProjectsState>,
    id: String,
    parent: PathBuf,
) -> Result<PathBuf> {
    let project = state
        .snapshot()
        .projects
        .into_iter()
        .find(|p| p.id == id)
        .ok_or_else(|| Error::Unsupported(format!("unknown project {id}")))?;
    let (Some(owner), Some(repo)) = (project.owner, project.repo) else {
        return Err(Error::Unsupported(format!("{id} has no GitHub repository")));
    };
    if let Some(path) = project.path {
        return Err(Error::PathExists(path));
    }
    let path = clones::clone(&owner, &repo, &parent).await?;
    app.state::<CloneStore>().add(path.clone())?;
    projects::refresh_local(&app, false).await;
    Ok(path)
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Accepted {
    /// `owner/name` as GitHub spells it.
    repo: String,
    /// The new project's id, once the sync that follows the accept has found it.
    project_id: Option<String>,
}

/// Accepts an unaccepted assignment with `gh student accept`, then syncs so the new repo
/// shows up. `new_team` founds a group for a team assignment formed by students; without
/// it such an assignment fails with `needsTeam`.
#[tauri::command]
pub async fn accept_assignment(
    app: AppHandle,
    state: State<'_, ProjectsState>,
    id: String,
    new_team: bool,
) -> Result<Accepted> {
    let item = state
        .snapshot()
        .unaccepted
        .into_iter()
        .find(|u| u.id == id)
        .ok_or_else(|| Error::Unsupported(format!("unknown assignment {id}")))?;
    let accepted = student::accept(&student::AcceptRequest {
        org: &item.classroom.org,
        classroom: &item.classroom.slug,
        assignment: &item.assignment.slug,
        key: item.classroom.secret.as_deref(),
        new_team,
    })
    .await;
    let repo = match accepted {
        Ok(repo) => repo,
        // A failure or a timeout may come after the repo was made: look again.
        Err(err @ Error::Student(_)) => {
            let app = app.clone();
            tauri::async_runtime::spawn(async move {
                projects::refresh_after_write(&app).await;
            });
            return Err(err);
        }
        Err(err) => return Err(err),
    };
    let snapshot = projects::refresh_after_write(&app).await;
    let key = repo.to_ascii_lowercase();
    let project_id = snapshot
        .projects
        .iter()
        .find(|p| p.id == key)
        .map(|p| p.id.clone());
    if project_id.is_none() {
        // GitHub may list a new repo a little later; the next full sync could be an hour off.
        let app = app.clone();
        tauri::async_runtime::spawn(async move {
            tokio::time::sleep(std::time::Duration::from_secs(20)).await;
            projects::refresh_after_write(&app).await;
        });
    }
    Ok(Accepted { repo, project_id })
}

/// Gives a classmate push access to project `id`: joins them to the group on a team
/// assignment, invites them as a collaborator otherwise. A full sync follows, so the
/// group's members are read again.
#[tauri::command]
pub async fn add_teammate(
    app: AppHandle,
    state: State<'_, ProjectsState>,
    id: String,
    username: String,
) -> Result<()> {
    let project = state
        .snapshot()
        .projects
        .into_iter()
        .find(|p| p.id == id)
        .ok_or_else(|| Error::Unsupported(format!("unknown project {id}")))?;
    let (Some(owner), Some(repo)) = (project.owner.as_deref(), project.repo.as_deref()) else {
        return Err(Error::Unsupported(format!("{id} isn't on GitHub")));
    };
    let team = project.group.is_some()
        || project
            .assignment
            .as_ref()
            .is_some_and(|a| a.mode == crate::classroom50::AssignmentMode::Team);
    let full_name = format!("{owner}/{repo}");
    let addition = if team {
        student::Addition::Team {
            org: owner,
            classroom: &project.config.classroom,
            assignment: &project.config.assignment,
            key: project
                .classroom
                .as_ref()
                .and_then(|c| c.secret.as_deref())
                .or(project.config.secret.as_deref()),
        }
    } else {
        student::Addition::Invite {
            repo: &full_name,
            clone: project.path.as_deref(),
        }
    };
    student::add(&addition, &username).await?;
    projects::refresh_after_write(&app).await;
    Ok(())
}

/// The clone of project `id`, which must exist and still take submissions.
fn submittable_clone(state: &ProjectsState, id: &str) -> Result<PathBuf> {
    let project = state
        .snapshot()
        .projects
        .into_iter()
        .find(|p| p.id == id)
        .ok_or_else(|| Error::Unsupported(format!("unknown project {id}")))?;
    let closed = project.assignment.as_ref().is_some_and(|a| a.closed)
        || project.remote.as_ref().is_some_and(|r| r.archived);
    if closed {
        return Err(Error::Student(
            "submissions are closed for this assignment".into(),
        ));
    }
    project
        .path
        .ok_or_else(|| Error::Unsupported(format!("{id} has no local clone")))
}

/// What submitting project `id` would send, for the confirmation dialog.
#[tauri::command]
pub async fn submit_check(
    state: State<'_, ProjectsState>,
    id: String,
) -> Result<student::SubmitCheck> {
    let path = submittable_clone(&state, &id)?;
    student::check(&path).await
}

/// Submits project `id` with `gh student submit`, then re-reads its status while the
/// autograder runs (the clone's refs don't move, so the watcher wouldn't notice).
#[tauri::command]
pub async fn submit_project(
    app: AppHandle,
    state: State<'_, ProjectsState>,
    id: String,
) -> Result<student::Submitted> {
    let path = submittable_clone(&state, &id)?;
    let submitted = student::submit(&path).await?;
    tauri::async_runtime::spawn(async move {
        projects::refresh_after_push(&app).await;
    });
    Ok(submitted)
}

/// Fast-forwards project `id`'s clone to GitHub's default branch (`pull.rs` says when it
/// refuses). The watcher sees the remote refs move and re-reads the status.
#[tauri::command]
pub async fn pull_project(
    app: AppHandle,
    state: State<'_, ProjectsState>,
    id: String,
) -> Result<()> {
    let project = state
        .snapshot()
        .projects
        .into_iter()
        .find(|p| p.id == id)
        .ok_or_else(|| Error::Unsupported(format!("unknown project {id}")))?;
    let path = project
        .path
        .ok_or_else(|| Error::Unsupported(format!("{id} has no local clone")))?;
    let branch = project
        .remote
        .and_then(|r| r.default_branch)
        .ok_or_else(|| Error::Unsupported(format!("{id} has no default branch on GitHub")))?;
    pull::pull(&path, &branch).await?;
    projects::refresh_local(&app, false).await;
    Ok(())
}

#[tauri::command]
pub async fn check_update(app: AppHandle) -> Result<Option<UpdateInfo>> {
    updater::check(&app).await
}

#[tauri::command]
pub async fn install_update(app: AppHandle) -> Result<()> {
    updater::install_and_restart(&app).await
}

/// Dev builds only: lights the tray badge after a few seconds, once the popover has
/// closed (closing it clears the badge). The next open and close clears it again.
#[tauri::command]
pub async fn preview_badge(app: AppHandle) -> Result<()> {
    if !cfg!(debug_assertions) {
        return Err(Error::Unsupported(
            "the badge preview is for dev builds".into(),
        ));
    }
    tokio::time::sleep(std::time::Duration::from_secs(3)).await;
    crate::tray::set_badge(&app, true);
    Ok(())
}

/// Dev builds only: pretends GitHub is down or the device is offline, until set back.
#[tauri::command]
pub fn simulate_network(network: simulate::Network) -> Result<()> {
    simulate::set_network(network)
}

#[tauri::command]
pub fn simulated_network() -> simulate::Network {
    simulate::network()
}
