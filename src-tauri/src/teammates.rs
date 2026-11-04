//! Adding classmates in the background: `add_teammates` queues the usernames and returns,
//! and one task per row runs `gh student team add` or `invite` for them one by one. Rust
//! keeps every queue and sends `teammate-jobs` on each change, so the row shows where each
//! username stands even after the popover was hidden, and more can be queued meanwhile.

use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use serde::{Serialize, Serializer};
use tauri::{AppHandle, Manager};

use crate::alerts::Notice;
use crate::error::Error;
use crate::events::{IpcEvent, Notify, TeammateJobsChanged};
use crate::student::{self, Addition};

/// Where classmates are added, owned so the queue can outlive the command.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Target {
    Team {
        org: String,
        classroom: String,
        assignment: String,
        key: Option<String>,
    },
    Invite {
        repo: String,
        clone: Option<PathBuf>,
    },
}

impl Target {
    fn addition(&self) -> Addition<'_> {
        match self {
            Target::Team {
                org,
                classroom,
                assignment,
                key,
            } => Addition::Team {
                org,
                classroom,
                assignment,
                key: key.as_deref(),
            },
            Target::Invite { repo, clone } => Addition::Invite {
                repo,
                clone: clone.as_deref(),
            },
        }
    }
}

/// A group's size limit (`max_group_size`) and who is in it already, so a queue never
/// adds past it. Advisory upstream: gh-student's `team add` checks it too, but a founder
/// can pass it on GitHub.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Cap {
    pub max: u32,
    /// The members' logins as the last sync read them (the founder alone for a group
    /// just founded).
    pub members: Vec<String>,
}

impl Cap {
    /// Whether adding `username` would pass the limit: counts the members plus those this
    /// queue added since, each once. Someone already in counts as no addition.
    fn refuses(&self, username: &str, entries: &[TeammateEntry]) -> bool {
        let mut logins: Vec<String> = self.members.iter().map(|m| m.to_lowercase()).collect();
        for entry in entries.iter().filter(|e| e.state == AddState::Added) {
            logins.push(entry.username.to_lowercase());
        }
        logins.sort_unstable();
        logins.dedup();
        let login = username.to_lowercase();
        !logins.contains(&login) && logins.len() >= self.max as usize
    }
}

/// Where one username stands.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[cfg_attr(test, derive(specta::Type))]
#[serde(rename_all = "camelCase")]
pub enum AddState {
    Waiting,
    Adding,
    Added,
    Failed,
}

/// Why adding someone failed, shared so a job's view can be copied.
#[derive(Debug, Clone)]
pub struct Failure(Arc<Error>);

impl Serialize for Failure {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        self.0.serialize(serializer)
    }
}

#[cfg(test)]
impl specta::Type for Failure {
    fn definition(types: &mut specta::Types) -> specta::datatype::DataType {
        Error::definition(types)
    }
}

#[derive(Debug, Clone, Serialize)]
#[cfg_attr(test, derive(specta::Type))]
#[serde(rename_all = "camelCase")]
pub struct TeammateEntry {
    pub username: String,
    pub state: AddState,
    /// Why it failed, None unless `failed`.
    pub error: Option<Failure>,
}

/// The classmates queued for one row, as the UI shows them.
#[derive(Debug, Clone, Serialize)]
#[cfg_attr(test, derive(specta::Type))]
#[serde(rename_all = "camelCase")]
pub struct TeammateJob {
    /// What `add_teammates` was given: a project id, or the assignment of a group just
    /// founded whose repo no sync has listed yet.
    pub id: String,
    /// The project id of the repo (`owner/repo`, lowercase), so the row that replaces a
    /// just-accepted assignment's shows the same job.
    pub project: String,
    /// `owner/repo` as GitHub spells it.
    pub repo: String,
    /// The assignment's name, for the notification.
    pub assignment: String,
    /// Joins the group (`team add`) rather than inviting.
    pub team: bool,
    /// Whether usernames are still waiting or being added.
    pub running: bool,
    pub entries: Vec<TeammateEntry>,
}

struct Job {
    view: TeammateJob,
    target: Arc<Target>,
    /// The group's limit, as the newest call knew it.
    cap: Option<Cap>,
    /// Where the entries of the current run start, for its summary.
    run_start: usize,
}

/// What a run gave once its queue is empty: the logins added and those that failed.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Summary {
    pub added: Vec<String>,
    pub failed: Vec<String>,
}

