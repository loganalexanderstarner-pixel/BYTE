//! Form helper: fill the text boxes of a form in the window in front from a profile the person saved here (name, email, phone,
//! address). It shows what it will fill first, types only after the person agrees, goes through PC control's own executor (logged,
//! Stop works), and NEVER fills or even holds a password, a card number, a security code or other secret: the profile has no place
//! for them, values that look like card numbers are refused, and boxes whose label or type says password/card/PIN are skipped.
//! It never presses Submit.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::control::uia::{self, Element, Kind};

/// The fields a profile can hold: (key, label shown to the person).
pub const FIELDS: &[(&str, &str)] = &[
    ("full_name", "Full name"),
    ("first_name", "First name"),
    ("last_name", "Last name"),
    ("email", "Email"),
    ("phone", "Phone"),
    ("street", "Street address"),
    ("city", "City"),
    ("state", "State or region"),
    ("postal_code", "Postal code"),
    ("country", "Country"),
    ("company", "Company"),
];

pub type Profile = BTreeMap<String, String>;

/// Is this number string a payment card number (13 to 19 digits that pass the Luhn check)?
pub fn looks_like_card(s: &str) -> bool {
    let digits: Vec<u32> = s.chars().filter(|c| c.is_ascii_digit()).filter_map(|c| c.to_digit(10)).collect();
    if !(13..=19).contains(&digits.len()) || s.chars().any(|c| c.is_alphabetic()) {
        return false;
    }
    let sum: u32 = digits.iter().rev().enumerate().map(|(i, &d)| if i % 2 == 1 { let x = d * 2; if x > 9 { x - 9 } else { x } } else { d }).sum();
    sum % 10 == 0
}

/// Keeps only the known fields, tidied; a value that looks like a card number or mentions a password is dropped and named in the
/// second part.
pub fn clean_profile(raw: &Profile) -> (Profile, Vec<String>) {
    let mut out = Profile::new();
    let mut refused = Vec::new();
    for (key, _) in FIELDS {
        let Some(v) = raw.get(*key) else { continue };
        let v: String = v.chars().filter(|c| !c.is_control()).collect::<String>().trim().chars().take(200).collect();
        if v.is_empty() {
            continue;
        }
        if looks_like_card(&v) || v.to_lowercase().contains("password") {
            refused.push((*key).to_string());
            continue;
        }
        out.insert((*key).to_string(), v);
    }
    (out, refused)
}

const NEVER_IN_LABEL: &[&str] = &["user", "login", "log in", "captcha", "coupon", "promo", "search", "card", "cvv", "cvc", "iban", "password", "passcode", "pin ", "ssn", "social security", "security", "secret", "token"];

/// Which profile field a box's label asks for, if any.
pub fn field_for(label: &str) -> Option<&'static str> {
    let l = label.to_lowercase();
    let l = l.trim().trim_end_matches(['*', ':']).trim();
    if l.is_empty() || uia::sensitive_name(l) || NEVER_IN_LABEL.iter().any(|w| l.contains(w)) {
        return None;
    }
    let has = |ws: &[&str]| ws.iter().any(|w| l.contains(w));
    if has(&["email", "e-mail"]) {
        return Some("email");
    }
    if has(&["first name", "given name", "forename"]) {
        return Some("first_name");
    }
    if has(&["last name", "surname", "family name"]) {
        return Some("last_name");
    }
    if has(&["full name", "your name"]) || l == "name" {
        return Some("full_name");
    }
    if has(&["company", "organization", "organisation", "employer"]) {
        return Some("company");
    }
    if has(&["phone", "mobile", "telephone", "cell"]) {
        return Some("phone");
    }
    if has(&["zip", "postal", "postcode"]) {
        return Some("postal_code");
    }
    if has(&["street", "address line 1", "address 1"]) || l == "address" {
        return Some("street");
    }
    if has(&["city", "town"]) {
        return Some("city");
    }
    if has(&["state", "province", "region"]) {
        return Some("state");
    }
    if has(&["country"]) {
        return Some("country");
    }
    None
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Fill {
    pub id: u32,
    pub label: String,
    pub field: &'static str,
    pub value: String,
}

/// The boxes to fill: text boxes only, never a sensitive one, and only for fields the profile has.
pub fn matches(elements: &[Element], profile: &Profile) -> Vec<Fill> {
    elements
        .iter()
        .filter(|e| e.kind == Kind::Edit && e.enabled && !e.sensitive)
        .filter_map(|e| {
            let field = field_for(&e.name)?;
            let value = profile.get(field)?.clone();
            Some(Fill { id: e.id, label: e.name.trim().to_string(), field, value })
        })
        .take(30)
        .collect()
}

pub fn describe(fills: &[Fill]) -> String {
    fills.iter().map(|f| format!("{}: {}", f.label, f.value)).collect::<Vec<_>>().join("\n")
}

