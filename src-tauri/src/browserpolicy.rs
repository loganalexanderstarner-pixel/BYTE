//! Which sites BYTE's browser may open (Settings → Browser), and a small local log of what it did there.
//!
//! The rules, in this order:
//! 1. a site on the person's block list is never opened (the block list always wins);
//! 2. when the allow list has entries, only those sites (and their sub-sites) are opened;
//! 3. otherwise any public site, except the built-in sensitive ones (banks and payments, password
//!    managers, health portals, government sites, sign-in pages of the big account providers).
//!    A site the person adds to the allow list is opened even if it is built in (their own choice).
//!
//! The rules are checked in `web_agent::allowed_host`, which every page BYTE opens goes through (both the
//! hidden window and the browser driven through the DevTools protocol), so one function covers all of them.
//! The activity log keeps the address and the kind of step, never the text BYTE typed.

use std::path::Path;
use std::sync::RwLock;

use once_cell::sync::Lazy;
use serde::{Deserialize, Serialize};

/// The person's own lists (Settings → Browser). Entries are domains: `example.com` also covers `shop.example.com`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct SitePolicy {
    pub allow: Vec<String>,
    pub block: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Verdict {
    Ok,
    /// On the person's block list.
    Blocked,
    /// The allow list has entries and this site isn't one of them.
    NotAllowed,
    /// A built-in sensitive category (the name of the category).
    Sensitive(&'static str),
}

/// Sites that are blocked unless the person allows them by name: (category, domains).
pub const BUILTIN: &[(&str, &[&str])] = &[
    (
        "banking and payments",
        &[
            "chase.com", "bankofamerica.com", "wellsfargo.com", "citi.com", "citibank.com", "capitalone.com", "usbank.com", "pnc.com", "tdbank.com",
            "truist.com", "ally.com", "discover.com", "americanexpress.com", "hsbc.com", "barclays.co.uk", "lloydsbank.com", "natwest.com",
            "santander.com", "ing.com", "paypal.com", "venmo.com", "cash.app", "wise.com", "revolut.com", "stripe.com", "schwab.com",
            "fidelity.com", "vanguard.com", "etrade.com", "robinhood.com", "coinbase.com", "binance.com", "kraken.com",
        ],
    ),
    ("password managers", &["1password.com", "lastpass.com", "bitwarden.com", "dashlane.com", "keepersecurity.com", "nordpass.com", "roboform.com", "pass.proton.me"]),
    (
        "health portals",
        &["mychart.com", "kaiserpermanente.org", "anthem.com", "cigna.com", "aetna.com", "uhc.com", "unitedhealthcare.com", "humana.com", "bcbs.com", "zocdoc.com", "teladoc.com", "nhs.uk"],
    ),
    (
        "sign-in pages",
        &["accounts.google.com", "login.microsoftonline.com", "login.live.com", "account.microsoft.com", "appleid.apple.com", "idmsa.apple.com", "signin.aws.amazon.com", "id.atlassian.com"],
    ),
];

/// Host words that mark a site as a sensitive portal even when the domain isn't in the list.
const HOST_WORDS: &[(&str, &str)] = &[
    ("onlinebanking", "banking and payments"),
    ("netbanking", "banking and payments"),
    ("ebanking", "banking and payments"),
    ("mychart", "health portals"),
    ("patientportal", "health portals"),
];

/// The built-in sensitive category of a host, if any.
pub fn builtin_category(host: &str) -> Option<&'static str> {
    let host = normalize(host);
    if host.is_empty() {
        return None;
    }
    // Government and military: .gov, .mil, .gov.uk, .gob.mx, .gc.ca …
    let labels: Vec<&str> = host.split('.').collect();
    let n = labels.len();
    let government = labels.last().is_some_and(|t| matches!(*t, "gov" | "mil"))
        || (n >= 2 && matches!(labels[n - 2], "gov" | "gob" | "gouv" | "gc") && labels[n - 1].len() <= 3)
        || (n >= 3 && matches!(labels[n - 3], "gov" | "gob" | "gouv"));
    if government {
        return Some("government sites");
    }
    for (cat, domains) in BUILTIN {
        if domains.iter().any(|d| covers(d, &host)) {
            return Some(cat);
        }
    }
    HOST_WORDS.iter().find(|(w, _)| host.contains(w)).map(|(_, c)| *c)
}

