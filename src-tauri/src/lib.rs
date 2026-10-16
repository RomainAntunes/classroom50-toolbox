pub mod accounts;
pub mod alerts;
mod calendar;
pub mod classroom50;
pub mod clones;
mod commands;
mod deeplink;
mod diagnostics;
pub mod discovery;
pub mod editors;
pub mod error;
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
mod simulate;
pub mod status;
pub mod student;
mod sync_progress;
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
        // With its `deep-link` feature, a link's arguments reach the deep-link plugin first;
        // `deeplink::handle` shows the popover for a valid one and ignores the rest.
        .plugin(tauri_plugin_single_instance::init(|app, args, _cwd| {
            if !deeplink::is_link_launch(&args) {
                popover::show(app);
            }
        }))
        .plugin(tauri_plugin_deep_link::init())
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
        .plugin(tauri_plugin_clipboard_manager::init())
        .plugin(tauri_plugin_opener::init())
        .manage(popover::PopoverState::default())
        .manage(deeplink::DeepLinkState::default())
        .manage(student::FoundedGroups::default())
        .manage(login::LoginState::default())
        .manage(tools::ToolJobs::default())
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
            alerts::restore_badge(handle);
            projects::start_background_sync(handle.clone());
            deeplink::setup(handle);
            updater::start(handle.clone());
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::app_info,
            commands::diagnostics_report,
            commands::set_popover_pinned,
            commands::hide_popover,
            commands::tool_statuses,
            commands::install_command,
            commands::install_tool,
            commands::upgrade_tool,
            commands::tool_jobs,
            commands::cancel_tool_job,
            commands::tool_update,
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
            commands::preview_badge,
            commands::export_calendar,
            commands::native_calendars,
            commands::open_calendar_file,
            commands::set_calendar_labels,
            commands::set_tray_labels,
            commands::system_languages,
            commands::set_native_calendar,
            commands::simulate_network,
            commands::simulated_network,
            commands::take_deep_link,
            commands::fetch_linked_assignment,
            commands::deep_link_done,
            commands::simulate_deep_link,
            commands::get_settings,
            commands::update_settings,
            commands::login,
            commands::cancel_login,
            commands::grant_scopes,
            commands::sign_out,
            commands::open_terminal,
            commands::installed_editors,
            commands::open_in_editor,
            commands::add_teammates,
            commands::clone_project,
            commands::git_protocol,
            commands::accept_assignment,
            commands::submit_check,
            commands::submit_project,
            commands::pull_project,
            commands::push_check,
            commands::commit_push,
            commands::commit_check,
            commands::commit_only,
            commands::suggested_git_identity,
            commands::set_git_identity,
            commands::update_status,
            commands::check_update,
            commands::install_update,
            commands::defer_update,
            commands::quit_app,
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