pub const FILE: &str = "form_profile.json";

pub fn load(path: &std::path::Path) -> Profile {
    std::fs::read_to_string(path).ok().and_then(|t| serde_json::from_str(&t).ok()).map(|p: Profile| clean_profile(&p).0).unwrap_or_default()
}

pub fn save(path: &std::path::Path, p: &Profile) -> std::io::Result<()> {
    std::fs::write(path, serde_json::to_string_pretty(p).unwrap_or_default())
}

// ---------------------------------------------------------------- commands

#[cfg(desktop)]
fn file(app: &tauri::AppHandle) -> std::path::PathBuf {
    use tauri::Manager;
    app.state::<crate::state::AppState>().paths.data.join(FILE)
}

/// The saved profile (known fields only), and the list of fields for the form.
#[tauri::command]
pub fn form_profile_get(app: tauri::AppHandle) -> (Profile, Vec<(String, String)>) {
    (load(&file(&app)), FIELDS.iter().map(|(k, l)| (k.to_string(), l.to_string())).collect())
}

/// Saves the profile. Returns the fields that were refused (card numbers, passwords).
#[tauri::command]
pub fn form_profile_set(app: tauri::AppHandle, profile: Profile) -> Result<Vec<String>, crate::error::AppError> {
    let (clean, refused) = clean_profile(&profile);
    save(&file(&app), &clean)?;
    Ok(refused)
}

// ---------------------------------------------------------------- the flow

#[cfg(desktop)]
static PENDING: std::sync::Mutex<Option<(isize, String, Vec<Fill>)>> = std::sync::Mutex::new(None);

#[cfg(desktop)]
fn close(app: &tauri::AppHandle, body: String) {
    use super::popup::{self, Kind, Payload};
    popup::show(app, Kind::PageTools, Payload { title: "Fill this form".into(), body, buttons: vec![popup::button("close", "Close")], auto_hide: 12_000, ..Default::default() }, None);
}

/// Looks at the window in front (below BYTE), lists what it would fill, and waits for "Fill them".
#[cfg(desktop)]
pub async fn offer(app: &tauri::AppHandle) {
    use super::popup::{self, Kind, Payload};
    use crate::control::uia::Screen;
    use tauri::Manager;
    let state = app.state::<crate::state::AppState>();
    let (excludes, on) = {
        let s = state.settings.lock().await;
        (s.screen_exclude_apps.clone(), s.helper_form && s.pc_control && !s.kids_mode)
    };
    if !on {
        return close(app, "Form filling needs PC control to be on as well (Settings > Control and screen).".into());
    }
    let profile = load(&file(app));
    if profile.is_empty() {
        return close(app, "Save your details first: Settings > Control and screen > Everyday helpers > Form helper.".into());
    }
    let screen: std::sync::Arc<dyn Screen> = crate::control::real_screen().into();
    let s2 = screen.clone();
    let wins = tokio::task::spawn_blocking(move || s2.windows()).await.unwrap_or_default();
    let Some(target) = uia::pick_target(&wins).cloned() else { return close(app, "BYTE couldn't find a window with a form.".into()) };
    if let Err(why) = crate::control::guide::allowed(&target.app, &target.title, &excludes) {
        return close(app, why);
    }
    let id = target.id;
    let snap = match tokio::task::spawn_blocking(move || screen.snapshot_window(id)).await {
        Ok(Ok(s)) => s,
        _ => return close(app, "BYTE couldn't read that window.".into()),
    };
    if let Err(why) = crate::control::guide::allowed(&snap.app, &snap.title, &excludes) {
        return close(app, why);
    }
    let fills = matches(&snap.elements, &profile);
    if fills.is_empty() {
        return close(app, "BYTE found no boxes it knows how to fill in that window (it fills name, email, phone and address boxes; never passwords or cards).".into());
    }
    let body = format!("BYTE will type this into \"{}\" (it will not submit anything, and never touches password or card boxes):\n\n{}", snap.title.trim(), describe(&fills));
    if let Ok(mut p) = PENDING.lock() {
        *p = Some((target.id, snap.title.clone(), fills));
    }
    popup::show(app, Kind::PageTools, Payload { title: "Fill this form".into(), body, buttons: vec![popup::button("fill", "Fill them"), popup::button("close", "Cancel")], ..Default::default() }, None);
}

