//! Shared application state.
//!
//! [`AppState`] owns the [`brightness_core`] manager, the cached monitor list,
//! and the persisted settings + profiles. Every command and helper goes
//! through this type so locking is uniform.

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use brightness_core::{Monitor, MonitorManager};
use parking_lot::RwLock;
use serde::Serialize;

use crate::config::{self, Profiles, Settings};

#[derive(Serialize, Clone, Debug)]
pub struct MonitorRow {
    pub id: String,
    pub name: String,
    pub kind: String,
    pub percent: Option<f32>,
}

pub struct AppState {
    manager: Arc<dyn MonitorManager>,
    monitors: RwLock<Vec<Monitor>>,
    settings: RwLock<Settings>,
    profiles: RwLock<Profiles>,
    /// Whether the app is currently in user-toggled "night" mode (a hotkey
    /// flag, separate from auto-dim).
    night_mode: RwLock<bool>,
    /// Cached brightness percentage per monitor id. Populated lazily by a
    /// background thread on startup so the tray menu and main window can
    /// render before every monitor has answered its DDC/CI query.
    brightness_cache: RwLock<HashMap<String, f32>>,
    /// Set when the user explicitly requested a full quit (tray menu, hotkey,
    /// `quit_app` command). The Tauri RunEvent::ExitRequested handler
    /// otherwise vetoes every exit so the app keeps living in the tray.
    quitting: AtomicBool,
}

impl AppState {
    pub fn initialize() -> anyhow::Result<Self> {
        let manager = brightness_core::platform::default_manager()
            .map_err(|e| anyhow::anyhow!("backend init: {e}"))?;
        // Monitor enumeration involves DDC/CI capability roundtrips and a
        // synchronous WMI probe for the internal panel — both expensive on
        // Windows. We defer the call to a background thread so Tauri's
        // initialization (window + tray) is not blocked. `refresh_monitors`
        // populates the list once the app is up.
        let settings = config::load_settings();
        let profiles = config::load_profiles();
        Ok(Self {
            manager,
            monitors: RwLock::new(Vec::new()),
            settings: RwLock::new(settings),
            profiles: RwLock::new(profiles),
            night_mode: RwLock::new(false),
            brightness_cache: RwLock::new(HashMap::new()),
            quitting: AtomicBool::new(false),
        })
    }

    /// Synchronously query every monitor's current brightness and refresh
    /// the cache. Heavy (DDC/CI roundtrips); call from a background thread.
    pub fn refresh_brightness_cache(&self) {
        let mut next = HashMap::new();
        for m in self.monitors_snapshot() {
            if let Ok(p) = m.get_brightness_percent() {
                next.insert(m.info().id.0.clone(), p);
            }
        }
        *self.brightness_cache.write() = next;
    }

    pub fn request_quit(&self) {
        self.quitting.store(true, Ordering::SeqCst);
    }

    pub fn is_quitting(&self) -> bool {
        self.quitting.load(Ordering::SeqCst)
    }

    pub fn refresh_monitors(&self) {
        match self.manager.refresh() {
            Ok(list) => {
                *self.monitors.write() = list;
            }
            Err(e) => log::warn!("refresh monitors: {e}"),
        }
    }

    pub fn monitors_snapshot(&self) -> Vec<Monitor> {
        self.monitors.read().clone()
    }

    pub fn rows(&self) -> Vec<MonitorRow> {
        let cache = self.brightness_cache.read();
        self.monitors_snapshot()
            .into_iter()
            .map(|m| {
                let info = m.info();
                let percent = cache.get(info.id.as_str()).copied();
                MonitorRow {
                    id: info.id.0.clone(),
                    name: info.name.clone(),
                    kind: info.kind.to_string(),
                    percent,
                }
            })
            .collect()
    }

    pub fn settings(&self) -> Settings {
        self.settings.read().clone()
    }

    pub fn replace_settings(&self, s: Settings) -> std::io::Result<()> {
        config::save_settings(&s)?;
        *self.settings.write() = s;
        Ok(())
    }

    pub fn profiles(&self) -> Profiles {
        self.profiles.read().clone()
    }

    pub fn replace_profiles(&self, p: Profiles) -> std::io::Result<()> {
        config::save_profiles(&p)?;
        *self.profiles.write() = p;
        Ok(())
    }

    pub fn set_night_mode(&self, on: bool) {
        *self.night_mode.write() = on;
    }
    pub fn night_mode(&self) -> bool {
        *self.night_mode.read()
    }

    /// Apply the user's saved per-monitor initial brightness on startup.
    pub fn apply_initial_settings(&self) {
        let settings = self.settings();
        for m in self.monitors_snapshot() {
            if let Some(pct) = settings
                .initial_brightness
                .get(m.info().id.as_str())
                .copied()
            {
                if let Err(e) = m.set_brightness_percent(pct as f32) {
                    log::warn!("apply initial brightness {pct}% to {}: {e}", m.info().id);
                }
            }
        }
    }

    /// Return the monitor handle for the given id.
    pub fn find(&self, id: &str) -> Option<Monitor> {
        self.monitors_snapshot()
            .into_iter()
            .find(|m| m.info().id.as_str() == id)
    }

    /// Apply a brightness percentage to one monitor or all.
    pub fn set_brightness(
        &self,
        id: Option<&str>,
        percent: f32,
    ) -> Vec<(String, Result<(), String>)> {
        let mut out = Vec::new();
        let monitors = self.monitors_snapshot();
        for m in monitors {
            let info = m.info().clone();
            if let Some(target) = id {
                if info.id.as_str() != target {
                    continue;
                }
            }
            let r = m.set_brightness_percent(percent).map_err(|e| e.to_string());
            if r.is_ok() {
                self.brightness_cache
                    .write()
                    .insert(info.id.0.clone(), percent.clamp(0.0, 100.0));
            }
            out.push((info.id.0.clone(), r));
        }
        out
    }

    /// Step brightness by `delta` on every monitor (or one). Returns the new
    /// percentages.
    pub fn step_brightness(&self, id: Option<&str>, delta: f32) -> Vec<(String, f32)> {
        let mut out = Vec::new();
        for m in self.monitors_snapshot() {
            let info = m.info().clone();
            if let Some(target) = id {
                if info.id.as_str() != target {
                    continue;
                }
            }
            // Prefer the cached value to avoid a DDC roundtrip — the hardware
            // is the source of truth, but a cached read is good enough for
            // computing the next step.
            let cur = self
                .brightness_cache
                .read()
                .get(info.id.as_str())
                .copied()
                .or_else(|| m.get_brightness_percent().ok())
                .unwrap_or(50.0);
            let next = (cur + delta).clamp(0.0, 100.0);
            if let Err(e) = m.set_brightness_percent(next) {
                log::warn!("step brightness on {}: {e}", info.id);
                continue;
            }
            self.brightness_cache
                .write()
                .insert(info.id.0.clone(), next);
            out.push((info.id.0.clone(), next));
        }
        out
    }
}
