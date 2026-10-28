use std::sync::Mutex;

use serde::{Deserialize, Serialize};
use tauri::image::Image;
use tauri::menu::{IsMenuItem, Menu, MenuItem, PredefinedMenuItem, Submenu};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Wry};

use crate::events::{IpcEvent, TrayAction};
use crate::popover;
use crate::updater;

pub const ID: &str = "main";

/// The menu, computed and worded by the UI (`set_tray_menu`, `src/lib/tray.ts`): sent at
/// startup, after a sync and whenever a label or the language changes. Rust has no
/// catalogs, so dates come worded.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[cfg_attr(test, derive(specta::Type))]
#[serde(rename_all = "camelCase")]
pub struct TrayMenu {
    pub open: String,
    pub quit: String,
    pub refresh: String,
    pub settings: String,
    /// "Synced today at 2:32 PM", a disabled line; None before the first sync.
    pub synced: Option<String>,
    /// Rows with the nearest deadlines of unfinished work, nearest first.
    pub deadlines: Vec<TrayItem>,
    /// The submenu's title and the clones it opens, most recently active first.
    pub recent_title: String,
    pub recent: Vec<TrayItem>,
    /// "Restart to Update to X", while a downloaded update waits.
    pub update: Option<String>,
}

/// A row that leads to a project or an unaccepted assignment, by its list id.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[cfg_attr(test, derive(specta::Type))]
pub struct TrayItem {
    pub id: String,
    pub label: String,
}

impl Default for TrayMenu {
    /// Until the UI has sent its own (it loads at startup, hidden).
    fn default() -> Self {
        Self {
            open: "Open Classroom 50 Toolbox".into(),
            quit: "Quit".into(),
            refresh: "Refresh".into(),
            settings: "Settings\u{2026}".into(),
            synced: None,
            deadlines: Vec::new(),
            recent_title: "Open Recent".into(),
            recent: Vec::new(),
            update: None,
        }
    }
}

/// What a click on the menu asks the UI to do (`tray-action`); the rest stays in Rust.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[cfg_attr(test, derive(specta::Type))]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum Action {
    /// Settings, in the popover Rust just showed.
    Settings,
    /// A deadline's row, pinned in the list as a link's is.
    Focus { id: String },
    /// A recent clone, opened in its editor with no popover.
    Open { id: String },
    /// Restart now, as the update banner's button.
    InstallUpdate,
}

/// The menu last built, so an unchanged one isn't rebuilt (that closes it on some trays).
static BUILT: Mutex<Option<TrayMenu>> = Mutex::new(None);

/// One line of the menu, before it becomes a native item.
#[derive(Debug, PartialEq)]
enum Entry<'a> {
    Item {
        id: String,
        label: &'a str,
        enabled: bool,
    },
    Submenu {
        title: &'a str,
        items: Vec<Entry<'a>>,
    },
    Separator,
}

fn item(id: impl Into<String>, label: &str) -> Entry<'_> {
    Entry::Item {
        id: id.into(),
        label,
        enabled: true,
    }
}

/// The menu's lines: Open; the deadlines; Open Recent, Refresh, Synced and the update;
/// Settings and Quit. An empty section is left out, so separators are never doubled nor
/// last.
fn layout(menu: &TrayMenu) -> Vec<Entry<'_>> {
    let deadlines = menu
        .deadlines
        .iter()
        .map(|row| item(format!("deadline:{}", row.id), &row.label))
        .collect();
    let mut status = Vec::new();
    if !menu.recent.is_empty() {
        status.push(Entry::Submenu {
            title: &menu.recent_title,
            items: menu
                .recent
                .iter()
                .map(|row| item(format!("recent:{}", row.id), &row.label))
                .collect(),
        });
    }
    status.push(item("refresh", &menu.refresh));
    if let Some(synced) = &menu.synced {
        status.push(Entry::Item {
            id: "synced".into(),
            label: synced,
            enabled: false,
        });
    }
    if let Some(update) = &menu.update {
        status.push(item("update", update));
    }
    let sections = [
        vec![item("open", &menu.open)],
        deadlines,
        status,
        vec![item("settings", &menu.settings), item("quit", &menu.quit)],
    ];
    let mut entries = Vec::new();
    for section in sections.into_iter().filter(|section| !section.is_empty()) {
        if !entries.is_empty() {
            entries.push(Entry::Separator);
        }
        entries.extend(section);
    }
    entries
}

