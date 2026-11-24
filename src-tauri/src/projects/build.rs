//! One list from the repos seen on GitHub, the classes and the clones on disk.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::PathBuf;

use chrono::{DateTime, Utc};
use futures_util::future::join_all;

use super::{
    ClassroomKey, Found, MANIFEST_MAX_AGE, Progress, Project, ProjectsState, RemoteRepo,
    StatusAuth, Unaccepted, check_clones, identify, identify_by, key, late_seconds, local_states,
    merge, pending, refresh_statuses, renamed_clone, sort_key, without_doubtful_templates,
};
use crate::classroom50::{self, Assignment, Classroom, RepoConfig};
use crate::discovery::LocalProject;
use crate::error::Error;
use crate::github::HttpCache;
use crate::manual::{ManualInfo, ManualProject};
use crate::sync_progress::Phase;

/// Resolves which repos are assignments, then merges, annotates and sorts them.
/// What a build merges: repos seen on GitHub, classrooms from teams, clones on disk.
pub(super) struct Sources {
    pub(super) remote: Vec<RemoteRepo>,
    pub(super) classrooms: Vec<Classroom>,
    pub(super) locals: Vec<LocalProject>,
    /// Whether `remote` is the account's repo list (fresh or cached). Without it a missing
    /// repo proves nothing, so no assignment is offered for accepting.
    pub(super) discovered: bool,
    /// The projects the student added by hand (`manual::ManualStore`).
    pub(super) manual: Vec<ManualProject>,
}

