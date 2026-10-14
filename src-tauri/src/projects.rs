//! The merged project list: GitHub repos holding `.classroom50.yaml`, clones found in the
//! watched folders, and each classroom's published assignment manifest, joined by
//! `owner/repo`. A sync runs on a timer, when the popover opens and on demand; the result
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

use crate::classroom50::{self, Assignment, RepoConfig, SUBMIT_TAG_PREFIX, SubmissionMode};
use crate::discovery::{self, LocalProject};
use crate::error::{Error, Result};
use crate::github::{self, Http, HttpCache};
use crate::process;
use crate::settings::SettingsStore;
use crate::tools;

pub const UPDATED_EVENT: &str = "projects-updated";

const CACHE_FILE: &str = "repos.json";
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
    pub config: RepoConfig,
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
    pub config: RepoConfig,
    pub path: Option<PathBuf>,
    pub local: Option<LocalState>,
    pub remote: Option<RemoteRepo>,
    pub assignment: Option<Assignment>,
    pub pending: Option<Pending>,
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
    /// The Classroom 50 repos among them.
    repos: BTreeMap<String, RemoteRepo>,
}

pub struct ProjectsState {
    snapshot: Mutex<Snapshot>,
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
            sync_lock: tokio::sync::Mutex::new(()),
            remote: Mutex::new(remote),
            remote_path,
            http_cache: HttpCache::load(cache_dir),
        }
    }

    pub fn snapshot(&self) -> Snapshot {
        self.snapshot.lock().map(|s| s.clone()).unwrap_or_default()
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

/// Re-reads only the watched folders (after a clone, commit or settings change) and keeps
/// the last GitHub data, so it costs no API request.
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
    let folders = app.state::<SettingsStore>().get().watched_folders;

    let snapshot = sync(&state, http, folders, mode).await;
    if let Ok(mut current) = state.snapshot.lock() {
        *current = snapshot.clone();
    }
    if let Err(err) = app.emit(UPDATED_EVENT, &snapshot) {
        log::warn!("could not emit {UPDATED_EVENT}: {err}");
    }
    snapshot
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

async fn sync(
    state: &ProjectsState,
    http: &reqwest::Client,
    folders: Vec<PathBuf>,
    mode: Mode,
) -> Snapshot {
    let now = Utc::now();
    if mode == Mode::Local {
        let previous = state.snapshot();
        let remote_repos = previous
            .projects
            .into_iter()
            .filter_map(|p| p.remote)
            .collect();
        let projects = build(remote_repos, scan_local(folders).await, state, http, now).await;
        return Snapshot {
            projects,
            synced_at: previous.synced_at,
            issue: previous.issue,
        };
    }

    let (remote, locals) = tokio::join!(sync_remote(state, http), scan_local(folders));
    let (remote_repos, issue) = match remote {
        Ok(repos) => (repos, None),
        // Without a usable login the cached repos may belong to another account.
        Err(err @ (Error::NotSignedIn | Error::NotInstalled { .. })) => {
            (Vec::new(), Some(SyncIssue::from(&err)))
        }
        // Offline or rate limited: keep showing what was last seen.
        Err(err) => {
            log::warn!("GitHub sync failed: {err}");
            (
                state.remote().repos.into_values().collect(),
                Some(SyncIssue::from(&err)),
            )
        }
    };

    Snapshot {
        projects: build(remote_repos, locals, state, http, now).await,
        synced_at: Some(now),
        issue,
    }
}

async fn build(
    remote: Vec<RemoteRepo>,
    locals: Vec<(LocalProject, Option<LocalState>)>,
    state: &ProjectsState,
    http: &reqwest::Client,
    now: DateTime<Utc>,
) -> Vec<Project> {
    let mut projects = merge(remote, locals);
    attach_assignments(&mut projects, &state.http_cache, http).await;
    state.http_cache.save();
    for project in &mut projects {
        project.pending = pending(project);
    }
    projects.sort_by_cached_key(|p| sort_key(p, now));
    projects
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
    /// `None` when the repo isn't a Classroom 50 assignment (no or invalid config).
    fn into_remote(self) -> Option<RemoteRepo> {
        let config = RepoConfig::parse(self.config?.text.as_deref()?).ok()?;
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
        Some(RemoteRepo {
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
        })
    }
}

/// `2026-10-14T12-00-05Z-abc1234` (the part after `submit/`) -> its UTC instant.
fn parse_submit_time(name: &str) -> Option<DateTime<Utc>> {
    let stamp = name.get(..20)?;
    NaiveDateTime::parse_from_str(stamp, "%Y-%m-%dT%H-%M-%SZ")
        .ok()
        .map(|t| t.and_utc())
}

/// Lists every repo the user can see, then inspects only those pushed since the last
/// sync. Progress is kept even when a later request fails.
async fn sync_remote(state: &ProjectsState, http: &reqwest::Client) -> Result<Vec<RemoteRepo>> {
    let token = tools::gh_token().await?;
    let (login, listed) = list_repos(http, &token).await?;

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
                match node.and_then(RepoNode::into_remote) {
                    Some(repo) => cache.repos.insert(k, repo),
                    None => cache.repos.remove(&k),
                };
            }
        }
    }
    let repos = cache.repos.values().cloned().collect();
    state.store_remote(cache);
    result.map(|()| repos)
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

