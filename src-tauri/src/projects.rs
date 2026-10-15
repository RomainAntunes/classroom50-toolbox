//! The merged project list. Classrooms come from the user's classroom teams and from
//! `.classroom50.yaml` files; a repo (on GitHub or cloned in a watched folder) belongs to
//! a classroom through that file or, for assignments without an autograder, which commit
//! no file, through the `<classroom>-<assignment>-...` naming rule checked against the
//! classroom's published manifest. Everything is joined by `owner/repo`. A sync runs on a timer, when the popover opens and on demand; the result
//! is pushed to the UI as the `projects-updated` event.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::Duration;

use chrono::{DateTime, NaiveDateTime, Utc};
use futures_util::future::join_all;
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};
use tauri::{AppHandle, Emitter, Manager};
use tokio::task::JoinSet;

use crate::classroom50::{
    self, Assignment, Classroom, RepoConfig, SUBMIT_TAG_PREFIX, SubmissionMode,
};
use crate::clones::CloneStore;
use crate::discovery::{self, LocalProject};
use crate::editors;
use crate::error::{Error, Result};
use crate::github::{self, Http, HttpCache};
use crate::process;
use crate::settings::{Settings, SettingsStore};
use crate::status::{self, Branch, Feedback, Grade, Grading, StatusFields};
use crate::tools;
use crate::watch::{self, WatchTarget};

pub const UPDATED_EVENT: &str = "projects-updated";
pub const PROGRESS_EVENT: &str = "sync-progress";

/// v2 keeps every inspected repo, not only those with a marker file.
const CACHE_FILE: &str = "repos-v2.json";
/// The last snapshot and sync stats, so a restart shows everything at once and doesn't
/// sync again before it's due.
const STATE_FILE: &str = "snapshot.json";
/// A full sync (repo list, teams) is due after this long while work is due this week.
const FULL_SYNC_AGE: chrono::TimeDelta = chrono::TimeDelta::minutes(15);
/// Opening the popover re-reads the status at most this often.
const MIN_STATUS_ON_OPEN: chrono::TimeDelta = chrono::TimeDelta::minutes(1);
/// Repos per GraphQL request when reading `.classroom50.yaml`; larger batches time out.
const DETAIL_BATCH: usize = 25;
/// Batches in flight at once on a cold cache; GitHub discourages more concurrency.
const PARALLEL_BATCHES: usize = 3;
/// Repos per status request. GitHub's response time grows with the batch, so a few small
/// requests in parallel answer faster than one big one.
const STATUS_BATCH: usize = 3;
/// A class's assignment list is re-downloaded at most this often (it's on GitHub Pages,
/// and rarely changes).
const MANIFEST_MAX_AGE: chrono::TimeDelta = chrono::TimeDelta::minutes(10);
/// Status requests in flight at once.
const MAX_STATUS_REQUESTS: usize = 4;
/// 100 repos per page; anything past 2000 accessible repos isn't scanned.
const MAX_LIST_PAGES: usize = 20;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Commit {
    pub oid: String,
    #[serde(default)]
    pub at: Option<DateTime<Utc>>,
    /// GitHub login of the author, else the name in the commit.
    #[serde(default)]
    pub author: Option<String>,
}

/// The newest `submit/*` tag (names sort chronologically).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Submission {
    pub tag: String,
    pub commit: String,
    /// Parsed from the tag name; `None` for hand-made tags that don't follow the format.
    pub at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RemoteRepo {
    pub owner: String,
    pub name: String,
    pub url: String,
    pub archived: bool,
    pub can_push: bool,
    pub default_branch: Option<String>,
    pub head: Option<Commit>,
    pub last_submission: Option<Submission>,
    /// The repo's `.classroom50.yaml`, when it has a valid one.
    pub config: Option<RepoConfig>,
    /// Branches with commits missing from the default branch. Like `grade` and `feedback`,
    /// read by the status query, for assignment repos only.
    #[serde(default)]
    pub unmerged: Vec<Branch>,
    #[serde(default)]
    pub grade: Option<Grade>,
    /// The autograder's run on the head commit, while running or when it broke.
    #[serde(default)]
    pub grading: Option<Grading>,
    #[serde(default)]
    pub feedback: Option<Feedback>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LocalState {
    pub branch: Option<String>,
    pub head: Option<String>,
    /// `None` when the branch has no upstream.
    pub ahead: Option<u32>,
    pub behind: Option<u32>,
    pub changed_files: u32,
    /// Local branches with commits missing from the local default branch.
    pub unmerged: Vec<String>,
    /// GitHub's default-branch head isn't in the clone: someone pushed since the last pull.
    pub remote_ahead: bool,
}

/// The most pressing reason the graded state lags behind the student's work.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Pending {
    /// Someone pushed commits the clone doesn't have while it holds uncommitted or
    /// unpushed work: pull now, before the two diverge further.
    Conflict,
    Uncommitted,
    Unpushed,
    /// A branch, local or pushed, holds commits that aren't on the default branch, which
    /// is what gets graded.
    Unmerged,
    /// Tag-mode assignment whose pushed head has no `submit/*` tag.
    Unsubmitted,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Project {
    /// `owner/repo` in lowercase, or the clone path when it has no GitHub origin.
    pub id: String,
    pub owner: Option<String>,
    pub repo: Option<String>,
    /// From `.classroom50.yaml`, or derived from the repo name.
    pub config: RepoConfig,
    pub path: Option<PathBuf>,
    pub local: Option<LocalState>,
    pub remote: Option<RemoteRepo>,
    pub classroom: Option<Classroom>,
    pub assignment: Option<Assignment>,
    pub pending: Option<Pending>,
    /// The assignment's page in the Classroom 50 web app.
    pub web_url: Option<String>,
    /// How late the graded work is: the submit tag's time (tag mode) or the head commit's
    /// date, as upstream grades it, past the due date. Advisory: nothing is blocked.
    #[serde(default)]
    pub late_seconds: Option<i64>,
}

/// Why the last sync was partial, as `{ kind, message }` like command errors.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SyncIssue {
    pub kind: String,
    pub message: String,
    /// Kept across restarts, so a rate-limit backoff survives one.
    #[serde(default)]
    retry_after: Option<DateTime<Utc>>,
}

