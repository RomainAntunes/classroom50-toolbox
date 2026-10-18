//! Git writes to a clone, on a click only: clone, pull, commit, commit and push, the
//! clone's identity.

use std::path::PathBuf;

use tauri::{AppHandle, Manager, State};

use super::submittable_clone;
use crate::clones::{self, CloneStore};
use crate::error::{Error, Result};
use crate::projects::{self, Project, ProjectsState};
use crate::pull;
use crate::push;
use crate::settings::{CommitActions, SettingsStore};

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
pub(super) fn push_target(project: &Project) -> Result<(String, Option<String>)> {
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
pub(super) fn allow_commit(actions: CommitActions, push: bool) -> Result<()> {
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::commands::fixtures::every_push;

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
