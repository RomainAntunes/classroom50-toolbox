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
use crate::groups::{self, Group, GroupTeam};
use crate::noise;
use crate::process;
use crate::settings::{Settings, SettingsStore};
use crate::status::{self, Branch, Feedback, Grade, GradeSummary, Grading, StatusFields};
use crate::sync_progress::{Costs, Phase, Tracker};
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
/// How old the full sync may get when the popover opens on an assignment to accept.
const MIN_FULL_ON_OPEN: chrono::TimeDelta = chrono::TimeDelta::minutes(2);
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
/// Threads walking scan roots at once.
const MAX_SCAN_THREADS: usize = 4;
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
    /// `owner/repo` of the template GitHub generated it from, from the repo list; `None`
    /// when it wasn't generated from one, the template is private to the viewer, or the
    /// repo can't be the user's work (`ListedRepo::generated_from`).
    #[serde(default)]
    pub template: Option<String>,
    /// Branches with commits missing from the default branch. Like `grade` and `feedback`,
    /// read by the status query, for assignment repos only.
    #[serde(default)]
    pub unmerged: Vec<Branch>,
    #[serde(default)]
    pub grade: Option<Grade>,
    /// Every graded submission still listed (the last 10 releases), newest first.
    #[serde(default)]
    pub history: Vec<GradeSummary>,
    /// The autograder's run on the head commit, while running or when it broke.
    #[serde(default)]
    pub grading: Option<Grading>,
    #[serde(default)]
    pub feedback: Option<Feedback>,
}

/// How many changed paths `LocalState` names; `changed_files` counts them all.
pub const MAX_CHANGED_PATHS: usize = 20;

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LocalState {
    pub branch: Option<String>,
    pub head: Option<String>,
    /// `None` when the branch has no upstream.
    pub ahead: Option<u32>,
    pub behind: Option<u32>,
    pub changed_files: u32,
    /// The first `MAX_CHANGED_PATHS` of them, in git's order: a folder git collapsed (all
    /// new) once, with its trailing `/`.
    #[serde(default)]
    pub changed_paths: Vec<String>,
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
    /// The editor that last opened the clone (`editors::Editor::id`), when one did.
    #[serde(default)]
    pub editor: Option<String>,
    /// The user's group, on a team assignment.
    #[serde(default)]
    pub group: Option<Group>,
}

/// Why the last sync was partial, as `{ kind, message, params }` like command errors.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SyncIssue {
    pub kind: String,
    pub message: String,
    #[serde(default, skip_serializing_if = "std::collections::BTreeMap::is_empty")]
    pub params: std::collections::BTreeMap<String, String>,
    /// Kept across restarts, so a rate-limit backoff survives one.
    #[serde(default)]
    retry_after: Option<DateTime<Utc>>,
}

impl SyncIssue {
    /// When GitHub said to try again, on a rate limit.
    pub fn retry_after(&self) -> Option<DateTime<Utc>> {
        self.retry_after
    }
}

impl From<&Error> for SyncIssue {
    fn from(err: &Error) -> Self {
        Self {
            kind: err.kind().to_owned(),
            message: err.to_string(),
            params: err
                .params()
                .into_iter()
                .map(|(name, value)| (name.to_owned(), value))
                .collect(),
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
    /// The group the user is already on (a teacher-formed group, or one they joined).
    #[serde(default)]
    pub group: Option<Group>,
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
    /// The user's group teams with their members.
    #[serde(default)]
    groups: Vec<Group>,
    /// The last sync stopped before reading every listed repo, so `repos` may lack some.
    #[serde(default)]
    incomplete: bool,
}

impl RemoteCache {
    /// Holds the whole repo list of an account, so a repo missing from it doesn't exist.
    fn complete(&self) -> bool {
        self.login.is_some() && !self.incomplete
    }
}

/// How far the sync in flight is (see `sync_progress`).
#[derive(Debug, Clone, Copy, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SyncProgress {
    pub running: bool,
    /// Started by the user (refresh button, rescan, sign-in). Background checks run
    /// without a spinner.
    pub foreground: bool,
    /// From 0 to 1, never lower than the last report of the same sync.
    pub fraction: f32,
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
    /// What each phase cost in the last sync of each mode, to size the progress bar.
    #[serde(default)]
    pub phases: BTreeMap<String, Costs>,
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
    /// When the last full sync started, worked or not: a failed one keeps the snapshot's
    /// `synced_at`, and this spaces out the retries.
    full_attempt: Mutex<Option<DateTime<Utc>>>,
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
            full_attempt: Mutex::default(),
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

    /// The names the last discovery knew, for the diagnostics report to mask: classroom
    /// teams, groups, and the repos of a classroom's org or holding a marker file.
    pub fn discovery_names(&self) -> crate::diagnostics::Discovery {
        let cache = self.remote();
        let orgs: HashSet<String> = cache
            .classrooms
            .iter()
            .map(|c| c.org.to_lowercase())
            .collect();
        let repos = cache
            .repos
            .values()
            .filter(|r| r.config.is_some() || orgs.contains(&r.owner.to_lowercase()))
            .map(|r| (r.owner.clone(), r.name.clone(), r.config.clone()))
            .collect();
        crate::diagnostics::Discovery {
            classrooms: cache.classrooms,
            groups: cache.groups,
            repos,
        }
    }

    /// A classroom of the user's, as the last full sync read it (name, key, Pages host).
    pub fn known_classroom(&self, org: &str, slug: &str) -> Option<Classroom> {
        self.remote()
            .classrooms
            .into_iter()
            .find(|c| c.org.eq_ignore_ascii_case(org) && c.slug.eq_ignore_ascii_case(slug))
    }

    /// One classroom's published assignments, through the same cache as the syncs.
    pub async fn manifest(
        &self,
        http: &reqwest::Client,
        classroom: &Classroom,
    ) -> Result<Vec<Assignment>> {
        let url = classroom.assignments_url();
        let body = self
            .http_cache
            .get_text_or_cached(http, &url, MANIFEST_MAX_AGE)
            .await?;
        classroom50::parse_assignments(&url, &body)
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

/// The project holding assignment `org/classroom/assignment`, slugs compared without
/// regard to case; the slug may be the one the assignment was renamed from.
pub fn assignment_project<'a>(
    snapshot: &'a Snapshot,
    org: &str,
    classroom: &str,
    assignment: &str,
) -> Option<&'a Project> {
    snapshot.projects.iter().find(|p| {
        p.owner
            .as_deref()
            .is_some_and(|o| o.eq_ignore_ascii_case(org))
            && p.config.classroom.eq_ignore_ascii_case(classroom)
            && (p.config.assignment.eq_ignore_ascii_case(assignment)
                || p.assignment
                    .as_ref()
                    .is_some_and(|a| a.answers_to(assignment)))
    })
}

/// The id `project` had before `gh teacher assignment rename` renamed its repo (same
/// suffix, the old slug): what the hidden list, `alerts.json` and calendar UIDs may still
/// hold. `None` for a repo never renamed, or one the rename skipped.
pub fn previous_id(project: &Project) -> Option<String> {
    let assignment = project.assignment.as_ref()?;
    let old = classroom50::previous_repo_name(
        project.repo.as_deref()?,
        &project.config.classroom,
        std::slice::from_ref(assignment),
    )?;
    Some(key(&format!("{}/{old}", project.owner.as_deref()?)))
}

/// The id an unaccepted assignment had before its rename (`org/classroom/<old slug>`).
pub fn previous_unaccepted_id(item: &Unaccepted) -> Option<String> {
    let old = item.assignment.old_slug()?;
    Some(unaccepted_key(
        &item.classroom.org,
        &item.classroom.slug,
        old,
    ))
}

/// `org/classroom/assignment`, lowercase: an unaccepted assignment's id, and how
/// `alerts.json` keys due dates and closings.
pub fn unaccepted_key(org: &str, classroom: &str, assignment: &str) -> String {
    format!("{org}/{classroom}/{assignment}").to_ascii_lowercase()
}

/// What renames changed among the ids of `snapshot`, as `(old, new)`: projects (`owner/repo`)
/// and assignments (`org/classroom/slug`), for the hidden list and `alerts.json` to follow.
/// An old id still in use (a repo the rename skipped) is left alone.
pub fn renamed_ids(snapshot: &Snapshot) -> Vec<(String, String)> {
    let in_use: HashSet<&str> = snapshot
        .projects
        .iter()
        .map(|p| p.id.as_str())
        .chain(snapshot.unaccepted.iter().map(|u| u.id.as_str()))
        .collect();
    let projects = snapshot.projects.iter().flat_map(|p| {
        let assignment = p.assignment.as_ref();
        let slug_pair = p.owner.as_deref().zip(assignment).and_then(|(owner, a)| {
            let old = a.old_slug()?;
            Some((
                unaccepted_key(owner, &p.config.classroom, old),
                unaccepted_key(owner, &p.config.classroom, &a.slug),
            ))
        });
        let id_pair = previous_id(p).map(|old| (old, p.id.clone()));
        id_pair.into_iter().chain(slug_pair)
    });
    let unaccepted = snapshot
        .unaccepted
        .iter()
        .filter_map(|u| Some((previous_unaccepted_id(u)?, u.id.clone())));
    let mut pairs: Vec<(String, String)> = projects
        .chain(unaccepted)
        .filter(|(old, new)| old != new && !in_use.contains(old.as_str()))
        .collect();
    pairs.sort();
    pairs.dedup();
    pairs
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

/// Runs a full sync of its own, after any sync in flight: for a change the app just made on
/// GitHub (accept, adding a teammate), which a sync started earlier may not have seen.
pub async fn refresh_after_write(app: &AppHandle) -> Snapshot {
    let state = app.state::<ProjectsState>();
    let guard = state.sync_lock.lock().await;
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

impl Mode {
    fn key(self) -> &'static str {
        match self {
            Mode::Full => "full",
            Mode::Status => "status",
            Mode::Local => "local",
        }
    }

    fn phases(self) -> &'static [Phase] {
        use Phase::*;
        match self {
            Mode::Full => &[Listing, Details, Scan, Manifests, Statuses, Clones],
            Mode::Status => &[Scan, Manifests, Statuses, Clones],
            Mode::Local => &[Scan, Manifests, Clones],
        }
    }
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

    let progress = Progress::start(app, mode, foreground);
    let started = std::time::Instant::now();
    let clones = app.state::<CloneStore>().list();
    let (snapshot, targets) = sync(&state, http, &settings, clones, mode, &progress).await;
    progress.finish(mode);
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
    crate::calendar::sync(app);
    snapshot
}

/// Reports sync progress to the UI as `sync-progress`.
#[derive(Clone)]
struct Progress {
    app: AppHandle,
    tracker: std::sync::Arc<Mutex<Tracker>>,
    foreground: bool,
    /// Reports nothing: for the preview built while the real sync goes on.
    quiet: bool,
}

impl Progress {
    fn start(app: &AppHandle, mode: Mode, foreground: bool) -> Self {
        let last = app
            .state::<ProjectsState>()
            .stats()
            .phases
            .remove(mode.key())
            .unwrap_or_default();
        let progress = Self {
            app: app.clone(),
            tracker: std::sync::Arc::new(Mutex::new(Tracker::new(mode.phases(), &last))),
            foreground,
            quiet: false,
        };
        progress.emit(|p| {
            *p = SyncProgress {
                running: true,
                foreground,
                fraction: 0.0,
            }
        });
        progress
    }