/// A menu item's id, read back.
#[derive(Debug, PartialEq)]
enum Click<'a> {
    Open,
    Quit,
    Refresh,
    Settings,
    Update,
    Deadline(&'a str),
    Recent(&'a str),
}

/// `deadline:<id>` and `recent:<id>` split on the first `:` only: an id may hold more
/// (a Windows path).
fn parse(id: &str) -> Option<Click<'_>> {
    Some(match id {
        "open" => Click::Open,
        "quit" => Click::Quit,
        "refresh" => Click::Refresh,
        "settings" => Click::Settings,
        "update" => Click::Update,
        _ => match id.split_once(':')? {
            ("deadline", rest) if !rest.is_empty() => Click::Deadline(rest),
            ("recent", rest) if !rest.is_empty() => Click::Recent(rest),
            _ => return None,
        },
    })
}

fn on_click(app: &AppHandle, id: &str) {
    let emit = |action: Action| {
        if let Err(err) = TrayAction::emit(app, &action) {
            log::warn!("tray action: {err}");
        }
    };
    match parse(id) {
        Some(Click::Open) => popover::show(app),
        Some(Click::Quit) => updater::quit(app),
        // As the list's button, cooldown included; the list follows through
        // `projects-updated` whether or not the popover is open.
        Some(Click::Refresh) => {
            let app = app.clone();
            tauri::async_runtime::spawn(async move {
                crate::commands::refresh_projects(app).await;
            });
        }
        Some(Click::Settings) => {
            popover::show(app);
            emit(Action::Settings);
        }
        Some(Click::Deadline(id)) => {
            popover::show(app);
            emit(Action::Focus { id: id.into() });
        }
        Some(Click::Recent(id)) => emit(Action::Open { id: id.into() }),
        Some(Click::Update) => emit(Action::InstallUpdate),
        None => log::warn!("tray menu: unknown item {id:?}"),
    }
}

fn build(app: &AppHandle, entries: &[Entry]) -> tauri::Result<Vec<Box<dyn IsMenuItem<Wry>>>> {
    let mut items: Vec<Box<dyn IsMenuItem<Wry>>> = Vec::new();
    for entry in entries {
        match entry {
            Entry::Item { id, label, enabled } => items.push(Box::new(MenuItem::with_id(
                app,
                id,
                label,
                *enabled,
                None::<&str>,
            )?)),
            Entry::Submenu { title, items: rows } => {
                let rows = build(app, rows)?;
                let refs: Vec<&dyn IsMenuItem<Wry>> = rows.iter().map(|row| row.as_ref()).collect();
                items.push(Box::new(Submenu::with_items(app, title, true, &refs)?));
            }
            Entry::Separator => items.push(Box::new(PredefinedMenuItem::separator(app)?)),
        }
    }
    Ok(items)
}

fn menu(app: &AppHandle, tray: &TrayMenu) -> tauri::Result<Menu<Wry>> {
    let items = build(app, &layout(tray))?;
    let refs: Vec<&dyn IsMenuItem<Wry>> = items.iter().map(|item| item.as_ref()).collect();
    Menu::with_items(app, &refs)
}

/// Rebuilds the menu the UI sent, unless it is the one already there.
pub fn set_menu(app: &AppHandle, next: TrayMenu) {
    let Some(tray) = app.tray_by_id(ID) else {
        return;
    };
    let mut built = BUILT
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    if built.as_ref() == Some(&next) {
        return;
    }
    match menu(app, &next).and_then(|menu| tray.set_menu(Some(menu))) {
        Ok(()) => *built = Some(next),
        Err(err) => log::warn!("tray menu: {err}"),
    }
}

