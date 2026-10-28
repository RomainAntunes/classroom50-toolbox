//! The OS's 12- or 24-hour clock, which the UI's times follow whatever its language
//! (WKWebView's and WebKitGTK's `Intl` only know the language's default). Read once, when
//! the UI first asks at startup, then again each time the popover shows where that runs no
//! program (`platform::HOUR_CYCLE_IN_PROCESS`); a change goes out as `hour-cycle`. Never
//! polled.

use std::sync::Mutex;

use tauri::{AppHandle, Manager};

use crate::events::{HourCycleChanged, IpcEvent};
use crate::platform::{self, HourCycle};

/// The last reading: `None` until the first, then what the OS said (itself `None` when it
/// couldn't say).
#[derive(Default)]
pub struct ClockState(Mutex<Option<Option<HourCycle>>>);

impl ClockState {
    fn get(&self) -> Option<Option<HourCycle>> {
        self.0.lock().ok().and_then(|cached| *cached)
    }

    /// Keeps `cycle`, telling whether it differs from a previous reading.
    fn set(&self, cycle: Option<HourCycle>) -> bool {
        let Ok(mut cached) = self.0.lock() else {
            return false;
        };
        let changed = cached.is_some_and(|before| before != cycle);
        *cached = Some(cycle);
        changed
    }
}

/// The cached reading, read first if there is none yet.
pub async fn hour_cycle(app: &AppHandle) -> Option<HourCycle> {
    let state = app.state::<ClockState>();
    if let Some(cycle) = state.get() {
        return cycle;
    }
    let cycle = platform::hour_cycle().await;
    state.set(cycle);
    cycle
}

/// The popover showed: reads the clock again where that's in-process, and tells the UI
/// only when it changed (the user flipped the 24-hour switch while the app ran).
pub fn refresh(app: &AppHandle) {
    if !platform::HOUR_CYCLE_IN_PROCESS || app.state::<ClockState>().get().is_none() {
        return;
    }
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        let cycle = platform::hour_cycle().await;
        if app.state::<ClockState>().set(cycle) {
            log::info!("the OS clock changed to {cycle:?}");
            let _ = HourCycleChanged::emit(&app, &cycle);
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tells_only_a_change() {
        let state = ClockState::default();
        assert_eq!(state.get(), None);
        assert!(
            !state.set(Some(HourCycle::H12)),
            "the first reading is no change"
        );
        assert_eq!(state.get(), Some(Some(HourCycle::H12)));
        assert!(!state.set(Some(HourCycle::H12)));
        assert!(state.set(Some(HourCycle::H23)));
        assert!(state.set(None), "a clock the OS no longer reports");
        assert_eq!(state.get(), Some(None));
    }

    /// Whatever this machine's clock is, reading it works (macOS always says).
    #[tokio::test]
    async fn reads_this_machines_clock() {
        let cycle = platform::hour_cycle().await;
        println!("hour cycle: {cycle:?}");
        if cfg!(target_os = "macos") {
            assert!(cycle.is_some());
        }
    }
}
