//! The `gh student` writes: accept, adding classmates, submit.

use std::path::{Path, PathBuf};

use serde::Serialize;
use tauri::{AppHandle, Manager, State};

use crate::deeplink::DeepLinkState;
use crate::error::{Error, Result};
use crate::projects::{self, Project, ProjectsState, Snapshot};
use crate::student;
use crate::teammates;

#[derive(Serialize)]
#[cfg_attr(test, derive(specta::Type))]
#[serde(rename_all = "camelCase")]
pub struct Accepted {
    /// `owner/name` as GitHub spells it.
    repo: String,
    /// The new project's id, once the sync that follows the accept has found it.
    project_id: Option<String>,
    /// A group name was given but the student was already in a group (a teammate added
    /// them), so that group's repo was accepted and no group was founded.
    joined_group: bool,
}

/// Accepts an unaccepted assignment with `gh student accept`, then syncs so the new repo
/// shows up. `team_name` founds a group with that name for a team assignment formed by
/// students, unless the student is in a group already; without it such an assignment fails
/// with `needsTeam` while they aren't.
#[tauri::command]
#[cfg_attr(test, specta::specta)]
pub async fn accept_assignment(
    app: AppHandle,
    state: State<'_, ProjectsState>,
    id: String,
    team_name: Option<String>,
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
    let key = item.classroom.secret.as_deref();
    let accepted = student::accept(&student::AcceptRequest {
        org: &item.classroom.org,
        classroom: &item.classroom.slug,
        assignment: &item.assignment.slug,
        key,
        new_team: team_name.as_deref(),
    })
    .await;
    let student::AcceptOutcome { repo, joined_group } = match accepted {
        Ok(outcome) => outcome,
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
    if item.assignment.mode == crate::classroom50::AssignmentMode::Team {
        // Teammates can be added before a sync lists the group's repo.
        app.state::<student::FoundedGroups>().insert(
            &id,
            student::FoundedGroup {
                repo: repo.clone(),
                org: item.classroom.org.clone(),
                classroom: item.classroom.slug.clone(),
                assignment: item.assignment.slug.clone(),
                key: key.map(str::to_owned),
            },
        );
    }
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
    Ok(Accepted {
        repo,
        project_id,
        joined_group,
    })
}

/// The project of the assignment behind unaccepted id `org/classroom/assignment`.
fn accepted_project(snapshot: &Snapshot, id: &str) -> Option<Accepted> {
    let mut parts = id.splitn(3, '/');
    let (org, classroom, assignment) = (parts.next()?, parts.next()?, parts.next()?);
    let project = projects::assignment_project(snapshot, org, classroom, assignment)?;
    Some(Accepted {
        repo: format!("{}/{}", project.owner.as_deref()?, project.repo.as_deref()?),
        project_id: Some(project.id.clone()),
        joined_group: false,
    })
}

/// Queues classmates to get push access to project `id` and returns at once: they join
/// the group on a team assignment, get an invitation otherwise, one by one in the
/// background (`teammates.rs`, sent as `teammate-jobs`), then a full sync reads the group's
/// members again. `id` may also be the assignment of a group just founded from the app
/// whose repo no sync has listed yet. Usernames queued while that row's are still being
/// added join its queue.
#[tauri::command]
#[cfg_attr(test, specta::specta)]
pub async fn add_teammates(
    app: AppHandle,
    state: State<'_, ProjectsState>,
    founded: State<'_, student::FoundedGroups>,
    id: String,
    usernames: Vec<String>,
) -> Result<()> {
    // Async only to stay off the main thread, where Tauri runs sync commands.
    let snapshot = state.snapshot();
    let project = snapshot.projects.iter().find(|p| p.id == id).cloned();
    let (meta, target, cap) = match project {
        None => {
            let group = founded
                .get(&id)
                .ok_or_else(|| Error::Unsupported(format!("unknown project {id}")))?;
            let listed = snapshot.unaccepted.iter().find(|u| u.id == id);
            let assignment =
                listed.map_or_else(|| group.assignment.clone(), |u| u.assignment.name.clone());
            // Founded from the app moments ago: unless a sync read it since, its founder is
            // the only member yet.
            let cap = listed.and_then(|u| {
                let members = match &u.group {
                    Some(group) => group.members.iter().map(|m| m.login.clone()).collect(),
                    None => vec![snapshot.login.clone()?],
                };
                Some(teammates::Cap {
                    max: u.assignment.max_group_size?,
                    members,
                })
            });
            let meta = job(
                &id,
                group.repo.to_ascii_lowercase(),
                &group.repo,
                assignment,
                true,
            );
            let target = teammates::Target::Team {
                org: group.org,
                classroom: group.classroom,
                assignment: group.assignment,
                key: group.key,
            };
            (meta, target, cap)
        }
        Some(project) => {
            let (Some(owner), Some(repo)) = (project.owner.as_deref(), project.repo.as_deref())
            else {
                return Err(Error::Unsupported(format!("{id} isn't on GitHub")));
            };
            let team = project.group.is_some()
                || project
                    .assignment
                    .as_ref()
                    .is_some_and(|a| a.mode == crate::classroom50::AssignmentMode::Team);
            let full_name = format!("{owner}/{repo}");
            let assignment = project
                .assignment
                .as_ref()
                .map_or_else(|| project.config.assignment.clone(), |a| a.name.clone());
            let target = if team {
                teammates::Target::Team {
                    org: owner.to_owned(),
                    classroom: project.config.classroom.clone(),
                    assignment: project.config.assignment.clone(),
                    key: project
                        .classroom
                        .as_ref()
                        .and_then(|c| c.secret.clone())
                        .or_else(|| project.config.secret.clone()),
                }
            } else {
                teammates::Target::Invite {
                    repo: full_name.clone(),
                    clone: project.path.clone(),
                }
            };
            // `invite` checks a legacy group's size itself, in the founder's clone.
            let cap = project
                .assignment
                .as_ref()
                .and_then(|a| a.max_group_size)
                .zip(project.group.as_ref())
                .filter(|_| team)
                .map(|(max, group)| teammates::Cap {
                    max,
                    members: group.members.iter().map(|m| m.login.clone()).collect(),
                });
            (
                job(&id, id.clone(), &full_name, assignment, team),
                target,
                cap,
            )
        }
    };
    teammates::enqueue(&app, meta, target, cap, usernames);
    Ok(())
}

/// A new queue of classmates for the row of `id`, on repo `repo` (`owner/name`), whose
/// project id is `project`.
fn job(
    id: &str,
    project: String,
    repo: &str,
    assignment: String,
    team: bool,
) -> teammates::TeammateJob {
    teammates::TeammateJob {
        id: id.to_owned(),
        project,
        repo: repo.to_owned(),
        assignment,
        team,
        running: false,
        entries: Vec::new(),
    }
}

/// Every row's queue of classmates, for a view that opens while one runs.
#[tauri::command]
#[cfg_attr(test, specta::specta)]
pub fn teammate_jobs(jobs: State<'_, teammates::TeammateJobs>) -> Vec<teammates::TeammateJob> {
    jobs.list()
}

/// Done: drops the results the row of `id` shows (usernames still queued stay).
#[tauri::command]
#[cfg_attr(test, specta::specta)]
pub fn dismiss_teammate_job(app: AppHandle, jobs: State<'_, teammates::TeammateJobs>, id: String) {
    if jobs.dismiss(&id) {
        teammates::emit(&app);
    }
}

/// The clone of project `id`, which must exist and still take submissions.
pub(super) fn submittable_clone(state: &ProjectsState, id: &str) -> Result<PathBuf> {
    submittable(state, id).map(|(path, _)| path)
}

/// Project `id` and its clone, which must exist and still take submissions.
fn submittable(state: &ProjectsState, id: &str) -> Result<(PathBuf, Project)> {
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
    let path = project
        .path
        .clone()
        .ok_or_else(|| Error::Unsupported(format!("{id} has no local clone")))?;
    Ok((path, project))
}

/// What a clone without a marker needs for `gh student submit`: whether the repo's name
/// matches its assignment (gh-student reads nothing else: a repo known by its template
/// alone, or renamed by hand, it can't place), and the unlisted class's key. Plus the
/// assignment's `allowed_files`, for the files the submission leaves out.
pub(super) fn submit_target(project: &Project) -> student::SubmitTarget<'_> {
    let named = project
        .repo
        .as_deref()
        .zip(project.assignment.as_ref())
        .is_some_and(|(name, assignment)| {
            crate::classroom50::assignment_for_repo(
                name,
                &project.config.classroom,
                std::slice::from_ref(assignment),
            )
            .is_some()
        });
    student::SubmitTarget {
        assignment_known: named,
        key: project.classroom.as_ref().and_then(|c| c.secret.as_deref()),
        allowed_files: project
            .assignment
            .as_ref()
            .map_or(&[], |a| a.allowed_files.as_slice()),
    }
}

