//! Zirtola Launcher: Tauri shell around `zirtola-core`.

mod auth;
mod commands;
mod crash;
mod hardware;
mod jobs;
mod playtime;
mod reports;
mod saves;
mod state;

use tauri::{Emitter, Manager};

pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_single_instance::init(|app, args, _cwd| {
            // A second start (e.g. the website's zirtola://refresh link) is
            // forwarded here: refresh the running launcher and bring it forward.
            if let Some(link) = commands::deep_link_arg(args) {
                let _ = app.emit("deep-link", link);
            }
            if let Some(w) = app.get_webview_window("main") {
                let _ = w.unminimize();
                let _ = w.show();
                let _ = w.set_focus();
            }
        }))
        .plugin(tauri_plugin_deep_link::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .setup(|app| {
            let dir = app.path().app_data_dir()?;
            app.manage(state::AppState::new(dir)?);
            Ok(())
        })
        .on_window_event(|window, event| {
            if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                // Pause running downloads cleanly so their journals are saved.
                let app = window.app_handle().clone();
                if commands::cancel_all_jobs(&app) {
                    api.prevent_close();
                    tauri::async_runtime::spawn(async move {
                        for _ in 0..50 {
                            if app
                                .state::<state::AppState>()
                                .jobs
                                .lock()
                                .unwrap()
                                .is_empty()
                            {
                                break;
                            }
                            tokio::time::sleep(std::time::Duration::from_millis(100)).await;
                        }
                        app.exit(0);
                    });
                }
            }
        })
        .invoke_handler(tauri::generate_handler![
            commands::app_info,
            commands::restore_session,
            commands::sign_in,
            commands::cancel_sign_in,
            commands::sign_out,
            commands::redeem_key,
            commands::library,
            commands::suggest_install_dir,
            commands::pick_folder,
            commands::disk_info,
            commands::install_game,
            commands::update_game,
            commands::verify_game,
            commands::uninstall_game,
            commands::pause_job,
            commands::forget_install,
            commands::launch_game,
            commands::open_url,
            commands::open_install_folder,
            commands::get_settings,
            commands::set_settings,
            commands::check_launcher_update,
            commands::install_launcher_update,
            commands::startup_deep_link,
            commands::deactivate_device,
            commands::game_changelog,
            commands::game_license,
            commands::delete_game_saves,
            commands::resolve_save_location,
            commands::set_branch,
            commands::hardware_report,
            commands::pending_crashes,
            commands::send_crash_report,
            commands::dismiss_crash,
        ])
        .run(tauri::generate_context!())
        .expect("error while running the Zirtola Launcher");
}
