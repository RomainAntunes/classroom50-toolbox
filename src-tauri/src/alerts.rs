//! Turns snapshot changes into notifications and the tray badge (a teammate's push only): deadline reminders, new
//! grades, grading that failed, new feedback and newly published assignments. What was already announced is
//! remembered in `alerts.json` (app cache dir) so a restart doesn't repeat it, and the
//! first snapshot only records a baseline. The UI formats and sends the notifications
//! (`notify` event), so the text goes through i18n.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use chrono::{DateTime, TimeDelta, Utc};
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter, Manager};

use crate::classroom50::SubmissionMode;
use crate::github;
use crate::projects::{Project, Snapshot};
use crate::settings::SettingsStore;
use crate::status::Grading;
use crate::tray;

pub const NOTIFY_EVENT: &str = "notify";
pub const UNSEEN_EVENT: &str = "unseen-updated";

const FILE_NAME: &str = "alerts.json";
/// A grade or comment first seen for a repo is announced only if this recent; older ones
/// predate the app.
const FRESH: TimeDelta = TimeDelta::days(1);

/// A notification for the UI to word.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum Notice {
    Due {
        assignment: String,
        due: DateTime<Utc>,
    },
    Grade {
        assignment: String,
        score: u32,
        max_score: u32,
    },
    /// The autograder couldn't grade the last push (not failing tests: those are a grade).
    GradingFailed {
        assignment: String,
    },
    Feedback {
        assignment: String,
        count: u32,
    },
    NewAssignment {
        assignment: String,
        classroom: String,
    },
    TeammatePush {
        assignment: String,
        author: String,
    },
    /// Classmates joined the user's group (names, else logins).
    GroupJoined {
        assignment: String,
        members: Vec<String>,
    },
    /// `due` is `None` when the deadline was removed.
    DeadlineChanged {
        assignment: String,
        due: Option<DateTime<Utc>>,
    },
    Closed {
        assignment: String,
    },
    /// A new version of the app is downloaded and installs on restart (`updater.rs`).
    UpdateReady {
        version: String,
    },
}

#[derive(Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
struct Memory {
    /// The account this memory belongs to; another account starts a new baseline.
    login: Option<String>,
    /// Assignment slugs seen per `org/classroom`.
    assignments: BTreeMap<String, BTreeSet<String>>,
    /// Graded `submit/*` tag per project.
    grades: BTreeMap<String, String>,
    /// Default-branch head whose failed grading was announced, per project.
    grading_failed: BTreeMap<String, String>,
    /// Feedback PR comment count per project.
    feedback: BTreeMap<String, u32>,
    /// Default-branch head per project.
    heads: BTreeMap<String, String>,
    /// Group member logins (lowercase) per project.
    groups: BTreeMap<String, BTreeSet<String>>,
    /// Due date (Unix seconds, `None` for no deadline) per `org/classroom/assignment`.
    dues: BTreeMap<String, Option<i64>>,
    /// Assignments seen closed, by the same key.
    closed: BTreeSet<String>,
    /// `<project>|<due>|<hours>` reminders already sent.
    reminded: BTreeSet<String>,
    /// Project and unaccepted-assignment ids with news the user hasn't opened the popover
    /// on yet. They light the badge and get a dot in the list.
    unseen: BTreeSet<String>,
    /// The subset of `unseen` that lights the tray badge: a teammate pushed.
    badge: BTreeSet<String>,
}

pub struct AlertsState {
    path: PathBuf,
    memory: Mutex<Memory>,
    badge: Mutex<Option<bool>>,
}

impl AlertsState {
    pub fn load(cache_dir: &Path) -> Self {
        let path = cache_dir.join(FILE_NAME);
        let memory = std::fs::read_to_string(&path)
            .ok()
            .and_then(|json| serde_json::from_str(&json).ok())
            .unwrap_or_default();
        Self {
            path,
            memory: Mutex::new(memory),
            badge: Mutex::new(None),
        }
    }

    pub fn unseen(&self) -> Vec<String> {
        self.memory
            .lock()
            .map(|m| m.unseen.iter().cloned().collect())
            .unwrap_or_default()
    }