pub fn setup(app: &AppHandle) -> tauri::Result<()> {
    let first = TrayMenu::default();
    let menu = menu(app, &first)?;

    let builder = TrayIconBuilder::with_id(ID)
        .icon(icon()?)
        .icon_as_template(cfg!(target_os = "macos"))
        .tooltip("Classroom 50 Toolbox")
        .menu(&menu)
        // Left click opens the popover; the menu stays on right click. Linux trays don't
        // emit click events at all, so the "Open" item is the way in there.
        .show_menu_on_left_click(false)
        .on_menu_event(|app, event| on_click(app, event.id().as_ref()))
        .on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                rect,
                ..
            } = event
            {
                popover::toggle(tray.app_handle(), Some(rect.into()));
            }
        });
    builder.build(app)?;
    *BUILT
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner()) = Some(first);
    Ok(())
}

/// Where the icon is, for opening without a click (the shortcut, the menu's Open). Linux
/// trays report no position.
pub fn icon_rect(app: &AppHandle) -> Option<popover::Area> {
    let rect = app.tray_by_id(ID)?.rect().ok()??;
    Some(rect.into())
}

/// Swaps the icon for one with a dot in the top-right corner, or back.
pub fn set_badge(app: &AppHandle, on: bool) {
    let Some(tray) = app.tray_by_id(ID) else {
        return;
    };
    let icon = if on { icon().map(badged) } else { icon() };
    match icon {
        Ok(icon) => {
            let _ = tray.set_icon(Some(icon));
            // `set_icon` resets the template flag on macOS.
            let _ = tray.set_icon_as_template(cfg!(target_os = "macos"));
        }
        Err(err) => log::warn!("tray icon: {err}"),
    }
}

