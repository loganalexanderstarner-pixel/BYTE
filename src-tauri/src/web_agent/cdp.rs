//! Driving a real Edge/Chrome through the DevTools protocol (CDP), the part that needs no browser to test.
//!
//! The page is shown to the model as a short numbered list of what can be used ("set of marks": role, name,
//! number) taken from the browser's accessibility tree, not as pictures. Every action is a typed `Action`,
//! validated before it runs (the number exists, the field isn't private, the site is public and allowed, a
//! button that commits waits for approval) and checked afterwards (did the address, title or page change?),
//! with one retry by another route. The approval cards, the private-field rule and the public-sites rule are
//! the web agent's own functions (`allowed_host`, `is_private_field`, the `commits` flag, `Session` approvals):
//! `CdpBrowser` plugs into the same `Session`, so nothing is duplicated except the page-side classification
//! that `bridge.js` does in JavaScript for the hidden window (`commits`, `is_sensitive`), kept in step by a test.
//!
//! Text typed by BYTE only ever travels as a protocol argument, never inside script text.

use std::sync::Mutex;
use std::time::Duration;

use futures_util::future::BoxFuture;
use serde_json::{json, Value};

use super::{allowed_host, is_private_field, Element};
use crate::error::{AppError, AppResult};

/// How BYTE talks to the browser: one request, one reply. The real one is a websocket (`cdp_ws.rs`);
/// tests use a scripted fake.
pub trait Transport: Send + Sync {
    fn send<'a>(&'a self, method: &'a str, params: Value) -> BoxFuture<'a, AppResult<Value>>;
    /// The browser went away (the person closed the window).
    fn closed(&self) -> bool {
        false
    }
}

// ---------------------------------------------------------------- protocol

/// A request as the browser wants it: `{"id":1,"method":"…","params":{…}}`.
pub fn encode_request(id: u64, method: &str, params: &Value) -> String {
    json!({ "id": id, "method": method, "params": params }).to_string()
}

/// What the browser sends back.
#[derive(Debug, Clone, PartialEq)]
pub enum Incoming {
    Reply { id: u64, result: Result<Value, String> },
    Event { method: String, params: Value },
}

pub fn parse_incoming(text: &str) -> Option<Incoming> {
    let v: Value = serde_json::from_str(text).ok()?;
    if let Some(id) = v.get("id").and_then(Value::as_u64) {
        let result = match v.get("error") {
            Some(e) => Err(e.get("message").and_then(Value::as_str).unwrap_or("the browser refused").to_string()),
            None => Ok(v.get("result").cloned().unwrap_or(Value::Null)),
        };
        return Some(Incoming::Reply { id, result });
    }
    let method = v.get("method").and_then(Value::as_str)?.to_string();
    Some(Incoming::Event { method, params: v.get("params").cloned().unwrap_or(Value::Null) })
}

/// What BYTE does when the browser reports something (the websocket layer sends these back at once).
/// `own_target`: the id of the one page BYTE drives. Every other page that opens is closed; documents and
/// frames are checked against the site rules before they load (`Fetch.requestPaused`), and a main frame that
/// still ends up somewhere forbidden is sent to about:blank.
pub fn react(own_target: &str, method: &str, params: &Value) -> Vec<(String, Value)> {
    match method {
        "Page.javascriptDialogOpening" => vec![("Page.handleJavaScriptDialog".into(), json!({ "accept": params["type"] == "beforeunload" }))],
        "Fetch.requestPaused" => {
            let id = params["requestId"].as_str().unwrap_or("");
            let url = params.pointer("/request/url").and_then(Value::as_str).unwrap_or("");
            let ok = match url::Url::parse(url) {
                Ok(u) if matches!(u.scheme(), "http" | "https") => allowed_host(&u),
                Ok(u) => !matches!(u.scheme(), "file" | "ftp" | "javascript"),
                Err(_) => false,
            };
            if ok {
                vec![("Fetch.continueRequest".into(), json!({ "requestId": id }))]
            } else {
                vec![("Fetch.failRequest".into(), json!({ "requestId": id, "errorReason": "BlockedByClient" }))]
            }
        }
        "Target.targetCreated" => {
            let t = &params["targetInfo"];
            let tid = t["targetId"].as_str().unwrap_or("");
            if t["type"] == "page" && !tid.is_empty() && tid != own_target {
                vec![("Target.closeTarget".into(), json!({ "targetId": tid }))]
            } else {
                Vec::new()
            }
        }
        "Page.frameNavigated" => {
            let f = &params["frame"];
            let main = f.get("parentId").is_none();
            let bad = url::Url::parse(f["url"].as_str().unwrap_or("")).is_ok_and(|u| matches!(u.scheme(), "http" | "https") && !allowed_host(&u));
            if main && bad {
                vec![("Page.navigate".into(), json!({ "url": "about:blank" }))]
            } else {
                Vec::new()
            }
        }
        _ => Vec::new(),
    }
}

// ------------------------------------------------------- page-side classification

/// Phrases on a button or link that mean "this commits something" (same list as `COMMIT_WORDS` in bridge.js).
pub const COMMIT_WORDS: &[&str] = &[
    "submit", "send", "place order", "order now", "buy", "pay", "purchase", "checkout", "check out", "confirm", "sign up", "register",
    "create account", "book now", "reserve", "subscribe", "donate", "delete", "publish", "post comment", "apply now", "transfer",
];

/// Whole-word / whole-phrase match, like `\b…\b`.
fn has_phrase(text: &str, phrase: &str) -> bool {
    let t = text.to_lowercase();
    let mut from = 0;
    while let Some(i) = t[from..].find(phrase) {
        let s = from + i;
        let e = s + phrase.len();
        let before = t[..s].chars().next_back().is_none_or(|c| !c.is_alphanumeric() && c != '_');
        let after = t[e..].chars().next().is_none_or(|c| !c.is_alphanumeric() && c != '_');
        if before && after {
            return true;
        }
        from = e;
        if from >= t.len() {
            break;
        }
    }
    false
}

