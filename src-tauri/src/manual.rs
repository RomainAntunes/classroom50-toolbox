//! Projects the student adds by hand: work Classroom 50 doesn't track (another platform, a
//! date given in class, a repo of their own). Each has a name, a class label of their own,
//! maybe a due date, notes, a folder on disk and a GitHub repo. They live in
//! `manual-projects.json` in the app config dir (so Clear cache keeps them), never on GitHub
//! or in the folder, and every build turns them into rows (`projects::build`): reminders,
//! calendar events and the tray's deadlines follow from `Project.manual`.

use std::collections::hash_map::RandomState;
use std::hash::BuildHasher;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::error::{Error, Result};

const FILE_NAME: &str = "manual-projects.json";
/// Ids start with this, which no `owner/repo` can (logins have no `:`).
pub const ID_PREFIX: &str = "manual:";
/// More than a student plausibly keeps by hand.
pub const MAX_PROJECTS: usize = 100;
/// A name's or a class label's length in UTF-16 units, as the UI counts it.
pub const MAX_NAME: usize = 80;
/// The notes' length in UTF-16 units.
pub const MAX_NOTES: usize = 4000;

/// One project as stored, and as Settings and the form read it back.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(specta::Type))]
#[serde(rename_all = "camelCase")]
pub struct ManualProject {
    /// `manual:<random>`, made when it is added and stable across edits: hidden ids,
    /// phases, reminders and calendar UIDs go by it.
    pub id: String,
    pub name: String,
    /// The class label the student typed: projects with the same one share a section.
    #[serde(default)]
    pub class: Option<String>,
    #[serde(default)]
    pub due: Option<DateTime<Utc>>,
    /// Plain text, never rendered as Markdown.
    #[serde(default)]
    pub notes: Option<String>,
    /// Ticked once it's handed in: no more reminders.
    #[serde(default)]
    pub done: bool,
    #[serde(default)]
    pub path: Option<PathBuf>,
    /// `owner/repo` as typed; the folder's GitHub origin wins when it has one.
    #[serde(default)]
    pub repo: Option<String>,
}

/// What the form sends: a project without an id is a new one.
#[derive(Debug, Clone, Deserialize)]
#[cfg_attr(test, derive(specta::Type))]
#[serde(rename_all = "camelCase")]
pub struct ManualDraft {
    #[serde(default)]
    pub id: Option<String>,
    pub name: String,
    #[serde(default)]
    pub class: Option<String>,
    #[serde(default)]
    pub due: Option<DateTime<Utc>>,
    #[serde(default)]
    pub notes: Option<String>,
    #[serde(default)]
    pub done: bool,
    #[serde(default)]
    pub path: Option<PathBuf>,
    /// `owner/repo`, or a GitHub URL pasted as is.
    #[serde(default)]
    pub repo: Option<String>,
}

/// What a row carries of its manual project, beside the name (`config.assignment`), the
/// class (`config.classroom`), the folder (`path`) and the repo (`owner`, `repo`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(specta::Type))]
#[serde(rename_all = "camelCase")]
pub struct ManualInfo {
    #[serde(default)]
    pub due: Option<DateTime<Utc>>,
    #[serde(default)]
    pub notes: Option<String>,
    #[serde(default)]
    pub done: bool,
    /// The repo the student typed, for the form; `owner`/`repo` say which one is used.
    #[serde(default)]
    pub repo: Option<String>,
}

impl From<&ManualProject> for ManualInfo {
    fn from(project: &ManualProject) -> Self {
        Self {
            due: project.due,
            notes: project.notes.clone(),
            done: project.done,
            repo: project.repo.clone(),
        }
    }
}

pub struct ManualStore {
    path: PathBuf,
    projects: Mutex<Vec<ManualProject>>,
    /// Held across every write, so two saves never undo each other.
    writing: Mutex<()>,
}

impl ManualStore {
    /// Loads `<dir>/manual-projects.json`. A corrupt file is moved aside to
    /// `manual-projects.json.bak` rather than overwritten at the next save.
    pub fn load(dir: &Path) -> Self {
        let path = dir.join(FILE_NAME);
        let projects = match std::fs::read_to_string(&path) {
            Ok(json) => serde_json::from_str(&json).unwrap_or_else(|err| {
                log::warn!("unreadable {}: {err}; starting empty", path.display());
                let _ = std::fs::rename(&path, path.with_extension("json.bak"));
                Vec::new()
            }),
            Err(_) => Vec::new(),
        };
        Self {
            path,
            projects: Mutex::new(projects),
            writing: Mutex::new(()),
        }
    }

    pub fn all(&self) -> Vec<ManualProject> {
        self.projects.lock().map(|p| p.clone()).unwrap_or_default()
    }