/// Draws the badge: a disc in the top-right corner, cut out of the icon by a transparent
/// ring so it reads as separate. On macOS the template tints it like the icon; elsewhere
/// it is the system's usual alert red.
fn badged(icon: Image<'static>) -> Image<'static> {
    let (width, height) = (icon.width(), icon.height());
    let mut rgba = icon.rgba().to_vec();
    let size = width.min(height) as f32;
    let radius = size * 0.2;
    let ring = radius + size * 0.07;
    let (cx, cy) = (width as f32 - radius - 0.5, radius + 0.5);
    let color: [u8; 4] = if cfg!(target_os = "macos") {
        [0, 0, 0, 255]
    } else {
        [0xe8, 0x3b, 0x30, 255]
    };
    for y in 0..height {
        for x in 0..width {
            let distance = ((x as f32 + 0.5 - cx).powi(2) + (y as f32 + 0.5 - cy).powi(2)).sqrt();
            let i = ((y * width + x) * 4) as usize;
            if distance <= radius {
                rgba[i..i + 4].copy_from_slice(&color);
            } else if distance <= ring {
                rgba[i + 3] = 0;
            }
        }
    }
    Image::new_owned(rgba, width, height)
}

/// macOS uses a monochrome template image that the menu bar tints for light and dark mode.
/// Windows and Linux get the heavy-stroke coloured icon, which reads on both light and dark
/// taskbars. Both are generated by `scripts/build-icons.sh`.
fn icon() -> tauri::Result<Image<'static>> {
    if cfg!(target_os = "macos") {
        Image::from_bytes(include_bytes!("../icons/tray-template.png"))
    } else {
        Image::from_bytes(include_bytes!("../icons/tray-color.png"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row(id: &str, label: &str) -> TrayItem {
        TrayItem {
            id: id.into(),
            label: label.into(),
        }
    }

    /// The lines as text: ids, `-` for a separator, `[title: ...]` for a submenu and `(id)`
    /// for a disabled line.
    fn lines(menu: &TrayMenu) -> Vec<String> {
        fn text(entry: &Entry) -> String {
            match entry {
                Entry::Item { id, enabled, .. } if *enabled => id.clone(),
                Entry::Item { id, .. } => format!("({id})"),
                Entry::Submenu { title, items } => {
                    let items: Vec<String> = items.iter().map(text).collect();
                    format!("[{title}: {}]", items.join(" "))
                }
                Entry::Separator => "-".into(),
            }
        }
        layout(menu).iter().map(text).collect()
    }

    #[test]
    fn the_default_menu_has_no_empty_section() {
        assert_eq!(
            lines(&TrayMenu::default()),
            ["open", "-", "refresh", "-", "settings", "quit"]
        );
    }

    #[test]
    fn every_section_in_order() {
        let menu = TrayMenu {
            synced: Some("Synced today at 2:32 PM".into()),
            deadlines: vec![row("a/lab3", "Lab 3"), row("a/p2", "Project 2")],
            recent: vec![row("a/lab1", "Lab 1"), row("C:\\x", "Lab 2")],
            update: Some("Restart to Update to 0.14.0".into()),
            ..TrayMenu::default()
        };
        assert_eq!(
            lines(&menu),
            [
                "open",
                "-",
                "deadline:a/lab3",
                "deadline:a/p2",
                "-",
                "[Open Recent: recent:a/lab1 recent:C:\\x]",
                "refresh",
                "(synced)",
                "update",
                "-",
                "settings",
                "quit",
            ]
        );
    }

    #[test]
    fn rows_keep_their_labels() {
        let menu = TrayMenu {
            deadlines: vec![row("a/lab3", "Lab 3 · Due tomorrow")],
            ..TrayMenu::default()
        };
        assert_eq!(
            layout(&menu)[2],
            Entry::Item {
                id: "deadline:a/lab3".into(),
                label: "Lab 3 · Due tomorrow",
                enabled: true,
            }
        );
        assert!(
            matches!(layout(&TrayMenu::default())[4], Entry::Item { label, .. } if label == "Settings…")
        );
    }

    #[test]
    fn no_recent_clone_no_submenu() {
        let menu = TrayMenu {
            deadlines: vec![row("a/lab3", "Lab 3")],
            synced: Some("Synced yesterday at 9:00 PM".into()),
            ..TrayMenu::default()
        };
        assert_eq!(
            lines(&menu),
            [
                "open",
                "-",
                "deadline:a/lab3",
                "-",
                "refresh",
                "(synced)",
                "-",
                "settings",
                "quit",
            ]
        );
    }

    #[test]
    fn reads_item_ids_back() {
        assert_eq!(parse("open"), Some(Click::Open));
        assert_eq!(parse("quit"), Some(Click::Quit));
        assert_eq!(parse("refresh"), Some(Click::Refresh));
        assert_eq!(parse("settings"), Some(Click::Settings));
        assert_eq!(parse("update"), Some(Click::Update));
        assert_eq!(
            parse("deadline:acme/cs50-lab3-alex"),
            Some(Click::Deadline("acme/cs50-lab3-alex"))
        );
        // Only the first `:` splits: the rest is the id, a Windows path's drive included.
        assert_eq!(
            parse("recent:C:\\Users\\alex\\lab3"),
            Some(Click::Recent("C:\\Users\\alex\\lab3"))
        );
        assert_eq!(parse("deadline:a:b"), Some(Click::Deadline("a:b")));
    }

    #[test]
    fn ignores_other_ids() {
        for id in [
            "synced",
            "",
            "deadline:",
            "recent:",
            "other:x",
            "Open",
            "deadline",
        ] {
            assert_eq!(parse(id), None, "{id:?}");
        }
    }

    /// The UI gets `{ kind: "focus", id }`, the shape `TrayAction` in bindings.ts says.
    #[test]
    fn actions_carry_their_kind() {
        let json = |action: Action| serde_json::to_value(action).unwrap();
        assert_eq!(
            json(Action::Settings),
            serde_json::json!({ "kind": "settings" })
        );
        assert_eq!(
            json(Action::Focus { id: "a/b".into() }),
            serde_json::json!({ "kind": "focus", "id": "a/b" })
        );
        assert_eq!(
            json(Action::Open { id: "a/b".into() }),
            serde_json::json!({ "kind": "open", "id": "a/b" })
        );
        assert_eq!(
            json(Action::InstallUpdate),
            serde_json::json!({ "kind": "installUpdate" })
        );
    }

    #[test]
    fn reads_the_menu_the_ui_sends() {
        let menu: TrayMenu = serde_json::from_value(serde_json::json!({
            "open": "Open", "quit": "Quit", "refresh": "Refresh", "settings": "Settings…",
            "synced": null, "deadlines": [{ "id": "a/b", "label": "Lab" }],
            "recentTitle": "Open Recent", "recent": [], "update": null,
        }))
        .unwrap();
        assert_eq!(menu.deadlines, [row("a/b", "Lab")]);
        assert_eq!(menu.recent_title, "Open Recent");
    }
}
