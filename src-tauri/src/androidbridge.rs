//! The small Android plugins besides the texts one (messages.rs): what was shared into BYTE (the share sheet and the
//! home-screen widget, `SharePlugin.kt`) and the phone's calendar for the daily briefing (`CalendarPlugin.kt`).
//! On other systems the commands answer "nothing" so shared code needn't care.

use chrono::{Local, TimeZone};
use serde::Deserialize;

/// One calendar event as the plugin reports it.
#[derive(Debug, Clone, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct CalEvent {
    pub title: String,
    pub start: i64,
    pub all_day: bool,
    #[serde(default)]
    pub calendar: String,
}

/// The events as the notes the briefing already reads (the same shape the Mac's Calendar gives), earliest first.
pub fn calendar_notes(events: &[CalEvent], day_label: &str) -> String {
    let mut out = format!("The user's calendar {day_label} (from the phone's calendar; events that repeat may be missing):");
    for e in events {
        let when = if e.all_day {
            "All day".to_string()
        } else {
            Local.timestamp_millis_opt(e.start).single().map(|d| d.format("%-I:%M:%S %p").to_string()).unwrap_or_default()
        };
        let cal = if e.calendar.trim().is_empty() { String::new() } else { format!(" ({})", e.calendar.trim()) };
        out.push_str(&format!("\n- {when} — {}{cal}", e.title.trim()));
    }
    out
}

#[cfg(target_os = "android")]
pub mod android {
    use std::sync::OnceLock;

    use serde::de::DeserializeOwned;
    use tauri::plugin::{PluginHandle, TauriPlugin};
    use tauri::Wry;

    use crate::error::{AppError, AppResult};

    static SHARE: OnceLock<PluginHandle<Wry>> = OnceLock::new();
    static CALENDAR: OnceLock<PluginHandle<Wry>> = OnceLock::new();
    static CAMERA: OnceLock<PluginHandle<Wry>> = OnceLock::new();

    fn plugin(name: &'static str, class: &'static str, slot: &'static OnceLock<PluginHandle<Wry>>) -> TauriPlugin<Wry> {
        tauri::plugin::Builder::<Wry>::new(name)
            .setup(move |_app, api| {
                let handle = api.register_android_plugin("com.loganstarner.byteapp", class)?;
                let _ = slot.set(handle);
                Ok(())
            })
            .build()
    }

    pub fn share_plugin() -> TauriPlugin<Wry> {
        plugin("share", "SharePlugin", &SHARE)
    }

    pub fn calendar_plugin() -> TauriPlugin<Wry> {
        plugin("calendar", "CalendarPlugin", &CALENDAR)
    }

    pub fn camera_plugin() -> TauriPlugin<Wry> {
        plugin("camera", "CameraPlugin", &CAMERA)
    }

    /// Kotlin-only: reports the fold posture to the page by itself; Rust never calls it.
    pub fn fold_plugin() -> TauriPlugin<Wry> {
        static FOLD: OnceLock<PluginHandle<Wry>> = OnceLock::new();
        plugin("fold", "FoldPlugin", &FOLD)
    }

    pub fn camera<T: DeserializeOwned>(command: &str, payload: serde_json::Value) -> AppResult<T> {
        CAMERA.get().ok_or_else(|| AppError::msg("The camera service isn't ready."))?.run_mobile_plugin(command, payload).map_err(|e| AppError::msg(e.to_string()))
    }

    pub fn share<T: DeserializeOwned>(command: &str, payload: serde_json::Value) -> AppResult<T> {
        SHARE.get().ok_or_else(|| AppError::msg("The share service isn't ready."))?.run_mobile_plugin(command, payload).map_err(|e| AppError::msg(e.to_string()))
    }

    pub fn calendar<T: DeserializeOwned>(command: &str, payload: serde_json::Value) -> AppResult<T> {
        CALENDAR.get().ok_or_else(|| AppError::msg("The calendar service isn't ready."))?.run_mobile_plugin(command, payload).map_err(|e| AppError::msg(e.to_string()))
    }
}

/// Text shared into BYTE since last asked (Some("") from the widget: just start a new chat). Once.
#[tauri::command]
pub async fn shared_take() -> Option<String> {
    #[cfg(target_os = "android")]
    {
        let r = tauri::async_runtime::spawn_blocking(|| android::share::<serde_json::Value>("take", serde_json::json!({}))).await.ok()?.ok()?;
        return r["text"].as_str().map(str::to_string);
    }
    #[cfg(not(target_os = "android"))]
    None
}

