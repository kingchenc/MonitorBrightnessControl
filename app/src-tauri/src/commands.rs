//! Tauri command handlers exposed to the frontend.

use std::sync::Arc;

use brightness_core::Capabilities;
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter, State};

use crate::config::{Profiles, Settings};
use crate::state::{AppState, MonitorRow};
use crate::tray;

#[tauri::command]
pub fn list_monitors(state: State<'_, Arc<AppState>>) -> Vec<MonitorRow> {
    state.rows()
}

/// Kick off a monitor refresh in the background and return immediately.
/// The frontend should listen for `monitors-changed` events and re-fetch
/// `list_monitors` on each one.
#[tauri::command]
pub fn trigger_refresh(app: AppHandle, state: State<'_, Arc<AppState>>) {
    let state = state.inner().clone();
    std::thread::Builder::new()
        .name("refresh".into())
        .spawn(move || {
            let _ = app.emit("scan-state", true);
            state.refresh_monitors();
            tray::rebuild_menu(&app, &state);
            let _ = app.emit("monitors-changed", ());
            state.refresh_brightness_cache();
            tray::rebuild_menu(&app, &state);
            let _ = app.emit("monitors-changed", ());
            let _ = app.emit("scan-state", false);
        })
        .ok();
}

#[tauri::command]
pub fn get_brightness(state: State<'_, Arc<AppState>>, id: String) -> Result<f32, String> {
    let m = state.find(&id).ok_or_else(|| format!("no monitor {id}"))?;
    m.get_brightness_percent().map_err(|e| e.to_string())
}

#[tauri::command]
pub fn set_brightness(
    state: State<'_, Arc<AppState>>,
    id: Option<String>,
    percent: f32,
) -> Vec<SetResult> {
    state
        .set_brightness(id.as_deref(), percent)
        .into_iter()
        .map(|(id, res)| SetResult {
            id,
            ok: res.is_ok(),
            error: res.err(),
        })
        .collect()
}

#[tauri::command]
pub fn step_brightness(
    state: State<'_, Arc<AppState>>,
    id: Option<String>,
    delta: f32,
) -> Vec<StepResult> {
    state
        .step_brightness(id.as_deref(), delta)
        .into_iter()
        .map(|(id, percent)| StepResult { id, percent })
        .collect()
}

#[tauri::command]
pub fn get_vcp(
    state: State<'_, Arc<AppState>>,
    id: String,
    code: u8,
) -> Result<VcpView, String> {
    let m = state.find(&id).ok_or_else(|| format!("no monitor {id}"))?;
    let v = m.get_vcp(code).map_err(|e| e.to_string())?;
    Ok(VcpView {
        current: v.current,
        maximum: v.maximum,
    })
}

#[tauri::command]
pub fn set_vcp(
    state: State<'_, Arc<AppState>>,
    id: String,
    code: u8,
    value: u16,
) -> Result<(), String> {
    let m = state.find(&id).ok_or_else(|| format!("no monitor {id}"))?;
    m.set_vcp(code, value).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn get_capabilities(
    state: State<'_, Arc<AppState>>,
    id: String,
) -> Result<Capabilities, String> {
    let m = state.find(&id).ok_or_else(|| format!("no monitor {id}"))?;
    m.capabilities().map_err(|e| e.to_string())
}

#[tauri::command]
pub fn load_settings(state: State<'_, Arc<AppState>>) -> Settings {
    state.settings()
}

#[tauri::command]
pub fn save_settings(
    state: State<'_, Arc<AppState>>,
    settings: Settings,
) -> Result<(), String> {
    state
        .replace_settings(settings)
        .map_err(|e| format!("save settings: {e}"))
}

#[tauri::command]
pub fn load_profiles(state: State<'_, Arc<AppState>>) -> Profiles {
    state.profiles()
}

#[tauri::command]
pub fn save_profiles(
    app: AppHandle,
    state: State<'_, Arc<AppState>>,
    profiles: Profiles,
) -> Result<(), String> {
    state
        .replace_profiles(profiles)
        .map_err(|e| format!("save profiles: {e}"))?;
    tray::rebuild_menu(&app, state.inner());
    Ok(())
}

#[tauri::command]
pub fn set_auto_dim(state: State<'_, Arc<AppState>>, enabled: bool) -> Result<(), String> {
    let mut s = state.settings();
    s.auto_dim.enabled = enabled;
    state
        .replace_settings(s)
        .map_err(|e| format!("save settings: {e}"))
}

#[tauri::command]
pub fn quit_app(app: tauri::AppHandle, state: State<'_, Arc<AppState>>) {
    state.request_quit();
    app.exit(0);
}

#[derive(Serialize, Debug)]
pub struct SetResult {
    pub id: String,
    pub ok: bool,
    pub error: Option<String>,
}

#[derive(Serialize, Debug)]
pub struct StepResult {
    pub id: String,
    pub percent: f32,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct VcpView {
    pub current: u16,
    pub maximum: u16,
}
