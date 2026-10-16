//! Smart paste toast: when the clipboard gets a tracking number, a street address, a link or a one-time code, a small toast
//! offers the useful action (track it, show it on a map, open or clean the link, clear the code). It works on this computer
//! only: nothing is looked up and no model is asked; the only thing that ever goes out is the page the person chooses to open.
//!
//! Never for anything that looks like a password or a key (`clipboard::looks_secret`, the same filter the clipboard history
//! uses, plus the password managers' "do not record" flag), and never for long or many-line text.

use crate::clipboard::looks_secret;

/// What was recognised.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Kind {
    Tracking { carrier: &'static str },
    Address,
    Link { tracked: bool },
    Code,
}

/// One button on the toast.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Act {
    pub id: &'static str,
    pub label: &'static str,
    /// A page to open (for the open/track/map buttons).
    pub url: Option<String>,
    /// The text to put on the clipboard (clean link).
    pub replace: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Smart {
    pub kind: Kind,
    pub title: String,
    pub body: String,
    pub acts: Vec<Act>,
}

/// Longest text looked at.
const MAX_CHARS: usize = 300;

fn enc(s: &str) -> String {
    url::form_urlencoded::byte_serialize(s.as_bytes()).collect()
}

fn all_digits(s: &str) -> bool {
    !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit())
}

/// A parcel number, and who carries it. Only shapes that are unlikely to be anything else: a phone number or an order number
/// is not matched.
pub fn tracking(t: &str) -> Option<(&'static str, String)> {
    let c: String = t.chars().filter(|c| !c.is_whitespace()).collect();
    let up = c.to_uppercase();
    if !up.is_ascii() {
        return None;
    }
    let b = up.as_bytes();
    // UPS: 1Z + 16 letters/digits.
    if up.len() == 18 && up.starts_with("1Z") && up[2..].bytes().all(|x| x.is_ascii_alphanumeric()) {
        return Some(("UPS", up));
    }
    // Royal Mail / international post: 2 letters, 9 digits, 2 letters.
    if up.len() == 13 && b[..2].iter().all(u8::is_ascii_uppercase) && all_digits(&up[2..11]) && b[11..].iter().all(u8::is_ascii_uppercase) {
        return Some(("Royal Mail", up));
    }
    // Amazon Logistics.
    if up.len() == 15 && up.starts_with("TBA") && all_digits(&up[3..]) {
        return Some(("Amazon", up));
    }
    // USPS: 20 to 22 digits starting 92, 93, 94 or 95 (or 9 + more).
    if (20..=22).contains(&up.len()) && all_digits(&up) && matches!(&up[..2], "92" | "93" | "94" | "95") {
        return Some(("USPS", up));
    }
    // FedEx: 12 or 15 digits (only when the text is nothing but the number, spaces allowed in groups).
    if matches!(up.len(), 12 | 15) && all_digits(&up) && !t.contains(['-', '(', ')', '+', '.']) {
        return Some(("FedEx", up));
    }
    None
}

pub fn tracking_url(carrier: &str, number: &str) -> String {
    let n = enc(number);
    match carrier {
        "UPS" => format!("https://www.ups.com/track?tracknum={n}"),
        "USPS" => format!("https://tools.usps.com/go/TrackConfirmAction?tLabels={n}"),
        "FedEx" => format!("https://www.fedex.com/fedextrack/?trknbr={n}"),
        "Royal Mail" => format!("https://www.royalmail.com/track-your-item#/tracking-results/{n}"),
        _ => format!("https://www.google.com/search?q={}", enc(&format!("{carrier} {number} track"))),
    }
}

const STREET_WORDS: &[&str] = &[
    "st", "street", "ave", "avenue", "rd", "road", "blvd", "boulevard", "dr", "drive", "ln", "lane", "ct", "court", "way", "pl", "place", "pkwy", "parkway", "hwy", "highway", "cir", "circle", "ter", "terrace",
];

/// A street address: a house number, a few words ending in a street word, optionally a city, state and postcode on the
/// same or the next line. Not a sentence, not a list.
pub fn address(t: &str) -> bool {
    let lines: Vec<&str> = t.lines().map(str::trim).filter(|l| !l.is_empty()).collect();
    if lines.is_empty() || lines.len() > 3 || t.chars().count() > 140 {
        return false;
    }
    let first = lines[0];
    let words: Vec<&str> = first.split(|c: char| c.is_whitespace() || c == ',').filter(|w| !w.is_empty()).collect();
    if words.len() < 3 || words.len() > 12 {
        return false;
    }
    let number = words[0];
    let digits = number.chars().take_while(char::is_ascii_digit).count();
    if digits == 0 || digits > 6 || number.len() > digits + 1 {
        return false;
    }
    let ends_with_street = words.iter().skip(1).any(|w| STREET_WORDS.contains(&w.trim_end_matches('.').to_lowercase().as_str()));
    // A sentence ("2 cups of sugar in the bowl.") is not an address.
    let sentence = first.ends_with(['.', '!', '?']) && !first.split_whitespace().last().is_some_and(|w| STREET_WORDS.contains(&w.trim_end_matches('.').to_lowercase().as_str()));
    ends_with_street && !sentence
}

