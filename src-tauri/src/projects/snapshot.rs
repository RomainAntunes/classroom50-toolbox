//! The snapshot a sync shows: built from a full sync, or from the last discovery for a
//! status check, a preview or a rescan.

use chrono::{DateTime, Utc};

use super::{
    Progress, ProjectsState, RemoteCache, RemoteRepo, Snapshot, Sources, SyncIssue, build,
    status_auth,
};
use crate::discovery::LocalProject;
use crate::editors;
use crate::error::{Error, Result};
use crate::groups;
use crate::sync_progress::Phase;

/// Adds what isn't decided by the build: groups, the editor that opened each clone, the
/// account.
pub(super) fn annotate(
    state: &ProjectsState,
    snapshot: &mut Snapshot,
    recents: &[editors::Recent],
) {
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

pub(super) async fn local_snapshot(
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

pub(super) async fn full_snapshot(
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    use crate::projects::fixtures::{config, remote};
    use crate::projects::merge;

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
}
