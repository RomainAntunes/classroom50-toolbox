//! Clones made by the app (discovery source 3). Each one is registered when it's created,
//! so it's found even outside the watched folders and the editors' projects. The list is
//! kept in `clones.json` in the app config dir, next to the settings, because clearing the
//! cache must not forget it.

use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::Duration;

use crate::error::{Error, Result};
use crate::process;
use crate::pull::GH_CREDENTIALS;
use crate::ssh::{self, Who};
use crate::tools::GitProtocol;

/// A student repo is small; a slow link still gets its time, a hung ssh doesn't.
const CLONE_TIMEOUT: Duration = Duration::from_secs(10 * 60);

const FILE_NAME: &str = "clones.json";

pub struct CloneStore {
    path: PathBuf,
    clones: Mutex<Vec<PathBuf>>,
}

impl CloneStore {
    pub fn load(dir: &Path) -> Self {
        let path = dir.join(FILE_NAME);
        let clones = std::fs::read_to_string(&path)
            .ok()
            .and_then(|json| serde_json::from_str(&json).ok())
            .unwrap_or_default();
        Self {
            path,
            clones: Mutex::new(clones),
        }
    }

    /// The registered clones that still exist on disk.
    pub fn list(&self) -> Vec<PathBuf> {
        self.clones
            .lock()
            .map(|c| c.iter().filter(|p| p.is_dir()).cloned().collect())
            .unwrap_or_default()
    }

    /// Registers `path`, dropping clones deleted since.
    pub fn add(&self, path: PathBuf) -> Result<()> {
        let mut clones = self.list();
        if !clones.contains(&path) {
            clones.push(path);
        }
        let json = serde_json::to_vec_pretty(&clones).map_err(std::io::Error::other)?;
        crate::github::write_atomic(&self.path, &json)?;
        if let Ok(mut current) = self.clones.lock() {
            *current = clones;
        }
        Ok(())
    }
}

/// Clones `owner/repo` into `parent/<repo>` over `protocol`, with git and gh's credentials
/// for HTTPS passed for this one command (`pull::GH_CREDENTIALS`), so a private repo needs
/// no git setup and nothing is saved in the clone. An SSH failure is `Error::Ssh`
/// (`ssh::failure`, with `who`); a clone stopped by the time limit leaves no half-made
/// folder. Returns the new folder.
pub async fn clone(
    owner: &str,
    repo: &str,
    parent: &Path,
    protocol: GitProtocol,
    who: &Who<'_>,
) -> Result<PathBuf> {
    if !is_plain_name(owner) || !is_plain_name(repo) {
        return Err(Error::Unsupported(format!(
            "unexpected repository name {owner}/{repo}"
        )));
    }
    let target = parent.join(repo);
    if target.exists() {
        return Err(Error::PathExists(target));
    }
    std::fs::create_dir_all(parent)?;
    let url = remote_url(owner, repo, protocol);
    match process::run_with_timeout("git", clone_args(&url, &target), None, CLONE_TIMEOUT).await {
        Ok(_) => Ok(target),
        Err(err) => {
            // git removes what it made when it fails, not when it's killed.
            if matches!(err, Error::TimedOut { .. }) {
                let _ = std::fs::remove_dir_all(&target);
            }
            Err(match err {
                Error::CommandFailed { stderr, .. } => match ssh::failure(&stderr, who).await {
                    Some(err) => err,
                    None => Error::Unsupported(
                        ssh::github_error(&stderr).unwrap_or_else(|| last_line(&stderr)),
                    ),
                },
                err => err,
            })
        }
    }
}

