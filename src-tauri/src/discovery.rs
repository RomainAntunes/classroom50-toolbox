//! Finds candidate clones on disk: directories holding a `.classroom50.yaml`, and git
//! repos with a GitHub origin (assignments without an autograder have no marker file).
//! `projects` decides which of them belong to a classroom.

use std::path::{Path, PathBuf};

use serde::Serialize;
use walkdir::WalkDir;

use crate::classroom50::{REPO_CONFIG_FILE, RepoConfig};

/// Deep enough for `~/code/<course>/<assignment>`, shallow enough to stay fast on a home dir.
pub const DEFAULT_MAX_DEPTH: usize = 4;

pub const SKIPPED_DIRS: &[&str] = &["node_modules", "target", "Library", "AppData", "venv"];

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LocalProject {
    pub path: PathBuf,
    pub owner: Option<String>,
    pub repo: Option<String>,
    pub config: Option<RepoConfig>,
}

/// Walks `root` and returns every candidate repo. A repo's own subtree isn't descended
/// into. Blocking: call from `spawn_blocking`.
pub fn scan(root: &Path, max_depth: usize) -> Vec<LocalProject> {
    let mut projects = Vec::new();
    let mut walker = WalkDir::new(root)
        .max_depth(max_depth)
        .follow_links(false)
        .into_iter()
        .filter_entry(|entry| entry.depth() == 0 || !is_skipped(entry.file_name()));

    while let Some(entry) = walker.next() {
        let Ok(entry) = entry else { continue };
        let dir = entry.path();
        let has_config = dir.join(REPO_CONFIG_FILE).is_file();
        if !entry.file_type().is_dir() || !(has_config || dir.join(".git").exists()) {
            continue;
        }
        walker.skip_current_dir();
        let config = RepoConfig::read(dir).ok().flatten();
        let (owner, repo) = origin_of(dir).unzip();
        if config.is_some() || owner.is_some() {
            projects.push(LocalProject {
                path: dir.to_path_buf(),
                owner,
                repo,
                config,
            });
        }
    }
    projects
}

fn is_skipped(name: &std::ffi::OsStr) -> bool {
    let name = name.to_string_lossy();
    name.starts_with('.') || SKIPPED_DIRS.contains(&name.as_ref())
}

/// `owner/repo` of the `origin` remote, read straight from the repo's git config (no git
/// process).
fn origin_of(repo_dir: &Path) -> Option<(String, String)> {
    let config = std::fs::read_to_string(git_dir(repo_dir)?.join("config")).ok()?;
    let mut in_origin = false;
    for line in config.lines().map(str::trim) {
        if line.starts_with('[') {
            in_origin = line == r#"[remote "origin"]"#;
        } else if in_origin
            && let Some((name, value)) = line.split_once('=')
            && name.trim().eq_ignore_ascii_case("url")
        {
            return parse_github_remote(value.trim().trim_matches('"'));
        }
    }
    None
}

/// The directory holding the repo's `config`: `.git` itself, or for a linked worktree or a
/// submodule (where `.git` is a `gitdir: <path>` file) the directory it points to, and
/// that one's common dir when it has one.
fn git_dir(repo_dir: &Path) -> Option<PathBuf> {
    let dot_git = repo_dir.join(".git");
    if dot_git.is_dir() {
        return Some(dot_git);
    }
    let link = std::fs::read_to_string(&dot_git).ok()?;
    let target = repo_dir.join(link.trim().strip_prefix("gitdir:")?.trim());
    match std::fs::read_to_string(target.join("commondir")) {
        Ok(common) => Some(target.join(common.trim())),
        Err(_) => Some(target),
    }
}

/// `owner/repo` of a GitHub remote URL: `git@github.com:o/r.git`, `https://github.com/o/r`,
/// `ssh://git@github.com:22/o/r.git`, with or without a user name (`https://me@github.com/`,
/// which some editors and credential helpers write), `www.` or a trailing slash. The host is
/// matched without regard to case.
pub fn parse_github_remote(url: &str) -> Option<(String, String)> {
    let path = match url.split_once("://") {
        Some((scheme, rest)) => {
            if !["https", "http", "ssh", "git"]
                .iter()
                .any(|s| scheme.eq_ignore_ascii_case(s))
            {
                return None;
            }
            let (authority, path) = rest.split_once('/')?;
            let host = authority.rsplit_once('@').map_or(authority, |(_, h)| h);
            let host = host.split_once(':').map_or(host, |(h, _)| h);
            is_github(host).then_some(path)?
        }
        // scp-like syntax: `[user@]github.com:o/r`.
        None => {
            let (authority, path) = url.split_once(':')?;
            let host = authority.rsplit_once('@').map_or(authority, |(_, h)| h);
            is_github(host).then_some(path)?
        }
    };
    let path = path.trim_matches('/');
    let path = path.strip_suffix(".git").unwrap_or(path);
    let (owner, repo) = path.split_once('/')?;
    if owner.is_empty() || repo.is_empty() || repo.contains('/') {
        return None;
    }
    Some((owner.to_owned(), repo.to_owned()))
}

