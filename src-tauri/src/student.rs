//! Student actions that write to GitHub, done by shelling out to `gh student` (its accept
//! and submit logic has many steps and changes upstream). The CLI never prompts here: it
//! only reads a terminal when stdin and stderr both are one, and `process` pipes stderr.

use std::collections::HashMap;
use std::ffi::OsString;
use std::path::Path;
use std::sync::Mutex;
use std::time::Duration;

use serde::Serialize;

use crate::classroom50::{self, Assignment, REPO_CONFIG_FILE, RepoConfig};
use crate::error::{Error, Result};
use crate::process;
use crate::push;
use crate::tools::{self, Tool, ToolStatus};

/// Accept's retry loops are bounded at a minute or two.
const ACCEPT_TIMEOUT: Duration = Duration::from_secs(180);
/// Submit clones the repo and pushes; gh student gives up on git after 10 minutes.
const SUBMIT_TIMEOUT: Duration = Duration::from_secs(11 * 60);
/// Adding someone is one or two API calls.
const ADD_TIMEOUT: Duration = Duration::from_secs(60);

/// How a classmate gets push access, which depends on the assignment.
pub enum Addition<'a> {
    /// `gh student team add`: joins the group of a team assignment (founders only).
    Team {
        org: &'a str,
        classroom: &'a str,
        assignment: &'a str,
        /// The unlisted classroom's key.
        key: Option<&'a str>,
    },
    /// `gh student invite`: a collaborator on the student's own repo. Run inside the clone
    /// when there is one, where it also checks a group assignment's size limit.
    Invite {
        repo: &'a str,
        clone: Option<&'a Path>,
    },
}

/// Where to add classmates to a group founded from the app, by the accepted assignment's id
/// (`org/classroom/assignment`), while the sync that lists the new repo hasn't run yet.
#[derive(Default)]
pub struct FoundedGroups(Mutex<HashMap<String, FoundedGroup>>);

#[derive(Clone)]
pub struct FoundedGroup {
    pub org: String,
    pub classroom: String,
    pub assignment: String,
    pub key: Option<String>,
}

impl FoundedGroups {
    pub fn insert(&self, id: &str, group: FoundedGroup) {
        let mut map = self.0.lock().unwrap_or_else(|e| e.into_inner());
        map.insert(id.to_owned(), group);
    }

    pub fn get(&self, id: &str) -> Option<FoundedGroup> {
        let map = self.0.lock().unwrap_or_else(|e| e.into_inner());
        map.get(id).cloned()
    }
}

/// A GitHub login: letters, digits and single inner hyphens, at most 39 characters. Checked
/// before it reaches gh, so it can never be read as a flag.
pub fn valid_login(login: &str) -> bool {
    let bytes = login.as_bytes();
    !bytes.is_empty()
        && bytes.len() <= 39
        && bytes
            .iter()
            .all(|b| b.is_ascii_alphanumeric() || *b == b'-')
        && bytes[0] != b'-'
        && bytes[bytes.len() - 1] != b'-'
        && !login.contains("--")
}

/// Gives `username` push access, with `team add` or `invite`.
pub async fn add(addition: &Addition<'_>, username: &str) -> Result<()> {
    let args = add_args(addition, username)?;
    let temp = std::env::temp_dir();
    let cwd = match addition {
        Addition::Invite {
            clone: Some(clone), ..
        } => clone,
        _ => temp.as_path(),
    };
    run(args, cwd, ADD_TIMEOUT).await?;
    Ok(())
}

/// The `gh` arguments for an addition, after checking the username.
fn add_args(addition: &Addition<'_>, username: &str) -> Result<Vec<OsString>> {
    let username = username.trim().trim_start_matches('@');
    if !valid_login(username) {
        return Err(Error::InvalidUsername(username.to_owned()));
    }
    let mut args: Vec<&str> = match addition {
        Addition::Team {
            org,
            classroom,
            assignment,
            ..
        } => vec![
            "student", "team", "add", org, classroom, assignment, username,
        ],
        Addition::Invite { repo, .. } => vec!["student", "invite", repo, username],
    };
    if let Addition::Team { key: Some(key), .. } = addition {
        args.extend(["--key", key]);
    }
    Ok(args.into_iter().map(OsString::from).collect())
}