    fn quiet(app: &AppHandle) -> Self {
        Self {
            app: app.clone(),
            tracker: std::sync::Arc::new(Mutex::new(Tracker::new(&[], &Costs::new()))),
            foreground: false,
            quiet: true,
        }
    }

    fn set_total(&self, phase: Phase, total: usize) {
        self.track(|t| t.set_total(phase, total));
    }

    fn advance(&self, phase: Phase) {
        self.track(|t| t.advance(phase));
    }

    fn finish_phase(&self, phase: Phase) {
        self.track(|t| t.finish(phase));
    }

    fn skip(&self, phase: Phase) {
        self.track(|t| t.skip(phase));
    }

    /// Ends the run and keeps what each phase cost for the next one of this mode.
    fn finish(&self, mode: Mode) {
        let state = self.app.state::<ProjectsState>();
        if let (Ok(tracker), Ok(mut stats)) = (self.tracker.lock(), state.stats.lock()) {
            let last = stats.phases.get(mode.key()).cloned().unwrap_or_default();
            stats
                .phases
                .insert(mode.key().to_owned(), tracker.costs(&last));
        }
        self.emit(|p| {
            p.running = false;
            p.fraction = 1.0;
        });
    }

    fn track(&self, change: impl FnOnce(&mut Tracker)) {
        if self.quiet {
            return;
        }
        let Ok(fraction) = self.tracker.lock().map(|mut tracker| {
            change(&mut tracker);
            tracker.fraction()
        }) else {
            return;
        };
        self.emit(|p| p.fraction = fraction);
    }

    fn emit(&self, change: impl FnOnce(&mut SyncProgress)) {
        let state = self.app.state::<ProjectsState>();
        let Ok(mut progress) = state.progress.lock() else {
            return;
        };
        change(&mut progress);
        if let Err(err) = self.app.emit(PROGRESS_EVENT, *progress) {
            log::debug!("could not emit {PROGRESS_EVENT}: {err}");
        }
    }
}

/// Starts whatever is due in the background: a full sync when the last one is older than
/// `FULL_SYNC_AGE`, else a status check when the last GitHub read is older than
/// `min_status_age`. Nothing while GitHub asks to back off.
fn refresh_due(app: &AppHandle, min_status_age: chrono::TimeDelta, on_open: bool) {
    if !may_call_github(app) {
        return;
    }
    let state = app.state::<ProjectsState>();
    let snapshot = state.snapshot();
    let full_attempt = state.full_attempt.lock().ok().and_then(|at| *at);
    let now = Utc::now();
    let app = app.clone();
    let (_, mut full_age) = cadence(&snapshot, now);
    // Only a full sync finds a repo accepted elsewhere (the web page, the CLI), so while an
    // assignment waits for it, opening the popover looks again.
    if on_open && awaiting_accept(&snapshot, &app.state::<SettingsStore>().get(), now) {
        full_age = full_age.min(MIN_FULL_ON_OPEN);
    }
    let Some(mode) = due_mode(&snapshot, full_attempt, now, full_age, min_status_age) else {
        return;
    };
    tauri::async_runtime::spawn(async move {
        refresh_in_background(&app, mode).await;
    });
}

/// What is due: a full sync once the last one that worked is `full_age` old, retried after a
/// failure no more often than status checks run; else a status check once the last GitHub
/// read is `min_status_age` old.
fn due_mode(
    snapshot: &Snapshot,
    full_attempt: Option<DateTime<Utc>>,
    now: DateTime<Utc>,
    full_age: chrono::TimeDelta,
    min_status_age: chrono::TimeDelta,
) -> Option<Mode> {
    let older = |at: Option<DateTime<Utc>>, age| at.is_none_or(|at| now - at >= age);
    if older(snapshot.synced_at, full_age) && older(full_attempt, min_status_age) {
        Some(Mode::Full)
    } else if older(snapshot.checked_at, min_status_age) {
        Some(Mode::Status)
    } else {
        None
    }
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
    refresh_due(app, MIN_STATUS_ON_OPEN, true);
}

/// Whether the list offers an assignment the user could accept right now (not hidden,
/// locked or upcoming).
fn awaiting_accept(snapshot: &Snapshot, settings: &Settings, now: DateTime<Utc>) -> bool {
    snapshot.unaccepted.iter().any(|item| {
        let class = format!("{}/{}", item.classroom.org, item.classroom.slug).to_ascii_lowercase();
        !item.assignment.locked
            && item.assignment.available_from.is_none_or(|at| at <= now)
            && !settings.hidden.contains(&item.id)
            && !settings.hidden_classes.contains(&class)
    })
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
                    refresh_due(&app, chrono::TimeDelta::zero(), false);
                }
            } else if slept {
                log::info!("woke from sleep; syncing");
                // Give Wi-Fi a moment to reconnect.
                tokio::time::sleep(Duration::from_secs(5)).await;
                refresh_due(&app, chrono::TimeDelta::zero(), false);
            } else {
                let (status_age, _) = cadence(&state.snapshot(), Utc::now());
                refresh_due(&app, status_age, false);
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
    if !crate::simulate::reachable() {
        return false;
    }
    let (host, port) =
        crate::testenv::probe_addr().unwrap_or_else(|| ("api.github.com".to_owned(), 443));
    let connect = tokio::net::TcpStream::connect((host.as_str(), port));
    matches!(
        tokio::time::timeout(Duration::from_secs(3), connect).await,
        Ok(Ok(_))
    )
}

/// Returns the snapshot and what to watch for local changes. A sync the user waits on
/// first shows the clones as found on disk with the last GitHub data, so the list doesn't
/// wait on GitHub (or on a connection that times out).
async fn sync(
    state: &ProjectsState,
    http: &reqwest::Client,
    settings: &Settings,
    clones: Vec<PathBuf>,
    mode: Mode,
    progress: &Progress,
) -> (Snapshot, Vec<WatchTarget>) {
    let now = Utc::now();
    let preview = mode != Mode::Local && (progress.foreground || state.is_offline());
    let scan = async {
        let scanned = scan_local(
            settings.watched_folders.clone(),
            clones,
            settings.scan_editor_projects,
            progress.clone(),
        )
        .await;
        if preview {
            let (locals, _, recents) = &scanned;
            let quiet = Progress::quiet(&progress.app);
            let mut snapshot =
                local_snapshot(state, http, locals.clone(), now, false, false, &quiet).await;
            annotate(state, &mut snapshot, recents);
            if let Err(err) = progress.app.emit(UPDATED_EVENT, &snapshot) {
                log::warn!("could not emit {UPDATED_EVENT}: {err}");
            }
        }
        scanned
    };
    // Offline, the manifests come from the cache rather than a request that times out.
    let network = !state.is_offline();
    let (snapshot, scan_targets, recents) = if mode != Mode::Full {
        let (locals, targets, recents) = scan.await;
        let status = mode == Mode::Status;
        (
            local_snapshot(state, http, locals, now, status, network, progress).await,
            targets,
            recents,
        )
    } else {
        let (remote, (locals, targets, recents)) =
            tokio::join!(sync_remote(state, http, progress), scan);
        (
            full_snapshot(state, http, remote, locals, now, progress).await,
            targets,
            recents,
        )
    };
    let mut snapshot = snapshot;
    annotate(state, &mut snapshot, &recents);
    commit_emails(&mut snapshot.projects).await;
    let targets = watch_targets(&settings.watched_folders, scan_targets, &snapshot.projects);
    (snapshot, targets)
}

/// Teammates without a public email on GitHub get the one they commit with in the shared
/// clone (read from the local history, no request). GitHub's `noreply` addresses don't count.
async fn commit_emails(projects: &mut [Project]) {
    for project in projects {
        let (Some(path), Some(group)) = (project.path.clone(), project.group.as_mut()) else {
            continue;
        };
        if group.members.iter().all(|m| m.email.is_some()) {
            continue;
        }
        let args = ["log", "--no-merges", "-n", "500", "--format=%an%x1f%ae"];
        let Ok(out) = process::run("git", args, Some(&path)).await else {
            continue;
        };
        for member in group.members.iter_mut().filter(|m| m.email.is_none()) {
            member.email = author_email(&out.stdout, member);
        }
    }
}

/// The newest commit email of `member`: an author named like their login or GitHub name, or
/// an address whose local part is their login.
fn author_email(log: &str, member: &groups::Member) -> Option<String> {
    log.lines()
        .filter_map(|line| line.split_once('\x1f'))
        .find(|(name, email)| {
            let name = name.trim();
            let local = email.split('@').next().unwrap_or("");
            email.contains('@')
                && !email.ends_with("@users.noreply.github.com")
                && (name.eq_ignore_ascii_case(&member.login)
                    || member
                        .name
                        .as_deref()
                        .is_some_and(|n| n.trim().eq_ignore_ascii_case(name))
                    || local.eq_ignore_ascii_case(&member.login))
        })
        .map(|(_, email)| email.trim().to_owned())
}

