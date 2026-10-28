pub mod accounts;
pub mod alerts;
mod calendar;
pub mod classroom50;
mod clock;
pub mod clones;
mod commands;
mod cooldown;
pub mod credentials;
mod deeplink;
mod diagnostics;
pub mod discovery;
pub mod editors;
pub mod error;
mod events;
mod failures;
pub mod github;
pub mod groups;
mod login;
pub mod noise;
pub mod platform;
mod popover;
pub mod process;
pub mod projects;
pub mod pull;
pub mod push;
pub mod settings;
mod shortcut;
mod simulate;
pub mod status;
pub mod student;
mod sync_progress;
mod testenv;
pub mod tools;
mod tray;
mod updater;
mod watch;

use tauri::{Manager, generate_handler};
use tauri_plugin_autostart::MacosLauncher;

const USER_AGENT: &str = concat!("classroom50-toolbox/", env!("CARGO_PKG_VERSION"));

/// Every IPC command, listed once: `run` registers them (`tauri::generate_handler!`) and the
/// `bindings` test types them for `src/lib/bindings.ts` (`tauri_specta::collect_commands!`).
macro_rules! ipc_commands {
    ($collect:ident) => {
        $collect![
            crate::commands::app_info,
            crate::commands::diagnostics_report,
            crate::commands::set_popover_pinned,
            crate::commands::hide_popover,
            crate::commands::active_global_shortcut,
            crate::commands::pause_global_shortcut,
            crate::commands::tool_statuses,
            crate::commands::install_command,
            crate::commands::install_tool,
            crate::commands::upgrade_tool,
            crate::commands::tool_jobs,
            crate::commands::cancel_tool_job,
            crate::commands::tool_update,
            crate::commands::auth_status,
            crate::commands::list_projects,
            crate::commands::refresh_projects,
            crate::commands::sync_progress,
            crate::commands::rescan_local,
            crate::commands::unseen,
            crate::commands::sync_details,
            crate::commands::clear_cache,
            crate::commands::list_accounts,
            crate::commands::switch_account,
            crate::commands::account_avatar,
            crate::commands::preview_badge,
            crate::commands::export_calendar,
            crate::commands::native_calendars,
            crate::commands::open_calendar_file,
            crate::commands::open_calendar_privacy,
            crate::commands::set_calendar_labels,
            crate::commands::set_tray_menu,
            crate::commands::system_languages,
            crate::commands::system_hour_cycle,
            crate::commands::set_native_calendar,
            crate::commands::simulate_network,
            crate::commands::simulated_network,
            crate::commands::take_deep_link,
            crate::commands::fetch_linked_assignment,
            crate::commands::deep_link_done,
            crate::commands::simulate_deep_link,
            crate::commands::get_settings,
            crate::commands::update_settings,
            crate::commands::login,
            crate::commands::cancel_login,
            crate::commands::grant_scopes,
            crate::commands::sign_out,
            crate::commands::open_terminal,
            crate::commands::installed_editors,
            crate::commands::open_in_editor,
            crate::commands::add_teammates,
            crate::commands::clone_project,
            crate::commands::git_protocol,
            crate::commands::git_credentials,
            crate::commands::set_up_git_credentials,
            crate::commands::accept_assignment,
            crate::commands::submit_check,
            crate::commands::submit_project,
            crate::commands::pull_project,
            crate::commands::push_check,
            crate::commands::commit_push,
            crate::commands::commit_check,
            crate::commands::commit_only,
            crate::commands::suggested_git_identity,
            crate::commands::set_git_identity,
            crate::commands::update_status,
            crate::commands::check_update,
            crate::commands::install_update,
            crate::commands::defer_update,
            crate::commands::quit_app,
            crate::commands::simulate_update,
        ]
    };
}

#[cfg(test)]
mod bindings;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        // Must be registered first: a second launch just reopens the running instance.
        // With its `deep-link` feature, a link's arguments reach the deep-link plugin first;
        // `deeplink::handle` shows the popover for a valid one and ignores the rest.
        .plugin(tauri_plugin_single_instance::init(|app, args, _cwd| {
            if !deeplink::is_link_launch(&args) {
                popover::show(app);
            }
        }))
        .plugin(tauri_plugin_deep_link::init())
        .plugin(
            // 128 KB per file and seven older files kept: 1 MB at most on disk, where the
            // plugin's default deletes the whole file every 40 KB.
            tauri_plugin_log::Builder::new()
                .level(log::LevelFilter::Info)
                .max_file_size(128 * 1024)
                .rotation_strategy(tauri_plugin_log::RotationStrategy::KeepSome(7))
                .build(),
        )
        .plugin(tauri_plugin_autostart::init(
            MacosLauncher::LaunchAgent,
            None,
        ))
        .plugin(tauri_plugin_notification::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .plugin(tauri_plugin_process::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_clipboard_manager::init())
        .plugin(tauri_plugin_opener::init())
        .manage(popover::PopoverState::default())
        .manage(clock::ClockState::default())
        .manage(shortcut::ShortcutRegistration::default())
        .manage(deeplink::DeepLinkState::default())
        .manage(student::FoundedGroups::default())
        .manage(login::LoginState::default())
        .manage(tools::ToolJobs::default())
        .manage(tools::ToolChecks::default())
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
            } else {
                commands::migrate_launch_at_login(handle);
            }
            app.manage(settings);
            app.manage(clones::CloneStore::load(&app.path().app_config_dir()?));
            app.manage(calendar::CalendarState::load(
                &app.path().app_config_dir()?,
                &app.path().app_data_dir()?,
            ));
            let cache_dir = app.path().app_cache_dir()?;
            app.manage(projects::ProjectsState::load(&cache_dir));
            app.manage(alerts::AlertsState::load(&cache_dir));
            app.manage(updater::UpdaterState::load(&cache_dir));

            popover::setup(handle)?;
            tray::setup(handle)?;
            shortcut::setup(handle);
            alerts::restore_badge(handle);
            projects::start_background_sync(handle.clone());
            deeplink::setup(handle);
            if testenv::keep_open() {
                popover::show(handle);
            }
            updater::start(handle.clone());
            Ok(())
        })
        .invoke_handler(ipc_commands!(generate_handler))
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