/// Would activating this submit a form or commit something? (bridge.js `commits`.)
pub fn commits(tag: &str, input_type: &str, in_form: bool, label: &str) -> bool {
    if tag == "input" && (input_type == "submit" || input_type == "image") {
        return true;
    }
    if tag == "button" && in_form && (input_type.is_empty() || input_type == "submit") {
        return true;
    }
    if tag == "a" {
        return false;
    }
    COMMIT_WORDS.iter().any(|w| has_phrase(label, w))
}

/// A private field by its type, autocomplete hint or names (bridge.js `isSensitive`).
pub fn is_sensitive(input_type: &str, autocomplete: &str, hay: &str) -> bool {
    if input_type.eq_ignore_ascii_case("password") {
        return true;
    }
    let ac = autocomplete.to_ascii_lowercase();
    if ac.starts_with("cc-") || ac.contains("current-password") || ac.contains("new-password") || ac.contains("one-time-code") {
        return true;
    }
    const WORDS: &[&str] = &[
        "pass", "pwd", "cvv", "cvc", "csc", "securitycode", "cardnumber", "cardnum", "cardno", "ccnumber", "ccnum", "iban", "ssn", "socialsecurity",
        "routing", "accountnumber", "accountnum", "otp", "onetime",
    ];
    let squashed: String = hay.to_lowercase().chars().filter(|c| c.is_alphanumeric()).collect();
    if WORDS.iter().any(|w| squashed.contains(w)) {
        return true;
    }
    // "PIN" only as a word of its own.
    hay.to_lowercase().split(|c: char| !c.is_alphanumeric()).any(|w| w == "pin")
}

// ------------------------------------------------------------------ the marks

/// Roles worth a number.
const INTERACTIVE: &[&str] = &[
    "link", "button", "textbox", "searchbox", "combobox", "checkbox", "radio", "menuitem", "menuitemcheckbox", "menuitemradio", "tab", "switch", "spinbutton", "option",
];
/// Most marks per page view (the rest is counted).
pub const MAX_MARKS: usize = 150;

/// An interactive node from the accessibility tree.
#[derive(Debug, Clone, PartialEq)]
pub struct AxNode {
    pub backend_id: i64,
    pub role: String,
    pub name: String,
    pub disabled: bool,
}

/// Reads `Accessibility.getFullAXTree`: the interactive, not-ignored nodes with a DOM node behind them, in page order.
pub fn parse_ax_tree(result: &Value) -> Vec<AxNode> {
    let Some(nodes) = result.get("nodes").and_then(Value::as_array) else { return Vec::new() };
    nodes
        .iter()
        .filter(|n| n.get("ignored").and_then(Value::as_bool) != Some(true))
        .filter_map(|n| {
            let role = n.pointer("/role/value").and_then(Value::as_str)?.to_string();
            if !INTERACTIVE.contains(&role.as_str()) {
                return None;
            }
            let backend_id = n.get("backendDOMNodeId").and_then(Value::as_i64)?;
            let name = n.pointer("/name/value").and_then(Value::as_str).unwrap_or("").to_string();
            let disabled = n
                .get("properties")
                .and_then(Value::as_array)
                .is_some_and(|ps| ps.iter().any(|p| p["name"] == "disabled" && p.pointer("/value/value").and_then(Value::as_bool) == Some(true)));
            Some(AxNode { backend_id, role, name, disabled })
        })
        .collect()
}

/// What the page says about one node (the `DESCRIBE_FN` result).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct NodeInfo {
    pub tag: String,
    pub input_type: String,
    pub autocomplete: String,
    pub hay: String,
    pub in_form: bool,
    pub visible: bool,
    pub disabled: bool,
    pub download: bool,
    pub editable: bool,
    pub href: Option<String>,
    pub value: Option<String>,
    pub checked: Option<bool>,
    pub options: Option<Vec<String>>,
}

pub fn parse_node_info(v: &Value) -> NodeInfo {
    let s = |k: &str| v.get(k).and_then(Value::as_str).unwrap_or("").to_string();
    let b = |k: &str| v.get(k).and_then(Value::as_bool).unwrap_or(false);
    NodeInfo {
        tag: s("tag"),
        input_type: s("type"),
        autocomplete: s("autocomplete"),
        hay: s("hay"),
        in_form: b("inForm"),
        visible: v.get("visible").and_then(Value::as_bool).unwrap_or(true),
        disabled: b("disabled"),
        download: b("download"),
        editable: b("editable"),
        href: v.get("href").and_then(Value::as_str).map(str::to_string),
        value: v.get("value").and_then(Value::as_str).map(str::to_string),
        checked: v.get("checked").and_then(Value::as_bool).filter(|_| matches!(s("type").as_str(), "checkbox" | "radio")),
        options: v.get("options").and_then(Value::as_array).map(|a| a.iter().filter_map(|o| o.as_str().map(str::to_string)).collect()),
    }
}

/// "Customer name:" becomes "Customer name"; spaces collapsed; at most 80 characters.
pub fn tidy_label(s: &str) -> String {
    let one: String = s.split_whitespace().collect::<Vec<_>>().join(" ");
    let cut: String = if one.chars().count() > 80 { one.chars().take(79).collect::<String>() + "\u{2026}" } else { one };
    cut.trim_end_matches([':', '*', ' ']).to_string()
}

/// One numbered thing on the page.
#[derive(Debug, Clone, PartialEq)]
pub struct Mark {
    pub backend_id: i64,
    pub element: Element,
}

fn kind_of(ax: &AxNode, info: &NodeInfo) -> String {
    match (ax.role.as_str(), info.tag.as_str()) {
        ("link", _) => "link".into(),
        ("combobox", "select") => "select".into(),
        ("textbox", "textarea") => "textbox".into(),
        ("textbox", _) if info.editable && info.tag != "input" => "textbox".into(),
        ("textbox" | "searchbox" | "spinbutton" | "combobox", _) => "input".into(),
        ("switch", _) => "checkbox".into(),
        (role, _) => role.into(),
    }
}