    /// Adds `draft` (no id) or replaces the project it names, once `validate` accepts it.
    /// Returns what was stored.
    pub fn save(&self, draft: ManualDraft) -> Result<ManualProject> {
        let mut saved = None;
        self.change(|all| {
            let project = validate(draft, all)?;
            match all.iter_mut().find(|p| p.id == project.id) {
                Some(slot) => *slot = project.clone(),
                None => all.push(project.clone()),
            }
            saved = Some(project);
            Ok(true)
        })?;
        saved.ok_or(Error::InvalidManualProject)
    }

    /// Forgets the project; whether it was there. Its folder is never touched.
    pub fn remove(&self, id: &str) -> Result<bool> {
        let mut removed = false;
        self.change(|all| {
            let before = all.len();
            all.retain(|p| p.id != id);
            removed = all.len() != before;
            Ok(removed)
        })?;
        Ok(removed)
    }

    fn change(&self, change: impl FnOnce(&mut Vec<ManualProject>) -> Result<bool>) -> Result<()> {
        let _writing = self.writing.lock().unwrap_or_else(|e| e.into_inner());
        let mut next = self.all();
        if !change(&mut next)? {
            return Ok(());
        }
        let json = serde_json::to_vec_pretty(&next).map_err(std::io::Error::other)?;
        crate::github::write_atomic(&self.path, &json)?;
        if let Ok(mut current) = self.projects.lock() {
            *current = next;
        }
        Ok(())
    }
}

/// Checks what the form sends against the projects already stored: a name of 1 to
/// `MAX_NAME` on one line, a class label of at most `MAX_NAME`, notes of at most
/// `MAX_NOTES`, a folder that exists, a repo that reads as `owner/repo`, an id it knows (an
/// edit) or none (a new one, within `MAX_PROJECTS`). Blank optional fields become `None`.
pub fn validate(draft: ManualDraft, existing: &[ManualProject]) -> Result<ManualProject> {
    let name = draft.name.trim().to_owned();
    if !one_line(&name, MAX_NAME) || name.is_empty() {
        return Err(Error::InvalidManualProject);
    }
    let class = blank_to_none(draft.class);
    if class.as_deref().is_some_and(|c| !one_line(c, MAX_NAME)) {
        return Err(Error::InvalidManualProject);
    }
    let notes = draft
        .notes
        .map(|n| n.trim_end().to_owned())
        .filter(|n| !n.trim().is_empty());
    if notes
        .as_deref()
        .is_some_and(|n| n.encode_utf16().count() > MAX_NOTES)
    {
        return Err(Error::InvalidManualProject);
    }
    let repo = match blank_to_none(draft.repo) {
        Some(text) => {
            let (owner, repo) = parse_repo(&text).ok_or(Error::InvalidManualProject)?;
            Some(format!("{owner}/{repo}"))
        }
        None => None,
    };
    let path = draft.path.filter(|p| !p.as_os_str().is_empty());
    if path
        .as_deref()
        .is_some_and(|p| !p.is_absolute() || !p.is_dir())
    {
        return Err(Error::InvalidManualProject);
    }
    let id = match draft.id {
        Some(id) if existing.iter().any(|p| p.id == id) => id,
        Some(_) => return Err(Error::InvalidManualProject),
        None if existing.len() >= MAX_PROJECTS => return Err(Error::InvalidManualProject),
        None => new_id(existing),
    };
    Ok(ManualProject {
        id,
        name,
        class,
        due: draft.due,
        notes,
        done: draft.done,
        path,
        repo,
    })
}

/// Trimmed, on one line, at most `max` UTF-16 units.
fn one_line(text: &str, max: usize) -> bool {
    text.encode_utf16().count() <= max && !text.chars().any(char::is_control)
}

fn blank_to_none(text: Option<String>) -> Option<String> {
    text.map(|t| t.trim().to_owned()).filter(|t| !t.is_empty())
}

/// `owner/repo` from what the student typed: `owner/repo`, `github.com/owner/repo` or any
/// GitHub URL git would take, with or without `.git`.
pub fn parse_repo(text: &str) -> Option<(String, String)> {
    let text = text.trim();
    let (owner, repo) = crate::discovery::parse_github_remote(text).or_else(|| {
        let path = text
            .strip_prefix("www.")
            .unwrap_or(text)
            .strip_prefix("github.com/")
            .unwrap_or(text)
            .trim_end_matches('/');
        let (owner, repo) = path.split_once('/')?;
        Some((owner.to_owned(), repo.trim_end_matches(".git").to_owned()))
    })?;
    (crate::student::valid_login(&owner) && crate::deeplink::valid_repo_name(&repo))
        .then_some((owner, repo))
}

/// A fresh `manual:<16 hex digits>`, from std's randomly seeded hasher and the clock, and
/// none of `existing`'s.
fn new_id(existing: &[ManualProject]) -> String {
    let nanos = Utc::now().timestamp_nanos_opt().unwrap_or_default();
    let mut salt = 0u64;
    loop {
        let bits = RandomState::new().hash_one((nanos, salt, std::process::id()));
        let id = format!("{ID_PREFIX}{bits:016x}");
        if !existing.iter().any(|p| p.id == id) {
            return id;
        }
        salt += 1;
    }
}