/// Sets the clone's `origin` to `owner/repo` over `protocol`, and each of its push URLs
/// that is that repo too, so a pull and a push both switch; a push URL elsewhere (a backup,
/// a mirror) is kept. Refused unless `origin` is that repo already (in any spelling: case,
/// `.git`, a user name), so another remote is never rewritten.
pub async fn set_remote(
    clone: &Path,
    owner: &str,
    repo: &str,
    protocol: GitProtocol,
) -> Result<()> {
    let is_repo = |url: &str| {
        crate::discovery::parse_github_remote(url)
            .is_some_and(|(o, r)| o.eq_ignore_ascii_case(owner) && r.eq_ignore_ascii_case(repo))
    };
    let same = crate::discovery::origin_url(clone).is_some_and(|url| is_repo(&url));
    if !same || !is_plain_name(owner) || !is_plain_name(repo) {
        return Err(Error::Unsupported(format!(
            "the clone's origin isn't {owner}/{repo}"
        )));
    }
    let url = remote_url(owner, repo, protocol);
    set_url(clone, &["remote", "set-url", "origin", &url]).await?;
    // None set: exit 1 and nothing printed.
    let push_urls = process::run(
        "git",
        ["config", "--get-all", "remote.origin.pushurl"],
        Some(clone),
    )
    .await
    .map(|out| out.stdout)
    .unwrap_or_default();
    for old in push_urls.lines().map(str::trim).filter(|u| !u.is_empty()) {
        if is_repo(old) {
            let matching = format!("^{}$", regex_escape(old));
            set_url(
                clone,
                &["remote", "set-url", "--push", "origin", &url, &matching],
            )
            .await?;
        }
    }
    Ok(())
}

/// `text` as a POSIX extended regex matching only itself (git reads `set-url`'s old URL as
/// one).
fn regex_escape(text: &str) -> String {
    let mut escaped = String::with_capacity(text.len());
    for c in text.chars() {
        if r"\.^$|?*+()[]{}".contains(c) {
            escaped.push('\\');
        }
        escaped.push(c);
    }
    escaped
}

async fn set_url(clone: &Path, args: &[&str]) -> Result<()> {
    match process::run("git", args, Some(clone)).await {
        Ok(_) => Ok(()),
        Err(Error::CommandFailed { stderr, .. }) => Err(Error::Unsupported(last_line(&stderr))),
        Err(err) => Err(err),
    }
}

/// The URL git clones `owner/repo` from: `https://github.com/o/r.git` or
/// `git@github.com:o/r.git`, as `gh repo clone` and GitHub's Code button write them.
pub fn remote_url(owner: &str, repo: &str, protocol: GitProtocol) -> String {
    match protocol {
        GitProtocol::Https => format!("https://github.com/{owner}/{repo}.git"),
        GitProtocol::Ssh => format!("git@github.com:{owner}/{repo}.git"),
    }
}

/// `git -c ... clone -- <url> <target>`: the credentials before `clone`, so they're for this
/// command only (`clone -c` would write them into the clone's config).
fn clone_args(url: &str, target: &Path) -> Vec<OsString> {
    GH_CREDENTIALS
        .iter()
        .map(OsString::from)
        .chain(["clone".into(), "--".into(), url.into()])
        .chain([target.as_os_str().to_owned()])
        .collect()
}

/// git's error, without its `fatal: ` prefix.
fn last_line(stderr: &str) -> String {
    let line = stderr
        .lines()
        .map(str::trim)
        .rfind(|l| !l.is_empty() && !l.starts_with("hint:"))
        .unwrap_or_default();
    line.strip_prefix("fatal: ").unwrap_or(line).to_owned()
}

