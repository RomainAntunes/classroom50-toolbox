//! Watches the folders from the settings so a `git clone`, a commit or an edit inside them
//! updates the project list within seconds, without any GitHub request.

use std::path::{Component, Path};
use std::sync::Mutex;
use std::time::Duration;

use notify::{Event, EventKind, RecommendedWatcher, RecursiveMode, Watcher};
use tauri::{AppHandle, Manager};
use tokio::sync::mpsc;

use crate::discovery::SKIPPED_DIRS;
use crate::projects;
use crate::settings::SettingsStore;

/// A clone writes thousands of files; wait for the burst to settle before rescanning.
const DEBOUNCE: Duration = Duration::from_secs(2);

#[derive(Default)]
pub struct FolderWatcher(Mutex<Option<RecommendedWatcher>>);

/// (Re)starts watching the current watched folders. Replacing the old watcher closes its
/// channel, which ends the old debounce task.
pub fn rewatch(app: &AppHandle) {
    let folders = app.state::<SettingsStore>().get().watched_folders;
    let (tx, mut rx) = mpsc::unbounded_channel::<()>();
    let watcher = notify::recommended_watcher(move |res: notify::Result<Event>| {
        if res.is_ok_and(|event| is_relevant(&event)) {
            let _ = tx.send(());
        }
    });
    let mut watcher = match watcher {
        Ok(watcher) => watcher,
        Err(err) => {
            log::warn!("folder watching unavailable: {err}");
            return;
        }
    };
    for folder in &folders {
        if let Err(err) = watcher.watch(folder, RecursiveMode::Recursive) {
            log::warn!("cannot watch {}: {err}", folder.display());
        }
    }
    if let Ok(mut current) = app.state::<FolderWatcher>().0.lock() {
        *current = Some(watcher);
    }

    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        while rx.recv().await.is_some() {
            tokio::time::sleep(DEBOUNCE).await;
            while rx.try_recv().is_ok() {}
            projects::refresh_local(&app).await;
        }
    });
}

/// Writes anywhere in a watched folder count, except reads (our own `git status`) and
/// build or dependency output.
fn is_relevant(event: &Event) -> bool {
    !matches!(event.kind, EventKind::Access(_)) && event.paths.iter().any(|path| !is_ignored(path))
}

fn is_ignored(path: &Path) -> bool {
    path.components().any(|component| match component {
        Component::Normal(name) => {
            let name = name.to_string_lossy();
            SKIPPED_DIRS.contains(&name.as_ref())
                || (name.starts_with('.') && name != ".git" && name != ".classroom50.yaml")
        }
        _ => false,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ignores_build_output_and_hidden_dirs() {
        assert!(!is_ignored(Path::new("/code/hello/.classroom50.yaml")));
        assert!(!is_ignored(Path::new("/code/hello/.git/refs/heads/main")));
        assert!(!is_ignored(Path::new("/code/hello/src/main.c")));
        assert!(is_ignored(Path::new("/code/hello/node_modules/x/index.js")));
        assert!(is_ignored(Path::new("/code/hello/target/debug/app")));
        assert!(is_ignored(Path::new("/code/hello/.venv/bin/python")));
    }
}
