//! Which assignment a repo is: its marker file, its name, or the template it was
//! generated from.

use std::collections::{BTreeMap, HashMap, HashSet};

use super::{ClassroomKey, RemoteRepo};
use crate::classroom50::{self, Assignment, Classroom, RepoConfig};

/// What `identify` knows of a repo: from GitHub, from a clone, or both.
#[derive(Clone, Copy, Default)]
pub(super) struct Found<'a> {
    pub(super) owner: Option<&'a str>,
    pub(super) name: Option<&'a str>,
    pub(super) config: Option<&'a RepoConfig>,
    /// `owner/repo` of the template GitHub generated it from (`RemoteRepo::template`).
    pub(super) template: Option<&'a str>,
    /// The user may push to it (`RemoteRepo::can_push`): a template match needs it unless
    /// the entry's submissions are closed, which makes every student repo read-only.
    pub(super) can_push: bool,
}

/// How `identify_by` told a repo's assignment.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum MatchedBy {
    Marker,
    Name,
    Template,
}

/// The assignment identity of a repo: its marker file, else its name matched against the
/// manifests of the owner's classrooms, else the template it was generated from
/// (`by_template`). `None` means it isn't an assignment repo. A marker naming the slug an
/// assignment was renamed from (a repo the rename skipped, a clone not pulled since) counts
/// as that assignment, under its current slug.
pub(super) fn identify(
    found: Found<'_>,
    classrooms: &BTreeMap<ClassroomKey, Classroom>,
    manifests: &HashMap<ClassroomKey, Vec<Assignment>>,
) -> Option<RepoConfig> {
    identify_by(found, classrooms, manifests).map(|(config, _)| config)
}

/// `identify`, saying how the assignment was told.
pub(super) fn identify_by(
    found: Found<'_>,
    classrooms: &BTreeMap<ClassroomKey, Classroom>,
    manifests: &HashMap<ClassroomKey, Vec<Assignment>>,
) -> Option<(RepoConfig, MatchedBy)> {
    let Found {
        owner,
        name,
        config,
        template,
        can_push,
    } = found;
    if let Some(config) = config {
        let mut config = config.clone();
        let current = owner
            .and_then(|owner| {
                let key = (
                    owner.to_ascii_lowercase(),
                    config.classroom.to_ascii_lowercase(),
                );
                manifests.get(&key)
            })
            .and_then(|list| classroom50::assignment_for_slug(&config.assignment, list));
        if let Some(assignment) = current {
            config.assignment = assignment.slug.clone();
        }
        return Some((config, MatchedBy::Marker));
    }
    let (owner, name) = (owner?.to_ascii_lowercase(), name?);
    let by_name = classrooms
        .iter()
        .filter(|((org, _), _)| *org == owner)
        .find_map(|(key, classroom)| {
            let found =
                classroom50::assignment_for_repo(name, &classroom.slug, manifests.get(key)?)?;
            Some((classroom, found.assignment, MatchedBy::Name))
        });
    let (classroom, assignment, by) = by_name.or_else(|| {
        let (classroom, assignment) =
            by_template(&owner, name, template?, can_push, classrooms, manifests)?;
        Some((classroom, assignment, MatchedBy::Template))
    })?;
    let config = RepoConfig {
        classroom: classroom.slug.clone(),
        assignment: assignment.slug.clone(),
        secret: classroom.secret.clone(),
    };
    Some((config, by))
}