/// The longest group name, in UTF-16 units as upstream's web app counts them (its group
/// name fields are `maxLength={80}`; `gh student` itself only trims `--team-name`).
pub const MAX_TEAM_NAME: usize = 80;

/// A group's display name: not blank, at most `MAX_TEAM_NAME` long, no control characters
/// (the name lands in the team's one-line description record). Checked after trimming.
pub fn valid_team_name(name: &str) -> bool {
    !name.is_empty()
        && name.trim() == name
        && name.encode_utf16().count() <= MAX_TEAM_NAME
        && !name.chars().any(char::is_control)
}

/// What accept needs to know about an assignment.
pub struct AcceptRequest<'a> {
    pub org: &'a str,
    pub classroom: &'a str,
    pub assignment: &'a str,
    /// The unlisted classroom's key.
    pub key: Option<&'a str>,
    /// Found a new group with this name (team assignments formed by students). A group is
    /// never founded without one.
    pub new_team: Option<&'a str>,
}

/// Runs `gh student accept` and returns the repo's `owner/name`. Accepting again is safe:
/// an existing repo is reported as accepted (and repaired if its setup was cut short).
pub async fn accept(request: &AcceptRequest<'_>) -> Result<String> {
    let args = accept_args(request)?;
    // Accept prints extra advice when run inside a git repo.
    let cwd = std::env::temp_dir();
    let output = run(args, &cwd, ACCEPT_TIMEOUT).await?;
    accepted_repo(&output.stdout).ok_or_else(|| {
        Error::Student(format!(
            "gh student accept succeeded but printed no repository: {}",
            output.stdout
        ))
    })
}

/// The `gh` arguments for an accept, after checking the group name. `--new-team` always
/// comes with `--team-name`, in its `=` form so no name can be read as another flag.
fn accept_args(request: &AcceptRequest<'_>) -> Result<Vec<OsString>> {
    let mut args: Vec<OsString> = [
        "student",
        "accept",
        request.org,
        request.classroom,
        request.assignment,
    ]
    .into_iter()
    .map(OsString::from)
    .collect();
    if let Some(key) = request.key {
        args.push("--key".into());
        args.push(key.into());
    }
    if let Some(name) = request.new_team {
        let name = name.trim();
        if !valid_team_name(name) {
            return Err(Error::InvalidTeamName);
        }
        args.push("--new-team".into());
        args.push(format!("--team-name={name}").into());
    }
    Ok(args)
}

/// What a submit is about to send, for the confirmation. `gh student submit` snapshots the
/// working tree (tracked files and untracked ones that aren't ignored) and pushes it to the
/// default branch as a new commit on top of GitHub's head; the clone itself isn't changed.
/// The app only submits a clean clone, so what is sent is what is committed.
#[derive(Debug, Clone, Default, Serialize)]
#[cfg_attr(test, derive(specta::Type))]
#[serde(rename_all = "camelCase")]
pub struct SubmitCheck {
    /// The clone has `.classroom50.yaml`. Without it (`no_autograder` and `empty_repo`
    /// assignments, or a deleted marker) gh-student finds the assignment from the repo's name
    /// (`MIN_GH_STUDENT`), which the check has already made sure it can.
    pub has_marker: bool,
    /// Changes not committed yet (new, modified, deleted, staged or not), editor, OS and
    /// build files aside: submit refuses while there are any.
    pub uncommitted: Vec<String>,
    /// Untracked editor, OS and build files (`noise`, by root): they don't block a submit,
    /// but `gh student submit` sends them too unless a `.gitignore` lists them.
    pub editor_files: Vec<String>,
}

/// What the app knows of a clone's assignment, which a clone without `.classroom50.yaml`
/// needs: gh-student then finds the assignment from the repo's name in the class's published
/// list, which an unlisted class only opens with its key.
#[derive(Debug, Clone, Copy, Default)]
pub struct SubmitTarget<'a> {
    /// The repo's name matches an assignment of its class's manifest.
    pub assignment_known: bool,
    /// The unlisted classroom's key.
    pub key: Option<&'a str>,
}

/// Lists what `check` reports, without taking the index lock. A clone without a marker is
/// refused here (`markerless`) when gh-student couldn't submit it.
pub async fn check(path: &Path, target: SubmitTarget<'_>) -> Result<SubmitCheck> {
    let has_marker = path.join(REPO_CONFIG_FILE).is_file();
    if !has_marker {
        markerless(target, || tools::status(Tool::GhStudent)).await?;
    }
    let tree = push::work_tree(path).await?;
    Ok(SubmitCheck {
        has_marker,
        uncommitted: tree.files,
        editor_files: tree.skipped,
    })
}

