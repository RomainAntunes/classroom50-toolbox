//! Student actions that write to GitHub, done by shelling out to `gh student` (its accept
//! and submit logic has many steps and changes upstream). The CLI never prompts here: it
//! only reads a terminal when stdin and stderr both are one, and `process` pipes stderr.

use std::ffi::OsString;
use std::path::Path;
use std::time::Duration;

use serde::Serialize;

use crate::classroom50::REPO_CONFIG_FILE;
use crate::error::{Error, Result};
use crate::process;

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

/// What accept needs to know about an assignment.
pub struct AcceptRequest<'a> {
    pub org: &'a str,
    pub classroom: &'a str,
    pub assignment: &'a str,
    /// The unlisted classroom's key.
    pub key: Option<&'a str>,
    /// Found a new group (team assignments formed by students).
    pub new_team: bool,
}

/// Runs `gh student accept` and returns the repo's `owner/name`. Accepting again is safe:
/// an existing repo is reported as accepted (and repaired if its setup was cut short).
pub async fn accept(request: &AcceptRequest<'_>) -> Result<String> {
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
    if request.new_team {
        args.push("--new-team".into());
    }
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

/// What a submit is about to send, for the confirmation. `gh student submit` snapshots the
/// working tree (tracked files and untracked ones that aren't ignored) and pushes it to the
/// default branch as a new commit on top of GitHub's head; the clone itself isn't changed.
#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SubmitCheck {
    /// Submit needs `.classroom50.yaml`; `empty_repo` assignments have none and are pushed
    /// by hand.
    pub has_marker: bool,
    /// New files that will be included.
    pub untracked: Vec<String>,
    /// Tracked files deleted without `git rm`: submit fails on them.
    pub deleted: Vec<String>,
}

/// Lists what `check` reports, without taking the index lock.
pub async fn check(path: &Path) -> Result<SubmitCheck> {
    let list = |flag: &'static str| async move {
        let args = [
            "--no-optional-locks",
            "ls-files",
            flag,
            "--exclude-standard",
            "-z",
        ];
        process::run("git", args, Some(path))
            .await
            .map(|out| split_nul(&out.stdout))
    };
    let (untracked, deleted) = tokio::try_join!(list("--others"), list("--deleted"))?;
    Ok(SubmitCheck {
        has_marker: path.join(REPO_CONFIG_FILE).is_file(),
        untracked,
        deleted,
    })
}

fn split_nul(out: &str) -> Vec<String> {
    out.split('\0')
        .filter(|s| !s.is_empty())
        .map(str::to_owned)
        .collect()
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Submitted {
    /// The submission commit on GitHub.
    pub url: Option<String>,
}

/// Runs `gh student submit` in the clone at `path`.
pub async fn submit(path: &Path) -> Result<Submitted> {
    let args = vec![OsString::from("student"), "submit".into()];
    let output = run(args, path, SUBMIT_TIMEOUT).await?;
    Ok(Submitted {
        url: submission_url(&output.stdout),
    })
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
    fn splits_git_file_lists() {
        assert_eq!(
            split_nul("a.txt\0dir/b c.txt\0"),
            vec!["a.txt", "dir/b c.txt"]
        );
        assert!(split_nul("").is_empty());
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