/// Drops the repos told by their template alone (`MatchedBy::Template`) whose assignment
/// already has a repo told by its marker or name, or another one told by its template: a
/// scratch copy generated from the same template is no second submission, and two of them
/// can't be told apart. Each assignment keeps one row, one calendar event, one reminder.
pub(super) fn without_doubtful_templates(
    repos: Vec<(RemoteRepo, RepoConfig, MatchedBy)>,
) -> Vec<(RemoteRepo, RepoConfig)> {
    let assignment = |repo: &RemoteRepo, config: &RepoConfig| {
        (
            repo.owner.to_ascii_lowercase(),
            config.classroom.to_ascii_lowercase(),
            config.assignment.to_ascii_lowercase(),
        )
    };
    let mut told: HashSet<(String, String, String)> = HashSet::new();
    let mut by_template: HashMap<(String, String, String), usize> = HashMap::new();
    for (repo, config, by) in &repos {
        match by {
            MatchedBy::Template => *by_template.entry(assignment(repo, config)).or_default() += 1,
            MatchedBy::Marker | MatchedBy::Name => {
                told.insert(assignment(repo, config));
            }
        }
    }
    repos
        .into_iter()
        .filter(|(repo, config, by)| {
            let key = assignment(repo, config);
            *by != MatchedBy::Template || (!told.contains(&key) && by_template[&key] == 1)
        })
        .map(|(repo, config, _)| (repo, config))
        .collect()
}

