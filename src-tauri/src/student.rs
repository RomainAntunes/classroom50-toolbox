//! Student actions that write to GitHub, done by shelling out to `gh student` (its accept
//! and submit logic has many steps and changes upstream). The CLI never prompts here: it
//! only reads a terminal when stdin and stderr both are one, and `process` pipes stderr.

use std::ffi::OsString;
use std::path::Path;
use std::time::Duration;

use crate::error::{Error, Result};
use crate::process;

/// Accept's retry loops are bounded at a minute or two.
const ACCEPT_TIMEOUT: Duration = Duration::from_secs(180);

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

async fn run(args: Vec<OsString>, cwd: &Path, timeout: Duration) -> Result<process::Output> {
    match tokio::time::timeout(timeout, process::run("gh", args, Some(cwd))).await {
        Err(_) => Err(Error::Student(format!(
            "gh student didn't finish within {} minutes",
            timeout.as_secs() / 60
        ))),
        Ok(Err(Error::CommandFailed { stderr, .. })) => Err(failure(&stderr)),
        Ok(result) => result,
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
