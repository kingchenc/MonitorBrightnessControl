//! Tauri command handlers exposed to the frontend.

use std::sync::Arc;

use brightness_core::Capabilities;
use serde::Serialize;
use tauri::{AppHandle, Emitter, State};

use crate::config::{self, BackupInfo, Profiles, Settings};
use crate::state::{AppState, MonitorRow, VcpView};
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
            crate::set_scan_state(&app, &state, true);
            state.refresh_monitors();
            tray::rebuild_menu(&app, &state);
            let _ = app.emit("monitors-changed", ());
            state.refresh_brightness_cache();
            tray::rebuild_menu(&app, &state);
            let _ = app.emit("monitors-changed", ());
            crate::set_scan_state(&app, &state, false);
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

/// Read a VCP feature, from the cache when it has been read before (also a
/// remembered "unsupported" error). [`clear_vcp_cache`] forces a re-read.
#[tauri::command]
pub fn get_vcp(state: State<'_, Arc<AppState>>, id: String, code: u8) -> Result<VcpView, String> {
    if let Some(cached) = state.vcp_cached(&id, code) {
        return cached;
    }
    let m = state.find(&id).ok_or_else(|| format!("no monitor {id}"))?;
    let read = m
        .get_vcp(code)
        .map(|v| VcpView {
            current: v.current,
            maximum: v.maximum,
        })
        .map_err(|e| e.to_string());
    state.vcp_store(&id, code, read.clone());
    read
}

#[tauri::command]
pub fn set_vcp(
    state: State<'_, Arc<AppState>>,
    id: String,
    code: u8,
    value: u16,
) -> Result<(), String> {
    let m = state.find(&id).ok_or_else(|| format!("no monitor {id}"))?;
    m.set_vcp(code, value).map_err(|e| e.to_string())?;
    state.vcp_written(&id, code, value);
    Ok(())
}

/// Forget every cached VCP read so the next `get_vcp` asks the hardware
/// (explicit Refresh: picks up changes made on the monitor's own menu).
#[tauri::command]
pub fn clear_vcp_cache(state: State<'_, Arc<AppState>>) {
    state.clear_vcp_cache();
}

/// Whether a monitor scan is running right now. A freshly built window asks
/// this once; afterwards it follows the `scan-state` events.
#[tauri::command]
pub fn scan_state(state: State<'_, Arc<AppState>>) -> bool {
    state.is_scanning()
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
pub fn save_settings(state: State<'_, Arc<AppState>>, settings: Settings) -> Result<(), String> {
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

// --- Settings backups -------------------------------------------------------

#[tauri::command]
pub fn backup_settings_now() -> Result<Vec<BackupInfo>, String> {
    config::backup_now().map_err(|e| format!("backup failed: {e}"))
}

#[tauri::command]
pub fn list_settings_backups() -> Vec<BackupInfo> {
    config::list_backups()
}

#[tauri::command]
pub fn delete_settings_backup(file_name: String) -> Result<(), String> {
    config::delete_backup(&file_name).map_err(|e| format!("delete failed: {e}"))
}

/// Restore a backup and refresh the in-memory settings + tray so the change
/// takes effect without a restart. Returns the restored settings to the UI.
#[tauri::command]
pub fn restore_settings_backup(
    app: AppHandle,
    state: State<'_, Arc<AppState>>,
    file_name: String,
) -> Result<Settings, String> {
    let restored =
        config::restore_backup(&file_name).map_err(|e| format!("restore failed: {e}"))?;
    state.reload_settings(restored.clone());
    tray::rebuild_menu(&app, state.inner());
    Ok(restored)
}

/// Restore a profiles backup and refresh the in-memory profiles + tray so the
/// change takes effect without a restart. Returns the restored profiles.
#[tauri::command]
pub fn restore_profiles_backup(
    app: AppHandle,
    state: State<'_, Arc<AppState>>,
    file_name: String,
) -> Result<Profiles, String> {
    let restored =
        config::restore_profiles(&file_name).map_err(|e| format!("restore failed: {e}"))?;
    state.reload_profiles(restored.clone());
    tray::rebuild_menu(&app, state.inner());
    Ok(restored)
}

// --- Elevated autostart (Task Scheduler) ------------------------------------

#[tauri::command]
pub fn admin_autostart_status() -> Result<bool, String> {
    crate::admin_autostart::status()
}

/// Create or remove the elevated Task Scheduler autostart entry. On Windows
/// this triggers a UAC prompt. Returns the resulting status (true = enabled).
#[tauri::command]
pub fn set_admin_autostart(enabled: bool) -> Result<bool, String> {
    crate::admin_autostart::set(enabled)?;
    crate::admin_autostart::status()
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

/// A monitor-agnostic starting point the UI can turn into a full profile by
/// applying the values to each connected monitor. `color_preset` uses the
/// same VCP 0x14 encoding the rest of the app exposes (1=sRGB, 2=Native,
/// 4=5000K, 5=6500K, 6=7500K, 8=9300K, 11=User).
#[derive(Serialize, Debug, Clone)]
pub struct ProfileTemplate {
    pub id: String,
    pub name: String,
    pub brightness: u8,
    pub contrast: u8,
    pub color_preset: u16,
}

/// Curated starting points for common usage scenarios. The frontend localizes
/// the label by `id` and applies the values to every connected monitor.
#[tauri::command]
pub fn default_profile_templates() -> Vec<ProfileTemplate> {
    vec![
        ProfileTemplate {
            id: "gaming".into(),
            name: "Gaming".into(),
            brightness: 100,
            contrast: 75,
            color_preset: 2, // Native — punchy, full gamut
        },
        ProfileTemplate {
            id: "movie".into(),
            name: "Movie / Video".into(),
            brightness: 35,
            contrast: 60,
            color_preset: 4, // 5000K — warm, cinematic
        },
        ProfileTemplate {
            id: "reading".into(),
            name: "Reading / Night".into(),
            brightness: 25,
            contrast: 50,
            color_preset: 4, // 5000K — easy on the eyes at night
        },
        ProfileTemplate {
            id: "office".into(),
            name: "Office / Productivity".into(),
            brightness: 75,
            contrast: 60,
            color_preset: 5, // 6500K — neutral daylight
        },
        ProfileTemplate {
            id: "photo".into(),
            name: "Photo editing".into(),
            brightness: 70,
            contrast: 50,
            color_preset: 1, // sRGB — calibrated reference
        },
    ]
}