    fn save(&self, memory: &Memory) {
        match serde_json::to_vec(memory) {
            Ok(json) => {
                if let Err(err) = github::write_atomic(&self.path, &json) {
                    log::warn!("could not save {}: {err}", self.path.display());
                }
            }
            Err(err) => log::warn!("could not encode alerts: {err}"),
        }
    }
}

/// Called after every sync with the new snapshot.
pub fn process(app: &AppHandle, snapshot: &Snapshot) {
    // Before the first GitHub read (or signed out) there's nothing to compare.
    if snapshot.checked_at.is_none() {
        return;
    }
    let settings = app.state::<SettingsStore>().get();
    let hours = settings.notify_hours_before;
    let snapshot = &without_hidden(snapshot, &settings.hidden, &settings.hidden_classes);
    let state = app.state::<AlertsState>();
    let Ok(mut memory) = state.memory.lock() else {
        return;
    };
    if snapshot.login.is_some() && memory.login != snapshot.login {
        *memory = Memory {
            login: snapshot.login.clone(),
            ..Memory::default()
        };
    }
    let notices = diff(&mut memory, snapshot, &hours, Utc::now());
    state.save(&memory);
    let unseen: Vec<String> = memory.unseen.iter().cloned().collect();
    let attention = !memory.badge.is_empty();
    drop(memory);

    for notice in notices {
        if let Err(err) = app.emit(NOTIFY_EVENT, &notice) {
            log::warn!("could not emit {NOTIFY_EVENT}: {err}");
        }
    }
    let _ = app.emit(UNSEEN_EVENT, &unseen);
    set_badge(app, attention);
}

/// The snapshot minus hidden assignments and classes, which stay quiet; what changed
/// meanwhile is told once they're shown again.
fn without_hidden(snapshot: &Snapshot, hidden: &[String], classes: &[String]) -> Snapshot {
    let class_hidden = |owner: &str, classroom: &str| {
        let key = format!("{owner}/{classroom}").to_lowercase();
        classes.contains(&key)
    };
    let mut visible = snapshot.clone();
    visible.projects.retain(|p| {
        !hidden.contains(&p.id)
            && !class_hidden(p.owner.as_deref().unwrap_or(""), &p.config.classroom)
    });
    visible
        .unaccepted
        .retain(|u| !hidden.contains(&u.id) && !class_hidden(&u.classroom.org, &u.classroom.slug));
    visible
}

/// At startup: the badge from the saved news, before any sync.
pub fn restore_badge(app: &AppHandle) {
    let state = app.state::<AlertsState>();
    let on = state.memory.lock().is_ok_and(|m| !m.badge.is_empty());
    set_badge(app, on);
}

/// The popover was open, so its news has been seen.
pub fn mark_seen(app: &AppHandle) {
    let state = app.state::<AlertsState>();
    let Ok(mut memory) = state.memory.lock() else {
        return;
    };
    if memory.unseen.is_empty() {
        return;
    }
    memory.unseen.clear();
    memory.badge.clear();
    state.save(&memory);
    drop(memory);
    let _ = app.emit(UNSEEN_EVENT, Vec::<String>::new());
    set_badge(app, false);
}

fn set_badge(app: &AppHandle, on: bool) {
    let state = app.state::<AlertsState>();
    let Ok(mut current) = state.badge.lock() else {
        return;
    };
    if *current != Some(on) {
        *current = Some(on);
        tray::set_badge(app, on);
    }
}