pub(super) async fn build(
    sources: Sources,
    state: &ProjectsState,
    http: &reqwest::Client,
    status: Option<&StatusAuth>,
    network: bool,
    now: DateTime<Utc>,
    progress: &Progress,
) -> (Vec<Project>, Vec<Unaccepted>) {
    let Sources {
        remote,
        classrooms: team_classrooms,
        locals,
        discovered,
        manual,
    } = sources;
    // Teams carry the display name and secret, so they win over marker files.
    let mut classrooms: BTreeMap<ClassroomKey, Classroom> =
        team_classrooms.into_iter().map(|c| (c.key(), c)).collect();
    let configs = remote
        .iter()
        .filter_map(|r| Some((r.owner.as_str(), r.config.as_ref()?)))
        .chain(
            locals
                .iter()
                .filter_map(|l| Some((l.owner.as_deref()?, l.config.as_ref()?))),
        );
    for (owner, config) in configs {
        let classroom = Classroom::from_config(owner, config);
        classrooms.entry(classroom.key()).or_insert(classroom);
    }
    let manifests = fetch_manifests(&classrooms, &state.http_cache, http, network, progress).await;
    state.http_cache.save();

    // Where each manual project lives on GitHub: its folder's origin, else the repo typed.
    let manual_repos: Vec<Option<(String, String)>> = manual
        .iter()
        .map(|m| {
            m.path
                .as_deref()
                .and_then(crate::discovery::origin_url)
                .and_then(|url| crate::discovery::parse_github_remote(&url))
                .or_else(|| crate::manual::parse_repo(m.repo.as_deref()?))
        })
        .collect();
    let wanted: HashSet<String> = manual_repos
        .iter()
        .flatten()
        .map(|(owner, repo)| key(&format!("{owner}/{repo}")))
        .collect();
    let listed_for_manual: Vec<RemoteRepo> = remote
        .iter()
        .filter(|repo| wanted.contains(&repo_key(repo)))
        .cloned()
        .collect();
    let remote = remote
        .into_iter()
        .filter_map(|repo| {
            let found = Found {
                owner: Some(&repo.owner),
                name: Some(&repo.name),
                config: repo.config.as_ref(),
                template: repo.template.as_deref(),
                can_push: repo.can_push,
            };
            let (config, by) = identify_by(found, &classrooms, &manifests)?;
            Some((repo, config, by))
        })
        .collect();
    let mut remote = without_doubtful_templates(remote);
    // A manual project's repo is read with the assignments' (head, default branch), unless
    // it is one of them: Classroom 50 wins.
    let identified_ids: HashSet<String> = remote.iter().map(|(repo, _)| repo_key(repo)).collect();
    let manual_ids: HashSet<String> = manual_repos
        .iter()
        .flatten()
        .map(|(owner, repo)| key(&format!("{owner}/{repo}")))
        .filter(|id| !identified_ids.contains(id))
        .collect();
    let identified_count = remote.len();
    remote.extend(
        listed_for_manual
            .into_iter()
            .filter(|repo| manual_ids.contains(&repo_key(repo)))
            .map(|repo| {
                let config = RepoConfig {
                    classroom: String::new(),
                    assignment: String::new(),
                    secret: None,
                };
                (repo, config)
            }),
    );
    let remote = match status {
        Some(auth) => {
            let patterns: Vec<Vec<String>> = remote
                .iter()
                .take(identified_count)
                .map(|(repo, config)| milestone_patterns(&repo.owner, config, &manifests))
                .collect();
            let remote = refresh_statuses(state, http, auth, remote, &patterns, progress).await;
            progress.finish_phase(Phase::Statuses);
            remote
        }
        None => {
            progress.skip(Phase::Statuses);
            remote
        }
    };
    let (remote, manual_remote): (Vec<_>, Vec<_>) = remote
        .into_iter()
        .partition(|(repo, _)| !manual_ids.contains(&repo_key(repo)));
    let manual_remote: HashMap<String, RemoteRepo> = manual_remote
        .into_iter()
        .map(|(repo, _)| (repo_key(&repo), repo))
        .collect();
    // A clone made before its assignment was renamed joins the renamed repo's row.
    let listed: HashMap<String, String> = remote
        .iter()
        .map(|(repo, _)| {
            (
                key(&format!("{}/{}", repo.owner, repo.name)),
                repo.name.clone(),
            )
        })
        .collect();
    // A clone without a marker whose repo GitHub knew by its template.
    let identified: HashMap<String, &RepoConfig> = remote
        .iter()
        .map(|(repo, config)| (key(&format!("{}/{}", repo.owner, repo.name)), config))
        .collect();
    let locals: Vec<(LocalProject, RepoConfig)> = locals
        .into_iter()
        .map(|mut clone| {
            if let (Some(owner), Some(name)) = (&clone.owner, &clone.repo)
                && let Some(renamed) = renamed_clone(owner, name, &classrooms, &manifests, &listed)
            {
                clone.repo = Some(renamed);
            }
            clone
        })
        .filter_map(|clone| {
            let found = Found {
                owner: clone.owner.as_deref(),
                name: clone.repo.as_deref(),
                config: clone.config.as_ref(),
                template: None,
                can_push: false,
            };
            let config = identify(found, &classrooms, &manifests).or_else(|| {
                let id = key(&format!(
                    "{}/{}",
                    clone.owner.as_deref()?,
                    clone.repo.as_deref()?
                ));
                identified.get(&id).map(|config| (*config).clone())
            })?;
            Some((clone, config))
        })
        .collect();
    let manual_paths: Vec<PathBuf> = manual.iter().filter_map(|m| m.path.clone()).collect();
    let mut states = local_states(
        locals
            .iter()
            .map(|(clone, _)| clone.path.clone())
            .chain(manual_paths.iter().cloned()),
        progress,
    )
    .await;
    let manual_states: HashMap<PathBuf, Option<super::LocalState>> = manual_paths
        .into_iter()
        .zip(states.split_off(locals.len()))
        .collect();

    let mut projects = merge(
        remote,
        locals
            .into_iter()
            .zip(states)
            .map(|((c, cfg), s)| (c, cfg, s))
            .collect(),
    );
    for project in &mut projects {
        let key = project.owner.as_deref().map(|owner| {
            (
                owner.to_ascii_lowercase(),
                project.config.classroom.to_ascii_lowercase(),
            )
        });
        project.classroom = key.as_ref().and_then(|k| classrooms.get(k)).cloned();
        project.assignment = key
            .as_ref()
            .and_then(|k| manifests.get(k))
            .and_then(|list| {
                list.iter()
                    .find(|a| a.slug.eq_ignore_ascii_case(&project.config.assignment))
            })
            .cloned();
        project.web_url = project.owner.as_deref().map(|owner| {
            classroom50::submission_url(
                owner,
                &project.config.classroom,
                &project.config.assignment,
            )
        });
    }
    add_manual(
        &mut projects,
        manual.into_iter().zip(manual_repos).collect(),
        manual_remote,
        &manual_states,
    );
    check_clones(&mut projects).await;
    progress.finish_phase(Phase::Clones);
    for project in &mut projects {
        project.pending = pending(project);
        project.needs_work = crate::alerts::needs_work(project);
        project.late_seconds = late_seconds(project);
    }
    projects.sort_by_cached_key(|p| sort_key(p, now));
    let unaccepted = if discovered {
        unaccepted(&projects, &classrooms, &manifests)
    } else {
        Vec::new()
    };
    (projects, unaccepted)
}

