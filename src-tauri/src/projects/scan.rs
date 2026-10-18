//! Where clones are looked for (watched folders, the app's clones, the editors' recent
//! projects) and what to watch for new ones.

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use tauri::Manager;

use super::{Progress, Project, ProjectsState};
use crate::discovery::{self, LocalProject};
use crate::editors;
use crate::sync_progress::Phase;
use crate::watch::WatchTarget;

/// Threads walking scan roots at once.
const MAX_SCAN_THREADS: usize = 4;

/// Watched folders recursively, clones found elsewhere recursively too (to see commits),
/// and the editor files and folders that could reveal a new clone, non-recursively.
pub(super) fn watch_targets(
    folders: &[PathBuf],
    scan_targets: Vec<WatchTarget>,
    projects: &[Project],
) -> Vec<WatchTarget> {
    let clones = projects
        .iter()
        .filter_map(|p| p.path.clone())
        .filter(|path| !folders.iter().any(|f| path.starts_with(f)));
    let mut targets: Vec<WatchTarget> = folders
        .iter()
        .cloned()
        .chain(clones)
        .map(WatchTarget::recursive)
        .chain(scan_targets)
        .collect();
    targets.sort();
    targets.dedup_by(|a, b| a.path == b.path);
    targets
}

/// Candidate clones, and the editor files and folders to watch for new ones. Looks in the
/// watched folders, the clones the app made and, when `editors` is on, in the editors' recent projects, the folders
/// holding them and the IDEs' default project folders.
pub(super) async fn scan_local(
    folders: Vec<PathBuf>,
    clones: Vec<PathBuf>,
    editors: bool,
    progress: Progress,
) -> (Vec<LocalProject>, Vec<WatchTarget>, Vec<editors::Recent>) {
    tauri::async_runtime::spawn_blocking(move || {
        let (roots, targets, recents) = scan_roots(folders, clones, editors);
        progress.set_total(Phase::Scan, roots.len());
        if let Ok(mut stats) = progress.app.state::<ProjectsState>().stats.lock() {
            stats.scan_roots = roots.len();
        }
        let found = scan_parallel(&roots, &progress);
        progress.finish_phase(Phase::Scan);
        // In root order, so a clone found from two roots is kept once, as before.
        let mut seen = HashSet::new();
        let locals = found
            .into_iter()
            .flatten()
            .filter(|l| seen.insert(l.path.clone()))
            .collect();
        (locals, targets, recents)
    })
    .await
    .unwrap_or_default()
}

/// Walks the roots on a few threads (disk latency, not CPU, is what a walk waits on),
/// capped so a scan never takes over the machine. Results come back in root order.
fn scan_parallel(roots: &[(PathBuf, usize)], progress: &Progress) -> Vec<Vec<LocalProject>> {
    let threads = std::thread::available_parallelism()
        .map_or(1, |n| n.get())
        .clamp(1, MAX_SCAN_THREADS)
        .min(roots.len().max(1));
    let next = std::sync::atomic::AtomicUsize::new(0);
    let results: Vec<Mutex<Vec<LocalProject>>> = roots.iter().map(|_| Mutex::default()).collect();
    std::thread::scope(|scope| {
        for _ in 0..threads {
            scope.spawn(|| {
                loop {
                    let i = next.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                    let Some((root, depth)) = roots.get(i) else {
                        break;
                    };
                    let found = discovery::scan(root, *depth);
                    if let Ok(mut slot) = results[i].lock() {
                        *slot = found;
                    }
                    progress.advance(Phase::Scan);
                }
            });
        }
    });
    results
        .into_iter()
        .map(|slot| slot.into_inner().unwrap_or_default())
        .collect()
}

/// Folders to scan with their depth, the non-recursive watch targets, and the folders
/// editors opened (to tell which editor a clone belongs to).
fn scan_roots(
    folders: Vec<PathBuf>,
    clones: Vec<PathBuf>,
    editors: bool,
) -> (
    Vec<(PathBuf, usize)>,
    Vec<WatchTarget>,
    Vec<editors::Recent>,
) {
    let mut roots: Vec<(PathBuf, usize)> = folders
        .iter()
        .map(|folder| (folder.clone(), discovery::DEFAULT_MAX_DEPTH))
        .collect();
    // A clone the app made is a repo itself: only its own folder is read.
    roots.extend(
        clones
            .into_iter()
            .filter(|clone| !folders.iter().any(|f| clone.starts_with(f)))
            .map(|clone| (clone, 0)),
    );
    let mut targets = Vec::new();
    if !editors {
        return (roots, targets, Vec::new());
    }
    let found = editors::find();
    let recents: Vec<editors::Recent> = found.recent.iter().chain(&found.roots).cloned().collect();
    let home = crate::platform::home_dir();
    let usable =
        |dir: &Path| !folders.iter().any(|f| dir.starts_with(f)) && is_narrow(dir, home.as_deref());
    let parents: Vec<PathBuf> = found
        .recent
        .iter()
        .filter_map(|recent| recent.path.parent().map(Path::to_path_buf))
        .collect();
    let roots_found: Vec<PathBuf> = found.roots.into_iter().map(|root| root.path).collect();
    // A recent folder may be a course folder holding the clones; its siblings may be
    // clones the editor never opened.
    let candidates = found
        .recent
        .into_iter()
        .map(|recent| (recent.path, EDITOR_SCAN_DEPTH))
        .chain(parents.iter().cloned().map(|dir| (dir, 1)))
        .chain(
            roots_found
                .iter()
                .cloned()
                .map(|dir| (dir, EDITOR_SCAN_DEPTH)),
        );
    for (dir, depth) in candidates {
        if !usable(&dir) {
            continue;
        }
        match roots.iter_mut().find(|(root, _)| *root == dir) {
            Some((_, existing)) => *existing = (*existing).max(depth),
            None => roots.push((dir, depth)),
        }
    }
    targets.extend(found.files.into_iter().map(WatchTarget::flat));
    targets.extend(
        parents
            .into_iter()
            .chain(roots_found)
            .filter(|dir| usable(dir))
            .map(WatchTarget::flat),
    );
    (roots, targets, recents)
}

/// Deep enough for `~/IdeaProjects/<course>/<assignment>`.
const EDITOR_SCAN_DEPTH: usize = 2;

/// Whether walking `dir` stays clear of the home dir as a whole: on macOS, touching
/// `~/Documents`, `~/Desktop` or `~/Downloads` raises a permission prompt, and the home
/// dir is too big anyway. Filesystem roots are refused for the same reason.
fn is_narrow(dir: &Path, home: Option<&Path>) -> bool {
    dir.parent().is_some() && home.is_none_or(|home| !home.starts_with(dir))
}