/// The web agent's `Element` for an accessibility node, with the page's answers about it.
pub fn element_from(n: u32, ax: &AxNode, info: &NodeInfo) -> Element {
    let label = tidy_label(&ax.name);
    let tag = info.tag.as_str();
    let kind = kind_of(ax, info);
    let is_text = matches!(tag, "input" | "textarea");
    Element {
        n,
        kind: kind.clone(),
        label: label.clone(),
        input_type: if is_text { Some(if info.input_type.is_empty() { if tag == "textarea" { "textarea".into() } else { "text".into() } } else { info.input_type.clone() }) } else { None },
        value: info.value.clone().filter(|v| !v.is_empty() || tag == "select"),
        checked: info.checked.filter(|_| kind == "checkbox" || kind == "radio"),
        options: info.options.clone(),
        href: info.href.clone().filter(|_| tag == "a"),
        disabled: ax.disabled || info.disabled,
        sensitive: is_sensitive(&info.input_type, &info.autocomplete, &info.hay),
        commits: commits(tag, &info.input_type, info.in_form, &label),
        download: info.download,
    }
}

/// An element as the bridge reports it (what `Snapshot` reads).
pub fn element_json(e: &Element) -> Value {
    let mut o = json!({ "n": e.n, "kind": e.kind, "label": e.label });
    let m = o.as_object_mut().expect("object");
    if let Some(t) = &e.input_type {
        m.insert("type".into(), json!(t));
    }
    if let Some(v) = &e.value {
        m.insert("value".into(), json!(v));
    }
    if let Some(c) = e.checked {
        m.insert("checked".into(), json!(c));
    }
    if let Some(o2) = &e.options {
        m.insert("options".into(), json!(o2));
    }
    if let Some(h) = &e.href {
        m.insert("href".into(), json!(h));
    }
    for (k, v) in [("disabled", e.disabled), ("sensitive", e.sensitive), ("commits", e.commits), ("download", e.download)] {
        if v {
            m.insert(k.into(), json!(true));
        }
    }
    o
}

// ---------------------------------------------------------------- the actions

/// What BYTE can ask the browser to do.
#[derive(Debug, Clone, PartialEq)]
pub enum Action {
    Open { url: String },
    Click { n: u32 },
    Type { n: u32, text: String },
    Select { n: u32, option: String },
    Scroll { direction: String },
    Back,
    WaitFor { target: WaitTarget, timeout_ms: u64 },
}

#[derive(Debug, Clone, PartialEq)]
pub enum WaitTarget {
    /// Some text is on the page.
    Text(String),
    /// A thing with this name (a button, link, box…) is on the page.
    Element(String),
}

/// Why an action wasn't run.
#[derive(Debug, Clone, PartialEq)]
pub enum Refusal {
    NoSuchElement(u32),
    Disabled(String),
    Private(String),
    NotATextBox(String),
    NotAChoice(String),
    SiteNotAllowed(String),
    BadAddress(String),
    Download(String),
    BadDirection(String),
    EmptyWait,
    /// Not a refusal: the button commits, so the person's OK is needed first.
    NeedsApproval,
}

impl Refusal {
    pub fn message(&self) -> String {
        match self {
            Refusal::NoSuchElement(n) => format!("There's no element {n} on the page now; read the page again."),
            Refusal::Disabled(l) => format!("\u{201c}{l}\u{201d} is disabled."),
            Refusal::Private(l) => format!("BYTE doesn't type into password, card or other private fields (\u{201c}{l}\u{201d}); the user has to fill that in."),
            Refusal::NotATextBox(l) => format!("\u{201c}{l}\u{201d} isn't a text box; use click for it."),
            Refusal::NotAChoice(l) => format!("\u{201c}{l}\u{201d} isn't a list or a set of choices; click it instead."),
            Refusal::SiteNotAllowed(h) => format!("BYTE's browser doesn't open {h}: only public web sites you allow, not files, the local network or blocked sites."),
            Refusal::BadAddress(a) => format!("not a web address: {a}"),
            Refusal::Download(l) => format!("\u{201c}{l}\u{201d} downloads a file: use download_file, which asks the user first."),
            Refusal::BadDirection(d) => format!("can't scroll \u{201c}{d}\u{201d}; use down, up, top or bottom."),
            Refusal::EmptyWait => "say what to wait for".into(),
            Refusal::NeedsApproval => "needs approval".into(),
        }
    }
}

/// A web address from what the model gave (adds https:// when missing).
pub fn parse_address(raw: &str) -> Result<url::Url, Refusal> {
    let raw = raw.trim();
    url::Url::parse(raw).or_else(|_| url::Url::parse(&format!("https://{raw}"))).map_err(|_| Refusal::BadAddress(raw.to_string()))
}

fn find(marks: &[Mark], n: u32) -> Result<&Element, Refusal> {
    marks.iter().find(|m| m.element.n == n).map(|m| &m.element).ok_or(Refusal::NoSuchElement(n))
}

/// Checks an action against the page as last read, before anything is done.
/// `approved`: the person already pressed Approve for this click.
pub fn validate(action: &Action, marks: &[Mark], approved: bool) -> Result<(), Refusal> {
    match action {
        Action::Open { url } => {
            let u = parse_address(url)?;
            if allowed_host(&u) {
                Ok(())
            } else {
                Err(Refusal::SiteNotAllowed(u.host_str().unwrap_or("that address").to_string()))
            }
        }
        Action::Click { n } => {
            let el = find(marks, *n)?;
            if el.disabled {
                return Err(Refusal::Disabled(el.label.clone()));
            }
            if el.download {
                return Err(Refusal::Download(el.label.clone()));
            }
            if let Some(h) = el.href.as_deref().and_then(|h| url::Url::parse(h).ok()) {
                // A normal link to somewhere BYTE may not go is not followed (javascript:, files, blocked sites).
                if matches!(h.scheme(), "http" | "https") && !allowed_host(&h) {
                    return Err(Refusal::SiteNotAllowed(h.host_str().unwrap_or("that address").to_string()));
                }
                if !matches!(h.scheme(), "http" | "https" | "about") {
                    return Err(Refusal::BadAddress(h.scheme().to_string() + ":"));
                }
            }
            if el.commits && !approved {
                return Err(Refusal::NeedsApproval);
            }
            Ok(())
        }
        Action::Type { n, .. } => {
            let el = find(marks, *n)?;
            if is_private_field(el) {
                return Err(Refusal::Private(el.label.clone()));
            }
            if el.disabled {
                return Err(Refusal::Disabled(el.label.clone()));
            }
            if !matches!(el.kind.as_str(), "input" | "textbox") || matches!(el.input_type.as_deref(), Some("checkbox" | "radio" | "submit" | "button" | "reset" | "image" | "file" | "hidden")) {
                return Err(Refusal::NotATextBox(el.label.clone()));
            }
            Ok(())
        }
        Action::Select { n, .. } => {
            let el = find(marks, *n)?;
            if is_private_field(el) {
                return Err(Refusal::Private(el.label.clone()));
            }
            if el.disabled {
                return Err(Refusal::Disabled(el.label.clone()));
            }
            if !matches!(el.kind.as_str(), "select" | "radio" | "checkbox" | "combobox") {
                return Err(Refusal::NotAChoice(el.label.clone()));
            }
            Ok(())
        }
        Action::Scroll { direction } => match direction.as_str() {
            "down" | "up" | "top" | "bottom" => Ok(()),
            d => Err(Refusal::BadDirection(d.to_string())),
        },
        Action::Back => Ok(()),
        Action::WaitFor { target, .. } => match target {
            WaitTarget::Text(t) | WaitTarget::Element(t) if t.trim().is_empty() => Err(Refusal::EmptyWait),
            _ => Ok(()),
        },
    }
}

