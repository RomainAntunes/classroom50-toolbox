use std::sync::Mutex;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use tauri::image::Image;
use tauri::menu::{IsMenuItem, Menu, MenuItem, PredefinedMenuItem, Submenu};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Wry};

use crate::events::{IpcEvent, TrayAction, TrayExpired};
use crate::popover;
use crate::updater;

pub const ID: &str = "main";

/// The menu, computed and worded by the UI (`set_tray_menu`, `src/lib/tray.ts`): sent at
/// startup and whenever what it shows changes (a sync, the language, `tray-expired`). Rust
/// has no catalogs, so dates come worded.
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
    /// When a label or the selection would change (midnight, a deadline): the background
    /// tick then sends `tray-expired` (`check_expiry`). None when nothing depends on time.
    pub valid_until: Option<DateTime<Utc>>,
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
            valid_until: None,
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

/// The menu there now and the handles of its items, so a new one with the same structure
/// only gets new text (rebuilding closes an open menu on some trays, and costs more).
struct Built {
    menu: TrayMenu,
    /// What `labels` lists, in its order.
    natives: Vec<Native>,
}

static BUILT: Mutex<Option<Built>> = Mutex::new(None);

/// A built item whose text can change.
enum Native {
    Item(MenuItem<Wry>),
    Submenu(Submenu<Wry>),
}

impl Native {
    fn set_text(&self, text: &str) -> tauri::Result<()> {
        match self {
            Native::Item(item) => item.set_text(text),
            Native::Submenu(submenu) => submenu.set_text(text),
        }
    }
}

/// muda reads `&` as a mnemonic marker on every OS: macOS drops it, GTK and Windows
/// underline the next letter. `&&` shows one `&` ("R&D" stays "R&D").
fn escape(label: &str) -> String {
    label.replace('&', "&&")
}

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

/// What decides the native items, labels aside: each line's kind, id and enabled flag, a
/// submenu's rows included.
#[derive(Debug, PartialEq)]
enum Shape<'e> {
    Item { id: &'e str, enabled: bool },
    Submenu(Vec<Shape<'e>>),
    Separator,
}

fn shape<'e>(entries: &'e [Entry<'_>]) -> Vec<Shape<'e>> {
    entries
        .iter()
        .map(|entry| match entry {
            Entry::Item { id, enabled, .. } => Shape::Item {
                id,
                enabled: *enabled,
            },
            Entry::Submenu { items, .. } => Shape::Submenu(shape(items)),
            Entry::Separator => Shape::Separator,
        })
        .collect()
}

/// Every label in build order: the items, and a submenu's title after its rows.
fn labels<'a>(entries: &[Entry<'a>]) -> Vec<&'a str> {
    let mut out = Vec::new();
    for entry in entries {
        match entry {
            Entry::Item { label, .. } => out.push(*label),
            Entry::Submenu { title, items } => {
                out.extend(labels(items));
                out.push(*title);
            }
            Entry::Separator => {}
        }
    }
    out
}

