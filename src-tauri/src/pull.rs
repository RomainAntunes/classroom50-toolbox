//! Pulling a clone's default branch, the one write the app makes to a clone and only when
//! the student clicks Pull. It only fast-forwards: a pull that would merge, or touch
//! uncommitted work, is refused with a reason the UI explains, so it can't leave a clone
//! mid-merge with conflicts the student has to solve in a terminal.

use std::path::Path;
use std::time::Duration;

use crate::error::{Error, Result};
use crate::process;
use crate::ssh::{self, Who};

/// A pull of a student repo is small. The limit catches an ssh that waits on a prompt
/// nobody sees (a passphrase on a terminal the app doesn't show).
const PULL_TIMEOUT: Duration = Duration::from_secs(5 * 60);

/// gh's credentials for github.com, as `gh auth setup-git` would configure them, so a
/// private repo pulls without any git setup. Scoped to the command line; nothing is saved.
pub(crate) const GH_CREDENTIALS: [&str; 4] = [
    "-c",
    "credential.https://github.com.helper=",
    "-c",
    "credential.https://github.com.helper=!gh auth git-credential",
];

/// Fast-forwards `default_branch` in the clone at `path` from `origin`. `who` tells an SSH
/// key of another account from a repo the account can't reach (`ssh::failure`).
pub async fn pull(path: &Path, default_branch: &str, who: &Who<'_>) -> Result<()> {
    let branch = git(path, &["rev-parse", "--abbrev-ref", "HEAD"]).await?;
    if branch.trim() != default_branch {
        return Err(Error::OtherBranch(default_branch.to_owned()));
    }
    // Untracked files only block a pull that would overwrite them, which git reports.
    let changes = git(
        path,
        &[
            "--no-optional-locks",
            "status",
            "--porcelain",
            "--untracked-files=no",
        ],
    )
    .await?;
    if !changes.trim().is_empty() {
        return Err(Error::LocalChanges);
    }
    let args = GH_CREDENTIALS.iter().copied().chain([
        "pull",
        "--ff-only",
        "--no-rebase",
        "origin",
        default_branch,
    ]);
    match process::run_with_timeout("git", args, Some(path), PULL_TIMEOUT).await {
        Ok(_) => Ok(()),
        Err(Error::CommandFailed { stderr, .. }) => Err(failure(&stderr, who).await),
        Err(err) => Err(err),
    }
}

async fn git(path: &Path, args: &[&str]) -> Result<String> {
    Ok(process::run("git", args, Some(path)).await?.stdout)
}

/// Both sides have new commits: fast-forwarding is impossible and merging is left to the
/// student. An SSH failure is `Error::Ssh`; anything else keeps GitHub's own refusal, else
/// git's last line.
async fn failure(stderr: &str, who: &Who<'_>) -> Error {
    if let Some(err) = ssh::failure(stderr, who).await {
        return err;
    }
    if stderr.contains("Not possible to fast-forward") || stderr.contains("diverging branches") {
        return Error::Diverged;
    }
    if let Some(message) = ssh::github_error(stderr) {
        return Error::Unsupported(message);
    }
    let line = stderr
        .lines()
        .map(str::trim)
        .rfind(|l| !l.is_empty())
        .unwrap_or_default();
    Error::Unsupported(line.strip_prefix("fatal: ").unwrap_or(line).to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn alice() -> Who<'static> {
        Who::Known {
            active: Some("alice".into()),
            probe: ssh::SshStatus::Ready {
                login: Some("alice".into()),
            },
        }
    }

    #[tokio::test]
    async fn explains_failed_pulls() {
        let failure = async |stderr: &str| failure(stderr, &alice()).await;
        let stderr = "hint: Diverging branches can't be fast-forwarded, you need to either:\n\
            hint: ...\nfatal: Not possible to fast-forward, aborting.\n";
        assert!(matches!(failure(stderr).await, Error::Diverged));
        let stderr = "fatal: unable to access 'https://github.com/o/r/': Could not resolve host\n";
        assert!(matches!(
            failure(stderr).await,
            Error::Unsupported(m) if m == "unable to access 'https://github.com/o/r/': Could not resolve host"
        ));
        let stderr = "git@github.com: Permission denied (publickey).\r\n\
            fatal: Could not read from remote repository.\r\n";
        assert!(matches!(
            failure(stderr).await,
            Error::Ssh {
                problem: crate::ssh::SshProblem::KeyRejected,
                ..
            }
        ));
        // The key is the account's: GitHub's own words, not an SSH problem.
        let stderr =
            "ERROR: Repository not found.\nfatal: Could not read from remote repository.\n";
        assert!(matches!(
            failure(stderr).await,
            Error::Unsupported(m) if m == "Repository not found."
        ));
    }

    /// A real clone pulled from a local "origin": fast-forwards, refuses local changes,
    /// another branch and diverged history.
    #[tokio::test]
    async fn pulls_only_by_fast_forward() {
        let root = std::env::temp_dir().join(format!("c50tb-pull-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let origin = root.join("origin");
        let clone = root.join("clone");
        std::fs::create_dir_all(&origin).unwrap();
        let sh = |dir: &Path, args: &[&str]| {
            let out = std::process::Command::new("git")
                .args([
                    "-c",
                    "user.name=t",
                    "-c",
                    "user.email=t@t",
                    "-c",
                    "commit.gpgsign=false",
                ])
                .args(args)
                .current_dir(dir)
                .output()
                .unwrap();
            assert!(
                out.status.success(),
                "{args:?}: {}",
                String::from_utf8_lossy(&out.stderr)
            );
        };
        sh(&origin, &["init", "-q", "-b", "main"]);
        sh(&origin, &["commit", "-q", "--allow-empty", "-m", "one"]);
        sh(&root, &["clone", "-q", "origin", "clone"]);
        sh(&origin, &["commit", "-q", "--allow-empty", "-m", "two"]);

        std::fs::write(clone.join("a.txt"), "x").unwrap();
        sh(&clone, &["add", "a.txt"]);
        assert!(matches!(
            pull(&clone, "main", &alice()).await,
            Err(Error::LocalChanges)
        ));
        sh(&clone, &["rm", "-q", "--cached", "a.txt"]);

        sh(&clone, &["switch", "-q", "-c", "work"]);
        assert!(
            matches!(pull(&clone, "main", &alice()).await, Err(Error::OtherBranch(b)) if b == "main")
        );
        sh(&clone, &["switch", "-q", "main"]);

        // An untracked file doesn't block the pull.
        pull(&clone, "main", &alice()).await.unwrap();

        sh(&origin, &["commit", "-q", "--allow-empty", "-m", "three"]);
        sh(&clone, &["commit", "-q", "--allow-empty", "-m", "mine"]);
        assert!(matches!(
            pull(&clone, "main", &alice()).await,
            Err(Error::Diverged)
        ));
        let _ = std::fs::remove_dir_all(&root);
    }
}