/// The page's state, compared before and after an action.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Finger {
    pub url: String,
    pub title: String,
    /// Characters of page text and number of elements: changes when the page redraws.
    pub text_len: u64,
    pub nodes: u64,
    pub scroll_y: i64,
}

impl Finger {
    pub fn from_value(v: &Value) -> Finger {
        Finger {
            url: v["url"].as_str().unwrap_or("").to_string(),
            title: v["title"].as_str().unwrap_or("").to_string(),
            text_len: v["len"].as_u64().unwrap_or(0),
            nodes: v["nodes"].as_u64().unwrap_or(0),
            scroll_y: v["y"].as_i64().unwrap_or(0),
        }
    }
    /// The page itself changed (not just the scroll position).
    pub fn page_changed(&self, other: &Finger) -> bool {
        self.url != other.url || self.title != other.title || self.text_len != other.text_len || self.nodes != other.nodes
    }
}

/// Centre of the first quad of `DOM.getContentQuads` (8 numbers: four x,y corners).
pub fn quad_center(result: &Value) -> Option<(f64, f64)> {
    let q: Vec<f64> = result.pointer("/quads/0")?.as_array()?.iter().filter_map(Value::as_f64).collect();
    if q.len() < 8 {
        return None;
    }
    let (x, y) = ((q[0] + q[2] + q[4] + q[6]) / 4.0, (q[1] + q[3] + q[5] + q[7]) / 4.0);
    (x.is_finite() && y.is_finite()).then_some((x, y))
}

// ------------------------------------------------- scripts run on a node (arguments are data)

const DESCRIBE_FN: &str = "function(){var el=this;var tag=el.tagName.toLowerCase();var type=(el.getAttribute('type')||'').toLowerCase();\
function vis(){for(var x=el;x&&x.nodeType===1;x=x.parentElement){if(x.hidden||x.getAttribute('aria-hidden')==='true')return false;var st=getComputedStyle(x);if(st.display==='none'||st.visibility==='hidden')return false;}return el.getClientRects().length>0;}\
var f=el.form||(el.closest&&el.closest('form'));\
var o={tag:tag,type:type,autocomplete:(el.getAttribute('autocomplete')||'').toLowerCase(),hay:[el.getAttribute('name'),el.id,el.getAttribute('placeholder'),el.getAttribute('aria-label')].join(' '),inForm:!!f,visible:vis(),disabled:!!el.disabled||el.getAttribute('aria-disabled')==='true',download:el.hasAttribute('download'),editable:!!el.isContentEditable};\
if(tag==='a')o.href=el.href;\
if(tag==='input'||tag==='textarea'){if(type!=='password'&&type!=='checkbox'&&type!=='radio')o.value=String(el.value||'').slice(0,80);o.checked=!!el.checked;}\
if(tag==='select'){o.options=Array.prototype.slice.call(el.options,0,25).map(function(x){return String(x.text).trim().slice(0,40)});o.value=el.selectedIndex>=0?String(el.options[el.selectedIndex].text).trim().slice(0,40):'';}\
return o;}";

const CLICK_FN: &str = "function(){try{if(this.tagName==='A'&&this.getAttribute('target'))this.removeAttribute('target');this.scrollIntoView({block:'center'});}catch(e){}this.click();return true;}";

const TYPE_FN: &str = "function(text){var el=this,tag=el.tagName;\
if(el.isContentEditable&&tag!=='INPUT'&&tag!=='TEXTAREA'){el.focus();el.textContent=text;el.dispatchEvent(new Event('input',{bubbles:true}));return String(el.textContent);}\
if(tag!=='INPUT'&&tag!=='TEXTAREA')throw new Error('not a text box');\
el.focus();var proto=tag==='TEXTAREA'?HTMLTextAreaElement.prototype:HTMLInputElement.prototype;var d=Object.getOwnPropertyDescriptor(proto,'value');\
if(d&&d.set)d.set.call(el,text);else el.value=text;\
el.dispatchEvent(new Event('input',{bubbles:true}));el.dispatchEvent(new Event('change',{bubbles:true}));return String(el.value);}";

/// Focus and select the box's text, so `Input.insertText` replaces it (the second way to type).
const FOCUS_FN: &str = "function(){this.focus();if(this.select)this.select();return String(this.value!==undefined?this.value:this.textContent);}";
const READ_VALUE_FN: &str = "function(){return String(this.value!==undefined?this.value:this.textContent);}";

