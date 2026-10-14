pub mod classroom50;
mod commands;
pub mod discovery;
pub mod error;
pub mod platform;
mod popover;
pub mod process;
pub mod tools;
mod tray;
mod updater;

use tauri::Manager;
use tauri_plugin_autostart::MacosLauncher;

const USER_AGENT: &str = concat!("classroom50-toolbox/", env!("CARGO_PKG_VERSION"));

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        // Must be registered first: a second launch just reopens the running instance.
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
            popover::show(app);
        }))
        .plugin(
            tauri_plugin_log::Builder::new()
                .level(log::LevelFilter::Info)
                .build(),
        )
        .plugin(tauri_plugin_positioner::init())
        .plugin(tauri_plugin_autostart::init(
            MacosLauncher::LaunchAgent,
            None,
        ))
        .plugin(tauri_plugin_notification::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .plugin(tauri_plugin_process::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
        .manage(popover::PopoverState::default())
        .manage(commands::Http(
            reqwest::Client::builder()
                .user_agent(USER_AGENT)
                .build()
                .expect("HTTP client"),
        ))
        .setup(|app| {
            // Tray-only app: no Dock icon or app switcher entry on macOS.
            #[cfg(target_os = "macos")]
            app.set_activation_policy(tauri::ActivationPolicy::Accessory);

            let handle = app.handle();
            popover::setup(handle)?;
            tray::setup(handle)?;
            if !cfg!(debug_assertions) {
                updater::check_in_background(handle.clone());
            }
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::app_info,
            commands::tool_statuses,
            commands::install_command,
            commands::install_tool,
            commands::auth_status,
            commands::scan_folders,
            commands::fetch_assignments,
            commands::open_terminal,
            commands::check_update,
            commands::install_update,
        ])
        .build(tauri::generate_context!())
        .expect("error while building tauri application")
        .run(|app, event| {
            // Closing the popover must not quit a tray app; only the tray's Quit does.
            if let tauri::RunEvent::ExitRequested {
                code: None, api, ..
            } = event
            {
                api.prevent_exit();
                let _ = app.get_webview_window(popover::LABEL).map(|w| w.hide());
            }
        });
}
