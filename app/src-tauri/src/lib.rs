//! Tauri application entry point.
//!
//! Wires together:
//! * the cross-platform brightness backend ([`brightness_core`]),
//! * a [`State`] cache that the rest of the app talks to,
//! * the system tray (per-monitor sliders),
//! * global hotkeys,
//! * persistent settings + per-app profiles,
//! * the auto-dim engine.

mod admin_autostart;
mod auto_dim;
mod commands;
mod config;
mod foreground;
mod hotkeys;
mod profiles;
mod scheduler;
mod state;
mod tray;

use std::sync::Arc;

use tauri::{Emitter, Manager, RunEvent, WindowEvent};
use tauri_plugin_autostart::MacosLauncher;

use crate::state::AppState;

/// Application entry point. Invoked from `main.rs`.
pub fn run() {
    init_logging();

    let state = Arc::new(AppState::initialize().expect("failed to initialize backend"));

    let app_state_for_setup = state.clone();
    let app_state_for_invoke = state.clone();

    tauri::Builder::default()
        .plugin(tauri_plugin_single_instance::init(|app, _argv, _cwd| {
            // When a second instance is launched, focus the existing window.
            if let Some(w) = app.get_webview_window("main") {
                let _ = w.show();
                let _ = w.set_focus();
            }
        }))
        .plugin(tauri_plugin_autostart::init(
            MacosLauncher::LaunchAgent,
            Some(vec!["--minimized"]),
        ))
        .plugin(tauri_plugin_window_state::Builder::default().build())
        .plugin(tauri_plugin_os::init())
        .plugin(tauri_plugin_shell::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_global_shortcut::Builder::new().build())
        .manage(app_state_for_invoke)
        .invoke_handler(tauri::generate_handler![
            commands::list_monitors,
            commands::get_brightness,
            commands::set_brightness,
            commands::step_brightness,
            commands::get_vcp,
            commands::set_vcp,
            commands::get_capabilities,
            commands::load_settings,
            commands::save_settings,
            commands::load_profiles,
            commands::save_profiles,
            commands::set_auto_dim,
            commands::trigger_refresh,
            commands::quit_app,
            commands::backup_settings_now,
            commands::list_settings_backups,
            commands::delete_settings_backup,
            commands::restore_settings_backup,
            commands::admin_autostart_status,
            commands::set_admin_autostart,
            commands::default_profile_templates,
        ])
        .setup(move |app| {
            let handle = app.handle().clone();
            let state = app_state_for_setup;
            tray::install(&handle, state.clone())?;
            hotkeys::install(&handle, state.clone())?;
            auto_dim::install(handle.clone(), state.clone());
            scheduler::install(handle.clone(), state.clone());
            foreground::install(handle.clone(), state.clone());

            // Heavy DDC/CI work (monitor enumeration, current brightness
            // reads, push initial values from disk) runs in the background
            // so the tray and main window can appear immediately. The tray
            // menu is rebuilt once each stage completes.
            {
                let state = state.clone();
                let handle = handle.clone();
                std::thread::Builder::new()
                    .name("startup-brightness".into())
                    .spawn(move || {
                        let _ = handle.emit("scan-state", true);
                        state.refresh_monitors();
                        tray::rebuild_menu(&handle, &state);
                        let _ = handle.emit("monitors-changed", ());
                        state.refresh_brightness_cache();
                        tray::rebuild_menu(&handle, &state);
                        let _ = handle.emit("monitors-changed", ());
                        state.apply_initial_settings();
                        state.refresh_brightness_cache();
                        tray::rebuild_menu(&handle, &state);
                        let _ = handle.emit("monitors-changed", ());
                        let _ = handle.emit("scan-state", false);
                    })
                    .ok();
            }

            // Hide the main window if launched with --minimized (autostart).
            if std::env::args().any(|a| a == "--minimized") {
                if let Some(w) = app.get_webview_window("main") {
                    let _ = w.hide();
                }
            } else if let Some(w) = app.get_webview_window("main") {
                let _ = w.show();
            }
            Ok(())
        })
        .build(tauri::generate_context!())
        .expect("error while building tauri application")
        .run(move |app, event| match event {
            RunEvent::ExitRequested { api, .. } => {
                // Keep the process alive in the tray UNLESS the user has
                // explicitly asked to quit (tray menu, `quit_app` command,
                // hotkey).
                let state = app.state::<Arc<AppState>>();
                if !state.is_quitting() {
                    api.prevent_exit();
                }
            }
            RunEvent::WindowEvent {
                label,
                event: WindowEvent::CloseRequested { api, .. },
                ..
            } if label == "main" => {
                if let Some(w) = app.get_webview_window(&label) {
                    let _ = w.hide();
                }
                api.prevent_close();
            }
            _ => {}
        });
}

fn init_logging() {
    let _ = env_logger::Builder::from_env(
        env_logger::Env::default().default_filter_or("info,brightness_core=debug"),
    )
    .format_timestamp_secs()
    .try_init();
}
