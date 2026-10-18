//! Commit & push for every-push assignments, where each push to the default branch is
//! graded. Only on a click: the clone's changes are committed with the student's message and
//! the default branch is pushed, so local history matches GitHub (unlike `gh student
//! submit`, which pushes a snapshot and leaves the clone a commit behind). Anything that
//! would need a merge, or that git or GitHub would refuse halfway, is refused before the
//! first write with a reason the UI explains. Commit alone (`commit`) works on any clone,
//! branch or submission mode: it never pushes, so dev builds allow it.

use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::time::Duration;

use serde::Serialize;

use crate::error::{Error, Result};
use crate::noise;
use crate::process;
use crate::pull::GH_CREDENTIALS;

/// Hooks or a signing agent may take a moment.
const COMMIT_TIMEOUT: Duration = Duration::from_secs(120);
/// A push of a student repo is small; a slow network still gets its time.
const PUSH_TIMEOUT: Duration = Duration::from_secs(5 * 60);
/// GitHub warns about files over 50 MB and refuses them over 100 MB.
const LARGE_FILE: u64 = 50 * 1024 * 1024;
const MAX_FILE: u64 = 100 * 1024 * 1024;

/// What Commit & push would do, for the prompt.
#[derive(Debug, Clone, Default, Serialize)]
#[cfg_attr(test, derive(specta::Type))]
#[serde(rename_all = "camelCase")]
pub struct PushCheck {
    /// Every changed path the commit will hold (new, modified, deleted, renamed).
    pub files: Vec<String>,
    /// The new ones among them.
    pub untracked: Vec<String>,
    /// Untracked editor and OS files left out of the commit (`noise`), a folder once with
    /// its trailing `/`: `.idea/`, `.DS_Store`.
    pub skipped: Vec<String>,
    /// Over 50 MB: GitHub warns about them.
    pub large: Vec<String>,
    /// Over 100 MB: GitHub refuses them, so the commit isn't made.
    pub too_large: Vec<String>,
    /// Local commits GitHub doesn't have yet.
    pub ahead: u32,
    /// git has a `user.name` and `user.email` for this clone.
    pub has_identity: bool,
}

/// The new head commit (pushed by Commit & push, local only after a commit).
#[derive(Debug, Clone, Serialize)]
#[cfg_attr(test, derive(specta::Type))]
#[serde(rename_all = "camelCase")]
pub struct Pushed {
    pub commit: String,
}

/// Checks the clone at `path` can be pushed to `branch` without a merge. `github_head` is
/// GitHub's head of that branch, when it has one.
pub async fn check(path: &Path, branch: &str, github_head: Option<&str>) -> Result<PushCheck> {
    let current = git(path, &["symbolic-ref", "--short", "-q", "HEAD"])
        .await
        .unwrap_or_default();
    if current.trim() != branch {
        return Err(Error::OtherBranch(branch.to_owned()));
    }
    if in_progress(path).await? {
        return Err(Error::GitInProgress);
    }
    if let Some(head) = github_head {
        // Missing locally, or fetched but not merged: either way the push would be refused.
        match git(path, &["merge-base", "--is-ancestor", head, "HEAD"]).await {
            Ok(_) => {}
            Err(Error::CommandFailed { .. }) => return Err(Error::RemoteAhead),
            Err(err) => return Err(err),
        }
    }
    let mut state = changes(path).await?;
    state.ahead = ahead(path, github_head).await?;
    if state.files.is_empty() && state.ahead == 0 {
        return Err(Error::NothingToPush);
    }
    Ok(state)
}

/// What a local commit of the clone at `path` would hold (`ahead` stays 0). Lighter than
/// `check`: any branch, and GitHub may be ahead, since a commit never leaves the clone.
pub async fn commit_check(path: &Path) -> Result<PushCheck> {
    if in_progress(path).await? {
        return Err(Error::GitInProgress);
    }
    let state = changes(path).await?;
    if state.files.is_empty() {
        return Err(Error::NothingToCommit);
    }
    Ok(state)
}

/// Commits every change with `message`, after the checks of `commit_check`. Never pushes,
/// so dev builds allow it. Returns the new commit.
pub async fn commit(path: &Path, message: &str) -> Result<Pushed> {
    let state = commit_check(path).await?;
    commit_changes(path, &state, Some(message)).await?;
    let commit = git(path, &["rev-parse", "HEAD"]).await?.trim().to_owned();
    Ok(Pushed { commit })
}

