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
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};
use tauri::{AppHandle, Emitter, Manager};
use tokio::task::JoinSet;

use crate::classroom50::{
    self, Assignment, Classroom, RepoConfig, SUBMIT_TAG_PREFIX, SubmissionMode,
};
use crate::discovery::{self, LocalProject};
use crate::editors;
use crate::error::{Error, Result};
use crate::github::{self, Http, HttpCache};
use crate::process;
use crate::settings::{Settings, SettingsStore};
use crate::tools;
use crate::watch::{self, WatchTarget};

pub const UPDATED_EVENT: &str = "projects-updated";
pub const PROGRESS_EVENT: &str = "sync-progress";

/// v2 keeps every inspected repo, not only those with a marker file.
const CACHE_FILE: &str = "repos-v2.json";
const SYNC_INTERVAL: Duration = Duration::from_secs(15 * 60);
/// Opening the popover refreshes at most this often.
const MIN_AUTO_INTERVAL: chrono::TimeDelta = chrono::TimeDelta::minutes(2);
/// Repos per GraphQL request when reading `.classroom50.yaml`; larger batches time out.
const DETAIL_BATCH: usize = 25;
/// Batches in flight at once on a cold cache; GitHub discourages more concurrency.
const PARALLEL_BATCHES: usize = 3;
/// 100 repos per page; anything past 2000 accessible repos isn't scanned.
const MAX_LIST_PAGES: usize = 20;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Commit {
    pub oid: String,
    #[serde(default)]
    pub at: Option<DateTime<Utc>>,
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
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LocalState {
    pub branch: Option<String>,
    pub head: Option<String>,
    /// `None` when the branch has no upstream.
    pub ahead: Option<u32>,
    pub behind: Option<u32>,
    pub changed_files: u32,
}

/// The most pressing reason the graded state lags behind the student's work.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Pending {
    Uncommitted,
    Unpushed,
    /// Tag-mode assignment whose pushed head has no `submit/*` tag.
    Unsubmitted,
}

#[derive(Debug, Clone, Serialize)]
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
}

/// Why the last sync was partial, as `{ kind, message }` like command errors.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SyncIssue {
    pub kind: String,
    pub message: String,
    #[serde(skip)]
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

#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Snapshot {
    pub projects: Vec<Project>,
    pub synced_at: Option<DateTime<Utc>>,
    pub issue: Option<SyncIssue>,
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
    pub done: usize,
    pub total: usize,
}

