//! `classroom50-toolbox://` links.

use tauri::{AppHandle, State};

use crate::deeplink::{self, DeepLinkState};
use crate::error::Result;

/// The link the OS opened the app with, if one waits, and where the list says it leads.
#[tauri::command]
#[cfg_attr(test, specta::specta)]
pub fn take_deep_link(app: AppHandle) -> Option<deeplink::Target> {
    deeplink::take(&app)
}

/// The UI is done with the link it took; the next one may come.
#[tauri::command]
#[cfg_attr(test, specta::specta)]
pub fn deep_link_done(state: State<'_, DeepLinkState>) {
    state.finish();
}

/// Reads the manifest behind the `unlisted` link the student just agreed to open.
#[tauri::command]
#[cfg_attr(test, specta::specta)]
pub async fn fetch_linked_assignment(app: AppHandle) -> Result<Option<deeplink::Target>> {
    deeplink::fetch_unlisted(&app).await
}
