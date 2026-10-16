//! The opt-in system-wide shortcut that shows or hides the popover from any app (the
//! `globalShortcut` setting). Registered from Rust through `tauri-plugin-global-shortcut`,
//! so it works while the webview is hidden; the plugin is only set up where it can work
//! (`platform::global_shortcuts_supported`: not in a Wayland session).

use std::sync::Mutex;

use tauri::{AppHandle, Manager, Wry};
use tauri_plugin_global_shortcut::{GlobalShortcut, ShortcutState};

use crate::error::{Error, Result};
use crate::platform;
use crate::settings::SettingsStore;

/// Modifiers in the order an accelerator stores them.
const MODIFIERS: [&str; 4] = ["Control", "Alt", "Shift", "Super"];

/// The keys a shortcut can end with, as `KeyboardEvent.code` names (which the plugin
/// parses too). Escape is left out (it cancels the recorder), and so are Tab, lock and
/// media keys, which the OS keeps for itself.
const KEYS: &[&str] = &[
    "KeyA",
    "KeyB",
    "KeyC",
    "KeyD",
    "KeyE",
    "KeyF",
    "KeyG",
    "KeyH",
    "KeyI",
    "KeyJ",
    "KeyK",
    "KeyL",
    "KeyM",
    "KeyN",
    "KeyO",
    "KeyP",
    "KeyQ",
    "KeyR",
    "KeyS",
    "KeyT",
    "KeyU",
    "KeyV",
    "KeyW",
    "KeyX",
    "KeyY",
    "KeyZ",
    "Digit0",
    "Digit1",
    "Digit2",
    "Digit3",
    "Digit4",
    "Digit5",
    "Digit6",
    "Digit7",
    "Digit8",
    "Digit9",
    "F1",
    "F2",
    "F3",
    "F4",
    "F5",
    "F6",
    "F7",
    "F8",
    "F9",
    "F10",
    "F11",
    "F12",
    "F13",
    "F14",
    "F15",
    "F16",
    "F17",
    "F18",
    "F19",
    "F20",
    "F21",
    "F22",
    "F23",
    "F24",
    "Space",
    "Enter",
    "Backspace",
    "Delete",
    "Insert",
    "Home",
    "End",
    "PageUp",
    "PageDown",
    "ArrowUp",
    "ArrowDown",
    "ArrowLeft",
    "ArrowRight",
    "Backquote",
    "Minus",
    "Equal",
    "BracketLeft",
    "BracketRight",
    "Backslash",
    "Semicolon",
    "Quote",
    "Comma",
    "Period",
    "Slash",
];

/// The accelerator in the one form the setting stores (`Control+Alt+KeyC`: modifiers in
/// `MODIFIERS` order, then a key of `KEYS`), or `None` when it isn't a shortcut the app
/// accepts: one key, each modifier once, and at least one modifier other than Shift (a
/// shortcut of Shift and a letter would take that capital letter from every app).
pub fn normalize(accelerator: &str) -> Option<String> {
    let mut tokens: Vec<&str> = accelerator.split('+').map(str::trim).collect();
    let key = tokens.pop()?;
    let key = *KEYS.iter().find(|k| k.eq_ignore_ascii_case(key))?;
    let mut held = [false; MODIFIERS.len()];
    for token in tokens {
        let index = match token.to_ascii_lowercase().as_str() {
            "control" | "ctrl" => 0,
            "alt" | "option" => 1,
            "shift" => 2,
            "super" | "cmd" | "command" | "meta" => 3,
            _ => return None,
        };
        if std::mem::replace(&mut held[index], true) {
            return None;
        }
    }
    if !(held[0] || held[1] || held[3]) {
        return None;
    }
    let mut parts: Vec<&str> = MODIFIERS
        .iter()
        .zip(held)
        .filter_map(|(name, on)| on.then_some(*name))
        .collect();
    parts.push(key);
    Some(parts.join("+"))
}

/// The accelerator registered right now, if any.
#[derive(Default)]
pub struct ShortcutRegistration(Mutex<Option<String>>);

impl ShortcutRegistration {
    pub fn active(&self) -> Option<String> {
        self.0.lock().ok().and_then(|active| active.clone())
    }
}

/// Whether the plugin is set up, so a shortcut can be registered at all.
pub fn available(app: &AppHandle) -> bool {
    app.try_state::<GlobalShortcut<Wry>>().is_some()
}