/// Lower-case host without `www.`, a scheme, a path or a port.
pub fn normalize(entry: &str) -> String {
    let mut s = entry.trim().to_ascii_lowercase();
    if let Some(i) = s.find("://") {
        s = s[i + 3..].to_string();
    }
    let s = s.split(['/', '?', '#']).next().unwrap_or("");
    let s = s.rsplit('@').next().unwrap_or("");
    // A port (not an IPv6 address).
    let s = if s.starts_with('[') { s } else { s.split(':').next().unwrap_or("") };
    s.trim_start_matches("*.").trim_start_matches("www.").trim_matches('.').to_string()
}

/// `domain` covers `host` when they are the same or `host` is a sub-site of `domain`.
fn covers(domain: &str, host: &str) -> bool {
    host == domain || host.ends_with(&format!(".{domain}"))
}

impl SitePolicy {
    pub fn check(&self, host: &str) -> Verdict {
        let host = normalize(host);
        let in_list = |list: &[String]| list.iter().map(|e| normalize(e)).filter(|e| !e.is_empty()).any(|e| covers(&e, &host));
        if in_list(&self.block) {
            return Verdict::Blocked;
        }
        let allow_all = self.allow.iter().all(|e| normalize(e).is_empty());
        if !allow_all {
            return if in_list(&self.allow) { Verdict::Ok } else { Verdict::NotAllowed };
        }
        match builtin_category(&host) {
            Some(c) => Verdict::Sensitive(c),
            None => Verdict::Ok,
        }
    }

    /// Why a site was refused, for the model and the person (empty when it is allowed).
    pub fn why_not(&self, host: &str) -> Option<String> {
        match self.check(host) {
            Verdict::Ok => None,
            Verdict::Blocked => Some(format!("{host} is on your blocked list (Settings, Browser).")),
            Verdict::NotAllowed => Some(format!("{host} isn't on your list of allowed sites (Settings, Browser).")),
            Verdict::Sensitive(c) => Some(format!("{host} is blocked by default ({c}). You can allow it by name in Settings, Browser.")),
        }
    }
}

static GLOBAL: Lazy<RwLock<SitePolicy>> = Lazy::new(|| RwLock::new(SitePolicy::default()));

/// The policy for the answer that is starting (set from Settings each time a browser opens).
pub fn set_global(p: SitePolicy) {
    if let Ok(mut g) = GLOBAL.write() {
        *g = p;
    }
}

pub fn global() -> SitePolicy {
    GLOBAL.read().map(|g| g.clone()).unwrap_or_default()
}

/// The check `web_agent::allowed_host` makes for every page.
pub fn global_permits(host: &str) -> bool {
    global().check(host) == Verdict::Ok
}

/// The reason for a refusal under the current policy.
pub fn global_why_not(host: &str) -> Option<String> {
    global().why_not(host)
}

/// Every built-in blocked domain, for Settings to show.
pub fn builtin_list() -> Vec<(String, Vec<String>)> {
    let mut out: Vec<(String, Vec<String>)> = BUILTIN.iter().map(|(c, d)| (c.to_string(), d.iter().map(|s| s.to_string()).collect())).collect();
    out.push(("government sites".into(), vec![".gov".into(), ".mil".into(), ".gov.uk".into(), ".gob.*".into(), ".gouv.*".into()]));
    out
}

// ------------------------------------------------------------------ activity

const KEEP: usize = 500;

/// One thing BYTE did in the browser.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Activity {
    /// Milliseconds since 1970.
    pub at: i64,
    /// The page the step happened on.
    pub url: String,
    /// What was done, without any typed text: "Opened", "Clicked \"Next\"", "Typed into \"Search\"".
    pub action: String,
}