async fn scan_local(folders: Vec<PathBuf>) -> Vec<(LocalProject, Option<LocalState>)> {
    let found = tauri::async_runtime::spawn_blocking(move || {
        folders
            .iter()
            .flat_map(|root| discovery::scan(root, discovery::DEFAULT_MAX_DEPTH))
            .collect::<Vec<_>>()
    })
    .await
    .unwrap_or_default();

    let mut tasks = JoinSet::new();
    for (i, project) in found.iter().enumerate() {
        let path = project.path.clone();
        tasks.spawn(async move { (i, local_state(&path).await) });
    }
    let mut states = vec![None; found.len()];
    while let Some(Ok((i, state))) = tasks.join_next().await {
        states[i] = state;
    }
    found.into_iter().zip(states).collect()
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

fn merge(remote: Vec<RemoteRepo>, locals: Vec<(LocalProject, Option<LocalState>)>) -> Vec<Project> {
    let mut by_id: BTreeMap<String, Project> = remote
        .into_iter()
        .map(|repo| {
            let id = key(&format!("{}/{}", repo.owner, repo.name));
            let project = Project {
                id: id.clone(),
                owner: Some(repo.owner.clone()),
                repo: Some(repo.name.clone()),
                config: repo.config.clone(),
                path: None,
                local: None,
                remote: Some(repo),
                assignment: None,
                pending: None,
            };
            (id, project)
        })
        .collect();

    for (clone, state) in locals {
        let id = match (&clone.owner, &clone.repo) {
            (Some(owner), Some(repo)) => key(&format!("{owner}/{repo}")),
            _ => clone.path.to_string_lossy().into_owned(),
        };
        match by_id.get_mut(&id) {
            // Two clones of one repo: the first one found is shown.
            Some(project) if project.path.is_some() => {}
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
                        config: clone.config,
                        path: Some(clone.path),
                        local: state,
                        remote: None,
                        assignment: None,
                        pending: None,
                    },
                );
            }
        }
    }
    by_id.into_values().collect()
}

/// Fetches each classroom's manifest once and attaches the matching entry to each project.
async fn attach_assignments(projects: &mut [Project], cache: &HttpCache, http: &reqwest::Client) {
    let mut manifests: HashMap<String, Option<Vec<Assignment>>> = HashMap::new();
    for project in projects.iter() {
        let Some(owner) = &project.owner else {
            continue;
        };
        let url = classroom50::assignments_url(
            owner,
            &project.config.classroom,
            project.config.secret.as_deref(),
        );
        manifests.entry(url).or_default();
    }
    let fetches = manifests.keys().map(|url| async move {
        let manifest = match cache.get_text(http, url).await {
            Ok(body) => classroom50::parse_assignments(url, &body)
                .inspect_err(|err| log::warn!("{err}"))
                .ok(),
            Err(err) => {
                log::warn!("could not fetch {url}: {err}");
                None
            }
        };
        (url.clone(), manifest)
    });
    let fetched: Vec<_> = join_all(fetches).await;
    manifests.extend(fetched);

    for project in projects {
        let Some(owner) = &project.owner else {
            continue;
        };
        let url = classroom50::assignments_url(
            owner,
            &project.config.classroom,
            project.config.secret.as_deref(),
        );
        project.assignment = manifests
            .get(&url)
            .and_then(Option::as_ref)
            .and_then(|list| list.iter().find(|a| a.slug == project.config.assignment))
            .cloned();
    }
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
    let closed =
        assignment.is_some_and(|a| a.closed) || project.remote.as_ref().is_some_and(|r| r.archived);
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
            config: config("hello"),
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
            .into_remote()
            .unwrap();
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
        assert!(plain.into_remote().is_none());
    }

    #[test]
    fn merges_clones_into_remote_repos() {
        let clone = LocalProject {
            path: "/code/hello".into(),
            owner: Some("cs50".into()),
            repo: Some("Fall-Hello-Alice".into()),
            config: config("hello"),
        };
        let orphan = LocalProject {
            path: "/code/scratch".into(),
            owner: None,
            repo: None,
            config: config("scratch"),
        };
        let projects = merge(
            vec![remote("fall-hello-alice", "aaa", None)],
            vec![(clone, Some(LocalState::default())), (orphan, None)],
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
            assignment: Some(assignment.clone()),
            pending: None,
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
