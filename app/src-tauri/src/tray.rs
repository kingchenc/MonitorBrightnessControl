//! System tray icon and menu.
//!
//! The menu lists every detected monitor, plus brightness "Up / Down" entries
//! for each of them, plus night-mode toggle and Quit. Left-clicking the tray
//! icon shows the main window.

use std::sync::Arc;

use tauri::{
    menu::{Menu, MenuBuilder, MenuEvent, MenuItemBuilder, PredefinedMenuItem, SubmenuBuilder},
    tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent},
    AppHandle, Manager,
};

use crate::state::AppState;

const STEP_PERCENT: f32 = 5.0;

pub fn install(app: &AppHandle, state: Arc<AppState>) -> tauri::Result<()> {
    let menu = build_menu(app, &state)?;
    let _tray = TrayIconBuilder::with_id("main-tray")
        .menu(&menu)
        .show_menu_on_left_click(false)
        .tooltip("Monitor Brightness Control")
        .icon(app.default_window_icon().cloned().unwrap_or_else(|| {
            // Fallback to a solid 16×16 black PNG embedded at compile time so
            // the tray always has something to display even when icons are
            // missing during dev.
            tauri::image::Image::from_bytes(include_bytes!("../icons/tray.png"))
                .expect("embedded tray icon")
        }))
        .on_tray_icon_event({
            let state = state.clone();
            move |tray, event| {
                if let TrayIconEvent::Click {
                    button: MouseButton::Left,
                    button_state: MouseButtonState::Up,
                    ..
                } = event
                {
                    let app = tray.app_handle();
                    if let Some(w) = app.get_webview_window("main") {
                        let _ = w.show();
                        let _ = w.set_focus();
                        let _ = w.unminimize();
                    }
                    state.refresh_monitors();
                }
            }
        })
        .on_menu_event({
            let state = state.clone();
            move |app, event| handle_menu_event(app, &state, event)
        })
        .build(app)?;
    Ok(())
}

fn build_menu(app: &AppHandle, state: &AppState) -> tauri::Result<Menu<tauri::Wry>> {
    let mut builder = MenuBuilder::new(app);
    let monitors = state.rows();
    if monitors.is_empty() {
        builder = builder.item(
            &MenuItemBuilder::with_id("no-monitors", "No monitors detected")
                .enabled(false)
                .build(app)?,
        );
    } else {
        for m in &monitors {
            let pct_text = m
                .percent
                .map(|p| format!("{:.0}%", p))
                .unwrap_or_else(|| "—".into());
            let mut sub = SubmenuBuilder::new(app, format!("{} ({pct_text})", m.name));
            for &v in &[100u8, 80, 60, 40, 20, 10, 5, 0] {
                sub = sub.item(
                    &MenuItemBuilder::with_id(
                        format!("set:{}::{v}", m.id),
                        format!("{}%", v),
                    )
                    .build(app)?,
                );
            }
            sub = sub.separator();
            sub = sub.item(
                &MenuItemBuilder::with_id(
                    format!("up:{}", m.id),
                    format!("+{:.0}%", STEP_PERCENT),
                )
                .build(app)?,
            );
            sub = sub.item(
                &MenuItemBuilder::with_id(
                    format!("down:{}", m.id),
                    format!("-{:.0}%", STEP_PERCENT),
                )
                .build(app)?,
            );
            builder = builder.item(&sub.build()?);
        }
        builder = builder.separator();
        builder = builder.item(
            &MenuItemBuilder::with_id("all-up", format!("All +{:.0}%", STEP_PERCENT))
                .build(app)?,
        );
        builder = builder.item(
            &MenuItemBuilder::with_id("all-down", format!("All -{:.0}%", STEP_PERCENT))
                .build(app)?,
        );
        builder = builder.item(
            &MenuItemBuilder::with_id("toggle-night", "Toggle night mode")
                .build(app)?,
        );
    }
    // Profile picker — only when at least one profile exists.
    let profiles = state.profiles();
    if !profiles.items.is_empty() {
        let mut sub = SubmenuBuilder::new(app, "Apply profile");
        for p in &profiles.items {
            let label = if p.name.trim().is_empty() {
                p.app_id.clone()
            } else {
                p.name.clone()
            };
            sub = sub.item(
                &MenuItemBuilder::with_id(format!("profile:{}", p.id), label).build(app)?,
            );
        }
        builder = builder.separator();
        builder = builder.item(&sub.build()?);
    }

    builder = builder.separator();
    builder = builder.item(
        &MenuItemBuilder::with_id("show", "Show window").build(app)?,
    );
    builder = builder.item(&PredefinedMenuItem::separator(app)?);
    builder = builder.item(&MenuItemBuilder::with_id("quit", "Quit").build(app)?);

    builder.build()
}

fn handle_menu_event(app: &AppHandle, state: &Arc<AppState>, event: MenuEvent) {
    let id = event.id().0.as_str().to_string();
    match id.as_str() {
        "show" => {
            if let Some(w) = app.get_webview_window("main") {
                let _ = w.show();
                let _ = w.set_focus();
            }
        }
        "quit" => {
            state.request_quit();
            app.exit(0);
        }
        "all-up" => {
            state.step_brightness(None, STEP_PERCENT);
            rebuild_menu(app, state);
        }
        "all-down" => {
            state.step_brightness(None, -STEP_PERCENT);
            rebuild_menu(app, state);
        }
        "toggle-night" => {
            let on = !state.night_mode();
            state.set_night_mode(on);
            let target = if on {
                state.settings().hotkeys.night_brightness_percent
            } else {
                80.0
            };
            state.set_brightness(None, target);
            rebuild_menu(app, state);
        }
        other => {
            if let Some(rest) = other.strip_prefix("profile:") {
                let profiles = state.profiles();
                if let Some(p) = profiles.items.iter().find(|p| p.id == rest) {
                    crate::profiles::apply_profile(state, p);
                    rebuild_menu(app, state);
                }
            } else if let Some(rest) = other.strip_prefix("up:") {
                state.step_brightness(Some(rest), STEP_PERCENT);
                rebuild_menu(app, state);
            } else if let Some(rest) = other.strip_prefix("down:") {
                state.step_brightness(Some(rest), -STEP_PERCENT);
                rebuild_menu(app, state);
            } else if let Some(rest) = other.strip_prefix("set:") {
                if let Some((id, val)) = rest.split_once("::") {
                    if let Ok(v) = val.parse::<f32>() {
                        state.set_brightness(Some(id), v);
                        rebuild_menu(app, state);
                    }
                }
            }
        }
    }
}

pub fn rebuild_menu(app: &AppHandle, state: &Arc<AppState>) {
    if let Ok(menu) = build_menu(app, state) {
        if let Some(tray) = app.tray_by_id("main-tray") {
            let _ = tray.set_menu(Some(menu));
        }
    }
}
