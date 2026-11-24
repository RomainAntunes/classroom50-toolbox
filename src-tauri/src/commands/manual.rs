//! Projects the student adds by hand (`manual`).

use tauri::{AppHandle, Manager};

use crate::error::{Error, Result};
use crate::manual::{self, ManualDraft, ManualStore};
use crate::phases::PhaseStore;
use crate::projects::{self, ProjectsState};

/// Adds a manual project (no `id`) or changes the one `id` names, then rescans the disk so
/// its row shows with the folder's state (no GitHub request). Refuses a folder or repo
/// that already is a Classroom 50 project. Returns the project's id.
#[tauri::command]
#[cfg_attr(test, specta::specta)]
pub async fn save_manual_project(app: AppHandle, project: ManualDraft) -> Result<String> {
    let snapshot = app.state::<ProjectsState>().snapshot();
    let repo = project
        .path
        .as_deref()
        .and_then(crate::discovery::origin_url)
        .and_then(|url| crate::discovery::parse_github_remote(&url))
        .or_else(|| manual::parse_repo(project.repo.as_deref()?))
        .map(|(owner, repo)| format!("{owner}/{repo}").to_lowercase());
    let listed = snapshot
        .projects
        .iter()
        .filter(|p| !p.is_manual())
        .any(|p| {
            repo.as_deref() == Some(p.id.as_str())
                || (project.path.is_some() && p.path.as_deref() == project.path.as_deref())
        });
    if listed {
        return Err(Error::AlreadyListed);
    }
    let saved = app.state::<ManualStore>().save(project)?;
    log::info!("saved a manual project");
    projects::refresh_local(&app, false).await;
    Ok(saved.id)
}

/// Forgets a manual project and its phases; its folder is never touched.
#[tauri::command]
#[cfg_attr(test, specta::specta)]
pub async fn remove_manual_project(app: AppHandle, id: String) -> Result<()> {
    if !manual::is_manual_id(&id) || !app.state::<ManualStore>().remove(&id)? {
        return Err(Error::Unsupported(format!("unknown manual project {id}")));
    }
    app.state::<PhaseStore>().set(&id, Vec::new())?;
    log::info!("removed a manual project");
    projects::refresh_local(&app, false).await;
    Ok(())
}