/// Adds what isn't decided by the build: groups, the editor that opened each clone, the
/// account.
fn annotate(state: &ProjectsState, snapshot: &mut Snapshot, recents: &[editors::Recent]) {
    let groups = state.remote().groups;
    // A rename leaves the group teams as they were, named after the old slug.
    let find = |org: &str, classroom: &str, slug: &str, old: Option<&str>| {
        groups::find(&groups, org, classroom, slug)
            .or_else(|| groups::find(&groups, org, classroom, old?))
            .cloned()
    };
    for item in &mut snapshot.unaccepted {
        item.group = find(
            &item.classroom.org,
            &item.classroom.slug,
            &item.assignment.slug,
            item.assignment.old_slug(),
        );
    }
    for project in &mut snapshot.projects {
        project.group = project.owner.as_deref().and_then(|owner| {
            find(
                owner,
                &project.config.classroom,
                &project.config.assignment,
                project.assignment.as_ref().and_then(|a| a.old_slug()),
            )
        });
        project.editor = project
            .path
            .as_deref()
            .and_then(|path| editors::editor_for(path, recents))
            .map(|editor| editor.id.to_owned());
    }
    snapshot.login = state.remote().login;
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

/// Whether `previous` holds the account's GitHub repos: a full sync has run, and the last
/// one wasn't cut short by a missing sign-in or gh (it then lists clones only).
fn discovered(previous: &Snapshot) -> bool {
    previous.synced_at.is_some()
        && !previous
            .issue
            .as_ref()
            .is_some_and(|issue| matches!(issue.kind.as_str(), "notSignedIn" | "notInstalled"))
}

/// The GitHub repos a build without discovery starts from: every repo of the last one, not
/// only those the last build recognized. A build without a class manifest (offline, a
/// failed download) drops the repos recognized by name only, which then come back as soon
/// as the manifest is read again.
fn last_discovery(previous: &Snapshot, cache: &RemoteCache) -> Vec<RemoteRepo> {
    if discovered(previous) {
        cache.repos.values().cloned().collect()
    } else {
        previous
            .projects
            .iter()
            .filter_map(|p| p.remote.clone())
            .collect()
    }
}

async fn local_snapshot(
    state: &ProjectsState,
    http: &reqwest::Client,
    locals: Vec<LocalProject>,
    now: DateTime<Utc>,
    status: bool,
    network: bool,
    progress: &Progress,
) -> Snapshot {
    let previous = state.snapshot();
    let cache = state.remote();
    let remote_repos = last_discovery(&previous, &cache);
    let discovered = discovered(&previous) && cache.complete();
    let classrooms = cache.classrooms;
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
        discovered,
    };
    let (projects, unaccepted) =
        build(sources, state, http, auth.as_ref(), network, now, progress).await;
    // A status read replaces the issue: cleared when it worked, its error when it didn't.
    // Taken only by a run that read the status, never by a preview running alongside one.
    let issue = auth.is_some().then(|| state.take_status_issue());
    let (checked_at, issue) = match issue {
        Some(None) => (checked_at, None),
        Some(Some(issue)) => (previous.checked_at, Some(issue)),
        None => (checked_at, previous.issue),
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
    if remote.is_err() {
        progress.skip(Phase::Listing);
        progress.skip(Phase::Details);
    }
    if let Ok(mut at) = state.full_attempt.lock() {
        *at = Some(now);
    }
    // A failed sync doesn't count as one, so the next is due soon. The very first keeps a
    // time anyway: the list waits for one before showing.
    let previous = state.snapshot();
    let (synced_at, checked_at) = if remote.is_ok() {
        (Some(now), Some(now))
    } else {
        (previous.synced_at.or(Some(now)), previous.checked_at)
    };
    let discovered = !matches!(remote, Err(Error::NotSignedIn | Error::NotInstalled { .. }))
        && state.remote().complete();
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
        discovered,
    };
    let (projects, unaccepted) =
        build(sources, state, http, auth.as_ref(), true, now, progress).await;
    Snapshot {
        projects,
        unaccepted,
        synced_at,
        checked_at,
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
    /// Whether `remote` is the account's repo list (fresh or cached). Without it a missing
    /// repo proves nothing, so no assignment is offered for accepting.
    discovered: bool,
}

async fn build(
    sources: Sources,
    state: &ProjectsState,
    http: &reqwest::Client,
    status: Option<&StatusAuth>,
    network: bool,
    now: DateTime<Utc>,
    progress: &Progress,
) -> (Vec<Project>, Vec<Unaccepted>) {
    let Sources {
        remote,
        classrooms: team_classrooms,
        locals,
        discovered,
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
    let manifests = fetch_manifests(&classrooms, &state.http_cache, http, network, progress).await;
    state.http_cache.save();

    let remote = remote
        .into_iter()
        .filter_map(|repo| {
            let found = Found {
                owner: Some(&repo.owner),
                name: Some(&repo.name),
                config: repo.config.as_ref(),
                template: repo.template.as_deref(),
                can_push: repo.can_push,
            };
            let (config, by) = identify_by(found, &classrooms, &manifests)?;
            Some((repo, config, by))
        })
        .collect();
    let remote = without_doubtful_templates(remote);
    let remote = match status {
        Some(auth) => {
            let remote = refresh_statuses(state, http, auth, remote, progress).await;
            progress.finish_phase(Phase::Statuses);
            remote
        }
        None => {
            progress.skip(Phase::Statuses);
            remote
        }
    };
    // A clone made before its assignment was renamed joins the renamed repo's row.
    let listed: HashMap<String, String> = remote
        .iter()
        .map(|(repo, _)| {
            (
                key(&format!("{}/{}", repo.owner, repo.name)),
                repo.name.clone(),
            )
        })
        .collect();
    // A clone without a marker whose repo GitHub knew by its template.
    let identified: HashMap<String, &RepoConfig> = remote
        .iter()
        .map(|(repo, config)| (key(&format!("{}/{}", repo.owner, repo.name)), config))
        .collect();
    let locals: Vec<(LocalProject, RepoConfig)> = locals
        .into_iter()
        .map(|mut clone| {
            if let (Some(owner), Some(name)) = (&clone.owner, &clone.repo)
                && let Some(renamed) = renamed_clone(owner, name, &classrooms, &manifests, &listed)
            {
                clone.repo = Some(renamed);
            }
            clone
        })
        .filter_map(|clone| {
            let found = Found {
                owner: clone.owner.as_deref(),
                name: clone.repo.as_deref(),
                config: clone.config.as_ref(),
                template: None,
                can_push: false,
            };
            let config = identify(found, &classrooms, &manifests).or_else(|| {
                let id = key(&format!(
                    "{}/{}",
                    clone.owner.as_deref()?,
                    clone.repo.as_deref()?
                ));
                identified.get(&id).map(|config| (*config).clone())
            })?;
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
            .and_then(|list| {
                list.iter()
                    .find(|a| a.slug.eq_ignore_ascii_case(&project.config.assignment))
            })
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
    progress.finish_phase(Phase::Clones);
    for project in &mut projects {
        project.pending = pending(project);
        project.late_seconds = late_seconds(project);
    }
    projects.sort_by_cached_key(|p| sort_key(p, now));
    let unaccepted = if discovered {
        unaccepted(&projects, &classrooms, &manifests)
    } else {
        Vec::new()
    };
    (projects, unaccepted)
}

/// Open assignments of the user's classrooms with no repo yet, published or upcoming, by
/// due date. An assignment counts as accepted when a repo of the classroom's org names it
/// in its marker file or by the naming rule, slugs compared without regard to case (a marker
/// written by hand or by an older CLI may differ from the manifest in case, or name the
/// assignment differently from the repo).
fn unaccepted(
    projects: &[Project],
    classrooms: &BTreeMap<ClassroomKey, Classroom>,
    manifests: &HashMap<ClassroomKey, Vec<Assignment>>,
) -> Vec<Unaccepted> {
    let mut accepted: HashSet<(ClassroomKey, String)> = HashSet::new();
    for p in projects {
        let Some(owner) = p.owner.as_deref().map(str::to_ascii_lowercase) else {
            continue;
        };
        accepted.insert((
            (owner.clone(), p.config.classroom.to_ascii_lowercase()),
            p.config.assignment.to_ascii_lowercase(),
        ));
        let Some(name) = p.repo.as_deref() else {
            continue;
        };
        for (key, classroom) in classrooms.iter().filter(|((org, _), _)| *org == owner) {
            let Some(list) = manifests.get(key) else {
                continue;
            };
            if let Some(found) = classroom50::assignment_for_repo(name, &classroom.slug, list) {
                accepted.insert((key.clone(), found.assignment.slug.to_ascii_lowercase()));
            }
        }
    }
    let mut list: Vec<Unaccepted> = manifests
        .iter()
        .filter_map(|(key, assignments)| Some((classrooms.get(key)?, key, assignments)))
        .filter(|(classroom, _, _)| classroom.active)
        .flat_map(|(classroom, key, assignments)| {
            assignments
                .iter()
                // Upcoming ones (`available_from` ahead) are listed too, as "Opens ...".
                .filter(|a| !a.closed)
                .filter(|a| !accepted.contains(&(key.clone(), a.slug.to_ascii_lowercase())))
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
                    group: None,
                })
        })
        .collect();
    list.sort_by_key(|u| (u.assignment.due.is_none(), u.assignment.due));
    list
}

/// What `identify` knows of a repo: from GitHub, from a clone, or both.
#[derive(Clone, Copy, Default)]
struct Found<'a> {
    owner: Option<&'a str>,
    name: Option<&'a str>,
    config: Option<&'a RepoConfig>,
    /// `owner/repo` of the template GitHub generated it from (`RemoteRepo::template`).
    template: Option<&'a str>,
    /// The user may push to it (`RemoteRepo::can_push`): a template match needs it unless
    /// the entry's submissions are closed, which makes every student repo read-only.
    can_push: bool,
}

/// How `identify_by` told a repo's assignment.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum MatchedBy {
    Marker,
    Name,
    Template,
}

/// The assignment identity of a repo: its marker file, else its name matched against the
/// manifests of the owner's classrooms, else the template it was generated from
/// (`by_template`). `None` means it isn't an assignment repo. A marker naming the slug an
/// assignment was renamed from (a repo the rename skipped, a clone not pulled since) counts
/// as that assignment, under its current slug.
fn identify(
    found: Found<'_>,
    classrooms: &BTreeMap<ClassroomKey, Classroom>,
    manifests: &HashMap<ClassroomKey, Vec<Assignment>>,
) -> Option<RepoConfig> {
    identify_by(found, classrooms, manifests).map(|(config, _)| config)
}

/// `identify`, saying how the assignment was told.
fn identify_by(
    found: Found<'_>,
    classrooms: &BTreeMap<ClassroomKey, Classroom>,
    manifests: &HashMap<ClassroomKey, Vec<Assignment>>,
) -> Option<(RepoConfig, MatchedBy)> {
    let Found {
        owner,
        name,
        config,
        template,
        can_push,
    } = found;
    if let Some(config) = config {
        let mut config = config.clone();
        let current = owner
            .and_then(|owner| {
                let key = (
                    owner.to_ascii_lowercase(),
                    config.classroom.to_ascii_lowercase(),
                );
                manifests.get(&key)
            })
            .and_then(|list| classroom50::assignment_for_slug(&config.assignment, list));
        if let Some(assignment) = current {
            config.assignment = assignment.slug.clone();
        }
        return Some((config, MatchedBy::Marker));
    }
    let (owner, name) = (owner?.to_ascii_lowercase(), name?);
    let by_name = classrooms
        .iter()
        .filter(|((org, _), _)| *org == owner)
        .find_map(|(key, classroom)| {
            let found =
                classroom50::assignment_for_repo(name, &classroom.slug, manifests.get(key)?)?;
            Some((classroom, found.assignment, MatchedBy::Name))
        });
    let (classroom, assignment, by) = by_name.or_else(|| {
        let (classroom, assignment) =
            by_template(&owner, name, template?, can_push, classrooms, manifests)?;
        Some((classroom, assignment, MatchedBy::Template))
    })?;
    let config = RepoConfig {
        classroom: classroom.slug.clone(),
        assignment: assignment.slug.clone(),
        secret: classroom.secret.clone(),
    };
    Some((config, by))
}

