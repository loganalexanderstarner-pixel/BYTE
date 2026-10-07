//! Kids mode: a simple BYTE for children. No web, no Mac control, files,
//! terminal, connectors or automations, answers written for kids, and a
//! simpler screen. Turning it off needs the PIN set when it was turned on.
//!
//! It's enforced where answers are made (`backend`), not just hidden in the
//! UI, and settings can't be changed while it's on. The PIN is stored salted
//! and hashed (PBKDF2), never as typed. It's a guard against a child changing
//! things, not against someone who can read files on the Mac.

use std::sync::atomic::{AtomicBool, AtomicI64, AtomicU32, Ordering};

use ring::rand::{SecureRandom, SystemRandom};
use tauri::State;

use crate::error::{AppError, AppResult};
use crate::settings::Settings;
use crate::state::AppState;

/// Whether kids mode is on, for the quick chat commands (set at launch and on enter/exit).
static ACTIVE: AtomicBool = AtomicBool::new(false);

/// Chats made in kids mode live in this folder; kids mode sees only these.
pub const FOLDER: &str = "Kids";

pub fn is_on() -> bool {
    ACTIVE.load(Ordering::Relaxed)
}

pub fn set(on: bool) {
    ACTIVE.store(on, Ordering::Relaxed);
}

/// Err in kids mode: for grown-up data (notes, memories, boards, the activity log, backups…).
pub fn grownups_only() -> AppResult<()> {
    if is_on() {
        Err(AppError::msg("That isn't available in kids mode."))
    } else {
        Ok(())
    }
}

/// In kids mode, only chats in the Kids folder (or brand new ones) may be opened or changed.
pub fn may_touch(db: &crate::db::Db, id: &str) -> AppResult<()> {
    if !is_on() {
        return Ok(());
    }
    match db.list()?.into_iter().find(|c| c.id == id) {
        Some(c) if c.folder.as_deref() != Some(FOLDER) => Err(AppError::msg("That chat isn't available in kids mode.")),
        _ => Ok(()),
    }
}

const ROUNDS: u32 = if cfg!(test) { 1_000 } else { 100_000 };

/// Added to the system prompt in kids mode.
pub const PROMPT: &str = "\n\n## Talking with a child\nThe person you're talking with is a child. Use simple words and short sentences, be warm and encouraging, and explain things with everyday examples. Keep everything suitable for kids: no violence, scary or adult content, no dangerous instructions, and no personal information requests. If they ask about something that isn't for kids, gently say it's a question for a grown-up they trust and suggest something fun to talk about instead. If they seem upset, hurt or in danger, kindly tell them to talk to a parent, teacher or another trusted adult right away. For homework, help them think it through step by step instead of just giving the answer.";

/// Settings a child may still change in kids mode (looks only; kids mode answers on this Mac anyway).
pub fn harmless(patch: &serde_json::Value) -> bool {
    const OK: [&str; 5] = ["theme", "density", "fontScale", "workspace", "useCloud"];
    patch.as_object().is_some_and(|o| o.keys().all(|k| OK.contains(&k.as_str())))
}

fn hex(b: &[u8]) -> String {
    b.iter().map(|x| format!("{x:02x}")).collect()
}

fn unhex(s: &str) -> Option<Vec<u8>> {
    (s.len() % 2 == 0).then(|| (0..s.len()).step_by(2).map(|i| u8::from_str_radix(&s[i..i + 2], 16).ok()).collect::<Option<Vec<u8>>>()).flatten()
}

fn valid_pin(pin: &str) -> bool {
    (4..=6).contains(&pin.len()) && pin.chars().all(|c| c.is_ascii_digit())
}

/// "pbkdf2$<rounds>$<salt>$<hash>".
pub fn hash_pin(pin: &str) -> AppResult<String> {
    if !valid_pin(pin) {
        return Err(AppError::msg("The PIN is 4 to 6 digits."));
    }
    let mut salt = [0u8; 16];
    SystemRandom::new().fill(&mut salt).map_err(|_| AppError::msg("No randomness available."))?;
    let mut out = [0u8; 32];
    ring::pbkdf2::derive(ring::pbkdf2::PBKDF2_HMAC_SHA256, std::num::NonZeroU32::new(ROUNDS).unwrap(), &salt, pin.as_bytes(), &mut out);
    Ok(format!("pbkdf2${ROUNDS}${}${}", hex(&salt), hex(&out)))
}

pub fn check_pin(pin: &str, stored: &str) -> bool {
    let parts: Vec<&str> = stored.split('$').collect();
    let [kind, rounds, salt, hash] = parts.as_slice() else { return false };
    let (Some(rounds), Some(salt), Some(hash)) = (rounds.parse::<u32>().ok().and_then(std::num::NonZeroU32::new), unhex(salt), unhex(hash)) else { return false };
    *kind == "pbkdf2" && ring::pbkdf2::verify(ring::pbkdf2::PBKDF2_HMAC_SHA256, rounds, &salt, pin.as_bytes(), &hash).is_ok()
}

/// Wrong PINs in a row, and until when the next try has to wait.
static WRONG: AtomicU32 = AtomicU32::new(0);
static WAIT_UNTIL: AtomicI64 = AtomicI64::new(0);

fn now() -> i64 {
    chrono::Utc::now().timestamp()
}

