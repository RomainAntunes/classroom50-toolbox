//! Test data shared by the projects tests.

use std::collections::{BTreeMap, HashMap};

use serde_json::json;

use super::{ClassroomKey, Commit, RemoteRepo, Submission};
use crate::classroom50::{Assignment, Classroom, RepoConfig};

pub(super) fn config(assignment: &str) -> RepoConfig {
    RepoConfig {
        classroom: "fall".into(),
        assignment: assignment.into(),
        secret: None,
    }
}

pub(super) fn remote(name: &str, head: &str, submitted: Option<&str>) -> RemoteRepo {
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
            author: None,
        }),
        last_submission: submitted.map(|commit| Submission {
            tag: "submit/x".into(),
            commit: commit.into(),
            at: None,
        }),
        config: Some(config("hello")),
        template: None,
        unmerged: Vec::new(),
        grade: None,
        history: Vec::new(),
        grading: None,
        feedback: None,
    }
}

/// `cs/ps3` was `problem-set-three` before `gh teacher assignment rename`.
pub(super) fn renamed_class() -> (
    BTreeMap<ClassroomKey, Classroom>,
    HashMap<ClassroomKey, Vec<Assignment>>,
) {
    let cs = Classroom::from_team("CS50", "classroom50-cs", None).unwrap();
    let manifest: Vec<Assignment> = serde_json::from_value(json!([
        {"slug": "ps3", "name": "Problem Set 3", "mode": "individual",
         "renamed_from": "problem-set-three"},
        {"slug": "ps4", "name": "Problem Set 4", "mode": "individual"}
    ]))
    .unwrap();
    (
        BTreeMap::from([(cs.key(), cs)]),
        HashMap::from([(("cs50".to_owned(), "cs".to_owned()), manifest)]),
    )
}

pub(super) fn cs_config(assignment: &str) -> RepoConfig {
    RepoConfig {
        classroom: "cs".into(),
        assignment: assignment.into(),
        secret: None,
    }
}
