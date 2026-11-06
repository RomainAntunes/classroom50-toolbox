//! Phases the student sets on a project: named dates the teacher announced outside the
//! published manifest (intermediate hand-ins, a project with no due date). They live in
//! `phases.json` in the app config dir, keyed by project id, never on GitHub or in the
//! clone, and reach the rest of the app on `Project.phases`: reminders (`alerts`), calendar
//! events (`calendar`), the tray's deadlines and the row's date line (UI).

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::error::{Error, Result};
use crate::projects::Project;

const FILE_NAME: &str = "phases.json";
/// As many as a class plausibly announces, with room to spare.
pub const MAX_PHASES: usize = 30;
/// A phase name's length in UTF-16 units, as the UI counts it.
pub const MAX_NAME: usize = 80;
const MAX_ID: usize = 40;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(specta::Type), serde(rename = "ProjectPhase"))]
#[serde(rename_all = "camelCase")]
pub struct Phase {
    /// Made by the UI (`crypto.randomUUID()`), stable across edits: calendar UIDs and
    /// reminder keys go by it.
    pub id: String,
    pub name: String,
    pub due: DateTime<Utc>,
    /// Ticked by the student once it's handed in: no more reminders.
    #[serde(default)]
    pub done: bool,
}

pub struct PhaseStore {
    path: PathBuf,
    phases: Mutex<BTreeMap<String, Vec<Phase>>>,
    /// Held across every write, so a rename and a save never undo each other.
    writing: Mutex<()>,
}

impl PhaseStore {
    /// Loads `<dir>/phases.json`. A corrupt file is moved aside to `phases.json.bak`
    /// rather than overwritten at the next save.
    pub fn load(dir: &Path) -> Self {
        let path = dir.join(FILE_NAME);
        let phases = match std::fs::read_to_string(&path) {
            Ok(json) => serde_json::from_str(&json).unwrap_or_else(|err| {
                log::warn!("unreadable {}: {err}; starting empty", path.display());
                let _ = std::fs::rename(&path, path.with_extension("json.bak"));
                BTreeMap::new()
            }),
            Err(_) => BTreeMap::new(),
        };
        Self {
            path,
            phases: Mutex::new(phases),
            writing: Mutex::new(()),
        }
    }

    pub fn get(&self, id: &str) -> Vec<Phase> {
        self.phases
            .lock()
            .ok()
            .and_then(|p| p.get(id).cloned())
            .unwrap_or_default()
    }

    pub fn all(&self) -> BTreeMap<String, Vec<Phase>> {
        self.phases.lock().map(|p| p.clone()).unwrap_or_default()
    }

    /// Replaces the project's phases (already `validate`d); an empty list forgets it.
    pub fn set(&self, id: &str, phases: Vec<Phase>) -> Result<()> {
        self.change(|all| {
            if phases.is_empty() {
                all.remove(id).is_some()
            } else {
                all.insert(id.to_owned(), phases);
                true
            }
        })
    }

    /// Moves each renamed project's phases to its new id (`projects::renamed_ids`). The
    /// new id keeps its own list when it already has one.
    pub fn rename(&self, renames: &[(String, String)]) -> Result<()> {
        self.change(|all| renamed(all, renames))
    }

    fn change(&self, change: impl FnOnce(&mut BTreeMap<String, Vec<Phase>>) -> bool) -> Result<()> {
        let _writing = self.writing.lock().unwrap_or_else(|e| e.into_inner());
        let mut next = self.all();
        if !change(&mut next) {
            return Ok(());
        }
        let json = serde_json::to_vec_pretty(&next).map_err(std::io::Error::other)?;
        crate::github::write_atomic(&self.path, &json)?;
        if let Ok(mut current) = self.phases.lock() {
            *current = next;
        }
        Ok(())
    }
}

/// Whether anything moved.
fn renamed(all: &mut BTreeMap<String, Vec<Phase>>, renames: &[(String, String)]) -> bool {
    let mut moved = false;
    for (old, new) in renames {
        if all.contains_key(new) {
            continue;
        }
        if let Some(phases) = all.remove(old) {
            all.insert(new.clone(), phases);
            moved = true;
        }
    }
    moved
}

/// Checks what the UI sends: names trimmed, on one line and at most `MAX_NAME` long, ids
/// plain and unique, at most `MAX_PHASES`. Returns them by date, then name.
pub fn validate(phases: Vec<Phase>) -> Result<Vec<Phase>> {
    if phases.len() > MAX_PHASES {
        return Err(Error::InvalidPhase);
    }
    let mut seen = std::collections::BTreeSet::new();
    let mut out = Vec::with_capacity(phases.len());
    for phase in phases {
        let name = phase.name.trim().to_owned();
        let valid_name = !name.is_empty()
            && name.encode_utf16().count() <= MAX_NAME
            && !name.chars().any(char::is_control);
        if !valid_name || !valid_id(&phase.id) || !seen.insert(phase.id.clone()) {
            return Err(Error::InvalidPhase);
        }
        out.push(Phase { name, ..phase });
    }
    out.sort_by(|a, b| a.due.cmp(&b.due).then_with(|| a.name.cmp(&b.name)));
    Ok(out)
}

