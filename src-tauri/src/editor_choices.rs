//! The editor the student last opened each clone with from the app ("Open in X", "Open
//! with", the tray's Open Recent, the editor a new clone opens in), so the row offers that
//! one next time. Kept in `editor-choices.json` in the app config dir, next to
//! `clones.json`, so clearing the cache keeps it; keyed by the clone's canonical path.
//!
//! The editors' own recent lists can't tell when a folder was opened, so the choice wins
//! over them, until they name a different editor than they did when the choice was made:
//! the student opened the clone elsewhere since, and that editor wins again.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::error::Result;

const FILE_NAME: &str = "editor-choices.json";

/// Choices kept at most; the oldest go first. Far more clones than a student keeps.
const MAX_CHOICES: usize = 200;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Choice {
    /// `editors::Editor::id`.
    pub editor: String,
    /// The editor the editors' recent lists named for the clone when the choice was made.
    #[serde(default)]
    pub detected: Option<String>,
    pub at: DateTime<Utc>,
}

impl Choice {
    /// The choice, unless the editors' lists now name another editor than both it and what
    /// they named when it was made.
    pub fn standing(&self, detected: Option<&str>) -> Option<&str> {
        match detected {
            Some(now) if now != self.editor && Some(now) != self.detected.as_deref() => None,
            _ => Some(&self.editor),
        }
    }
}

pub type Choices = BTreeMap<PathBuf, Choice>;

/// The choice made for the clone at `path`, by its path as given, else canonical.
pub fn lookup<'a>(choices: &'a Choices, path: &Path) -> Option<&'a Choice> {
    choices
        .get(path)
        .or_else(|| choices.get(&std::fs::canonicalize(path).ok()?))
}

/// Drops the choices of clones that are gone, then the oldest past `MAX_CHOICES`.
fn prune(choices: &mut Choices, exists: impl Fn(&Path) -> bool) {
    choices.retain(|path, _| exists(path));
    while choices.len() > MAX_CHOICES {
        let oldest = choices
            .iter()
            .min_by_key(|(_, choice)| choice.at)
            .map(|(path, _)| path.clone());
        match oldest {
            Some(path) => choices.remove(&path),
            None => break,
        };
    }
}

pub struct EditorChoices {
    path: PathBuf,
    choices: Mutex<Choices>,
}

impl EditorChoices {
    pub fn load(dir: &Path) -> Self {
        let path = dir.join(FILE_NAME);
        let choices = std::fs::read_to_string(&path)
            .ok()
            .and_then(|json| serde_json::from_str(&json).ok())
            .unwrap_or_default();
        Self {
            path,
            choices: Mutex::new(choices),
        }
    }

    pub fn all(&self) -> Choices {
        self.choices.lock().map(|c| c.clone()).unwrap_or_default()
    }

    /// Records that the clone at `dir` was opened in `editor`, `detected` being the editor
    /// the editors' lists named for it then. Saved at once.
    pub fn record(&self, dir: &Path, editor: &str, detected: Option<String>) -> Result<()> {
        let key = std::fs::canonicalize(dir).unwrap_or_else(|_| dir.to_path_buf());
        let mut choices = self.choices.lock().map_err(|_| poisoned())?;
        choices.insert(
            key,
            Choice {
                editor: editor.to_owned(),
                detected,
                at: Utc::now(),
            },
        );
        prune(&mut choices, Path::is_dir);
        let json = serde_json::to_vec_pretty(&*choices).map_err(std::io::Error::other)?;
        crate::github::write_atomic(&self.path, &json)?;
        Ok(())
    }
}

fn poisoned() -> std::io::Error {
    std::io::Error::other("editor choices lock poisoned")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn choice(editor: &str, detected: Option<&str>, minute: u32) -> Choice {
        Choice {
            editor: editor.to_owned(),
            detected: detected.map(str::to_owned),
            at: format!("2026-11-03T10:{minute:02}:00Z").parse().unwrap(),
        }
    }

    #[test]
    fn the_choice_stands_until_another_editor_opens_the_clone() {
        let picked = choice("datagrip", Some("idea"), 0);
        // Nothing detected, the same as when it was made, or the chosen editor itself.
        assert_eq!(picked.standing(None), Some("datagrip"));
        assert_eq!(picked.standing(Some("idea")), Some("datagrip"));
        assert_eq!(picked.standing(Some("datagrip")), Some("datagrip"));
        // The student opened it in another editor since.
        assert_eq!(picked.standing(Some("vscode")), None);

        let fresh = choice("zed", None, 0);
        assert_eq!(fresh.standing(Some("zed")), Some("zed"));
        assert_eq!(fresh.standing(Some("cursor")), None);
    }

    #[test]
    fn prunes_gone_clones_then_the_oldest() {
        let mut choices: Choices = (0..MAX_CHOICES + 5)
            .map(|i| {
                let path = PathBuf::from(format!("/code/repo-{i}"));
                (path, choice("zed", None, (i % 60) as u32))
            })
            .collect();
        choices.insert("/gone".into(), choice("zed", None, 59));
        prune(&mut choices, |path| path != Path::new("/gone"));
        assert_eq!(choices.len(), MAX_CHOICES);
        assert!(!choices.contains_key(Path::new("/gone")));
        // Minute 0 is the oldest: repo-0, repo-60, repo-120, repo-180 went first.
        assert!(!choices.contains_key(Path::new("/code/repo-0")));
        assert!(choices.contains_key(Path::new("/code/repo-59")));
    }

    #[test]
    fn remembers_choices_across_restarts() {
        let dir = std::env::temp_dir().join(format!("c50tb-editors-{}", std::process::id()));
        let clone = dir.join("repo");
        std::fs::create_dir_all(&clone).unwrap();
        let store = EditorChoices::load(&dir);
        store.record(&clone, "idea", None).unwrap();
        store
            .record(&clone, "datagrip", Some("idea".into()))
            .unwrap();
        store.record(&dir.join("gone"), "zed", None).unwrap();

        let reloaded = EditorChoices::load(&dir).all();
        assert_eq!(reloaded.len(), 1);
        let found = lookup(&reloaded, &clone).unwrap();
        assert_eq!(found.editor, "datagrip");
        assert_eq!(found.detected.as_deref(), Some("idea"));
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