/// Adds each manual project (with the GitHub repo it lives in) to the Classroom 50 ones,
/// with GitHub's data and the folder's state; one whose repo or folder already is a
/// Classroom 50 project is left out, that one showing.
fn add_manual(
    projects: &mut Vec<Project>,
    manual: Vec<(ManualProject, Option<(String, String)>)>,
    mut remote: HashMap<String, RemoteRepo>,
    states: &HashMap<PathBuf, Option<super::LocalState>>,
) {
    let listed_ids: HashSet<String> = projects.iter().map(|p| p.id.clone()).collect();
    let listed_paths: HashSet<PathBuf> = projects.iter().filter_map(|p| p.path.clone()).collect();
    for (entry, repo) in manual {
        let id = repo
            .as_ref()
            .map(|(owner, name)| key(&format!("{owner}/{name}")));
        if id.as_ref().is_some_and(|id| listed_ids.contains(id))
            || entry
                .path
                .as_ref()
                .is_some_and(|p| listed_paths.contains(p))
        {
            log::info!("a manual project is also a Classroom 50 one; showing that one");
            continue;
        }
        let github = id.as_ref().and_then(|id| remote.remove(id));
        let local = entry
            .path
            .as_ref()
            .and_then(|p| states.get(p).cloned().flatten());
        projects.push(manual_project(entry, repo, github, local));
    }
}

fn repo_key(repo: &RemoteRepo) -> String {
    key(&format!("{}/{}", repo.owner, repo.name))
}

/// A manual project's row: its name and class stand where a repo's marker would, and it
/// never has a classroom, an assignment or a Classroom 50 page.
fn manual_project(
    entry: ManualProject,
    repo: Option<(String, String)>,
    remote: Option<RemoteRepo>,
    local: Option<super::LocalState>,
) -> Project {
    let (owner, name) = repo.unzip();
    Project {
        manual: Some(ManualInfo::from(&entry)),
        id: entry.id,
        owner,
        repo: name,
        config: RepoConfig {
            classroom: entry.class.unwrap_or_default(),
            assignment: entry.name,
            secret: None,
        },
        path: entry.path,
        local,
        remote,
        classroom: None,
        assignment: None,
        pending: None,
        needs_work: false,
        web_url: None,
        late_seconds: None,
        editor: None,
        last_editor: None,
        group: None,
        phases: Vec::new(),
    }
}

/// The milestone tag patterns of the assignment `config` names in `owner`'s classroom.
fn milestone_patterns(
    owner: &str,
    config: &RepoConfig,
    manifests: &HashMap<ClassroomKey, Vec<Assignment>>,
) -> Vec<String> {
    let key = (
        owner.to_ascii_lowercase(),
        config.classroom.to_ascii_lowercase(),
    );
    manifests
        .get(&key)
        .and_then(|list| classroom50::assignment_for_slug(&config.assignment, list))
        .map(|a| a.submission_tags.clone())
        .unwrap_or_default()
}

