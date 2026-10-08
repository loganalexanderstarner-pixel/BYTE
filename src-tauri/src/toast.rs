//! Notifications you can answer (Windows). A reminder arrives as a toast with a **Snooze** list (5 minutes, 10 minutes, an hour:
//! Windows keeps and re-shows it by itself, even if BYTE has closed) and a **Done** button that BYTE handles while it runs. The
//! plain `scheduler::notify` stays for everything else, and is the fallback on any other system or when a toast can't be shown.
//!
//! The wording of a button is ours; what it does is decided here from a short argument (`done:<task id>`), never from anything
//! the notification could carry in its text.

use chrono::{DateTime, TimeZone};

use crate::db::Db;
use crate::error::AppResult;

/// The app's identity for notifications. It is the identifier in `tauri.conf.json` (a test checks), which is also what the
/// installer gives BYTE's Start menu shortcut.
pub const APP_ID: &str = "com.loganstarner.byte";

/// What a button asks BYTE to do.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Do {
    /// Mark this task done (a repeating task moves to its next time).
    Done(i64),
    /// The toast itself was clicked: bring BYTE forward.
    Open,
}

/// The argument a button carries → what it means. Anything unexpected means nothing.
pub fn parse_arg(arg: &str) -> Option<Do> {
    let arg = arg.trim();
    if arg == "open" {
        return Some(Do::Open);
    }
    let id = arg.strip_prefix("done:")?;
    // Digits only: no sign, no spaces, and a length a task id can have.
    (!id.is_empty() && id.len() <= 18 && id.chars().all(|c| c.is_ascii_digit())).then(|| id.parse().ok()).flatten().map(Do::Done)
}

fn esc(s: &str) -> String {
    s.chars()
        .filter(|c| !c.is_control() || matches!(c, '\n' | '\t'))
        .map(|c| match c {
            '&' => "&amp;".to_string(),
            '<' => "&lt;".to_string(),
            '>' => "&gt;".to_string(),
            '"' => "&quot;".to_string(),
            '\'' => "&apos;".to_string(),
            c => c.to_string(),
        })
        .collect()
}

/// The toast for a due reminder.
pub fn reminder_xml(title: &str, task_id: i64) -> String {
    let title: String = title.chars().take(200).collect();
    format!(
        concat!(
            r#"<toast scenario="reminder" launch="open" activationType="foreground">"#,
            r#"<visual><binding template="ToastGeneric"><text>Reminder</text><text>{title}</text></binding></visual>"#,
            r#"<actions>"#,
            r#"<input id="snooze" type="selection" defaultInput="10">"#,
            r#"<selection id="5" content="5 minutes"/><selection id="10" content="10 minutes"/><selection id="60" content="1 hour"/>"#,
            r#"</input>"#,
            r#"<action content="Snooze" arguments="snooze" hint-inputId="snooze" activationType="system"/>"#,
            r#"<action content="Done" arguments="done:{id}" activationType="foreground"/>"#,
            r#"<action content="Dismiss" arguments="dismiss" activationType="system"/>"#,
            r#"</actions></toast>"#
        ),
        title = esc(&title),
        id = task_id
    )
}

/// Does what a button asked. Returns a short line for the log.
pub fn apply<Tz: TimeZone>(db: &Db, what: &Do, now: &DateTime<Tz>) -> AppResult<String> {
    match what {
        Do::Done(id) => {
            let t = crate::tasks::set_done(db, *id, true, now)?;
            Ok(if t.done_at.is_some() { format!("Marked \"{}\" done", t.title) } else { format!("\"{}\" moved to its next time", t.title) })
        }
        Do::Open => Ok("Opened BYTE".into()),
    }
}

/// A due reminder, as a toast with buttons where the system can, else as a plain notification.
pub fn reminder(app: &tauri::AppHandle, task: &crate::tasks::Task) {
    #[cfg(windows)]
    {
        if win::show(app, &reminder_xml(&task.title, task.id)).is_ok() {
            return;
        }
    }
    crate::scheduler::notify(app, "Reminder", &task.title);
}

