pub mod accounts;
pub mod alerts;
pub mod classroom50;
mod commands;
pub mod discovery;
pub mod editors;
pub mod error;
pub mod github;
mod login;
pub mod platform;
mod popover;
pub mod process;
pub mod projects;
pub mod settings;
pub mod status;
pub mod tools;
mod tray;
mod updater;
mod watch;

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
        .manage(login::LoginState::default())
        .manage(watch::FolderWatcher::default())
        .manage(github::Http(
            reqwest::Client::builder()
                .user_agent(USER_AGENT)
                // Fail fast when offline instead of hanging a sync.
                .connect_timeout(std::time::Duration::from_secs(10))
                .timeout(std::time::Duration::from_secs(60))
                .build()
                .expect("HTTP client"),
        ))
        .setup(|app| {
            // Tray-only app: no Dock icon or app switcher entry on macOS.
            #[cfg(target_os = "macos")]
            app.set_activation_policy(tauri::ActivationPolicy::Accessory);

            let handle = app.handle();
            let (settings, first_run) =
                settings::SettingsStore::load(&app.path().app_config_dir()?);
            if first_run {
                let defaults = settings.get();
                if let Err(err) = commands::apply_launch_at_login(handle, defaults.launch_at_login)
                    .and_then(|()| settings.set(defaults).map(drop))
                {
                    log::warn!("first-run setup failed: {err}");
                }
            }
            app.manage(settings);
            let cache_dir = app.path().app_cache_dir()?;
            app.manage(projects::ProjectsState::load(&cache_dir));
            app.manage(alerts::AlertsState::load(&cache_dir));

            popover::setup(handle)?;
            tray::setup(handle)?;
            alerts::restore_badge(handle);
            projects::start_background_sync(handle.clone());
            if !cfg!(debug_assertions) {
                updater::check_in_background(handle.clone());
            }
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::app_info,
            commands::set_popover_pinned,
            commands::tool_statuses,
            commands::install_command,
            commands::install_tool,
            commands::auth_status,
            commands::list_projects,
            commands::refresh_projects,
            commands::sync_progress,
            commands::rescan_local,
            commands::unseen,
            commands::sync_details,
            commands::clear_cache,
            commands::list_accounts,
            commands::switch_account,
            commands::account_avatar,
            commands::get_settings,
            commands::update_settings,
            commands::login,
            commands::cancel_login,
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