/// Updates `memory` and returns what to announce.
fn diff(
    memory: &mut Memory,
    snapshot: &Snapshot,
    hours_before: &[u32],
    now: DateTime<Utc>,
) -> Vec<Notice> {
    let mut notices = Vec::new();
    let fresh = |at: Option<DateTime<Utc>>| at.is_some_and(|at| now - at < FRESH);

    // Newly published assignments, per classroom. A classroom seen for the first time
    // only records its list.
    let mut catalog: BTreeMap<String, Vec<(String, String, String, String)>> = BTreeMap::new();
    for project in &snapshot.projects {
        let Some(owner) = &project.owner else {
            continue;
        };
        let classroom = classroom_key(owner, &project.config.classroom);
        catalog.entry(classroom).or_default().push((
            project.config.assignment.clone(),
            assignment_name(project),
            classroom_name(project),
            project.id.clone(),
        ));
    }
    for item in &snapshot.unaccepted {
        let classroom = classroom_key(&item.classroom.org, &item.classroom.slug);
        catalog.entry(classroom).or_default().push((
            item.assignment.slug.clone(),
            item.assignment.name.clone(),
            item.classroom
                .name
                .clone()
                .unwrap_or_else(|| item.classroom.slug.clone()),
            item.id.clone(),
        ));
    }
    for (classroom, assignments) in catalog {
        let first_time = !memory.assignments.contains_key(&classroom);
        let known = memory.assignments.entry(classroom).or_default();
        for (slug, name, classroom_name, id) in assignments {
            if known.insert(slug) && !first_time {
                notices.push(Notice::NewAssignment {
                    assignment: name,
                    classroom: classroom_name,
                });
                memory.unseen.insert(id);
            }
        }
    }

    // Deadline moves and closings, for accepted and unaccepted assignments alike.
    let schedule = snapshot
        .projects
        .iter()
        .filter_map(|p| {
            let assignment = p.assignment.as_ref()?;
            let key = format!(
                "{}/{}",
                classroom_key(p.owner.as_deref()?, &p.config.classroom),
                p.config.assignment.to_ascii_lowercase()
            );
            Some((key, assignment))
        })
        .chain(
            snapshot
                .unaccepted
                .iter()
                .map(|u| (u.id.clone(), &u.assignment)),
        );
    for (key, assignment) in schedule {
        let due = assignment.due.map(|d| d.timestamp());
        let previous = memory.dues.insert(key.clone(), due);
        if assignment.closed {
            if memory.closed.insert(key) && previous.is_some() {
                notices.push(Notice::Closed {
                    assignment: assignment.name.clone(),
                });
            }
        } else if previous.is_some_and(|before| before != due) {
            notices.push(Notice::DeadlineChanged {
                assignment: assignment.name.clone(),
                due: assignment.due.map(|d| d.with_timezone(&Utc)),
            });
        }
    }

    let login = memory.login.clone().unwrap_or_default();
    for project in &snapshot.projects {
        if let Some(group) = &project.group {
            let members: BTreeSet<String> = group
                .members
                .iter()
                .map(|m| m.login.to_ascii_lowercase())
                .collect();
            let previous = memory.groups.insert(project.id.clone(), members);
            if let Some(before) = previous {
                let joined: Vec<String> = group
                    .members
                    .iter()
                    .filter(|m| {
                        let lower = m.login.to_ascii_lowercase();
                        !before.contains(&lower) && !lower.eq_ignore_ascii_case(&login)
                    })
                    .map(|m| m.name.clone().unwrap_or_else(|| m.login.clone()))
                    .collect();
                if !joined.is_empty() {
                    notices.push(Notice::GroupJoined {
                        assignment: assignment_name(project),
                        members: joined,
                    });
                    memory.unseen.insert(project.id.clone());
                }
            }
        }

        let Some(remote) = &project.remote else {
            continue;
        };
        let name = assignment_name(project);

        if let Some(head) = &remote.head {
            let previous = memory.heads.insert(project.id.clone(), head.oid.clone());
            let teammate = head
                .author
                .as_ref()
                .filter(|author| !login.is_empty() && !author.eq_ignore_ascii_case(&login));
            if let (Some(before), Some(author)) = (previous, teammate)
                && before != head.oid
            {
                notices.push(Notice::TeammatePush {
                    assignment: name.clone(),
                    author: author.clone(),
                });
                memory.unseen.insert(project.id.clone());
                memory.badge.insert(project.id.clone());
            }
        }

        if let Some(grade) = &remote.grade {
            let previous = memory.grades.insert(project.id.clone(), grade.tag.clone());
            let new = match previous {
                Some(tag) => tag != grade.tag,
                None => fresh(grade.published_at),
            };
            if new {
                notices.push(Notice::Grade {
                    assignment: name.clone(),
                    score: grade.score,
                    max_score: grade.max_score,
                });
                memory.unseen.insert(project.id.clone());
            }
        }

        if let (Some(Grading::Failed { .. }), Some(head)) = (&remote.grading, &remote.head) {
            let previous = memory
                .grading_failed
                .insert(project.id.clone(), head.oid.clone());
            if previous.as_ref() != Some(&head.oid) && fresh(head.at) {
                notices.push(Notice::GradingFailed {
                    assignment: name.clone(),
                });
                memory.unseen.insert(project.id.clone());
            }
        }

        if let Some(feedback) = &remote.feedback {
            let previous = memory
                .feedback
                .insert(project.id.clone(), feedback.comments);
            let added = match previous {
                Some(count) => feedback.comments.saturating_sub(count),
                None if fresh(feedback.latest_at) => feedback.comments,
                None => 0,
            };
            if added > 0 {
                notices.push(Notice::Feedback {
                    assignment: name.clone(),
                    count: added,
                });
                memory.unseen.insert(project.id.clone());
            }
        }

        if let Some(due) = open_due(project) {
            // The tightest reminder already reached; the wider ones are skipped with it,
            // so a late first sync sends one reminder, not one per setting.
            let mut reached: Vec<u32> = hours_before
                .iter()
                .copied()
                .filter(|h| due > now && due - now <= TimeDelta::hours(i64::from(*h)))
                .collect();
            reached.sort_unstable();
            if let Some(&tightest) = reached.first() {
                let key = |h: u32| format!("{}|{}|{h}", project.id, due.timestamp());
                if needs_work(project) && !memory.reminded.contains(&key(tightest)) {
                    notices.push(Notice::Due {
                        assignment: name.clone(),
                        due,
                    });
                }
                memory.reminded.extend(reached.into_iter().map(key));
            }
        }
    }

    // Reminders for deadlines that have passed can go.
    memory.reminded.retain(|key| {
        key.split('|')
            .nth(1)
            .and_then(|t| t.parse::<i64>().ok())
            .and_then(|t| DateTime::from_timestamp(t, 0))
            .is_some_and(|due| due > now)
    });
    notices
}