const CHOOSE_FN: &str = "function(want){want=String(want).replace(/\\s+/g,' ').trim().toLowerCase();var el=this;var type=(el.getAttribute('type')||'').toLowerCase();\
function lab(e){return String((e.labels&&e.labels[0]&&e.labels[0].innerText)||e.getAttribute('aria-label')||e.value||'').replace(/\\s+/g,' ').trim();}\
function fire(t){el.dispatchEvent(new Event(t,{bubbles:true}));}\
if(el.tagName==='INPUT'&&(type==='radio'||type==='checkbox')){var group=el.name?document.querySelectorAll('input[name=\"'+el.name.replace(/\"/g,'')+'\"]'):[el];var hit=null,i;\
for(i=0;i<group.length&&!hit;i++)if(lab(group[i]).toLowerCase()===want||String(group[i].value).toLowerCase()===want)hit=group[i];\
for(i=0;i<group.length&&!hit;i++)if(lab(group[i]).toLowerCase().indexOf(want)>=0)hit=group[i];\
if(!hit)throw new Error('No choice like \"'+want+'\" there.');if(!(type==='radio'&&hit.checked))hit.click();return {label:hit.name||lab(hit),value:lab(hit)||hit.value};}\
if(el.tagName!=='SELECT')throw new Error('not a list');var opts=el.options,pick=-1,k;\
for(k=0;k<opts.length&&pick<0;k++)if(String(opts[k].text).replace(/\\s+/g,' ').trim().toLowerCase()===want||String(opts[k].value).toLowerCase()===want)pick=k;\
for(k=0;k<opts.length&&pick<0;k++)if(String(opts[k].text).toLowerCase().indexOf(want)>=0)pick=k;\
if(pick<0)throw new Error('No option like \"'+want+'\" in the list.');el.selectedIndex=pick;fire('input');fire('change');return {label:lab(el),value:String(opts[pick].text).trim().slice(0,60)};}";

const FORM_FN: &str = "function(){var el=this;var form=el.form||(el.closest&&el.closest('form'));var fields=[];\
function sens(f){var t=(f.getAttribute('type')||'').toLowerCase();if(t==='password')return true;return /(pass|pwd|cvv|cvc|csc|card.?(number|num|no)|iban|ssn|routing|account.?(number|num)|otp|one.?time)/i.test([f.getAttribute('name'),f.id,f.getAttribute('placeholder'),f.getAttribute('aria-label'),f.getAttribute('autocomplete')].join(' '));}\
if(form){var items=form.querySelectorAll('input,select,textarea');for(var i=0;i<items.length;i++){var f=items[i];var t=(f.getAttribute('type')||'').toLowerCase();\
if(/^(hidden|submit|button|reset|image)$/.test(t)||f.getClientRects().length===0)continue;var v;\
if(t==='checkbox')v=f.checked?'yes':'no';else if(t==='radio'){if(!f.checked)continue;v=String(f.value||'').slice(0,80);}\
else if(f.tagName==='SELECT')v=f.selectedIndex>=0?String(f.options[f.selectedIndex].text).trim().slice(0,80):'';\
else v=sens(f)?(f.value?'\\u2022\\u2022\\u2022\\u2022 (filled in)':''):String(f.value||'').slice(0,200);\
fields.push({label:String(f.getAttribute('aria-label')||f.getAttribute('placeholder')||f.getAttribute('name')||t||'field').slice(0,80),value:v});}}\
return {button:String(el.getAttribute('aria-label')||el.value||el.innerText||'').trim().slice(0,80),action:form?(form.action||location.href):null,method:form?(form.getAttribute('method')||'get').toLowerCase():null,fields:fields};}";

const NO_POPUPS: &str = "window.open=function(u){if(u){try{location.href=u;}catch(e){}}return null;};";

const FINGER_EXPR: &str = "({url:location.href,title:document.title,len:document.body?document.body.innerText.length:0,nodes:document.getElementsByTagName('*').length,y:Math.round(window.scrollY||0)})";

/// URL, title, readable text (from `offset`, at most `max` characters) and form count in one go.
fn page_expr(offset: usize, max: usize) -> String {
    format!(
        "(function(){{var root=document.querySelector('main,[role=main],article')||document.body;if(!root)return {{url:location.href,title:document.title,text:'',total:0,forms:0}};\
var t=String(root.innerText!=null?root.innerText:root.textContent||'').replace(/[ \\t\\u00a0]+/g,' ').replace(/\\s*\\n\\s*/g,'\\n').replace(/\\n{{3,}}/g,'\\n\\n').trim();\
return {{url:location.href,title:String(document.title||'').trim().slice(0,120),text:t.slice({offset},{end}),total:t.length,forms:document.forms.length}};}})()",
        offset = offset,
        end = offset + max
    )
}

// ------------------------------------------------------------------- the driver

/// What an action did: the bridge-shaped value plus whether the check saw a change.
#[derive(Debug, Clone, PartialEq)]
pub struct Outcome {
    pub value: Value,
    pub verified: bool,
    pub retried: bool,
}

/// Waits and limits (zero waits in tests).
#[derive(Debug, Clone, Copy)]
pub struct Timing {
    /// After a click or key press, before looking at the page again.
    pub after_action: Duration,
    /// Between checks while a page loads.
    pub poll: Duration,
    /// Longest a page may take to load.
    pub load_wait: Duration,
}

impl Timing {
    pub const REAL: Timing = Timing { after_action: Duration::from_millis(700), poll: Duration::from_millis(200), load_wait: Duration::from_secs(25) };
    pub const INSTANT: Timing = Timing { after_action: Duration::ZERO, poll: Duration::ZERO, load_wait: Duration::from_millis(50) };
}

pub struct Driver<T: Transport> {
    pub t: T,
    marks: Mutex<Vec<Mark>>,
    timing: Timing,
}

fn err(m: impl Into<String>) -> AppError {
    AppError::msg(m.into())
}

impl<T: Transport> Driver<T> {
    pub fn new(t: T, timing: Timing) -> Driver<T> {
        Driver { t, marks: Mutex::new(Vec::new()), timing }
    }

    /// Switches on what the driver needs: the page and accessibility domains, and no downloads by the page itself.
    pub async fn init(&self) {
        for (m, p) in [
            ("Page.enable", json!({})),
            ("DOM.enable", json!({})),
            ("Runtime.enable", json!({})),
            ("Accessibility.enable", json!({})),
            // Every document and frame is checked against the site rules before it loads, and any other page that opens is closed (`react`).
            ("Fetch.enable", json!({ "patterns": [{ "resourceType": "Document" }] })),
            ("Target.setDiscoverTargets", json!({ "discover": true })),
            // A page can't save files by itself; downloads go through download_file, after the person's OK.
            ("Browser.setDownloadBehavior", json!({ "behavior": "deny" })),
            // window.open opens in the same tab: BYTE only ever has one page to watch.
            ("Page.addScriptToEvaluateOnNewDocument", json!({ "source": NO_POPUPS })),
        ] {
            let _ = self.t.send(m, p).await;
        }
    }

