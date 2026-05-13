//! Global hotkey wiring (via `tauri-plugin-global-shortcut`).
//!
//! Each configured accelerator gets its own handler. We capture an `Arc<AppState>`
//! (and the `AppHandle` where needed) so handlers can run independent of the
//! plugin's internal lifecycle.

use std::sync::Arc;

use tauri::{AppHandle, Manager};
use tauri_plugin_global_shortcut::{GlobalShortcutExt, ShortcutState};

use crate::state::AppState;

pub fn install(app: &AppHandle, state: Arc<AppState>) -> tauri::Result<()> {
    let s = state.settings();
    let gs = app.global_shortcut();

    if let Some(accel) = s.hotkeys.brightness_up.clone() {
        let st = state.clone();
        let step = s.hotkeys.step_percent;
        if let Err(e) = gs.on_shortcut(accel.as_str(), move |_app, _shortcut, event| {
            if event.state == ShortcutState::Pressed {
                st.step_brightness(None, step);
            }
        }) {
            log::warn!("hotkey brightness_up '{accel}' failed: {e}");
        }
    }
    if let Some(accel) = s.hotkeys.brightness_down.clone() {
        let st = state.clone();
        let step = s.hotkeys.step_percent;
        if let Err(e) = gs.on_shortcut(accel.as_str(), move |_app, _shortcut, event| {
            if event.state == ShortcutState::Pressed {
                st.step_brightness(None, -step);
            }
        }) {
            log::warn!("hotkey brightness_down '{accel}' failed: {e}");
        }
    }
    if let Some(accel) = s.hotkeys.toggle_night.clone() {
        let st = state.clone();
        let night = s.hotkeys.night_brightness_percent;
        if let Err(e) = gs.on_shortcut(accel.as_str(), move |_app, _shortcut, event| {
            if event.state != ShortcutState::Pressed {
                return;
            }
            let on = !st.night_mode();
            st.set_night_mode(on);
            let target = if on { night } else { 80.0 };
            st.set_brightness(None, target);
        }) {
            log::warn!("hotkey toggle_night '{accel}' failed: {e}");
        }
    }
    if let Some(accel) = s.hotkeys.toggle_window.clone() {
        let app_handle = app.clone();
        if let Err(e) = gs.on_shortcut(accel.as_str(), move |_app, _shortcut, event| {
            if event.state != ShortcutState::Pressed {
                return;
            }
            if let Some(w) = app_handle.get_webview_window("main") {
                let visible = w.is_visible().unwrap_or(false);
                if visible {
                    let _ = w.hide();
                } else {
                    let _ = w.show();
                    let _ = w.set_focus();
                }
            }
        }) {
            log::warn!("hotkey toggle_window '{accel}' failed: {e}");
        }
    }
    if let Some(accel) = s.hotkeys.blackout.clone() {
        let st = state.clone();
        if let Err(e) = gs.on_shortcut(accel.as_str(), move |_app, _shortcut, event| {
            if event.state == ShortcutState::Pressed {
                st.set_brightness(None, 0.0);
            }
        }) {
            log::warn!("hotkey blackout '{accel}' failed: {e}");
        }
    }

    Ok(())
}