/// What the worker does next.
#[derive(Debug, PartialEq, Eq)]
enum Next {
    Add(Arc<Target>, String),
    Done(Summary),
}

/// The queues, one per row.
#[derive(Default)]
pub struct TeammateJobs(Mutex<Vec<Job>>);

impl TeammateJobs {
    fn lock(&self) -> std::sync::MutexGuard<'_, Vec<Job>> {
        self.0.lock().unwrap_or_else(|e| e.into_inner())
    }

    pub fn list(&self) -> Vec<TeammateJob> {
        self.lock().iter().map(|job| job.view.clone()).collect()
    }

    /// Queues `usernames` on the job of `meta.id` (made with `target` when there is none),
    /// skipping one already waiting or being added. Returns whether a worker must start.
    fn enqueue(
        &self,
        meta: TeammateJob,
        target: Target,
        cap: Option<Cap>,
        usernames: Vec<String>,
    ) -> bool {
        let mut jobs = self.lock();
        let index = match jobs.iter().position(|job| job.view.id == meta.id) {
            Some(index) => index,
            None => {
                jobs.push(Job {
                    view: TeammateJob {
                        running: false,
                        entries: Vec::new(),
                        ..meta
                    },
                    target: Arc::new(target),
                    cap: None,
                    run_start: 0,
                });
                jobs.len() - 1
            }
        };
        let job = &mut jobs[index];
        if cap.is_some() {
            job.cap = cap;
        }
        let start = !job.view.running;
        if start {
            job.run_start = job.view.entries.len();
        }
        for username in usernames {
            let queued = job.view.entries.iter().any(|e| {
                matches!(e.state, AddState::Waiting | AddState::Adding)
                    && e.username.eq_ignore_ascii_case(&username)
            });
            if !queued {
                job.view.entries.push(TeammateEntry {
                    username,
                    state: AddState::Waiting,
                    error: None,
                });
            }
        }
        let waiting = job
            .view
            .entries
            .iter()
            .any(|e| e.state == AddState::Waiting);
        if start && waiting {
            job.view.running = true;
            return true;
        }
        false
    }

    /// Marks the next waiting username of job `id` as being added, or, with none left,
    /// ends the run and sums it up. One the group has no room for fails at once with
    /// `GroupFull`, without running gh.
    fn next(&self, id: &str) -> Next {
        let mut jobs = self.lock();
        let Some(job) = jobs.iter_mut().find(|job| job.view.id == id) else {
            return Next::Done(Summary::default());
        };
        while let Some(index) = job
            .view
            .entries
            .iter()
            .position(|e| e.state == AddState::Waiting)
        {
            let username = job.view.entries[index].username.clone();
            let full = job
                .cap
                .as_ref()
                .filter(|cap| cap.refuses(&username, &job.view.entries));
            let entry = &mut job.view.entries[index];
            if let Some(cap) = full {
                entry.state = AddState::Failed;
                entry.error = Some(Failure(Arc::new(Error::GroupFull { max: cap.max })));
                continue;
            }
            entry.state = AddState::Adding;
            return Next::Add(job.target.clone(), username);
        }
        job.view.running = false;
        let mut summary = Summary::default();
        for entry in job.view.entries.iter().skip(job.run_start) {
            match entry.state {
                AddState::Added => summary.added.push(entry.username.clone()),
                AddState::Failed => summary.failed.push(entry.username.clone()),
                AddState::Waiting | AddState::Adding => {}
            }
        }
        Next::Done(summary)
    }

    /// Records how adding `username` to job `id` went.
    fn settle(&self, id: &str, username: &str, result: crate::error::Result<()>) {
        let mut jobs = self.lock();
        let Some(job) = jobs.iter_mut().find(|job| job.view.id == id) else {
            return;
        };
        if let Some(entry) = job
            .view
            .entries
            .iter_mut()
            .find(|e| e.state == AddState::Adding && e.username == username)
        {
            match result {
                Ok(()) => entry.state = AddState::Added,
                Err(err) => {
                    entry.state = AddState::Failed;
                    entry.error = Some(Failure(Arc::new(err)));
                }
            }
        }
    }

    /// Drops the results of job `id` (Done); one still running keeps its queue. Returns
    /// whether anything changed.
    pub fn dismiss(&self, id: &str) -> bool {
        let mut jobs = self.lock();
        let before = jobs.len();
        let mut changed = false;
        for job in jobs.iter_mut().filter(|job| job.view.id == id) {
            let pending =
                |e: &TeammateEntry| matches!(e.state, AddState::Waiting | AddState::Adding);
            // The run's summary starts as many entries earlier as Done drops before it.
            let dropped_before = job.view.entries[..job.run_start]
                .iter()
                .filter(|e| !pending(e))
                .count();
            let kept = job.view.entries.len();
            job.view.entries.retain(pending);
            changed |= kept != job.view.entries.len();
            job.run_start -= dropped_before;
        }
        jobs.retain(|job| job.view.running || !job.view.entries.is_empty());
        changed || before != jobs.len()
    }

    /// Drops every job that is done, as the popover hides: it showed them already, or the
    /// notification did. Returns whether anything changed.
    pub fn clear_finished(&self) -> bool {
        let mut jobs = self.lock();
        let before = jobs.len();
        jobs.retain(|job| job.view.running);
        before != jobs.len()
    }
}

