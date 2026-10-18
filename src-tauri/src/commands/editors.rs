//! Opening a clone in a terminal or an editor.

use std::path::PathBuf;

use serde::Serialize;

use crate::error::{Error, Result};
use crate::platform;

#[tauri::command]
pub fn open_terminal(path: PathBuf) -> Result<()> {
    platform::open_terminal(&path)
}

#[derive(Debug, Serialize)]
pub struct EditorInfo {
    id: &'static str,
    pub(super) name: &'static str,
}

/// The editors installed here that can open a folder, in a stable order.
#[tauri::command]
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

#[tauri::command]
pub async fn open_in_editor(path: PathBuf, editor: String) -> Result<()> {
    tauri::async_runtime::spawn_blocking(move || crate::editors::open(&editor, &path))
        .await
        .map_err(|err| Error::Unsupported(err.to_string()))?
}