#[cfg(windows)]
pub(crate) mod win {
    use std::sync::{Arc, Mutex, OnceLock};

    use tauri::{AppHandle, Manager};
    use windows::core::{Interface, HSTRING};
    use windows::Data::Xml::Dom::XmlDocument;
    use windows::Foundation::TypedEventHandler;
    use windows::UI::Notifications::{ToastActivatedEventArgs, ToastNotification, ToastNotificationManager};

    use super::*;

    /// Toasts still on screen or in the notification list: their buttons only work while the object lives.
    static ALIVE: Mutex<Vec<ToastNotification>> = Mutex::new(Vec::new());

    /// Makes Windows show "BYTE" as the sender (an unpackaged app has to say who it is). Once per run.
    fn register() {
        static ONCE: OnceLock<()> = OnceLock::new();
        ONCE.get_or_init(|| {
            use std::os::windows::process::CommandExt;
            let key = format!(r"HKCU\Software\Classes\AppUserModelId\{APP_ID}");
            let _ = std::process::Command::new("reg").args(["add", &key, "/v", "DisplayName", "/t", "REG_SZ", "/d", "BYTE", "/f"]).creation_flags(0x0800_0000).output();
        });
    }

    /// Shows a toast made from `xml`; a click on an action calls `on_arg` with that action's argument.
    pub(crate) fn show_with(xml: &str, tag: &str, on_arg: Arc<dyn Fn(String) + Send + Sync>) -> windows::core::Result<()> {
        register();
        let doc = XmlDocument::new()?;
        doc.LoadXml(&HSTRING::from(xml))?;
        let toast = ToastNotification::CreateToastNotification(&doc)?;
        if !tag.is_empty() {
            toast.SetTag(&HSTRING::from(tag))?;
        }
        toast.Activated(&TypedEventHandler::<ToastNotification, windows::core::IInspectable>::new(move |_, args| {
            if let Some(args) = args.as_ref() {
                if let Ok(a) = args.cast::<ToastActivatedEventArgs>() {
                    if let Ok(s) = a.Arguments() {
                        on_arg(s.to_string());
                    }
                }
            }
            Ok(())
        }))?;
        let notifier = ToastNotificationManager::CreateToastNotifierWithId(&HSTRING::from(APP_ID))?;
        notifier.Show(&toast)?;
        if let Ok(mut alive) = ALIVE.lock() {
            alive.push(toast);
            let n = alive.len();
            if n > 40 {
                alive.drain(..n - 40);
            }
        }
        Ok(())
    }

    /// A reminder toast whose Done button marks the task done in BYTE's own tasks.
    pub(crate) fn show(app: &AppHandle, xml: &str) -> windows::core::Result<()> {
        let app = app.clone();
        show_with(
            xml,
            "",
            Arc::new(move |arg| {
                let Some(what) = parse_arg(&arg) else { return };
                let state = app.state::<crate::state::AppState>();
                match apply(&state.db, &what, &chrono::Local::now()) {
                    Ok(line) => log::info!("toast: {line}"),
                    Err(e) => log::warn!("toast: {e}"),
                }
                if what == Do::Open {
                    if let Some(w) = app.get_webview_window("main") {
                        let _ = w.show();
                        let _ = w.set_focus();
                    }
                }
            }),
        )
    }

    /// Whether a toast with this tag is in the notification list (for the live test).
    #[cfg(test)]
    pub(crate) fn in_list(tag: &str) -> bool {
        use windows::UI::Notifications::ToastNotificationManager;
        ToastNotificationManager::History()
            .and_then(|h| h.GetHistoryWithId(&HSTRING::from(APP_ID)))
            .map(|list| list.into_iter().any(|t| t.Tag().map(|x| x.to_string() == tag).unwrap_or(false)))
            .unwrap_or(false)
    }
}

#[cfg(test)]
#[path = "toast_tests.rs"]
mod tests;
