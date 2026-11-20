//! Zirtola Launcher: Tauri shell around `zirtola-core`.

mod auth;
mod commands;
mod crash;
mod hardware;
mod jobs;
mod playtime;
mod prereqs;
mod reports;
mod running;
mod saves;
mod startup;
mod state;

use tauri::{Emitter, Manager};

pub fn run() {
    if !startup::begin() {
        return;
    }
    let built = tauri::Builder::default()
        .plugin(tauri_plugin_single_instance::init(|app, args, _cwd| {
            // A second start (e.g. the website's zirtola://refresh link) is
            // forwarded here: refresh the running launcher and bring it forward.
            if let Some(link) = commands::deep_link_arg(args) {
                let _ = app.emit("deep-link", link);
            }
            startup::note("a second start was handed to this running launcher: bringing its window forward");
            // (opened again while hidden behind a running game: the player wants it, so it stays open after the game)
            reports::CLOSE_WHEN_DONE.store(false, std::sync::atomic::Ordering::SeqCst);
            if let Some(w) = app.get_webview_window("main") {
                let _ = w.unminimize();
                let _ = w.show();
                // A window left on a monitor that is no longer there comes back to the middle of the main screen.
                let on_screen = match (w.outer_position(), w.available_monitors()) {
                    (Ok(p), Ok(ms)) => ms.iter().any(|m| {
                        let (mp, sz) = (m.position(), m.size());
                        p.x + 100 > mp.x && p.x < mp.x + sz.width as i32 - 100 && p.y >= mp.y - 20 && p.y < mp.y + sz.height as i32 - 100
                    }),
                    _ => true,
                };
                if !on_screen {
                    startup::note("its window was off every screen: centred");
                    let _ = w.center();
                }
                let _ = w.set_focus();
            } else {
                startup::note("the running launcher has no window");
            }
        }))
        .plugin(tauri_plugin_deep_link::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .setup(|app| {
            let dir = app.path().app_data_dir().map_err(|e| {
                startup::failed("finding its data folder", &e);
                e
            })?;
            startup::note(format!("setup: data folder {}", dir.display()));
            let st = state::AppState::new(dir).map_err(|e| {
                startup::failed("opening its data folder", &format!("{e:#}"));
                e
            })?;
            app.manage(st);
            // The window is made here, not from the config, so a WebView2 that won't start gets retried
            // (fresh profile folder, then without the GPU) instead of closing the launcher.
            if let Err(e) = startup::create_main_window(app) {
                startup::failed_webview(&e);
                std::process::exit(1);
            }
            match app.get_webview_window("main") {
                Some(w) => startup::note(format!(
                    "setup done: window created at {:?}, size {:?}, visible {:?}",
                    w.outer_position().ok(),
                    w.outer_size().ok(),
                    w.is_visible().ok()
                )),
                None => startup::note("setup done: NO main window was created"),
            }
            startup::watch_ui();
            Ok(())
        })
        .on_window_event(|window, event| {
            if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                // A game is still running: hide the window instead of closing, so the run keeps being timed for
                // playtime and a crash is still caught; the launcher closes itself once the game exits.
                if reports::GAMES_RUNNING.load(std::sync::atomic::Ordering::SeqCst) > 0 {
                    api.prevent_close();
                    reports::CLOSE_WHEN_DONE.store(true, std::sync::atomic::Ordering::SeqCst);
                    let _ = window.hide();
                    startup::note("window closed while a game runs: hidden until the game exits");
                    return;
                }
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
            commands::prereqs_check,
            commands::prereqs_install,
            commands::resolve_save_location,
            commands::set_branch,
            commands::hardware_report,
            commands::pending_crashes,
            commands::send_crash_report,
            commands::dismiss_crash,
        ])
        .build(tauri::generate_context!());
    match built {
        Ok(app) => {
            app.run(|_, event| {
                if let tauri::RunEvent::Exit = event {
                    startup::note("exit");
                }
            });
        }
        // (a second start handed to the running launcher exits inside the single-instance plugin and never gets here)
        Err(e) => startup::failed("creating its window", &e),
    }
}
