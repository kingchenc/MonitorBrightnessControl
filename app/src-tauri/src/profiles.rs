//! Per-app monitor profiles.
//!
//! When the foreground window changes (see [`crate::foreground`]) we look up
//! the matching profile and apply its per-monitor overrides (brightness,
//! contrast, color preset). When the app loses focus we revert to the
//! user's "default" profile, which is the saved
//! [`crate::config::Settings::initial_brightness`] map.

use std::sync::Arc;

use brightness_core::{Monitor, VcpValue};

use crate::config::{Profile, ProfileMonitorSettings};
use crate::state::{AppState, VcpView};

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
        // Only the maximum is needed to scale the percentage. Take it from
        // the cache when known, saving a multi-second DDC/CI read per app
        // switch; otherwise read the hardware (and remember the result).
        let cur = match state.vcp_cached(id, VCP_CONTRAST) {
            Some(Ok(v)) if v.maximum > 0 => Ok(VcpValue::new(v.current, v.maximum)),
            _ => {
                let read = m.get_vcp(VCP_CONTRAST);
                if let Ok(v) = &read {
                    state.vcp_store(
                        id,
                        VCP_CONTRAST,
                        Ok(VcpView {
                            current: v.current,
                            maximum: v.maximum,
                        }),
                    );
                }
                read
            }
        };
        if let Ok(cur) = cur {
            let abs = cur.percent_to_absolute(pct as f32);
            match m.set_vcp(VCP_CONTRAST, abs) {
                Ok(()) => state.vcp_written(id, VCP_CONTRAST, abs),
                Err(e) => log::warn!("apply profile {} → {} contrast: {e}", p.name, m.info().id),
            }
        }
    }
    if let Some(v) = over.color_preset {
        match m.set_vcp(VCP_COLOR_PRESET, v) {
            Ok(()) => state.vcp_written(id, VCP_COLOR_PRESET, v),
            Err(e) => log::warn!(
                "apply profile {} → {} color preset: {e}",
                p.name,
                m.info().id
            ),
        }
    }
}