/// The due date of an assignment that can still take work.
pub(crate) fn open_due(project: &Project) -> Option<DateTime<Utc>> {
    let assignment = project.assignment.as_ref()?;
    let archived = project.remote.as_ref().is_some_and(|r| r.archived);
    if assignment.closed || archived {
        return None;
    }
    assignment.due.map(|due| due.with_timezone(&Utc))
}

/// Whether the assignment may still need the student: anything pending, or no clone to
/// tell (unless a tag-mode submission covers the remote head).
pub(crate) fn needs_work(project: &Project) -> bool {
    if project.pending.is_some() {
        return true;
    }
    if project.path.is_some() {
        return false;
    }
    let tag_mode = project
        .assignment
        .as_ref()
        .is_some_and(|a| a.submission_mode == Some(SubmissionMode::Tag));
    let submitted_head = project.remote.as_ref().is_some_and(|r| {
        r.head.as_ref().is_some_and(|head| {
            r.last_submission
                .as_ref()
                .is_some_and(|s| s.commit == head.oid)
        })
    });
    !(tag_mode && submitted_head)
}

fn classroom_key(org: &str, classroom: &str) -> String {
    format!("{org}/{classroom}").to_ascii_lowercase()
}

fn assignment_name(project: &Project) -> String {
    project
        .assignment
        .as_ref()
        .map_or_else(|| project.config.assignment.clone(), |a| a.name.clone())
}

