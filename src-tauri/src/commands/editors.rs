//! Opening a clone in a terminal or an editor.

use std::path::PathBuf;

use serde::Serialize;
use tauri::{AppHandle, Manager};

use crate::editor_choices::EditorChoices;
use crate::error::{Error, Result};
use crate::events::{IpcEvent, ProjectsUpdated};
use crate::platform;
use crate::projects::ProjectsState;

#[tauri::command]
#[cfg_attr(test, specta::specta)]
pub fn open_terminal(path: PathBuf) -> Result<()> {
    platform::open_terminal(&path)
}

#[derive(Debug, Serialize)]
#[cfg_attr(test, derive(specta::Type))]
pub struct EditorInfo {
    id: &'static str,
    pub(super) name: &'static str,
}

/// The editors installed here that can open a folder, in a stable order.
#[tauri::command]
#[cfg_attr(test, specta::specta)]
pub async fn installed_editors() -> Vec<EditorInfo> {
    tauri::async_runtime::spawn_blocking(|| {
        crate::editors::installed()
            .into_iter()
            .map(|editor| EditorInfo {
                id: editor.id,
                name: editor.name,
            })
            .collect()
    })
    .await
    .unwrap_or_default()
}

/// Opens the clone at `path` in `editor` and remembers the choice (`editor_choices`): the
/// row's "Open in" offers it from then on, sent at once as `projects-updated`.
#[tauri::command]
#[cfg_attr(test, specta::specta)]
pub async fn open_in_editor(app: AppHandle, path: PathBuf, editor: String) -> Result<()> {
    let dir = path.clone();
    let id = editor.clone();
    tauri::async_runtime::spawn_blocking(move || crate::editors::open(&id, &dir))
        .await
        .map_err(|err| Error::Unsupported(err.to_string()))??;
    let updated = app.state::<ProjectsState>().set_last_editor(&path, &editor);
    let (detected, snapshot) = match updated {
        Some((detected, snapshot)) => (detected, Some(snapshot)),
        None => (None, None),
    };
    // The editor is open by now: a choice that can't be saved is only logged.
    if let Err(err) = app
        .state::<EditorChoices>()
        .record(&path, &editor, detected)
    {
        log::warn!("could not save the editor choice: {err}");
    }
    if let Some(snapshot) = snapshot
        && let Err(err) = ProjectsUpdated::emit(&app, &snapshot)
    {
        log::warn!("could not emit {}: {err}", ProjectsUpdated::NAME);
    }
    Ok(())
}