/// The changed files of the clone, their sizes and whether git knows who commits.
async fn changes(path: &Path) -> Result<PushCheck> {
    let WorkTree {
        files,
        untracked,
        skipped,
    } = work_tree(path).await?;
    let (large, too_large) = sizes(path, &files);
    let has_identity = has_config(path, "user.name").await && has_config(path, "user.email").await;
    Ok(PushCheck {
        files,
        untracked,
        skipped,
        large,
        too_large,
        ahead: 0,
        has_identity,
    })
}

/// What a commit of the clone would hold, and the editor and OS files it leaves out.
#[derive(Debug, Default)]
pub(crate) struct WorkTree {
    /// Every changed path, new, modified, deleted or renamed, noise aside.
    pub files: Vec<String>,
    /// The new ones among them.
    pub untracked: Vec<String>,
    /// Untracked noise, by root (`noise::root`), file by file in a folder that also holds
    /// a file of the commit.
    pub skipped: Vec<String>,
}

/// Reads the clone's changes (every untracked file, not folders), without taking the index
/// lock.
pub(crate) async fn work_tree(path: &Path) -> Result<WorkTree> {
    let status = git(
        path,
        &[
            "--no-optional-locks",
            "status",
            "--porcelain=v1",
            "-z",
            "--untracked-files=all",
        ],
    )
    .await?;
    Ok(parse_status(&status))
}

/// `git add --all` and `git commit`, refusing files GitHub would reject and a missing
/// identity first.
async fn commit_changes(path: &Path, state: &PushCheck, message: Option<&str>) -> Result<()> {
    if !state.too_large.is_empty() {
        return Err(Error::FileTooLarge(state.too_large.clone()));
    }
    if !state.has_identity {
        return Err(Error::NoIdentity);
    }
    let message = message
        .map(str::trim)
        .filter(|m| !m.is_empty())
        .ok_or_else(|| Error::Unsupported("a commit message is needed".into()))?;
    let saved = SavedIndex::read(path).await?;
    if let Err(err) = stage(path, state).await {
        saved.restore(path).await;
        return Err(err);
    }
    let args = commit_args(message);
    let committed = process::run_with_timeout("git", &args, Some(path), COMMIT_TIMEOUT)
        .await
        .map(drop)
        .map_err(git_failure);
    if committed.is_err() {
        saved.restore(path).await;
    }
    committed
}

/// Stages what the commit holds: every change (`git add --all`), or with editor and OS files
/// to leave out, exactly the listed paths. Those are named in a file rather than on the
/// command line (there may be thousands), and never staged at all, so a `node_modules`
/// isn't hashed into the object store just to be dropped again.
async fn stage(path: &Path, state: &PushCheck) -> Result<()> {
    if state.skipped.is_empty() {
        return git(path, &["add", "--all"]).await.map(drop);
    }
    let list = std::env::temp_dir().join(format!(
        "c50tb-pathspec-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or_default()
    ));
    let mut bytes = Vec::new();
    for file in &state.files {
        bytes.extend_from_slice(file.as_bytes());
        bytes.push(0);
    }
    std::fs::write(&list, bytes)?;
    let from = format!("--pathspec-from-file={}", list.display());
    let staged = git(
        path,
        &[
            "--literal-pathspecs",
            "add",
            "--all",
            from.as_str(),
            "--pathspec-file-nul",
        ],
    )
    .await
    .map(drop);
    let _ = std::fs::remove_file(&list);
    staged
}

/// The clone's index before `git add --all`, put back byte for byte when the commit then
/// fails (a hook, signing, the time limit), so a refused commit doesn't leave everything
/// staged or replace what the student had staged. Only the index file changes, never the
/// working tree.
struct SavedIndex {
    file: PathBuf,
    /// `None`: no index yet (a fresh clone of an empty repo).
    bytes: Option<Vec<u8>>,
    head: Option<String>,
}

impl SavedIndex {
    async fn read(path: &Path) -> Result<Self> {
        let file = PathBuf::from(
            git(path, &["rev-parse", "--git-path", "index"])
                .await?
                .trim(),
        );
        let file = if file.is_absolute() {
            file
        } else {
            path.join(file)
        };
        let bytes = match std::fs::read(&file) {
            Ok(bytes) => Some(bytes),
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => None,
            Err(err) => return Err(err.into()),
        };
        Ok(Self {
            file,
            bytes,
            head: head(path).await,
        })
    }