/// The activity line for a browser tool call: kind of step plus the element's label, never the text typed.
pub fn describe_step(tool: &str, label: Option<&str>, url_arg: &str) -> String {
    use crate::web_agent as w;
    let quoted = |l: Option<&str>| l.filter(|l| !l.is_empty()).map(|l| format!(" \u{201c}{}\u{201d}", l.chars().take(60).collect::<String>())).unwrap_or_default();
    match tool {
        w::OPEN_URL => format!("Opened {}", site_text(url_arg)),
        w::CLICK => format!("Clicked{}", quoted(label)),
        w::TYPE_TEXT => format!("Typed into{}", quoted(label)),
        w::CHOOSE => format!("Chose an option in{}", quoted(label)),
        w::SCROLL => "Scrolled".to_string(),
        w::BACK => "Went back".to_string(),
        w::READ_PAGE_AGAIN => "Read the page".to_string(),
        w::DOWNLOAD => format!("Download (asked first){}", quoted(label)),
        w::SAVE_PAGE => "Saved the page".to_string(),
        other => other.to_string(),
    }
}

fn site_text(raw: &str) -> String {
    let h = normalize(raw);
    if h.is_empty() {
        raw.chars().take(80).collect()
    } else {
        h
    }
}

/// Scheme, host and path only: no query string, fragment or login part (they can hold search words or tokens).
pub fn redact_url(raw: &str) -> String {
    match url::Url::parse(raw) {
        Ok(u) if u.host_str().is_some() => format!("{}://{}{}", u.scheme(), u.host_str().unwrap_or(""), u.path()),
        Ok(u) => format!("{}:", u.scheme()),
        Err(_) => raw.split(['?', '#']).next().unwrap_or("").chars().take(120).collect(),
    }
}

pub fn append(file: &Path, a: &Activity) {
    use std::io::Write;
    let a = &Activity { url: redact_url(&a.url), ..a.clone() };
    let Ok(line) = serde_json::to_string(a) else { return };
    if let Some(dir) = file.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(file) {
        let _ = writeln!(f, "{line}");
    }
    // Keep the file small: trim when it has grown well past what is shown.
    if let Ok(text) = std::fs::read_to_string(file) {
        let lines: Vec<&str> = text.lines().collect();
        if lines.len() > KEEP + 100 {
            let keep = lines[lines.len() - KEEP..].join("\n") + "\n";
            let tmp = file.with_extension("jsonl.tmp");
            if std::fs::write(&tmp, keep).is_ok() {
                let _ = std::fs::rename(&tmp, file);
            }
        }
    }
}

/// The newest `limit` entries, newest first.
pub fn read(file: &Path, limit: usize) -> Vec<Activity> {
    let text = std::fs::read_to_string(file).unwrap_or_default();
    let mut v: Vec<Activity> = text.lines().filter_map(|l| serde_json::from_str(l).ok()).collect();
    v.reverse();
    v.truncate(limit);
    v
}

pub fn clear(file: &Path) {
    let _ = std::fs::remove_file(file);
}