/// Opens the camera app; the photo's path in BYTE's cache (None: cancelled, or no camera). Android only.
#[tauri::command]
pub async fn camera_take() -> crate::error::AppResult<Option<String>> {
    #[cfg(target_os = "android")]
    {
        let r = tauri::async_runtime::spawn_blocking(|| android::camera::<serde_json::Value>("capture", serde_json::json!({})))
            .await
            .map_err(|e| crate::error::AppError::msg(e.to_string()))??;
        return Ok(r["path"].as_str().map(str::to_string));
    }
    #[cfg(not(target_os = "android"))]
    Ok(None)
}

/// Whether BYTE is the phone's default digital assistant (false elsewhere).
#[tauri::command]
pub async fn phone_assistant_status() -> bool {
    #[cfg(target_os = "android")]
    {
        return tauri::async_runtime::spawn_blocking(|| android::share::<serde_json::Value>("assistantStatus", serde_json::json!({})))
            .await
            .ok()
            .and_then(Result::ok)
            .is_some_and(|v| v["isDefault"] == true);
    }
    #[cfg(not(target_os = "android"))]
    false
}

/// Opens Android's default-assistant settings, where the user chooses BYTE.
#[tauri::command]
pub async fn phone_assistant_open_settings() {
    #[cfg(target_os = "android")]
    let _ = tauri::async_runtime::spawn_blocking(|| android::share::<serde_json::Value>("openAssistantSettings", serde_json::json!({}))).await;
}

/// Whether BYTE may read the phone's calendar (always false elsewhere: the Mac asks through macOS).
#[tauri::command]
pub async fn calendar_status() -> bool {
    #[cfg(target_os = "android")]
    {
        return tauri::async_runtime::spawn_blocking(|| android::calendar::<serde_json::Value>("status", serde_json::json!({})))
            .await
            .ok()
            .and_then(Result::ok)
            .is_some_and(|v| v["read"] == true);
    }
    #[cfg(not(target_os = "android"))]
    false
}

/// Android's permission prompt for the calendar; then whether it was allowed.
#[tauri::command]
pub async fn calendar_request_access() -> bool {
    #[cfg(target_os = "android")]
    {
        let _ = tauri::async_runtime::spawn_blocking(|| android::calendar::<serde_json::Value>("requestPermissions", serde_json::json!({}))).await;
        return calendar_status().await;
    }
    #[cfg(not(target_os = "android"))]
    false
}

/// Today's events from the phone's calendar as briefing notes (Err: not allowed or unreadable). Android only.
#[cfg(target_os = "android")]
pub async fn todays_calendar() -> Result<String, String> {
    #[derive(Deserialize)]
    struct Out {
        events: Vec<CalEvent>,
    }
    let now = Local::now();
    let start = now.date_naive().and_hms_opt(0, 0, 0).and_then(|d| d.and_local_timezone(Local).earliest()).map(|d| d.timestamp_millis()).unwrap_or(0);
    let end = start + 24 * 3600 * 1000;
    let out: Out = tauri::async_runtime::spawn_blocking(move || android::calendar("events", serde_json::json!({ "from": start, "to": end })))
        .await
        .map_err(|e| e.to_string())?
        .map_err(|e| e.to_string())?;
    Ok(calendar_notes(&out.events, "today"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn calendar_events_become_the_briefing_notes() {
        let at = |h: u32, m: u32| Local.with_ymd_and_hms(2026, 10, 12, h, m, 0).unwrap().timestamp_millis();
        let events = vec![
            CalEvent { title: "Dentist".into(), start: at(9, 30), all_day: false, calendar: "Home".into() },
            CalEvent { title: "Birthday: Sam".into(), start: at(0, 0), all_day: true, calendar: String::new() },
            CalEvent { title: "Project review".into(), start: at(14, 0), all_day: false, calendar: "Work".into() },
        ];
        let notes = calendar_notes(&events, "today");
        assert!(notes.starts_with("The user's calendar today"), "{notes}");
        // The briefing reads exactly this shape.
        let read = crate::briefing::events_from(&notes);
        assert_eq!(
            read,
            vec![("All day".to_string(), "Birthday: Sam".to_string()), ("9:30 AM".to_string(), "Dentist".to_string()), ("2:00 PM".to_string(), "Project review".to_string())],
            "{notes}"
        );
    }
}
