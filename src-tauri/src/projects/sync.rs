//! Running a sync: one at a time, its result broadcast as `projects-updated`, saved, and
//! passed on to the watcher, alerts and calendar.

use std::path::PathBuf;
use std::time::Duration;

use chrono::Utc;
use tauri::{AppHandle, Manager};

use super::{
    LastRun, Progress, ProjectsState, Snapshot, annotate, commit_emails, full_snapshot,
    local_snapshot, may_call_github, scan_local, sync_remote, watch_targets,
};
use crate::clones::CloneStore;
use crate::events::{IpcEvent, ProjectsUpdated};
use crate::github::{self, Http};
use crate::settings::{Settings, SettingsStore};
use crate::status::Grading;
use crate::sync_progress::Phase;
use crate::tools;
use crate::watch::{self, WatchTarget};

/// Runs a full sync for the user and broadcasts the result. Concurrent callers wait for
/// the sync in flight and get its result instead of starting another.
pub async fn refresh(app: &AppHandle) -> Snapshot {
    let state = app.state::<ProjectsState>();
    let Ok(guard) = state.sync_lock.try_lock() else {
        drop(state.sync_lock.lock().await);
        return state.snapshot();
    };
    publish(app, Mode::Full, true, guard).await
}

/// Runs a full sync of its own, after any sync in flight: for a change the app just made on
/// GitHub (accept, adding a teammate), which a sync started earlier may not have seen.
pub async fn refresh_after_write(app: &AppHandle) -> Snapshot {
    let state = app.state::<ProjectsState>();
    let guard = state.sync_lock.lock().await;
    publish(app, Mode::Full, true, guard).await
}

/// Re-reads only the disk (after a clone, commit or settings change) and keeps the last
/// GitHub data, so it costs no API request.
pub async fn refresh_local(app: &AppHandle, foreground: bool) -> Snapshot {
    let state = app.state::<ProjectsState>();
    let guard = state.sync_lock.lock().await;
    publish(app, Mode::Local, foreground, guard).await
}

/// Background syncs skip their turn when another one is running.
pub(super) async fn refresh_in_background(app: &AppHandle, mode: Mode) {
    let state = app.state::<ProjectsState>();
    let Ok(guard) = state.sync_lock.try_lock() else {
        return;
    };
    publish(app, mode, false, guard).await;
}

/// A push or pull just changed a clone's remote refs: re-read the status now, then twice
/// more while the autograder runs, then every minute while a run is still going, so the
/// new state and grade show up quickly.
pub async fn refresh_after_push(app: &AppHandle) {
    let state = app.state::<ProjectsState>();
    // An editor's auto-fetch can fire this often; one round of checks at a time.
    if state
        .after_push
        .swap(true, std::sync::atomic::Ordering::AcqRel)
    {
        return;
    }
    let started = tokio::time::Instant::now();
    let mut delays = AFTER_PUSH_CHECKS.into_iter();
    loop {
        let delay = match delays.next() {
            Some(delay) => delay,
            None if grading_running(&state.snapshot())
                && started.elapsed() < AFTER_PUSH_GRADING_LIMIT =>
            {
                AFTER_PUSH_GRADING_INTERVAL
            }
            None => break,
        };
        tokio::time::sleep(delay).await;
        if !may_call_github(app) {
            break;
        }
        let guard = state.sync_lock.lock().await;
        publish(app, Mode::Status, false, guard).await;
    }
    state
        .after_push
        .store(false, std::sync::atomic::Ordering::Release);
}

/// Delays between the status checks that follow a push; grading takes a minute or two.
const AFTER_PUSH_CHECKS: [Duration; 3] = [
    Duration::ZERO,
    Duration::from_secs(60),
    Duration::from_secs(120),
];

/// Past the checks above, how often to look again while an autograder is still running,
/// and for how long at most (a stuck run is left to the regular cadence).
const AFTER_PUSH_GRADING_INTERVAL: Duration = Duration::from_secs(60);
const AFTER_PUSH_GRADING_LIMIT: Duration = Duration::from_secs(15 * 60);