/// A clone without a marker submits only when its assignment is known (else gh-student
/// can't find it either: `Error::UnknownAssignment`) and gh-student finds assignments by
/// name (`MIN_GH_STUDENT`, else `Error::GhStudentOutdated`). gh-student's status is only
/// read once the assignment is known.
async fn markerless<F, Fut>(target: SubmitTarget<'_>, gh_student: F) -> Result<()>
where
    F: FnOnce() -> Fut,
    Fut: std::future::Future<Output = ToolStatus>,
{
    if !target.assignment_known {
        return Err(Error::UnknownAssignment);
    }
    let status = gh_student().await;
    if status.outdated {
        return Err(Error::GhStudentOutdated {
            version: status.version.unwrap_or_default(),
        });
    }
    Ok(())
}

#[derive(Debug, Clone, Serialize)]
#[cfg_attr(test, derive(specta::Type))]
#[serde(rename_all = "camelCase")]
pub struct Submitted {
    /// The submission commit on GitHub.
    pub url: Option<String>,
}

/// Runs `gh student submit` in the clone at `path`, which must have nothing uncommitted
/// (`Error::LocalChanges`): the snapshot would send work the student never committed.
pub async fn submit(path: &Path, target: SubmitTarget<'_>) -> Result<Submitted> {
    let check = check(path, target).await?;
    if !check.uncommitted.is_empty() {
        return Err(Error::LocalChanges);
    }
    let output = run(
        submit_args(check.has_marker, target.key),
        path,
        SUBMIT_TIMEOUT,
    )
    .await?;
    Ok(Submitted {
        url: submission_url(&output.stdout),
    })
}

/// `gh student submit`, plus `--key=<key>` for a clone without a marker in an unlisted
/// class: gh-student reads the class's published list under that key to find the
/// assignment. Never with a marker, which records its own key (gh-student refuses one that
/// differs), nor a key that isn't one (`valid_secret`), which gh-student would refuse too.
fn submit_args(has_marker: bool, key: Option<&str>) -> Vec<OsString> {
    let mut args = vec![OsString::from("student"), "submit".into()];
    if let Some(key) = key.filter(|key| !has_marker && classroom50::valid_secret(key)) {
        args.push(format!("--key={key}").into());
    }
    args
}

/// Whether the clone's marker (`local`) still names the slug `assignment` was renamed from
/// while GitHub's (`github`) doesn't: `gh teacher assignment rename` rewrote it, and
/// `gh student submit`, which pushes the clone's files as they are, would put the old one
/// back. Pulling first brings the new marker. A repo the rename skipped (GitHub's marker
/// old too) is left to gh-student.
pub fn stale_marker(
    local: Option<&RepoConfig>,
    github: Option<&RepoConfig>,
    assignment: &Assignment,
) -> bool {
    let Some(old) = assignment.old_slug() else {
        return false;
    };
    let names_old = |marker: &RepoConfig| marker.assignment.eq_ignore_ascii_case(old);
    local.is_some_and(names_old) && !github.is_some_and(names_old)
}

/// `View your submission at: https://github.com/o/r/commit/<sha>`.
fn submission_url(stdout: &str) -> Option<String> {
    stdout.lines().find_map(|line| {
        let url = line.strip_prefix("View your submission at: ")?.trim();
        url.starts_with("https://").then(|| url.to_owned())
    })
}

/// Runs `gh student`; past `timeout`, gh and the extension it started are both killed.
async fn run(args: Vec<OsString>, cwd: &Path, timeout: Duration) -> Result<process::Output> {
    match process::run_with_timeout("gh", args, Some(cwd), timeout).await {
        Err(Error::TimedOut { minutes, .. }) => Err(Error::Student(format!(
            "gh student didn't finish within {minutes} minutes"
        ))),
        Err(Error::CommandFailed { stderr, .. }) => Err(failure(&stderr)),
        result => result,
    }
}

/// `Assignment accepted: org/repo`, or `Assignment already accepted: org/repo`.
fn accepted_repo(stdout: &str) -> Option<String> {
    stdout.lines().find_map(|line| {
        let name = line
            .strip_prefix("Assignment accepted: ")
            .or_else(|| line.strip_prefix("Assignment already accepted: "))?
            .trim();
        name.contains('/').then(|| name.to_owned())
    })
}

/// gh student's messages are written for students already; keep the error and drop the
/// progress lines before it. A team assignment without a group gets its own error, so the
/// UI can offer to found one.
fn failure(stderr: &str) -> Error {
    let message = match stderr.rfind("gh-student: ") {
        Some(at) => stderr[at + "gh-student: ".len()..].trim(),
        None => stderr.lines().last().unwrap_or_default().trim(),
    };
    if message.contains("with --new-team to create one") {
        return Error::NeedsTeam;
    }
    Error::Student(message.to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_marker_from_before_a_rename_needs_a_pull() {
        let assignment: Assignment = serde_json::from_value(serde_json::json!({
            "slug": "ps3", "name": "PS3", "mode": "individual", "renamed_from": "problem-set-three"
        }))
        .unwrap();
        let marker = |slug: &str| RepoConfig {
            classroom: "cs".into(),
            assignment: slug.into(),
            secret: None,
        };
        let (old, new) = (marker("Problem-Set-Three"), marker("ps3"));
        // Rewritten on GitHub, not pulled yet; or no GitHub data to tell.
        assert!(stale_marker(Some(&old), Some(&new), &assignment));
        assert!(stale_marker(Some(&old), None, &assignment));
        // Pulled, a repo the rename skipped, no marker in the clone.
        assert!(!stale_marker(Some(&new), Some(&new), &assignment));
        assert!(!stale_marker(Some(&old), Some(&old), &assignment));
        assert!(!stale_marker(None, Some(&new), &assignment));
        // Never renamed.
        let mut plain = assignment.clone();
        plain.renamed_from = None;
        assert!(!stale_marker(Some(&old), Some(&new), &plain));
    }

    #[test]
    fn reads_the_accepted_repo() {
        let stdout = "Assignment accepted: HEIGVD-POO/poo-lab1-romain\n\nClone it with HTTPS:\n";
        assert_eq!(
            accepted_repo(stdout).as_deref(),
            Some("HEIGVD-POO/poo-lab1-romain")
        );
        assert_eq!(
            accepted_repo("Assignment already accepted: o/r").as_deref(),
            Some("o/r")
        );
        assert_eq!(accepted_repo("Something else"), None);
    }

    #[test]
    fn reads_the_submission_url() {
        let stdout = "Submitted assignment \"Lab 1\" at 2026-10-15 14:03:22 CEST\n\
            View your submission at: https://github.com/o/r/commit/abc123";
        assert_eq!(
            submission_url(stdout).as_deref(),
            Some("https://github.com/o/r/commit/abc123")
        );
        assert_eq!(submission_url("Submission pushed"), None);
    }

    #[test]
    fn accepts_only_github_logins() {
        assert!(valid_login("cs50-duck"));
        assert!(valid_login("A1"));
        assert!(!valid_login(""));
        assert!(!valid_login("--team"));
        assert!(!valid_login("a--b"));
        assert!(!valid_login("trailing-"));
        assert!(!valid_login("two words"));
        assert!(!valid_login(&"a".repeat(40)));
    }

    #[test]
    fn builds_team_add_and_invite_arguments() {
        let words = |args: Vec<OsString>| -> Vec<String> {
            args.iter()
                .map(|a| a.to_string_lossy().into_owned())
                .collect()
        };
        let team = Addition::Team {
            org: "HEIGVD-POO",
            classroom: "26-poo-c",
            assignment: "labo-3",
            key: Some("s3cret"),
        };
        assert_eq!(
            words(add_args(&team, " @cs50-duck ").unwrap()),
            [
                "student",
                "team",
                "add",
                "HEIGVD-POO",
                "26-poo-c",
                "labo-3",
                "cs50-duck",
                "--key",
                "s3cret"
            ]
        );
        let invite = Addition::Invite {
            repo: "o/r",
            clone: None,
        };
        assert_eq!(
            words(add_args(&invite, "alice").unwrap()),
            ["student", "invite", "o/r", "alice"]
        );
        assert!(matches!(
            add_args(&invite, "--help"),
            Err(Error::InvalidUsername(_))
        ));
    }

    /// Dev builds must refuse both writes, whatever the flags.
    #[test]
    fn additions_count_as_writes_to_github() {
        let team = Addition::Team {
            org: "o",
            classroom: "c",
            assignment: "a",
            key: Some("k"),
        };
        let invite = Addition::Invite {
            repo: "o/r",
            clone: None,
        };
        for addition in [team, invite] {
            let args = add_args(&addition, "alice").unwrap();
            assert!(process::writes_to_github("gh", &args));
        }
    }

    #[test]
    fn accepts_only_usable_group_names() {
        assert!(valid_team_name("The Sharks"));
        assert!(valid_team_name("--help"));
        assert!(valid_team_name("Équipe 3 🦈"));
        assert!(valid_team_name(&"é".repeat(MAX_TEAM_NAME)));
        assert!(!valid_team_name(""));
        assert!(!valid_team_name(" padded "));
        assert!(!valid_team_name("two\nlines"));
        assert!(!valid_team_name("tab\there"));
        assert!(!valid_team_name(&"a".repeat(MAX_TEAM_NAME + 1)));
        // An emoji is two UTF-16 units, as the web app's `maxLength` counts it.
        assert!(!valid_team_name(&"🦈".repeat(MAX_TEAM_NAME / 2 + 1)));
    }

    #[test]
    fn builds_accept_arguments() {
        let words = |args: Vec<OsString>| -> Vec<String> {
            args.iter()
                .map(|a| a.to_string_lossy().into_owned())
                .collect()
        };
        let request = |key, new_team| AcceptRequest {
            org: "HEIGVD-POO",
            classroom: "26-poo-c",
            assignment: "labo-3",
            key,
            new_team,
        };
        assert_eq!(
            words(accept_args(&request(None, None)).unwrap()),
            ["student", "accept", "HEIGVD-POO", "26-poo-c", "labo-3"]
        );
        assert_eq!(
            words(accept_args(&request(Some("s3cret"), Some("  The Sharks "))).unwrap()),
            [
                "student",
                "accept",
                "HEIGVD-POO",
                "26-poo-c",
                "labo-3",
                "--key",
                "s3cret",
                "--new-team",
                "--team-name=The Sharks"
            ]
        );
        assert_eq!(
            words(accept_args(&request(None, Some("--key"))).unwrap())
                .last()
                .unwrap(),
            "--team-name=--key"
        );
        // No path founds a group without a name.
        for name in ["", "   ", "a\nb"] {
            assert!(matches!(
                accept_args(&request(None, Some(name))),
                Err(Error::InvalidTeamName)
            ));
        }
        let args = accept_args(&request(None, Some("X"))).unwrap();
        assert!(process::writes_to_github("gh", &args));
    }

    /// Uncommitted work blocks a submit before gh runs; editor and build files don't. A dev build
    /// then refuses the submit itself, so this never reaches GitHub.
    #[tokio::test]
    async fn submits_only_a_clean_clone() {
        let root = std::env::temp_dir().join(format!("c50tb-submit-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        let git = |args: &[&str]| {
            let out = std::process::Command::new("git")
                .args(["-c", "user.name=t", "-c", "user.email=t@t"])
                .args(["-c", "commit.gpgsign=false"])
                .args(args)
                .current_dir(&root)
                .output()
                .unwrap();
            assert!(out.status.success(), "{args:?}");
        };
        git(&["init", "-q", "-b", "main"]);
        std::fs::write(root.join(REPO_CONFIG_FILE), "classroom: c\nassignment: a\n").unwrap();
        std::fs::write(root.join("a.c"), "x").unwrap();
        std::fs::write(root.join("Cargo.toml"), "[package]").unwrap();
        git(&["add", "--all"]);
        git(&["commit", "-q", "-m", "one"]);
        std::fs::write(root.join(".DS_Store"), "x").unwrap();
        std::fs::create_dir_all(root.join("target").join("debug")).unwrap();
        std::fs::write(root.join("target").join("debug").join("a"), "x").unwrap();
        let clean = check(&root, SubmitTarget::default()).await.unwrap();
        assert!(clean.has_marker);
        assert!(clean.uncommitted.is_empty());
        assert_eq!(clean.editor_files, [".DS_Store", "target/"]);

        std::fs::remove_file(root.join("a.c")).unwrap();
        std::fs::write(root.join("b.c"), "x").unwrap();
        assert_eq!(
            check(&root, SubmitTarget::default())
                .await
                .unwrap()
                .uncommitted,
            ["a.c", "b.c"]
        );
        assert!(matches!(
            submit(&root, SubmitTarget::default()).await,
            Err(Error::LocalChanges)
        ));

        git(&["add", "--all"]);
        git(&["commit", "-q", "-m", "two"]);
        if cfg!(debug_assertions) {
            assert!(matches!(
                submit(&root, SubmitTarget::default()).await,
                Err(Error::BlockedInDev(_))
            ));
        }
        let _ = std::fs::remove_dir_all(&root);
    }

    /// A clone without a marker whose assignment the app doesn't know is refused before gh
    /// runs, and before its uncommitted work is even looked at.
    #[tokio::test]
    async fn refuses_a_markerless_clone_of_an_unknown_assignment() {
        let root = std::env::temp_dir().join(format!("c50tb-markerless-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(root.join("a.c"), "x").unwrap();
        let unknown = SubmitTarget {
            assignment_known: false,
            key: Some("a1b2c3d4"),
        };
        assert!(matches!(
            check(&root, unknown).await,
            Err(Error::UnknownAssignment)
        ));
        assert!(matches!(
            submit(&root, unknown).await,
            Err(Error::UnknownAssignment)
        ));
        let _ = std::fs::remove_dir_all(&root);
    }

    #[tokio::test]
    async fn markerless_submit_needs_a_recent_gh_student() {
        let gh_student = |version: Option<&str>| {
            let version = version.map(str::to_owned);
            move || async move { ToolStatus::new(Tool::GhStudent, version.is_some(), version) }
        };
        let known = SubmitTarget {
            assignment_known: true,
            key: None,
        };
        let err = markerless(known, gh_student(Some("v1.55.0")))
            .await
            .unwrap_err();
        assert!(matches!(&err, Error::GhStudentOutdated { version } if version == "v1.55.0"));
        assert!(markerless(known, gh_student(Some("v1.56.0"))).await.is_ok());
        assert!(markerless(known, gh_student(Some("v1.58.1"))).await.is_ok());
        // A version that can't be told (a git clone of the extension) isn't refused, nor is
        // a missing gh-student (gh then says so itself).
        assert!(markerless(known, gh_student(None)).await.is_ok());
        // An unknown assignment is refused without reading gh-student's status.
        let unknown = SubmitTarget::default();
        let never = || async { panic!("gh-student's status was read") };
        assert!(matches!(
            markerless(unknown, never).await,
            Err(Error::UnknownAssignment)
        ));
    }

    #[test]
    fn passes_the_key_only_without_a_marker() {
        let words = |args: Vec<OsString>| -> Vec<String> {
            args.iter()
                .map(|a| a.to_string_lossy().into_owned())
                .collect()
        };
        assert_eq!(words(submit_args(true, None)), ["student", "submit"]);
        assert_eq!(words(submit_args(false, None)), ["student", "submit"]);
        assert_eq!(
            words(submit_args(false, Some("a1b2c3d4"))),
            ["student", "submit", "--key=a1b2c3d4"]
        );
        // The marker records its own key; gh-student refuses one that differs.
        assert_eq!(
            words(submit_args(true, Some("a1b2c3d4"))),
            ["student", "submit"]
        );
        // Not a key: never passed, so nothing else can reach gh as a flag.
        for key in ["", "--verbose", "ABCD", "a b c d", "abc"] {
            assert_eq!(
                words(submit_args(false, Some(key))),
                ["student", "submit"],
                "{key:?}"
            );
        }
        // The key doesn't hide the write from the dev build guard.
        assert!(process::writes_to_github(
            "gh",
            &submit_args(false, Some("a1b2c3d4"))
        ));
    }

    #[test]
    fn keeps_the_error_and_drops_progress() {
        let stderr = "Looking up lab1 in org/c...\nFound assignment lab1\n\
            gh-student: assignment \"lab1\" is locked by your teacher";
        assert!(matches!(
            failure(stderr),
            Error::Student(m) if m == "assignment \"lab1\" is locked by your teacher"
        ));
        let stderr = "gh-student: you are not in a group for this assignment yet. Run accept \
            again with --new-team to create one (you become its founder)";
        assert!(matches!(failure(stderr), Error::NeedsTeam));
        assert!(matches!(failure("fatal: boom"), Error::Student(m) if m == "fatal: boom"));
    }
}