/// Query parameters that only follow the person around.
pub const TRACKING_PARAMS: &[&str] = &["fbclid", "gclid", "gclsrc", "dclid", "msclkid", "mc_eid", "mc_cid", "igshid", "yclid", "_hsenc", "_hsmi", "ref_src", "ref_url", "si", "spm", "oly_enc_id", "oly_anon_id", "vero_id", "wickedid"];

fn is_tracking_param(k: &str) -> bool {
    let k = k.to_lowercase();
    k.starts_with("utm_") || TRACKING_PARAMS.contains(&k.as_str())
}

/// The link without its tracking parameters, and whether anything was removed.
pub fn clean_link(link: &str) -> Option<(String, bool)> {
    let mut u = url::Url::parse(link).ok()?;
    if !matches!(u.scheme(), "http" | "https") || u.host_str().is_none() {
        return None;
    }
    let pairs: Vec<(String, String)> = u.query_pairs().map(|(k, v)| (k.into_owned(), v.into_owned())).collect();
    let kept: Vec<&(String, String)> = pairs.iter().filter(|(k, _)| !is_tracking_param(k)).collect();
    let removed = kept.len() != pairs.len();
    if removed {
        if kept.is_empty() {
            u.set_query(None);
        } else {
            u.query_pairs_mut().clear().extend_pairs(kept.iter().map(|(k, v)| (k.as_str(), v.as_str())));
        }
    }
    Some((u.to_string(), removed))
}

/// A one-time code: 4 to 8 digits on their own, or "your code is 123456" style text.
pub fn one_time_code(t: &str) -> Option<String> {
    let s = t.trim();
    if (4..=8).contains(&s.len()) && all_digits(s) {
        return Some(s.to_string());
    }
    if s.chars().count() > 120 || s.lines().count() > 2 {
        return None;
    }
    let l = s.to_lowercase();
    if !["code", "otp", "passcode", "pin"].iter().any(|w| l.split(|c: char| !c.is_alphanumeric()).any(|x| x == *w)) {
        return None;
    }
    // The digits that follow the word, 4 to 8 of them.
    s.split(|c: char| !c.is_ascii_digit()).find(|d| (4..=8).contains(&d.len())).map(str::to_string)
}

/// What the clipboard text is, and the toast for it. `secret_flag`: a password manager marked it "do not record".
pub fn classify(text: &str, secret_flag: bool) -> Option<Smart> {
    let t = text.trim();
    if secret_flag || t.is_empty() || t.chars().count() > MAX_CHARS || looks_secret(t) {
        return None;
    }
    if let Some((carrier, number)) = tracking(t) {
        return Some(Smart {
            kind: Kind::Tracking { carrier },
            title: format!("{carrier} tracking number"),
            body: number.clone(),
            acts: vec![Act { id: "open", label: "Track it", url: Some(tracking_url(carrier, &number)), replace: None }, Act { id: "close", label: "Not now", url: None, replace: None }],
        });
    }
    if !t.contains(char::is_whitespace) {
        if let Some((cleaned, removed)) = clean_link(t) {
            let mut acts = vec![Act { id: "open", label: "Open", url: Some(t.to_string()), replace: None }];
            if removed {
                acts.push(Act { id: "clean", label: "Copy a clean link", url: None, replace: Some(cleaned) });
            }
            acts.push(Act { id: "close", label: "Not now", url: None, replace: None });
            let host = url::Url::parse(t).ok().and_then(|u| u.host_str().map(str::to_string)).unwrap_or_default();
            return Some(Smart { kind: Kind::Link { tracked: removed }, title: if removed { "Link with tracking in it".into() } else { "Link".into() }, body: host, acts });
        }
    }
    if address(t) {
        let one = t.lines().map(str::trim).filter(|l| !l.is_empty()).collect::<Vec<_>>().join(", ");
        return Some(Smart {
            kind: Kind::Address,
            title: "Address".into(),
            body: one.clone(),
            acts: vec![Act { id: "open", label: "Show on a map", url: Some(format!("https://www.openstreetmap.org/search?query={}", enc(&one))), replace: None }, Act { id: "close", label: "Not now", url: None, replace: None }],
        });
    }
    if one_time_code(t).is_some() {
        return Some(Smart {
            kind: Kind::Code,
            title: "That looks like a one-time code".into(),
            body: "Codes are best not left on the clipboard.".into(),
            acts: vec![Act { id: "wipe", label: "Clear the clipboard", url: None, replace: Some(String::new()) }, Act { id: "close", label: "Keep it", url: None, replace: None }],
        });
    }
    None
}

