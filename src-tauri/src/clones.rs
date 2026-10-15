//! Clones made by the app (discovery source 3). Each one is registered when it's created,
//! so it's found even outside the watched folders and the editors' projects. The list is
//! kept in `clones.json` in the app config dir, next to the settings, because clearing the
//! cache must not forget it.

use std::ffi::OsStr;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use crate::error::{Error, Result};
use crate::process;

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

/// Clones `owner/repo` into `parent/<repo>` with `gh repo clone`, which uses gh's
/// credentials, so a private repo needs no git credential setup. Returns the new folder.
pub async fn clone(owner: &str, repo: &str, parent: &Path) -> Result<PathBuf> {
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
    let name = format!("{owner}/{repo}");
    let args = [
        OsStr::new("repo"),
        OsStr::new("clone"),
        OsStr::new(&name),
        target.as_os_str(),
    ];
    process::run("gh", args, None).await?;
    Ok(target)
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
        let result = clone("owner", "repo", &dir).await;
        assert!(matches!(result, Err(Error::PathExists(_))));
        let result = clone("owner", "..", &dir).await;
        assert!(matches!(result, Err(Error::Unsupported(_))));
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