/// After 5 wrong PINs, a 30-second wait (doubling after more).
fn note_attempt(ok: bool, at: i64) {
    if ok {
        WRONG.store(0, Ordering::Relaxed);
        WAIT_UNTIL.store(0, Ordering::Relaxed);
        return;
    }
    let n = WRONG.fetch_add(1, Ordering::Relaxed) + 1;
    if n >= 5 {
        WAIT_UNTIL.store(at + 30 * (1i64 << (n - 5).min(6)), Ordering::Relaxed);
    }
}

/// Uncensored and "obliterated" models, and every other community remix (role-play and dark-story fine-tunes
/// among them), never run in kids mode: kids get the makers' own models. They all stay in the catalog for
/// grown-ups.
pub fn grown_up_model(catalog: &crate::models::Catalog, key: &str) -> bool {
    catalog.resolve(key).is_ok_and(|(m, _)| m.is_community() || m.tags.iter().any(|t| t == "uncensored"))
}

pub const GROWN_UP_MODEL: &str = "This model is for grown-ups only, so it can't answer in kids mode. A grown-up can pick a regular model in Settings → Models.";

/// Turns kids mode on with a PIN (needed to turn it off again).
#[tauri::command]
pub async fn kids_enter(state: State<'_, AppState>, pin: String) -> AppResult<Settings> {
    crate::lock::ensure(&state)?;
    if let Some(l) = state.engine.loaded().await {
        if grown_up_model(&state.catalog.get(), &l.key) {
            return Err(AppError::msg("The model in use is for grown-ups only (an uncensored or community model). Pick a regular model in Settings → Models first, then turn on kids mode."));
        }
    }
    let mut s = state.settings.lock().await;
    s.kids_pin = Some(hash_pin(pin.trim())?);
    s.kids_mode = true;
    s.save(&state.paths.settings_file)?;
    set(true);
    crate::messages::sync_background(false);
    Ok(s.clone())
}

/// Turns kids mode off, with the PIN.
#[tauri::command]
pub async fn kids_exit(state: State<'_, AppState>, pin: String) -> AppResult<Settings> {
    let wait = WAIT_UNTIL.load(Ordering::Relaxed) - now();
    if wait > 0 {
        return Err(AppError::msg(format!("Too many wrong PINs. Try again in {wait} seconds.")));
    }
    let mut s = state.settings.lock().await;
    if !s.kids_mode {
        return Ok(s.clone());
    }
    let ok = s.kids_pin.as_deref().is_some_and(|h| check_pin(pin.trim(), h));
    note_attempt(ok, now());
    if !ok {
        return Err(AppError::msg("That PIN isn't right."));
    }
    s.kids_mode = false;
    s.save(&state.paths.settings_file)?;
    set(false);
    crate::messages::sync_background(s.messages_inbox && s.messages_notify);
    Ok(s.clone())
}

#[cfg(test)]
mod tests {
    #[test]
    fn uncensored_models_are_grown_ups_only() {
        let catalog = crate::models::Catalog::embedded();
        let adult = catalog.models.iter().find(|m| m.tags.iter().any(|t| t == "uncensored")).expect("an uncensored model");
        let key = format!("{}:{}", adult.id, adult.variants[0].quant);
        assert!(super::grown_up_model(&catalog, &key));
        let regular = catalog.models.iter().find(|m| m.id == "qwen3.5-9b").unwrap();
        assert!(!super::grown_up_model(&catalog, &format!("qwen3.5-9b:{}", regular.variants[0].quant)));
        assert!(!super::grown_up_model(&catalog, "not-a-model:Q4"));
        // Role-play and dark-story remixes too, though not tagged uncensored.
        let remix = catalog.models.iter().find(|m| m.id == "community-l3-dark-planet-8b").unwrap();
        assert!(super::grown_up_model(&catalog, &format!("{}:{}", remix.id, remix.variants[0].quant)));
    }

    use super::*;

    #[test]
    fn pins_are_hashed_and_checked() {
        let h = hash_pin("4821").unwrap();
        assert!(h.starts_with("pbkdf2$") && !h.contains("4821"));
        assert!(check_pin("4821", &h));
        assert!(!check_pin("4822", &h));
        assert!(!check_pin("4821", "garbage"));
        assert!(!check_pin("4821", "pbkdf2$0$00$00"));
        assert_ne!(hash_pin("4821").unwrap(), h, "salted");
        for bad in ["123", "1234567", "12a4", ""] {
            assert!(hash_pin(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn many_wrong_pins_mean_a_wait() {
        WRONG.store(0, Ordering::Relaxed);
        WAIT_UNTIL.store(0, Ordering::Relaxed);
        for _ in 0..4 {
            note_attempt(false, 1_000);
        }
        assert_eq!(WAIT_UNTIL.load(Ordering::Relaxed), 0);
        note_attempt(false, 1_000);
        assert_eq!(WAIT_UNTIL.load(Ordering::Relaxed), 1_030);
        note_attempt(false, 1_100);
        assert_eq!(WAIT_UNTIL.load(Ordering::Relaxed), 1_160);
        note_attempt(true, 1_200);
        assert_eq!(WAIT_UNTIL.load(Ordering::Relaxed), 0);
    }

    #[test]
    fn only_looks_can_change_in_kids_mode() {
        assert!(harmless(&serde_json::json!({ "theme": "paper", "fontScale": 1.2 })));
        assert!(!harmless(&serde_json::json!({ "theme": "paper", "webSearch": true })));
        assert!(!harmless(&serde_json::json!({ "offline": false })));
        assert!(!harmless(&serde_json::json!("x")));
    }

    #[test]
    fn the_prompt_keeps_it_kid_friendly() {
        assert!(PROMPT.contains("child") && PROMPT.contains("trusted adult"));
    }
}
