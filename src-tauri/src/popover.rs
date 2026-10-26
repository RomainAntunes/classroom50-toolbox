//! The main surface: a frameless window anchored to the tray icon that hides when it loses
//! focus, like a menu-bar popover.

use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use tauri::{
    AppHandle, LogicalPosition, Manager, Monitor, PhysicalPosition, WebviewWindow, WindowEvent,
};

use crate::events::{IpcEvent, PopoverHidden, PopoverShortcut, PopoverShown};
use crate::settings::{Appearance, SettingsStore};

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

/// Light or dark for the window, its material and the webview's `prefers-color-scheme`;
/// `System` follows the OS setting.
pub fn apply_appearance(app: &AppHandle, appearance: Appearance) {
    let Some(window) = app.get_webview_window(LABEL) else {
        return;
    };
    let theme = match appearance {
        Appearance::System => None,
        Appearance::Light => Some(tauri::Theme::Light),
        Appearance::Dark => Some(tauri::Theme::Dark),
    };
    if let Err(err) = window.set_theme(theme) {
        log::warn!("could not set the window theme: {err}");
    }
}

pub fn setup(app: &AppHandle) -> tauri::Result<()> {
    let Some(window) = app.get_webview_window(LABEL) else {
        return Ok(());
    };
    apply_material(&window);
    apply_appearance(app, app.state::<SettingsStore>().get().appearance);

    let handle = app.clone();
    window.on_window_event(move |event| {
        if let WindowEvent::Focused(false) = event
            && !crate::testenv::keep_open()
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

/// A tray click: `icon` is the clicked icon's own rect, which on macOS is the menu bar of the
/// screen clicked (`TrayIcon::rect` may answer with another screen's).
pub fn toggle(app: &AppHandle, icon: Option<Area>) {
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
        show_at(app, icon);
    }
}

/// The global shortcut: hides the popover when it has the keyboard, else shows it as a tray
/// click would and lets the UI focus the list. No reopen guard: a key press elsewhere
/// doesn't blur the popover first.
pub fn toggle_from_shortcut(app: &AppHandle) {
    let Some(window) = app.get_webview_window(LABEL) else {
        return;
    };
    if window.is_visible().unwrap_or(false) && window.is_focused().unwrap_or(false) {
        hide(app);
        return;
    }
    show(app);
    let _ = PopoverShortcut::emit(app, &());
}

/// Opens under the tray icon, wherever it is (the shortcut, the menu's Open, a link).
pub fn show(app: &AppHandle) {
    show_at(app, crate::tray::icon_rect(app));
}

fn show_at(app: &AppHandle, icon: Option<Area>) {
    let Some(window) = app.get_webview_window(LABEL) else {
        return;
    };
    let scale = window.scale_factor().unwrap_or(1.0);
    move_to(app, &window, icon);
    // Windows: a move onto a screen of another scale resizes the window around a point of
    // its own; placing it again puts it back under the icon.
    if SPACE == Space::Pixels && window.scale_factor().unwrap_or(scale) != scale {
        move_to(app, &window, icon);
    }
    let _ = window.show();
    let _ = window.set_focus();
    let _ = PopoverShown::emit(app, &());
    crate::projects::refresh_if_stale(app);
}

fn move_to(app: &AppHandle, window: &WebviewWindow, icon: Option<Area>) {
    let screens: Vec<Screen> = app
        .available_monitors()
        .unwrap_or_default()
        .iter()
        .map(Screen::from)
        .collect();
    let Ok(size) = window.outer_size() else {
        return;
    };
    let size = size.to_logical::<f64>(window.scale_factor().unwrap_or(1.0));
    let size = (size.width, size.height);
    let primary = app
        .primary_monitor()
        .ok()
        .flatten()
        .map(|m| Screen::from(&m));
    // tao gives the pointer in the primary screen's scale on macOS and Linux.
    let pointer = app.cursor_position().ok().map(|at| match SPACE {
        Space::Points => {
            let scale = primary.map_or(1.0, |screen| screen.scale);
            (at.x / scale, at.y / scale)
        }
        Space::Pixels => (at.x, at.y),
    });
    let spot = icon
        .and_then(|icon| place(icon, pointer, &screens, size, SPACE))
        .or_else(|| {
            // No icon position (Linux): a corner of the screen under the pointer.
            let screen = pointer
                .and_then(|at| {
                    screens
                        .iter()
                        .map(|screen| to_space(screen, SPACE))
                        .find(|screen| screen.bounds.contains(at))
                })
                .or_else(|| primary.map(|screen| to_space(&screen, SPACE)))?;
            Some(corner(&screen, size, SPACE))
        });
    let Some((x, y)) = spot else {
        return;
    };
    let _ = match SPACE {
        Space::Points => window.set_position(LogicalPosition::new(x, y)),
        Space::Pixels => window.set_position(PhysicalPosition::new(x.round(), y.round())),
    };
}

pub fn hide(app: &AppHandle) {
    if let Some(window) = app.get_webview_window(LABEL) {
        let _ = window.hide();
        let _ = PopoverHidden::emit(app, &());
        crate::alerts::mark_seen(app);
        if let Ok(mut at) = app.state::<PopoverState>().hidden_at.lock() {
            *at = Some(Instant::now());
        }
    }
}

/// A rect as tao and tray-icon report it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Area {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

impl Area {
    pub fn new(x: f64, y: f64, width: f64, height: f64) -> Self {
        Self {
            x,
            y,
            width,
            height,
        }
    }

    fn right(&self) -> f64 {
        self.x + self.width
    }

    fn bottom(&self) -> f64 {
        self.y + self.height
    }

    fn center(&self) -> (f64, f64) {
        (self.x + self.width / 2.0, self.y + self.height / 2.0)
    }

    fn contains(&self, (x, y): (f64, f64)) -> bool {
        x >= self.x && x < self.right() && y >= self.y && y < self.bottom()
    }

    fn scaled(&self, by: f64) -> Self {
        Self::new(self.x * by, self.y * by, self.width * by, self.height * by)
    }
}

impl From<tauri::Rect> for Area {
    fn from(rect: tauri::Rect) -> Self {
        // tray-icon already reports physical values, so the scale given here is unused.
        let position = rect.position.to_physical::<f64>(1.0);
        let size = rect.size.to_physical::<f64>(1.0);
        Self::new(position.x, position.y, size.width, size.height)
    }
}

/// A screen with its usable part (the menu bar, Dock or taskbar left out).
#[derive(Debug, Clone, Copy, PartialEq)]
struct Screen {
    bounds: Area,
    work: Area,
    scale: f64,
}

impl From<&Monitor> for Screen {
    fn from(monitor: &Monitor) -> Self {
        let work = monitor.work_area();
        Self {
            bounds: Area::new(
                monitor.position().x.into(),
                monitor.position().y.into(),
                monitor.size().width.into(),
                monitor.size().height.into(),
            ),
            work: Area::new(
                work.position.x.into(),
                work.position.y.into(),
                work.size.width.into(),
                work.size.height.into(),
            ),
            scale: monitor.scale_factor(),
        }
    }
}

/// The coordinates a window is placed in. macOS (AppKit points) and Linux (GTK's logical
/// pixels) report every value as logical units times the scale of the screen it lies on, so a
/// value only makes sense divided by that screen's own scale, and windows are placed in
/// logical units. Windows uses the virtual desktop's real pixels throughout.
#[derive(Debug, Clone, Copy, PartialEq)]
enum Space {
    #[cfg_attr(target_os = "windows", allow(dead_code))]
    Points,
    Pixels,
}

#[cfg(target_os = "windows")]
const SPACE: Space = Space::Pixels;
#[cfg(not(target_os = "windows"))]
const SPACE: Space = Space::Points;

/// Between the popover and the icon, the panel or a screen's edge, in logical units.
const GAP: f64 = 6.0;

/// A screen in `space`, its scale kept for sizes.
fn to_space(screen: &Screen, space: Space) -> Screen {
    match space {
        Space::Points => Screen {
            bounds: screen.bounds.scaled(1.0 / screen.scale),
            work: screen.work.scaled(1.0 / screen.scale),
            scale: screen.scale,
        },
        Space::Pixels => *screen,
    }
}

/// The screen holding `icon`'s center, and `icon` in that screen's terms. In `Points` a value
/// can fit two screens of different scales (3000 is 1500 points on a 2x screen, or 3000 on a
/// 1x one to its right); the one under the pointer wins, as a click is on the icon.
fn screen_of(
    icon: Area,
    pointer: Option<(f64, f64)>,
    screens: &[Screen],
    space: Space,
) -> Option<(Screen, Area)> {
    let fits: Vec<(Screen, Area)> = screens
        .iter()
        .filter_map(|screen| {
            let local = to_space(screen, space);
            let icon = match space {
                Space::Points => icon.scaled(1.0 / screen.scale),
                Space::Pixels => icon,
            };
            local
                .bounds
                .contains(icon.center())
                .then_some((local, icon))
        })
        .collect();
    pointer
        .and_then(|at| fits.iter().find(|(screen, _)| screen.bounds.contains(at)))
        .or(fits.first())
        .copied()
}

/// Where the popover's top-left corner goes for a tray icon at `icon`: on the icon's screen,
/// on the side away from the panel the icon sits in (below the macOS menu bar, above a
/// bottom taskbar, beside a side one), centered on the icon and inside the usable area.
/// `size` is the window's logical size, `pointer` in `space`. `None` when no screen holds the
/// icon.
fn place(
    icon: Area,
    pointer: Option<(f64, f64)>,
    screens: &[Screen],
    size: (f64, f64),
    space: Space,
) -> Option<(f64, f64)> {
    let (screen, icon) = screen_of(icon, pointer, screens, space)?;
    let unit = match space {
        Space::Points => 1.0,
        Space::Pixels => screen.scale,
    };
    let (width, height) = (size.0 * unit, size.1 * unit);
    let gap = GAP * unit;
    let (work, (center_x, center_y)) = (screen.work, icon.center());

    let (x, y) = if center_x < work.x {
        (icon.right().max(work.x) + gap, center_y - height / 2.0)
    } else if center_x >= work.right() {
        (
            icon.x.min(work.right()) - width - gap,
            center_y - height / 2.0,
        )
    } else if center_y < screen.bounds.center().1 {
        (center_x - width / 2.0, icon.bottom().max(work.y) + gap)
    } else {
        (
            center_x - width / 2.0,
            icon.y.min(work.bottom()) - height - gap,
        )
    };
    Some((
        clamp(x, work.x + gap, work.right() - width - gap),
        clamp(y, work.y + gap, work.bottom() - height - gap),
    ))
}

/// Without an icon position: the top-right corner on macOS, where the menu bar is, the
/// bottom-right one elsewhere. `screen` is already in `space`.
fn corner(screen: &Screen, size: (f64, f64), space: Space) -> (f64, f64) {
    let unit = match space {
        Space::Points => 1.0,
        Space::Pixels => screen.scale,
    };
    let (width, height) = (size.0 * unit, size.1 * unit);
    let (gap, work) = (GAP * unit, screen.work);
    let x = work.right() - width - gap;
    let y = if cfg!(target_os = "macos") {
        work.y + gap
    } else {
        work.bottom() - height - gap
    };
    (x.max(work.x), y.max(work.y))
}

/// `value` within `low..=high`, `low` winning when the window is bigger than the room.
fn clamp(value: f64, low: f64, high: f64) -> f64 {
    value.min(high).max(low)
}

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

#[cfg(test)]
mod tests {
    use super::*;

    const SIZE: (f64, f64) = (380.0, 560.0);

    /// A screen given in logical units, reported the way tao reports it in `space`.
    fn screen(space: Space, bounds: Area, work: Area, scale: f64) -> Screen {
        let by = match space {
            Space::Points => scale,
            Space::Pixels => 1.0,
        };
        Screen {
            bounds: bounds.scaled(by),
            work: work.scaled(by),
            scale,
        }
    }

    /// A 2x built-in screen at the origin, its menu bar 24 points high.
    fn built_in() -> Screen {
        screen(
            Space::Points,
            Area::new(0.0, 0.0, 1512.0, 982.0),
            Area::new(0.0, 24.0, 1512.0, 958.0),
            2.0,
        )
    }

    /// A 1x external screen at `x` points.
    fn external(x: f64) -> Screen {
        screen(
            Space::Points,
            Area::new(x, 0.0, 1920.0, 1080.0),
            Area::new(x, 25.0, 1920.0, 1055.0),
            1.0,
        )
    }

    #[test]
    fn opens_under_the_icon_of_a_1x_screen_next_to_a_2x_one() {
        let screens = [built_in(), external(1512.0)];
        // tray-icon reports the icon in the external screen's own scale: 1x.
        let icon = Area::new(3000.0, 0.0, 30.0, 25.0);
        // 3000 also reads as 1500 points on the built-in screen; the pointer settles it.
        let spot = place(icon, Some((3010.0, 10.0)), &screens, SIZE, Space::Points);
        assert_eq!(spot, Some((3015.0 - 190.0, 25.0 + GAP)));
    }

    #[test]
    fn opens_under_the_icon_of_the_2x_screen() {
        let screens = [external(1512.0), built_in()];
        let icon = Area::new(2000.0, 0.0, 60.0, 48.0); // 1000 points on the 2x screen
        let spot = place(icon, Some((1010.0, 10.0)), &screens, SIZE, Space::Points);
        assert_eq!(spot, Some((1015.0 - 190.0, 24.0 + GAP)));
    }

    #[test]
    fn opens_on_a_screen_left_of_the_main_one() {
        let screens = [built_in(), external(-1920.0)];
        let icon = Area::new(-500.0, 0.0, 30.0, 25.0);
        let spot = place(icon, Some((-490.0, 10.0)), &screens, SIZE, Space::Points);
        assert_eq!(spot, Some((-485.0 - 190.0, 25.0 + GAP)));
    }

    #[test]
    fn takes_the_first_fit_without_a_pointer() {
        let screens = [built_in(), external(1512.0)];
        let icon = Area::new(2000.0, 0.0, 60.0, 48.0);
        let spot = place(icon, None, &screens, SIZE, Space::Points);
        assert_eq!(spot, Some((1015.0 - 190.0, 24.0 + GAP)));
    }

    #[test]
    fn stays_inside_the_screen_near_its_edge() {
        let screens = [built_in()];
        let icon = Area::new(2980.0, 0.0, 40.0, 48.0); // 1490 points, near the right edge
        let spot = place(icon, None, &screens, SIZE, Space::Points);
        assert_eq!(spot, Some((1512.0 - 380.0 - GAP, 24.0 + GAP)));
    }

    #[test]
    fn opens_nowhere_for_an_icon_off_every_screen() {
        let icon = Area::new(9000.0, 9000.0, 30.0, 25.0);
        assert_eq!(place(icon, None, &[built_in()], SIZE, Space::Points), None);
    }

    /// A 150% Windows screen of 2560x1440 pixels with a 72 px taskbar on `edge`.
    fn windows(edge: &str) -> Screen {
        let bounds = Area::new(0.0, 0.0, 2560.0, 1440.0);
        let work = match edge {
            "bottom" => Area::new(0.0, 0.0, 2560.0, 1368.0),
            "top" => Area::new(0.0, 72.0, 2560.0, 1368.0),
            _ => Area::new(72.0, 0.0, 2488.0, 1440.0),
        };
        screen(Space::Pixels, bounds, work, 1.5)
    }

    #[test]
    fn opens_above_a_bottom_taskbar() {
        let icon = Area::new(1800.0, 1380.0, 48.0, 48.0);
        let spot = place(icon, None, &[windows("bottom")], SIZE, Space::Pixels);
        assert_eq!(spot, Some((1824.0 - 285.0, 1368.0 - 840.0 - 9.0)));
    }

    #[test]
    fn opens_below_a_top_taskbar() {
        let icon = Area::new(1800.0, 12.0, 48.0, 48.0);
        let spot = place(icon, None, &[windows("top")], SIZE, Space::Pixels);
        assert_eq!(spot, Some((1824.0 - 285.0, 72.0 + 9.0)));
    }

    #[test]
    fn opens_beside_a_left_taskbar() {
        let icon = Area::new(12.0, 1300.0, 48.0, 48.0);
        let spot = place(icon, None, &[windows("left")], SIZE, Space::Pixels);
        // Centered on the icon, then kept above the screen's bottom edge.
        assert_eq!(spot, Some((72.0 + 9.0, 1440.0 - 840.0 - 9.0)));
    }

    #[test]
    fn a_pixel_screen_on_the_right_of_another_scale_keeps_its_pixels() {
        let left = screen(
            Space::Pixels,
            Area::new(0.0, 0.0, 1920.0, 1080.0),
            Area::new(0.0, 0.0, 1920.0, 1032.0),
            1.0,
        );
        let screens = [left, windows("bottom").moved(1920.0)];
        let icon = Area::new(1920.0 + 1800.0, 1380.0, 48.0, 48.0);
        let spot = place(icon, None, &screens, SIZE, Space::Pixels);
        assert_eq!(spot, Some((1920.0 + 1824.0 - 285.0, 1368.0 - 840.0 - 9.0)));
    }

    impl Screen {
        fn moved(self, x: f64) -> Self {
            let shift = |area: Area| Area::new(area.x + x, area.y, area.width, area.height);
            Self {
                bounds: shift(self.bounds),
                work: shift(self.work),
                scale: self.scale,
            }
        }
    }

    #[test]
    fn the_corner_sits_inside_the_work_area() {
        let (x, y) = corner(&to_space(&built_in(), Space::Points), SIZE, Space::Points);
        assert_eq!(x, 1512.0 - 380.0 - GAP);
        if cfg!(target_os = "macos") {
            assert_eq!(y, 24.0 + GAP);
        } else {
            assert_eq!(y, 982.0 - 560.0 - GAP);
        }
    }
}
