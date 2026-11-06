//! The phases a student sets on a project.

use tauri::{AppHandle, Manager};

use crate::error::{Error, Result};
use crate::phases::{self, Phase, PhaseStore};
use crate::projects::{self, ProjectsState};

/// Replaces the project's phases with `phases` (validated, sorted by date) and shows them at
/// once: `projects-updated` follows, reminders and calendar events with it, no GitHub call.
#[tauri::command]
#[cfg_attr(test, specta::specta)]
pub async fn set_phases(app: AppHandle, id: String, phases: Vec<Phase>) -> Result<Vec<Phase>> {
    let known = app
        .state::<ProjectsState>()
        .snapshot()
        .projects
        .iter()
        .any(|p| p.id == id);
    if !known {
        return Err(Error::Unsupported(format!("unknown project {id}")));
    }
    let phases = phases::validate(phases)?;
    app.state::<PhaseStore>().set(&id, phases.clone())?;
    log::info!("saved {} phases on a project", phases.len());
    projects::republish(&app);
    Ok(phases)
}
