//! Settings > Developer, for dev builds only: simulated network, update, badge and links.

use tauri::AppHandle;

use crate::deeplink;
use crate::error::{Error, Result};
use crate::simulate;
use crate::updater;

/// Dev builds only: a fake update, from the check to the ready banner.
#[tauri::command]
#[cfg_attr(test, specta::specta)]
pub async fn simulate_update(app: AppHandle) -> Result<()> {
    updater::simulate(&app).await
}

/// Dev builds only: lights the tray badge after a few seconds, once the popover has
/// closed (closing it clears the badge). The next open and close clears it again.
#[tauri::command]
#[cfg_attr(test, specta::specta)]
pub async fn preview_badge(app: AppHandle) -> Result<()> {
    if !cfg!(debug_assertions) {
        return Err(Error::Unsupported(
            "the badge preview is for dev builds".into(),
        ));
    }
    tokio::time::sleep(std::time::Duration::from_secs(3)).await;
    crate::tray::set_badge(&app, true);
    Ok(())
}

/// Dev builds only: pretends GitHub is down or the device is offline, until set back.
#[tauri::command]
#[cfg_attr(test, specta::specta)]
pub fn simulate_network(network: simulate::Network) -> Result<()> {
    simulate::set_network(network)
}

#[tauri::command]
#[cfg_attr(test, specta::specta)]
pub fn simulated_network() -> simulate::Network {
    simulate::network()
}

/// Dev builds only: handles `url` as if the OS had opened it.
#[tauri::command]
#[cfg_attr(test, specta::specta)]
pub fn simulate_deep_link(app: AppHandle, url: String) -> Result<()> {
    deeplink::simulate(&app, &url)
}
