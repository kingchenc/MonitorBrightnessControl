//! Time-of-day scheduler.
//!
//! Wakes once per minute, checks every enabled `ScheduleEntry` against the
//! current local time, and applies the configured brightness when an entry
//! is due. Each entry fires at most once per local day so the user can
//! still freely adjust brightness afterwards without the scheduler
//! immediately overriding the change.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use chrono::{DateTime, Datelike, Local, NaiveDate, NaiveTime, TimeZone};
use tauri::AppHandle;

use crate::config::ScheduleEntry;
use crate::state::AppState;

pub fn install(app: AppHandle, state: Arc<AppState>) {
    let _ = app;
    std::thread::Builder::new()
        .name("scheduler".into())
        .spawn(move || {
            let mut last_fired: HashMap<String, NaiveDate> = HashMap::new();
            loop {
                std::thread::sleep(Duration::from_secs(30));
                let s = state.settings();
                if !s.schedules.enabled {
                    continue;
                }
                let now = Local::now();
                for entry in &s.schedules.items {
                    if !entry.enabled || entry.id.is_empty() {
                        continue;
                    }
                    if let Err(e) = maybe_fire(&state, entry, now, &mut last_fired) {
                        log::warn!("schedule {} skipped: {e}", entry.id);
                    }
                }
            }
        })
        .ok();
}

fn maybe_fire(
    state: &AppState,
    entry: &ScheduleEntry,
    now: DateTime<Local>,
    last_fired: &mut HashMap<String, NaiveDate>,
) -> Result<(), String> {
    let parsed: NaiveTime = NaiveTime::parse_from_str(&entry.time, "%H:%M")
        .map_err(|e| format!("invalid time {:?}: {e}", entry.time))?;

    if !entry.days.is_empty() {
        let weekday = now.weekday().num_days_from_sunday() as u8;
        if !entry.days.contains(&weekday) {
            return Ok(());
        }
    }

    let today = now.date_naive();
    let scheduled_naive = today.and_time(parsed);
    let scheduled = match Local.from_local_datetime(&scheduled_naive).single() {
        Some(dt) => dt,
        None => return Ok(()), // ambiguous (DST) — wait for next minute
    };

    if now < scheduled {
        return Ok(());
    }
    if last_fired.get(&entry.id) == Some(&today) {
        return Ok(());
    }
    // Don't reach back hours after a missed window — only fire if we're
    // within ten minutes of the scheduled time on the same day. This
    // prevents a freshly-enabled entry from snapping to an already-passed
    // time.
    if (now - scheduled).num_minutes() > 10 {
        last_fired.insert(entry.id.clone(), today);
        return Ok(());
    }

    apply(state, entry);
    last_fired.insert(entry.id.clone(), today);
    Ok(())
}

fn apply(state: &AppState, entry: &ScheduleEntry) {
    let pct = entry.brightness_percent.min(100) as f32;
    if entry.monitor_ids.is_empty() {
        state.set_brightness(None, pct);
    } else {
        for id in &entry.monitor_ids {
            state.set_brightness(Some(id), pct);
        }
    }
    log::info!(
        "schedule {} fired: {}% on {}",
        entry.id,
        entry.brightness_percent,
        if entry.monitor_ids.is_empty() {
            "all monitors".to_string()
        } else {
            entry.monitor_ids.join(", ")
        }
    );
}