/// Sets the plugin up where it can work and registers the saved shortcut. A shortcut taken
/// since it was saved stays in the settings, unregistered; Settings says so.
pub fn setup(app: &AppHandle) {
    if !platform::global_shortcuts_supported() {
        return;
    }
    let plugin = tauri_plugin_global_shortcut::Builder::new()
        // Every backend reports one press and one release, without key repeat, so acting
        // on the press toggles once.
        .with_handler(|app, _shortcut, event| {
            if event.state == ShortcutState::Pressed {
                crate::popover::toggle_from_shortcut(app);
            }
        })
        .build();
    if let Err(err) = app.plugin(plugin) {
        log::warn!("global shortcuts unavailable: {err}");
        return;
    }
    let saved = app.state::<SettingsStore>().get().global_shortcut;
    if let Err(err) = apply(app, saved.as_deref()) {
        log::warn!("global shortcut not registered: {err}");
    }
}

/// Makes `next` the registered shortcut (`None`: none), unregistering the one before.
/// When `next` can't be registered, none is (the recorder resumes the saved one when it
/// closes, `pause`) and the error says why. Called on the main thread only (setup and sync commands): the plugin runs each
/// call there and waits for it, so another thread holding the lock meanwhile would block.
pub fn apply(app: &AppHandle, next: Option<&str>) -> Result<()> {
    let next = match next {
        Some(accelerator) => Some(normalize(accelerator).ok_or(Error::InvalidShortcut)?),
        None => None,
    };
    let registration = app.state::<ShortcutRegistration>();
    let mut active = registration
        .0
        .lock()
        .map_err(|_| Error::ShortcutUnsupported)?;
    if *active == next {
        return Ok(());
    }
    let Some(plugin) = app.try_state::<GlobalShortcut<Wry>>() else {
        // Nothing can be registered here; clearing is fine.
        return if next.is_some() {
            Err(Error::ShortcutUnsupported)
        } else {
            Ok(())
        };
    };
    if let Some(previous) = active.take()
        && let Err(err) = plugin.unregister(previous.as_str())
    {
        log::warn!("could not unregister the global shortcut: {err}");
    }
    let Some(next) = next else {
        return Ok(());
    };
    match plugin.register(next.as_str()) {
        Ok(()) => {
            *active = Some(next);
            Ok(())
        }
        Err(err) => {
            // The plugin flattens the OS's refusal into a message: taken by another app or
            // reserved by the system, which is what it means in practice.
            log::info!("global shortcut refused: {err}");
            Err(Error::ShortcutTaken)
        }
    }
}

/// While the recorder listens, the saved shortcut must reach it instead of toggling the
/// popover: unregistered on `true`, the saved one registered again on `false`.
pub fn pause(app: &AppHandle, paused: bool) -> Result<()> {
    if paused {
        apply(app, None)
    } else {
        let saved = app.state::<SettingsStore>().get().global_shortcut;
        apply(app, saved.as_deref())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalizes_accelerators() {
        let cases = [
            ("Control+Alt+KeyC", Some("Control+Alt+KeyC")),
            ("alt+ctrl+keyc", Some("Control+Alt+KeyC")),
            ("Cmd+Option+Space", Some("Alt+Super+Space")),
            ("Shift+Super+Digit1", Some("Shift+Super+Digit1")),
            (" Meta + F5 ", Some("Super+F5")),
            (
                "Control+Shift+Alt+Super+ArrowUp",
                Some("Control+Alt+Shift+Super+ArrowUp"),
            ),
            // Shift alone, no modifier, no key, an unknown or reserved key.
            ("Shift+KeyA", None),
            ("KeyA", None),
            ("F5", None),
            ("Control+Alt", None),
            ("Control+", None),
            ("", None),
            ("Control+Escape", None),
            ("Control+Tab", None),
            ("Control+KeyAA", None),
            // A modifier twice, two keys, a key before a modifier.
            ("Control+Ctrl+KeyA", None),
            ("Control+KeyA+KeyB", None),
            ("KeyA+Control", None),
            ("Hyper+KeyA", None),
        ];
        for (input, expected) in cases {
            assert_eq!(normalize(input).as_deref(), expected, "{input:?}");
        }
    }

    /// The recorder (`src/lib/shortcut.ts`) offers the same keys.
    #[test]
    fn keys_match_the_recorder() {
        assert_eq!(KEYS.len(), 26 + 10 + 24 + 9 + 4 + 11);
    }

    /// What `normalize` stores, the plugin can parse.
    #[test]
    fn every_key_parses_in_the_plugin() {
        for key in KEYS {
            let accelerator = normalize(&format!("Control+Alt+{key}")).unwrap();
            let parsed: std::result::Result<tauri_plugin_global_shortcut::Shortcut, _> =
                accelerator.parse();
            assert!(parsed.is_ok(), "{accelerator}");
        }
    }
}