// ---------------------------------------------------------------- the watcher (desktop)

#[cfg(desktop)]
mod watch {
    use super::*;
    use crate::helpers::popup::{self, Kind as PopKind, Payload};
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::Mutex;
    use tauri::{AppHandle, Manager};

    static RUNNING: AtomicBool = AtomicBool::new(false);
    /// The acts of the toast on screen.
    static ACTS: Mutex<Vec<Act>> = Mutex::new(Vec::new());

    pub fn apply(app: &AppHandle, on: bool) {
        if !on {
            RUNNING.store(false, Ordering::SeqCst);
            return;
        }
        if !cfg!(any(windows, target_os = "macos")) || RUNNING.swap(true, Ordering::SeqCst) {
            return;
        }
        let app = app.clone();
        tauri::async_runtime::spawn(async move {
            let mut last = crate::clipboard::board::change_count();
            let mut offered = String::new();
            while RUNNING.load(Ordering::SeqCst) {
                tokio::time::sleep(std::time::Duration::from_millis(800)).await;
                let now = crate::clipboard::board::change_count();
                if now == last {
                    continue;
                }
                last = now;
                // BYTE's own copies, and a selection BYTE is copying on purpose, are not the person copying.
                if now == crate::clipboard::own_write_count() || crate::clipboard::history_paused() {
                    continue;
                }
                if crate::helpers::blocked_now().is_some() {
                    continue;
                }
                let Some(text) = crate::clipboard::board::text() else { continue };
                if text == offered {
                    continue;
                }
                if let Some(smart) = classify(&text, crate::clipboard::board::secret()) {
                    offered = text;
                    show(&app, &smart);
                }
            }
        });
    }

    fn show(app: &AppHandle, smart: &Smart) {
        if let Ok(mut a) = ACTS.lock() {
            *a = smart.acts.clone();
        }
        let monitor = app.primary_monitor().ok().flatten();
        let at = monitor.map(|m| (m.position().x + m.size().width as i32 - 380, m.position().y + m.size().height as i32 - 250)).unwrap_or((1400, 800));
        let buttons = smart.acts.iter().map(|a| popup::button(a.id, a.label)).collect();
        popup::show(app, PopKind::Paste, Payload { title: smart.title.clone(), body: smart.body.clone(), buttons, auto_hide: 12_000, ..Default::default() }, Some(at));
    }

    pub fn clicked(app: &AppHandle, id: &str) {
        let act = ACTS.lock().ok().and_then(|a| a.iter().find(|a| a.id == id).cloned());
        let Some(act) = act else { return popup::hide(app) };
        if let Some(url) = &act.url {
            use tauri_plugin_opener::OpenerExt;
            let _ = app.opener().open_url(url, None::<&str>);
        }
        if let Some(text) = &act.replace {
            crate::clipboard::set_quietly(text);
        }
        popup::hide(app);
        let _ = app.state::<crate::state::AppState>();
    }
}