/// What the menu there needs to become the next one.
#[derive(Debug, PartialEq)]
enum Change<'a> {
    /// Nothing native: the same lines (only `valid_until` may have moved).
    None,
    /// The same structure: these labels, by their place in `labels`, get new text.
    Relabel(Vec<(usize, &'a str)>),
    /// Lines came, went, moved or changed state.
    Rebuild,
}

fn change<'a>(built: &[Entry<'_>], next: &[Entry<'a>]) -> Change<'a> {
    if shape(built) != shape(next) {
        return Change::Rebuild;
    }
    let relabel: Vec<_> = labels(built)
        .into_iter()
        .zip(labels(next))
        .enumerate()
        .filter(|(_, (before, after))| before != after)
        .map(|(at, (_, after))| (at, after))
        .collect();
    if relabel.is_empty() {
        Change::None
    } else {
        Change::Relabel(relabel)
    }
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

/// The native items, and in `natives` the ones `labels` lists, in its order.
fn build(
    app: &AppHandle,
    entries: &[Entry],
    natives: &mut Vec<Native>,
) -> tauri::Result<Vec<Box<dyn IsMenuItem<Wry>>>> {
    let mut items: Vec<Box<dyn IsMenuItem<Wry>>> = Vec::new();
    for entry in entries {
        match entry {
            Entry::Item { id, label, enabled } => {
                let item = MenuItem::with_id(app, id, escape(label), *enabled, None::<&str>)?;
                natives.push(Native::Item(item.clone()));
                items.push(Box::new(item));
            }
            Entry::Submenu { title, items: rows } => {
                let rows = build(app, rows, natives)?;
                let refs: Vec<&dyn IsMenuItem<Wry>> = rows.iter().map(|row| row.as_ref()).collect();
                let submenu = Submenu::with_items(app, escape(title), true, &refs)?;
                natives.push(Native::Submenu(submenu.clone()));
                items.push(Box::new(submenu));
            }
            Entry::Separator => items.push(Box::new(PredefinedMenuItem::separator(app)?)),
        }
    }
    Ok(items)
}

fn menu(app: &AppHandle, tray: &TrayMenu) -> tauri::Result<(Menu<Wry>, Vec<Native>)> {
    let mut natives = Vec::new();
    let items = build(app, &layout(tray), &mut natives)?;
    let refs: Vec<&dyn IsMenuItem<Wry>> = items.iter().map(|item| item.as_ref()).collect();
    Ok((Menu::with_items(app, &refs)?, natives))
}

/// Takes the menu the UI sent: nothing native when only `valid_until` moved, new text when
/// only labels changed (a sync moves "Synced ..."), a rebuild when lines came or went.
pub fn set_menu(app: &AppHandle, next: TrayMenu) {
    let Some(tray) = app.tray_by_id(ID) else {
        return;
    };
    let mut built = BUILT
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    if let Some(current) = built.as_mut() {
        let kept = match change(&layout(&current.menu), &layout(&next)) {
            Change::None => true,
            Change::Relabel(labels) => labels.into_iter().all(|(at, label)| {
                current.natives[at]
                    .set_text(&escape(label))
                    .inspect_err(|err| log::warn!("tray menu text: {err}"))
                    .is_ok()
            }),
            Change::Rebuild => false,
        };
        if kept {
            current.menu = next;
            return;
        }
    }
    match menu(app, &next).and_then(|(menu, natives)| {
        tray.set_menu(Some(menu))?;
        Ok(natives)
    }) {
        Ok(natives) => {
            *built = Some(Built {
                menu: next,
                natives,
            })
        }
        Err(err) => log::warn!("tray menu: {err}"),
    }
}

/// From the background tick (`projects::schedule`, every 30 s, waking from sleep included):
/// once the menu's `valid_until` has passed, asks the UI for a new one (`tray-expired`),
/// once per menu. No timer of its own, so the hidden app wakes no more often than before.
pub fn check_expiry(app: &AppHandle) {
    let expired = BUILT
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .as_mut()
        .is_some_and(|built| take_expiry(&mut built.menu, Utc::now()));
    if expired && let Err(err) = TrayExpired::emit(app, &()) {
        log::warn!("tray expired: {err}");
    }
}

/// Whether the menu's `valid_until` has passed; true once only, until the UI sends another.
fn take_expiry(menu: &mut TrayMenu, now: DateTime<Utc>) -> bool {
    let expired = menu.valid_until.is_some_and(|at| now >= at);
    if expired {
        menu.valid_until = None;
    }
    expired
}

pub fn setup(app: &AppHandle) -> tauri::Result<()> {
    let first = TrayMenu::default();
    let (menu, natives) = menu(app, &first)?;

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
        .unwrap_or_else(|poisoned| poisoned.into_inner()) = Some(Built {
        menu: first,
        natives,
    });
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
            "validUntil": "2026-10-28T23:00:00.000Z",
        }))
        .unwrap();
        assert_eq!(menu.deadlines, [row("a/b", "Lab")]);
        assert_eq!(menu.recent_title, "Open Recent");
        assert_eq!(
            menu.valid_until,
            Some("2026-10-28T23:00:00Z".parse().unwrap())
        );
    }

    #[test]
    fn a_label_keeps_its_ampersand() {
        assert_eq!(escape("R&D"), "R&&D");
        assert_eq!(escape("Lab 3 && more"), "Lab 3 &&&& more");
        assert_eq!(escape("Lab 3"), "Lab 3");
    }

    fn synced(label: &str) -> TrayMenu {
        TrayMenu {
            synced: Some(label.into()),
            deadlines: vec![row("a/lab3", "Lab 3 · Due tomorrow")],
            recent: vec![row("a/lab1", "Lab 1")],
            ..TrayMenu::default()
        }
    }

    #[test]
    fn the_same_menu_changes_nothing() {
        let (before, after) = (
            synced("Synced today at 2:32 PM"),
            synced("Synced today at 2:32 PM"),
        );
        assert_eq!(change(&layout(&before), &layout(&after)), Change::None);
        // Only when it expires moved: nothing native either.
        let later = TrayMenu {
            valid_until: Some("2026-10-28T23:00:00Z".parse().unwrap()),
            ..after
        };
        assert_eq!(change(&layout(&before), &layout(&later)), Change::None);
    }

    #[test]
    fn new_words_are_set_in_place() {
        let before = synced("Synced today at 2:32 PM");
        let after = synced("Synced today at 2:58 PM");
        // Labels in build order: open, the deadline, Lab 1 then its submenu's title,
        // refresh, synced, settings, quit.
        assert_eq!(
            labels(&layout(&after)),
            [
                "Open Classroom 50 Toolbox",
                "Lab 3 · Due tomorrow",
                "Lab 1",
                "Open Recent",
                "Refresh",
                "Synced today at 2:58 PM",
                "Settings…",
                "Quit",
            ]
        );
        assert_eq!(
            change(&layout(&before), &layout(&after)),
            Change::Relabel(vec![(5, "Synced today at 2:58 PM")])
        );
        let french = TrayMenu {
            recent_title: "Ouvrir un élément récent".into(),
            deadlines: vec![row("a/lab3", "Lab 3 · À rendre demain")],
            ..before.clone()
        };
        assert_eq!(
            change(&layout(&before), &layout(&french)),
            Change::Relabel(vec![
                (1, "Lab 3 · À rendre demain"),
                (3, "Ouvrir un élément récent")
            ])
        );
    }

    #[test]
    fn other_lines_rebuild() {
        let before = synced("Synced today at 2:32 PM");
        let rebuilt =
            |after: TrayMenu| change(&layout(&before), &layout(&after)) == Change::Rebuild;
        // A line comes or goes.
        assert!(rebuilt(TrayMenu {
            update: Some("Restart to Update to 0.14.0".into()),
            ..before.clone()
        }));
        assert!(rebuilt(TrayMenu {
            synced: None,
            ..before.clone()
        }));
        // Another row, or the same rows in another order.
        assert!(rebuilt(TrayMenu {
            deadlines: vec![row("a/p2", "Lab 3 · Due tomorrow")],
            ..before.clone()
        }));
        assert!(rebuilt(TrayMenu {
            recent: vec![row("a/lab1", "Lab 1"), row("a/lab2", "Lab 2")],
            ..before.clone()
        }));
        let two = TrayMenu {
            recent: vec![row("a/lab1", "Lab 1"), row("a/lab2", "Lab 2")],
            ..before.clone()
        };
        let swapped = TrayMenu {
            recent: vec![row("a/lab2", "Lab 2"), row("a/lab1", "Lab 1")],
            ..before.clone()
        };
        assert_eq!(change(&layout(&two), &layout(&swapped)), Change::Rebuild);
        // A disabled line is another item than an enabled one.
        let items = [item("a", "A")];
        let disabled = [Entry::Item {
            id: "a".into(),
            label: "A",
            enabled: false,
        }];
        assert_eq!(change(&items, &disabled), Change::Rebuild);
    }

    #[test]
    fn expires_once_per_menu() {
        let at = |text: &str| text.parse::<DateTime<Utc>>().unwrap();
        let mut menu = TrayMenu {
            valid_until: Some(at("2026-10-28T23:00:00Z")),
            ..TrayMenu::default()
        };
        assert!(!take_expiry(&mut menu, at("2026-10-28T22:59:59Z")));
        assert!(take_expiry(&mut menu, at("2026-10-28T23:00:00Z")));
        // Asked once: the next tick waits for the menu the UI sends then.
        assert!(!take_expiry(&mut menu, at("2026-10-28T23:00:30Z")));
        // Woken the next morning: one ask, not one per deadline missed.
        let mut menu = TrayMenu {
            valid_until: Some(at("2026-10-28T23:00:00Z")),
            ..TrayMenu::default()
        };
        assert!(take_expiry(&mut menu, at("2026-10-29T07:00:00Z")));
        assert!(!take_expiry(
            &mut TrayMenu::default(),
            at("2026-10-29T07:00:00Z")
        ));
    }
}
