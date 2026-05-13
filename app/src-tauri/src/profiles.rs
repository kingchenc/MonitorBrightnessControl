//! Per-app monitor profiles.
//!
//! When the foreground window changes (see [`crate::foreground`]) we look up
//! the matching profile and apply its per-monitor overrides (brightness,
//! contrast, color preset). When the app loses focus we revert to the
//! user's "default" profile, which is the saved
//! [`crate::config::Settings::initial_brightness`] map.

use std::sync::Arc;

use brightness_core::Monitor;

use crate::config::{Profile, ProfileMonitorSettings};
use crate::state::AppState;

const VCP_CONTRAST: u8 = 0x12;
const VCP_COLOR_PRESET: u8 = 0x14;

pub fn apply_profile_for_app(state: &Arc<AppState>, app_id: &str) {
    let profiles = state.profiles();
    // Profiles with no `app_id` are tray-only (manual apply) — never match
    // against the foreground watcher.
    let profile = profiles
        .items
        .iter()
        .find(|p| !p.app_id.is_empty() && p.app_id.eq_ignore_ascii_case(app_id));
    if let Some(p) = profile {
        apply_profile(state, p);
    } else {
        // Restore defaults on focus to a non-profiled app.
        let s = state.settings();
        let monitors = state.monitors_snapshot();
        for m in &monitors {
            if let Some(pct) = s.initial_brightness.get(m.info().id.as_str()).copied() {
                let _ = state.set_brightness(Some(m.info().id.as_str()), pct as f32);
            }
        }
    }
}

/// Apply a single profile to the currently-known monitors. Used by the
/// foreground watcher and the tray "Apply profile" submenu.
pub fn apply_profile(state: &Arc<AppState>, p: &Profile) {
    let monitors = state.monitors_snapshot();
    for m in &monitors {
        let id = m.info().id.as_str();
        // Prefer the new per-monitor block; fall back to the legacy
        // brightness-only map for profiles saved by older builds.
        if let Some(over) = p.monitors.get(id) {
            apply_override(state, m, p, over);
        } else if let Some(pct) = p.brightness.get(id).copied() {
            let _ = state.set_brightness(Some(id), pct as f32);
        }
    }
}

fn apply_override(state: &Arc<AppState>, m: &Monitor, p: &Profile, over: &ProfileMonitorSettings) {
    let id = m.info().id.as_str();
    if let Some(pct) = over.brightness {
        let _ = state.set_brightness(Some(id), pct as f32);
    }
    if let Some(pct) = over.contrast {
        if let Ok(cur) = m.get_vcp(VCP_CONTRAST) {
            let abs = cur.percent_to_absolute(pct as f32);
            if let Err(e) = m.set_vcp(VCP_CONTRAST, abs) {
                log::warn!("apply profile {} → {} contrast: {e}", p.name, m.info().id);
            }
        }
    }
    if let Some(v) = over.color_preset {
        if let Err(e) = m.set_vcp(VCP_COLOR_PRESET, v) {
            log::warn!(
                "apply profile {} → {} color preset: {e}",
                p.name,
                m.info().id
            );
        }
    }
}