/// Open assignments of the user's classrooms with no repo yet, published or upcoming, by
/// due date. An assignment counts as accepted when a repo of the classroom's org names it
/// in its marker file or by the naming rule, slugs compared without regard to case (a marker
/// written by hand or by an older CLI may differ from the manifest in case, or name the
/// assignment differently from the repo).
pub(super) fn unaccepted(
    projects: &[Project],
    classrooms: &BTreeMap<ClassroomKey, Classroom>,
    manifests: &HashMap<ClassroomKey, Vec<Assignment>>,
) -> Vec<Unaccepted> {
    let mut accepted: HashSet<(ClassroomKey, String)> = HashSet::new();
    for p in projects {
        let Some(owner) = p.owner.as_deref().map(str::to_ascii_lowercase) else {
            continue;
        };
        accepted.insert((
            (owner.clone(), p.config.classroom.to_ascii_lowercase()),
            p.config.assignment.to_ascii_lowercase(),
        ));
        let Some(name) = p.repo.as_deref() else {
            continue;
        };
        for (key, classroom) in classrooms.iter().filter(|((org, _), _)| *org == owner) {
            let Some(list) = manifests.get(key) else {
                continue;
            };
            if let Some(found) = classroom50::assignment_for_repo(name, &classroom.slug, list) {
                accepted.insert((key.clone(), found.assignment.slug.to_ascii_lowercase()));
            }
        }
    }
    let mut list: Vec<Unaccepted> = manifests
        .iter()
        .filter_map(|(key, assignments)| Some((classrooms.get(key)?, key, assignments)))
        .filter(|(classroom, _, _)| classroom.active)
        .flat_map(|(classroom, key, assignments)| {
            assignments
                .iter()
                // Upcoming ones (`available_from` ahead) are listed too, as "Opens ...".
                .filter(|a| !a.closed)
                .filter(|a| !accepted.contains(&(key.clone(), a.slug.to_ascii_lowercase())))
                .map(|a| Unaccepted {
                    id: format!("{}/{}/{}", key.0, key.1, a.slug),
                    classroom: classroom.clone(),
                    assignment: a.clone(),
                    accept_url: classroom50::accept_url(
                        &classroom.org,
                        &classroom.slug,
                        &a.slug,
                        classroom.secret.as_deref(),
                    ),
                    group: None,
                })
        })
        .collect();
    list.sort_by_key(|u| (u.assignment.due.is_none(), u.assignment.due));
    list
}