/// "Fill them": a switch to the window, then for each box a click, select-all and the typed value.
#[cfg(desktop)]
pub async fn fill(app: &tauri::AppHandle) {
    use crate::control::actions::{Action, Expect, Step};
    let Some((_, title, fills)) = PENDING.lock().ok().and_then(|mut p| p.take()) else { return close(app, "That offer has gone. Start again.".into()) };
    let mut steps = vec![Step { action: Action::Focus { title: title.chars().take(200).collect() }, expect: Expect::default() }];
    for f in &fills {
        steps.push(Step { action: Action::Click { id: f.id }, expect: Expect::default() });
        steps.push(Step { action: Action::Key { combo: "ctrl+a".into() }, expect: Expect::default() });
        steps.push(Step { action: Action::Type { text: f.value.clone() }, expect: Expect::default() });
    }
    let cancel = tokio_util::sync::CancellationToken::new();
    let send = |_e: crate::chat::ChatEvent| -> crate::error::AppResult<()> { Ok(()) };
    let report = crate::control::run_plan_steps(app, steps, "Fill this form", &cancel, &send).await;
    close(app, if report.ok { format!("Filled {} box{}. Check them, then submit the form yourself.", fills.len(), if fills.len() == 1 { "" } else { "es" }) } else { report.summary });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::control::uia::Rect;

    fn edit(id: u32, name: &str) -> Element {
        Element { id, name: name.into(), kind: Kind::Edit, enabled: true, rect: Rect { left: 0, top: 0, right: 10, bottom: 10 }, sensitive: false }
    }

    fn profile() -> Profile {
        [("full_name", "Sam Lee"), ("email", "sam@example.com"), ("phone", "555 0100"), ("street", "1 Main St"), ("city", "Springfield"), ("postal_code", "12345"), ("country", "USA")].iter().map(|(k, v)| (k.to_string(), v.to_string())).collect()
    }

    #[test]
    fn labels_map_to_the_right_field() {
        for (l, f) in [("Email address", "email"), ("E-mail *", "email"), ("First name", "first_name"), ("Surname", "last_name"), ("Full name", "full_name"), ("Name", "full_name"), ("Mobile phone", "phone"), ("Address", "street"), ("Street address", "street"), ("ZIP code", "postal_code"), ("Postcode", "postal_code"), ("City", "city"), ("Country", "country"), ("Company name", "company")] {
            assert_eq!(field_for(l), Some(f), "{l}");
        }
    }

    #[test]
    fn secrets_and_look_alikes_are_never_matched() {
        for l in ["Password", "Confirm password", "Card number", "CVV", "Security code", "Username", "User name", "Login", "PIN", "Social security number", "Search", "Promo code", "Account number", "Enter the captcha", ""] {
            assert_eq!(field_for(l), None, "{l}");
        }
    }

    #[test]
    fn only_text_boxes_with_a_known_label_and_a_saved_value_are_filled() {
        let mut secret = edit(3, "Email");
        secret.sensitive = true;
        let mut off = edit(4, "City");
        off.enabled = false;
        let mut button = edit(5, "Email");
        button.kind = Kind::Button;
        let els = vec![edit(1, "Email"), edit(2, "Password"), secret, off, button, edit(6, "Company"), edit(7, "Phone")];
        let f = matches(&els, &profile());
        assert_eq!(f.iter().map(|x| (x.id, x.field)).collect::<Vec<_>>(), vec![(1, "email"), (7, "phone")]);
        assert!(describe(&f).contains("Email: sam@example.com"));
    }

    #[test]
    fn card_numbers_and_passwords_cannot_be_saved() {
        assert!(looks_like_card("4111 1111 1111 1111"));
        assert!(looks_like_card("4111-1111-1111-1111"));
        assert!(!looks_like_card("4111 1111 1111 1112"));
        assert!(!looks_like_card("555 0100"));
        assert!(!looks_like_card("1234567890123456789012"));
        let mut raw = profile();
        raw.insert("phone".into(), "4111 1111 1111 1111".into());
        raw.insert("company".into(), "my password is hunter2".into());
        raw.insert("card_number".into(), "4111111111111111".into());
        raw.insert("password".into(), "hunter2".into());
        let (clean, refused) = clean_profile(&raw);
        assert!(!clean.contains_key("phone") && !clean.contains_key("company") && !clean.contains_key("card_number") && !clean.contains_key("password"));
        assert_eq!(refused, vec!["phone", "company"]);
        assert_eq!(clean.get("email").map(String::as_str), Some("sam@example.com"));
    }

    #[test]
    fn the_profile_file_round_trips_and_is_cleaned_on_read() {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join(FILE);
        save(&p, &profile()).unwrap();
        assert_eq!(load(&p), profile());
        std::fs::write(&p, r#"{"email":"a@b.c","password":"x","phone":"4111111111111111"}"#).unwrap();
        let l = load(&p);
        assert_eq!(l.len(), 1);
        assert!(load(&d.path().join("none.json")).is_empty());
    }

    #[test]
    fn no_field_is_for_a_secret() {
        for (k, _) in FIELDS {
            for bad in ["pass", "card", "cvv", "pin", "secret", "ssn", "token"] {
                assert!(!k.contains(bad), "{k}");
            }
        }
    }
}