pub fn is_manual_id(id: &str) -> bool {
    id.starts_with(ID_PREFIX)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn draft(name: &str) -> ManualDraft {
        ManualDraft {
            id: None,
            name: name.into(),
            class: None,
            due: None,
            notes: None,
            done: false,
            path: None,
            repo: None,
        }
    }

    fn temp_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("c50tb-manual-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn validate_trims_and_blanks_optional_fields() {
        let mut d = draft("  Essay ");
        d.class = Some("  ".into());
        d.notes = Some(" \n ".into());
        d.repo = Some(String::new());
        let project = validate(d, &[]).unwrap();
        assert_eq!(project.name, "Essay");
        assert_eq!(project.class, None);
        assert_eq!(project.notes, None);
        assert_eq!(project.repo, None);
        assert!(is_manual_id(&project.id));
    }

    #[test]
    fn validate_refuses_bad_fields() {
        for bad in ["", "   ", "two\nlines", &"x".repeat(MAX_NAME + 1)] {
            assert!(validate(draft(bad), &[]).is_err(), "{bad:?}");
        }
        let mut d = draft("Essay");
        d.class = Some("a\tb".into());
        assert!(validate(d, &[]).is_err());
        let mut d = draft("Essay");
        d.notes = Some("x".repeat(MAX_NOTES + 1));
        assert!(validate(d, &[]).is_err());
        let mut d = draft("Essay");
        d.repo = Some("not a repo".into());
        assert!(validate(d, &[]).is_err());
        let mut d = draft("Essay");
        d.path = Some(std::env::temp_dir().join("c50tb-surely-missing-folder"));
        assert!(validate(d, &[]).is_err());
        let mut d = draft("Essay");
        d.path = Some("relative/folder".into());
        assert!(validate(d, &[]).is_err());
        let mut d = draft("Essay");
        d.id = Some("manual:unknown".into());
        assert!(validate(d, &[]).is_err());
    }

    #[test]
    fn validate_keeps_multiline_notes_and_an_existing_folder() {
        let dir = temp_dir("folder");
        let mut d = draft("Essay");
        d.notes = Some("Line one\nLine two\n\n".into());
        d.path = Some(dir.clone());
        let project = validate(d, &[]).unwrap();
        assert_eq!(project.notes.as_deref(), Some("Line one\nLine two"));
        assert_eq!(project.path, Some(dir));
    }

    #[test]
    fn validate_caps_the_count_but_edits_any() {
        let existing: Vec<ManualProject> = (0..MAX_PROJECTS)
            .map(|i| ManualProject {
                id: format!("{ID_PREFIX}{i:016x}"),
                ..validate(draft(&format!("P{i}")), &[]).unwrap()
            })
            .collect();
        assert!(validate(draft("One more"), &existing).is_err());
        let mut edit = draft("Renamed");
        edit.id = Some(existing[3].id.clone());
        assert_eq!(validate(edit, &existing).unwrap().id, existing[3].id);
    }

    #[test]
    fn reads_repos_as_typed_or_pasted() {
        let expected = Some(("octo".to_owned(), "essay".to_owned()));
        for text in [
            "octo/essay",
            " octo/essay.git ",
            "github.com/octo/essay",
            "www.github.com/octo/essay/",
            "https://github.com/octo/essay",
            "git@github.com:octo/essay.git",
        ] {
            assert_eq!(parse_repo(text), expected, "{text}");
        }
        for text in [
            "octo",
            "octo/",
            "/essay",
            "-octo/essay",
            "octo/es say",
            "https://gitlab.com/octo/essay",
        ] {
            assert_eq!(parse_repo(text), None, "{text}");
        }
    }

    #[test]
    fn ids_are_fresh() {
        let a = new_id(&[]);
        let b = new_id(&[]);
        assert_ne!(a, b);
        assert!(is_manual_id(&a));
        assert_eq!(a.len(), ID_PREFIX.len() + 16);
    }

    #[test]
    fn store_adds_edits_removes_and_reloads() {
        let dir = temp_dir("store");
        let store = ManualStore::load(&dir);
        let added = store.save(draft("Essay")).unwrap();
        let mut edit = draft("Essay, final");
        edit.id = Some(added.id.clone());
        edit.done = true;
        store.save(edit).unwrap();
        let reloaded = ManualStore::load(&dir).all();
        assert_eq!(reloaded.len(), 1);
        assert_eq!(reloaded[0].name, "Essay, final");
        assert!(reloaded[0].done);
        assert!(store.remove(&added.id).unwrap());
        assert!(!store.remove(&added.id).unwrap());
        assert!(ManualStore::load(&dir).all().is_empty());
    }

    #[test]
    fn a_corrupt_file_is_moved_aside() {
        let dir = temp_dir("corrupt");
        std::fs::write(dir.join(FILE_NAME), "{not json").unwrap();
        assert!(ManualStore::load(&dir).all().is_empty());
        assert!(dir.join("manual-projects.json.bak").exists());
    }
}