/// Refuses to submit project `id` while its clone's marker names the slug the assignment
/// was renamed from (`student::stale_marker`): it needs a pull first.
fn refuse_stale_marker(state: &ProjectsState, id: &str, path: &Path) -> Result<()> {
    let snapshot = state.snapshot();
    let Some(project) = snapshot.projects.iter().find(|p| p.id == id) else {
        return Ok(());
    };
    let Some(assignment) = project.assignment.as_ref() else {
        return Ok(());
    };
    let local = crate::classroom50::RepoConfig::read(path).ok().flatten();
    let github = project.remote.as_ref().and_then(|r| r.config.as_ref());
    if student::stale_marker(local.as_ref(), github, assignment) {
        return Err(Error::StaleMarker);
    }
    Ok(())
}

/// What submitting project `id` would send, for the confirmation dialog.
#[tauri::command]
#[cfg_attr(test, specta::specta)]
pub async fn submit_check(
    state: State<'_, ProjectsState>,
    id: String,
) -> Result<student::SubmitCheck> {
    let (path, project) = submittable(&state, &id)?;
    refuse_stale_marker(&state, &id, &path)?;
    student::check(&path, submit_target(&project)).await
}

/// Submits project `id` with `gh student submit`, then re-reads its status while the
/// autograder runs (the clone's refs don't move, so the watcher wouldn't notice).
#[tauri::command]
#[cfg_attr(test, specta::specta)]
pub async fn submit_project(
    app: AppHandle,
    state: State<'_, ProjectsState>,
    id: String,
) -> Result<student::Submitted> {
    let (path, project) = submittable(&state, &id)?;
    refuse_stale_marker(&state, &id, &path)?;
    let submitted = student::submit(&path, submit_target(&project)).await?;
    tauri::async_runtime::spawn(async move {
        projects::refresh_after_push(&app).await;
    });
    Ok(submitted)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::commands::fixtures::{every_push, project, snapshot};

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

    /// A clone without a marker submits with its class's key, and only when its name
    /// matches its assignment, as gh-student finds it.
    #[test]
    fn submits_with_the_classs_key() {
        let mut p = every_push("main", None);
        p.repo = Some("C-Lab-1-alice".into());
        let target = submit_target(&p);
        assert!(target.assignment_known);
        assert_eq!(target.key, None);
        p.classroom = Some(crate::classroom50::Classroom {
            org: "org".into(),
            slug: "c".into(),
            name: None,
            term: None,
            active: true,
            secret: Some("a1b2c3d4".into()),
            pages_base_url: None,
        });
        assert_eq!(submit_target(&p).key, Some("a1b2c3d4"));
        // Renamed by hand, known by its template alone: gh-student can't place it.
        p.repo = Some("lab-1-alice".into());
        assert!(!submit_target(&p).assignment_known);
        p.repo = Some("c-lab-1-alice".into());
        p.assignment = None;
        assert!(!submit_target(&p).assignment_known);
    }

    /// A clone made before a rename keeps the old name, which gh-student still places.
    #[test]
    fn a_name_from_before_a_rename_still_submits() {
        let mut p = every_push("main", None);
        p.repo = Some("c-lab-one-alice".into());
        p.assignment = Some(
            serde_json::from_value(serde_json::json!({
                "slug": "lab-1", "name": "Lab 1", "mode": "individual", "renamed_from": "lab-one"
            }))
            .unwrap(),
        );
        assert!(submit_target(&p).assignment_known);
    }
}