pub struct ProjectsState {
    snapshot: Mutex<Snapshot>,
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
        Self {
            snapshot: Mutex::default(),
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

/// Runs a full sync and broadcasts the result. Concurrent callers wait for the sync in
/// flight and get its result instead of starting another.
pub async fn refresh(app: &AppHandle) -> Snapshot {
    let state = app.state::<ProjectsState>();
    let Ok(guard) = state.sync_lock.try_lock() else {
        drop(state.sync_lock.lock().await);
        return state.snapshot();
    };
    publish(app, Mode::Full, guard).await
}

/// Re-reads only the disk (after a clone, commit or settings change) and keeps the last
/// GitHub data, so it costs no API request.
pub async fn refresh_local(app: &AppHandle) -> Snapshot {
    let state = app.state::<ProjectsState>();
    let guard = state.sync_lock.lock().await;
    publish(app, Mode::Local, guard).await
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Mode {
    /// GitHub discovery, local scan and manifests.
    Full,
    /// Local scan only, on top of the previous snapshot's GitHub data.
    Local,
}

async fn publish(app: &AppHandle, mode: Mode, _guard: tokio::sync::MutexGuard<'_, ()>) -> Snapshot {
    let state = app.state::<ProjectsState>();
    let http = &app.state::<Http>().0;
    let settings = app.state::<SettingsStore>().get();

    let progress = Progress::start(app);
    let (snapshot, targets) = sync(&state, http, &settings, mode, &progress).await;
    progress.finish();
    if let Ok(mut current) = state.snapshot.lock() {
        *current = snapshot.clone();
    }
    if let Err(err) = app.emit(UPDATED_EVENT, &snapshot) {
        log::warn!("could not emit {UPDATED_EVENT}: {err}");
    }
    watch::rewatch(app, targets);
    snapshot
}

/// Reports sync progress to the UI as `sync-progress`.
#[derive(Clone)]
struct Progress(AppHandle);

impl Progress {
    fn start(app: &AppHandle) -> Self {
        let progress = Self(app.clone());
        progress.update(|p| {
            *p = SyncProgress {
                running: true,
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

/// Refreshes in the background unless a sync ran recently or GitHub asked to back off.
pub fn refresh_if_stale(app: &AppHandle) {
    let snapshot = app.state::<ProjectsState>().snapshot();
    let now = Utc::now();
    let recent = snapshot
        .synced_at
        .is_some_and(|at| now - at < MIN_AUTO_INTERVAL);
    let backing_off = snapshot
        .issue
        .and_then(|issue| issue.retry_after)
        .is_some_and(|until| now < until);
    if recent || backing_off {
        return;
    }
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        refresh(&app).await;
    });
}

/// Syncs once at startup, then every `SYNC_INTERVAL`.
pub fn start_background_sync(app: AppHandle) {
    tauri::async_runtime::spawn(async move {
        loop {
            refresh_if_stale(&app);
            tokio::time::sleep(SYNC_INTERVAL).await;
        }
    });
}

/// Returns the snapshot and what to watch for local changes.
async fn sync(
    state: &ProjectsState,
    http: &reqwest::Client,
    settings: &Settings,
    mode: Mode,
    progress: &Progress,
) -> (Snapshot, Vec<WatchTarget>) {
    let now = Utc::now();
    let scan = scan_local(
        settings.watched_folders.clone(),
        settings.scan_editor_projects,
        progress.clone(),
    );
    let (snapshot, scan_targets) = if mode == Mode::Local {
        let (locals, targets) = scan.await;
        (
            local_snapshot(state, http, locals, now, progress).await,
            targets,
        )
    } else {
        let (remote, (locals, targets)) = tokio::join!(sync_remote(state, http, progress), scan);
        (
            full_snapshot(state, http, remote, locals, now, progress).await,
            targets,
        )
    };
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
    progress: &Progress,
) -> Snapshot {
    let previous = state.snapshot();
    let remote_repos = previous
        .projects
        .into_iter()
        .filter_map(|p| p.remote)
        .collect();
    let classrooms = state.remote().classrooms;
    let projects = build(remote_repos, classrooms, locals, state, http, now, progress).await;
    Snapshot {
        projects,
        synced_at: previous.synced_at,
        issue: previous.issue,
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

    Snapshot {
        projects: build(repos, classrooms, locals, state, http, now, progress).await,
        synced_at: Some(now),
        issue,
    }
}

type ClassroomKey = (String, String);

/// Resolves which repos are assignments, then merges, annotates and sorts them.
async fn build(
    remote: Vec<RemoteRepo>,
    team_classrooms: Vec<Classroom>,
    locals: Vec<LocalProject>,
    state: &ProjectsState,
    http: &reqwest::Client,
    now: DateTime<Utc>,
    progress: &Progress,
) -> Vec<Project> {
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
        project.pending = pending(project);
        project.web_url = project.owner.as_deref().map(|owner| {
            classroom50::submission_url(
                owner,
                &project.config.classroom,
                &project.config.assignment,
            )
        });
    }
    projects.sort_by_cached_key(|p| sort_key(p, now));
    projects
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
        let manifest = match cache.get_text(http, &url).await {
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
  defaultBranchRef { name target { oid ... on Commit { committedDate } } }
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
    /// Set on annotated tags: the object the tag points at.
    target: Option<Box<GitObject>>,
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
    let (login, listed) = list_repos(http, &token).await?;
    progress.advance();
    let classrooms = list_classrooms(http, &token, &login).await;
    progress.advance();

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
        let responses = join_all(
            batches
                .iter()
                .map(|batch| fetch_details(http, &token, batch)),
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
                    Some(node) => cache.repos.insert(k, node.into_remote()),
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

/// One aliased query (`r0`, `r1`, ...) for a batch of repos. Names travel as variables,
/// never spliced into the query text.
async fn fetch_details(
    http: &reqwest::Client,
    token: &str,
    batch: &[&ListedRepo],
) -> Result<HashMap<String, Option<RepoNode>>> {
    let mut params = Vec::new();
    let mut fields = String::new();
    let mut variables = Map::new();
    for (i, repo) in batch.iter().enumerate() {
        let Some((owner, name)) = repo.name_with_owner.split_once('/') else {
            continue;
        };
        params.push(format!("$o{i}: String!, $n{i}: String!"));
        fields.push_str(&format!(
            "r{i}: repository(owner: $o{i}, name: $n{i}) {{ ...Repo }}\n"
        ));
        variables.insert(format!("o{i}"), owner.into());
        variables.insert(format!("n{i}"), name.into());
    }
    let query = format!(
        "query({}) {{\n{fields}}}\n{REPO_FRAGMENT}",
        params.join(", ")
    );
    github::graphql(http, token, &query, Value::Object(variables)).await
}

fn key(name_with_owner: &str) -> String {
    name_with_owner.to_ascii_lowercase()
}

// ---- Local clones ----------------------------------------------------------------------

/// Candidate clones, and the editor files and folders to watch for new ones. Looks in the
/// watched folders and, when `editors` is on, in the editors' recent projects, the folders
/// holding them and the IDEs' default project folders.
async fn scan_local(
    folders: Vec<PathBuf>,
    editors: bool,
    progress: Progress,
) -> (Vec<LocalProject>, Vec<WatchTarget>) {
    tauri::async_runtime::spawn_blocking(move || {
        let (roots, targets) = scan_roots(folders, editors);
        progress.add(roots.len());
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
fn scan_roots(folders: Vec<PathBuf>, editors: bool) -> (Vec<(PathBuf, usize)>, Vec<WatchTarget>) {
    let mut roots: Vec<(PathBuf, usize)> = folders
        .iter()
        .map(|folder| (folder.clone(), discovery::DEFAULT_MAX_DEPTH))
        .collect();
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
        .filter_map(|dir| dir.parent().map(Path::to_path_buf))
        .collect();
    // A recent folder may be a course folder holding the clones; its siblings may be
    // clones the editor never opened.
    let candidates = found
        .recent
        .into_iter()
        .map(|dir| (dir, EDITOR_SCAN_DEPTH))
        .chain(parents.iter().cloned().map(|dir| (dir, 1)))
        .chain(
            found
                .roots
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
            .chain(found.roots)
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
                    },
                );
            }
        }
    }
    by_id.into_values().collect()
}

/// When the clone's index last changed (any stage, commit, checkout or pull).
fn last_activity(path: &Path) -> Option<std::time::SystemTime> {
    std::fs::metadata(path.join(".git").join("index"))
        .and_then(|meta| meta.modified())
        .ok()
}

fn pending(project: &Project) -> Option<Pending> {
    if let Some(local) = &project.local {
        if local.changed_files > 0 {
            return Some(Pending::Uncommitted);
        }
        if local.ahead.is_some_and(|n| n > 0) {
            return Some(Pending::Unpushed);
        }
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
            }),
            last_submission: submitted.map(|commit| Submission {
                tag: "submit/x".into(),
                commit: commit.into(),
                at: None,
            }),
            config: Some(config("hello")),
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
        let edited = project(remote("r", "aaa", Some("aaa")), Some(dirty));
        assert_eq!(pending(&edited), Some(Pending::Uncommitted));
    }
}
