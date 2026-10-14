//! Watches the places clones live so a `git clone`, a commit or an edit updates the
//! project list within seconds, without any GitHub request. The targets come from each
//! sync (`projects::watch_targets`): watched folders and known clones recursively, the
//! editors' recent-project files and project folders non-recursively.

use std::path::{Component, Path, PathBuf};
use std::sync::Mutex;
use std::time::Duration;

use notify::{Event, EventKind, RecommendedWatcher, RecursiveMode, Watcher};
use tauri::{AppHandle, Manager};
use tokio::sync::mpsc;

use crate::discovery::SKIPPED_DIRS;
use crate::projects;

/// A clone writes thousands of files; wait for the burst to settle before rescanning.
const DEBOUNCE: Duration = Duration::from_secs(2);

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct WatchTarget {
    pub path: PathBuf,
    pub recursive: bool,
}

impl WatchTarget {
    pub fn recursive(path: PathBuf) -> Self {
        Self {
            path,
            recursive: true,
        }
    }

    /// A file, or a folder whose direct children only matter.
    pub fn flat(path: PathBuf) -> Self {
        Self {
            path,
            recursive: false,
        }
    }
}

#[derive(Default)]
pub struct FolderWatcher(Mutex<Option<(Vec<WatchTarget>, RecommendedWatcher)>>);

/// Watches `targets`, unless they're already what is watched. Replacing the old watcher
/// closes its channel, which ends the old debounce task.
pub fn rewatch(app: &AppHandle, targets: Vec<WatchTarget>) {
    let state = app.state::<FolderWatcher>();
    let Ok(mut current) = state.0.lock() else {
        return;
    };
    if current
        .as_ref()
        .is_some_and(|(watched, _)| *watched == targets)
    {
        return;
    }

    let (tx, mut rx) = mpsc::unbounded_channel::<()>();
    let roots = targets.clone();
    let watcher = notify::recommended_watcher(move |res: notify::Result<Event>| {
        if res.is_ok_and(|event| is_relevant(&event, &roots)) {
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
    for target in &targets {
        let mode = if target.recursive {
            RecursiveMode::Recursive
        } else {
            RecursiveMode::NonRecursive
        };
        if let Err(err) = watcher.watch(&target.path, mode) {
            log::debug!("cannot watch {}: {err}", target.path.display());
        }
    }
    *current = Some((targets, watcher));

    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        while rx.recv().await.is_some() {
            tokio::time::sleep(DEBOUNCE).await;
            while rx.try_recv().is_ok() {}
            projects::refresh_local(&app).await;
        }
    });
}

/// Writes count, except reads (our own `git status`) and build or dependency output below
/// a target. Only the part of the path inside the target is judged, so a target may itself
/// sit in a dot-dir like `~/.config`.
fn is_relevant(event: &Event, targets: &[WatchTarget]) -> bool {
    !matches!(event.kind, EventKind::Access(_))
        && event.paths.iter().any(|path| {
            targets
                .iter()
                .filter_map(|target| path.strip_prefix(&target.path).ok())
                .any(|inside| !is_ignored(inside))
        })
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
    use notify::event::{AccessKind, CreateKind};

    #[test]
    fn ignores_build_output_and_hidden_dirs() {
        assert!(!is_ignored(Path::new("hello/.classroom50.yaml")));
        assert!(!is_ignored(Path::new("hello/.git/refs/heads/main")));
        assert!(!is_ignored(Path::new("hello/src/main.c")));
        assert!(is_ignored(Path::new("hello/node_modules/x/index.js")));
        assert!(is_ignored(Path::new("hello/target/debug/app")));
        assert!(is_ignored(Path::new("hello/.venv/bin/python")));
    }

    #[test]
    fn judges_paths_relative_to_their_target() {
        let targets = [
            WatchTarget::flat("/home/a/.config/JetBrains/IU/options/recentProjects.xml".into()),
            WatchTarget::recursive("/home/a/code".into()),
        ];
        let event = |kind, path: &str| Event::new(kind).add_path(PathBuf::from(path));
        let create = EventKind::Create(CreateKind::Any);
        assert!(is_relevant(
            &event(
                create,
                "/home/a/.config/JetBrains/IU/options/recentProjects.xml"
            ),
            &targets
        ));
        assert!(is_relevant(
            &event(create, "/home/a/code/lab1/.git/HEAD"),
            &targets
        ));
        assert!(!is_relevant(
            &event(create, "/home/a/code/lab1/target/x"),
            &targets
        ));
        assert!(!is_relevant(&event(create, "/home/a/elsewhere"), &targets));
        assert!(!is_relevant(
            &event(EventKind::Access(AccessKind::Any), "/home/a/code/lab1/x"),
            &targets
        ));
    }
}