pub fn emit(app: &AppHandle) {
    let jobs = app.state::<TeammateJobs>().list();
    if let Err(err) = TeammateJobsChanged::emit(app, &jobs) {
        log::warn!("could not emit {}: {err}", TeammateJobsChanged::NAME);
    }
}

/// Queues `usernames` for the row of `meta` and starts adding them unless that row's queue
/// is already running.
pub fn enqueue(
    app: &AppHandle,
    meta: TeammateJob,
    target: Target,
    cap: Option<Cap>,
    usernames: Vec<String>,
) {
    let id = meta.id.clone();
    let start = app
        .state::<TeammateJobs>()
        .enqueue(meta, target, cap, usernames);
    emit(app);
    if start {
        let app = app.clone();
        tauri::async_runtime::spawn(async move { work(&app, &id).await });
    }
}

/// Adds the queued usernames of job `id` one by one, then reads the group again and, with
/// the popover hidden, says how it went.
async fn work(app: &AppHandle, id: &str) {
    let jobs = app.state::<TeammateJobs>();
    let summary = loop {
        match jobs.next(id) {
            Next::Add(target, username) => {
                emit(app);
                let result = student::add(&target.addition(), &username).await;
                if let Err(err) = &result {
                    log::warn!("adding a classmate failed: {:?}", err.error_kind());
                }
                jobs.settle(id, &username, result);
                emit(app);
            }
            Next::Done(summary) => break summary,
        }
    };
    emit(app);
    let view = jobs.list().into_iter().find(|job| job.id == id);
    if !summary.added.is_empty() {
        let app = app.clone();
        tauri::async_runtime::spawn(async move {
            crate::projects::refresh_after_write(&app).await;
        });
    }
    if let Some(view) = view
        && !crate::popover::is_visible(app)
        && !(summary.added.is_empty() && summary.failed.is_empty())
    {
        let notice = Notice::TeammatesAdded {
            assignment: view.assignment,
            team: view.team,
            added: summary.added,
            failed: summary.failed,
        };
        if let Err(err) = Notify::emit(app, &notice) {
            log::warn!("could not emit {}: {err}", Notify::NAME);
        }
    }
}

