//! The merged project list. Classrooms come from the user's classroom teams and from
//! `.classroom50.yaml` files; a repo (on GitHub or cloned in a watched folder) belongs to
//! a classroom through that file or, for assignments without an autograder, which commit
//! no file, through the `<classroom>-<assignment>-...` naming rule checked against the
//! classroom's published manifest. Everything is joined by `owner/repo`. A sync runs on a timer, when the popover opens and on demand; the result
//! is pushed to the UI as the `projects-updated` event.
//!
//! The parts: `identify` (which assignment a repo is), `renames` (ids before a rename),
//! `build` and `merge` (one list from GitHub, the classes and the clones), `remote` (GitHub
//! discovery), `statuses` (assignment status and grades), `scan` and `git` (clones on disk),
//! `snapshot` (what a sync shows), `schedule` (when a sync is due), `sync` and `progress`
//! (running one).

mod build;
#[cfg(test)]
mod fixtures;
mod git;
mod identify;
mod merge;
mod progress;
mod remote;
mod renames;
mod scan;
mod schedule;
mod snapshot;
mod statuses;
mod sync;

use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::classroom50::{self, Assignment, Classroom, RepoConfig};
use crate::error::{Error, Result};
use crate::github::{self, HttpCache};
use crate::groups::Group;
use crate::status::{Branch, Feedback, Grade, GradeSummary, Grading};
use crate::sync_progress::Costs;

pub use renames::{previous_id, previous_unaccepted_id, renamed_ids, unaccepted_key};
pub use schedule::{refresh_if_stale, start_background_sync};
pub use sync::{refresh, refresh_after_push, refresh_after_write, refresh_local};

// What the parts share, so each imports it from here.
use build::{Sources, build};
use git::{check_clones, commit_emails, local_states};
use identify::{Found, identify, identify_by, without_doubtful_templates};
use merge::{late_seconds, merge, pending, sort_key};
use progress::Progress;
use remote::{REPO_FRAGMENT, RepoNode, fetch_nodes, sync_remote};
use renames::renamed_clone;
use scan::{scan_local, watch_targets};
use schedule::may_call_github;
use snapshot::{annotate, full_snapshot, local_snapshot};
use statuses::{StatusAuth, keep_status, refresh_statuses, status_auth};
use sync::{Mode, refresh_in_background};

/// v2 keeps every inspected repo, not only those with a marker file.
const CACHE_FILE: &str = "repos-v2.json";
/// The last snapshot and sync stats, so a restart shows everything at once and doesn't
/// sync again before it's due.
const STATE_FILE: &str = "snapshot.json";
/// A class's assignment list is re-downloaded at most this often (it's on GitHub Pages,
/// and rarely changes).
const MANIFEST_MAX_AGE: chrono::TimeDelta = chrono::TimeDelta::minutes(10);

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(test, derive(specta::Type))]
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
#[cfg_attr(test, derive(specta::Type))]
#[serde(rename_all = "camelCase")]
pub struct Submission {
    pub tag: String,
    pub commit: String,
    /// Parsed from the tag name; `None` for hand-made tags that don't follow the format.
    pub at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(test, derive(specta::Type))]
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
#[cfg_attr(test, derive(specta::Type))]
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
#[cfg_attr(test, derive(specta::Type))]
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
#[cfg_attr(test, derive(specta::Type))]
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
#[cfg_attr(test, derive(specta::Type))]
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
#[cfg_attr(test, derive(specta::Type))]
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
#[cfg_attr(test, derive(specta::Type))]
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
#[cfg_attr(test, derive(specta::Type))]
#[serde(rename_all = "camelCase")]
pub struct SyncProgress {
    pub running: bool,
    /// Started by the user (refresh button, rescan, sign-in). Background checks run
    /// without a spinner.
    pub foreground: bool,
    /// From 0 to 1, never lower than the last report of the same sync.
    #[cfg_attr(test, specta(type = specta_typescript::Number))]
    pub fraction: f32,
}

/// How the last sync of a kind went, for the sync details in Settings.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[cfg_attr(test, derive(specta::Type))]
#[serde(rename_all = "camelCase")]
pub struct LastRun {
    pub at: DateTime<Utc>,
    #[cfg_attr(test, specta(type = specta_typescript::Number))]
    pub seconds: f32,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[cfg_attr(test, derive(specta::Type))]
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
    /// When the last Clear cache finished rebuilding the list.
    cleared_at: Mutex<Option<DateTime<Utc>>>,
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
            cleared_at: Mutex::default(),
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

    /// When the last Clear cache finished, for its cooldown.
    pub fn cleared_at(&self) -> Option<DateTime<Utc>> {
        self.cleared_at.lock().ok().and_then(|at| *at)
    }

    pub fn set_cleared_at(&self, at: DateTime<Utc>) {
        if let Ok(mut cleared) = self.cleared_at.lock() {
            *cleared = Some(at);
        }
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

type ClassroomKey = (String, String);

fn key(name_with_owner: &str) -> String {
    name_with_owner.to_ascii_lowercase()
}
