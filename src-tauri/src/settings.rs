//! User settings, persisted as JSON in the OS config dir (e.g. `~/Library/Application Support/<id>/`).

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use serde::{Deserialize, Serialize};

use crate::error::Result;

pub const FILE_NAME: &str = "settings.json";

/// Reminders further out than a week aren't useful for a tray app.
const MAX_NOTIFY_HOURS: u32 = 7 * 24;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Settings {
    /// Folders scanned for `.classroom50.yaml` clones.
    pub watched_folders: Vec<PathBuf>,
    /// Also look for clones in the editors' recent projects and default project folders.
    pub scan_editor_projects: bool,
    /// Where the app clones assignments; asked the first time.
    pub clone_folder: Option<PathBuf>,
    pub launch_at_login: bool,
    /// Which assignments the list shows.
    pub list_filter: ListFilter,
    /// Hours before a due date at which to send a reminder, largest first.
    pub notify_hours_before: Vec<u32>,
    /// Editor for clones no editor has opened yet (`editors::Editor::id`); `None` picks the
    /// first one installed.
    pub default_editor: Option<String>,
    /// Project and unaccepted-assignment ids the user hid: left out of the list unless a
    /// search matches them, and never notified about.
    pub hidden: Vec<String>,
    /// The first-run guide was finished or skipped.
    pub onboarded: bool,
}

/// The date filter above the project list.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ListFilter {
    #[default]
    All,
    /// Not closed, due ahead or without a due date.
    Open,
    /// Due within the next 7 days.
    Week,
    /// Due date passed, or closed.
    Past,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            watched_folders: Vec::new(),
            scan_editor_projects: true,
            clone_folder: None,
            launch_at_login: true,
            list_filter: ListFilter::All,
            notify_hours_before: vec![24, 1],
            default_editor: None,
            hidden: Vec::new(),
            onboarded: false,
        }
    }
}

impl Settings {
    /// Drops duplicate folders (keeping order) and out-of-range reminder hours.
    pub fn normalized(mut self) -> Self {
        let mut seen = BTreeSet::new();
        self.watched_folders
            .retain(|path| seen.insert(path.clone()));
        let mut seen = BTreeSet::new();
        self.hidden.retain(|id| seen.insert(id.clone()));

        let hours: BTreeSet<u32> = self
            .notify_hours_before
            .into_iter()
            .filter(|h| (1..=MAX_NOTIFY_HOURS).contains(h))
            .collect();
        self.notify_hours_before = hours.into_iter().rev().collect();
        self
    }
}

pub struct SettingsStore {
    path: PathBuf,
    current: Mutex<Settings>,
}

impl SettingsStore {
    /// Loads `<dir>/settings.json`. Returns `true` as second value on first run (no file).
    /// A corrupt file is moved aside to `settings.json.bak` rather than silently overwritten.
    pub fn load(dir: &Path) -> (Self, bool) {
        let path = dir.join(FILE_NAME);
        let (settings, first_run) = match std::fs::read_to_string(&path) {
            Ok(json) => match serde_json::from_str::<Settings>(&json) {
                Ok(mut settings) => {
                    // Settings from before the guide existed: the app is set up already.
                    if !json.contains("\"onboarded\"") {
                        settings.onboarded = true;
                    }
                    (settings.normalized(), false)
                }
                Err(err) => {
                    log::warn!(
                        "unreadable {}: {err}; starting from defaults",
                        path.display()
                    );
                    let _ = std::fs::rename(&path, path.with_extension("json.bak"));
                    (Settings::default(), false)
                }
            },
            Err(_) => (Settings::default(), true),
        };
        let store = Self {
            path,
            current: Mutex::new(settings),
        };
        (store, first_run)
    }

    pub fn get(&self) -> Settings {
        self.current.lock().map(|s| s.clone()).unwrap_or_default()
    }

    /// Normalizes, writes atomically (temp file + rename), then swaps the in-memory copy.
    pub fn set(&self, settings: Settings) -> Result<Settings> {
        let settings = settings.normalized();
        if let Some(dir) = self.path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let tmp = self.path.with_extension("json.tmp");
        let json = serde_json::to_vec_pretty(&settings).map_err(std::io::Error::other)?;
        std::fs::write(&tmp, json)?;
        std::fs::rename(&tmp, &self.path)?;
        if let Ok(mut current) = self.current.lock() {
            *current = settings.clone();
        }
        Ok(settings)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("c50tb-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        dir
    }

    #[test]
    fn normalizes_folders_and_hours() {
        let settings = Settings {
            watched_folders: vec!["/a".into(), "/b".into(), "/a".into()],
            scan_editor_projects: true,
            clone_folder: None,
            launch_at_login: false,
            list_filter: ListFilter::All,
            notify_hours_before: vec![1, 24, 0, 1, 1000, 6],
            default_editor: None,
            hidden: vec!["a".into(), "a".into()],
            onboarded: true,
        }
        .normalized();
        assert_eq!(
            settings.watched_folders,
            vec![PathBuf::from("/a"), "/b".into()]
        );
        assert_eq!(settings.notify_hours_before, vec![24, 6, 1]);
    }

    #[test]
    fn missing_fields_take_defaults() {
        let settings: Settings = serde_json::from_str(r#"{"watchedFolders": ["/x"]}"#).unwrap();
        assert!(settings.launch_at_login);
        assert!(settings.scan_editor_projects);
        assert_eq!(settings.notify_hours_before, vec![24, 1]);
    }

    #[test]
    fn round_trips_and_reports_first_run() {
        let dir = temp_dir("settings");
        let (store, first_run) = SettingsStore::load(&dir);
        assert!(first_run);

        let mut settings = store.get();
        settings.watched_folders.push("/courses".into());
        store.set(settings).unwrap();

        let (reloaded, first_run) = SettingsStore::load(&dir);
        assert!(!first_run);
        assert_eq!(
            reloaded.get().watched_folders,
            vec![PathBuf::from("/courses")]
        );
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn shows_the_guide_to_new_installs_only() {
        let dir = temp_dir("settings-guide");
        let (store, _) = SettingsStore::load(&dir);
        assert!(!store.get().onboarded);
        // Saved while the guide is still open: it shows again next time.
        store.set(store.get()).unwrap();
        assert!(!SettingsStore::load(&dir).0.get().onboarded);
        // A file from before the guide existed: already set up.
        std::fs::write(dir.join(FILE_NAME), r#"{"watchedFolders": []}"#).unwrap();
        assert!(SettingsStore::load(&dir).0.get().onboarded);
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn corrupt_file_is_backed_up() {
        let dir = temp_dir("settings-corrupt");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join(FILE_NAME), "{not json").unwrap();

        let (store, first_run) = SettingsStore::load(&dir);
        assert!(!first_run);
        assert_eq!(store.get(), Settings::default());
        assert!(dir.join("settings.json.bak").is_file());
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
