//! Projects shared by the commands tests.

use crate::classroom50::RepoConfig;
use crate::projects::{LocalState, Project, RemoteRepo, Snapshot};

pub(super) fn project(
    owner: Option<&str>,
    repo: &str,
    classroom: &str,
    assignment: &str,
) -> Project {
    Project {
        id: match owner {
            Some(owner) => format!("{owner}/{repo}").to_ascii_lowercase(),
            None => format!("/clones/{repo}"),
        },
        owner: owner.map(str::to_owned),
        repo: owner.map(|_| repo.to_owned()),
        config: RepoConfig {
            classroom: classroom.into(),
            assignment: assignment.into(),
            secret: None,
        },
        path: None,
        local: None,
        remote: None,
        classroom: None,
        assignment: None,
        pending: None,
        web_url: None,
        late_seconds: None,
        editor: None,
        group: None,
    }
}

pub(super) fn snapshot(projects: Vec<Project>) -> Snapshot {
    Snapshot {
        projects,
        ..Snapshot::default()
    }
}

/// An every-push project on branch `local_branch`, with GitHub's data when `remote` is
/// `Some((default branch, head))`.
pub(super) fn every_push(
    local_branch: &str,
    remote: Option<(Option<&str>, Option<&str>)>,
) -> Project {
    let mut p = project(Some("org"), "lab-1-alice", "c", "lab-1");
    p.assignment = Some(
        serde_json::from_value(serde_json::json!({
            "slug": "lab-1", "name": "Lab 1", "mode": "individual"
        }))
        .unwrap(),
    );
    p.local = Some(LocalState {
        branch: Some(local_branch.into()),
        ..LocalState::default()
    });
    p.remote = remote.map(|(branch, head)| {
        let mut repo: RemoteRepo = serde_json::from_value(serde_json::json!({
            "owner": "org", "name": "lab-1-alice", "url": "https://github.com/org/lab-1-alice",
            "archived": false, "canPush": true
        }))
        .unwrap();
        repo.default_branch = branch.map(str::to_owned);
        repo.head =
            head.map(|oid| serde_json::from_value(serde_json::json!({ "oid": oid })).unwrap());
        repo
    });
    p
}
