//! IPC surface for the Svelte UI. Keep these thin: the logic lives in the domain modules.
//! Every command here has a typed wrapper in `src/lib/api.ts`.

use std::path::PathBuf;

use serde::Serialize;
use tauri::{AppHandle, Manager, State};
use tauri_plugin_autostart::ManagerExt;

use crate::clones::{self, CloneStore};
use crate::deeplink::{self, DeepLinkState};
use crate::error::{Error, Result};
use crate::login::{self, LoginState};
use crate::platform::{self, Os};
use crate::popover::{self, PopoverState};
use crate::projects::{self, Project, ProjectsState, Snapshot, SyncProgress};
use crate::pull;
use crate::push;
use crate::settings::{CommitActions, Settings, SettingsStore};
use crate::simulate;
use crate::student;
use crate::tools::{self, AuthStatus, Tool, ToolJob, ToolJobs, ToolStatus};
use crate::updater::{self, UpdateInfo};

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AppInfo {
    os: Os,
    version: String,
    /// The OS has a calendar store the app can write into (macOS; Linux with Evolution Data
    /// Server or Akonadi).
    native_calendar: bool,
}

/// Async, so detecting a calendar store (Linux reads the XDG data dirs and PATH) runs off
/// the main thread.
#[tauri::command]
pub async fn app_info(app: AppHandle) -> AppInfo {
    let native_calendar = tauri::async_runtime::spawn_blocking(platform::has_native_calendar)
        .await
        .unwrap_or(false);
    AppInfo {
        os: platform::os(),
        version: app.package_info().version.to_string(),
        native_calendar,
    }
}

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
pub fn install_command(tool: Tool) -> Result<platform::InstallPlan> {
    tools::install_plan(tool)
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

/// Opens the exported `.ics` file with the default app (Calendar, Outlook...), which
/// imports it; importing again updates the events (stable UIDs). Only that file: the path
/// comes from the settings, not the UI.
#[tauri::command]
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

/// The OS's preferred languages, most wanted first, as BCP 47 tags (`fr-CH`). The UI's
/// `navigator.languages` can't stand in: WKWebView reports the bundle's localizations
/// there, not the user's.
#[tauri::command]
pub fn system_languages() -> Vec<String> {
    sys_locale::get_locales().collect()
}

/// The words of the tray menu, from the UI's i18n.
#[tauri::command]
pub fn set_tray_labels(app: AppHandle, labels: crate::tray::Labels) {
    crate::tray::set_labels(&app, &labels);
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

/// Runs gh's device flow; the code arrives through the `login-device-code` event. The new
/// account is gh's active one afterwards, so its projects are synced right away.
#[tauri::command]
pub async fn login(app: AppHandle, state: State<'_, LoginState>) -> Result<AuthStatus> {
    signed_in(
        app.clone(),
        login::login(&app, &state, login::Flow::SignIn).await,
    )
}

/// Adds gh-student's missing scopes to the active account (`gh auth refresh`), through the
/// same device flow as `login`.
#[tauri::command]
pub async fn grant_scopes(app: AppHandle, state: State<'_, LoginState>) -> Result<AuthStatus> {
    signed_in(
        app.clone(),
        login::login(&app, &state, login::Flow::GrantScopes).await,
    )
}

fn signed_in(app: AppHandle, result: Result<AuthStatus>) -> Result<AuthStatus> {
    if result.as_ref().is_ok_and(|status| status.logged_in) {
        // A view that asks for a sync too shares this one.
        tauri::async_runtime::spawn(async move {
            projects::refresh(&app).await;
        });
    }
    result
}

/// Signs `login` out of gh, then syncs whichever account is left (or clears the list).
#[tauri::command]
pub async fn sign_out(app: AppHandle, login: String) -> Result<AuthStatus> {
    crate::accounts::sign_out(&login).await?;
    let status = tools::auth_status().await;
    tauri::async_runtime::spawn(async move {
        projects::refresh(&app).await;
    });
    Ok(status)
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
    let snapshot = state.snapshot();
    let listed = snapshot.unaccepted.iter().find(|u| u.id == id).cloned();
    let Some(item) = listed.or_else(|| {
        // An assignment a link pointed to, outside the list (an upcoming one, a class not
        // synced yet), unless a sync has found its repo since.
        accepted_project(&snapshot, &id)
            .is_none()
            .then(|| app.state::<DeepLinkState>().linked(&id))
            .flatten()
    }) else {
        // A sync may have found the repo (accepted elsewhere) since the row was drawn.
        return accepted_project(&snapshot, &id)
            .ok_or_else(|| Error::Unsupported(format!("unknown assignment {id}")));
    };
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

/// The project of the assignment behind unaccepted id `org/classroom/assignment`.
fn accepted_project(snapshot: &Snapshot, id: &str) -> Option<Accepted> {
    let mut parts = id.splitn(3, '/');
    let (org, classroom, assignment) = (parts.next()?, parts.next()?, parts.next()?);
    let project = projects::assignment_project(snapshot, org, classroom, assignment)?;
    Some(Accepted {
        repo: format!("{}/{}", project.owner.as_deref()?, project.repo.as_deref()?),
        project_id: Some(project.id.clone()),
    })
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
        return Err(Error::SubmissionsClosed);
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

/// Where Commit & push writes for project `id`: its clone, the branch, and GitHub's head of
/// it. Only every-push assignments that still take submissions; tag-mode ones submit.
fn pushable(state: &ProjectsState, id: &str) -> Result<(PathBuf, String, Option<String>)> {
    let path = submittable_clone(state, id)?;
    let project = state
        .snapshot()
        .projects
        .into_iter()
        .find(|p| p.id == id)
        .ok_or_else(|| Error::Unsupported(format!("unknown project {id}")))?;
    let (branch, head) = push_target(&project)?;
    Ok((path, branch, head))
}

/// The branch Commit & push writes for `project`, and GitHub's head of it. Always GitHub's
/// default branch, so a clone on another branch is refused by `push::check`; the clone's own
/// branch only when GitHub's repo is known to be empty (the first push makes it the
/// default). Without GitHub's data (another account, a sync that hasn't listed the repo
/// yet) nothing says which branch is graded, so push isn't offered.
fn push_target(project: &Project) -> Result<(String, Option<String>)> {
    let id = &project.id;
    let every_push = project
        .assignment
        .as_ref()
        .is_some_and(|a| a.submission_mode != Some(crate::classroom50::SubmissionMode::Tag));
    if !every_push {
        return Err(Error::Unsupported(format!(
            "{id} isn't graded on every push; submit it instead"
        )));
    }
    let remote = project.remote.as_ref().ok_or_else(|| {
        Error::Unsupported(format!(
            "{id} isn't among your GitHub repos yet; refresh first"
        ))
    })?;
    let head = remote.head.as_ref().map(|h| h.oid.clone());
    let empty = remote.default_branch.is_none() && head.is_none();
    let branch = match &remote.default_branch {
        Some(branch) => Some(branch.clone()),
        None if empty => project.local.as_ref().and_then(|l| l.branch.clone()),
        None => None,
    }
    .ok_or_else(|| Error::Unsupported(format!("{id} has no default branch on GitHub")))?;
    Ok((branch, head))
}

/// Why push isn't offered next to Commit (`CommitCheck::push_blocked`), from `pushable`'s
/// refusal: `closed`, `notOnGitHub` (no GitHub data for the project), or the error's kind.
fn push_blocked(err: &Error, on_github: bool) -> String {
    match err {
        Error::SubmissionsClosed => "closed",
        _ if !on_github => "notOnGitHub",
        err => err.kind(),
    }
    .into()
}

/// Refuses what the `commitActions` setting leaves out of the app: any commit when it is
/// `none`, a push unless it is `push`.
fn allow_commit(actions: CommitActions, push: bool) -> Result<()> {
    let allowed = match actions {
        CommitActions::None => false,
        CommitActions::Commit => !push,
        CommitActions::Push => true,
    };
    if allowed {
        Ok(())
    } else {
        Err(Error::Unsupported(
            if push {
                "push is turned off in Settings"
            } else {
                "commit is turned off in Settings"
            }
            .into(),
        ))
    }
}

/// What Commit & push would send for project `id`, or why it can't (`push.rs`).
#[tauri::command]
pub async fn push_check(
    state: State<'_, ProjectsState>,
    settings: State<'_, SettingsStore>,
    id: String,
) -> Result<push::PushCheck> {
    allow_commit(settings.get().commit_actions, true)?;
    let (path, branch, head) = pushable(&state, &id)?;
    push::check(&path, &branch, head.as_deref()).await
}

/// Commits project `id`'s changes with `message` and pushes its default branch, then
/// re-reads the status while the autograder runs.
#[tauri::command]
pub async fn commit_push(
    app: AppHandle,
    state: State<'_, ProjectsState>,
    settings: State<'_, SettingsStore>,
    id: String,
    message: Option<String>,
) -> Result<push::Pushed> {
    allow_commit(settings.get().commit_actions, true)?;
    let (path, branch, head) = pushable(&state, &id)?;
    let pushed = push::commit_and_push(&path, &branch, head.as_deref(), message.as_deref()).await?;
    projects::refresh_local(&app, false).await;
    tauri::async_runtime::spawn(async move {
        projects::refresh_after_push(&app).await;
    });
    Ok(pushed)
}

/// Project `id`'s clone, closed or not: a local commit is always allowed.
fn local_clone(state: &ProjectsState, id: &str) -> Result<PathBuf> {
    state
        .snapshot()
        .projects
        .into_iter()
        .find(|p| p.id == id)
        .ok_or_else(|| Error::Unsupported(format!("unknown project {id}")))?
        .path
        .ok_or_else(|| Error::Unsupported(format!("{id} has no local clone")))
}

/// What a commit of project `id` would hold, and whether Commit & push is offered with it.
#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CommitCheck {
    #[serde(flatten)]
    pub check: push::PushCheck,
    /// Why push isn't offered (an error kind, `tagMode`, or `disabled` by the `commitActions`
    /// setting); none when it is.
    pub push_blocked: Option<String>,
}

/// What Commit (and Commit & push, when it applies) would do for project `id`. Refuses
/// only what blocks a local commit (`push::commit_check`).
#[tauri::command]
pub async fn commit_check(
    state: State<'_, ProjectsState>,
    settings: State<'_, SettingsStore>,
    id: String,
) -> Result<CommitCheck> {
    allow_commit(settings.get().commit_actions, false)?;
    let path = local_clone(&state, &id)?;
    let changes = push::commit_check(&path).await?;
    if allow_commit(settings.get().commit_actions, true).is_err() {
        return Ok(CommitCheck {
            check: changes,
            push_blocked: Some("disabled".into()),
        });
    }
    let snapshot = state.snapshot();
    let project = snapshot.projects.iter().find(|p| p.id == id);
    let tag_mode = project.is_some_and(|p| {
        p.assignment
            .as_ref()
            .is_some_and(|a| a.submission_mode == Some(crate::classroom50::SubmissionMode::Tag))
    });
    let on_github = project.is_some_and(|p| p.remote.is_some());
    if tag_mode {
        return Ok(CommitCheck {
            check: changes,
            push_blocked: Some("tagMode".into()),
        });
    }
    let pushed = match pushable(&state, &id) {
        Ok((path, branch, head)) => push::check(&path, &branch, head.as_deref()).await,
        Err(err) => Err(err),
    };
    Ok(match pushed {
        Ok(check) => CommitCheck {
            check,
            push_blocked: None,
        },
        Err(err) => CommitCheck {
            check: changes,
            push_blocked: Some(push_blocked(&err, on_github)),
        },
    })
}

/// Commits project `id`'s changes with `message` in its clone and nothing else: no push, so
/// dev builds allow it, and only the local state is re-read.
#[tauri::command]
pub async fn commit_only(
    app: AppHandle,
    state: State<'_, ProjectsState>,
    settings: State<'_, SettingsStore>,
    id: String,
    message: String,
) -> Result<push::Pushed> {
    allow_commit(settings.get().commit_actions, false)?;
    let path = local_clone(&state, &id)?;
    let committed = push::commit(&path, &message).await?;
    projects::refresh_local(&app, false).await;
    Ok(committed)
}

/// The name and private email of the signed-in account, offered when git has no identity.
#[tauri::command]
pub async fn suggested_git_identity() -> Result<push::Identity> {
    push::suggested_identity().await
}

/// Sets project `id`'s clone's own `user.name` and `user.email` (not the global config).
#[tauri::command]
pub async fn set_git_identity(
    state: State<'_, ProjectsState>,
    id: String,
    name: String,
    email: String,
) -> Result<()> {
    let path = local_clone(&state, &id)?;
    push::set_identity(&path, &name, &email).await
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

/// The link the OS opened the app with, if one waits, and where the list says it leads.
#[tauri::command]
pub fn take_deep_link(app: AppHandle) -> Option<deeplink::Target> {
    deeplink::take(&app)
}

/// The UI is done with the link it took; the next one may come.
#[tauri::command]
pub fn deep_link_done(state: State<'_, DeepLinkState>) {
    state.finish();
}

/// Reads the manifest behind the `unlisted` link the student just agreed to open.
#[tauri::command]
pub async fn fetch_linked_assignment(app: AppHandle) -> Result<Option<deeplink::Target>> {
    deeplink::fetch_unlisted(&app).await
}

/// Dev builds only: handles `url` as if the OS had opened it.
#[tauri::command]
pub fn simulate_deep_link(app: AppHandle, url: String) -> Result<()> {
    deeplink::simulate(&app, &url)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::classroom50::RepoConfig;
    use crate::projects::{LocalState, RemoteRepo};

    /// Whatever this machine prefers comes out as BCP 47 tags, ready for `resolveLocale`.
    #[test]
    fn system_languages_are_language_tags() {
        let languages = system_languages();
        println!("system languages: {languages:?}");
        for tag in languages {
            assert!(!tag.is_empty() && !tag.contains('_'), "{tag:?}");
        }
    }

    fn project(owner: Option<&str>, repo: &str, classroom: &str, assignment: &str) -> Project {
        Project {
            id: match owner {
                Some(owner) => format!("{owner}/{repo}").to_ascii_lowercase(),
                None => format!("/clones/{repo}"),
            },
            owner: owner.map(str::to_owned),
            repo: owner.map(|_| repo.to_owned()),
            config: RepoConfig {
                classroom: classroom.into(),
                assignment: assignment.into(),
                secret: None,
            },
            path: None,
            local: None,
            remote: None,
            classroom: None,
            assignment: None,
            pending: None,
            web_url: None,
            late_seconds: None,
            editor: None,
            group: None,
        }
    }

    fn snapshot(projects: Vec<Project>) -> Snapshot {
        Snapshot {
            projects,
            ..Snapshot::default()
        }
    }

    #[test]
    fn answers_an_accept_with_the_project_a_sync_found() {
        let snapshot = snapshot(vec![
            project(Some("HEIGVD"), "poo-labo-1-alice", "poo", "labo-1"),
            project(Some("HEIGVD"), "POO-Labo-2-alice", "POO", "Labo-2"),
        ]);

        let accepted = accepted_project(&snapshot, "heigvd/poo/labo-2").unwrap();
        assert_eq!(accepted.repo, "HEIGVD/POO-Labo-2-alice");
        assert_eq!(
            accepted.project_id.as_deref(),
            Some("heigvd/poo-labo-2-alice")
        );
    }

    #[test]
    fn finds_no_project_in_another_org_or_classroom() {
        let snapshot = snapshot(vec![
            project(Some("other"), "poo-labo-1-alice", "poo", "labo-1"),
            project(Some("heigvd"), "bdr-labo-1-alice", "bdr", "labo-1"),
        ]);

        assert!(accepted_project(&snapshot, "heigvd/poo/labo-1").is_none());
    }

    #[test]
    fn ignores_clones_without_a_github_origin() {
        let snapshot = snapshot(vec![project(None, "labo-1", "poo", "labo-1")]);

        assert!(accepted_project(&snapshot, "heigvd/poo/labo-1").is_none());
    }

    #[test]
    fn rejects_ids_that_are_not_org_classroom_assignment() {
        let snapshot = snapshot(vec![project(
            Some("heigvd"),
            "poo-labo-1-a",
            "poo",
            "labo-1",
        )]);

        assert!(accepted_project(&snapshot, "heigvd/poo").is_none());
        assert!(accepted_project(&snapshot, "").is_none());
    }

    /// An every-push project on branch `local_branch`, with GitHub's data when `remote` is
    /// `Some((default branch, head))`.
    fn every_push(local_branch: &str, remote: Option<(Option<&str>, Option<&str>)>) -> Project {
        let mut p = project(Some("org"), "lab-1-alice", "c", "lab-1");
        p.assignment = Some(
            serde_json::from_value(serde_json::json!({
                "slug": "lab-1", "name": "Lab 1", "mode": "individual"
            }))
            .unwrap(),
        );
        p.local = Some(LocalState {
            branch: Some(local_branch.into()),
            ..LocalState::default()
        });
        p.remote = remote.map(|(branch, head)| {
            let mut repo: RemoteRepo = serde_json::from_value(serde_json::json!({
                "owner": "org", "name": "lab-1-alice", "url": "https://github.com/org/lab-1-alice",
                "archived": false, "canPush": true
            }))
            .unwrap();
            repo.default_branch = branch.map(str::to_owned);
            repo.head =
                head.map(|oid| serde_json::from_value(serde_json::json!({ "oid": oid })).unwrap());
            repo
        });
        p
    }

    #[test]
    fn pushes_only_githubs_default_branch() {
        // GitHub's default branch, whatever the clone has checked out (push::check refuses).
        let (branch, head) =
            push_target(&every_push("experiment", Some((Some("main"), Some("abc"))))).unwrap();
        assert_eq!((branch.as_str(), head.as_deref()), ("main", Some("abc")));
        // An empty repo on GitHub: the first push makes the clone's branch the default.
        let (branch, head) = push_target(&every_push("trunk", Some((None, None)))).unwrap();
        assert_eq!((branch.as_str(), head), ("trunk", None));
        // No GitHub data (another account, not synced yet): no push, whatever the branch.
        let err = push_target(&every_push("experiment", None)).unwrap_err();
        assert_eq!(push_blocked(&err, false), "notOnGitHub");
        // A head without a default branch isn't an empty repo: no guessing.
        assert!(push_target(&every_push("experiment", Some((None, Some("abc"))))).is_err());
        // Tag mode submits instead.
        let mut tag = every_push("main", Some((Some("main"), None)));
        tag.assignment.as_mut().unwrap().submission_mode =
            Some(crate::classroom50::SubmissionMode::Tag);
        assert!(push_target(&tag).is_err());
    }

    #[test]
    fn names_why_push_is_blocked() {
        assert_eq!(push_blocked(&Error::SubmissionsClosed, true), "closed");
        assert_eq!(push_blocked(&Error::SubmissionsClosed, false), "closed");
        assert_eq!(
            push_blocked(&Error::OtherBranch("main".into()), true),
            "otherBranch"
        );
        assert_eq!(push_blocked(&Error::RemoteAhead, true), "remoteAhead");
        assert_eq!(
            push_blocked(&Error::Unsupported("x".into()), false),
            "notOnGitHub"
        );
        assert_eq!(
            push_blocked(&Error::Unsupported("x".into()), true),
            "unsupported"
        );
    }

    #[test]
    fn commit_actions_gate_commit_and_push() {
        assert!(allow_commit(CommitActions::None, false).is_err());
        assert!(allow_commit(CommitActions::None, true).is_err());
        assert!(allow_commit(CommitActions::Commit, false).is_ok());
        assert!(allow_commit(CommitActions::Commit, true).is_err());
        assert!(allow_commit(CommitActions::Push, false).is_ok());
        assert!(allow_commit(CommitActions::Push, true).is_ok());
    }
}
