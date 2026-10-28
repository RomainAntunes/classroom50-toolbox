//! Joins GitHub's repos and the clones by `owner/repo`, then what is pending, how late, in
//! which order.

use std::collections::BTreeMap;
use std::path::Path;

use chrono::{DateTime, Utc};

use super::{LocalState, Pending, Project, RemoteRepo, key};
use crate::classroom50::{RepoConfig, SubmissionMode};
use crate::discovery::LocalProject;

pub(super) fn merge(
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
                needs_work: false,
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
                        needs_work: false,
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

pub(super) fn late_seconds(project: &Project) -> Option<i64> {
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

pub(super) fn pending(project: &Project) -> Option<Pending> {
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
pub(super) fn sort_key(project: &Project, now: DateTime<Utc>) -> (u8, i64, String) {
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
    use serde_json::json;

    use crate::classroom50::Assignment;
    use crate::projects::fixtures::{config, remote};

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
            needs_work: false,
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
}
