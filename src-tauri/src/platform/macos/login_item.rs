//! Launch at login through `SMAppService.mainApp` (macOS 13+), which System Settings lists
//! under the app's name and icon. The autostart plugin's Launch Agent points at the
//! executable inside the bundle, which an app without a Developer ID shows as a bare
//! binary from an unidentified developer.

use objc2::available;
use objc2_service_management::{SMAppService, SMAppServiceStatus};

use crate::error::{Error, Result};

fn supported() -> bool {
    available!(macos = 13.0)
}

/// Registers or removes the app as a login item. `None` before macOS 13, where the
/// plugin's Launch Agent stays.
pub fn set_login_item(enabled: bool) -> Option<Result<()>> {
    if !supported() {
        return None;
    }
    let service = unsafe { SMAppService::mainAppService() };
    let status = unsafe { service.status() };
    let result = if enabled {
        unsafe { service.registerAndReturnError() }.map(|()| {
            // Turned off in System Settings before: only the user can turn it back on there.
            if unsafe { service.status() } == SMAppServiceStatus::RequiresApproval {
                unsafe { SMAppService::openSystemSettingsLoginItems() };
            }
        })
    } else if status == SMAppServiceStatus::NotRegistered || status == SMAppServiceStatus::NotFound
    {
        Ok(())
    } else {
        unsafe { service.unregisterAndReturnError() }
    };
    Some(result.map_err(|err| {
        Error::Autostart(tauri_plugin_autostart::Error::Anyhow(
            err.localizedDescription().to_string(),
        ))
    }))
}

/// Whether the app opens at login, `None` before macOS 13.
pub fn login_item_enabled() -> Option<bool> {
    supported()
        .then(|| unsafe { SMAppService::mainAppService().status() } == SMAppServiceStatus::Enabled)
}