    fn marks_now(&self) -> Vec<Mark> {
        self.marks.lock().map(|m| m.clone()).unwrap_or_default()
    }

    async fn eval(&self, expression: &str) -> AppResult<Value> {
        let r = self.t.send("Runtime.evaluate", json!({ "expression": expression, "returnByValue": true, "awaitPromise": false })).await?;
        if let Some(d) = r.get("exceptionDetails") {
            return Err(err(format!("the page couldn't do that: {}", d.pointer("/exception/description").or_else(|| d.get("text")).and_then(Value::as_str).unwrap_or("script error"))));
        }
        Ok(r.pointer("/result/value").cloned().unwrap_or(Value::Null))
    }

    async fn object_for(&self, backend_id: i64) -> AppResult<String> {
        let r = self.t.send("DOM.resolveNode", json!({ "backendNodeId": backend_id })).await.map_err(|_| err("That element is gone from the page; read the page again."))?;
        r.pointer("/object/objectId").and_then(Value::as_str).map(str::to_string).ok_or_else(|| err("That element is gone from the page; read the page again."))
    }

    /// Runs a function on a node. The arguments are data, never script text.
    async fn call_on(&self, backend_id: i64, function: &str, args: Vec<Value>) -> AppResult<Value> {
        let object_id = self.object_for(backend_id).await?;
        let arguments: Vec<Value> = args.into_iter().map(|a| json!({ "value": a })).collect();
        let r = self.t.send("Runtime.callFunctionOn", json!({ "objectId": object_id, "functionDeclaration": function, "arguments": arguments, "returnByValue": true })).await;
        let _ = self.t.send("Runtime.releaseObject", json!({ "objectId": object_id })).await;
        let r = r?;
        if let Some(d) = r.get("exceptionDetails") {
            let text = d.pointer("/exception/description").or_else(|| d.get("text")).and_then(Value::as_str).unwrap_or("script error");
            return Err(err(format!("the page couldn't do that: {}", text.lines().next().unwrap_or(text).trim_start_matches("Error: "))));
        }
        Ok(r.pointer("/result/value").cloned().unwrap_or(Value::Null))
    }

    pub async fn fingerprint(&self) -> AppResult<Finger> {
        Ok(Finger::from_value(&self.eval(FINGER_EXPR).await?))
    }

    async fn pause(&self) {
        if !self.timing.after_action.is_zero() {
            tokio::time::sleep(self.timing.after_action).await;
        }
    }

    /// Waits for the page to finish loading (or the time to run out).
    pub async fn wait_loaded(&self) {
        let start = std::time::Instant::now();
        loop {
            let ready = self.eval("document.readyState").await.ok().and_then(|v| v.as_str().map(str::to_string));
            if ready.as_deref() == Some("complete") || start.elapsed() >= self.timing.load_wait {
                break;
            }
            tokio::time::sleep(self.timing.poll).await;
        }
        self.pause().await;
    }

    /// After anything that may have moved the page: if it's somewhere BYTE may not go, leave it.
    pub async fn enforce_site_rules(&self) -> AppResult<()> {
        let f = self.fingerprint().await?;
        if f.url.is_empty() || f.url == "about:blank" {
            return Ok(());
        }
        match url::Url::parse(&f.url) {
            Ok(u) if allowed_host(&u) => Ok(()),
            _ => {
                let _ = self.t.send("Page.navigate", json!({ "url": "about:blank" })).await;
                self.marks.lock().map(|mut m| m.clear()).ok();
                Err(err("That page is somewhere BYTE's browser may not go (a blocked site, a file or the local network), so BYTE left it."))
            }
        }
    }

    // ----------------------------------------------------------- reading

    /// The page as the bridge reports it: address, title, text and the numbered things to use.
    pub async fn snapshot(&self, offset: usize, max_text: usize) -> AppResult<Value> {
        let page = self.eval(&page_expr(offset, max_text.clamp(200, 400_000))).await?;
        let tree = self.t.send("Accessibility.getFullAXTree", json!({})).await?;
        let nodes = parse_ax_tree(&tree);
        let mut marks: Vec<Mark> = Vec::new();
        let mut more = 0usize;
        for ax in nodes {
            if marks.len() >= MAX_MARKS {
                more += 1;
                continue;
            }
            // A node that vanished between the two calls is simply skipped.
            let Ok(info) = self.call_on(ax.backend_id, DESCRIBE_FN, vec![]).await else { continue };
            let info = parse_node_info(&info);
            if !info.visible || info.input_type == "hidden" {
                continue;
            }
            let el = element_from(marks.len() as u32 + 1, &ax, &info);
            if el.kind == "link" && el.label.is_empty() {
                continue;
            }
            marks.push(Mark { backend_id: ax.backend_id, element: el });
        }
        let elements: Vec<Value> = marks.iter().map(|m| element_json(&m.element)).collect();
        if let Ok(mut g) = self.marks.lock() {
            *g = marks;
        }
        Ok(json!({
            "url": page["url"], "title": page["title"], "text": page["text"], "offset": offset,
            "textTotal": page["total"], "elements": elements, "moreElements": more, "forms": page["forms"],
        }))
    }

    pub async fn form_info(&self, n: u32) -> AppResult<Value> {
        let marks = self.marks_now();
        let m = marks.iter().find(|m| m.element.n == n).ok_or_else(|| err(Refusal::NoSuchElement(n).message()))?;
        self.call_on(m.backend_id, FORM_FN, vec![]).await
    }

    // ----------------------------------------------------------- actions