/// A GitHub owner or repo name: it becomes a folder name, so nothing that could leave
/// `parent` or start an option.
fn is_plain_name(name: &str) -> bool {
    !name.is_empty()
        && name != "."
        && name != ".."
        && !name.starts_with('-')
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_only_plain_names() {
        assert!(is_plain_name("heigvd-poo-lab1-romain"));
        assert!(is_plain_name("my.repo_2"));
        assert!(!is_plain_name(".."));
        assert!(!is_plain_name("a/b"));
        assert!(!is_plain_name("-x"));
        assert!(!is_plain_name(""));
    }

    #[test]
    fn remembers_clones_that_exist() {
        let dir = std::env::temp_dir().join(format!("c50tb-clones-{}", std::process::id()));
        let clone = dir.join("repo");
        std::fs::create_dir_all(&clone).unwrap();
        let store = CloneStore::load(&dir);
        store.add(clone.clone()).unwrap();
        store.add(clone.clone()).unwrap();
        store.add(dir.join("gone")).unwrap();
        let reloaded = CloneStore::load(&dir);
        assert_eq!(reloaded.list(), vec![clone]);
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[tokio::test]
    async fn refuses_an_existing_folder() {
        let dir = std::env::temp_dir().join(format!("c50tb-exists-{}", std::process::id()));
        std::fs::create_dir_all(dir.join("repo")).unwrap();
        let who = Who::Known {
            active: None,
            probe: ssh::SshStatus::NotInstalled,
        };
        let result = clone("owner", "repo", &dir, GitProtocol::Https, &who).await;
        assert!(matches!(result, Err(Error::PathExists(_))));
        let result = clone("owner", "..", &dir, GitProtocol::Ssh, &who).await;
        assert!(matches!(result, Err(Error::Unsupported(_))));
        std::fs::remove_dir_all(&dir).unwrap();
    }

    /// `origin` (and its push URL) switch between HTTPS and SSH; another repo's isn't touched.
    #[tokio::test]
    async fn switches_the_origin_of_the_projects_repo_only() {
        let root = std::env::temp_dir().join(format!("c50tb-remote-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        let git = |args: &[&str]| {
            let out = std::process::Command::new("git")
                .args(args)
                .current_dir(&root)
                .output()
                .unwrap();
            assert!(out.status.success(), "{args:?}");
            String::from_utf8_lossy(&out.stdout).trim().to_owned()
        };
        git(&["init", "-q"]);
        git(&[
            "remote",
            "add",
            "origin",
            "https://github.com/Acme-CS/lab-2-alex",
        ]);
        set_remote(&root, "acme-cs", "lab-2-alex", GitProtocol::Ssh)
            .await
            .unwrap();
        assert_eq!(
            git(&["remote", "get-url", "origin"]),
            "git@github.com:acme-cs/lab-2-alex.git"
        );
        // No push URL was set, so none is added.
        assert!(
            std::process::Command::new("git")
                .args(["config", "--get", "remote.origin.pushurl"])
                .current_dir(&root)
                .output()
                .unwrap()
                .stdout
                .is_empty()
        );
        git(&[
            "remote",
            "set-url",
            "--push",
            "origin",
            "git@github.com:acme-cs/lab-2-alex.git",
        ]);
        set_remote(&root, "acme-cs", "lab-2-alex", GitProtocol::Https)
            .await
            .unwrap();
        assert_eq!(
            git(&["remote", "get-url", "origin"]),
            "https://github.com/acme-cs/lab-2-alex.git"
        );
        assert_eq!(
            git(&["remote", "get-url", "--push", "origin"]),
            "https://github.com/acme-cs/lab-2-alex.git"
        );
        // Several push URLs: the project's are switched, a backup elsewhere is kept.
        git(&[
            "remote",
            "set-url",
            "--add",
            "--push",
            "origin",
            "git@github.com:alice/lab-2-backup.git",
        ]);
        git(&[
            "remote",
            "set-url",
            "--add",
            "--push",
            "origin",
            "https://alice@github.com/Acme-CS/lab-2-alex",
        ]);
        set_remote(&root, "acme-cs", "lab-2-alex", GitProtocol::Ssh)
            .await
            .unwrap();
        assert_eq!(
            git(&["config", "--get-all", "remote.origin.pushurl"]),
            "git@github.com:acme-cs/lab-2-alex.git\n\
             git@github.com:alice/lab-2-backup.git\n\
             git@github.com:acme-cs/lab-2-alex.git"
        );
        assert_eq!(
            git(&["remote", "get-url", "origin"]),
            "git@github.com:acme-cs/lab-2-alex.git"
        );
        git(&["config", "--unset-all", "remote.origin.pushurl"]);
        git(&[
            "remote",
            "set-url",
            "origin",
            "https://github.com/acme-cs/lab-2-alex.git",
        ]);
        assert!(matches!(
            set_remote(&root, "acme-cs", "other", GitProtocol::Ssh).await,
            Err(Error::Unsupported(_))
        ));
        assert_eq!(
            git(&["remote", "get-url", "origin"]),
            "https://github.com/acme-cs/lab-2-alex.git"
        );
        std::fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn escapes_an_old_url_for_git() {
        assert_eq!(
            regex_escape("https://u@github.com/o/r.git"),
            r"https://u@github\.com/o/r\.git"
        );
        assert_eq!(
            regex_escape(r"a+b(c)[d]{e}|^$?*\"),
            r"a\+b\(c\)\[d\]\{e\}\|\^\$\?\*\\"
        );
    }

    #[test]
    fn builds_the_clone_command() {
        assert_eq!(
            remote_url("acme-cs", "lab-2-alex", GitProtocol::Https),
            "https://github.com/acme-cs/lab-2-alex.git"
        );
        assert_eq!(
            remote_url("acme-cs", "lab-2-alex", GitProtocol::Ssh),
            "git@github.com:acme-cs/lab-2-alex.git"
        );
        // What the clone's origin reads back as.
        for protocol in [GitProtocol::Https, GitProtocol::Ssh] {
            let url = remote_url("acme-cs", "lab-2-alex", protocol);
            assert_eq!(crate::discovery::remote_protocol(&url), Some(protocol));
            assert_eq!(
                crate::discovery::parse_github_remote(&url),
                Some(("acme-cs".into(), "lab-2-alex".into()))
            );
        }
        let args = clone_args("git@github.com:o/r.git", Path::new("/w/r"));
        let args: Vec<&str> = args.iter().map(|a| a.to_str().unwrap()).collect();
        assert_eq!(&args[..4], GH_CREDENTIALS);
        assert_eq!(
            &args[4..],
            ["clone", "--", "git@github.com:o/r.git", "/w/r"]
        );
    }

    /// A clone through the app's command line from a local "GitHub": the credentials stay
    /// out of the clone's config.
    #[tokio::test]
    async fn clones_without_saving_credentials() {
        let root = std::env::temp_dir().join(format!("c50tb-clone-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let origin = root.join("origin.git");
        std::fs::create_dir_all(&root).unwrap();
        let git = |dir: &Path, args: &[&str]| {
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
            String::from_utf8_lossy(&out.stdout).into_owned()
        };
        let work = root.join("work");
        std::fs::create_dir_all(&work).unwrap();
        git(&work, &["init", "-q", "-b", "main"]);
        git(&work, &["commit", "-q", "--allow-empty", "-m", "one"]);
        git(&root, &["clone", "-q", "--bare", "work", "origin.git"]);

        let target = root.join("clones").join("lab");
        let url = origin.to_string_lossy();
        process::run_with_timeout("git", clone_args(&url, &target), None, CLONE_TIMEOUT)
            .await
            .unwrap();
        assert_eq!(git(&target, &["log", "--format=%s"]).trim(), "one");
        let config = std::fs::read_to_string(target.join(".git").join("config")).unwrap();
        assert!(!config.contains("credential"), "{config}");
        // A failure keeps git's own words.
        let missing = root.join("missing.git");
        let err = clone_args(&missing.to_string_lossy(), &root.join("x"));
        let err = process::run_with_timeout("git", err, None, CLONE_TIMEOUT)
            .await
            .unwrap_err();
        let Error::CommandFailed { stderr, .. } = err else {
            panic!("{err:?}")
        };
        assert!(!last_line(&stderr).starts_with("fatal:"), "{stderr}");
        std::fs::remove_dir_all(&root).unwrap();
    }
}
