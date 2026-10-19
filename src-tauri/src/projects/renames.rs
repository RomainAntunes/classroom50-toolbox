//! What `gh teacher assignment rename` changed: the ids from before, and a clone whose
//! `origin` still names the old repo.

use std::collections::{BTreeMap, HashMap, HashSet};

use super::{ClassroomKey, Project, Snapshot, Unaccepted, key};
use crate::classroom50::{self, Assignment, Classroom};

/// The id `project` had before `gh teacher assignment rename` renamed its repo (same
/// suffix, the old slug): what the hidden list, `alerts.json` and calendar UIDs may still
/// hold. `None` for a repo never renamed, or one the rename skipped.
pub fn previous_id(project: &Project) -> Option<String> {
    let assignment = project.assignment.as_ref()?;
    let old = classroom50::previous_repo_name(
        project.repo.as_deref()?,
        &project.config.classroom,
        std::slice::from_ref(assignment),
    )?;
    Some(key(&format!("{}/{old}", project.owner.as_deref()?)))
}

/// The id an unaccepted assignment had before its rename (`org/classroom/<old slug>`).
pub fn previous_unaccepted_id(item: &Unaccepted) -> Option<String> {
    let old = item.assignment.old_slug()?;
    Some(unaccepted_key(
        &item.classroom.org,
        &item.classroom.slug,
        old,
    ))
}

/// `org/classroom/assignment`, lowercase: an unaccepted assignment's id, and how
/// `alerts.json` keys due dates and closings.
pub fn unaccepted_key(org: &str, classroom: &str, assignment: &str) -> String {
    format!("{org}/{classroom}/{assignment}").to_ascii_lowercase()
}

/// What renames changed among the ids of `snapshot`, as `(old, new)`: projects (`owner/repo`)
/// and assignments (`org/classroom/slug`), for the hidden list and `alerts.json` to follow.
/// An old id still in use (a repo the rename skipped) is left alone.
pub fn renamed_ids(snapshot: &Snapshot) -> Vec<(String, String)> {
    let in_use: HashSet<&str> = snapshot
        .projects
        .iter()
        .map(|p| p.id.as_str())
        .chain(snapshot.unaccepted.iter().map(|u| u.id.as_str()))
        .collect();
    let projects = snapshot.projects.iter().flat_map(|p| {
        let assignment = p.assignment.as_ref();
        let slug_pair = p.owner.as_deref().zip(assignment).and_then(|(owner, a)| {
            let old = a.old_slug()?;
            Some((
                unaccepted_key(owner, &p.config.classroom, old),
                unaccepted_key(owner, &p.config.classroom, &a.slug),
            ))
        });
        let id_pair = previous_id(p).map(|old| (old, p.id.clone()));
        id_pair.into_iter().chain(slug_pair)
    });
    let unaccepted = snapshot
        .unaccepted
        .iter()
        .filter_map(|u| Some((previous_unaccepted_id(u)?, u.id.clone())));
    let mut pairs: Vec<(String, String)> = projects
        .chain(unaccepted)
        .filter(|(old, new)| old != new && !in_use.contains(old.as_str()))
        .collect();
    pairs.sort();
    pairs.dedup();
    pairs
}

