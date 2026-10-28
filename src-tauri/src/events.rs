//! Every event Rust sends the UI, named once with its payload type. Emitting goes through
//! `IpcEvent::emit`, so a payload can't drift from what `src/lib/bindings.ts` says the UI
//! receives: the `bindings` test writes the same list there as the `Events` type.

use serde::Serialize;
use tauri::{Emitter, Runtime};

use crate::alerts::Notice;
use crate::login::DeviceCode;
use crate::platform::HourCycle;
use crate::projects::{Snapshot, SyncProgress};
use crate::settings::Settings;
use crate::tools::ToolJob;
use crate::tray::Action;
use crate::updater::UpdateStatus;

/// An event the UI listens to: its name and what it carries.
pub trait IpcEvent {
    const NAME: &'static str;
    type Payload: Serialize;

    /// Sends the event to every webview.
    fn emit<R: Runtime>(app: &impl Emitter<R>, payload: &Self::Payload) -> tauri::Result<()> {
        app.emit(Self::NAME, payload)
    }
}

macro_rules! events {
    ($($(#[doc = $doc:literal])* $marker:ident = $name:literal => $payload:ty;)*) => {
        $(
            $(#[doc = $doc])*
            pub struct $marker;

            impl IpcEvent for $marker {
                const NAME: &'static str = $name;
                type Payload = $payload;
            }
        )*

        /// The payload of each event, by name: `Events` in `src/lib/bindings.ts`.
        #[cfg(test)]
        #[derive(Serialize, specta::Type)]
        #[allow(non_snake_case, dead_code)]
        pub struct Events {
            $(
                $(#[doc = $doc])*
                #[serde(rename = $name)]
                pub $marker: $payload,
            )*
        }
    };
}

events! {
    /// A new project list, after a sync, a preview or a rescan.
    ProjectsUpdated = "projects-updated" => Snapshot;
    /// The titlebar bar of the sync running.
    SyncProgressed = "sync-progress" => SyncProgress;
    /// Every install or update running, sent when one starts, prints a line or ends.
    ToolJobsChanged = "tool-jobs" => Vec<ToolJob>;
    /// Self-update changed state.
    UpdateStateChanged = "update-state" => UpdateStatus;
    /// Something worth a notification; the UI words it and sends it.
    Notify = "notify" => Notice;
    /// Ids of projects and unaccepted assignments with unseen news.
    UnseenUpdated = "unseen-updated" => Vec<String>;
    /// Settings Rust changed by itself (hidden ids moved by a rename).
    SettingsUpdated = "settings-updated" => Settings;
    /// The one-time code of the sign-in running.
    LoginDeviceCode = "login-device-code" => DeviceCode;
    /// A sign-in ended (success, failure or cancel), so every view resets.
    LoginEnded = "login-ended" => ();
    /// A `classroom50-toolbox://` link is waiting; `take_deep_link` reads it.
    DeepLinkWaiting = "deep-link" => ();
    /// The popover hid: the UI drops what it only showed for the moment.
    PopoverHidden = "popover-hidden" => ();
    /// The popover showed: the UI refreshes what it paused while hidden.
    PopoverShown = "popover-shown" => ();
    /// The global shortcut showed the popover: the list takes the keyboard focus.
    PopoverShortcut = "popover-shortcut" => ();
    /// A tray menu item that the UI carries out (Settings, a deadline, a recent clone,
    /// Restart to Update).
    TrayAction = "tray-action" => Action;
    /// The tray menu's `validUntil` passed (midnight, a deadline): the UI words it again.
    TrayExpired = "tray-expired" => ();
    /// The OS's 12- or 24-hour clock changed (`system_hour_cycle` gives the first reading).
    HourCycleChanged = "hour-cycle" => Option<HourCycle>;
}