    /// Puts the index back, unless the commit was made after all (HEAD moved: a hook that
    /// timed out after it) or a git process holds the index's lock.
    async fn restore(self, path: &Path) {
        if head(path).await != self.head {
            return;
        }
        if let Err(err) = self.write() {
            log::warn!("couldn't restore the index after a failed commit: {err}");
        }
    }

    /// git's own protocol: create `index.lock` exclusively, fill it, rename it over `index`.
    fn write(&self) -> std::io::Result<()> {
        let mut lock_name = self.file.clone().into_os_string();
        lock_name.push(".lock");
        let lock = PathBuf::from(lock_name);
        let mut out = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&lock)?;
        let written = match &self.bytes {
            Some(bytes) => std::io::Write::write_all(&mut out, bytes)
                .and_then(|()| out.sync_all())
                .and_then(|()| {
                    drop(out);
                    std::fs::rename(&lock, &self.file)
                }),
            None => {
                drop(out);
                std::fs::remove_file(&self.file)
                    .or_else(|err| match err.kind() {
                        std::io::ErrorKind::NotFound => Ok(()),
                        _ => Err(err),
                    })
                    .and_then(|()| std::fs::remove_file(&lock))
            }
        };
        if written.is_err() {
            let _ = std::fs::remove_file(&lock);
        }
        written
    }
}

/// The commit HEAD points to, `None` before the first commit.
async fn head(path: &Path) -> Option<String> {
    git(path, &["rev-parse", "--quiet", "--verify", "HEAD"])
        .await
        .ok()
        .map(|out| out.trim().to_owned())
}

/// Commits every change with `message` (when there are changes) and pushes `branch` to
/// `origin`, after the same checks as `check`. Returns the pushed commit.
pub async fn commit_and_push(
    path: &Path,
    branch: &str,
    github_head: Option<&str>,
    message: Option<&str>,
) -> Result<Pushed> {
    let state = check(path, branch, github_head).await?;
    let push = push_args(branch);
    // Dev builds refuse the push: stop before the commit, so nothing is left behind.
    process::ensure_allowed("git", &push)?;
    if !state.files.is_empty() {
        commit_changes(path, &state, message).await?;
    }
    match process::run_with_timeout("git", &push, Some(path), PUSH_TIMEOUT).await {
        Ok(_) => {}
        Err(err) => return Err(push_failure(err)),
    }
    let commit = git(path, &["rev-parse", "HEAD"]).await?.trim().to_owned();
    Ok(Pushed { commit })
}

/// Writes `user.name` and `user.email` to the clone's own config (not the global one).
pub async fn set_identity(path: &Path, name: &str, email: &str) -> Result<()> {
    let (name, email) = (name.trim(), email.trim());
    let valid = |s: &str| !s.is_empty() && !s.contains(['\n', '\r', '<', '>']);
    if !valid(name) || !valid(email) || !email.contains('@') {
        return Err(Error::Unsupported(format!(
            "\"{name} <{email}>\" isn't a valid identity"
        )));
    }
    git(path, &["config", "--local", "user.name", name]).await?;
    git(path, &["config", "--local", "user.email", email]).await?;
    Ok(())
}

/// The account's private GitHub address, which GitHub links to the account without showing
/// the real one.
pub fn noreply_email(id: u64, login: &str) -> String {
    format!("{id}+{login}@users.noreply.github.com")
}

/// Who to commit as when git doesn't know: the signed-in GitHub account, with its private
/// address.
#[derive(Debug, Clone, Serialize)]
#[cfg_attr(test, derive(specta::Type))]
#[serde(rename_all = "camelCase")]
pub struct Identity {
    pub name: String,
    pub email: String,
}

/// Reads the signed-in account (`gh api user`, read-only) for `Identity`.
pub async fn suggested_identity() -> Result<Identity> {
    #[derive(serde::Deserialize)]
    struct User {
        login: String,
        id: u64,
        name: Option<String>,
    }
    let out = process::run("gh", ["api", "user"], None).await?;
    let user: User = serde_json::from_str(&out.stdout)
        .map_err(|err| Error::GitHub(format!("unexpected user response: {err}")))?;
    Ok(Identity {
        name: user
            .name
            .filter(|n| !n.trim().is_empty())
            .unwrap_or_else(|| user.login.clone()),
        email: noreply_email(user.id, &user.login),
    })
}