/// The name GitHub now lists a clone's repo under, when the clone's `origin` still names
/// it as it was before `gh teacher assignment rename` (GitHub redirects the old name): the
/// old name isn't listed, the renamed one is (`listed`, lowercase `owner/repo` to GitHub's
/// spelling). `None` when the clone needs no new name.
pub(super) fn renamed_clone(
    owner: &str,
    name: &str,
    classrooms: &BTreeMap<ClassroomKey, Classroom>,
    manifests: &HashMap<ClassroomKey, Vec<Assignment>>,
    listed: &HashMap<String, String>,
) -> Option<String> {
    let owner = owner.to_ascii_lowercase();
    if listed.contains_key(&format!("{owner}/{}", name.to_ascii_lowercase())) {
        return None;
    }
    classrooms
        .iter()
        .filter(|((org, _), _)| *org == owner)
        .find_map(|(key, classroom)| {
            let renamed =
                classroom50::renamed_repo_name(name, &classroom.slug, manifests.get(key)?)?;
            listed.get(&format!("{owner}/{renamed}")).cloned()
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::discovery::LocalProject;
    use crate::projects::fixtures::{cs_config, remote, renamed_class};
    use crate::projects::{LocalState, merge};

    #[test]
    fn clones_made_before_a_rename_join_the_renamed_repo() {
        let (classrooms, manifests) = renamed_class();
        let listed = HashMap::from([("cs50/cs-ps3-alice".to_owned(), "cs-ps3-alice".to_owned())]);
        let rename = |owner, name| renamed_clone(owner, name, &classrooms, &manifests, &listed);
        assert_eq!(
            rename("cs50", "CS-problem-set-three-Alice").as_deref(),
            Some("cs-ps3-alice")
        );
        // Not renamed on GitHub yet, another student's, a current name, another org.
        assert_eq!(rename("cs50", "cs-problem-set-three-bob"), None);
        assert_eq!(rename("cs50", "cs-ps3-alice"), None);
        assert_eq!(rename("other", "cs-problem-set-three-alice"), None);
        let straggler = HashMap::from([
            ("cs50/cs-ps3-alice".to_owned(), "cs-ps3-alice".to_owned()),
            (
                "cs50/cs-problem-set-three-alice".to_owned(),
                "cs-problem-set-three-alice".to_owned(),
            ),
        ]);
        assert_eq!(
            renamed_clone(
                "cs50",
                "cs-problem-set-three-alice",
                &classrooms,
                &manifests,
                &straggler
            ),
            None
        );

        // Renamed, the clone and GitHub's repo are one row.
        let clone = LocalProject {
            path: "/code/ps3".into(),
            owner: Some("cs50".into()),
            repo: rename("cs50", "cs-problem-set-three-alice"),
            config: Some(cs_config("problem-set-three")),
        };
        let projects = merge(
            vec![(remote("cs-ps3-alice", "aaa", None), cs_config("ps3"))],
            vec![(clone, cs_config("ps3"), Some(LocalState::default()))],
        );
        assert_eq!(projects.len(), 1);
        assert_eq!(projects[0].id, "cs50/cs-ps3-alice");
        assert!(projects[0].path.is_some() && projects[0].remote.is_some());
    }

    #[test]
    fn tells_which_ids_a_rename_changed() {
        let (classrooms, manifests) = renamed_class();
        let key = ("cs50".to_owned(), "cs".to_owned());
        let ps3 = manifests[&key][0].clone();
        let classroom = classrooms[&key].clone();
        let project = |name: &str| {
            let mut project = merge(
                vec![(remote(name, "aaa", None), cs_config("ps3"))],
                Vec::new(),
            )
            .remove(0);
            project.assignment = Some(ps3.clone());
            project
        };
        let unaccepted = Unaccepted {
            id: "cs50/cs/ps3".into(),
            classroom,
            assignment: ps3.clone(),
            accept_url: String::new(),
            group: None,
        };
        let snapshot = Snapshot {
            projects: vec![project("cs-ps3-alice")],
            unaccepted: vec![unaccepted],
            ..Snapshot::default()
        };
        assert_eq!(
            renamed_ids(&snapshot),
            [
                (
                    "cs50/cs-problem-set-three-alice".to_owned(),
                    "cs50/cs-ps3-alice".to_owned()
                ),
                (
                    "cs50/cs/problem-set-three".to_owned(),
                    "cs50/cs/ps3".to_owned()
                ),
            ]
        );
        assert_eq!(
            previous_id(&snapshot.projects[0]).as_deref(),
            Some("cs50/cs-problem-set-three-alice")
        );
        // A straggler keeps its id, and nothing moves onto it.
        let snapshot = Snapshot {
            projects: vec![
                project("cs-problem-set-three-alice"),
                project("cs-ps3-alice"),
            ],
            ..Snapshot::default()
        };
        assert_eq!(
            renamed_ids(&snapshot),
            [(
                "cs50/cs/problem-set-three".to_owned(),
                "cs50/cs/ps3".to_owned()
            )]
        );
    }
}