/// Drops the repos told by their template alone (`MatchedBy::Template`) whose assignment
/// already has a repo told by its marker or name, or another one told by its template: a
/// scratch copy generated from the same template is no second submission, and two of them
/// can't be told apart. Each assignment keeps one row, one calendar event, one reminder.
fn without_doubtful_templates(
    repos: Vec<(RemoteRepo, RepoConfig, MatchedBy)>,
) -> Vec<(RemoteRepo, RepoConfig)> {
    let assignment = |repo: &RemoteRepo, config: &RepoConfig| {
        (
            repo.owner.to_ascii_lowercase(),
            config.classroom.to_ascii_lowercase(),
            config.assignment.to_ascii_lowercase(),
        )
    };
    let mut told: HashSet<(String, String, String)> = HashSet::new();
    let mut by_template: HashMap<(String, String, String), usize> = HashMap::new();
    for (repo, config, by) in &repos {
        match by {
            MatchedBy::Template => *by_template.entry(assignment(repo, config)).or_default() += 1,
            MatchedBy::Marker | MatchedBy::Name => {
                told.insert(assignment(repo, config));
            }
        }
    }
    repos
        .into_iter()
        .filter(|(repo, config, by)| {
            let key = assignment(repo, config);
            *by != MatchedBy::Template || (!told.contains(&key) && by_template[&key] == 1)
        })
        .map(|(repo, config, _)| (repo, config))
        .collect()
}

/// The assignment a repo without a marker or a matching name was generated for: the one
/// entry, among the manifests of the user's classrooms in the repo's org, whose `template`
/// is the repo's (a teacher renamed the repo by hand, say). Several entries sharing it (an
/// assignment reused, one template in two classes) are told apart by the name: the
/// classroom's `<classroom>-` prefix, then the slug inside it. Still several, or none:
/// `None`. Never the template repo itself, nor a repo outside the classroom's org (`owner`,
/// lowercase), nor one named after another of the org's classrooms (`<other>-...`, the
/// longest prefix deciding): those are no student's work for that entry. `empty_repo`
/// assignments have no template. Nor a repo the user can't push to (`can_push`), unless the
/// entry is closed (Close submission sets every student repo read-only). The repo list only
/// passes a template on for repos that aren't templates or archived
/// (`ListedRepo::generated_from`).
fn by_template<'a>(
    owner: &str,
    name: &str,
    template: &str,
    can_push: bool,
    classrooms: &'a BTreeMap<ClassroomKey, Classroom>,
    manifests: &'a HashMap<ClassroomKey, Vec<Assignment>>,
) -> Option<(&'a Classroom, &'a Assignment)> {
    let entries: Vec<(&Classroom, &Assignment)> = classrooms
        .iter()
        .filter(|((org, _), _)| org == owner)
        .flat_map(|(key, classroom)| {
            manifests
                .get(key)
                .into_iter()
                .flatten()
                .map(move |assignment| (classroom, assignment))
        })
        .collect();
    let itself = format!("{owner}/{name}");
    if entries.iter().any(|(_, a)| {
        a.template_name()
            .is_some_and(|t| t.eq_ignore_ascii_case(&itself))
    }) {
        return None;
    }
    let lower = name.to_ascii_lowercase();
    // The classroom the name starts with, if any: `cs-2026-...` belongs to `cs-2026`,
    // not `cs`.
    let named_after = classrooms
        .values()
        .filter(|c| c.org.eq_ignore_ascii_case(owner))
        .map(|c| c.slug.to_ascii_lowercase())
        .filter(|slug| lower.starts_with(&format!("{slug}-")))
        .max_by_key(String::len);
    let mut candidates: Vec<(&Classroom, &Assignment)> = entries
        .into_iter()
        .filter(|(_, a)| {
            a.template_name()
                .is_some_and(|t| t.eq_ignore_ascii_case(template))
        })
        .filter(|(c, _)| {
            named_after
                .as_ref()
                .is_none_or(|slug| c.slug.eq_ignore_ascii_case(slug))
        })
        // Read only: a teacher's copy or a classmate's public repo, unless the teacher
        // closed submissions, which makes the student's own repo read-only too.
        .filter(|(_, a)| can_push || a.closed)
        .collect();
    /// Keeps the candidates `keep` accepts, unless none or a single one is left.
    fn narrow(
        candidates: &mut Vec<(&Classroom, &Assignment)>,
        keep: impl Fn(&Classroom, &Assignment) -> bool,
    ) {
        if candidates.len() > 1 && candidates.iter().any(|(c, a)| keep(c, a)) {
            candidates.retain(|(c, a)| keep(c, a));
        }
    }
    narrow(&mut candidates, |_, a| {
        lower.contains(&a.slug.to_ascii_lowercase())
    });
    match candidates.as_slice() {
        [one] => Some(*one),
        _ => None,
    }
}

/// The name GitHub now lists a clone's repo under, when the clone's `origin` still names
/// it as it was before `gh teacher assignment rename` (GitHub redirects the old name): the
/// old name isn't listed, the renamed one is (`listed`, lowercase `owner/repo` to GitHub's
/// spelling). `None` when the clone needs no new name.
fn renamed_clone(
    owner: &str,
    name: &str,
    classrooms: &BTreeMap<ClassroomKey, Classroom>,
    manifests: &HashMap<ClassroomKey, Vec<Assignment>>,
    listed: &HashMap<String, String>,
) -> Option<String> {
    let owner = owner.to_ascii_lowercase();
    if listed.contains_key(&format!("{owner}/{}", name.to_ascii_lowercase())) {
        return None;
    }
    classrooms
        .iter()
        .filter(|((org, _), _)| *org == owner)
        .find_map(|(key, classroom)| {
            let renamed =
                classroom50::renamed_repo_name(name, &classroom.slug, manifests.get(key)?)?;
            listed.get(&format!("{owner}/{renamed}")).cloned()
        })
}

/// Fetches each classroom's manifest once (conditional GET), or without `network` takes
/// the last downloaded copy whatever its age. A failed download falls back to that copy
/// too: a repo without a marker file is only recognized through the manifest, and would
/// otherwise drop out of the list, its assignment showing as not accepted.
async fn fetch_manifests(
    classrooms: &BTreeMap<ClassroomKey, Classroom>,
    cache: &HttpCache,
    http: &reqwest::Client,
    network: bool,
    progress: &Progress,
) -> HashMap<ClassroomKey, Vec<Assignment>> {
    progress.set_total(Phase::Manifests, classrooms.len());
    let fetches = classrooms.iter().map(|(key, classroom)| async move {
        let url = classroom.assignments_url();
        let body = if network {
            cache.get_text_or_cached(http, &url, MANIFEST_MAX_AGE).await
        } else {
            cache.cached_text(&url).ok_or(Error::Offline)
        };
        // Logged without the key of an unlisted class, which the URL (and a request
        // error quoting it) holds.
        let manifest = match body {
            Ok(body) => classroom50::parse_assignments(&url, &body)
                .inspect_err(|err| log::warn!("{}", classroom.hide_key(&err.to_string())))
                .ok(),
            Err(err) => {
                let message = format!("could not fetch {url}: {err}");
                log::warn!("{}", classroom.hide_key(&message));
                None
            }
        };
        progress.advance(Phase::Manifests);
        manifest.map(|m| (key.clone(), m))
    });
    let manifests = join_all(fetches).await.into_iter().flatten().collect();
    progress.finish_phase(Phase::Manifests);
    manifests
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
      nodes {
        nameWithOwner pushedAt isTemplate isArchived viewerPermission
        templateRepository { nameWithOwner }
      }
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
    #[serde(default)]
    is_template: bool,
    #[serde(default)]
    is_archived: bool,
    #[serde(default)]
    viewer_permission: Option<String>,
    #[serde(default)]
    template_repository: Option<NameWithOwner>,
}

impl ListedRepo {
    /// The template GitHub generated the repo from, only when the repo may be the user's
    /// own work: not a template itself (one made from the template), not archived. Whether
    /// a read-only one counts depends on the entry it matches (`by_template`).
    fn generated_from(&self) -> Option<String> {
        if self.is_template || self.is_archived {
            return None;
        }
        let template = self.template_repository.as_ref()?;
        Some(template.name_with_owner.clone())
    }

