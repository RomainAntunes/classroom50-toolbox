//! Projects the student adds by hand (`manual`).

use tauri::{AppHandle, Manager};

use crate::error::{Error, Result};
use crate::manual::{self, ManualDraft, ManualStore};
use crate::phases::PhaseStore;
use crate::projects::{self, ProjectsState};

/// Adds a manual project (no `id`) or changes the one `id` names and shows it at once; the
/// folder's state follows from a rescan in the background (no GitHub request). Refuses a
/// folder or repo that already is a Classroom 50 project. Returns the project's id.
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
    shown_then_rescanned(app);
    Ok(saved.id)
}

/// Forgets a manual project and its phases, at once; its folder is never touched.
#[tauri::command]
#[cfg_attr(test, specta::specta)]
pub async fn remove_manual_project(app: AppHandle, id: String) -> Result<()> {
    if !manual::is_manual_id(&id) || !app.state::<ManualStore>().remove(&id)? {
        return Err(Error::Unsupported(format!("unknown manual project {id}")));
    }
    app.state::<PhaseStore>().set(&id, Vec::new())?;
    log::info!("removed a manual project");
    shown_then_rescanned(app);
    Ok(())
}

/// Shows the change at once (`republish`), then reads the folder's state in the background:
/// a rescan can take a while, and waits for a sync already running.
fn shown_then_rescanned(app: AppHandle) {
    projects::republish(&app);
    tauri::async_runtime::spawn(async move {
        projects::refresh_local(&app, false).await;
    });
}