fn grading_running(snapshot: &Snapshot) -> bool {
    snapshot.projects.iter().any(|p| {
        p.remote
            .as_ref()
            .is_some_and(|r| matches!(r.grading, Some(Grading::Running { .. })))
    })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Mode {
    /// GitHub discovery, assignment status, local scan and manifests.
    Full,
    /// Assignment status and local scan, on top of the last discovery.
    Status,
    /// Local scan only, on top of the previous snapshot's GitHub data.
    Local,
}

impl Mode {
    pub(super) fn key(self) -> &'static str {
        match self {
            Mode::Full => "full",
            Mode::Status => "status",
            Mode::Local => "local",
        }
    }

    pub(super) fn phases(self) -> &'static [Phase] {
        use Phase::*;
        match self {
            Mode::Full => &[Listing, Details, Scan, Manifests, Statuses, Clones],
            Mode::Status => &[Scan, Manifests, Statuses, Clones],
            Mode::Local => &[Scan, Manifests, Clones],
        }
    }
}

async fn publish(
    app: &AppHandle,
    mode: Mode,
    foreground: bool,
    _guard: tokio::sync::MutexGuard<'_, ()>,
) -> Snapshot {
    let state = app.state::<ProjectsState>();
    let http = &app.state::<Http>().0;
    let settings = app.state::<SettingsStore>().get();

    // After `gh auth switch` the cached repos belong to the other account: start over.
    let mut mode = mode;
    if mode == Mode::Status && tools::active_login().await != state.remote().login {
        log::info!("GitHub account changed; running a full sync");
        mode = Mode::Full;
    }

    let progress = Progress::start(app, mode, foreground);
    let started = std::time::Instant::now();
    let clones = app.state::<CloneStore>().list();
    let (snapshot, targets) = sync(&state, http, &settings, clones, mode, &progress).await;
    progress.finish(mode);
    let run = LastRun {
        at: Utc::now(),
        seconds: started.elapsed().as_secs_f32(),
    };
    log::info!(
        "{mode:?} sync: {} projects in {:.1}s",
        snapshot.projects.len(),
        run.seconds
    );
    if let Ok(mut stats) = state.stats.lock() {
        match mode {
            Mode::Full => stats.full = Some(run),
            Mode::Status => stats.status = Some(run),
            Mode::Local => stats.local = Some(run),
        }
        stats.watch_targets = targets.len();
        if let Some(rate) = github::rate_limit() {
            stats.rate_limit = Some(rate);
        }
    }
    if let Ok(mut current) = state.snapshot.lock() {
        *current = snapshot.clone();
    }
    if let Err(err) = ProjectsUpdated::emit(app, &snapshot) {
        log::warn!("could not emit {}: {err}", ProjectsUpdated::NAME);
    }
    state.save_state();
    watch::rewatch(app, targets);
    crate::alerts::process(app, &snapshot);
    crate::calendar::sync(app);
    snapshot
}

/// Returns the snapshot and what to watch for local changes. A sync the user waits on
/// first shows the clones as found on disk with the last GitHub data, so the list doesn't
/// wait on GitHub (or on a connection that times out).
async fn sync(
    state: &ProjectsState,
    http: &reqwest::Client,
    settings: &Settings,
    clones: Vec<PathBuf>,
    mode: Mode,
    progress: &Progress,
) -> (Snapshot, Vec<WatchTarget>) {
    let now = Utc::now();
    let preview = mode != Mode::Local && (progress.foreground || state.is_offline());
    let scan = async {
        let scanned = scan_local(
            settings.watched_folders.clone(),
            clones,
            settings.scan_editor_projects,
            progress.clone(),
        )
        .await;
        if preview {
            let (locals, _, recents) = &scanned;
            let quiet = Progress::quiet(&progress.app);
            let mut snapshot =
                local_snapshot(state, http, locals.clone(), now, false, false, &quiet).await;
            annotate(state, &mut snapshot, recents);
            if let Err(err) = ProjectsUpdated::emit(&progress.app, &snapshot) {
                log::warn!("could not emit {}: {err}", ProjectsUpdated::NAME);
            }
        }
        scanned
    };
    // Offline, the manifests come from the cache rather than a request that times out.
    let network = !state.is_offline();
    let (snapshot, scan_targets, recents) = if mode != Mode::Full {
        let (locals, targets, recents) = scan.await;
        let status = mode == Mode::Status;
        (
            local_snapshot(state, http, locals, now, status, network, progress).await,
            targets,
            recents,
        )
    } else {
        let (remote, (locals, targets, recents)) =
            tokio::join!(sync_remote(state, http, progress), scan);
        (
            full_snapshot(state, http, remote, locals, now, progress).await,
            targets,
            recents,
        )
    };
    let mut snapshot = snapshot;
    annotate(state, &mut snapshot, &recents);
    commit_emails(&mut snapshot.projects).await;
    let targets = watch_targets(&settings.watched_folders, scan_targets, &snapshot.projects);
    (snapshot, targets)
}