    /// Opens an address: checked first, loaded, checked again (redirects), one retry.
    pub async fn open(&self, raw: &str) -> AppResult<Outcome> {
        validate(&Action::Open { url: raw.to_string() }, &[], false).map_err(|r| err(r.message()))?;
        let url = parse_address(raw).map_err(|r| err(r.message()))?;
        let mut last = String::new();
        for attempt in 0..2 {
            let r = self.t.send("Page.navigate", json!({ "url": url.as_str() })).await;
            match r {
                Ok(v) => match v.get("errorText").and_then(Value::as_str) {
                    Some(e) if !e.is_empty() => last = e.to_string(),
                    _ => {
                        self.wait_loaded().await;
                        self.marks.lock().map(|mut m| m.clear()).ok();
                        let f = self.fingerprint().await?;
                        if f.url.starts_with("chrome-error:") || f.url.starts_with("edge-error:") {
                            last = "the page didn't load".into();
                        } else {
                            self.enforce_site_rules().await?;
                            return Ok(Outcome { value: json!({ "url": f.url, "title": f.title }), verified: true, retried: attempt == 1 });
                        }
                    }
                },
                Err(e) => last = e.to_string(),
            }
        }
        Err(err(format!("couldn't open {}: {last}", url.host_str().unwrap_or("that page"))))
    }

    pub async fn click(&self, n: u32, approved: bool) -> AppResult<Outcome> {
        let marks = self.marks_now();
        match validate(&Action::Click { n }, &marks, approved) {
            Ok(()) => {}
            Err(Refusal::NeedsApproval) => return Ok(Outcome { value: json!({ "needsApproval": true }), verified: true, retried: false }),
            Err(r) => return Err(err(r.message())),
        }
        let m = marks.iter().find(|m| m.element.n == n).ok_or_else(|| err(Refusal::NoSuchElement(n).message()))?;
        let before = self.fingerprint().await?;
        let mut retried = false;
        let mut changed = false;
        for attempt in 0..2 {
            let r = if attempt == 0 { self.click_by_script(m.backend_id).await } else { retried = true; self.click_by_mouse(m.backend_id).await };
            if let Err(e) = r {
                if attempt == 1 {
                    return Err(e);
                }
                continue;
            }
            self.wait_loaded().await;
            let after = self.fingerprint().await?;
            if after.page_changed(&before) {
                changed = true;
                break;
            }
        }
        self.marks.lock().map(|mut g| g.clear()).ok();
        self.enforce_site_rules().await?;
        let label = m.element.label.clone();
        Ok(Outcome { value: json!({ "label": label, "kind": m.element.kind, "href": m.element.href, "verified": changed }), verified: changed, retried })
    }

    async fn click_by_script(&self, backend_id: i64) -> AppResult<()> {
        match self.call_on(backend_id, CLICK_FN, vec![]).await {
            Ok(_) => Ok(()),
            // The click started a page load that ended the script's context: that is a click that worked.
            Err(e) if e.to_string().contains("context") || e.to_string().contains("destroyed") => Ok(()),
            Err(e) => Err(e),
        }
    }

    async fn click_by_mouse(&self, backend_id: i64) -> AppResult<()> {
        let _ = self.t.send("DOM.scrollIntoViewIfNeeded", json!({ "backendNodeId": backend_id })).await;
        let quads = self.t.send("DOM.getContentQuads", json!({ "backendNodeId": backend_id })).await?;
        let (x, y) = quad_center(&quads).ok_or_else(|| err("that element has no place on the page to click"))?;
        for (kind, extra) in [("mouseMoved", json!({})), ("mousePressed", json!({ "button": "left", "clickCount": 1 })), ("mouseReleased", json!({ "button": "left", "clickCount": 1 }))] {
            let mut p = json!({ "type": kind, "x": x, "y": y });
            if let (Some(o), Some(e)) = (p.as_object_mut(), extra.as_object()) {
                o.extend(e.clone());
            }
            self.t.send("Input.dispatchMouseEvent", p).await?;
        }
        Ok(())
    }

    pub async fn type_text(&self, n: u32, text: &str) -> AppResult<Outcome> {
        let marks = self.marks_now();
        validate(&Action::Type { n, text: text.to_string() }, &marks, false).map_err(|r| err(r.message()))?;
        let m = marks.iter().find(|m| m.element.n == n).ok_or_else(|| err(Refusal::NoSuchElement(n).message()))?;
        let label = m.element.label.clone();
        let got = self.call_on(m.backend_id, TYPE_FN, vec![json!(text)]).await?;
        let got = got.as_str().unwrap_or("").to_string();
        if got == text || (!text.is_empty() && !got.is_empty()) {
            return Ok(Outcome { value: json!({ "label": label, "value": clip(&got, 80) }), verified: got == text, retried: false });
        }
        // The box didn't take it (a page that guards its value): type it the way a keyboard would.
        self.call_on(m.backend_id, FOCUS_FN, vec![]).await?;
        self.t.send("Input.insertText", json!({ "text": text })).await?;
        let again = self.call_on(m.backend_id, READ_VALUE_FN, vec![]).await?;
        let again = again.as_str().unwrap_or("").to_string();
        if again.is_empty() && !text.is_empty() {
            return Err(err(format!("\u{201c}{label}\u{201d} didn't take the text")));
        }
        Ok(Outcome { value: json!({ "label": label, "value": clip(&again, 80) }), verified: again == text, retried: true })
    }

    pub async fn select(&self, n: u32, option: &str) -> AppResult<Outcome> {
        let marks = self.marks_now();
        validate(&Action::Select { n, option: option.to_string() }, &marks, false).map_err(|r| err(r.message()))?;
        let m = marks.iter().find(|m| m.element.n == n).ok_or_else(|| err(Refusal::NoSuchElement(n).message()))?;
        let v = self.call_on(m.backend_id, CHOOSE_FN, vec![json!(option)]).await?;
        let picked = v["value"].as_str().unwrap_or("").to_lowercase();
        let want = option.trim().to_lowercase();
        let verified = !picked.is_empty() && (picked.contains(&want) || want.contains(&picked));
        // One more try: the choice may have been reset by the page's own script.
        if !verified {
            let v2 = self.call_on(m.backend_id, CHOOSE_FN, vec![json!(option)]).await?;
            let p2 = v2["value"].as_str().unwrap_or("").to_lowercase();
            let ok2 = !p2.is_empty() && (p2.contains(&want) || want.contains(&p2));
            return Ok(Outcome { value: v2, verified: ok2, retried: true });
        }
        Ok(Outcome { value: v, verified, retried: false })
    }