/// The assignment a repo without a marker or a matching name was generated for: the one
/// entry, among the manifests of the user's classrooms in the repo's org, whose `template`
/// is the repo's (a teacher renamed the repo by hand, say). Several entries sharing it (an
/// assignment reused, one template in two classes) are told apart by the name: the
/// classroom's `<classroom>-` prefix, then the slug inside it. Still several, or none:
/// `None`. Never the template repo itself, nor a repo outside the classroom's org (`owner`,
/// lowercase), nor one named after another of the org's classrooms (`<other>-...`, the
/// longest prefix deciding): those are no student's work for that entry. `empty_repo`
/// assignments have no template. Nor a repo the user can't push to (`can_push`), unless the
/// entry is closed (Close submission sets every student repo read-only). The repo list only
/// passes a template on for repos that aren't templates or archived
/// (`ListedRepo::generated_from`).
fn by_template<'a>(
    owner: &str,
    name: &str,
    template: &str,
    can_push: bool,
    classrooms: &'a BTreeMap<ClassroomKey, Classroom>,
    manifests: &'a HashMap<ClassroomKey, Vec<Assignment>>,
) -> Option<(&'a Classroom, &'a Assignment)> {
    let entries: Vec<(&Classroom, &Assignment)> = classrooms
        .iter()
        .filter(|((org, _), _)| org == owner)
        .flat_map(|(key, classroom)| {
            manifests
                .get(key)
                .into_iter()
                .flatten()
                .map(move |assignment| (classroom, assignment))
        })
        .collect();
    let itself = format!("{owner}/{name}");
    if entries.iter().any(|(_, a)| {
        a.template_name()
            .is_some_and(|t| t.eq_ignore_ascii_case(&itself))
    }) {
        return None;
    }
    let lower = name.to_ascii_lowercase();
    // The classroom the name starts with, if any: `cs-2026-...` belongs to `cs-2026`,
    // not `cs`.
    let named_after = classrooms
        .values()
        .filter(|c| c.org.eq_ignore_ascii_case(owner))
        .map(|c| c.slug.to_ascii_lowercase())
        .filter(|slug| lower.starts_with(&format!("{slug}-")))
        .max_by_key(String::len);
    let mut candidates: Vec<(&Classroom, &Assignment)> = entries
        .into_iter()
        .filter(|(_, a)| {
            a.template_name()
                .is_some_and(|t| t.eq_ignore_ascii_case(template))
        })
        .filter(|(c, _)| {
            named_after
                .as_ref()
                .is_none_or(|slug| c.slug.eq_ignore_ascii_case(slug))
        })
        // Read only: a teacher's copy or a classmate's public repo, unless the teacher
        // closed submissions, which makes the student's own repo read-only too.
        .filter(|(_, a)| can_push || a.closed)
        .collect();
    /// Keeps the candidates `keep` accepts, unless none or a single one is left.
    fn narrow(
        candidates: &mut Vec<(&Classroom, &Assignment)>,
        keep: impl Fn(&Classroom, &Assignment) -> bool,
    ) {
        if candidates.len() > 1 && candidates.iter().any(|(c, a)| keep(c, a)) {
            candidates.retain(|(c, a)| keep(c, a));
        }
    }
    narrow(&mut candidates, |_, a| {
        lower.contains(&a.slug.to_ascii_lowercase())
    });
    match candidates.as_slice() {
        [one] => Some(*one),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    use crate::projects::build::unaccepted;
    use crate::projects::fixtures::{config, cs_config, remote, renamed_class};
    use crate::projects::merge;

    #[test]
    fn identifies_repos_by_marker_or_name() {
        let poo = Classroom::from_team("HEIGVD-POO", "classroom50-26-poo-c", None).unwrap();
        let classrooms = BTreeMap::from([(poo.key(), poo)]);
        let manifest: Vec<Assignment> = serde_json::from_value(json!([
            {"slug": "lab02", "name": "Labo 2", "mode": "team"},
            {"slug": "labo-3-hogwarts", "name": "Labo 3", "mode": "team"}
        ]))
        .unwrap();
        let manifests =
            HashMap::from([(("heigvd-poo".to_owned(), "26-poo-c".to_owned()), manifest)]);
        let find = |owner, name, config| {
            let found = Found {
                owner,
                name,
                config,
                template: None,
                can_push: true,
            };
            identify(found, &classrooms, &manifests)
        };

        let lab3 = find(
            Some("HEIGVD-POO"),
            Some("26-poo-c-labo-3-hogwarts-group-5"),
            None,
        )
        .unwrap();
        assert_eq!(
            (lab3.classroom.as_str(), lab3.assignment.as_str()),
            ("26-poo-c", "labo-3-hogwarts")
        );
        // A marker file wins; unrelated repos and other orgs don't match.
        let marked = config("hello");
        assert_eq!(
            find(Some("x"), Some("y"), Some(&marked)),
            Some(marked.clone())
        );
        assert_eq!(find(Some("HEIGVD-POO"), Some("hogwarts"), None), None);
        assert_eq!(
            find(Some("other-org"), Some("26-poo-c-lab02-group-1"), None),
            None
        );
    }

    #[test]
    fn renamed_assignments_keep_their_repos() {
        let (classrooms, manifests) = renamed_class();
        let find = |name, config| {
            let found = Found {
                owner: Some("CS50"),
                name: Some(name),
                config,
                template: None,
                can_push: true,
            };
            identify(found, &classrooms, &manifests)
        };
        // A marker the rename didn't rewrite, or a clone not pulled since.
        let old = cs_config("problem-set-three");
        assert_eq!(
            find("cs-problem-set-three-alice", Some(&old)).map(|c| c.assignment),
            Some("ps3".to_owned())
        );
        // No marker: the old name or the new one.
        assert_eq!(
            find("cs-problem-set-three-bob", None).map(|c| c.assignment),
            Some("ps3".to_owned())
        );
        assert_eq!(
            find("cs-ps3-bob", None).map(|c| c.assignment),
            Some("ps3".to_owned())
        );
        // A marker of another class or naming something else stays as it is.
        let other = cs_config("lab");
        assert_eq!(find("cs-lab-bob", Some(&other)), Some(other));

        // A straggler the rename skipped counts as accepting the renamed assignment.
        let mut straggler = remote("cs-problem-set-three-alice", "aaa", None);
        straggler.config = Some(old.clone());
        let projects = merge(vec![(straggler, cs_config("ps3"))], Vec::new());
        let list = unaccepted(&projects, &classrooms, &manifests);
        let slugs: Vec<&str> = list.iter().map(|u| u.assignment.slug.as_str()).collect();
        assert_eq!(slugs, ["ps4"]);
        // The old name alone is enough too, whatever its marker says.
        let mut projects = projects;
        projects[0].config.assignment = "something-else".into();
        let list = unaccepted(&projects, &classrooms, &manifests);
        let slugs: Vec<&str> = list.iter().map(|u| u.assignment.slug.as_str()).collect();
        assert_eq!(slugs, ["ps4"]);
    }

    #[test]
    fn identifies_repos_by_their_template() {
        let c = Classroom::from_team("HEIGVD-POO", "classroom50-26-poo-c", None).unwrap();
        let d = Classroom::from_team("HEIGVD-POO", "classroom50-26-poo-d", None).unwrap();
        let other = Classroom::from_team("Elsewhere", "classroom50-web", None).unwrap();
        // A classroom of the org whose slug starts the others' (no manifest read).
        let poo = Classroom::from_team("HEIGVD-POO", "classroom50-26-poo", None).unwrap();
        let template = |repo: &str| json!({"owner": "HEIGVD-POO", "repo": repo, "branch": "main"});
        let entry = |slug: &str, repo: Option<&str>| json!({"slug": slug, "name": slug, "mode": "team", "template": repo.map(template)});
        let list = |entries: Vec<serde_json::Value>| -> Vec<Assignment> {
            serde_json::from_value(serde_json::Value::Array(entries)).unwrap()
        };
        let manifests = HashMap::from([
            (
                c.key(),
                list(vec![
                    entry("labo-3-hogwarts", Some("hogwarts")),
                    entry("labo-4", Some("shared")),
                    entry("labo-5", Some("shared")),
                    entry("labo-6", Some("unique")),
                    entry("labo-e", None),
                ]),
            ),
            (d.key(), list(vec![entry("labo-3", Some("hogwarts"))])),
            (other.key(), list(vec![entry("site", Some("unique"))])),
        ]);
        let classrooms = BTreeMap::from([
            (c.key(), c),
            (d.key(), d),
            (other.key(), other),
            (poo.key(), poo),
        ]);
        let find = |owner: &str, name: &str, template: Option<&str>| {
            let found = Found {
                owner: Some(owner),
                name: Some(name),
                config: None,
                template,
                can_push: true,
            };
            identify(found, &classrooms, &manifests)
                .map(|c| format!("{}/{}", c.classroom, c.assignment))
        };
        let hogwarts = Some("HEIGVD-POO/hogwarts");
        let shared = Some("heigvd-poo/Shared");

        // One entry with that template: renamed by hand, still found.
        assert_eq!(
            find("HEIGVD-POO", "renamed-by-hand", Some("HEIGVD-POO/unique")).as_deref(),
            Some("26-poo-c/labo-6")
        );
        // The name rule comes first.
        assert_eq!(
            find("HEIGVD-POO", "26-poo-c-labo-4-alice", hogwarts).as_deref(),
            Some("26-poo-c/labo-4")
        );
        // One template in two classes: the classroom prefix decides, else nothing.
        assert_eq!(
            find("HEIGVD-POO", "26-poo-c-hogwarts-alice", hogwarts).as_deref(),
            Some("26-poo-c/labo-3-hogwarts")
        );
        assert_eq!(
            find("heigvd-poo", "26-POO-D-mine", hogwarts).as_deref(),
            Some("26-poo-d/labo-3")
        );
        assert_eq!(find("HEIGVD-POO", "my-hogwarts", hogwarts), None);
        // One template reused in a class: the slug in the name decides, else nothing.
        assert_eq!(
            find("HEIGVD-POO", "26-poo-c-my-labo-5", shared).as_deref(),
            Some("26-poo-c/labo-5")
        );
        assert_eq!(find("HEIGVD-POO", "26-poo-c-renamed", shared), None);
        assert_eq!(find("HEIGVD-POO", "labo-4-and-labo-5", shared), None);
        // No template, an unknown one, the template itself, or a repo outside the org.
        assert_eq!(find("HEIGVD-POO", "renamed-by-hand", None), None);
        assert_eq!(find("HEIGVD-POO", "x", Some("HEIGVD-POO/other")), None);
        assert_eq!(
            find("HEIGVD-POO", "hogwarts", Some("HEIGVD-POO/unique")),
            None
        );
        assert_eq!(
            find("alice", "renamed-by-hand", Some("HEIGVD-POO/unique")),
            None
        );
        assert_eq!(
            find("Elsewhere", "copy", Some("HEIGVD-POO/unique")).as_deref(),
            Some("web/site")
        );
        // Named after another classroom of the org (the longest prefix deciding): an old
        // term's repo, a sibling class's, never this entry's.
        assert_eq!(
            find("HEIGVD-POO", "26-poo-d-renamed", Some("HEIGVD-POO/unique")),
            None
        );
        assert_eq!(
            find("HEIGVD-POO", "26-poo-x-renamed", Some("HEIGVD-POO/unique")),
            None
        );
        assert_eq!(
            find("HEIGVD-POO", "26-poo-c-renamed", Some("HEIGVD-POO/unique")).as_deref(),
            Some("26-poo-c/labo-6")
        );

        // Accepted that way, the assignment isn't offered again.
        let mut repo = remote("renamed-by-hand", "aaa", None);
        repo.owner = "HEIGVD-POO".into();
        repo.config = None;
        repo.template = Some("HEIGVD-POO/unique".into());
        let config = identify(
            Found {
                owner: Some(&repo.owner),
                name: Some(&repo.name),
                config: None,
                template: repo.template.as_deref(),
                can_push: repo.can_push,
            },
            &classrooms,
            &manifests,
        )
        .unwrap();
        let projects = merge(vec![(repo, config)], Vec::new());
        let list = unaccepted(&projects, &classrooms, &manifests);
        assert!(list.iter().all(|u| u.assignment.slug != "labo-6"));
        assert!(list.iter().any(|u| u.assignment.slug == "labo-5"));
    }

    /// Close submission sets every student repo read-only: the student's own repo, known
    /// by its template, stays; a read-only copy of an open assignment's template never counts.
    #[test]
    fn a_read_only_repo_matches_a_closed_assignment_only() {
        let class = Classroom::from_team("cs50", "classroom50-cs", None).unwrap();
        let manifest: Vec<Assignment> = serde_json::from_value(json!([
            {"slug": "lab1", "name": "Lab 1", "mode": "individual", "closed": true,
             "template": {"owner": "cs50", "repo": "lab1-template", "branch": "main"}},
            {"slug": "lab2", "name": "Lab 2", "mode": "individual",
             "template": {"owner": "cs50", "repo": "lab2-template", "branch": "main"}},
        ]))
        .unwrap();
        assert!(manifest[0].closed && !manifest[1].closed);
        let manifests = HashMap::from([(class.key(), manifest)]);
        let classrooms = BTreeMap::from([(class.key(), class)]);
        let find = |template: &str, can_push: bool| {
            let found = Found {
                owner: Some("cs50"),
                name: Some("renamed-by-hand"),
                config: None,
                template: Some(template),
                can_push,
            };
            identify(found, &classrooms, &manifests).map(|c| c.assignment)
        };
        assert_eq!(find("cs50/lab1-template", false).as_deref(), Some("lab1"));
        assert_eq!(find("cs50/lab1-template", true).as_deref(), Some("lab1"));
        assert_eq!(find("cs50/lab2-template", false), None);
        assert_eq!(find("cs50/lab2-template", true).as_deref(), Some("lab2"));
    }

    #[test]
    fn a_template_alone_never_doubles_an_assignment() {
        let repo = |name: &str| {
            let mut repo = remote(name, "aaa", None);
            repo.config = None;
            repo
        };
        let lab = |slug: &str| cs_config(slug);
        let names = |repos: Vec<(RemoteRepo, RepoConfig)>| -> Vec<String> {
            repos.into_iter().map(|(r, _)| r.name).collect()
        };
        // A scratch copy next to the real repo (by name or marker) drops out.
        let kept = without_doubtful_templates(vec![
            (repo("cs-lab1-alice"), lab("lab1"), MatchedBy::Name),
            (
                repo("alice-lab1-practice"),
                lab("lab1"),
                MatchedBy::Template,
            ),
            (repo("cs-lab2-alice"), lab("LAB2"), MatchedBy::Marker),
            (repo("lab2-again"), lab("lab2"), MatchedBy::Template),
            (repo("renamed-lab3"), lab("lab3"), MatchedBy::Template),
        ]);
        assert_eq!(
            names(kept),
            ["cs-lab1-alice", "cs-lab2-alice", "renamed-lab3"]
        );
        // Two told by their template alone: neither can be trusted.
        let kept = without_doubtful_templates(vec![
            (repo("mine"), lab("lab4"), MatchedBy::Template),
            (repo("also-mine"), lab("lab4"), MatchedBy::Template),
        ]);
        assert!(kept.is_empty());
    }
}
