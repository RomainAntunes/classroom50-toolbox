//! Sync progress for the titlebar bar (`sync_progress::Tracker`).

use std::sync::Mutex;

use tauri::{AppHandle, Emitter, Manager};

use super::{Mode, PROGRESS_EVENT, ProjectsState, SyncProgress};
use crate::sync_progress::{Costs, Phase, Tracker};

/// Reports sync progress to the UI as `sync-progress`.
#[derive(Clone)]
pub(super) struct Progress {
    pub(super) app: AppHandle,
    tracker: std::sync::Arc<Mutex<Tracker>>,
    pub(super) foreground: bool,
    /// Reports nothing: for the preview built while the real sync goes on.
    quiet: bool,
}

impl Progress {
    pub(super) fn start(app: &AppHandle, mode: Mode, foreground: bool) -> Self {
        let last = app
            .state::<ProjectsState>()
            .stats()
            .phases
            .remove(mode.key())
            .unwrap_or_default();
        let progress = Self {
            app: app.clone(),
            tracker: std::sync::Arc::new(Mutex::new(Tracker::new(mode.phases(), &last))),
            foreground,
            quiet: false,
        };
        progress.emit(|p| {
            *p = SyncProgress {
                running: true,
                foreground,
                fraction: 0.0,
            }
        });
        progress
    }

    pub(super) fn quiet(app: &AppHandle) -> Self {
        Self {
            app: app.clone(),
            tracker: std::sync::Arc::new(Mutex::new(Tracker::new(&[], &Costs::new()))),
            foreground: false,
            quiet: true,
        }
    }

    pub(super) fn set_total(&self, phase: Phase, total: usize) {
        self.track(|t| t.set_total(phase, total));
    }

    pub(super) fn advance(&self, phase: Phase) {
        self.track(|t| t.advance(phase));
    }

    pub(super) fn finish_phase(&self, phase: Phase) {
        self.track(|t| t.finish(phase));
    }

    pub(super) fn skip(&self, phase: Phase) {
        self.track(|t| t.skip(phase));
    }

    /// Ends the run and keeps what each phase cost for the next one of this mode.
    pub(super) fn finish(&self, mode: Mode) {
        let state = self.app.state::<ProjectsState>();
        if let (Ok(tracker), Ok(mut stats)) = (self.tracker.lock(), state.stats.lock()) {
            let last = stats.phases.get(mode.key()).cloned().unwrap_or_default();
            stats
                .phases
                .insert(mode.key().to_owned(), tracker.costs(&last));
        }
        self.emit(|p| {
            p.running = false;
            p.fraction = 1.0;
        });
    }

    fn track(&self, change: impl FnOnce(&mut Tracker)) {
        if self.quiet {
            return;
        }
        let Ok(fraction) = self.tracker.lock().map(|mut tracker| {
            change(&mut tracker);
            tracker.fraction()
        }) else {
            return;
        };
        self.emit(|p| p.fraction = fraction);
    }

    fn emit(&self, change: impl FnOnce(&mut SyncProgress)) {
        let state = self.app.state::<ProjectsState>();
        let Ok(mut progress) = state.progress.lock() else {
            return;
        };
        change(&mut progress);
        if let Err(err) = self.app.emit(PROGRESS_EVENT, *progress) {
            log::debug!("could not emit {PROGRESS_EVENT}: {err}");
        }
    }
}
