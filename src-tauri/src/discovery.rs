//! Finds Classroom 50 clones on disk: any directory holding a `.classroom50.yaml`.

use std::path::{Path, PathBuf};

use serde::Serialize;
use walkdir::WalkDir;

use crate::classroom50::{REPO_CONFIG_FILE, RepoConfig};

/// Deep enough for `~/code/<course>/<assignment>`, shallow enough to stay fast on a home dir.
pub const DEFAULT_MAX_DEPTH: usize = 4;

const SKIPPED_DIRS: &[&str] = &["node_modules", "target", "Library", "AppData", "venv"];

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LocalProject {
    pub path: PathBuf,
    pub owner: Option<String>,
    pub repo: Option<String>,
    pub config: RepoConfig,
}

/// Walks `root` and returns every Classroom 50 repo found. A repo's own subtree isn't
/// descended into. Blocking: call from `spawn_blocking`.
pub fn scan(root: &Path, max_depth: usize) -> Vec<LocalProject> {
    let mut projects = Vec::new();
    let mut walker = WalkDir::new(root)
        .max_depth(max_depth)
        .follow_links(false)
        .into_iter()
        .filter_entry(|entry| entry.depth() == 0 || !is_skipped(entry.file_name()));

    while let Some(entry) = walker.next() {
        let Ok(entry) = entry else { continue };
        if !entry.file_type().is_dir() || !entry.path().join(REPO_CONFIG_FILE).is_file() {
            continue;
        }
        if let Ok(Some(config)) = RepoConfig::read(entry.path()) {
            let (owner, repo) = origin_of(entry.path()).unzip();
            projects.push(LocalProject {
                path: entry.path().to_path_buf(),
                owner,
                repo,
                config,
            });
        }
        walker.skip_current_dir();
    }
    projects
}

fn is_skipped(name: &std::ffi::OsStr) -> bool {
    let name = name.to_string_lossy();
    name.starts_with('.') || SKIPPED_DIRS.contains(&name.as_ref())
}

/// `owner/repo` of the `origin` remote, read straight from `.git/config` (no git process).
fn origin_of(repo_dir: &Path) -> Option<(String, String)> {
    let config = std::fs::read_to_string(repo_dir.join(".git").join("config")).ok()?;
    let mut in_origin = false;
    for line in config.lines().map(str::trim) {
        if line.starts_with('[') {
            in_origin = line == r#"[remote "origin"]"#;
        } else if in_origin && let Some(url) = line.strip_prefix("url").map(str::trim_start) {
            return url
                .strip_prefix('=')
                .and_then(|u| parse_github_remote(u.trim()));
        }
    }
    None
}

/// Accepts `git@github.com:o/r.git`, `https://github.com/o/r(.git)` and `ssh://git@github.com/o/r.git`.
pub fn parse_github_remote(url: &str) -> Option<(String, String)> {
    let path = url
        .strip_prefix("git@github.com:")
        .or_else(|| url.strip_prefix("ssh://git@github.com/"))
        .or_else(|| url.strip_prefix("https://github.com/"))
        .or_else(|| url.strip_prefix("http://github.com/"))?;
    let path = path.trim_end_matches('/');
    let path = path.strip_suffix(".git").unwrap_or(path);
    let (owner, repo) = path.split_once('/')?;
    if owner.is_empty() || repo.is_empty() || repo.contains('/') {
        return None;
    }
    Some((owner.to_owned(), repo.to_owned()))
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
        assert_eq!(parse_github_remote("https://gitlab.com/cs50/x.git"), None);
        assert_eq!(parse_github_remote("https://github.com/cs50"), None);
    }

    #[test]
    fn scans_for_marker_files() {
        let root = std::env::temp_dir().join(format!("c50tb-scan-{}", std::process::id()));
        let repo = root.join("course").join("hello");
        let nested = repo.join("sub");
        let hidden = root.join(".cache").join("ignored");
        for dir in [&nested, &hidden, &repo.join(".git")] {
            std::fs::create_dir_all(dir).unwrap();
        }
        let yaml = "classroom: fall\nassignment: hello\n";
        std::fs::write(repo.join(REPO_CONFIG_FILE), yaml).unwrap();
        std::fs::write(nested.join(REPO_CONFIG_FILE), yaml).unwrap();
        std::fs::write(hidden.join(REPO_CONFIG_FILE), yaml).unwrap();
        std::fs::write(
            repo.join(".git").join("config"),
            "[core]\n\tbare = false\n[remote \"origin\"]\n\turl = git@github.com:cs50/fall-hello-alice.git\n",
        )
        .unwrap();

        let projects = scan(&root, DEFAULT_MAX_DEPTH);
        std::fs::remove_dir_all(&root).unwrap();

        assert_eq!(projects.len(), 1);
        assert_eq!(projects[0].path, repo);
        assert_eq!(projects[0].owner.as_deref(), Some("cs50"));
        assert_eq!(projects[0].repo.as_deref(), Some("fall-hello-alice"));
        assert_eq!(projects[0].config.assignment, "hello");
    }
}