#[cfg(desktop)]
pub fn apply(app: &tauri::AppHandle, on: bool) {
    watch::apply(app, on);
}
#[cfg(desktop)]
pub async fn clicked(app: &tauri::AppHandle, id: &str) {
    watch::clicked(app, id);
}
#[cfg(not(desktop))]
pub fn apply(_app: &tauri::AppHandle, _on: bool) {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parcel_numbers_are_recognised_by_their_shape() {
        assert_eq!(tracking("1Z999AA10123456784").unwrap().0, "UPS");
        assert_eq!(tracking("  1z 999 aa1 0123 456 784 ").unwrap().0, "UPS");
        assert_eq!(tracking("9400111899223197428490").unwrap().0, "USPS");
        assert_eq!(tracking("794644790138").unwrap().0, "FedEx");
        assert_eq!(tracking("TBA123456789012").unwrap().0, "Amazon");
        assert_eq!(tracking("RB123456785GB").unwrap().0, "Royal Mail");
    }

    #[test]
    fn phone_numbers_dates_and_ordinary_numbers_are_not_parcels() {
        for t in ["555-123-4567", "(555) 123-4567", "+1 555 123 4567", "123456", "2026-10-15", "1234567890", "12.345.678.901", "hello", "99999999999999999999999"] {
            assert!(tracking(t).is_none(), "{t}");
        }
    }

    #[test]
    fn tracking_pages_carry_the_number_encoded() {
        assert_eq!(tracking_url("UPS", "1Z999AA10123456784"), "https://www.ups.com/track?tracknum=1Z999AA10123456784");
        assert!(tracking_url("Amazon", "TBA1 2").contains("TBA1%202") || tracking_url("Amazon", "TBA1 2").contains("TBA1+2"));
    }

    #[test]
    fn street_addresses_are_found_and_sentences_are_not() {
        for a in ["1600 Pennsylvania Ave NW, Washington, DC 20500", "221 Baker Street", "42 Wallaby Way\nSydney NSW 2000", "10 Downing St."] {
            assert!(address(a), "{a}");
        }
        for n in ["2 cups of sugar in the bowl.", "Meet me at the park", "I live on Baker Street", "5 things to do in town"] {
            assert!(!address(n), "{n}");
        }
        assert!(!address(&"1 Main St ".repeat(30)));
    }

    #[test]
    fn links_lose_only_their_tracking_parameters() {
        let (c, removed) = clean_link("https://shop.example.com/item?id=42&utm_source=mail&fbclid=abc&color=red").unwrap();
        assert!(removed);
        assert_eq!(c, "https://shop.example.com/item?id=42&color=red");
        let (c, removed) = clean_link("https://example.com/a?utm_campaign=x").unwrap();
        assert!(removed && c == "https://example.com/a");
        let (c, removed) = clean_link("https://example.com/a?q=rust").unwrap();
        assert!(!removed && c.contains("q=rust"));
        assert!(clean_link("javascript:alert(1)").is_none());
        assert!(clean_link("file:///etc/passwd").is_none());
        assert!(clean_link("not a link").is_none());
    }

    #[test]
    fn one_time_codes_are_short_digit_runs_or_named_codes() {
        assert_eq!(one_time_code("482913").as_deref(), Some("482913"));
        assert_eq!(one_time_code("Your verification code is 482913").as_deref(), Some("482913"));
        assert!(one_time_code("123").is_none());
        assert!(one_time_code("Call me at 5551234567 tomorrow").is_none());
        assert!(one_time_code("I have 482913 apples").is_none());
    }

    #[test]
    fn a_toast_is_built_for_each_kind_with_the_right_action() {
        let s = classify("1Z999AA10123456784", false).unwrap();
        assert_eq!(s.kind, Kind::Tracking { carrier: "UPS" });
        assert!(s.acts[0].url.as_deref().unwrap().starts_with("https://www.ups.com/"));
        let s = classify("https://example.com/p?utm_source=x&id=1", false).unwrap();
        assert_eq!(s.kind, Kind::Link { tracked: true });
        assert!(s.acts.iter().any(|a| a.id == "clean" && a.replace.as_deref() == Some("https://example.com/p?id=1")));
        let s = classify("https://example.com/p?id=1", false).unwrap();
        assert_eq!(s.kind, Kind::Link { tracked: false });
        assert!(!s.acts.iter().any(|a| a.id == "clean"));
        let s = classify("221 Baker Street", false).unwrap();
        assert!(s.acts[0].url.as_deref().unwrap().starts_with("https://www.openstreetmap.org/search?query=221"));
        let s = classify("482913", false).unwrap();
        assert_eq!(s.kind, Kind::Code);
        assert_eq!(s.acts[0].replace.as_deref(), Some(""));
    }

    #[test]
    fn nothing_that_looks_like_a_password_or_a_key_ever_gets_a_toast() {
        // Made in two parts so the repository's secret scan does not mistake these made-up values for real keys.
        let fakes = [("sk", "-abcdefghijklmnop"), ("gh", "p_abcdefghijklmnopqrstuvwxyz0123456789"), ("Xk9#", "mP2$vL8qZ"), ("AK", "IAABCDEFGHIJKLMNOP"), ("ey", "JhbGciOiJIUzI1NiJ9.e30.abc"), ("P@ss", "w0rd!2026x")];
        for (a, b) in fakes {
            let t = format!("{a}{b}");
            assert!(classify(&t, false).is_none(), "{t}");
        }
        // The password managers' own "do not record" flag wins over everything, even a parcel number.
        assert!(classify("1Z999AA10123456784", true).is_none());
        assert!(classify("482913", true).is_none());
    }

    #[test]
    fn long_text_and_ordinary_sentences_get_nothing() {
        assert!(classify(&"1Z999AA10123456784 ".repeat(30), false).is_none());
        assert!(classify("Meeting moved to Thursday at 10", false).is_none());
        assert!(classify("", false).is_none());
        assert!(classify("see https://example.com for more", false).is_none());
    }
}