/// After the popover hid: forgets what it showed.
pub fn popover_hidden(app: &AppHandle) {
    if app.state::<TeammateJobs>().clear_finished() {
        emit(app);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn meta(id: &str) -> TeammateJob {
        TeammateJob {
            id: id.into(),
            project: id.into(),
            repo: "org/c-lab-1-group-1".into(),
            assignment: "Lab 1".into(),
            team: true,
            running: true,
            entries: vec![TeammateEntry {
                username: "ignored".into(),
                state: AddState::Added,
                error: None,
            }],
        }
    }

    fn target() -> Target {
        Target::Team {
            org: "org".into(),
            classroom: "c".into(),
            assignment: "lab-1".into(),
            key: None,
        }
    }

    fn names(list: &[&str]) -> Vec<String> {
        list.iter().map(|s| s.to_string()).collect()
    }

    fn states(jobs: &TeammateJobs) -> Vec<(String, AddState)> {
        jobs.list()[0]
            .entries
            .iter()
            .map(|e| (e.username.clone(), e.state))
            .collect()
    }

    fn add(jobs: &TeammateJobs, ok: bool) -> Option<String> {
        match jobs.next("a") {
            Next::Add(_, user) => {
                let result = if ok {
                    Ok(())
                } else {
                    Err(Error::Student("no such user".into()))
                };
                jobs.settle("a", &user, result);
                Some(user)
            }
            Next::Done(_) => None,
        }
    }

    #[test]
    fn adds_the_queue_in_order_and_sums_up_the_run() {
        let jobs = TeammateJobs::default();
        assert!(jobs.enqueue(meta("a"), target(), None, names(&["alice", "bob"])));
        let job = &jobs.list()[0];
        // A new job starts empty, whatever the template carried.
        assert!(job.running);
        assert_eq!(job.entries.len(), 2);

        assert!(matches!(jobs.next("a"), Next::Add(t, u) if *t == target() && u == "alice"));
        assert_eq!(
            states(&jobs),
            vec![
                ("alice".into(), AddState::Adding),
                ("bob".into(), AddState::Waiting)
            ]
        );
        jobs.settle("a", "alice", Ok(()));
        assert_eq!(add(&jobs, false).as_deref(), Some("bob"));
        assert_eq!(
            jobs.next("a"),
            Next::Done(Summary {
                added: names(&["alice"]),
                failed: names(&["bob"]),
            })
        );
        let job = &jobs.list()[0];
        assert!(!job.running);
        assert_eq!(
            serde_json::to_value(job.entries[1].error.as_ref().unwrap()).unwrap()["kind"],
            "student"
        );
    }

    #[test]
    fn more_usernames_join_the_running_queue() {
        let jobs = TeammateJobs::default();
        assert!(jobs.enqueue(meta("a"), target(), None, names(&["alice"])));
        assert!(matches!(jobs.next("a"), Next::Add(..)));
        // A second batch while alice is being added: no second worker, bob waits his turn,
        // and alice isn't queued twice.
        assert!(!jobs.enqueue(meta("a"), target(), None, names(&["Alice", "bob"])));
        assert_eq!(
            states(&jobs),
            vec![
                ("alice".into(), AddState::Adding),
                ("bob".into(), AddState::Waiting)
            ]
        );
        jobs.settle("a", "alice", Ok(()));
        assert_eq!(add(&jobs, true).as_deref(), Some("bob"));
        assert_eq!(
            jobs.next("a"),
            Next::Done(Summary {
                added: names(&["alice", "bob"]),
                failed: vec![],
            })
        );
    }

    #[test]
    fn a_new_run_reports_only_its_own_usernames() {
        let jobs = TeammateJobs::default();
        jobs.enqueue(meta("a"), target(), None, names(&["alice"]));
        add(&jobs, true);
        assert!(matches!(jobs.next("a"), Next::Done(_)));
        // Add another after it finished: a new worker, and the earlier result stays shown.
        assert!(jobs.enqueue(meta("a"), target(), None, names(&["alice", "carol"])));
        add(&jobs, false);
        add(&jobs, true);
        assert_eq!(
            jobs.next("a"),
            Next::Done(Summary {
                added: names(&["carol"]),
                failed: names(&["alice"]),
            })
        );
        assert_eq!(jobs.list()[0].entries.len(), 3);
    }

    #[test]
    fn keeps_the_first_target_of_a_row() {
        let jobs = TeammateJobs::default();
        jobs.enqueue(meta("a"), target(), None, names(&["alice"]));
        let invite = Target::Invite {
            repo: "o/r".into(),
            clone: None,
        };
        jobs.enqueue(meta("a"), invite, None, names(&["bob"]));
        assert!(matches!(jobs.next("a"), Next::Add(t, _) if *t == target()));
    }

    #[test]
    fn rows_have_their_own_queues() {
        let jobs = TeammateJobs::default();
        assert!(jobs.enqueue(meta("a"), target(), None, names(&["alice"])));
        assert!(jobs.enqueue(meta("b"), target(), None, names(&["alice"])));
        assert_eq!(jobs.list().len(), 2);
        assert_eq!(jobs.next("unknown"), Next::Done(Summary::default()));
    }

    #[test]
    fn queues_nothing_without_usernames() {
        let jobs = TeammateJobs::default();
        assert!(!jobs.enqueue(meta("a"), target(), None, vec![]));
        assert!(!jobs.list()[0].running);
        assert!(jobs.clear_finished());
        assert!(jobs.list().is_empty());
    }

    #[test]
    fn done_drops_results_but_never_a_running_queue() {
        let jobs = TeammateJobs::default();
        jobs.enqueue(meta("a"), target(), None, names(&["alice", "bob"]));
        add(&jobs, true);
        assert!(jobs.dismiss("a"));
        assert_eq!(states(&jobs), vec![("bob".into(), AddState::Waiting)]);
        assert!(!jobs.clear_finished());
        add(&jobs, true);
        // The summary still covers the run, minus what Done dropped.
        assert_eq!(
            jobs.next("a"),
            Next::Done(Summary {
                added: names(&["bob"]),
                failed: vec![],
            })
        );
        assert!(jobs.dismiss("a"));
        assert!(jobs.list().is_empty());
        assert!(!jobs.dismiss("a"));
    }

    fn cap(max: u32, members: &[&str]) -> Option<Cap> {
        Some(Cap {
            max,
            members: names(members),
        })
    }

    fn kind(jobs: &TeammateJobs, index: usize) -> serde_json::Value {
        let entry = &jobs.list()[0].entries[index];
        serde_json::to_value(entry.error.as_ref().unwrap()).unwrap()
    }

    #[test]
    fn refuses_adding_past_the_group_size() {
        let jobs = TeammateJobs::default();
        // Two of three already: one more fits, the next is refused without running gh.
        jobs.enqueue(
            meta("a"),
            target(),
            cap(3, &["me", "alice"]),
            names(&["bob", "carol"]),
        );
        assert_eq!(add(&jobs, true).as_deref(), Some("bob"));
        assert_eq!(
            jobs.next("a"),
            Next::Done(Summary {
                added: names(&["bob"]),
                failed: names(&["carol"]),
            })
        );
        let error = kind(&jobs, 1);
        assert_eq!(error["kind"], "groupFull");
        assert_eq!(error["params"]["max"], "3");
        assert_eq!(error["message"], "your group is full (maximum: 3)");
    }

    #[test]
    fn counts_those_added_earlier_in_the_queue_once() {
        let jobs = TeammateJobs::default();
        jobs.enqueue(meta("a"), target(), cap(3, &["me"]), names(&["alice"]));
        add(&jobs, true);
        assert!(matches!(jobs.next("a"), Next::Done(_)));
        // Add another, after a sync that already lists alice: she counts once, so bob fits
        // and carol doesn't. Someone already in isn't an addition, whatever the count.
        jobs.enqueue(
            meta("a"),
            target(),
            cap(3, &["me", "Alice"]),
            names(&["bob", "carol", "ALICE"]),
        );
        assert_eq!(add(&jobs, true).as_deref(), Some("bob"));
        assert_eq!(add(&jobs, true).as_deref(), Some("ALICE"));
        assert_eq!(
            jobs.next("a"),
            Next::Done(Summary {
                added: names(&["bob", "ALICE"]),
                failed: names(&["carol"]),
            })
        );
        assert_eq!(kind(&jobs, 2)["kind"], "groupFull");
    }

    #[test]
    fn an_oversized_group_takes_no_one_and_no_cap_takes_anyone() {
        let jobs = TeammateJobs::default();
        jobs.enqueue(
            meta("a"),
            target(),
            cap(2, &["me", "a", "b"]),
            names(&["c"]),
        );
        assert_eq!(
            jobs.next("a"),
            Next::Done(Summary {
                added: vec![],
                failed: names(&["c"]),
            })
        );
        jobs.enqueue(meta("b"), target(), None, names(&["c", "d", "e"]));
        assert_eq!(states(&jobs).len(), 1);
        for _ in 0..3 {
            assert!(matches!(jobs.next("b"), Next::Add(..)));
            let user = jobs.list()[1]
                .entries
                .iter()
                .find(|e| e.state == AddState::Adding)
                .unwrap()
                .username
                .clone();
            jobs.settle("b", &user, Ok(()));
        }
        assert!(matches!(jobs.next("b"), Next::Done(s) if s.added.len() == 3));
    }

    #[test]
    fn hiding_the_popover_drops_finished_jobs_only() {
        let jobs = TeammateJobs::default();
        jobs.enqueue(meta("a"), target(), None, names(&["alice"]));
        jobs.enqueue(meta("b"), target(), None, names(&["bob"]));
        add(&jobs, true);
        assert!(matches!(jobs.next("a"), Next::Done(_)));
        assert!(jobs.clear_finished());
        let left: Vec<String> = jobs.list().into_iter().map(|j| j.id).collect();
        assert_eq!(left, vec!["b".to_string()]);
    }
}