/// `--message=` keeps a message that starts with `-` (or is the word "push", which the dev
/// guard would take for a push) from being read as anything else.
fn commit_args(message: &str) -> Vec<OsString> {
    vec![
        "commit".into(),
        "--quiet".into(),
        format!("--message={message}").into(),
    ]
}

/// gh's credentials (nothing saved), then the branch pushed to the same name on `origin`.
pub(crate) fn push_args(branch: &str) -> Vec<OsString> {
    let refspec = format!("refs/heads/{branch}:refs/heads/{branch}");
    GH_CREDENTIALS
        .iter()
        .copied()
        .map(OsString::from)
        .chain(["push".into(), "origin".into(), refspec.into()])
        .collect()
}

async fn git(path: &Path, args: &[&str]) -> Result<String> {
    Ok(process::run("git", args, Some(path)).await?.stdout)
}

async fn has_config(path: &Path, key: &str) -> bool {
    git(path, &["config", "--get", key])
        .await
        .is_ok_and(|v| !v.trim().is_empty())
}

/// A merge, rebase, cherry-pick or revert waiting for the student to finish it.
async fn in_progress(path: &Path) -> Result<bool> {
    let dir = PathBuf::from(git(path, &["rev-parse", "--git-dir"]).await?.trim());
    let dir = if dir.is_absolute() {
        dir
    } else {
        path.join(dir)
    };
    Ok([
        "MERGE_HEAD",
        "CHERRY_PICK_HEAD",
        "REVERT_HEAD",
        "rebase-merge",
        "rebase-apply",
    ]
    .iter()
    .any(|name| dir.join(name).exists()))
}

/// Commits on HEAD that GitHub's head doesn't have (all of them when GitHub has none).
async fn ahead(path: &Path, github_head: Option<&str>) -> Result<u32> {
    let range = github_head.map(|head| format!("{head}..HEAD"));
    let range = range.as_deref().unwrap_or("HEAD");
    match git(path, &["rev-list", "--count", range]).await {
        Ok(out) => Ok(out.trim().parse().unwrap_or(0)),
        // No commit yet (a fresh clone of an empty repo).
        Err(Error::CommandFailed { .. }) if github_head.is_none() => Ok(0),
        Err(err) => Err(err),
    }
}

/// `git status --porcelain=v1 -z`: every path, the untracked ones, and the untracked noise
/// left out of both. A rename's second entry (its old path) is skipped.
fn parse_status(out: &str) -> WorkTree {
    let mut tree = WorkTree::default();
    let mut noise = Vec::new();
    let mut entries = out.split('\0').filter(|e| !e.is_empty());
    while let Some(entry) = entries.next() {
        let (Some(code), Some(file)) = (entry.get(..2), entry.get(3..)) else {
            continue;
        };
        if code.contains(['R', 'C']) {
            entries.next();
        }
        if code == "??" {
            if noise::is_noise(file) {
                noise.push(file);
                continue;
            }
            tree.untracked.push(file.to_owned());
        }
        tree.files.push(file.to_owned());
    }
    // A folder that also holds a file going into the commit (a tracked `.vscode/settings.json`)
    // is named file by file, so "left out" never covers what goes in.
    let mut skipped: Vec<String> = Vec::new();
    for file in noise {
        let root = noise::root(file).unwrap_or(file);
        let shared = root.ends_with('/') && tree.files.iter().any(|f| f.starts_with(root));
        let shown = if shared { file } else { root };
        if !skipped.iter().any(|s| s == shown) {
            skipped.push(shown.to_owned());
        }
    }
    tree.skipped = skipped;
    tree
}

/// The files over GitHub's warning size, and those over its limit.
fn sizes(path: &Path, files: &[String]) -> (Vec<String>, Vec<String>) {
    let mut large = Vec::new();
    let mut too_large = Vec::new();
    for file in files {
        let Ok(meta) = std::fs::metadata(path.join(file)) else {
            continue;
        };
        if meta.len() > MAX_FILE {
            too_large.push(file.clone());
        } else if meta.len() > LARGE_FILE {
            large.push(file.clone());
        }
    }
    (large, too_large)
}

/// A hook or signing failure keeps git's last line.
fn git_failure(err: Error) -> Error {
    match err {
        Error::CommandFailed { stderr, .. } => Error::Unsupported(last_line(&stderr)),
        err => err,
    }
}