    pub async fn scroll(&self, direction: &str) -> AppResult<Outcome> {
        validate(&Action::Scroll { direction: direction.to_string() }, &[], false).map_err(|r| err(r.message()))?;
        let js = match direction {
            "top" => "window.scrollTo(0,0)".to_string(),
            "bottom" => "window.scrollTo(0,document.documentElement.scrollHeight)".to_string(),
            "up" => "window.scrollBy(0,-0.8*(window.innerHeight||800))".to_string(),
            _ => "window.scrollBy(0,0.8*(window.innerHeight||800))".to_string(),
        };
        let before = self.fingerprint().await?;
        self.eval(&js).await?;
        self.pause().await;
        let after = self.fingerprint().await?;
        let moved = after.scroll_y != before.scroll_y || after.page_changed(&before);
        // Pages that load more as you go: a second nudge, then give up (the end of the page isn't an error).
        if !moved && (direction == "down" || direction == "up") {
            self.eval(&js).await?;
            self.pause().await;
            let a2 = self.fingerprint().await?;
            return Ok(Outcome { value: json!({ "y": a2.scroll_y }), verified: a2.scroll_y != before.scroll_y || a2.page_changed(&before), retried: true });
        }
        Ok(Outcome { value: json!({ "y": after.scroll_y }), verified: moved, retried: false })
    }

    pub async fn back(&self) -> AppResult<Outcome> {
        let before = self.fingerprint().await?;
        let hist = self.t.send("Page.getNavigationHistory", json!({})).await?;
        let idx = hist["currentIndex"].as_i64().unwrap_or(0);
        let entry = if idx > 0 { hist.pointer(&format!("/entries/{}/id", idx - 1)).and_then(Value::as_i64) } else { None };
        let Some(entry) = entry else { return Err(err("there's no earlier page to go back to")) };
        self.t.send("Page.navigateToHistoryEntry", json!({ "entryId": entry })).await?;
        self.wait_loaded().await;
        let mut after = self.fingerprint().await?;
        let mut retried = false;
        if after.url == before.url {
            retried = true;
            let _ = self.eval("history.back()").await;
            self.wait_loaded().await;
            after = self.fingerprint().await?;
        }
        self.marks.lock().map(|mut g| g.clear()).ok();
        self.enforce_site_rules().await?;
        Ok(Outcome { value: json!({ "url": after.url }), verified: after.url != before.url, retried })
    }

    /// Waits until some text, or a thing with a name, is on the page (at most `timeout_ms`, never over 15 s).
    pub async fn wait_for(&self, target: &WaitTarget, timeout_ms: u64) -> AppResult<Outcome> {
        validate(&Action::WaitFor { target: target.clone(), timeout_ms }, &[], false).map_err(|r| err(r.message()))?;
        let limit = Duration::from_millis(timeout_ms.clamp(100, 15_000));
        let start = std::time::Instant::now();
        loop {
            let found = match target {
                WaitTarget::Text(t) => {
                    let lit = serde_json::to_string(t).unwrap_or_else(|_| "\"\"".into());
                    self.eval(&format!("document.body?document.body.innerText.toLowerCase().includes(String({lit}).toLowerCase()):false")).await?.as_bool().unwrap_or(false)
                }
                WaitTarget::Element(name) => {
                    let tree = self.t.send("Accessibility.getFullAXTree", json!({})).await?;
                    let want = name.to_lowercase();
                    parse_ax_tree(&tree).iter().any(|a| a.name.to_lowercase().contains(&want))
                }
            };
            if found {
                return Ok(Outcome { value: json!({ "found": true }), verified: true, retried: false });
            }
            if start.elapsed() >= limit {
                return Ok(Outcome { value: json!({ "found": false }), verified: false, retried: false });
            }
            tokio::time::sleep(self.timing.poll.max(Duration::from_millis(1))).await;
        }
    }

    // ------------------------------------------------- the bridge's vocabulary

    /// The same `{ok, value | error}` replies `bridge.js` gives, so `Session` can't tell the two apart.
    pub async fn bridge_call(&self, method: &str, args: &Value) -> Value {
        let num = |k: &str| args.get(k).and_then(Value::as_u64).map(|n| n as u32);
        let text = |k: &str| args.get(k).and_then(Value::as_str).unwrap_or("").to_string();
        let r: AppResult<Value> = async {
            match method {
                "snapshot" => self.snapshot(args.get("offset").and_then(Value::as_u64).unwrap_or(0) as usize, args.get("maxText").and_then(Value::as_u64).unwrap_or(6000) as usize).await,
                "formInfo" => self.form_info(num("n").unwrap_or(0)).await,
                "click" => self.click(num("n").unwrap_or(0), args.get("approved").and_then(Value::as_bool).unwrap_or(false)).await.map(|o| o.value),
                "type" => self.type_text(num("n").unwrap_or(0), &text("text")).await.map(|o| o.value),
                "choose" => self.select(num("n").unwrap_or(0), &text("value")).await.map(|o| o.value),
                "scroll" => self.scroll(&text("direction")).await.map(|o| o.value),
                "size" => self.eval("({width:Math.max(document.documentElement.scrollWidth,document.documentElement.clientWidth),height:Math.max(document.documentElement.scrollHeight,document.documentElement.clientHeight)})").await,
                "wait" => {
                    let target = if args.get("text").is_some() { WaitTarget::Text(text("text")) } else { WaitTarget::Element(text("label")) };
                    self.wait_for(&target, args.get("timeoutMs").and_then(Value::as_u64).unwrap_or(5000)).await.map(|o| o.value)
                }
                other => Err(err(format!("unknown method {other}"))),
            }
        }
        .await;
        match r {
            Ok(v) => json!({ "ok": true, "value": v }),
            Err(e) => json!({ "ok": false, "error": e.to_string() }),
        }
    }
}

fn clip(s: &str, max: usize) -> String {
    if s.chars().count() > max {
        s.chars().take(max - 1).collect::<String>() + "\u{2026}"
    } else {
        s.to_string()
    }
}

#[cfg(test)]
pub const CLICK_FN_FOR_TESTS: &str = CLICK_FN;

#[cfg(test)]
#[path = "cdp_tests.rs"]
pub(crate) mod tests;