fn is_github(host: &str) -> bool {
    host.eq_ignore_ascii_case("github.com") || host.eq_ignore_ascii_case("www.github.com")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_github_remotes() {
        let expected = Some(("cs50".to_owned(), "fall-hello-alice".to_owned()));
        assert_eq!(
            parse_github_remote("git@github.com:cs50/fall-hello-alice.git"),
            expected
        );
        assert_eq!(
            parse_github_remote("https://github.com/cs50/fall-hello-alice"),
            expected
        );
        assert_eq!(
            parse_github_remote("ssh://git@github.com/cs50/fall-hello-alice.git"),
            expected
        );
        for url in [
            "https://alice@github.com/cs50/fall-hello-alice.git",
            "https://alice:token@github.com/cs50/fall-hello-alice",
            "https://GitHub.com/cs50/fall-hello-alice/",
            "https://www.github.com/cs50/fall-hello-alice.git",
            "ssh://git@github.com:22/cs50/fall-hello-alice.git",
            "git://github.com/cs50/fall-hello-alice.git",
            "github.com:cs50/fall-hello-alice.git",
            "org-123@github.com:cs50/fall-hello-alice.git",
        ] {
            assert_eq!(parse_github_remote(url), expected, "{url}");
        }
        assert_eq!(parse_github_remote("https://gitlab.com/cs50/x.git"), None);
        assert_eq!(parse_github_remote("https://github.com.evil.io/cs50/x"), None);
        assert_eq!(parse_github_remote("git@gitlab.com:cs50/x.git"), None);
        assert_eq!(parse_github_remote("https://github.com/cs50"), None);
        assert_eq!(parse_github_remote("C:/Users/alice/hello"), None);
        assert_eq!(parse_github_remote("/home/alice/hello.git"), None);
    }

    #[test]
    fn reads_the_origin_of_worktrees_and_quoted_urls() {
        let root = std::env::temp_dir().join(format!("c50tb-origin-{}", std::process::id()));
        let main = root.join("main");
        let admin = main.join(".git").join("worktrees").join("wt");
        let worktree = root.join("wt");
        std::fs::create_dir_all(&admin).unwrap();
        std::fs::create_dir_all(&worktree).unwrap();
        std::fs::write(
            main.join(".git").join("config"),
            "[remote \"origin\"]\r\n\tURL = \"https://alice@github.com/cs50/fall-hello-alice.git\"\r\n",
        )
        .unwrap();
        std::fs::write(admin.join("commondir"), "../..\n").unwrap();
        std::fs::write(
            worktree.join(".git"),
            format!("gitdir: {}\n", admin.display()),
        )
        .unwrap();

        let expected = Some(("cs50".to_owned(), "fall-hello-alice".to_owned()));
        let (from_main, from_worktree) = (origin_of(&main), origin_of(&worktree));
        std::fs::remove_dir_all(&root).unwrap();
        assert_eq!(from_main, expected);
        assert_eq!(from_worktree, expected);
    }

    #[test]
    fn scans_for_marker_files_and_github_clones() {
        let root = std::env::temp_dir().join(format!("c50tb-scan-{}", std::process::id()));
        let repo = root.join("course").join("hello");
        let nested = repo.join("sub");
        let hidden = root.join(".cache").join("ignored");
        let bare_clone = root.join("course").join("lab3");
        let local_only = root.join("scratch");
        for dir in [
            &nested,
            &hidden,
            &repo.join(".git"),
            &bare_clone.join(".git"),
            &local_only.join(".git"),
        ] {
            std::fs::create_dir_all(dir).unwrap();
        }
        let yaml = "classroom: fall\nassignment: hello\n";
        std::fs::write(repo.join(REPO_CONFIG_FILE), yaml).unwrap();
        std::fs::write(nested.join(REPO_CONFIG_FILE), yaml).unwrap();
        std::fs::write(hidden.join(REPO_CONFIG_FILE), yaml).unwrap();
        let origin =
            |url: &str| format!("[core]\n\tbare = false\n[remote \"origin\"]\n\turl = {url}\n");
        std::fs::write(
            repo.join(".git").join("config"),
            origin("git@github.com:cs50/fall-hello-alice.git"),
        )
        .unwrap();
        std::fs::write(
            bare_clone.join(".git").join("config"),
            origin("https://github.com/cs50/fall-lab3-group-5"),
        )
        .unwrap();
        std::fs::write(local_only.join(".git").join("config"), "[core]\n").unwrap();

        let mut projects = scan(&root, DEFAULT_MAX_DEPTH);
        std::fs::remove_dir_all(&root).unwrap();
        projects.sort_by(|a, b| a.path.cmp(&b.path));

        assert_eq!(projects.len(), 2);
        assert_eq!(projects[0].path, repo);
        assert_eq!(projects[0].owner.as_deref(), Some("cs50"));
        assert_eq!(projects[0].repo.as_deref(), Some("fall-hello-alice"));
        assert_eq!(projects[0].config.as_ref().unwrap().assignment, "hello");
        assert_eq!(projects[1].path, bare_clone);
        assert_eq!(projects[1].repo.as_deref(), Some("fall-lab3-group-5"));
        assert!(projects[1].config.is_none());
    }
}