pub fn now_ms() -> i64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis() as i64).unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p(allow: &[&str], block: &[&str]) -> SitePolicy {
        SitePolicy { allow: allow.iter().map(|s| s.to_string()).collect(), block: block.iter().map(|s| s.to_string()).collect() }
    }

    #[test]
    fn empty_lists_allow_any_public_site_but_not_the_sensitive_ones() {
        let d = SitePolicy::default();
        assert_eq!(d.check("example.com"), Verdict::Ok);
        assert_eq!(d.check("en.wikipedia.org"), Verdict::Ok);
        assert!(matches!(d.check("www.chase.com"), Verdict::Sensitive("banking and payments")));
        assert!(matches!(d.check("secure.paypal.com"), Verdict::Sensitive(_)));
        assert!(matches!(d.check("vault.bitwarden.com"), Verdict::Sensitive("password managers")));
        assert!(matches!(d.check("www.irs.gov"), Verdict::Sensitive("government sites")));
        assert!(matches!(d.check("www.gov.uk"), Verdict::Sensitive("government sites")));
        assert!(matches!(d.check("portal.example.gov.au"), Verdict::Sensitive("government sites")));
        assert!(matches!(d.check("mychart.example-hospital.org"), Verdict::Sensitive("health portals")));
        assert!(matches!(d.check("onlinebanking.smallcu.org"), Verdict::Sensitive(_)));
        assert!(matches!(d.check("accounts.google.com"), Verdict::Sensitive("sign-in pages")));
        // Look-alike words are fine.
        assert_eq!(d.check("bankrate.com"), Verdict::Ok);
        assert_eq!(d.check("govtech.example.com"), Verdict::Ok);
        assert_eq!(d.check("notchase.com"), Verdict::Ok);
    }

    #[test]
    fn the_block_list_always_wins() {
        let pol = p(&["example.com"], &["example.com"]);
        assert_eq!(pol.check("example.com"), Verdict::Blocked);
        assert_eq!(pol.check("shop.example.com"), Verdict::Blocked);
        let pol = p(&[], &["https://www.news.example.org/path", "*.ads.net"]);
        assert_eq!(pol.check("news.example.org"), Verdict::Blocked);
        assert_eq!(pol.check("x.ads.net"), Verdict::Blocked);
        assert_eq!(pol.check("other.org"), Verdict::Ok);
    }

    #[test]
    fn a_filled_allow_list_limits_the_browser_to_those_sites() {
        let pol = p(&["wikipedia.org", "docs.rs"], &[]);
        assert_eq!(pol.check("en.wikipedia.org"), Verdict::Ok);
        assert_eq!(pol.check("docs.rs"), Verdict::Ok);
        assert_eq!(pol.check("example.com"), Verdict::NotAllowed);
        // Blank entries don't turn the list on.
        assert_eq!(p(&["", "  "], &[]).check("example.com"), Verdict::Ok);
    }

    #[test]
    fn naming_a_built_in_site_in_the_allow_list_opens_it() {
        let pol = p(&["nasa.gov"], &[]);
        assert_eq!(pol.check("www.nasa.gov"), Verdict::Ok);
        assert_eq!(p(&["chase.com"], &[]).check("chase.com"), Verdict::Ok);
        assert!(pol.why_not("example.com").unwrap().contains("allowed"));
        assert!(SitePolicy::default().why_not("chase.com").unwrap().contains("blocked by default"));
        assert!(SitePolicy::default().why_not("example.com").is_none());
    }

    #[test]
    fn entries_are_normalised() {
        assert_eq!(normalize("HTTPS://www.Example.com:8080/a/b?c"), "example.com");
        assert_eq!(normalize("*.example.com"), "example.com");
        assert_eq!(normalize("user@host.org"), "host.org");
        assert_eq!(normalize(""), "");
    }

    #[test]
    fn logged_addresses_have_no_query_or_login() {
        assert_eq!(redact_url("https://user:pw@www.example.com/a/b?q=secret&t=1#frag"), "https://www.example.com/a/b");
        assert_eq!(redact_url("example.com/x?y=1"), "example.com/x");
        assert_eq!(redact_url(""), "");
    }

    #[test]
    fn steps_are_described_without_typed_text() {
        assert_eq!(describe_step("open_url", None, "https://www.example.com/a"), "Opened example.com");
        assert_eq!(describe_step("click", Some("Next"), ""), "Clicked \u{201c}Next\u{201d}");
        let typed = describe_step("type_text", Some("Search"), "");
        assert_eq!(typed, "Typed into \u{201c}Search\u{201d}");
        assert_eq!(describe_step("go_back", None, ""), "Went back");
    }

    #[test]
    fn the_activity_log_keeps_the_newest_and_can_be_cleared() {
        let dir = std::env::temp_dir().join(format!("byte-browser-log-{}", std::process::id()));
        let file = dir.join("a.jsonl");
        clear(&file);
        for i in 0..(KEEP + 130) {
            append(&file, &Activity { at: i as i64, url: format!("https://example.com/{i}"), action: "Opened".into() });
        }
        let all = read(&file, 10_000);
        assert!(all.len() <= KEEP + 100);
        assert_eq!(all[0].at, (KEEP + 129) as i64);
        assert_eq!(read(&file, 3).len(), 3);
        clear(&file);
        assert!(read(&file, 5).is_empty());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