/// GitHub moved on since the check: a typed refusal. Anything else keeps git's last line.
fn push_failure(err: Error) -> Error {
    let Error::CommandFailed { stderr, .. } = err else {
        return err;
    };
    if stderr.contains("[rejected]")
        || stderr.contains("non-fast-forward")
        || stderr.contains("fetch first")
    {
        return Error::PushRejected;
    }
    Error::Unsupported(last_line(&stderr))
}

fn last_line(stderr: &str) -> String {
    let line = stderr
        .lines()
        .map(str::trim)
        .rfind(|l| !l.is_empty() && !l.starts_with("hint:"))
        .unwrap_or_default();
    line.strip_prefix("fatal: ")
        .or_else(|| line.strip_prefix("error: "))
        .unwrap_or(line)
        .to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn failed(stderr: &str) -> Error {
        Error::CommandFailed {
            program: "git".into(),
            code: Some(1),
            stderr: stderr.into(),
        }
    }

    #[test]
    fn builds_args() {
        let push: Vec<String> = push_args("main")
            .iter()
            .map(|a| a.to_string_lossy().into_owned())
            .collect();
        assert_eq!(
            push,
            [
                "-c",
                "credential.https://github.com.helper=",
                "-c",
                "credential.https://github.com.helper=!gh auth git-credential",
                "push",
                "origin",
                "refs/heads/main:refs/heads/main",
            ]
        );
        assert_eq!(
            commit_args("-v push")[2],
            OsString::from("--message=-v push")
        );
        // The guard reads the message as a flag, not as the word "push".
        assert!(process::ensure_allowed("git", commit_args("push")).is_ok());
        assert_eq!(noreply_email(42, "ada"), "42+ada@users.noreply.github.com");
    }

    #[test]
    fn dev_builds_refuse_the_push() {
        assert_eq!(
            cfg!(debug_assertions),
            matches!(
                process::ensure_allowed("git", push_args("main")),
                Err(Error::BlockedInDev(_))
            )
        );
    }

    #[test]
    fn parses_status() {
        let out = " M a.txt\0R  new.txt\0old.txt\0?? dir/b.txt\0 D gone.txt\0\
            ?? .idea/workspace.xml\0?? .idea/misc.xml\0?? src/.DS_Store\0 M .vscode/settings.json\0";
        let tree = parse_status(out);
        assert_eq!(
            tree.files,
            [
                "a.txt",
                "new.txt",
                "dir/b.txt",
                "gone.txt",
                ".vscode/settings.json"
            ]
        );
        assert_eq!(tree.untracked, ["dir/b.txt"]);
        assert_eq!(tree.skipped, [".idea/", "src/.DS_Store"]);
    }

    #[test]
    fn explains_failed_pushes() {
        let stderr = "To https://github.com/o/r\n ! [rejected]        main -> main (fetch first)\n\
            error: failed to push some refs to 'https://github.com/o/r'\nhint: Updates were rejected\n";
        assert!(matches!(push_failure(failed(stderr)), Error::PushRejected));
        let stderr = "remote: Permission to o/r.git denied to ada.\n\
            fatal: unable to access 'https://github.com/o/r/': The requested URL returned error: 403\n";
        assert!(matches!(
            push_failure(failed(stderr)),
            Error::Unsupported(m) if m == "unable to access 'https://github.com/o/r/': The requested URL returned error: 403"
        ));
        assert!(matches!(push_failure(Error::Offline), Error::Offline));
    }

    fn sh(dir: &Path, args: &[&str]) -> String {
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
        String::from_utf8_lossy(&out.stdout).trim().to_owned()
    }

    /// A clone of a local bare "origin": every refusal, the commit, and the push itself.
    /// The app's push is refused in dev builds (and tests are one), so the test checks that
    /// refusal leaves no commit, then pushes with the same arguments outside the guard,
    /// against the local origin only.
    #[tokio::test]
    async fn commits_and_pushes_to_a_local_origin() {
        let root = std::env::temp_dir().join(format!("c50tb-push-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let seed = root.join("seed");
        let origin = root.join("origin.git");
        let clone = root.join("clone");
        std::fs::create_dir_all(&seed).unwrap();
        sh(&root, &["init", "-q", "--bare", "-b", "main", "origin.git"]);
        sh(&seed, &["init", "-q", "-b", "main"]);
        sh(&seed, &["commit", "-q", "--allow-empty", "-m", "one"]);
        sh(&seed, &["push", "-q", origin.to_str().unwrap(), "main"]);
        sh(&root, &["clone", "-q", "origin.git", "clone"]);
        let head = sh(&clone, &["rev-parse", "HEAD"]);

        assert!(matches!(
            check(&clone, "main", Some(&head)).await,
            Err(Error::NothingToPush)
        ));
        std::fs::write(clone.join("a.txt"), "x").unwrap();
        std::fs::write(clone.join(".gitignore"), "*.log\n").unwrap();
        std::fs::write(clone.join("debug.log"), "x").unwrap();
        let state = check(&clone, "main", Some(&head)).await.unwrap();
        assert_eq!(state.untracked, [".gitignore", "a.txt"]);
        assert_eq!(state.ahead, 0);

        sh(&clone, &["switch", "-q", "-c", "work"]);
        assert!(matches!(
            check(&clone, "main", Some(&head)).await,
            Err(Error::OtherBranch(b)) if b == "main"
        ));
        sh(&clone, &["switch", "-q", "main"]);

        // GitHub moved on: refused, whether or not the commit was fetched.
        sh(&seed, &["commit", "-q", "--allow-empty", "-m", "two"]);
        let newer = sh(&seed, &["rev-parse", "HEAD"]);
        assert!(matches!(
            check(&clone, "main", Some(&newer)).await,
            Err(Error::RemoteAhead)
        ));
        sh(&seed, &["push", "-q", origin.to_str().unwrap(), "main"]);
        sh(&clone, &["fetch", "-q"]);
        assert!(matches!(
            check(&clone, "main", Some(&newer)).await,
            Err(Error::RemoteAhead)
        ));
        sh(&clone, &["merge", "-q", "--ff-only", "origin/main"]);
        let head = newer;

        std::fs::write(clone.join(".git").join("MERGE_HEAD"), &head).unwrap();
        assert!(matches!(
            check(&clone, "main", Some(&head)).await,
            Err(Error::GitInProgress)
        ));
        std::fs::remove_file(clone.join(".git").join("MERGE_HEAD")).unwrap();

        // The guard fires before anything is written.
        if cfg!(debug_assertions) {
            let result = commit_and_push(&clone, "main", Some(&head), Some("Add a")).await;
            assert!(matches!(result, Err(Error::BlockedInDev(_))));
            assert_eq!(sh(&clone, &["rev-parse", "HEAD"]), head);
        }

        // Written to the clone's own config; the commit below runs with it and no `-c`.
        set_identity(&clone, "Ada", &noreply_email(1, "ada"))
            .await
            .unwrap();
        assert_eq!(
            sh(&clone, &["config", "--local", "user.email"]),
            "1+ada@users.noreply.github.com"
        );
        assert!(
            check(&clone, "main", Some(&head))
                .await
                .unwrap()
                .has_identity
        );

        // The commit step as the app runs it, then the push with the app's arguments.
        git(&clone, &["add", "--all"]).await.unwrap();
        let commit = [OsString::from("-c"), "commit.gpgsign=false".into()]
            .into_iter()
            .chain(commit_args("Add a"));
        process::run("git", commit, Some(&clone)).await.unwrap();
        assert_eq!(sh(&clone, &["log", "-1", "--format=%s"]), "Add a");
        let state = check(&clone, "main", Some(&head)).await.unwrap();
        assert!(state.files.is_empty());
        assert_eq!(state.ahead, 1);
        sh(
            &clone,
            &push_args("main")
                .iter()
                .map(|a| a.to_str().unwrap())
                .collect::<Vec<_>>(),
        );
        let pushed = sh(&clone, &["rev-parse", "HEAD"]);
        assert_eq!(sh(&origin, &["rev-parse", "main"]), pushed);
        assert!(matches!(
            check(&clone, "main", Some(&pushed)).await,
            Err(Error::NothingToPush)
        ));
        let _ = std::fs::remove_dir_all(&root);
    }

    /// Commit only: on another branch and with GitHub ahead (both refused by Commit & push),
    /// in a dev build too, since nothing is pushed.
    #[tokio::test]
    async fn commits_locally_on_any_branch() {
        let root = std::env::temp_dir().join(format!("c50tb-commit-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let clone = root.join("clone");
        std::fs::create_dir_all(&clone).unwrap();
        sh(&clone, &["init", "-q", "-b", "main"]);
        sh(&clone, &["commit", "-q", "--allow-empty", "-m", "one"]);
        sh(&clone, &["config", "--local", "commit.gpgsign", "false"]);
        set_identity(&clone, "Ada", &noreply_email(1, "ada"))
            .await
            .unwrap();
        assert!(matches!(
            commit_check(&clone).await,
            Err(Error::NothingToCommit)
        ));
        std::fs::write(clone.join("a.txt"), "x").unwrap();
        // A commit GitHub has and the clone lacks: Commit & push refuses, Commit doesn't.
        let github_head = "1111111111111111111111111111111111111111";
        assert!(matches!(
            check(&clone, "main", Some(github_head)).await,
            Err(Error::RemoteAhead)
        ));
        assert!(commit_check(&clone).await.is_ok());

        sh(&clone, &["switch", "-q", "-c", "work"]);
        assert!(matches!(
            check(&clone, "main", Some(github_head)).await,
            Err(Error::OtherBranch(_))
        ));
        let state = commit_check(&clone).await.unwrap();
        assert_eq!(state.untracked, ["a.txt"]);
        assert_eq!(state.ahead, 0);

        std::fs::write(clone.join(".git").join("MERGE_HEAD"), "x").unwrap();
        assert!(matches!(
            commit(&clone, "Add a").await,
            Err(Error::GitInProgress)
        ));
        std::fs::remove_file(clone.join(".git").join("MERGE_HEAD")).unwrap();

        let big = std::fs::File::create(clone.join("big.bin")).unwrap();
        big.set_len(MAX_FILE + 1).unwrap();
        drop(big);
        let before = sh(&clone, &["rev-parse", "HEAD"]);
        assert!(matches!(
            commit(&clone, "Add a").await,
            Err(Error::FileTooLarge(f)) if f == ["big.bin"]
        ));
        assert_eq!(sh(&clone, &["rev-parse", "HEAD"]), before);
        std::fs::remove_file(clone.join("big.bin")).unwrap();

        // The dev guard only stops pushes: this commit goes through in a debug build.
        let committed = commit(&clone, " Add a ").await.unwrap();
        assert_eq!(sh(&clone, &["rev-parse", "HEAD"]), committed.commit);
        assert_eq!(sh(&clone, &["log", "-1", "--format=%s"]), "Add a");
        assert_eq!(sh(&clone, &["branch", "--show-current"]), "work");
        assert!(matches!(
            commit_check(&clone).await,
            Err(Error::NothingToCommit)
        ));
        let _ = std::fs::remove_dir_all(&root);
    }

    /// Untracked editor and OS files stay out of the commit and stay untracked; a tracked
    /// file in an editor folder is real work and goes in. Nothing but noise: nothing to
    /// commit.
    #[tokio::test]
    async fn commits_leave_editor_files_out() {
        let root = std::env::temp_dir().join(format!("c50tb-noise-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let clone = root.join("clone");
        std::fs::create_dir_all(clone.join(".vscode")).unwrap();
        sh(&clone, &["init", "-q", "-b", "main"]);
        sh(&clone, &["config", "--local", "commit.gpgsign", "false"]);
        std::fs::write(clone.join(".vscode").join("settings.json"), "{}").unwrap();
        std::fs::write(clone.join("old name.c"), "o").unwrap();
        sh(&clone, &["add", "--all"]);
        sh(&clone, &["commit", "-q", "-m", "one"]);
        set_identity(&clone, "Ada", &noreply_email(1, "ada"))
            .await
            .unwrap();

        std::fs::create_dir_all(clone.join(".idea")).unwrap();
        std::fs::write(clone.join(".idea").join("workspace.xml"), "x").unwrap();
        std::fs::write(clone.join(".DS_Store"), "x").unwrap();
        std::fs::write(clone.join(".vscode").join("launch.json"), "{}").unwrap();
        assert!(matches!(
            commit_check(&clone).await,
            Err(Error::NothingToCommit)
        ));

        std::fs::write(clone.join(".vscode").join("settings.json"), "{\"a\":1}").unwrap();
        std::fs::write(clone.join("hello world.c"), "int main;").unwrap();
        // Glob characters in a name are taken literally (`*` and `?` aren't allowed in Windows
        // file names, `[` is).
        std::fs::write(clone.join("[x].c"), "glob").unwrap();
        sh(&clone, &["mv", "old name.c", "new name.c"]);
        let state = commit_check(&clone).await.unwrap();
        assert_eq!(
            state.files,
            [
                ".vscode/settings.json",
                "new name.c",
                "[x].c",
                "hello world.c"
            ]
        );
        assert_eq!(
            state.skipped,
            [".DS_Store", ".idea/", ".vscode/launch.json"]
        );

        commit(&clone, "Work").await.unwrap();
        let committed = sh(&clone, &["show", "--name-status", "--format=", "HEAD"]);
        assert_eq!(
            committed,
            "M\t.vscode/settings.json\nA\t[x].c\nA\thello world.c\nR100\told name.c\tnew name.c"
        );
        assert_eq!(
            sh(&clone, &["status", "--porcelain", "--untracked-files=all"]),
            "?? .DS_Store\n?? .idea/workspace.xml\n?? .vscode/launch.json"
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    /// A commit refused by a hook puts the index back as it was: the student's partial
    /// staging kept, nothing else left staged, the working tree untouched. Same for a fresh
    /// repo that had no index at all.
    #[cfg(unix)]
    #[tokio::test]
    async fn a_failed_commit_restores_the_index() {
        use std::os::unix::fs::PermissionsExt;
        let root = std::env::temp_dir().join(format!("c50tb-hook-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let hooks = root.join("hooks");
        std::fs::create_dir_all(&hooks).unwrap();
        let hook = hooks.join("pre-commit");
        std::fs::write(&hook, "#!/bin/sh\necho refused >&2\nexit 1\n").unwrap();
        std::fs::set_permissions(&hook, std::fs::Permissions::from_mode(0o755)).unwrap();
        let setup = |clone: &Path| {
            std::fs::create_dir_all(clone).unwrap();
            sh(clone, &["init", "-q", "-b", "main"]);
            sh(clone, &["config", "--local", "commit.gpgsign", "false"]);
            sh(clone, &["config", "--local", "user.name", "Ada"]);
            sh(clone, &["config", "--local", "user.email", "a@a"]);
        };
        let index = |clone: &Path| std::fs::read(clone.join(".git").join("index")).ok();

        let clone = root.join("clone");
        setup(&clone);
        std::fs::write(clone.join("tracked.txt"), "1").unwrap();
        sh(&clone, &["add", "tracked.txt"]);
        sh(&clone, &["commit", "-q", "-m", "one"]);
        sh(
            &clone,
            &[
                "config",
                "--local",
                "core.hooksPath",
                hooks.to_str().unwrap(),
            ],
        );
        std::fs::write(clone.join("tracked.txt"), "2").unwrap();
        std::fs::write(clone.join("staged.txt"), "s").unwrap();
        std::fs::write(clone.join("new.txt"), "n").unwrap();
        sh(&clone, &["add", "staged.txt"]);
        let head = sh(&clone, &["rev-parse", "HEAD"]);
        let (before, status) = (index(&clone), sh(&clone, &["status", "--porcelain"]));
        let result = commit(&clone, "Add").await;
        assert!(result.is_err(), "{result:?}");
        assert_eq!(sh(&clone, &["rev-parse", "HEAD"]), head);
        assert_eq!(index(&clone), before);
        assert_eq!(sh(&clone, &["status", "--porcelain"]), status);
        assert_eq!(
            sh(&clone, &["diff", "--cached", "--name-only"]),
            "staged.txt"
        );
        assert_eq!(
            std::fs::read_to_string(clone.join("tracked.txt")).unwrap(),
            "2"
        );
        assert!(!clone.join(".git").join("index.lock").exists());

        // No commit and no index yet: none afterwards either.
        let fresh = root.join("fresh");
        setup(&fresh);
        sh(
            &fresh,
            &[
                "config",
                "--local",
                "core.hooksPath",
                hooks.to_str().unwrap(),
            ],
        );
        std::fs::write(fresh.join("a.txt"), "a").unwrap();
        assert_eq!(index(&fresh), None);
        assert!(commit(&fresh, "Add").await.is_err());
        assert_eq!(index(&fresh), None);
        assert_eq!(sh(&fresh, &["status", "--porcelain"]), "?? a.txt");
        let _ = std::fs::remove_dir_all(&root);
    }
}
