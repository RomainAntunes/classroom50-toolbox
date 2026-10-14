//! The main surface: a frameless window anchored to the tray icon that hides when it loses
//! focus, like a menu-bar popover.

use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use tauri::{AppHandle, Manager, WebviewWindow, WindowEvent};
use tauri_plugin_positioner::{Position, WindowExt};

pub const LABEL: &str = "main";

/// Clicking the tray icon while the popover is open first blurs (hides) it, then delivers
/// the click; without this guard that click would immediately reopen it.
const REOPEN_GUARD: Duration = Duration::from_millis(250);

#[derive(Default)]
pub struct PopoverState {
    hidden_at: Mutex<Option<Instant>>,
    /// Set while a native dialog (folder picker, confirm) is open: it takes focus, and the
    /// popover must not hide underneath it.
    pinned: AtomicBool,
}

impl PopoverState {
    pub fn set_pinned(&self, pinned: bool) {
        self.pinned.store(pinned, Ordering::Relaxed);
    }
}

pub fn setup(app: &AppHandle) -> tauri::Result<()> {
    let Some(window) = app.get_webview_window(LABEL) else {
        return Ok(());
    };
    apply_material(&window);

    let handle = app.clone();
    window.on_window_event(move |event| {
        if let WindowEvent::Focused(false) = event
            && !handle
                .state::<PopoverState>()
                .pinned
                .load(Ordering::Relaxed)
        {
            hide(&handle);
        }
    });
    Ok(())
}

pub fn toggle(app: &AppHandle) {
    let Some(window) = app.get_webview_window(LABEL) else {
        return;
    };
    if window.is_visible().unwrap_or(false) {
        hide(app);
        return;
    }
    let recently_hidden = app
        .state::<PopoverState>()
        .hidden_at
        .lock()
        .ok()
        .and_then(|at| *at)
        .is_some_and(|at| at.elapsed() < REOPEN_GUARD);
    if !recently_hidden {
        show(app);
    }
}

pub fn show(app: &AppHandle) {
    let Some(window) = app.get_webview_window(LABEL) else {
        return;
    };
    // Linux trays don't report their position; fall back to the screen corner there or
    // before the first tray event.
    if window.move_window(TRAY_POSITION).is_err() {
        let _ = window.move_window(FALLBACK_POSITION);
    }
    let _ = window.show();
    let _ = window.set_focus();
}

pub fn hide(app: &AppHandle) {
    if let Some(window) = app.get_webview_window(LABEL) {
        let _ = window.hide();
        if let Ok(mut at) = app.state::<PopoverState>().hidden_at.lock() {
            *at = Some(Instant::now());
        }
    }
}

/// macOS: menu bar at the top, so open below the icon. Windows: taskbar at the bottom, so
/// open above it.
#[cfg(target_os = "macos")]
const TRAY_POSITION: Position = Position::TrayBottomCenter;
#[cfg(not(target_os = "macos"))]
const TRAY_POSITION: Position = Position::TrayCenter;

#[cfg(target_os = "macos")]
const FALLBACK_POSITION: Position = Position::TopRight;
#[cfg(not(target_os = "macos"))]
const FALLBACK_POSITION: Position = Position::BottomRight;

/// Native translucent backdrop: macOS popover vibrancy, Windows 11 Mica. Linux keeps the
/// opaque theme background from the CSS.
fn apply_material(window: &WebviewWindow) {
    #[cfg(target_os = "macos")]
    {
        use window_vibrancy::{NSVisualEffectMaterial, NSVisualEffectState, apply_vibrancy};
        if let Err(err) = apply_vibrancy(
            window,
            NSVisualEffectMaterial::Popover,
            Some(NSVisualEffectState::Active),
            Some(12.0),
        ) {
            log::warn!("vibrancy unavailable: {err}");
        }
    }
    #[cfg(target_os = "windows")]
    {
        if let Err(err) = window_vibrancy::apply_mica(window, None) {
            log::warn!("mica unavailable: {err}");
        }
    }
    #[cfg(target_os = "linux")]
    let _ = window;
}