impl From<&Error> for SyncIssue {
    fn from(err: &Error) -> Self {
        Self {
            kind: err.kind().to_owned(),
            message: err.to_string(),
            retry_after: match err {
                Error::RateLimited { reset_at } => {
                    Some(reset_at.unwrap_or_else(|| Utc::now() + chrono::TimeDelta::hours(1)))
                }
                _ => None,
            },
        }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Snapshot {
    pub projects: Vec<Project>,
    /// Assignments published in the user's classrooms that they haven't accepted yet.
    pub unaccepted: Vec<Unaccepted>,
    /// Last full sync (repo list and teams).
    pub synced_at: Option<DateTime<Utc>>,
    /// Last time GitHub was read at all, full sync or status check.
    pub checked_at: Option<DateTime<Utc>>,
    pub issue: Option<SyncIssue>,
    /// The GitHub account the data belongs to.
    pub login: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Unaccepted {
    /// `org/classroom/assignment`, lowercase.
    pub id: String,
    pub classroom: Classroom,
    pub assignment: Assignment,
    pub accept_url: String,
}

/// Discovery results persisted between runs, so a sync only re-reads repos pushed since.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
struct RemoteCache {
    login: Option<String>,
    /// `pushedAt` of every listed repo at the time it was last inspected, by lowercase
    /// `owner/repo`. Pushing a commit or tag changes it.
    checked: HashMap<String, Option<DateTime<Utc>>>,
    /// Details of every inspected repo; which are assignments is decided per sync.
    repos: BTreeMap<String, RemoteRepo>,
    /// From the user's classroom teams.
    classrooms: Vec<Classroom>,
}

/// How far the sync in flight is. `total` grows as the work is discovered (repo batches,
/// scan roots, clones), so the fraction can step back a little.
#[derive(Debug, Clone, Copy, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SyncProgress {
    pub running: bool,
    /// Started by the user (refresh button, rescan, sign-in). Background checks run
    /// without a spinner.
    pub foreground: bool,
    pub done: usize,
    pub total: usize,
}

/// How the last sync of a kind went, for the sync details in Settings.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LastRun {
    pub at: DateTime<Utc>,
    pub seconds: f32,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SyncStats {
    /// The GraphQL budget as of the last response.
    pub rate_limit: Option<github::RateLimit>,
    pub full: Option<LastRun>,
    pub status: Option<LastRun>,
    pub local: Option<LastRun>,
    /// Repos the user can see on GitHub (inspected for the marker file).
    pub repos_seen: usize,
    pub scan_roots: usize,
    pub watch_targets: usize,
}

#[derive(Default, Serialize, Deserialize)]
struct Saved {
    snapshot: Snapshot,
    stats: SyncStats,
}

pub struct ProjectsState {
    snapshot: Mutex<Snapshot>,
    state_path: PathBuf,
    /// The first error of the last status read, picked up by the snapshot.
    status_issue: Mutex<Option<SyncIssue>>,
    after_push: std::sync::atomic::AtomicBool,
    stats: Mutex<SyncStats>,
    progress: Mutex<SyncProgress>,
    sync_lock: tokio::sync::Mutex<()>,
    remote: Mutex<RemoteCache>,
    remote_path: PathBuf,
    http_cache: HttpCache,
}

impl ProjectsState {
    pub fn load(cache_dir: &Path) -> Self {
        let remote_path = cache_dir.join(CACHE_FILE);
        let remote = std::fs::read_to_string(&remote_path)
            .ok()
            .and_then(|json| serde_json::from_str(&json).ok())
            .unwrap_or_default();
        let state_path = cache_dir.join(STATE_FILE);
        let saved: Saved = std::fs::read_to_string(&state_path)
            .ok()
            .and_then(|json| serde_json::from_str(&json).ok())
            .unwrap_or_default();
        Self {
            snapshot: Mutex::new(saved.snapshot),
            state_path,
            status_issue: Mutex::default(),
            after_push: std::sync::atomic::AtomicBool::new(false),
            stats: Mutex::new(saved.stats),
            progress: Mutex::default(),
            sync_lock: tokio::sync::Mutex::new(()),
            remote: Mutex::new(remote),
            remote_path,
            http_cache: HttpCache::load(cache_dir),
        }
    }

    pub fn snapshot(&self) -> Snapshot {
        self.snapshot.lock().map(|s| s.clone()).unwrap_or_default()
    }

    pub fn stats(&self) -> SyncStats {
        self.stats.lock().map(|s| s.clone()).unwrap_or_default()
    }

    /// Forgets everything learned from GitHub; the next sync starts cold.
    pub fn clear_cache(&self) {
        self.store_remote(RemoteCache::default());
        self.http_cache.clear();
        let _ = std::fs::remove_file(&self.state_path);
    }

    fn save_state(&self) {
        let saved = Saved {
            snapshot: self.snapshot(),
            stats: self.stats(),
        };
        match serde_json::to_vec(&saved) {
            Ok(json) => {
                if let Err(err) = github::write_atomic(&self.state_path, &json) {
                    log::warn!("could not save {}: {err}", self.state_path.display());
                }
            }
            Err(err) => log::warn!("could not encode snapshot: {err}"),
        }
    }

    fn is_offline(&self) -> bool {
        self.snapshot().issue.is_some_and(|i| i.kind == "offline")
    }

    /// The network is back: forget the offline issue so syncs may run again.
    fn clear_offline(&self) {
        if let Ok(mut snapshot) = self.snapshot.lock()
            && snapshot.issue.as_ref().is_some_and(|i| i.kind == "offline")
        {
            snapshot.issue = None;
        }
    }

    fn take_status_issue(&self) -> Option<SyncIssue> {
        self.status_issue.lock().ok().and_then(|mut i| i.take())
    }

    pub fn progress(&self) -> SyncProgress {
        self.progress.lock().map(|p| *p).unwrap_or_default()
    }

    fn remote(&self) -> RemoteCache {
        self.remote.lock().map(|r| r.clone()).unwrap_or_default()
    }

    fn store_remote(&self, cache: RemoteCache) {
        match serde_json::to_vec(&cache) {
            Ok(json) => {
                if let Err(err) = github::write_atomic(&self.remote_path, &json) {
                    log::warn!("could not save {}: {err}", self.remote_path.display());
                }
            }
            Err(err) => log::warn!("could not encode repo cache: {err}"),
        }
        if let Ok(mut remote) = self.remote.lock() {
            *remote = cache;
        }
    }
}

/// Runs a full sync for the user and broadcasts the result. Concurrent callers wait for
/// the sync in flight and get its result instead of starting another.
pub async fn refresh(app: &AppHandle) -> Snapshot {
    let state = app.state::<ProjectsState>();
    let Ok(guard) = state.sync_lock.try_lock() else {
        drop(state.sync_lock.lock().await);
        return state.snapshot();
    };
    publish(app, Mode::Full, true, guard).await
}

/// Re-reads only the disk (after a clone, commit or settings change) and keeps the last
/// GitHub data, so it costs no API request.
pub async fn refresh_local(app: &AppHandle, foreground: bool) -> Snapshot {
    let state = app.state::<ProjectsState>();
    let guard = state.sync_lock.lock().await;
    publish(app, Mode::Local, foreground, guard).await
}

/// Background syncs skip their turn when another one is running.
async fn refresh_in_background(app: &AppHandle, mode: Mode) {
    let state = app.state::<ProjectsState>();
    let Ok(guard) = state.sync_lock.try_lock() else {
        return;
    };
    publish(app, mode, false, guard).await;
}

/// A push or pull just changed a clone's remote refs: re-read the status now, then twice
/// more while the autograder runs, then every minute while a run is still going, so the
/// new state and grade show up quickly.
pub async fn refresh_after_push(app: &AppHandle) {
    let state = app.state::<ProjectsState>();
    // An editor's auto-fetch can fire this often; one round of checks at a time.
    if state
        .after_push
        .swap(true, std::sync::atomic::Ordering::AcqRel)
    {
        return;
    }
    let started = tokio::time::Instant::now();
    let mut delays = AFTER_PUSH_CHECKS.into_iter();
    loop {
        let delay = match delays.next() {
            Some(delay) => delay,
            None if grading_running(&state.snapshot())
                && started.elapsed() < AFTER_PUSH_GRADING_LIMIT =>
            {
                AFTER_PUSH_GRADING_INTERVAL
            }
            None => break,
        };
        tokio::time::sleep(delay).await;
        if !may_call_github(app) {
            break;
        }
        let guard = state.sync_lock.lock().await;
        publish(app, Mode::Status, false, guard).await;
    }
    state
        .after_push
        .store(false, std::sync::atomic::Ordering::Release);
}

/// Delays between the status checks that follow a push; grading takes a minute or two.
const AFTER_PUSH_CHECKS: [Duration; 3] = [
    Duration::ZERO,
    Duration::from_secs(60),
    Duration::from_secs(120),
];

/// Past the checks above, how often to look again while an autograder is still running,
/// and for how long at most (a stuck run is left to the regular cadence).
const AFTER_PUSH_GRADING_INTERVAL: Duration = Duration::from_secs(60);
const AFTER_PUSH_GRADING_LIMIT: Duration = Duration::from_secs(15 * 60);

fn grading_running(snapshot: &Snapshot) -> bool {
    snapshot.projects.iter().any(|p| {
        p.remote
            .as_ref()
            .is_some_and(|r| matches!(r.grading, Some(Grading::Running { .. })))
    })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Mode {
    /// GitHub discovery, assignment status, local scan and manifests.
    Full,
    /// Assignment status and local scan, on top of the last discovery.
    Status,
    /// Local scan only, on top of the previous snapshot's GitHub data.
    Local,
}

async fn publish(
    app: &AppHandle,
    mode: Mode,
    foreground: bool,
    _guard: tokio::sync::MutexGuard<'_, ()>,
) -> Snapshot {
    let state = app.state::<ProjectsState>();
    let http = &app.state::<Http>().0;
    let settings = app.state::<SettingsStore>().get();

    // After `gh auth switch` the cached repos belong to the other account: start over.
    let mut mode = mode;
    if mode == Mode::Status && tools::active_login().await != state.remote().login {
        log::info!("GitHub account changed; running a full sync");
        mode = Mode::Full;
    }

    let progress = Progress::start(app, foreground);
    let started = std::time::Instant::now();
    let clones = app.state::<CloneStore>().list();
    let (snapshot, targets) = sync(&state, http, &settings, clones, mode, &progress).await;
    progress.finish();
    let run = LastRun {
        at: Utc::now(),
        seconds: started.elapsed().as_secs_f32(),
    };
    log::info!(
        "{mode:?} sync: {} projects in {:.1}s",
        snapshot.projects.len(),
        run.seconds
    );
    if let Ok(mut stats) = state.stats.lock() {
        match mode {
            Mode::Full => stats.full = Some(run),
            Mode::Status => stats.status = Some(run),
            Mode::Local => stats.local = Some(run),
        }
        stats.watch_targets = targets.len();
        if let Some(rate) = github::rate_limit() {
            stats.rate_limit = Some(rate);
        }
    }
    if let Ok(mut current) = state.snapshot.lock() {
        *current = snapshot.clone();
    }
    if let Err(err) = app.emit(UPDATED_EVENT, &snapshot) {
        log::warn!("could not emit {UPDATED_EVENT}: {err}");
    }
    state.save_state();
    watch::rewatch(app, targets);
    crate::alerts::process(app, &snapshot);
    snapshot
}

/// Reports sync progress to the UI as `sync-progress`.
#[derive(Clone)]
struct Progress(AppHandle);

impl Progress {
    fn start(app: &AppHandle, foreground: bool) -> Self {
        let progress = Self(app.clone());
        progress.update(|p| {
            *p = SyncProgress {
                running: true,
                foreground,
                done: 0,
                total: 0,
            }
        });
        progress
    }

    fn add(&self, steps: usize) {
        if steps > 0 {
            self.update(|p| p.total += steps);
        }
    }

    fn add_done(&self, steps: usize) {
        self.update(|p| p.done = (p.done + steps).min(p.total));
    }

    fn advance(&self) {
        self.update(|p| p.done = (p.done + 1).min(p.total));
    }

    fn finish(&self) {
        self.update(|p| p.running = false);
    }

    fn update(&self, change: impl FnOnce(&mut SyncProgress)) {
        let state = self.0.state::<ProjectsState>();
        let Ok(mut progress) = state.progress.lock() else {
            return;
        };
        change(&mut progress);
        if let Err(err) = self.0.emit(PROGRESS_EVENT, *progress) {
            log::debug!("could not emit {PROGRESS_EVENT}: {err}");
        }
    }
}

/// Starts whatever is due in the background: a full sync when the last one is older than
/// `FULL_SYNC_AGE`, else a status check when the last GitHub read is older than
/// `min_status_age`. Nothing while GitHub asks to back off.
fn refresh_due(app: &AppHandle, min_status_age: chrono::TimeDelta) {
    if !may_call_github(app) {
        return;
    }
    let snapshot = app.state::<ProjectsState>().snapshot();
    let now = Utc::now();
    let older = |at: Option<DateTime<Utc>>, age| at.is_none_or(|at| now - at >= age);
    let app = app.clone();
    let (_, full_age) = cadence(&snapshot, now);
    let mode = if older(snapshot.synced_at, full_age) {
        Mode::Full
    } else if older(snapshot.checked_at, min_status_age) {
        Mode::Status
    } else {
        return;
    };
    tauri::async_runtime::spawn(async move {
        refresh_in_background(&app, mode).await;
    });
}

/// Background requests wait while GitHub asks to back off, and keep a reserve of the
/// budget for the user's own tools: they pause below 10% until the window resets.
fn may_call_github(app: &AppHandle) -> bool {
    let state = app.state::<ProjectsState>();
    let now = Utc::now();
    let backing_off = state
        .snapshot()
        .issue
        .and_then(|issue| issue.retry_after)
        .is_some_and(|until| now < until);
    let low_budget = github::rate_limit()
        .or(state.stats().rate_limit)
        .is_some_and(|r| {
            r.remaining.saturating_mul(10) < r.limit && r.reset_at.is_none_or(|at| now < at)
        });
    if low_budget {
        log::info!("GitHub budget below 10%; background syncs paused until it resets");
    }
    // Offline, only the reachability probe in the background loop runs.
    !backing_off && !low_budget && !state.is_offline()
}

/// When the popover opens.
pub fn refresh_if_stale(app: &AppHandle) {
    refresh_due(app, MIN_STATUS_ON_OPEN);
}

/// At startup, re-reads the disk on top of the saved snapshot (no request). Then a tick
/// every 30 s decides what is due, at a pace set by the nearest unfinished deadline, and
/// reacts to waking from sleep and to the network coming back.
pub fn start_background_sync(app: AppHandle) {
    tauri::async_runtime::spawn(async move {
        if app.state::<ProjectsState>().snapshot().checked_at.is_some() {
            refresh_local(&app, false).await;
        }
        let mut wall = std::time::SystemTime::now();
        let mut monotonic = std::time::Instant::now();
        loop {
            // The monotonic clock stops while the machine sleeps; the wall clock doesn't.
            let (wall_now, monotonic_now) =
                (std::time::SystemTime::now(), std::time::Instant::now());
            let slept = wall_now
                .duration_since(wall)
                .unwrap_or_default()
                .saturating_sub(monotonic_now - monotonic)
                > Duration::from_secs(60);
            (wall, monotonic) = (wall_now, monotonic_now);

            let state = app.state::<ProjectsState>();
            if state.is_offline() {
                if github_reachable().await {
                    log::info!("network is back; syncing");
                    state.clear_offline();
                    refresh_due(&app, chrono::TimeDelta::zero());
                }
            } else if slept {
                log::info!("woke from sleep; syncing");
                // Give Wi-Fi a moment to reconnect.
                tokio::time::sleep(Duration::from_secs(5)).await;
                refresh_due(&app, chrono::TimeDelta::zero());
            } else {
                let (status_age, _) = cadence(&state.snapshot(), Utc::now());
                refresh_due(&app, status_age);
            }
            tokio::time::sleep(TICK).await;
        }
    });
}

const TICK: Duration = Duration::from_secs(30);

/// How old the last status check and the last full sync may get, from the nearest
/// deadline of unfinished work: every minute in its last 3 hours, every 2 within a day,
/// every 5 within a week, and 15 (full sync hourly) when nothing is due that soon.
fn cadence(snapshot: &Snapshot, now: DateTime<Utc>) -> (chrono::TimeDelta, chrono::TimeDelta) {
    use chrono::TimeDelta as T;
    let soonest = snapshot
        .projects
        .iter()
        .filter(|p| crate::alerts::needs_work(p))
        .filter_map(crate::alerts::open_due)
        .filter(|due| *due > now)
        .min();
    // A few seconds under each period, so a 30 s tick doesn't skip a turn.
    let slack = T::seconds(10);
    match soonest.map(|due| due - now) {
        Some(left) if left <= T::hours(3) => (T::minutes(1) - slack, FULL_SYNC_AGE),
        Some(left) if left <= T::hours(24) => (T::minutes(2) - slack, FULL_SYNC_AGE),
        Some(left) if left <= T::days(7) => (T::minutes(5) - slack, FULL_SYNC_AGE),
        _ => (T::minutes(15) - slack, T::hours(1)),
    }
}

/// A TCP handshake with the API host: no HTTP request, so nothing counts against any limit.
async fn github_reachable() -> bool {
    let connect = tokio::net::TcpStream::connect(("api.github.com", 443));
    matches!(
        tokio::time::timeout(Duration::from_secs(3), connect).await,
        Ok(Ok(_))
    )
}

/// Returns the snapshot and what to watch for local changes.
async fn sync(
    state: &ProjectsState,
    http: &reqwest::Client,
    settings: &Settings,
    clones: Vec<PathBuf>,
    mode: Mode,
    progress: &Progress,
) -> (Snapshot, Vec<WatchTarget>) {
    let now = Utc::now();
    let scan = scan_local(
        settings.watched_folders.clone(),
        clones,
        settings.scan_editor_projects,
        progress.clone(),
    );
    let (snapshot, scan_targets) = if mode != Mode::Full {
        let (locals, targets) = scan.await;
        let status = mode == Mode::Status;
        (
            local_snapshot(state, http, locals, now, status, progress).await,
            targets,
        )
    } else {
        let (remote, (locals, targets)) = tokio::join!(sync_remote(state, http, progress), scan);
        (
            full_snapshot(state, http, remote, locals, now, progress).await,
            targets,
        )
    };
    let mut snapshot = snapshot;
    snapshot.login = state.remote().login;
    let targets = watch_targets(&settings.watched_folders, scan_targets, &snapshot.projects);
    (snapshot, targets)
}

/// Watched folders recursively, clones found elsewhere recursively too (to see commits),
/// and the editor files and folders that could reveal a new clone, non-recursively.
fn watch_targets(
    folders: &[PathBuf],
    scan_targets: Vec<WatchTarget>,
    projects: &[Project],
) -> Vec<WatchTarget> {
    let clones = projects
        .iter()
        .filter_map(|p| p.path.clone())
        .filter(|path| !folders.iter().any(|f| path.starts_with(f)));
    let mut targets: Vec<WatchTarget> = folders
        .iter()
        .cloned()
        .chain(clones)
        .map(WatchTarget::recursive)
        .chain(scan_targets)
        .collect();
    targets.sort();
    targets.dedup_by(|a, b| a.path == b.path);
    targets
}

async fn local_snapshot(
    state: &ProjectsState,
    http: &reqwest::Client,
    locals: Vec<LocalProject>,
    now: DateTime<Utc>,
    status: bool,
    progress: &Progress,
) -> Snapshot {
    let previous = state.snapshot();
    let remote_repos = previous
        .projects
        .into_iter()
        .filter_map(|p| p.remote)
        .collect();
    let classrooms = state.remote().classrooms;
    let auth = if status {
        status_auth(state).await
    } else {
        None
    };
    let checked_at = if auth.is_some() {
        Some(now)
    } else {
        previous.checked_at
    };
    let sources = Sources {
        remote: remote_repos,
        classrooms,
        locals,
    };
    let (projects, unaccepted) = build(sources, state, http, auth.as_ref(), now, progress).await;
    // A status read replaces the issue: cleared when it worked, its error when it didn't.
    let (checked_at, issue) = match (auth.is_some(), state.take_status_issue()) {
        (true, None) => (checked_at, None),
        (true, Some(issue)) => (previous.checked_at, Some(issue)),
        (false, _) => (checked_at, previous.issue),
    };
    Snapshot {
        projects,
        unaccepted,
        synced_at: previous.synced_at,
        checked_at,
        issue,
        login: None,
    }
}

async fn full_snapshot(
    state: &ProjectsState,
    http: &reqwest::Client,
    remote: Result<()>,
    locals: Vec<LocalProject>,
    now: DateTime<Utc>,
    progress: &Progress,
) -> Snapshot {
    let (repos, classrooms, issue) = match remote {
        Ok(()) => {
            let cache = state.remote();
            (cache.repos.into_values().collect(), cache.classrooms, None)
        }
        // Without a usable login the cached repos may belong to another account.
        Err(err @ (Error::NotSignedIn | Error::NotInstalled { .. })) => {
            (Vec::new(), Vec::new(), Some(SyncIssue::from(&err)))
        }
        // Offline or rate limited: keep showing what was last seen.
        Err(err) => {
            log::warn!("GitHub sync failed: {err}");
            let cache = state.remote();
            (
                cache.repos.into_values().collect(),
                cache.classrooms,
                Some(SyncIssue::from(&err)),
            )
        }
    };

    let auth = if issue.is_none() {
        status_auth(state).await
    } else {
        None
    };
    let sources = Sources {
        remote: repos,
        classrooms,
        locals,
    };
    let (projects, unaccepted) = build(sources, state, http, auth.as_ref(), now, progress).await;
    Snapshot {
        projects,
        unaccepted,
        synced_at: Some(now),
        checked_at: Some(now),
        issue: issue.or_else(|| state.take_status_issue()),
        login: None,
    }
}

type ClassroomKey = (String, String);

/// Resolves which repos are assignments, then merges, annotates and sorts them.
/// What a build merges: repos seen on GitHub, classrooms from teams, clones on disk.
struct Sources {
    remote: Vec<RemoteRepo>,
    classrooms: Vec<Classroom>,
    locals: Vec<LocalProject>,
}

async fn build(
    sources: Sources,
    state: &ProjectsState,
    http: &reqwest::Client,
    status: Option<&StatusAuth>,
    now: DateTime<Utc>,
    progress: &Progress,
) -> (Vec<Project>, Vec<Unaccepted>) {
    let Sources {
        remote,
        classrooms: team_classrooms,
        locals,
    } = sources;
    // Teams carry the display name and secret, so they win over marker files.
    let mut classrooms: BTreeMap<ClassroomKey, Classroom> =
        team_classrooms.into_iter().map(|c| (c.key(), c)).collect();
    let configs = remote
        .iter()
        .filter_map(|r| Some((r.owner.as_str(), r.config.as_ref()?)))
        .chain(
            locals
                .iter()
                .filter_map(|l| Some((l.owner.as_deref()?, l.config.as_ref()?))),
        );
    for (owner, config) in configs {
        let classroom = Classroom::from_config(owner, config);
        classrooms.entry(classroom.key()).or_insert(classroom);
    }
    let manifests = fetch_manifests(&classrooms, &state.http_cache, http, progress).await;
    state.http_cache.save();

    let identify = |owner: Option<&str>, name: Option<&str>, config: Option<&RepoConfig>| {
        identify(owner, name, config, &classrooms, &manifests)
    };
    let remote: Vec<(RemoteRepo, RepoConfig)> = remote
        .into_iter()
        .filter_map(|repo| {
            let config = identify(Some(&repo.owner), Some(&repo.name), repo.config.as_ref())?;
            Some((repo, config))
        })
        .collect();
    let remote = match status {
        Some(auth) => refresh_statuses(state, http, auth, remote, progress).await,
        None => remote,
    };
    let locals: Vec<(LocalProject, RepoConfig)> = locals
        .into_iter()
        .filter_map(|clone| {
            let config = identify(
                clone.owner.as_deref(),
                clone.repo.as_deref(),
                clone.config.as_ref(),
            )?;
            Some((clone, config))
        })
        .collect();
    let states = local_states(locals.iter().map(|(clone, _)| clone.path.clone()), progress).await;

    let mut projects = merge(
        remote,
        locals
            .into_iter()
            .zip(states)
            .map(|((c, cfg), s)| (c, cfg, s))
            .collect(),
    );
    for project in &mut projects {
        let key = project.owner.as_deref().map(|owner| {
            (
                owner.to_ascii_lowercase(),
                project.config.classroom.to_ascii_lowercase(),
            )
        });
        project.classroom = key.as_ref().and_then(|k| classrooms.get(k)).cloned();
        project.assignment = key
            .as_ref()
            .and_then(|k| manifests.get(k))
            .and_then(|list| list.iter().find(|a| a.slug == project.config.assignment))
            .cloned();
        project.web_url = project.owner.as_deref().map(|owner| {
            classroom50::submission_url(
                owner,
                &project.config.classroom,
                &project.config.assignment,
            )
        });
    }
    check_clones(&mut projects).await;
    for project in &mut projects {
        project.pending = pending(project);
        project.late_seconds = late_seconds(project);
    }
    projects.sort_by_cached_key(|p| sort_key(p, now));
    let unaccepted = unaccepted(&projects, &classrooms, &manifests);
    (projects, unaccepted)
}

/// Open assignments of the user's classrooms with no repo yet, published or upcoming, by
/// due date.
fn unaccepted(
    projects: &[Project],
    classrooms: &BTreeMap<ClassroomKey, Classroom>,
    manifests: &HashMap<ClassroomKey, Vec<Assignment>>,
) -> Vec<Unaccepted> {
    let accepted: HashSet<(ClassroomKey, &str)> = projects
        .iter()
        .filter_map(|p| {
            let key = (
                p.owner.as_deref()?.to_ascii_lowercase(),
                p.config.classroom.to_ascii_lowercase(),
            );
            Some((key, p.config.assignment.as_str()))
        })
        .collect();
    let mut list: Vec<Unaccepted> = manifests
        .iter()
        .filter_map(|(key, assignments)| Some((classrooms.get(key)?, key, assignments)))
        .filter(|(classroom, _, _)| classroom.active)
        .flat_map(|(classroom, key, assignments)| {
            assignments
                .iter()
                // Upcoming ones (`available_from` ahead) are listed too, as "Opens ...".
                .filter(|a| !a.closed)
                .filter(|a| !accepted.contains(&(key.clone(), a.slug.as_str())))
                .map(|a| Unaccepted {
                    id: format!("{}/{}/{}", key.0, key.1, a.slug),
                    classroom: classroom.clone(),
                    assignment: a.clone(),
                    accept_url: classroom50::accept_url(
                        &classroom.org,
                        &classroom.slug,
                        &a.slug,
                        classroom.secret.as_deref(),
                    ),
                })
        })
        .collect();
    list.sort_by_key(|u| (u.assignment.due.is_none(), u.assignment.due));
    list
}

/// The assignment identity of a repo: its marker file, else its name matched against the
/// manifests of the owner's classrooms. `None` means it isn't an assignment repo.
fn identify(
    owner: Option<&str>,
    name: Option<&str>,
    config: Option<&RepoConfig>,
    classrooms: &BTreeMap<ClassroomKey, Classroom>,
    manifests: &HashMap<ClassroomKey, Vec<Assignment>>,
) -> Option<RepoConfig> {
    if let Some(config) = config {
        return Some(config.clone());
    }
    let (owner, name) = (owner?.to_ascii_lowercase(), name?);
    classrooms
        .iter()
        .filter(|((org, _), _)| *org == owner)
        .find_map(|(key, classroom)| {
            let slugs = manifests.get(key)?.iter().map(|a| a.slug.as_str());
            let slug = classroom50::assignment_for_repo(name, &classroom.slug, slugs)?;
            Some(RepoConfig {
                classroom: classroom.slug.clone(),
                assignment: slug.to_owned(),
                secret: classroom.secret.clone(),
            })
        })
}

/// Fetches each classroom's manifest once (conditional GET).
async fn fetch_manifests(
    classrooms: &BTreeMap<ClassroomKey, Classroom>,
    cache: &HttpCache,
    http: &reqwest::Client,
    progress: &Progress,
) -> HashMap<ClassroomKey, Vec<Assignment>> {
    progress.add(classrooms.len());
    let fetches = classrooms.iter().map(|(key, classroom)| async move {
        let url = classroom.assignments_url();
        let manifest = match cache.get_text(http, &url, MANIFEST_MAX_AGE).await {
            Ok(body) => classroom50::parse_assignments(&url, &body)
                .inspect_err(|err| log::warn!("{err}"))
                .ok(),
            Err(err) => {
                log::warn!("could not fetch {url}: {err}");
                None
            }
        };
        progress.advance();
        manifest.map(|m| (key.clone(), m))
    });
    join_all(fetches).await.into_iter().flatten().collect()
}

// ---- Remote discovery ----------------------------------------------------------------

const LIST_QUERY: &str = r#"
query($cursor: String) {
  viewer {
    login
    repositories(first: 100, after: $cursor,
                 affiliations: [OWNER, COLLABORATOR, ORGANIZATION_MEMBER],
                 ownerAffiliations: [OWNER, COLLABORATOR, ORGANIZATION_MEMBER],
                 orderBy: {field: PUSHED_AT, direction: DESC}) {
      pageInfo { hasNextPage endCursor }
      nodes { nameWithOwner pushedAt }
    }
  }
}"#;

const REPO_FRAGMENT: &str = r#"
fragment Repo on Repository {
  name
  owner { login }
  url
  isArchived
  viewerPermission
  defaultBranchRef {
    name target { oid ... on Commit { committedDate author { name user { login } } } }
  }
  config: object(expression: "HEAD:.classroom50.yaml") { ... on Blob { text } }
  submissions: refs(refPrefix: "refs/tags/submit/", first: 1,
                    orderBy: {field: ALPHABETICAL, direction: DESC}) {
    nodes { name target { oid ... on Tag { target { oid } } } }
  }
}"#;

#[derive(Deserialize)]
struct ListData {
    viewer: Viewer,
}

#[derive(Deserialize)]
struct Viewer {
    login: String,
    repositories: Connection<ListedRepo>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Connection<T> {
    page_info: PageInfo,
    nodes: Vec<Option<T>>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct PageInfo {
    has_next_page: bool,
    end_cursor: Option<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ListedRepo {
    name_with_owner: String,
    pushed_at: Option<DateTime<Utc>>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct RepoNode {
    name: String,
    owner: Login,
    url: String,
    is_archived: bool,
    viewer_permission: Option<String>,
    default_branch_ref: Option<BranchRef>,
    config: Option<Blob>,
    submissions: Option<RefConnection>,
}

#[derive(Deserialize)]
struct Login {
    login: String,
}

#[derive(Deserialize)]
struct BranchRef {
    name: String,
    target: Option<GitObject>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct GitObject {
    oid: String,
    committed_date: Option<DateTime<Utc>>,
    author: Option<CommitAuthor>,
    /// Set on annotated tags: the object the tag points at.
    target: Option<Box<GitObject>>,
}

#[derive(Deserialize)]
struct CommitAuthor {
    name: Option<String>,
    user: Option<Login>,
}

#[derive(Deserialize)]
struct Blob {
    text: Option<String>,
}

#[derive(Deserialize)]
struct RefConnection {
    nodes: Vec<RefNode>,
}

#[derive(Deserialize)]
struct RefNode {
    name: String,
    target: Option<GitObject>,
}

impl RepoNode {
    fn into_remote(self) -> RemoteRepo {
        let config = self
            .config
            .and_then(|blob| blob.text)
            .and_then(|text| RepoConfig::parse(&text).ok());
        let last_submission = self
            .submissions
            .and_then(|refs| refs.nodes.into_iter().next())
            .and_then(|node| {
                let target = node.target?;
                let commit = target.target.map_or(target.oid, |peeled| peeled.oid);
                Some(Submission {
                    at: parse_submit_time(&node.name),
                    tag: format!("{SUBMIT_TAG_PREFIX}{}", node.name),
                    commit,
                })
            });
        let (default_branch, head) = match self.default_branch_ref {
            Some(branch) => (
                Some(branch.name),
                branch.target.map(|t| Commit {
                    oid: t.oid,
                    at: t.committed_date,
                    author: t.author.and_then(|a| a.user.map(|u| u.login).or(a.name)),
                }),
            ),
            None => (None, None),
        };
        RemoteRepo {
            owner: self.owner.login,
            name: self.name,
            url: self.url,
            archived: self.is_archived,
            can_push: matches!(
                self.viewer_permission.as_deref(),
                Some("ADMIN" | "MAINTAIN" | "WRITE")
            ),
            default_branch,
            head,
            last_submission,
            config,
            unmerged: Vec::new(),
            grade: None,
            grading: None,
            feedback: None,
        }
    }
}

/// `2026-10-14T12-00-05Z-abc1234` (the part after `submit/`) -> its UTC instant.
fn parse_submit_time(name: &str) -> Option<DateTime<Utc>> {
    let stamp = name.get(..20)?;
    NaiveDateTime::parse_from_str(stamp, "%Y-%m-%dT%H-%M-%SZ")
        .ok()
        .map(|t| t.and_utc())
}

/// Lists every repo the user can see, inspects only those pushed since the last sync, and
/// reads the user's classroom teams. Results land in the persisted cache; progress is kept
/// even when a later request fails.
async fn sync_remote(
    state: &ProjectsState,
    http: &reqwest::Client,
    progress: &Progress,
) -> Result<()> {
    // Listing repos and reading teams; the detail batches are added once known.
    progress.add(2);
    let token = tools::gh_token().await?;
    // The teams query needs the login; gh knows it locally, so both run at once.
    let (login, listed, classrooms) = match tools::active_login().await {
        Some(active) => {
            let (repos, classrooms) = tokio::join!(
                list_repos(http, &token),
                list_classrooms(http, &token, &active)
            );
            let (login, listed) = repos?;
            (login, listed, classrooms)
        }
        None => {
            let (login, listed) = list_repos(http, &token).await?;
            let classrooms = list_classrooms(http, &token, &login).await;
            (login, listed, classrooms)
        }
    };
    progress.add_done(2);
    if let Ok(mut stats) = state.stats.lock() {
        stats.repos_seen = listed.len();
    }

    let mut cache = state.remote();
    if cache.login.as_deref() != Some(login.as_str()) {
        cache = RemoteCache {
            login: Some(login),
            ..RemoteCache::default()
        };
    }
    let listed_keys: HashSet<String> = listed.iter().map(|r| key(&r.name_with_owner)).collect();
    cache.checked.retain(|k, _| listed_keys.contains(k));
    cache.repos.retain(|k, _| listed_keys.contains(k));

    let stale: Vec<&ListedRepo> = listed
        .iter()
        .filter(|r| cache.checked.get(&key(&r.name_with_owner)) != Some(&r.pushed_at))
        .collect();
    progress.add(stale.len().div_ceil(DETAIL_BATCH));
    let mut result = Ok(());
    'requests: for group in stale.chunks(DETAIL_BATCH * PARALLEL_BATCHES) {
        let batches: Vec<_> = group.chunks(DETAIL_BATCH).collect();
        let names: Vec<Vec<&str>> = batches
            .iter()
            .map(|batch| batch.iter().map(|r| r.name_with_owner.as_str()).collect())
            .collect();
        let responses = join_all(
            names
                .iter()
                .map(|batch| fetch_nodes::<RepoNode>(http, &token, batch, "Repo", REPO_FRAGMENT)),
        )
        .await;
        for (batch, response) in batches.into_iter().zip(responses) {
            progress.advance();
            let mut nodes = match response {
                Ok(nodes) => nodes,
                Err(err) => {
                    result = Err(err);
                    break 'requests;
                }
            };
            for (i, listed) in batch.iter().enumerate() {
                // A missing alias means the repo vanished or became inaccessible.
                let Some(node) = nodes.remove(&format!("r{i}")) else {
                    continue;
                };
                let k = key(&listed.name_with_owner);
                cache.checked.insert(k.clone(), listed.pushed_at);
                match node {
                    Some(node) => {
                        let mut repo = node.into_remote();
                        if let Some(old) = cache.repos.get(&k) {
                            keep_status(&mut repo, old);
                        }
                        cache.repos.insert(k, repo)
                    }
                    None => cache.repos.remove(&k),
                };
            }
        }
    }
    match classrooms {
        Ok(classrooms) => cache.classrooms = classrooms,
        // Teams are a bonus over marker files; keep the last known list.
        Err(err) => log::warn!("could not read classroom teams: {err}"),
    }
    state.store_remote(cache);
    result
}

const TEAMS_QUERY: &str = r#"
query($login: String!, $cursor: String) {
  viewer {
    organizations(first: 100, after: $cursor) {
      pageInfo { hasNextPage endCursor }
      nodes {
        login
        teams(first: 100, userLogins: [$login], query: "classroom50-") {
          nodes { slug description }
        }
      }
    }
  }
}"#;

#[derive(Deserialize)]
struct TeamsData {
    viewer: TeamsViewer,
}

#[derive(Deserialize)]
struct TeamsViewer {
    organizations: Connection<OrgNode>,
}

#[derive(Deserialize)]
struct OrgNode {
    login: String,
    teams: TeamConnection,
}

#[derive(Deserialize)]
struct TeamConnection {
    nodes: Vec<Option<TeamNode>>,
}

#[derive(Deserialize)]
struct TeamNode {
    slug: String,
    description: Option<String>,
}

/// The classrooms the user is enrolled in, from their `classroom50-<classroom>` teams.
async fn list_classrooms(
    http: &reqwest::Client,
    token: &str,
    login: &str,
) -> Result<Vec<Classroom>> {
    let mut classrooms = Vec::new();
    let mut cursor: Option<String> = None;
    for _ in 0..MAX_LIST_PAGES {
        let variables = json!({ "login": login, "cursor": cursor });
        let data: TeamsData = github::graphql(http, token, TEAMS_QUERY, variables).await?;
        let page = data.viewer.organizations;
        for org in page.nodes.into_iter().flatten() {
            classrooms.extend(org.teams.nodes.into_iter().flatten().filter_map(|team| {
                Classroom::from_team(&org.login, &team.slug, team.description.as_deref())
            }));
        }
        match page.page_info.end_cursor {
            Some(next) if page.page_info.has_next_page => cursor = Some(next),
            _ => break,
        }
    }
    Ok(classrooms)
}

async fn list_repos(http: &reqwest::Client, token: &str) -> Result<(String, Vec<ListedRepo>)> {
    let mut repos = Vec::new();
    let mut cursor: Option<String> = None;
    let mut login = String::new();
    for _ in 0..MAX_LIST_PAGES {
        let data: ListData =
            github::graphql(http, token, LIST_QUERY, json!({ "cursor": cursor })).await?;
        login = data.viewer.login;
        let page = data.viewer.repositories;
        repos.extend(page.nodes.into_iter().flatten());
        match page.page_info.end_cursor {
            Some(next) if page.page_info.has_next_page => cursor = Some(next),
            _ => break,
        }
    }
    Ok((login, repos))
}

/// One aliased query (`r0`, `r1`, ...) spreading `fragment` on each `owner/name`. Names
/// travel as variables, never spliced into the query text.
async fn fetch_nodes<T: DeserializeOwned>(
    http: &reqwest::Client,
    token: &str,
    names: &[&str],
    fragment: &str,
    fragments: &str,
) -> Result<HashMap<String, Option<T>>> {
    let mut params = Vec::new();
    let mut fields = String::new();
    let mut variables = Map::new();
    for (i, name_with_owner) in names.iter().enumerate() {
        let Some((owner, name)) = name_with_owner.split_once('/') else {
            continue;
        };
        params.push(format!("$o{i}: String!, $n{i}: String!"));
        fields.push_str(&format!(
            "r{i}: repository(owner: $o{i}, name: $n{i}) {{ ...{fragment} }}\n"
        ));
        variables.insert(format!("o{i}"), owner.into());
        variables.insert(format!("n{i}"), name.into());
    }
    let query = format!("query({}) {{\n{fields}}}\n{fragments}", params.join(", "));
    github::graphql(http, token, &query, Value::Object(variables)).await
}

// ---- Assignment status ---------------------------------------------------------------

pub(crate) struct StatusAuth {
    token: String,
    login: String,
}

/// The token and login for a status read, or `None` when signed out or never synced.
async fn status_auth(state: &ProjectsState) -> Option<StatusAuth> {
    let login = state.remote().login?;
    let token = tools::gh_token().await.ok()?;
    Some(StatusAuth { token, login })
}

#[derive(Deserialize)]
struct StatusNode {
    #[serde(flatten)]
    repo: RepoNode,
    #[serde(flatten)]
    status: StatusFields,
}

/// Status fields survive a discovery re-read, which doesn't fetch them.
fn keep_status(repo: &mut RemoteRepo, old: &RemoteRepo) {
    repo.unmerged.clone_from(&old.unmerged);
    repo.grade.clone_from(&old.grade);
    repo.grading.clone_from(&old.grading);
    repo.feedback.clone_from(&old.feedback);
}

/// Re-reads the assignment repos with the status fragment, downloads new grades and
/// stores the result in the cache. A repo that can't be read keeps its last data.
async fn refresh_statuses(
    state: &ProjectsState,
    http: &reqwest::Client,
    auth: &StatusAuth,
    repos: Vec<(RemoteRepo, RepoConfig)>,
    progress: &Progress,
) -> Vec<(RemoteRepo, RepoConfig)> {
    let fragments = format!("{REPO_FRAGMENT}{}", status::FRAGMENT);
    let names: Vec<String> = repos
        .iter()
        .map(|(r, _)| format!("{}/{}", r.owner, r.name))
        .collect();
    let batches: Vec<Vec<&str>> = names
        .chunks(STATUS_BATCH)
        .map(|batch| batch.iter().map(String::as_str).collect())
        .collect();
    progress.add(batches.len());
    let mut responses = Vec::with_capacity(batches.len());
    for group in batches.chunks(MAX_STATUS_REQUESTS) {
        // GitHub asked to slow down: no more requests this round.
        if responses
            .iter()
            .any(|r: &Result<_>| matches!(r, Err(Error::RateLimited { .. })))
        {
            break;
        }
        responses.extend(
            join_all(group.iter().map(|batch| async {
                let response =
                    fetch_nodes::<StatusNode>(http, &auth.token, batch, "Status", &fragments).await;
                progress.advance();
                response
            }))
            .await,
        );
    }

    let mut nodes: HashMap<usize, StatusNode> = HashMap::new();
    for (b, response) in responses.into_iter().enumerate() {
        match response {
            Ok(batch) => {
                for (alias, node) in batch {
                    let Some(i) = alias
                        .strip_prefix('r')
                        .and_then(|i| i.parse::<usize>().ok())
                    else {
                        continue;
                    };
                    if let Some(node) = node {
                        nodes.insert(b * STATUS_BATCH + i, node);
                    }
                }
            }
            Err(err) => {
                log::warn!("status check failed: {err}");
                if let Ok(mut issue) = state.status_issue.lock() {
                    issue.get_or_insert_with(|| SyncIssue::from(&err));
                }
            }
        }
    }

    let mut updated = Vec::with_capacity(repos.len());
    for (i, (old, config)) in repos.into_iter().enumerate() {
        let Some(StatusNode { repo, mut status }) = nodes.remove(&i) else {
            updated.push((old, config));
            continue;
        };
        let mut repo = repo.into_remote();
        repo.unmerged = status.unmerged(repo.default_branch.as_deref());
        repo.feedback = status.feedback(&auth.login);
        repo.grading = status.grading();
        repo.grade = match status.graded_release() {
            Some(release)
                if old
                    .grade
                    .as_ref()
                    .is_some_and(|g| g.tag == release.tag && g.has_tests()) =>
            {
                old.grade
            }
            Some(release) => match status::download_grade(http, &release).await {
                Ok(grade) => Some(grade),
                Err(err) => {
                    log::warn!("{err}");
                    old.grade
                }
            },
            None => None,
        };
        updated.push((repo, config));
    }

    let mut cache = state.remote();
    for (repo, _) in &updated {
        cache
            .repos
            .insert(key(&format!("{}/{}", repo.owner, repo.name)), repo.clone());
    }
    state.store_remote(cache);
    updated
}

fn key(name_with_owner: &str) -> String {
    name_with_owner.to_ascii_lowercase()
}

// ---- Local clones ----------------------------------------------------------------------

/// Candidate clones, and the editor files and folders to watch for new ones. Looks in the
/// watched folders, the clones the app made and, when `editors` is on, in the editors' recent projects, the folders
/// holding them and the IDEs' default project folders.
async fn scan_local(
    folders: Vec<PathBuf>,
    clones: Vec<PathBuf>,
    editors: bool,
    progress: Progress,
) -> (Vec<LocalProject>, Vec<WatchTarget>) {
    tauri::async_runtime::spawn_blocking(move || {
        let (roots, targets) = scan_roots(folders, clones, editors);
        progress.add(roots.len());
        if let Ok(mut stats) = progress.0.state::<ProjectsState>().stats.lock() {
            stats.scan_roots = roots.len();
        }
        let mut seen = HashSet::new();
        let mut locals = Vec::new();
        for (root, depth) in &roots {
            let found = discovery::scan(root, *depth);
            locals.extend(found.into_iter().filter(|l| seen.insert(l.path.clone())));
            progress.advance();
        }
        (locals, targets)
    })
    .await
    .unwrap_or_default()
}

/// Folders to scan with their depth, and the non-recursive watch targets.
fn scan_roots(
    folders: Vec<PathBuf>,
    clones: Vec<PathBuf>,
    editors: bool,
) -> (Vec<(PathBuf, usize)>, Vec<WatchTarget>) {
    let mut roots: Vec<(PathBuf, usize)> = folders
        .iter()
        .map(|folder| (folder.clone(), discovery::DEFAULT_MAX_DEPTH))
        .collect();
    // A clone the app made is a repo itself: only its own folder is read.
    roots.extend(
        clones
            .into_iter()
            .filter(|clone| !folders.iter().any(|f| clone.starts_with(f)))
            .map(|clone| (clone, 0)),
    );
    let mut targets = Vec::new();
    if !editors {
        return (roots, targets);
    }
    let found = editors::find();
    let home = crate::platform::home_dir();
    let usable =
        |dir: &Path| !folders.iter().any(|f| dir.starts_with(f)) && is_narrow(dir, home.as_deref());
    let parents: Vec<PathBuf> = found
        .recent
        .iter()
        .filter_map(|recent| recent.path.parent().map(Path::to_path_buf))
        .collect();
    let roots_found: Vec<PathBuf> = found.roots.into_iter().map(|root| root.path).collect();
    // A recent folder may be a course folder holding the clones; its siblings may be
    // clones the editor never opened.
    let candidates = found
        .recent
        .into_iter()
        .map(|recent| (recent.path, EDITOR_SCAN_DEPTH))
        .chain(parents.iter().cloned().map(|dir| (dir, 1)))
        .chain(
            roots_found
                .iter()
                .cloned()
                .map(|dir| (dir, EDITOR_SCAN_DEPTH)),
        );
    for (dir, depth) in candidates {
        if !usable(&dir) {
            continue;
        }
        match roots.iter_mut().find(|(root, _)| *root == dir) {
            Some((_, existing)) => *existing = (*existing).max(depth),
            None => roots.push((dir, depth)),
        }
    }
    targets.extend(found.files.into_iter().map(WatchTarget::flat));
    targets.extend(
        parents
            .into_iter()
            .chain(roots_found)
            .filter(|dir| usable(dir))
            .map(WatchTarget::flat),
    );
    (roots, targets)
}

/// Deep enough for `~/IdeaProjects/<course>/<assignment>`.
const EDITOR_SCAN_DEPTH: usize = 2;

/// Whether walking `dir` stays clear of the home dir as a whole: on macOS, touching
/// `~/Documents`, `~/Desktop` or `~/Downloads` raises a permission prompt, and the home
/// dir is too big anyway. Filesystem roots are refused for the same reason.
fn is_narrow(dir: &Path, home: Option<&Path>) -> bool {
    dir.parent().is_some() && home.is_none_or(|home| !home.starts_with(dir))
}

/// `git status` of each clone, concurrently, in input order.
async fn local_states(
    paths: impl Iterator<Item = PathBuf>,
    progress: &Progress,
) -> Vec<Option<LocalState>> {
    let mut tasks = JoinSet::new();
    let mut count = 0;
    for (i, path) in paths.enumerate() {
        tasks.spawn(async move { (i, local_state(&path).await) });
        count += 1;
    }
    progress.add(count);
    let mut states = vec![None; count];
    while let Some(Ok((i, state))) = tasks.join_next().await {
        progress.advance();
        states[i] = state;
    }
    states
}

/// `git status` without taking the index lock, so it never races the student's own git.
async fn local_state(path: &Path) -> Option<LocalState> {
    let args = [
        "--no-optional-locks",
        "status",
        "--porcelain=v2",
        "--branch",
    ];
    match process::run("git", args, Some(path)).await {
        Ok(out) => Some(parse_status(&out.stdout)),
        Err(err) => {
            log::debug!("git status in {}: {err}", path.display());
            None
        }
    }
}

/// Compares each clone with GitHub's data: unmerged local branches, and whether the
/// remote head is missing (needs a pull). No network: only the local object store.
async fn check_clones(projects: &mut [Project]) {
    let mut tasks = JoinSet::new();
    for (i, project) in projects.iter().enumerate() {
        let (Some(path), Some(local)) = (project.path.clone(), project.local.as_ref()) else {
            continue;
        };
        let remote = project.remote.as_ref();
        let default_branch = remote.and_then(|r| r.default_branch.clone());
        let remote_head = remote
            .and_then(|r| r.head.as_ref())
            .map(|h| h.oid.clone())
            .filter(|oid| local.head.as_deref() != Some(oid.as_str()));
        tasks.spawn(async move {
            let unmerged = match default_branch {
                Some(branch) => unmerged_branches(&path, &branch).await,
                None => Vec::new(),
            };
            let remote_ahead = match remote_head {
                Some(oid) => !has_commit(&path, &oid).await,
                None => false,
            };
            (i, unmerged, remote_ahead)
        });
    }
    while let Some(Ok((i, unmerged, remote_ahead))) = tasks.join_next().await {
        if let Some(local) = projects[i].local.as_mut() {
            local.unmerged = unmerged;
            local.remote_ahead = remote_ahead;
        }
    }
}

/// Local branches with commits not on `default` (the feedback branch never counts).
async fn unmerged_branches(path: &Path, default: &str) -> Vec<String> {
    let no_merged = format!("--no-merged=refs/heads/{default}");
    let args = [
        "for-each-ref",
        no_merged.as_str(),
        "--format=%(refname:short)",
        "refs/heads",
    ];
    match process::run("git", args, Some(path)).await {
        Ok(out) => out
            .stdout
            .lines()
            .map(str::trim)
            .filter(|b| !b.is_empty() && *b != default && *b != "feedback")
            .map(str::to_owned)
            .collect(),
        // No local default branch yet: nothing to compare with.
        Err(_) => Vec::new(),
    }
}

async fn has_commit(path: &Path, oid: &str) -> bool {
    let object = format!("{oid}^{{commit}}");
    process::run("git", ["cat-file", "-e", object.as_str()], Some(path))
        .await
        .is_ok()
}

fn parse_status(porcelain: &str) -> LocalState {
    let mut state = LocalState::default();
    for line in porcelain.lines() {
        if let Some(header) = line.strip_prefix("# ") {
            let (name, value) = header.split_once(' ').unwrap_or((header, ""));
            match name {
                "branch.oid" if value != "(initial)" => state.head = Some(value.to_owned()),
                "branch.head" if value != "(detached)" => state.branch = Some(value.to_owned()),
                "branch.ab" => {
                    let mut counts = value
                        .split(' ')
                        .map(|c| c.trim_start_matches(['+', '-']).parse::<u32>().ok());
                    state.ahead = counts.next().flatten();
                    state.behind = counts.next().flatten();
                }
                _ => {}
            }
        } else if !line.is_empty() && !line.starts_with('!') {
            state.changed_files += 1;
        }
    }
    state
}

// ---- Merge -----------------------------------------------------------------------------

fn merge(
    remote: Vec<(RemoteRepo, RepoConfig)>,
    locals: Vec<(LocalProject, RepoConfig, Option<LocalState>)>,
) -> Vec<Project> {
    let mut by_id: BTreeMap<String, Project> = remote
        .into_iter()
        .map(|(repo, config)| {
            let id = key(&format!("{}/{}", repo.owner, repo.name));
            let project = Project {
                id: id.clone(),
                owner: Some(repo.owner.clone()),
                repo: Some(repo.name.clone()),
                config,
                path: None,
                local: None,
                remote: Some(repo),
                classroom: None,
                assignment: None,
                pending: None,
                web_url: None,
                late_seconds: None,
            };
            (id, project)
        })
        .collect();

    for (clone, config, state) in locals {
        let id = match (&clone.owner, &clone.repo) {
            (Some(owner), Some(repo)) => key(&format!("{owner}/{repo}")),
            _ => clone.path.to_string_lossy().into_owned(),
        };
        match by_id.get_mut(&id) {
            // Two clones of one repo: the one worked in last is shown.
            Some(project)
                if project
                    .path
                    .as_deref()
                    .is_some_and(|shown| last_activity(shown) >= last_activity(&clone.path)) => {}
            Some(project) => {
                project.path = Some(clone.path);
                project.local = state;
            }
            None => {
                by_id.insert(
                    id.clone(),
                    Project {
                        id,
                        owner: clone.owner,
                        repo: clone.repo,
                        config,
                        path: Some(clone.path),
                        local: state,
                        remote: None,
                        classroom: None,
                        assignment: None,
                        pending: None,
                        web_url: None,
                        late_seconds: None,
                    },
                );
            }
        }
    }
    by_id.into_values().collect()
}

fn late_seconds(project: &Project) -> Option<i64> {
    let assignment = project.assignment.as_ref()?;
    let due = assignment.due?.with_timezone(&Utc);
    let remote = project.remote.as_ref()?;
    let graded_at = if assignment.submission_mode == Some(SubmissionMode::Tag) {
        remote.last_submission.as_ref()?.at?
    } else {
        remote.head.as_ref()?.at?
    };
    let late = (graded_at - due).num_seconds();
    (late > 0).then_some(late)
}

/// When the clone's index last changed (any stage, commit, checkout or pull).
fn last_activity(path: &Path) -> Option<std::time::SystemTime> {
    std::fs::metadata(path.join(".git").join("index"))
        .and_then(|meta| meta.modified())
        .ok()
}

fn pending(project: &Project) -> Option<Pending> {
    if let Some(local) = &project.local {
        let incoming = local.remote_ahead || local.behind.is_some_and(|n| n > 0);
        let outgoing = local.changed_files > 0 || local.ahead.is_some_and(|n| n > 0);
        if incoming && outgoing {
            return Some(Pending::Conflict);
        }
        if local.changed_files > 0 {
            return Some(Pending::Uncommitted);
        }
        if local.ahead.is_some_and(|n| n > 0) {
            return Some(Pending::Unpushed);
        }
        if !local.unmerged.is_empty() {
            return Some(Pending::Unmerged);
        }
    }
    if project
        .remote
        .as_ref()
        .is_some_and(|r| !r.unmerged.is_empty())
    {
        return Some(Pending::Unmerged);
    }
    let tag_mode = project
        .assignment
        .as_ref()
        .is_some_and(|a| a.submission_mode == Some(SubmissionMode::Tag) && !a.closed);
    let remote = project.remote.as_ref()?;
    let head = remote.head.as_ref()?;
    let submitted = remote
        .last_submission
        .as_ref()
        .is_some_and(|s| s.commit == head.oid);
    (tag_mode && !submitted).then_some(Pending::Unsubmitted)
}

/// Open assignments by nearest due date, then those without one, then past or closed
/// ones (most recent first).
fn sort_key(project: &Project, now: DateTime<Utc>) -> (u8, i64, String) {
    let name = project
        .assignment
        .as_ref()
        .map_or(&project.config.assignment, |a| &a.name)
        .to_lowercase();
    let assignment = project.assignment.as_ref();
    let closed = assignment.is_some_and(|a| a.closed)
        || project.remote.as_ref().is_some_and(|r| r.archived)
        || project.classroom.as_ref().is_some_and(|c| !c.active);
    match assignment
        .and_then(|a| a.due)
        .map(|d| d.with_timezone(&Utc))
    {
        _ if closed => (2, 0, name),
        Some(due) if due >= now => (0, due.timestamp(), name),
        Some(due) => (2, -due.timestamp(), name),
        None => (1, 0, name),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn config(assignment: &str) -> RepoConfig {
        RepoConfig {
            classroom: "fall".into(),
            assignment: assignment.into(),
            secret: None,
        }
    }

    fn remote(name: &str, head: &str, submitted: Option<&str>) -> RemoteRepo {
        RemoteRepo {
            owner: "CS50".into(),
            name: name.into(),
            url: format!("https://github.com/CS50/{name}"),
            archived: false,
            can_push: true,
            default_branch: Some("main".into()),
            head: Some(Commit {
                oid: head.into(),
                at: None,
                author: None,
            }),
            last_submission: submitted.map(|commit| Submission {
                tag: "submit/x".into(),
                commit: commit.into(),
                at: None,
            }),
            config: Some(config("hello")),
            unmerged: Vec::new(),
            grade: None,
            grading: None,
            feedback: None,
        }
    }

    #[test]
    fn parses_porcelain_status() {
        let out = "# branch.oid 5f14f16\n# branch.head main\n# branch.upstream origin/main\n# branch.ab +2 -1\n1 .M N... 100644 100644 100644 a b src/a.c\n? notes.txt\n! build/";
        let state = parse_status(out);
        assert_eq!(state.head.as_deref(), Some("5f14f16"));
        assert_eq!(state.branch.as_deref(), Some("main"));
        assert_eq!((state.ahead, state.behind), (Some(2), Some(1)));
        assert_eq!(state.changed_files, 2);

        let fresh = parse_status("# branch.oid (initial)\n# branch.head (detached)");
        assert_eq!(fresh, LocalState::default());
    }

    #[test]
    fn parses_submit_tag_time() {
        let at = parse_submit_time("2026-10-14T12-00-05Z-abc1234").unwrap();
        assert_eq!(at.to_rfc3339(), "2026-10-14T12:00:05+00:00");
        assert_eq!(parse_submit_time("my-own-tag"), None);
    }

    #[test]
    fn reads_repo_nodes() {
        let json = r#"{
            "name": "fall-hello-alice", "owner": {"login": "CS50"},
            "url": "https://github.com/CS50/fall-hello-alice", "isArchived": false,
            "viewerPermission": "WRITE",
            "defaultBranchRef": {"name": "main", "target": {"oid": "aaa", "committedDate": "2026-10-07T20:50:13Z"}},
            "config": {"text": "classroom: fall\nassignment: hello\n"},
            "submissions": {"nodes": [{"name": "2026-10-14T12-00-05Z-bbbbbbb",
                                       "target": {"oid": "tagobj", "target": {"oid": "bbb"}}}]}
        }"#;
        let repo = serde_json::from_str::<RepoNode>(json)
            .unwrap()
            .into_remote();
        assert_eq!(repo.config.unwrap().assignment, "hello");
        assert!(repo.can_push);
        assert_eq!(repo.head.unwrap().oid, "aaa");
        let submission = repo.last_submission.unwrap();
        assert_eq!(submission.commit, "bbb");
        assert_eq!(submission.tag, "submit/2026-10-14T12-00-05Z-bbbbbbb");

        let plain: RepoNode = serde_json::from_str(
            r#"{"name": "dotfiles", "owner": {"login": "alice"}, "url": "u", "isArchived": false,
                "viewerPermission": "ADMIN", "defaultBranchRef": null, "config": null, "submissions": {"nodes": []}}"#,
        )
        .unwrap();
        assert!(plain.into_remote().config.is_none());
    }

    #[test]
    fn merges_clones_into_remote_repos() {
        let clone = LocalProject {
            path: "/code/hello".into(),
            owner: Some("cs50".into()),
            repo: Some("Fall-Hello-Alice".into()),
            config: Some(config("hello")),
        };
        let orphan = LocalProject {
            path: "/code/scratch".into(),
            owner: None,
            repo: None,
            config: Some(config("scratch")),
        };
        let projects = merge(
            vec![(remote("fall-hello-alice", "aaa", None), config("hello"))],
            vec![
                (clone, config("hello"), Some(LocalState::default())),
                (orphan, config("scratch"), None),
            ],
        );
        assert_eq!(projects.len(), 2);
        let hello = projects
            .iter()
            .find(|p| p.id == "cs50/fall-hello-alice")
            .unwrap();
        assert_eq!(hello.path.as_deref(), Some(Path::new("/code/hello")));
        assert!(hello.remote.is_some() && hello.local.is_some());
        assert!(
            projects
                .iter()
                .any(|p| p.id == "/code/scratch" && p.remote.is_none())
        );
    }

    #[test]
    fn identifies_repos_by_marker_or_name() {
        let poo = Classroom::from_team("HEIGVD-POO", "classroom50-26-poo-c", None).unwrap();
        let classrooms = BTreeMap::from([(poo.key(), poo)]);
        let manifest: Vec<Assignment> = serde_json::from_value(json!([
            {"slug": "lab02", "name": "Labo 2", "mode": "team"},
            {"slug": "labo-3-hogwarts", "name": "Labo 3", "mode": "team"}
        ]))
        .unwrap();
        let manifests =
            HashMap::from([(("heigvd-poo".to_owned(), "26-poo-c".to_owned()), manifest)]);
        let find = |owner, name, config| identify(owner, name, config, &classrooms, &manifests);

        let lab3 = find(
            Some("HEIGVD-POO"),
            Some("26-poo-c-labo-3-hogwarts-group-5"),
            None,
        )
        .unwrap();
        assert_eq!(
            (lab3.classroom.as_str(), lab3.assignment.as_str()),
            ("26-poo-c", "labo-3-hogwarts")
        );
        // A marker file wins; unrelated repos and other orgs don't match.
        let marked = config("hello");
        assert_eq!(
            find(Some("x"), Some("y"), Some(&marked)),
            Some(marked.clone())
        );
        assert_eq!(find(Some("HEIGVD-POO"), Some("hogwarts"), None), None);
        assert_eq!(
            find(Some("other-org"), Some("26-poo-c-lab02-group-1"), None),
            None
        );
    }

    #[test]
    fn computes_pending_work() {
        let assignment: Assignment = serde_json::from_value(json!({
            "slug": "hello", "name": "Hello", "mode": "individual", "submission_mode": "tag"
        }))
        .unwrap();
        let project = |remote: RemoteRepo, local: Option<LocalState>| Project {
            id: "x".into(),
            owner: None,
            repo: None,
            config: config("hello"),
            path: None,
            local,
            remote: Some(remote),
            classroom: None,
            assignment: Some(assignment.clone()),
            pending: None,
            web_url: None,
            late_seconds: None,
        };

        let unsubmitted = project(remote("r", "aaa", Some("old")), None);
        assert_eq!(pending(&unsubmitted), Some(Pending::Unsubmitted));
        let submitted = project(remote("r", "aaa", Some("aaa")), None);
        assert_eq!(pending(&submitted), None);
        let dirty = LocalState {
            changed_files: 1,
            ahead: Some(3),
            ..LocalState::default()
        };
        let edited = project(remote("r", "aaa", Some("aaa")), Some(dirty.clone()));
        assert_eq!(pending(&edited), Some(Pending::Uncommitted));
        // A teammate pushed while there is local work: pull first.
        let diverging = LocalState {
            remote_ahead: true,
            ..dirty
        };
        let risky = project(remote("r", "aaa", Some("aaa")), Some(diverging));
        assert_eq!(pending(&risky), Some(Pending::Conflict));

        // Graded at the submit tag's time in tag mode: 2 hours after the deadline.
        let mut late = project(remote("r", "aaa", Some("aaa")), None);
        late.assignment.as_mut().unwrap().due = Some("2026-10-14T10:00:00Z".parse().unwrap());
        late.remote
            .as_mut()
            .unwrap()
            .last_submission
            .as_mut()
            .unwrap()
            .at = Some("2026-10-14T12:00:00Z".parse().unwrap());
        assert_eq!(late_seconds(&late), Some(7200));
        late.remote
            .as_mut()
            .unwrap()
            .last_submission
            .as_mut()
            .unwrap()
            .at = Some("2026-10-14T09:00:00Z".parse().unwrap());
        assert_eq!(late_seconds(&late), None);
    }

    #[test]
    fn checks_more_often_as_a_deadline_nears() {
        let now = Utc::now();
        let snapshot = |hours: i64| Snapshot {
            projects: vec![Project {
                id: "x".into(),
                owner: None,
                repo: None,
                config: config("hello"),
                path: None,
                local: None,
                remote: None,
                classroom: None,
                assignment: Some(
                    serde_json::from_value(json!({
                        "slug": "hello", "name": "Hello", "mode": "individual",
                        "due": (now + chrono::TimeDelta::hours(hours)).to_rfc3339()
                    }))
                    .unwrap(),
                ),
                pending: None,
                web_url: None,
                late_seconds: None,
            }],
            ..Snapshot::default()
        };
        let status_minutes = |hours| cadence(&snapshot(hours), now).0.num_minutes();
        assert_eq!(status_minutes(2), 0); // just under a minute
        assert_eq!(status_minutes(20), 1);
        assert_eq!(status_minutes(72), 4);
        assert_eq!(status_minutes(24 * 30), 14);
        assert_eq!(
            cadence(&snapshot(24 * 30), now).1,
            chrono::TimeDelta::hours(1)
        );
    }
}