    /// The user may push to the repo, `None` when the list didn't say.
    fn can_push(&self) -> Option<bool> {
        let permission = self.viewer_permission.as_deref()?;
        Some(matches!(permission, "ADMIN" | "MAINTAIN" | "WRITE"))
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct NameWithOwner {
    name_with_owner: String,
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
            template: None,
            unmerged: Vec::new(),
            grade: None,
            history: Vec::new(),
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
    let token = tools::gh_token().await?;
    // The teams query needs the login; gh knows it locally, so both run at once.
    let (login, listed, classrooms) = match tools::active_login().await {
        Some(active) => {
            let (repos, classrooms) = tokio::join!(
                list_repos(http, &token, progress),
                list_classrooms(http, &token, &active)
            );
            let (login, listed) = repos?;
            (login, listed, classrooms)
        }
        None => {
            let (login, listed) = list_repos(http, &token, progress).await?;
            let classrooms = list_classrooms(http, &token, &login).await;
            (login, listed, classrooms)
        }
    };
    progress.finish_phase(Phase::Listing);
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
    // Group members are read last, as one more step.
    progress.set_total(Phase::Details, stale.len().div_ceil(DETAIL_BATCH) + 1);
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
            progress.advance(Phase::Details);
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
    // The template comes with the list, so every repo has it, read or not this time.
    for listed in &listed {
        if let Some(repo) = cache.repos.get_mut(&key(&listed.name_with_owner)) {
            repo.template = listed.generated_from();
            // Fresher than the details, which are only re-read after a push: closing
            // submissions changes the permission, not `pushedAt`.
            if let Some(can_push) = listed.can_push() {
                repo.can_push = can_push;
            }
        }
    }
    // Repos never read (the sync stopped first) are unknown: not proof of anything missing.
    cache.incomplete = listed_keys.iter().any(|k| !cache.checked.contains_key(k));
    match classrooms {
        Ok((classrooms, group_teams)) => {
            cache.classrooms = classrooms;
            match groups::load(http, &token, &state.http_cache, &group_teams).await {
                Ok(groups) => cache.groups = groups,
                Err(err) => log::warn!("could not read group members: {err}"),
            }
        }
        // Teams are a bonus over marker files; keep the last known list.
        Err(err) => log::warn!("could not read classroom teams: {err}"),
    }
    state.store_remote(cache);
    progress.finish_phase(Phase::Details);
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

/// The classrooms the user is enrolled in, from their `classroom50-<classroom>` teams, and
/// their group teams.
async fn list_classrooms(
    http: &reqwest::Client,
    token: &str,
    login: &str,
) -> Result<(Vec<Classroom>, Vec<GroupTeam>)> {
    let mut classrooms = Vec::new();
    let mut group_teams = Vec::new();
    let mut cursor: Option<String> = None;
    for _ in 0..MAX_LIST_PAGES {
        let variables = json!({ "login": login, "cursor": cursor });
        let data: TeamsData = github::graphql(http, token, TEAMS_QUERY, variables).await?;
        let page = data.viewer.organizations;
        for org in page.nodes.into_iter().flatten() {
            for team in org.teams.nodes.into_iter().flatten() {
                if groups::is_group_team(&team.slug) {
                    group_teams.push(GroupTeam {
                        org: org.login.clone(),
                        slug: team.slug,
                        description: team.description,
                    });
                } else if let Some(classroom) =
                    Classroom::from_team(&org.login, &team.slug, team.description.as_deref())
                {
                    classrooms.push(classroom);
                }
            }
        }
        match page.page_info.end_cursor {
            Some(next) if page.page_info.has_next_page => cursor = Some(next),
            _ => break,
        }
    }
    Ok((classrooms, group_teams))
}

async fn list_repos(
    http: &reqwest::Client,
    token: &str,
    progress: &Progress,
) -> Result<(String, Vec<ListedRepo>)> {
    let mut repos = Vec::new();
    let mut cursor: Option<String> = None;
    let mut login = String::new();
    for _ in 0..MAX_LIST_PAGES {
        let data: ListData =
            github::graphql(http, token, LIST_QUERY, json!({ "cursor": cursor })).await?;
        login = data.viewer.login;
        let page = data.viewer.repositories;
        repos.extend(page.nodes.into_iter().flatten());
        // The page count is only known at the end; the last run's stands in until then.
        progress.advance(Phase::Listing);
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
    repo.history.clone_from(&old.history);
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
    progress.set_total(Phase::Statuses, batches.len());
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
                progress.advance(Phase::Statuses);
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
        repo.template.clone_from(&old.template);
        repo.unmerged = status.unmerged(repo.default_branch.as_deref());
        repo.feedback = status.feedback(&auth.login);
        repo.grading = status.grading();
        let releases = status.graded_releases();
        repo.grade = match releases.first() {
            Some(release)
                if old
                    .grade
                    .as_ref()
                    .is_some_and(|g| g.tag == release.tag && g.has_tests()) =>
            {
                old.grade.clone()
            }
            Some(release) => match status::download_grade(http, release).await {
                Ok(grade) => Some(grade),
                Err(err) => {
                    log::warn!("{err}");
                    old.grade.clone()
                }
            },
            None => None,
        };
        if let Some(grade) = repo.grade.as_mut().filter(|g| g.needs_release_notes()) {
            match status::release_body(http, &auth.token, &repo.owner, &repo.name, &grade.tag).await
            {
                Ok(body) => grade.apply_release_body(body.as_deref()),
                Err(err) => log::warn!("release body of {}: {err}", grade.tag),
            }
        }
        repo.history = grade_history(&releases, repo.grade.as_ref(), &old, |release| {
            status::download_grade(http, release)
        })
        .await;
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

/// The summary of each graded release, newest first. A result is downloaded once per tag;
/// after that it comes from the last sync.
async fn grade_history<'a, F, Fut>(
    releases: &'a [status::GradedRelease],
    newest: Option<&Grade>,
    old: &RemoteRepo,
    mut download: F,
) -> Vec<GradeSummary>
where
    F: FnMut(&'a status::GradedRelease) -> Fut,
    Fut: std::future::Future<Output = Result<Grade>>,
{
    let mut history = Vec::with_capacity(releases.len());
    for release in releases {
        let known = newest
            .filter(|g| g.tag == release.tag)
            .map(GradeSummary::from)
            .or_else(|| old.history.iter().find(|g| g.tag == release.tag).cloned());
        let summary = match known {
            Some(summary) => summary,
            None => match download(release).await {
                Ok(grade) => GradeSummary::from(&grade),
                Err(err) => {
                    log::warn!("{err}");
                    continue;
                }
            },
        };
        history.push(summary);
    }
    history
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
) -> (Vec<LocalProject>, Vec<WatchTarget>, Vec<editors::Recent>) {
    tauri::async_runtime::spawn_blocking(move || {
        let (roots, targets, recents) = scan_roots(folders, clones, editors);
        progress.set_total(Phase::Scan, roots.len());
        if let Ok(mut stats) = progress.app.state::<ProjectsState>().stats.lock() {
            stats.scan_roots = roots.len();
        }
        let found = scan_parallel(&roots, &progress);
        progress.finish_phase(Phase::Scan);
        // In root order, so a clone found from two roots is kept once, as before.
        let mut seen = HashSet::new();
        let locals = found
            .into_iter()
            .flatten()
            .filter(|l| seen.insert(l.path.clone()))
            .collect();
        (locals, targets, recents)
    })
    .await
    .unwrap_or_default()
}

/// Walks the roots on a few threads (disk latency, not CPU, is what a walk waits on),
/// capped so a scan never takes over the machine. Results come back in root order.
fn scan_parallel(roots: &[(PathBuf, usize)], progress: &Progress) -> Vec<Vec<LocalProject>> {
    let threads = std::thread::available_parallelism()
        .map_or(1, |n| n.get())
        .clamp(1, MAX_SCAN_THREADS)
        .min(roots.len().max(1));
    let next = std::sync::atomic::AtomicUsize::new(0);
    let results: Vec<Mutex<Vec<LocalProject>>> = roots.iter().map(|_| Mutex::default()).collect();
    std::thread::scope(|scope| {
        for _ in 0..threads {
            scope.spawn(|| {
                loop {
                    let i = next.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                    let Some((root, depth)) = roots.get(i) else {
                        break;
                    };
                    let found = discovery::scan(root, *depth);
                    if let Ok(mut slot) = results[i].lock() {
                        *slot = found;
                    }
                    progress.advance(Phase::Scan);
                }
            });
        }
    });
    results
        .into_iter()
        .map(|slot| slot.into_inner().unwrap_or_default())
        .collect()
}

/// Folders to scan with their depth, the non-recursive watch targets, and the folders
/// editors opened (to tell which editor a clone belongs to).
fn scan_roots(
    folders: Vec<PathBuf>,
    clones: Vec<PathBuf>,
    editors: bool,
) -> (
    Vec<(PathBuf, usize)>,
    Vec<WatchTarget>,
    Vec<editors::Recent>,
) {
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
        return (roots, targets, Vec::new());
    }
    let found = editors::find();
    let recents: Vec<editors::Recent> = found.recent.iter().chain(&found.roots).cloned().collect();
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
    (roots, targets, recents)
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
    progress.set_total(Phase::Clones, count);
    let mut states = vec![None; count];
    while let Some(Ok((i, state))) = tasks.join_next().await {
        progress.advance(Phase::Clones);
        states[i] = state;
    }
    states
}

/// `git status` without taking the index lock, so it never races the student's own git.
/// Untracked editor and OS files (`noise`) don't count. A folder git collapsed into one
/// untracked entry is listed file by file once more, so a new folder holding only noise (a
/// `.DS_Store`) doesn't count either.
async fn local_state(path: &Path) -> Option<LocalState> {
    let args = [
        "--no-optional-locks",
        "status",
        "--porcelain=v2",
        "-z",
        "--branch",
    ];
    let out = match process::run("git", args, Some(path)).await {
        Ok(out) => out.stdout,
        Err(err) => {
            log::debug!("git status in {}: {err}", path.display());
            return None;
        }
    };
    let entries = status_entries(&out);
    let folders: Vec<&str> = entries
        .iter()
        .filter(|e| e.untracked && e.path.ends_with('/') && !noise::is_noise(e.path))
        .map(|e| e.path)
        .collect();
    let mut real_folders = std::collections::HashSet::new();
    if !folders.is_empty() {
        let args = [
            "--literal-pathspecs",
            "--no-optional-locks",
            "status",
            "--porcelain=v2",
            "-z",
            "--untracked-files=all",
            "--",
        ]
        .into_iter()
        .chain(folders.iter().copied());
        match process::run("git", args, Some(path)).await {
            Ok(inner) => {
                for entry in status_entries(&inner.stdout) {
                    if entry.untracked
                        && !noise::is_noise(entry.path)
                        && let Some(folder) = folders.iter().find(|f| entry.path.starts_with(**f))
                    {
                        real_folders.insert(*folder);
                    }
                }
            }
            // Can't tell: the folders count, as git reported them.
            Err(_) => real_folders.extend(folders.iter().copied()),
        }
    }
    Some(parse_status(&out, |folder| real_folders.contains(folder)))
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

/// One entry of `git status --porcelain=v2 -z`: headers (`# ...`) aside.
struct StatusEntry<'a> {
    path: &'a str,
    untracked: bool,
}

/// The changed and untracked paths of `git status --porcelain=v2 -z` (ignored ones, `!`,
/// skipped). A rename's original path, the entry after it, is skipped too.
fn status_entries(porcelain: &str) -> Vec<StatusEntry<'_>> {
    let mut entries = Vec::new();
    let mut fields = porcelain.split('\0');
    while let Some(entry) = fields.next() {
        // Ordinary, renamed or copied, unmerged: the path follows a fixed count of fields.
        let (path, untracked) = match entry.split_at_checked(2) {
            Some(("1 ", _)) => (entry.splitn(9, ' ').nth(8), false),
            Some(("2 ", _)) => {
                fields.next();
                (entry.splitn(10, ' ').nth(9), false)
            }
            Some(("u ", _)) => (entry.splitn(11, ' ').nth(10), false),
            Some(("? ", path)) => (Some(path), true),
            _ => (None, false),
        };
        if let Some(path) = path.filter(|p| !p.is_empty()) {
            entries.push(StatusEntry { path, untracked });
        }
    }
    entries
}

/// Reads `git status --porcelain=v2 -z --branch`. Untracked noise files don't count, nor
/// untracked folders `real_folder` says hold only noise.
fn parse_status(porcelain: &str, real_folder: impl Fn(&str) -> bool) -> LocalState {
    let mut state = LocalState::default();
    for header in porcelain.split('\0').filter_map(|e| e.strip_prefix("# ")) {
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
    }
    for entry in status_entries(porcelain) {
        let counts = !entry.untracked
            || !(noise::is_noise(entry.path)
                || entry.path.ends_with('/') && !real_folder(entry.path));
        if counts {
            state.changed_files += 1;
            if state.changed_paths.len() < MAX_CHANGED_PATHS {
                state.changed_paths.push(entry.path.to_owned());
            }
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
                editor: None,
                group: None,
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
                        editor: None,
                        group: None,
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

    #[test]
    fn finds_teammates_commit_emails() {
        let log = "Bob B\x1f12+bob@users.noreply.github.com\nBob B\x1fbob@heig-vd.ch\nalice\x1falice@x.ch\n";
        let member = |login: &str, name: Option<&str>| groups::Member {
            login: login.into(),
            name: name.map(Into::into),
            email: None,
        };
        assert_eq!(
            author_email(log, &member("bobby", Some("Bob B"))).as_deref(),
            Some("bob@heig-vd.ch")
        );
        assert_eq!(
            author_email(log, &member("ALICE", None)).as_deref(),
            Some("alice@x.ch")
        );
        assert_eq!(author_email(log, &member("carol", None)), None);
    }

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
            template: None,
            unmerged: Vec::new(),
            grade: None,
            history: Vec::new(),
            grading: None,
            feedback: None,
        }
    }

    #[test]
    fn parses_porcelain_status() {
        let out = [
            "# branch.oid 5f14f16",
            "# branch.head main",
            "# branch.upstream origin/main",
            "# branch.ab +2 -1",
            "1 .M N... 100644 100644 100644 a b src/my file.c",
            "2 R. N... 100644 100644 100644 a b R100 new name.c",
            "old name.c",
            "u UU N... 100644 100644 100644 100644 a b c both.c",
            "? notes.txt",
            "? .idea/",
            "? src/.DS_Store",
            "? empty/",
            "? work/",
            "1 .M N... 100644 100644 100644 a b .vscode/settings.json",
            "! build/",
            "",
        ]
        .join("\0");
        let entries: Vec<&str> = status_entries(&out).iter().map(|e| e.path).collect();
        assert_eq!(
            entries,
            [
                "src/my file.c",
                "new name.c",
                "both.c",
                "notes.txt",
                ".idea/",
                "src/.DS_Store",
                "empty/",
                "work/",
                ".vscode/settings.json",
            ]
        );
        let state = parse_status(&out, |folder| folder == "work/");
        assert_eq!(state.head.as_deref(), Some("5f14f16"));
        assert_eq!(state.branch.as_deref(), Some("main"));
        assert_eq!((state.ahead, state.behind), (Some(2), Some(1)));
        // Tracked files count even in a noise folder; untracked noise and the folder holding
        // only noise don't.
        assert_eq!(state.changed_files, 6);
        assert_eq!(
            state.changed_paths,
            [
                "src/my file.c",
                "new name.c",
                "both.c",
                "notes.txt",
                "work/",
                ".vscode/settings.json",
            ]
        );
        let many: String = (0..25).map(|i| format!("? f{i}.c\0")).collect();
        let state = parse_status(&many, |_| true);
        assert_eq!(state.changed_files, 25);
        assert_eq!(state.changed_paths.len(), MAX_CHANGED_PATHS);
        assert_eq!(state.changed_paths[19], "f19.c");

        let fresh = parse_status("# branch.oid (initial)\0# branch.head (detached)\0", |_| {
            true
        });
        assert_eq!(fresh, LocalState::default());
    }

    /// A real clone: untracked editor files and a new folder holding only a `.DS_Store`
    /// don't count; a new folder with real work does, once.
    #[tokio::test]
    async fn local_state_skips_editor_files() {
        let root = std::env::temp_dir().join(format!("c50tb-local-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let git = |args: &[&str]| {
            let out = std::process::Command::new("git")
                .args(args)
                .current_dir(&root)
                .output()
                .unwrap();
            assert!(out.status.success(), "{args:?}");
        };
        std::fs::create_dir_all(root.join(".idea")).unwrap();
        git(&["init", "-q", "-b", "main"]);
        std::fs::write(root.join(".idea").join("workspace.xml"), "x").unwrap();
        std::fs::create_dir_all(root.join("empty").join("__pycache__")).unwrap();
        std::fs::write(root.join("empty").join(".DS_Store"), "x").unwrap();
        std::fs::write(root.join("empty").join("__pycache__").join("m.pyc"), "x").unwrap();
        assert_eq!(local_state(&root).await.unwrap().changed_files, 0);

        std::fs::create_dir_all(root.join("work")).unwrap();
        std::fs::write(root.join("work").join("a.c"), "x").unwrap();
        std::fs::write(root.join("work").join("b.c"), "x").unwrap();
        std::fs::write(root.join("hello.c~"), "x").unwrap();
        assert_eq!(local_state(&root).await.unwrap().changed_files, 1);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[tokio::test]
    async fn downloads_each_grade_of_the_history_once() {
        let release = |tag: &str| status::GradedRelease {
            tag: tag.into(),
            published_at: None,
            url: format!("page-{tag}"),
            result_url: format!("result-{tag}"),
        };
        let grade = |tag: &str, score: u32| Grade {
            tag: tag.into(),
            score,
            max_score: 10,
            tests_passed: 0,
            tests_total: 0,
            tests: Vec::new(),
            published_at: None,
            url: format!("page-{tag}"),
            release_notes: None,
        };
        let releases = [release("c"), release("b"), release("a"), release("broken")];
        let newest = grade("c", 9);
        let mut old = remote("r", "h", None);
        old.history = vec![GradeSummary::from(&grade("b", 7))];
        let mut downloaded = Vec::new();
        let history = grade_history(&releases, Some(&newest), &old, |r| {
            downloaded.push(r.tag.clone());
            let result = if r.tag == "broken" {
                Err(Error::GitHub("gone".into()))
            } else {
                Ok(grade(&r.tag, 5))
            };
            async move { result }
        })
        .await;
        // The newest grade and the known one are reused; a failed download is left out.
        assert_eq!(downloaded, ["a", "broken"]);
        let scores: Vec<(&str, u32)> = history.iter().map(|g| (g.tag.as_str(), g.score)).collect();
        assert_eq!(scores, [("c", 9), ("b", 7), ("a", 5)]);
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
        let find = |owner, name, config| {
            let found = Found {
                owner,
                name,
                config,
                template: None,
                can_push: true,
            };
            identify(found, &classrooms, &manifests)
        };

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
    fn lists_only_assignments_without_a_repo() {
        let poo = Classroom::from_team("HEIGVD-POO", "classroom50-26-poo-c", None).unwrap();
        let classrooms = BTreeMap::from([(poo.key(), poo)]);
        let manifest: Vec<Assignment> = serde_json::from_value(json!([
            {"slug": "lab01", "name": "Labo 1", "mode": "team"},
            {"slug": "lab02", "name": "Labo 2", "mode": "team"},
            {"slug": "labo-3", "name": "Labo 3", "mode": "team"},
            {"slug": "labo-4", "name": "Labo 4", "mode": "team"},
            {"slug": "labo-5", "name": "Labo 5", "mode": "team"},
            {"slug": "old", "name": "Old", "mode": "team", "closed": true}
        ]))
        .unwrap();
        let manifests =
            HashMap::from([(("heigvd-poo".to_owned(), "26-poo-c".to_owned()), manifest)]);
        let poo_config = |assignment: &str| RepoConfig {
            classroom: "26-poo-c".into(),
            assignment: assignment.into(),
            secret: None,
        };
        let on_github = |name: &str, config: RepoConfig| {
            let mut repo = remote(name, "aaa", None);
            repo.owner = "HEIGVD-POO".into();
            (repo, config)
        };
        let clone = |path: &str, owner: Option<&str>, name: Option<&str>| LocalProject {
            path: path.into(),
            owner: owner.map(str::to_owned),
            repo: name.map(str::to_owned),
            config: Some(poo_config("labo-4")),
        };
        let projects = merge(
            vec![
                // A marker spelled in another case than the manifest.
                on_github("26-poo-c-lab01-group-3", poo_config("LAB01")),
                // A marker naming something else; the repo's name still says lab02.
                on_github("26-poo-c-lab02-group-6", poo_config("template-lab")),
            ],
            vec![
                // A clone not seen on GitHub yet, found by its origin.
                (
                    clone(
                        "/code/lab3",
                        Some("heigvd-poo"),
                        Some("26-poo-c-labo-3-alice"),
                    ),
                    poo_config("labo-3"),
                    None,
                ),
                // No GitHub origin: which org it belongs to is unknown.
                (clone("/code/copy", None, None), poo_config("labo-4"), None),
            ],
        );

        let list = unaccepted(&projects, &classrooms, &manifests);
        let slugs: Vec<&str> = list.iter().map(|u| u.assignment.slug.as_str()).collect();
        assert_eq!(slugs, ["labo-4", "labo-5"]);
        assert_eq!(list[0].id, "heigvd-poo/26-poo-c/labo-4");
    }

    /// `cs/ps3` was `problem-set-three` before `gh teacher assignment rename`.
    fn renamed_class() -> (
        BTreeMap<ClassroomKey, Classroom>,
        HashMap<ClassroomKey, Vec<Assignment>>,
    ) {
        let cs = Classroom::from_team("CS50", "classroom50-cs", None).unwrap();
        let manifest: Vec<Assignment> = serde_json::from_value(json!([
            {"slug": "ps3", "name": "Problem Set 3", "mode": "individual",
             "renamed_from": "problem-set-three"},
            {"slug": "ps4", "name": "Problem Set 4", "mode": "individual"}
        ]))
        .unwrap();
        (
            BTreeMap::from([(cs.key(), cs)]),
            HashMap::from([(("cs50".to_owned(), "cs".to_owned()), manifest)]),
        )
    }

    fn cs_config(assignment: &str) -> RepoConfig {
        RepoConfig {
            classroom: "cs".into(),
            assignment: assignment.into(),
            secret: None,
        }
    }

    #[test]
    fn renamed_assignments_keep_their_repos() {
        let (classrooms, manifests) = renamed_class();
        let find = |name, config| {
            let found = Found {
                owner: Some("CS50"),
                name: Some(name),
                config,
                template: None,
                can_push: true,
            };
            identify(found, &classrooms, &manifests)
        };
        // A marker the rename didn't rewrite, or a clone not pulled since.
        let old = cs_config("problem-set-three");
        assert_eq!(
            find("cs-problem-set-three-alice", Some(&old)).map(|c| c.assignment),
            Some("ps3".to_owned())
        );
        // No marker: the old name or the new one.
        assert_eq!(
            find("cs-problem-set-three-bob", None).map(|c| c.assignment),
            Some("ps3".to_owned())
        );
        assert_eq!(
            find("cs-ps3-bob", None).map(|c| c.assignment),
            Some("ps3".to_owned())
        );
        // A marker of another class or naming something else stays as it is.
        let other = cs_config("lab");
        assert_eq!(find("cs-lab-bob", Some(&other)), Some(other));

        // A straggler the rename skipped counts as accepting the renamed assignment.
        let mut straggler = remote("cs-problem-set-three-alice", "aaa", None);
        straggler.config = Some(old.clone());
        let projects = merge(vec![(straggler, cs_config("ps3"))], Vec::new());
        let list = unaccepted(&projects, &classrooms, &manifests);
        let slugs: Vec<&str> = list.iter().map(|u| u.assignment.slug.as_str()).collect();
        assert_eq!(slugs, ["ps4"]);
        // The old name alone is enough too, whatever its marker says.
        let mut projects = projects;
        projects[0].config.assignment = "something-else".into();
        let list = unaccepted(&projects, &classrooms, &manifests);
        let slugs: Vec<&str> = list.iter().map(|u| u.assignment.slug.as_str()).collect();
        assert_eq!(slugs, ["ps4"]);
    }

    #[test]
    fn clones_made_before_a_rename_join_the_renamed_repo() {
        let (classrooms, manifests) = renamed_class();
        let listed = HashMap::from([("cs50/cs-ps3-alice".to_owned(), "cs-ps3-alice".to_owned())]);
        let rename = |owner, name| renamed_clone(owner, name, &classrooms, &manifests, &listed);
        assert_eq!(
            rename("cs50", "CS-problem-set-three-Alice").as_deref(),
            Some("cs-ps3-alice")
        );
        // Not renamed on GitHub yet, another student's, a current name, another org.
        assert_eq!(rename("cs50", "cs-problem-set-three-bob"), None);
        assert_eq!(rename("cs50", "cs-ps3-alice"), None);
        assert_eq!(rename("other", "cs-problem-set-three-alice"), None);
        let straggler = HashMap::from([
            ("cs50/cs-ps3-alice".to_owned(), "cs-ps3-alice".to_owned()),
            (
                "cs50/cs-problem-set-three-alice".to_owned(),
                "cs-problem-set-three-alice".to_owned(),
            ),
        ]);
        assert_eq!(
            renamed_clone(
                "cs50",
                "cs-problem-set-three-alice",
                &classrooms,
                &manifests,
                &straggler
            ),
            None
        );

        // Renamed, the clone and GitHub's repo are one row.
        let clone = LocalProject {
            path: "/code/ps3".into(),
            owner: Some("cs50".into()),
            repo: rename("cs50", "cs-problem-set-three-alice"),
            config: Some(cs_config("problem-set-three")),
        };
        let projects = merge(
            vec![(remote("cs-ps3-alice", "aaa", None), cs_config("ps3"))],
            vec![(clone, cs_config("ps3"), Some(LocalState::default()))],
        );
        assert_eq!(projects.len(), 1);
        assert_eq!(projects[0].id, "cs50/cs-ps3-alice");
        assert!(projects[0].path.is_some() && projects[0].remote.is_some());
    }

    #[test]
    fn tells_which_ids_a_rename_changed() {
        let (classrooms, manifests) = renamed_class();
        let key = ("cs50".to_owned(), "cs".to_owned());
        let ps3 = manifests[&key][0].clone();
        let classroom = classrooms[&key].clone();
        let project = |name: &str| {
            let mut project = merge(
                vec![(remote(name, "aaa", None), cs_config("ps3"))],
                Vec::new(),
            )
            .remove(0);
            project.assignment = Some(ps3.clone());
            project
        };
        let unaccepted = Unaccepted {
            id: "cs50/cs/ps3".into(),
            classroom,
            assignment: ps3.clone(),
            accept_url: String::new(),
            group: None,
        };
        let snapshot = Snapshot {
            projects: vec![project("cs-ps3-alice")],
            unaccepted: vec![unaccepted],
            ..Snapshot::default()
        };
        assert_eq!(
            renamed_ids(&snapshot),
            [
                (
                    "cs50/cs-problem-set-three-alice".to_owned(),
                    "cs50/cs-ps3-alice".to_owned()
                ),
                (
                    "cs50/cs/problem-set-three".to_owned(),
                    "cs50/cs/ps3".to_owned()
                ),
            ]
        );
        assert_eq!(
            previous_id(&snapshot.projects[0]).as_deref(),
            Some("cs50/cs-problem-set-three-alice")
        );
        // A straggler keeps its id, and nothing moves onto it.
        let snapshot = Snapshot {
            projects: vec![
                project("cs-problem-set-three-alice"),
                project("cs-ps3-alice"),
            ],
            ..Snapshot::default()
        };
        assert_eq!(
            renamed_ids(&snapshot),
            [(
                "cs50/cs/problem-set-three".to_owned(),
                "cs50/cs/ps3".to_owned()
            )]
        );
    }

    #[test]
    fn identifies_repos_by_their_template() {
        let c = Classroom::from_team("HEIGVD-POO", "classroom50-26-poo-c", None).unwrap();
        let d = Classroom::from_team("HEIGVD-POO", "classroom50-26-poo-d", None).unwrap();
        let other = Classroom::from_team("Elsewhere", "classroom50-web", None).unwrap();
        // A classroom of the org whose slug starts the others' (no manifest read).
        let poo = Classroom::from_team("HEIGVD-POO", "classroom50-26-poo", None).unwrap();
        let template = |repo: &str| json!({"owner": "HEIGVD-POO", "repo": repo, "branch": "main"});
        let entry = |slug: &str, repo: Option<&str>| json!({"slug": slug, "name": slug, "mode": "team", "template": repo.map(template)});
        let list = |entries: Vec<serde_json::Value>| -> Vec<Assignment> {
            serde_json::from_value(serde_json::Value::Array(entries)).unwrap()
        };
        let manifests = HashMap::from([
            (
                c.key(),
                list(vec![
                    entry("labo-3-hogwarts", Some("hogwarts")),
                    entry("labo-4", Some("shared")),
                    entry("labo-5", Some("shared")),
                    entry("labo-6", Some("unique")),
                    entry("labo-e", None),
                ]),
            ),
            (d.key(), list(vec![entry("labo-3", Some("hogwarts"))])),
            (other.key(), list(vec![entry("site", Some("unique"))])),
        ]);
        let classrooms = BTreeMap::from([
            (c.key(), c),
            (d.key(), d),
            (other.key(), other),
            (poo.key(), poo),
        ]);
        let find = |owner: &str, name: &str, template: Option<&str>| {
            let found = Found {
                owner: Some(owner),
                name: Some(name),
                config: None,
                template,
                can_push: true,
            };
            identify(found, &classrooms, &manifests)
                .map(|c| format!("{}/{}", c.classroom, c.assignment))
        };
        let hogwarts = Some("HEIGVD-POO/hogwarts");
        let shared = Some("heigvd-poo/Shared");

        // One entry with that template: renamed by hand, still found.
        assert_eq!(
            find("HEIGVD-POO", "renamed-by-hand", Some("HEIGVD-POO/unique")).as_deref(),
            Some("26-poo-c/labo-6")
        );
        // The name rule comes first.
        assert_eq!(
            find("HEIGVD-POO", "26-poo-c-labo-4-alice", hogwarts).as_deref(),
            Some("26-poo-c/labo-4")
        );
        // One template in two classes: the classroom prefix decides, else nothing.
        assert_eq!(
            find("HEIGVD-POO", "26-poo-c-hogwarts-alice", hogwarts).as_deref(),
            Some("26-poo-c/labo-3-hogwarts")
        );
        assert_eq!(
            find("heigvd-poo", "26-POO-D-mine", hogwarts).as_deref(),
            Some("26-poo-d/labo-3")
        );
        assert_eq!(find("HEIGVD-POO", "my-hogwarts", hogwarts), None);
        // One template reused in a class: the slug in the name decides, else nothing.
        assert_eq!(
            find("HEIGVD-POO", "26-poo-c-my-labo-5", shared).as_deref(),
            Some("26-poo-c/labo-5")
        );
        assert_eq!(find("HEIGVD-POO", "26-poo-c-renamed", shared), None);
        assert_eq!(find("HEIGVD-POO", "labo-4-and-labo-5", shared), None);
        // No template, an unknown one, the template itself, or a repo outside the org.
        assert_eq!(find("HEIGVD-POO", "renamed-by-hand", None), None);
        assert_eq!(find("HEIGVD-POO", "x", Some("HEIGVD-POO/other")), None);
        assert_eq!(
            find("HEIGVD-POO", "hogwarts", Some("HEIGVD-POO/unique")),
            None
        );
        assert_eq!(
            find("alice", "renamed-by-hand", Some("HEIGVD-POO/unique")),
            None
        );
        assert_eq!(
            find("Elsewhere", "copy", Some("HEIGVD-POO/unique")).as_deref(),
            Some("web/site")
        );
        // Named after another classroom of the org (the longest prefix deciding): an old
        // term's repo, a sibling class's, never this entry's.
        assert_eq!(
            find("HEIGVD-POO", "26-poo-d-renamed", Some("HEIGVD-POO/unique")),
            None
        );
        assert_eq!(
            find("HEIGVD-POO", "26-poo-x-renamed", Some("HEIGVD-POO/unique")),
            None
        );
        assert_eq!(
            find("HEIGVD-POO", "26-poo-c-renamed", Some("HEIGVD-POO/unique")).as_deref(),
            Some("26-poo-c/labo-6")
        );

        // Accepted that way, the assignment isn't offered again.
        let mut repo = remote("renamed-by-hand", "aaa", None);
        repo.owner = "HEIGVD-POO".into();
        repo.config = None;
        repo.template = Some("HEIGVD-POO/unique".into());
        let config = identify(
            Found {
                owner: Some(&repo.owner),
                name: Some(&repo.name),
                config: None,
                template: repo.template.as_deref(),
                can_push: repo.can_push,
            },
            &classrooms,
            &manifests,
        )
        .unwrap();
        let projects = merge(vec![(repo, config)], Vec::new());
        let list = unaccepted(&projects, &classrooms, &manifests);
        assert!(list.iter().all(|u| u.assignment.slug != "labo-6"));
        assert!(list.iter().any(|u| u.assignment.slug == "labo-5"));
    }

    #[test]
    fn passes_templates_on_only_for_the_users_own_repos() {
        let listed = |is_template: bool, is_archived: bool, permission: Option<&str>| {
            serde_json::from_value::<ListedRepo>(json!({
                "nameWithOwner": "cs50/renamed",
                "pushedAt": null,
                "isTemplate": is_template,
                "isArchived": is_archived,
                "viewerPermission": permission,
                "templateRepository": {"nameWithOwner": "cs50/lab1-template"},
            }))
            .unwrap()
        };
        for permission in ["ADMIN", "MAINTAIN", "WRITE"] {
            let repo = listed(false, false, Some(permission));
            assert_eq!(repo.generated_from().as_deref(), Some("cs50/lab1-template"));
            assert_eq!(repo.can_push(), Some(true));
        }
        // Read only: passed on, `by_template` decides from the entry.
        for permission in ["READ", "TRIAGE"] {
            let repo = listed(false, false, Some(permission));
            assert!(repo.generated_from().is_some());
            assert_eq!(repo.can_push(), Some(false));
        }
        assert_eq!(listed(false, false, None).can_push(), None);
        // A template made from the template, an old term's archived repo.
        assert_eq!(listed(true, false, Some("ADMIN")).generated_from(), None);
        assert_eq!(listed(false, true, Some("ADMIN")).generated_from(), None);
    }

    /// Close submission sets every student repo read-only: the student's own repo, known
    /// by its template, stays; a read-only copy of an open assignment's template never counts.
    #[test]
    fn a_read_only_repo_matches_a_closed_assignment_only() {
        let class = Classroom::from_team("cs50", "classroom50-cs", None).unwrap();
        let manifest: Vec<Assignment> = serde_json::from_value(json!([
            {"slug": "lab1", "name": "Lab 1", "mode": "individual", "closed": true,
             "template": {"owner": "cs50", "repo": "lab1-template", "branch": "main"}},
            {"slug": "lab2", "name": "Lab 2", "mode": "individual",
             "template": {"owner": "cs50", "repo": "lab2-template", "branch": "main"}},
        ]))
        .unwrap();
        assert!(manifest[0].closed && !manifest[1].closed);
        let manifests = HashMap::from([(class.key(), manifest)]);
        let classrooms = BTreeMap::from([(class.key(), class)]);
        let find = |template: &str, can_push: bool| {
            let found = Found {
                owner: Some("cs50"),
                name: Some("renamed-by-hand"),
                config: None,
                template: Some(template),
                can_push,
            };
            identify(found, &classrooms, &manifests).map(|c| c.assignment)
        };
        assert_eq!(find("cs50/lab1-template", false).as_deref(), Some("lab1"));
        assert_eq!(find("cs50/lab1-template", true).as_deref(), Some("lab1"));
        assert_eq!(find("cs50/lab2-template", false), None);
        assert_eq!(find("cs50/lab2-template", true).as_deref(), Some("lab2"));
    }

    #[test]
    fn a_template_alone_never_doubles_an_assignment() {
        let repo = |name: &str| {
            let mut repo = remote(name, "aaa", None);
            repo.config = None;
            repo
        };
        let lab = |slug: &str| cs_config(slug);
        let names = |repos: Vec<(RemoteRepo, RepoConfig)>| -> Vec<String> {
            repos.into_iter().map(|(r, _)| r.name).collect()
        };
        // A scratch copy next to the real repo (by name or marker) drops out.
        let kept = without_doubtful_templates(vec![
            (repo("cs-lab1-alice"), lab("lab1"), MatchedBy::Name),
            (
                repo("alice-lab1-practice"),
                lab("lab1"),
                MatchedBy::Template,
            ),
            (repo("cs-lab2-alice"), lab("LAB2"), MatchedBy::Marker),
            (repo("lab2-again"), lab("lab2"), MatchedBy::Template),
            (repo("renamed-lab3"), lab("lab3"), MatchedBy::Template),
        ]);
        assert_eq!(
            names(kept),
            ["cs-lab1-alice", "cs-lab2-alice", "renamed-lab3"]
        );
        // Two told by their template alone: neither can be trusted.
        let kept = without_doubtful_templates(vec![
            (repo("mine"), lab("lab4"), MatchedBy::Template),
            (repo("also-mine"), lab("lab4"), MatchedBy::Template),
        ]);
        assert!(kept.is_empty());
    }

    #[test]
    fn opening_looks_for_repos_accepted_elsewhere() {
        let now = Utc::now();
        let poo = Classroom::from_team("HEIGVD-POO", "classroom50-26-poo-c", None).unwrap();
        let item = |slug: &str, extra: serde_json::Value| {
            let mut assignment = json!({"slug": slug, "name": slug, "mode": "team"});
            assignment
                .as_object_mut()
                .unwrap()
                .extend(extra.as_object().unwrap().clone());
            Unaccepted {
                id: format!("heigvd-poo/26-poo-c/{slug}"),
                classroom: poo.clone(),
                assignment: serde_json::from_value(assignment).unwrap(),
                accept_url: String::new(),
                group: None,
            }
        };
        let later = (now + chrono::TimeDelta::days(1)).to_rfc3339();
        let mut snapshot = Snapshot {
            unaccepted: vec![
                item("labo-4", json!({"locked": true})),
                item("labo-5", json!({"available_from": later})),
            ],
            ..Snapshot::default()
        };
        let mut settings = Settings::default();
        assert!(!awaiting_accept(&snapshot, &settings, now));
        snapshot.unaccepted.push(item("labo-6", json!({})));
        assert!(awaiting_accept(&snapshot, &settings, now));
        settings.hidden = vec!["heigvd-poo/26-poo-c/labo-6".into()];
        assert!(!awaiting_accept(&snapshot, &settings, now));
        settings.hidden.clear();
        settings.hidden_classes = vec!["heigvd-poo/26-poo-c".into()];
        assert!(!awaiting_accept(&snapshot, &settings, now));
    }

    #[test]
    fn rebuilds_from_every_repo_of_the_last_discovery() {
        let cache = RemoteCache {
            login: Some("alice".into()),
            repos: BTreeMap::from([
                (
                    "cs50/fall-hello-alice".into(),
                    remote("fall-hello-alice", "a", None),
                ),
                (
                    "cs50/fall-lab2-alice".into(),
                    remote("fall-lab2-alice", "b", None),
                ),
            ]),
            ..RemoteCache::default()
        };
        // The last build had no manifest and kept only the repo with a marker file.
        let mut previous = Snapshot {
            synced_at: Some(Utc::now()),
            ..Snapshot::default()
        };
        previous.projects = merge(
            vec![(remote("fall-hello-alice", "a", None), config("hello"))],
            Vec::new(),
        );
        let names =
            |repos: Vec<RemoteRepo>| -> Vec<String> { repos.into_iter().map(|r| r.name).collect() };
        assert_eq!(
            names(last_discovery(&previous, &cache)),
            ["fall-hello-alice", "fall-lab2-alice"]
        );
        // Signed out: the cache may hold another account's repos.
        previous.issue = Some(SyncIssue::from(&Error::NotSignedIn));
        assert_eq!(
            names(last_discovery(&previous, &cache)),
            ["fall-hello-alice"]
        );
    }

    #[test]
    fn retries_a_failed_full_sync_at_the_status_pace() {
        use chrono::TimeDelta as T;
        let now = Utc::now();
        let (full_age, status_age) = (T::hours(1), T::minutes(5));
        // The last full sync that worked is 50 min old, a failed one ran 10 min ago.
        let snapshot = Snapshot {
            synced_at: Some(now - T::minutes(50)),
            checked_at: Some(now - T::minutes(50)),
            ..Snapshot::default()
        };
        let attempt = Some(now - T::minutes(10));
        assert_eq!(
            due_mode(&snapshot, attempt, now, full_age, status_age),
            Some(Mode::Status)
        );
        // Due by age: retried, but not on every tick after a failure.
        let old = Snapshot {
            synced_at: Some(now - T::hours(2)),
            ..snapshot.clone()
        };
        assert_eq!(
            due_mode(&old, attempt, now, full_age, status_age),
            Some(Mode::Full)
        );
        let just_failed = Some(now - T::minutes(1));
        assert_eq!(
            due_mode(&old, just_failed, now, full_age, status_age),
            Some(Mode::Status)
        );
        // Network back or waking up asks with no minimum age: right away.
        assert_eq!(
            due_mode(&old, just_failed, now, full_age, T::zero()),
            Some(Mode::Full)
        );
        let fresh = Snapshot {
            synced_at: Some(now),
            checked_at: Some(now),
            ..Snapshot::default()
        };
        assert_eq!(due_mode(&fresh, Some(now), now, full_age, status_age), None);
    }

    #[test]
    fn offers_accepting_only_with_the_account_repo_list() {
        let mut snapshot = Snapshot::default();
        assert!(!discovered(&snapshot), "no full sync yet");
        snapshot.synced_at = Some(Utc::now());
        assert!(discovered(&snapshot));
        snapshot.issue = Some(SyncIssue::from(&Error::Offline));
        assert!(discovered(&snapshot), "offline keeps the cached list");
        snapshot.issue = Some(SyncIssue::from(&Error::NotSignedIn));
        assert!(!discovered(&snapshot), "signed out lists clones only");

        let mut cache = RemoteCache::default();
        assert!(!cache.complete());
        cache.login = Some("alice".into());
        assert!(cache.complete());
        cache.incomplete = true;
        assert!(!cache.complete());
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
            editor: None,
            group: None,
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
                editor: None,
                group: None,
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

    /// The WebDriver suite's mock GitHub answers read like GitHub's (`testenv::golden`).
    #[test]
    fn reads_the_suites_mock_listing_and_teams() {
        use crate::testenv::golden;
        let list: ListData = serde_json::from_str(&golden("list")).unwrap();
        assert_eq!(list.viewer.login, "ada-e2e");
        let nodes: Vec<&ListedRepo> = list.viewer.repositories.nodes.iter().flatten().collect();
        assert_eq!(nodes.len(), 6);
        let templates: Vec<Option<String>> = nodes.iter().map(|n| n.generated_from()).collect();
        assert_eq!(templates[3].as_deref(), Some("c50-e2e-lab/blog-starter"));
        assert_eq!(nodes[3].can_push(), Some(true));
        // A read-only copy generated from the same template.
        assert_eq!(nodes[4].name_with_owner, "c50-e2e-lab/blog-solution");
        assert_eq!(templates[4], templates[3]);
        assert_eq!(nodes[4].can_push(), Some(false));
        assert!(templates.iter().filter(|t| t.is_some()).count() == 2);
        // That repo matches no name: its template says which assignment it is.
        let web = Classroom::from_team("c50-e2e-lab", "classroom50-web", None).unwrap();
        let manifest = classroom50::parse_assignments("web", &golden("manifest-web")).unwrap();
        let manifests = HashMap::from([(web.key(), manifest)]);
        let classrooms = BTreeMap::from([(web.key(), web)]);
        let found = Found {
            owner: Some("c50-e2e-lab"),
            name: Some("ada-writes"),
            config: None,
            template: templates[3].as_deref(),
            can_push: true,
        };
        let config = identify(found, &classrooms, &manifests).unwrap();
        assert_eq!(config.assignment, "blog");
        // The read-only copy: Blog is open, so it is no student's work.
        let copy = Found {
            name: Some("blog-solution"),
            template: templates[4].as_deref(),
            can_push: false,
            ..found
        };
        assert!(identify(copy, &classrooms, &manifests).is_none());
        assert!(!list.viewer.repositories.page_info.has_next_page);

        let teams: TeamsData = serde_json::from_str(&golden("teams")).unwrap();
        let classrooms: Vec<Classroom> = teams
            .viewer
            .organizations
            .nodes
            .into_iter()
            .flatten()
            .flat_map(|org| {
                org.teams
                    .nodes
                    .into_iter()
                    .flatten()
                    .filter_map(move |team| {
                        Classroom::from_team(&org.login, &team.slug, team.description.as_deref())
                    })
            })
            .collect();
        let names: Vec<_> = classrooms.iter().map(|c| c.name.as_deref()).collect();
        assert_eq!(names, [Some("Algorithms"), Some("Web Programming")]);
        assert!(classrooms.iter().all(|c| c.active && c.secret.is_none()));
    }

    #[test]
    fn reads_the_suites_mock_repos_and_statuses() {
        use crate::testenv::golden;
        let mut details: HashMap<String, Option<RepoNode>> =
            serde_json::from_str(&golden("details")).unwrap();
        assert_eq!(details.len(), 7);
        assert!(details["r6"].is_none(), "an unknown repo is null");
        let hello = details.remove("r0").flatten().unwrap().into_remote();
        assert_eq!(
            hello.config.as_ref().map(|c| c.assignment.as_str()),
            Some("hello")
        );
        assert!(hello.can_push && !hello.archived);
        assert_eq!(
            hello.head.as_ref().unwrap().author.as_deref(),
            Some("ada-e2e")
        );
        let submission = hello.last_submission.unwrap();
        assert!(submission.tag.starts_with("submit/") && submission.at.is_some());
        assert_ne!(submission.commit, hello.head.unwrap().oid);
        let portfolio = details.remove("r2").flatten().unwrap().into_remote();
        assert!(portfolio.config.is_none());

        let mut statuses: HashMap<String, Option<StatusNode>> =
            serde_json::from_str(&golden("status")).unwrap();
        let mut hello = statuses.remove("r0").flatten().unwrap();
        let released = hello.status.graded_releases();
        assert_eq!(released.len(), 1);
        assert!(released[0].result_url.ends_with("/result.json"));
        assert_eq!(hello.status.grading(), None);
        assert_eq!(hello.status.feedback("ada-e2e"), None);
        assert!(hello.status.unmerged(Some("main")).is_empty());
        let mut sorting = statuses.remove("r1").flatten().unwrap();
        assert!(sorting.status.graded_releases().is_empty());
        assert!(sorting.repo.into_remote().config.is_some());
    }
}
