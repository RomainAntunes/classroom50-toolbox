//! Pulling a clone's default branch, the one write the app makes to a clone and only when
//! the student clicks Pull. It only fast-forwards: a pull that would merge, or touch
//! uncommitted work, is refused with a reason the UI explains, so it can't leave a clone
//! mid-merge with conflicts the student has to solve in a terminal.

use std::path::Path;

use crate::error::{Error, Result};
use crate::process;

/// gh's credentials for github.com, as `gh auth setup-git` would configure them, so a
/// private repo pulls without any git setup. Scoped to the command line; nothing is saved.
pub(crate) const GH_CREDENTIALS: [&str; 4] = [
    "-c",
    "credential.https://github.com.helper=",
    "-c",
    "credential.https://github.com.helper=!gh auth git-credential",
];

/// Fast-forwards `default_branch` in the clone at `path` from `origin`.
pub async fn pull(path: &Path, default_branch: &str) -> Result<()> {
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
    match process::run("git", args, Some(path)).await {
        Ok(_) => Ok(()),
        Err(Error::CommandFailed { stderr, .. }) => Err(failure(&stderr)),
        Err(err) => Err(err),
    }
}

async fn git(path: &Path, args: &[&str]) -> Result<String> {
    Ok(process::run("git", args, Some(path)).await?.stdout)
}

/// Both sides have new commits: fast-forwarding is impossible and merging is left to the
/// student. Anything else keeps git's own last line.
fn failure(stderr: &str) -> Error {
    if stderr.contains("Not possible to fast-forward") || stderr.contains("diverging branches") {
        return Error::Diverged;
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

    #[test]
    fn explains_failed_pulls() {
        let stderr = "hint: Diverging branches can't be fast-forwarded, you need to either:\n\
            hint: ...\nfatal: Not possible to fast-forward, aborting.\n";
        assert!(matches!(failure(stderr), Error::Diverged));
        let stderr = "fatal: unable to access 'https://github.com/o/r/': Could not resolve host\n";
        assert!(matches!(
            failure(stderr),
            Error::Unsupported(m) if m == "unable to access 'https://github.com/o/r/': Could not resolve host"
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
            pull(&clone, "main").await,
            Err(Error::LocalChanges)
        ));
        sh(&clone, &["rm", "-q", "--cached", "a.txt"]);

        sh(&clone, &["switch", "-q", "-c", "work"]);
        assert!(matches!(pull(&clone, "main").await, Err(Error::OtherBranch(b)) if b == "main"));
        sh(&clone, &["switch", "-q", "main"]);

        // An untracked file doesn't block the pull.
        pull(&clone, "main").await.unwrap();

        sh(&origin, &["commit", "-q", "--allow-empty", "-m", "three"]);
        sh(&clone, &["commit", "-q", "--allow-empty", "-m", "mine"]);
        assert!(matches!(pull(&clone, "main").await, Err(Error::Diverged)));
        let _ = std::fs::remove_dir_all(&root);
    }
}
