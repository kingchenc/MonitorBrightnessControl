//! Auto-dim engine: smoothly transitions brightness between day and night
//! values around sunrise/sunset for the configured latitude/longitude.
//!
//! Algorithm:
//! * Wake up every minute on a background tokio task.
//! * Compute today's sunrise and sunset for the configured coordinates.
//! * Target brightness = day during the day, night during the night, with a
//!   linear interpolation across `transition_minutes` either side of each
//!   transition.

use std::sync::Arc;
use std::time::Duration;

use chrono::{DateTime, Utc};
use sunrise::{Coordinates, SolarDay, SolarEvent};
use tauri::AppHandle;

use crate::state::AppState;

pub fn install(app: AppHandle, state: Arc<AppState>) {
    let _ = app;
    // A dedicated OS thread is simpler than depending on a Tokio runtime
    // (Tauri 2's `async_runtime` is available, but auto-dim is a one-tick-
    // per-minute loop where async buys us nothing).
    std::thread::Builder::new()
        .name("auto-dim".into())
        .spawn(move || loop {
            std::thread::sleep(Duration::from_secs(60));
            let s = state.settings();
            if !s.auto_dim.enabled {
                continue;
            }
            let now = Utc::now();
            let target = compute_target(&s.auto_dim, now);
            apply_target(&state, target);
        })
        .ok();
}

fn compute_target(s: &crate::config::AutoDimSettings, now: DateTime<Utc>) -> f32 {
    // Today's sunrise/sunset.
    let date = now.date_naive();
    let coords = Coordinates::new(s.latitude, s.longitude)
        .unwrap_or_else(|| Coordinates::new(0.0, 0.0).expect("default coords"));
    let day = SolarDay::new(coords, date);
    let sunrise: DateTime<Utc> = day.event_time(SolarEvent::Sunrise);
    let sunset: DateTime<Utc> = day.event_time(SolarEvent::Sunset);

    // Smooth transition over `transition_minutes` either side of both events.
    let trans = s.transition_minutes.max(1) as i64;
    let half = chrono::Duration::minutes(trans);

    let day_b = s.day_brightness as f32;
    let night_b = s.night_brightness as f32;

    if now < sunrise - half {
        night_b
    } else if now >= sunrise - half && now <= sunrise + half {
        let progress = (now - (sunrise - half)).num_seconds() as f32
            / (2.0 * half.num_seconds() as f32);
        lerp(night_b, day_b, progress.clamp(0.0, 1.0))
    } else if now > sunrise + half && now < sunset - half {
        day_b
    } else if now >= sunset - half && now <= sunset + half {
        let progress = (now - (sunset - half)).num_seconds() as f32
            / (2.0 * half.num_seconds() as f32);
        lerp(day_b, night_b, progress.clamp(0.0, 1.0))
    } else {
        night_b
    }
}

fn lerp(a: f32, b: f32, t: f32) -> f32 {
    a + (b - a) * t
}

fn apply_target(state: &AppState, target: f32) {
    state.set_brightness(None, target);
}