/// Fetches each classroom's manifest once (conditional GET), or without `network` takes
/// the last downloaded copy whatever its age. A failed download falls back to that copy
/// too: a repo without a marker file is only recognized through the manifest, and would
/// otherwise drop out of the list, its assignment showing as not accepted.
async fn fetch_manifests(
    classrooms: &BTreeMap<ClassroomKey, Classroom>,
    cache: &HttpCache,
    http: &reqwest::Client,
    network: bool,
    progress: &Progress,
) -> HashMap<ClassroomKey, Vec<Assignment>> {
    progress.set_total(Phase::Manifests, classrooms.len());
    let fetches = classrooms.iter().map(|(key, classroom)| async move {
        let url = classroom.assignments_url();
        let body = if network {
            cache.get_text_or_cached(http, &url, MANIFEST_MAX_AGE).await
        } else {
            cache.cached_text(&url).ok_or(Error::Offline)
        };
        // Logged without the key of an unlisted class, which the URL (and a request
        // error quoting it) holds.
        let manifest = match body {
            Ok(body) => classroom50::parse_assignments(&url, &body)
                .inspect_err(|err| log::warn!("{}", classroom.hide_key(&err.to_string())))
                .ok(),
            Err(err) => {
                let message = format!("could not fetch {url}: {err}");
                log::warn!("{}", classroom.hide_key(&message));
                None
            }
        };
        progress.advance(Phase::Manifests);
        manifest.map(|m| (key.clone(), m))
    });
    let manifests = join_all(fetches).await.into_iter().flatten().collect();
    progress.finish_phase(Phase::Manifests);
    manifests
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    use crate::projects::fixtures::remote;

    #[test]
    fn lists_only_assignments_without_a_repo() {
        let poo = Classroom::from_team("HEIGVD-POO", "classroom50-26-poo-c", None).unwrap();
        let classrooms = BTreeMap::from([(poo.key(), poo)]);
        let manifest: Vec<Assignment> = serde_json::from_value(json!([
            {"slug": "lab01", "name": "Labo 1", "mode": "team"},
            {"slug": "lab02", "name": "Labo 2", "mode": "team"},
            {"slug": "labo-3", "name": "Labo 3", "mode": "team"},
            {"slug": "labo-4", "name": "Labo 4", "mode": "team"},
            {"slug": "labo-5", "name": "Labo 5", "mode": "team"},
            {"slug": "old", "name": "Old", "mode": "team", "closed": true}
        ]))
        .unwrap();
        let manifests =
            HashMap::from([(("heigvd-poo".to_owned(), "26-poo-c".to_owned()), manifest)]);
        let poo_config = |assignment: &str| RepoConfig {
            classroom: "26-poo-c".into(),
            assignment: assignment.into(),
            secret: None,
        };
        let on_github = |name: &str, config: RepoConfig| {
            let mut repo = remote(name, "aaa", None);
            repo.owner = "HEIGVD-POO".into();
            (repo, config)
        };
        let clone = |path: &str, owner: Option<&str>, name: Option<&str>| LocalProject {
            path: path.into(),
            owner: owner.map(str::to_owned),
            repo: name.map(str::to_owned),
            config: Some(poo_config("labo-4")),
        };
        let projects = merge(
            vec![
                // A marker spelled in another case than the manifest.
                on_github("26-poo-c-lab01-group-3", poo_config("LAB01")),
                // A marker naming something else; the repo's name still says lab02.
                on_github("26-poo-c-lab02-group-6", poo_config("template-lab")),
            ],
            vec![
                // A clone not seen on GitHub yet, found by its origin.
                (
                    clone(
                        "/code/lab3",
                        Some("heigvd-poo"),
                        Some("26-poo-c-labo-3-alice"),
                    ),
                    poo_config("labo-3"),
                    None,
                ),
                // No GitHub origin: which org it belongs to is unknown.
                (clone("/code/copy", None, None), poo_config("labo-4"), None),
            ],
        );

        let list = unaccepted(&projects, &classrooms, &manifests);
        let slugs: Vec<&str> = list.iter().map(|u| u.assignment.slug.as_str()).collect();
        assert_eq!(slugs, ["labo-4", "labo-5"]);
        assert_eq!(list[0].id, "heigvd-poo/26-poo-c/labo-4");
    }

    fn manual_entry(id: &str, name: &str) -> ManualProject {
        ManualProject {
            id: id.into(),
            name: name.into(),
            class: Some("Networks".into()),
            due: None,
            notes: Some("Bring the report".into()),
            done: false,
            path: None,
            repo: None,
        }
    }

    #[test]
    fn manual_projects_join_the_list_unless_classroom_50_has_them() {
        let config = RepoConfig {
            classroom: "fall".into(),
            assignment: "hello".into(),
            secret: None,
        };
        let mut c50 = remote("fall-hello-alice", "aaa", None);
        c50.owner = "cs50".into();
        let clone = LocalProject {
            path: "/code/lab".into(),
            owner: None,
            repo: None,
            config: Some(config.clone()),
        };
        let mut projects = merge(vec![(c50, config.clone())], vec![(clone, config, None)]);

        let mut own = remote("essay", "bbb", None);
        own.owner = "alice".into();
        let mut with_folder = manual_entry("manual:2", "Report");
        with_folder.path = Some("/code/report".into());
        let mut same_folder = manual_entry("manual:4", "Copy of the lab");
        same_folder.path = Some("/code/lab".into());
        let manual = vec![
            (
                manual_entry("manual:1", "Essay"),
                Some(("Alice".into(), "essay".into())),
            ),
            (with_folder, None),
            // Already a Classroom 50 project, by repo or by folder.
            (
                manual_entry("manual:3", "Hello"),
                Some(("cs50".into(), "fall-hello-alice".into())),
            ),
            (same_folder, None),
        ];
        let states = HashMap::from([(PathBuf::from("/code/report"), None)]);
        add_manual(
            &mut projects,
            manual,
            HashMap::from([("alice/essay".to_owned(), own)]),
            &states,
        );

        let ids: Vec<&str> = projects.iter().map(|p| p.id.as_str()).collect();
        assert_eq!(
            ids,
            ["/code/lab", "cs50/fall-hello-alice", "manual:1", "manual:2"]
        );
        let essay = &projects[2];
        assert_eq!(essay.config.assignment, "Essay");
        assert_eq!(essay.config.classroom, "Networks");
        assert_eq!(essay.owner.as_deref(), Some("Alice"));
        assert_eq!(
            essay.remote.as_ref().map(|r| r.name.as_str()),
            Some("essay")
        );
        assert!(essay.assignment.is_none() && essay.web_url.is_none());
        assert_eq!(
            essay.manual.as_ref().and_then(|m| m.notes.as_deref()),
            Some("Bring the report")
        );
        assert_eq!(
            projects[3].path.as_deref(),
            Some(std::path::Path::new("/code/report"))
        );
    }
}