fn valid_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= MAX_ID
        && id.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-')
}

/// The phases that may still remind: not done, on an assignment that isn't closed or
/// archived (as `alerts::open_due`). A project whose assignment the app doesn't know
/// keeps its phases: the student set them.
pub fn open_phases(project: &Project) -> impl Iterator<Item = &Phase> {
    let closed = project.assignment.as_ref().is_some_and(|a| a.closed)
        || project.remote.as_ref().is_some_and(|r| r.archived);
    project.phases.iter().filter(move |p| !closed && !p.done)
}

/// The earliest open phase, past or not: what the row shows when the assignment has no
/// due date of its own.
pub fn next_open_phase(project: &Project) -> Option<&Phase> {
    open_phases(project).min_by_key(|p| p.due)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn phase(id: &str, name: &str, due: &str) -> Phase {
        Phase {
            id: id.into(),
            name: name.into(),
            due: DateTime::parse_from_rfc3339(due).unwrap().to_utc(),
            done: false,
        }
    }

    fn temp_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("c50tb-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        dir
    }

    #[test]
    fn validate_trims_and_sorts_by_date() {
        let phases = vec![
            phase("b", " Phase 2 ", "2026-12-01T12:00:00Z"),
            phase("a", "Phase 1", "2026-11-15T12:00:00Z"),
            phase("c", "Draft", "2026-12-01T12:00:00Z"),
        ];
        let out = validate(phases).unwrap();
        let names: Vec<_> = out.iter().map(|p| p.name.as_str()).collect();
        assert_eq!(names, ["Phase 1", "Draft", "Phase 2"]);
    }

    #[test]
    fn validate_refuses_bad_names_and_ids() {
        let ok = "2026-11-15T12:00:00Z";
        let refused = [
            vec![phase("a", "  ", ok)],
            vec![phase("a", "two\nlines", ok)],
            vec![phase("a", &"x".repeat(MAX_NAME + 1), ok)],
            vec![phase("", "Phase", ok)],
            vec![phase("../x", "Phase", ok)],
            vec![phase(&"a".repeat(MAX_ID + 1), "Phase", ok)],
            vec![phase("a", "One", ok), phase("a", "Two", ok)],
            (0..=MAX_PHASES)
                .map(|i| phase(&format!("p{i}"), "Phase", ok))
                .collect(),
        ];
        for phases in refused {
            assert!(matches!(validate(phases), Err(Error::InvalidPhase)));
        }
        let long = "é".repeat(MAX_NAME);
        assert!(validate(vec![phase("a", &long, ok)]).is_ok());
        assert!(validate(Vec::new()).unwrap().is_empty());
    }

    #[test]
    fn store_saves_reloads_and_forgets() {
        let dir = temp_dir("phases-store");
        let store = PhaseStore::load(&dir);
        let phases = vec![phase("a", "Phase 1", "2026-11-15T12:00:00Z")];
        store.set("cs50/lab-1-alex", phases.clone()).unwrap();
        assert_eq!(PhaseStore::load(&dir).get("cs50/lab-1-alex"), phases);
        store.set("cs50/lab-1-alex", Vec::new()).unwrap();
        assert!(PhaseStore::load(&dir).all().is_empty());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn corrupt_file_is_moved_aside() {
        let dir = temp_dir("phases-corrupt");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join(FILE_NAME), "{not json").unwrap();
        let store = PhaseStore::load(&dir);
        assert!(store.all().is_empty());
        assert!(dir.join("phases.json.bak").exists());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn rename_moves_phases_unless_the_new_id_has_some() {
        let one = vec![phase("a", "Old", "2026-11-15T12:00:00Z")];
        let two = vec![phase("b", "New", "2026-11-16T12:00:00Z")];
        let mut all = BTreeMap::from([
            ("o/old-1".to_owned(), one.clone()),
            ("o/old-2".to_owned(), one.clone()),
            ("o/new-2".to_owned(), two.clone()),
        ]);
        let renames = [
            ("o/old-1".to_owned(), "o/new-1".to_owned()),
            ("o/old-2".to_owned(), "o/new-2".to_owned()),
        ];
        assert!(renamed(&mut all, &renames));
        assert_eq!(all.get("o/new-1"), Some(&one));
        assert_eq!(all.get("o/new-2"), Some(&two));
        assert!(!all.contains_key("o/old-1"));
        assert!(!renamed(&mut all, &renames[..1]));
    }
}