fn classroom_name(project: &Project) -> String {
    project
        .classroom
        .as_ref()
        .and_then(|c| c.name.clone())
        .unwrap_or_else(|| project.config.classroom.clone())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::classroom50::{Assignment, RepoConfig};
    use crate::projects::RemoteRepo;
    use crate::status::{Feedback, Grade};

    fn assignment(slug: &str, due: Option<DateTime<Utc>>) -> Assignment {
        let mut assignment: Assignment = serde_json::from_value(serde_json::json!({
            "slug": slug, "name": slug.to_uppercase(), "mode": "individual"
        }))
        .unwrap();
        assignment.due = due.map(|d| d.fixed_offset());
        assignment
    }

    fn project(slug: &str, due: Option<DateTime<Utc>>) -> Project {
        let remote: RemoteRepo = serde_json::from_value(serde_json::json!({
            "owner": "org", "name": format!("c-{slug}-alice"), "url": "u",
            "archived": false, "canPush": true, "defaultBranch": "main",
            "head": null, "lastSubmission": null, "config": null
        }))
        .unwrap();
        Project {
            id: format!("org/c-{slug}-alice"),
            owner: Some("org".into()),
            repo: Some(remote.name.clone()),
            config: RepoConfig {
                classroom: "c".into(),
                assignment: slug.into(),
                secret: None,
            },
            path: None,
            local: None,
            remote: Some(remote),
            classroom: None,
            assignment: Some(assignment(slug, due)),
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
            checked_at: Some(Utc::now()),
            ..Snapshot::default()
        }
    }

    #[test]
    fn first_snapshot_is_a_silent_baseline() {
        let now = Utc::now();
        let mut memory = Memory::default();
        let mut graded = project("lab1", None);
        graded.remote.as_mut().unwrap().grade = Some(Grade {
            tag: "submit/old".into(),
            score: 5,
            max_score: 10,
            tests_passed: 1,
            tests_total: 2,
            tests: Vec::new(),
            published_at: Some(now - TimeDelta::days(30)),
            url: "u".into(),
            release_notes: None,
        });
        let notices = diff(&mut memory, &snapshot(vec![graded]), &[24, 1], now);
        assert!(notices.is_empty());
        assert!(memory.unseen.is_empty());
    }

    #[test]
    fn announces_new_grades_feedback_and_assignments() {
        let now = Utc::now();
        let mut memory = Memory::default();
        diff(
            &mut memory,
            &snapshot(vec![project("lab1", None)]),
            &[],
            now,
        );

        let mut lab1 = project("lab1", None);
        let remote = lab1.remote.as_mut().unwrap();
        remote.grade = Some(Grade {
            tag: "submit/new".into(),
            score: 8,
            max_score: 10,
            tests_passed: 4,
            tests_total: 5,
            tests: Vec::new(),
            published_at: Some(now),
            url: "u".into(),
            release_notes: None,
        });
        remote.feedback = Some(Feedback {
            url: "u".into(),
            comments: 2,
            latest_at: Some(now),
        });
        let notices = diff(
            &mut memory,
            &snapshot(vec![lab1, project("lab2", None)]),
            &[],
            now,
        );
        assert_eq!(
            notices,
            vec![
                Notice::NewAssignment {
                    assignment: "LAB2".into(),
                    classroom: "c".into()
                },
                Notice::Grade {
                    assignment: "LAB1".into(),
                    score: 8,
                    max_score: 10
                },
                Notice::Feedback {
                    assignment: "LAB1".into(),
                    count: 2
                },
            ]
        );
        assert!(memory.unseen.contains("org/c-lab1-alice"));
    }

    #[test]
    fn announces_failed_grading_once_per_push() {
        let now = Utc::now();
        let mut memory = Memory::default();
        let failed = |oid: &str| {
            let mut lab1 = project("lab1", None);
            let remote = lab1.remote.as_mut().unwrap();
            remote.head = Some(crate::projects::Commit {
                oid: oid.into(),
                at: Some(now),
                author: None,
            });
            remote.grading = Some(Grading::Failed {
                url: None,
                message: None,
            });
            snapshot(vec![lab1])
        };
        let expected = vec![Notice::GradingFailed {
            assignment: "LAB1".into(),
        }];
        assert_eq!(diff(&mut memory, &failed("a"), &[], now), expected);
        assert!(diff(&mut memory, &failed("a"), &[], now).is_empty());
        assert_eq!(diff(&mut memory, &failed("b"), &[], now), expected);
    }

    #[test]
    fn announces_classmates_joining_the_group() {
        let now = Utc::now();
        let mut memory = Memory {
            login: Some("alice".into()),
            ..Memory::default()
        };
        let member = |login: &str, name: Option<&str>| crate::groups::Member {
            login: login.into(),
            name: name.map(Into::into),
            email: None,
        };
        let with_group = |members: Vec<crate::groups::Member>| {
            let mut p = project("lab1", None);
            p.group = Some(crate::groups::Group {
                org: "org".into(),
                slug: "classroom50-group-0123456789abcdef-1".into(),
                number: 1,
                name: None,
                members,
                founders: None,
                url: String::new(),
                classroom: None,
                assignment: None,
            });
            snapshot(vec![p])
        };
        // The first sight of a group is the baseline.
        let first = with_group(vec![member("Alice", None)]);
        assert!(diff(&mut memory, &first, &[], now).is_empty());
        let joined = with_group(vec![member("Alice", None), member("bob", Some("Bob B"))]);
        assert_eq!(
            diff(&mut memory, &joined, &[], now),
            vec![Notice::GroupJoined {
                assignment: "LAB1".into(),
                members: vec!["Bob B".into()],
            }]
        );
        assert!(diff(&mut memory, &joined, &[], now).is_empty());
        assert!(memory.unseen.contains("org/c-lab1-alice"));
        // A classmate leaving says nothing, and the user's own login is never news.
        let left = with_group(vec![member("bob", Some("Bob B"))]);
        assert!(diff(&mut memory, &left, &[], now).is_empty());
        let back = with_group(vec![member("bob", Some("Bob B")), member("ALICE", None)]);
        assert!(diff(&mut memory, &back, &[], now).is_empty());
    }

    #[test]
    fn hidden_assignments_stay_quiet() {
        let now = Utc::now();
        let soon = || snapshot(vec![project("lab1", Some(now + TimeDelta::minutes(30)))]);
        let hidden = vec!["org/c-lab1-alice".to_owned()];
        let mut memory = Memory::default();
        let quiet = without_hidden(&soon(), &hidden, &[]);
        assert!(quiet.projects.is_empty());
        assert!(diff(&mut memory, &quiet, &[1], now).is_empty());
        // A hidden class quiets all of its assignments, whatever the case of its key.
        let class = vec!["org/c".to_owned()];
        assert!(without_hidden(&soon(), &[], &class).projects.is_empty());
        // Shown again, the pending reminder goes out.
        let shown = without_hidden(&soon(), &[], &[]);
        assert_eq!(diff(&mut memory, &shown, &[1], now).len(), 1);
    }

    #[test]
    fn reminds_once_per_deadline() {
        let now = Utc::now();
        let mut memory = Memory::default();
        // Due in 30 min: past both thresholds, but only one reminder.
        let snap = snapshot(vec![project("lab1", Some(now + TimeDelta::minutes(30)))]);
        let notices = diff(&mut memory, &snap, &[24, 1], now);
        assert_eq!(notices.len(), 1);
        assert!(diff(&mut memory, &snap, &[24, 1], now).is_empty());
    }

    #[test]
    fn announces_teammate_pushes_and_deadline_moves() {
        let now = Utc::now();
        let mut memory = Memory {
            login: Some("alice".into()),
            ..Memory::default()
        };
        let with_head = |oid: &str, author: &str, due: DateTime<Utc>| {
            let mut p = project("lab1", Some(due));
            p.remote.as_mut().unwrap().head = Some(crate::projects::Commit {
                oid: oid.into(),
                at: None,
                author: Some(author.into()),
            });
            p
        };
        let due = now + TimeDelta::days(5);
        assert!(
            diff(
                &mut memory,
                &snapshot(vec![with_head("a", "alice", due)]),
                &[],
                now
            )
            .is_empty()
        );
        // Own push: silent. Teammate push and a moved deadline: announced.
        assert!(
            diff(
                &mut memory,
                &snapshot(vec![with_head("b", "Alice", due)]),
                &[],
                now
            )
            .is_empty()
        );
        let later = due + TimeDelta::days(2);
        let notices = diff(
            &mut memory,
            &snapshot(vec![with_head("c", "gilles", later)]),
            &[],
            now,
        );
        assert_eq!(
            notices,
            vec![
                Notice::DeadlineChanged {
                    assignment: "LAB1".into(),
                    due: Some(later.with_timezone(&Utc)),
                },
                Notice::TeammatePush {
                    assignment: "LAB1".into(),
                    author: "gilles".into()
                },
            ]
        );
        assert!(memory.badge.contains("org/c-lab1-alice"));
    }
}
