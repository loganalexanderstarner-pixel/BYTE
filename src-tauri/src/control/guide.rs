//! Guided help: "how do I change my shipping address on this site?" BYTE answers in words, then offers (a card under the answer) to
//! either walk the person through it, ringing each button in turn ("Show me as I go"), or do it ("Do it for me"). Both use the SAME
//! plan, built once from the window in front (its UI Automation list) with the control model, kept in memory only.
//!
//! Safety: off by default and off in private chats and kids mode; never reads a window on the screen track's never-look list;
//! password and card fields are not in the list the model sees, are never ringed and never clicked; the overlay window cannot take
//! focus or catch a click; the typed words of a step are shown before they are typed; refused actions stay refused (the plan is
//! read by the same strict parser, with the person's step limit).
//!
//! The pure parts (phrase, plan reading, the step machine, finding an element again, the overlay's geometry) are tested everywhere;
//! the overlay window and the polling loop are Windows only.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;

use serde::Serialize;
use serde_json::{json, Value};
use tauri::AppHandle;
use tokio_util::sync::CancellationToken;

use super::actions::{self, Action, PlanError, Risk, Step};
use super::uia::{Element, Kind, Rect, Snapshot};
use crate::agent::{Emit, Turn};
use crate::chat::ChatEvent;
use crate::error::{AppError, AppResult};
use crate::tools::SourceBook;

pub const MAX_SAVED: usize = 8;

// ------------------------------------------------------------------ the question

const ON_SCREEN: &[&str] = &[
    "on this site", "on this page", "on this website", "on this screen", "on this window", "on this app", "in this app", "in this program", "in this window", "in this menu", "on this form", "in this form", "on the screen", "on my screen", "on here", "in here", "on this one", "on this thing",
];

/// Programs a person may name in "how do I … in Notepad"; one of these that is not open gets an offer to open it.
const COMMON_APPS: &[&str] = &[
    "notepad", "calculator", "paint", "edge", "chrome", "firefox", "word", "excel", "powerpoint", "outlook", "teams", "discord", "spotify", "steam", "settings", "explorer", "terminal", "powershell", "vscode", "code", "photoshop", "obs", "vlc", "zoom", "slack", "onenote", "wordpad", "photos", "mail", "calendar", "maps", "store",
];

const FIND_THINGS: &[&str] = &["button", "link", "setting", "settings", "option", "options", "menu", "tab", "page", "field", "form", "icon", "toggle", "checkbox"];

/// A how-to question about what is in front of the person ("how do I change my shipping address on this site", "where is the setting for
/// notifications", "I can't find the button"). Returns the goal in their words. Ordinary how-to questions with nothing on the screen in
/// them ("how do I make pasta") are not this.
pub fn howto_ask(q: &str) -> Option<String> {
    let mut l = q.trim().to_lowercase();
    for p in ["hey byte,", "hey byte", "ok byte,", "okay byte,", "please ", "byte,"] {
        if let Some(r) = l.strip_prefix(p) {
            l = r.trim_start_matches([',', ' ']).to_string();
        }
    }
    let l = l.trim_end_matches(['?', '!', '.']).trim().to_string();
    if l.is_empty() || l.chars().count() > 240 {
        return None;
    }
    let words: Vec<&str> = l.split(|c: char| !c.is_alphanumeric() && c != '\'').filter(|w| !w.is_empty()).collect();
    let has = |p: &str| l.contains(p);
    let here = ON_SCREEN.iter().any(|p| has(p)) || words.windows(2).any(|w| w == ["do", "here"] || w == ["it", "here"]) || l.ends_with(" here") || l.ends_with(" on here");
    let how = ["how do i", "how can i", "how do you", "how would i", "how to", "show me how to", "tell me how to", "help me", "how should i", "what do i click", "where do i click", "what should i click"].iter().any(|p| has(p));
    let where_ = ["where is the", "where's the", "where do i find", "where can i find", "where would i find", "where is my", "where's my", "where can i change", "where do i change"].iter().any(|p| has(p));
    let cant = ["i can't find", "i cant find", "i cannot find", "can't find the", "cant find the", "i can't figure out", "i cant figure out", "i can't figure", "can't figure out", "i'm lost", "im lost", "i'm stuck", "im stuck", "i don't know where", "i dont know where"].iter().any(|p| has(p));
    let thing = words.iter().any(|w| FIND_THINGS.contains(w));
    let ok = (how && here) || (where_ && (here || thing)) || (cant && (here || thing || has("stuck") || has("lost")));
    // Not a question about something in front of them: it names a place, a person, a recipe, a place to install something.
    let elsewhere = ["install ", "download and install", "nearest", " shop", " store", "recipe", "cook ", "bake ", "tie a ", "learn ", "get to ", "directions", "near me", "in real life", "in minecraft", "in the game", "in this game"].iter().any(|p| has(p));
    (ok && !elsewhere).then_some(l)
}

/// A how-to question that names the app ("how do I change the font in Notepad"): the goal in their words and the app's name. It only
/// counts when that app is the window being looked at (the caller checks), so "how do I bake bread in the oven" falls through.
pub fn howto_named_app(q: &str) -> Option<(String, String)> {
    let mut l = q.trim().to_lowercase();
    for p in ["hey byte,", "hey byte", "ok byte,", "okay byte,", "please ", "byte,"] {
        if let Some(r) = l.strip_prefix(p) {
            l = r.trim_start_matches([',', ' ']).to_string();
        }
    }
    let l = l.trim_end_matches(['?', '!', '.']).trim().to_string();
    if l.is_empty() || l.chars().count() > 240 {
        return None;
    }
    let how = ["how do i", "how can i", "how do you", "how would i", "how to", "show me how to", "tell me how to", "how should i"].iter().any(|p| l.contains(p));
    if !how {
        return None;
    }
    let words: Vec<&str> = l.split(|c: char| !c.is_alphanumeric() && c != '\'' && c != '+').filter(|w| !w.is_empty()).collect();
    const SKIP: &[&str] = &["the", "a", "an", "my", "this", "that", "your", "our", "real", "life", "general", "windows"];
    let app = words.windows(2).enumerate().filter_map(|(i, w)| {
        if !(w[0] == "in" || w[0] == "on" || w[0] == "using" || w[0] == "with") {
            return None;
        }
        let name = words[i + 1..].iter().find(|x| !SKIP.contains(x))?;
        (name.chars().count() >= 3 && name.chars().any(|c| c.is_alphabetic())).then(|| name.to_string())
    }).last()?;
    Some((l, app))
}

/// "next", "skip" and friends said while a walk-through is on.
pub fn voice_command(q: &str) -> Option<Cmd> {
    let l = q.trim().to_lowercase();
    let l = l.trim_start_matches("hey byte").trim_start_matches([',', ' ']).trim_end_matches(['.', '!', '?']).trim();
    match l {
        "next" | "next step" | "done" | "okay next" | "ok next" | "got it" | "go on" | "continue" => Some(Cmd::Next),
        "skip" | "skip this" | "skip it" | "skip this step" | "skip that" => Some(Cmd::Skip),
        "stop" | "stop guiding" | "stop the guide" | "exit guide" | "cancel the guide" | "that's enough" => Some(Cmd::Stop),
        "replan" | "re-plan" | "try again" | "plan again" | "look again" | "look again please" => Some(Cmd::Replan),
        _ => None,
    }
}

/// What is said back for a spoken walk-through command.
pub fn voice_reply(c: Cmd) -> &'static str {
    match c {
        Cmd::Next => "Next.",
        Cmd::Skip => "Skipped.",
        Cmd::Stop => "Okay, I stopped guiding.",
        Cmd::Replan => "Looking again.",
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Cmd {
    Next,
    Skip,
    Stop,
    Replan,
}

// ------------------------------------------------------------------ the plan

/// The element a click step means, as it was when the plan was made.
#[derive(Debug, Clone, PartialEq)]
pub struct TargetRef {
    pub id: u32,
    pub name: String,
    pub kind: Kind,
    pub rect: Rect,
}

#[derive(Debug, Clone, PartialEq)]
pub struct GuideStep {
    pub step: Step,
    /// A short human reason, "Click Account".
    pub reason: String,
    pub target: Option<TargetRef>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct GuidePlan {
    pub id: String,
    pub goal: String,
    /// The program and window the plan was made for.
    pub app: String,
    pub title: String,
    /// The window (handle) the plan was made for; every later look is at this window, not whichever is in front.
    pub hwnd: isize,
    pub steps: Vec<GuideStep>,
}

/// What the card under the answer shows.
#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct OfferStep {
    pub text: String,
    pub reason: String,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct GuideOffer {
    pub id: String,
    pub title: String,
    pub app: String,
    pub steps: Vec<OfferStep>,
}

impl GuidePlan {
    pub fn offer(&self) -> GuideOffer {
        GuideOffer { id: self.id.clone(), title: self.goal.clone(), app: self.app.clone(), steps: self.steps.iter().map(|s| OfferStep { text: caption(s), reason: s.reason.clone() }).collect() }
    }
}

/// What the caption says for a step. Typed text is shown whole, so the person sees it before it is typed.
pub fn caption(s: &GuideStep) -> String {
    match &s.step.action {
        Action::Type { text } => {
            let shown: String = text.chars().take(120).collect();
            format!("Type: {shown}{}", if text.chars().count() > 120 { "…" } else { "" })
        }
        _ if !s.reason.trim().is_empty() => s.reason.trim().to_string(),
        other => actions::describe(other),
    }
}

/// Takes the `"reason"` of each step out of the model's JSON (the strict parser knows no such field) and returns the JSON without them.
pub fn split_reasons(text: &str) -> Result<(String, Vec<String>), PlanError> {
    let Some(json) = actions::extract_json(text) else { return Err(PlanError("no JSON plan found".into())) };
    let mut v: Value = serde_json::from_str(json).map_err(|e| PlanError(format!("the plan is not valid JSON: {e}")))?;
    let mut reasons = Vec::new();
    {
        let list: Option<&mut Vec<Value>> = match &mut v {
            Value::Array(a) => Some(a),
            Value::Object(o) => o.get_mut("steps").and_then(Value::as_array_mut),
            _ => None,
        };
        if let Some(list) = list {
            for s in list.iter_mut() {
                let r = s.as_object_mut().and_then(|o| o.remove("reason")).and_then(|r| r.as_str().map(|t| t.trim().to_string())).unwrap_or_default();
                reasons.push(r.chars().take(120).collect());
            }
        }
    }
    Ok((v.to_string(), reasons))
}

/// Reads the model's reply into a plan for `snap`'s window. Click ids must be real, enabled, not secret fields.
pub fn plan_from_reply(reply: &str, goal: &str, snap: &Snapshot, max_steps: usize) -> Result<GuidePlan, PlanError> {
    let (json, reasons) = split_reasons(reply)?;
    let steps = actions::parse_plan_with(&json, max_steps)?;
    let mut out = Vec::new();
    for (i, step) in steps.into_iter().enumerate() {
        if matches!(step.action, Action::Power { .. }) || actions::risk(&step.action) == Risk::Never {
            return Err(PlanError(format!("step {}: a walk-through never uses that", i + 1)));
        }
        let target = match &step.action {
            Action::Click { id } => match snap.find(*id) {
                Some(e) if !e.sensitive && e.enabled => Some(TargetRef { id: e.id, name: e.name.clone(), kind: e.kind, rect: e.rect }),
                Some(_) => return Err(PlanError(format!("step {}: item {id} can't be used", i + 1))),
                None => return Err(PlanError(format!("step {}: item {id} is not in the window", i + 1))),
            },
            _ => None,
        };
        let reason = reasons.get(i).cloned().filter(|r| !r.is_empty()).unwrap_or_else(|| match &target {
            Some(t) if !t.name.is_empty() => format!("Click {}", t.name),
            _ => actions::describe(&step.action),
        });
        out.push(GuideStep { step, reason, target });
    }
    Ok(GuidePlan { id: uuid::Uuid::new_v4().simple().to_string(), goal: goal.to_string(), app: snap.app.clone(), title: snap.title.clone(), hwnd: 0, steps: out })
}

/// Finds a step's element again in a fresh reading: the same id with the same name and kind, else the same name and kind, else the
/// only element of that kind whose name contains it. Never a secret field.
pub fn refind<'a>(snap: &'a Snapshot, t: &TargetRef) -> Option<&'a Element> {
    let usable = |e: &&Element| !e.sensitive;
    let same = |e: &Element| e.kind == t.kind && e.name.trim().eq_ignore_ascii_case(t.name.trim());
    if let Some(e) = snap.find(t.id).filter(usable).filter(|e| same(e)) {
        return Some(e);
    }
    if let Some(e) = snap.elements.iter().filter(usable).find(|e| same(e)) {
        return Some(e);
    }
    let n = t.name.trim().to_lowercase();
    if n.is_empty() {
        return None;
    }
    let mut like = snap.elements.iter().filter(usable).filter(|e| e.kind == t.kind && e.name.to_lowercase().contains(&n));
    match (like.next(), like.next()) {
        (Some(e), None) => Some(e),
        _ => None,
    }
}

// ------------------------------------------------------------------ kept in memory only

static PLANS: Mutex<Vec<GuidePlan>> = Mutex::new(Vec::new());

pub fn store(plan: GuidePlan) {
    if let Ok(mut p) = PLANS.lock() {
        p.push(plan);
        while p.len() > MAX_SAVED {
            p.remove(0);
        }
    }
}

pub fn get(id: &str) -> Option<GuidePlan> {
    PLANS.lock().ok().and_then(|p| p.iter().find(|g| g.id == id).cloned())
}

pub fn forget(id: &str) {
    if let Ok(mut p) = PLANS.lock() {
        p.retain(|g| g.id != id);
    }
}

// ------------------------------------------------------------------ the walk-through, as a state machine

#[derive(Debug, Clone, PartialEq)]
pub enum Phase {
    At(usize),
    /// The step's element is not in the window now.
    Missing(usize),
    Done,
    Stopped,
    /// The person went to another program.
    Left,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Found {
    /// Not looked at this time.
    Unknown,
    Yes(Rect),
    No,
}

/// What was seen since the last look.
#[derive(Debug, Clone, PartialEq)]
pub struct Obs {
    pub cmd: Option<Cmd>,
    pub clicked_in_target: bool,
    pub target: Found,
    /// The front window is another program (not the one guided, not BYTE).
    pub program_changed: bool,
}

impl Default for Obs {
    fn default() -> Self {
        Obs { cmd: None, clicked_in_target: false, target: Found::Unknown, program_changed: false }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum Out {
    Nothing,
    /// Show this step (its ring is where `Found::Yes` says).
    Show(usize),
    Missing(usize),
    Replan,
    Finished(Phase),
}

pub struct Session {
    pub steps: Vec<GuideStep>,
    pub phase: Phase,
}

impl Session {
    pub fn new(steps: Vec<GuideStep>) -> Self {
        let phase = if steps.is_empty() { Phase::Done } else { Phase::At(0) };
        Session { steps, phase }
    }

    fn advance(&mut self, from: usize) -> Out {
        if from + 1 >= self.steps.len() {
            self.phase = Phase::Done;
            Out::Finished(Phase::Done)
        } else {
            self.phase = Phase::At(from + 1);
            Out::Show(from + 1)
        }
    }

    pub fn finished(&self) -> bool {
        matches!(self.phase, Phase::Done | Phase::Stopped | Phase::Left)
    }

    pub fn observe(&mut self, o: &Obs) -> Out {
        if self.finished() {
            return Out::Nothing;
        }
        if o.cmd == Some(Cmd::Stop) {
            self.phase = Phase::Stopped;
            return Out::Finished(Phase::Stopped);
        }
        if o.program_changed {
            self.phase = Phase::Left;
            return Out::Finished(Phase::Left);
        }
        match self.phase.clone() {
            Phase::At(i) => {
                let has_target = self.steps[i].target.is_some();
                if matches!(o.cmd, Some(Cmd::Next | Cmd::Skip)) || (has_target && o.clicked_in_target) {
                    return self.advance(i);
                }
                if has_target && o.target == Found::No {
                    self.phase = Phase::Missing(i);
                    return Out::Missing(i);
                }
                Out::Nothing
            }
            Phase::Missing(i) => match o.cmd {
                Some(Cmd::Next | Cmd::Skip) => self.advance(i),
                Some(Cmd::Replan) => Out::Replan,
                _ => {
                    if matches!(o.target, Found::Yes(_)) {
                        self.phase = Phase::At(i);
                        Out::Show(i)
                    } else {
                        Out::Nothing
                    }
                }
            },
            _ => Out::Nothing,
        }
    }
}

// ------------------------------------------------------------------ the overlay's geometry

const PAD: i32 = 12;
const CAPTION_H: i32 = 76;
const MIN_W: i32 = 340;

#[derive(Debug, Clone, PartialEq)]
pub struct Layout {
    /// The overlay window, in screen pixels.
    pub x: i32,
    pub y: i32,
    pub w: i32,
    pub h: i32,
    /// The ring inside it, relative to the window; none for a step without an element.
    pub ring: Option<(i32, i32, i32, i32)>,
    /// The caption is under the ring (else above it).
    pub caption_below: bool,
}

/// Where the overlay goes for an element on a screen `(left, top, right, bottom)`: the ring around it, and the caption in the room
/// below (or above, when the element is near the bottom). The window never leaves the screen.
pub fn layout(rect: Option<Rect>, screen: (i32, i32, i32, i32)) -> Layout {
    let (sl, st, sr, sb) = screen;
    let Some(r) = rect.filter(|r| !r.is_empty()) else {
        let w = MIN_W.min(sr - sl);
        return Layout { x: sl + ((sr - sl) - w) / 2, y: st + 24, w, h: CAPTION_H, ring: None, caption_below: true };
    };
    let rw = r.right - r.left;
    let w = (rw + 2 * PAD).max(MIN_W).min(sr - sl);
    let below_room = sb - (r.bottom + PAD);
    let caption_below = below_room >= CAPTION_H;
    let h = (r.bottom - r.top) + 2 * PAD + CAPTION_H;
    let mut x = r.left - PAD;
    x = x.min(sr - w).max(sl);
    let mut y = if caption_below { r.top - PAD } else { r.top - PAD - CAPTION_H };
    y = y.min(sb - h).max(st);
    let ring_x = r.left - x;
    let ring_y = r.top - y;
    Layout { x, y, w, h, ring: Some((ring_x, ring_y, rw, r.bottom - r.top)), caption_below }
}

// ------------------------------------------------------------------ the question route

pub const RULES: &str = "\nThis is a walk-through: plan ONLY the steps that get the user to what they asked, using the numbered items below. Add a short \"reason\" to every step, in plain words, like \"Click Account\" or \"Open the Addresses tab\". Never plan clicking Send, Buy, Pay, Delete or Save for them: stop at the point where they confirm it themselves. Do not use items that are not in the list.";

pub fn plan_prompt(goal: &str, screen: &str) -> String {
    format!("{}\n\nThe person asked: {}", super::brain::plan_prompt(goal, screen), goal)
}

/// Builds the plan for a how-to question from the window in front.
pub(crate) async fn make_plan(app: &AppHandle, goal: &str, snap: &Snapshot) -> Result<GuidePlan, String> {
    let limit = super::settings_now(app).map(|s| super::executor::step_limit(&s.control_max_steps)).unwrap_or(actions::MAX_STEPS);
    let mut messages = vec![json!({"role":"system","content":format!("{}{}", super::brain::SYSTEM, RULES)}), json!({"role":"user","content":plan_prompt(goal, &super::uia::render(snap))})];
    let mut last = String::new();
    for _ in 0..2 {
        let reply = super::ask_model(app, &messages).await.map_err(|e| e.to_string())?;
        match plan_from_reply(&reply, goal, snap, limit) {
            Ok(p) => return Ok(p),
            Err(e) => {
                last = e.to_string();
                messages.push(json!({"role":"assistant","content":reply}));
                messages.push(json!({"role":"user","content":super::brain::fix_prompt(&last)}));
            }
        }
    }
    Err(format!("no usable plan ({last})"))
}

/// May this window be read? The screen track's rule (BYTE's own window, password managers, banking, the person's never-look list).
pub fn allowed(app: &str, title: &str, excludes: &[String]) -> Result<(), String> {
    let info = crate::screen::policy::WindowInfo { program: app.to_string(), title: title.to_string() };
    crate::screen::policy::may_capture(Some(&info), excludes, false).map_err(|r| r.say())
}

/// What the BYTE cloud gets for a guided-help turn: nothing from the window (no program, title or caption). In cloud mode the notes
/// of a route are sent along with the question, so this route's notes are replaced by a neutral line; the offer card is still shown
/// here, made from the local model's plan.
pub fn notes_for_cloud(kind: &str, notes: String) -> String {
    if kind == "guide" {
        "The user asked how to do something on their screen. Answer in general terms from the question alone (you cannot see their screen). BYTE shows a card under your answer offering to guide them step by step or do it for them; finish by asking if they would like that help.".to_string()
    } else {
        notes
    }
}

/// The notes the model answers from: explain in words, then ask whether the person wants BYTE's help (the card has the buttons).
pub fn notes_for(plan: &GuidePlan) -> String {
    let steps: Vec<String> = plan.steps.iter().enumerate().map(|(i, s)| format!("{}. {}", i + 1, caption(s))).collect();
    format!(
        "The user asked how to do something in the window in front of them ({} - \"{}\"). Explain in plain, short steps they can follow, using this plan of what is on their screen:\n{}\nDo not say you did anything. BYTE shows a card under your answer with the buttons \"Show me as I go\" and \"Do it for me\"; finish by asking whether they would like that help.",
        plan.app,
        plan.title,
        steps.join("\n")
    )
}

/// The how-to route: look at the window in front (if allowed), plan, keep the plan, show the offer card, and give the model notes.
pub async fn route(turn: &Turn<'_>, q: &str, _cancel: &CancellationToken, send: Emit<'_>) -> AppResult<Option<(SourceBook, String)>> {
    let Some(app) = turn.app else { return Ok(None) };
    if !turn.modules.control.guided_help {
        return Ok(None);
    }
    // While a walk-through is on, "next", "skip", "stop" and "look again" are for it (before any model, and before media "next").
    if running() {
        if let Some(c) = voice_command(crate::router::without_wake_phrase(q)) {
            send_cmd(c);
            return Ok(Some((SourceBook::default(), format!("Done: {}", voice_reply(c)))));
        }
    }
    let said = crate::router::without_wake_phrase(q);
    // "…on this page" needs nothing more; "…in Notepad" only counts when Notepad is the window being looked at.
    let (goal, named_app) = match howto_ask(said) {
        Some(g) => (g, None),
        None => match howto_named_app(said) {
            Some((g, a)) => (g, Some(a)),
            None => return Ok(None),
        },
    };
    let Some(settings) = super::settings_now(app) else { return Ok(None) };
    if settings.kids_mode || !settings.guided_help {
        return Ok(None);
    }
    // The window to look at: the one in front, or, when BYTE's own chat is in front (a typed question), the next real window below it.
    let screen: std::sync::Arc<dyn super::uia::Screen> = super::real_screen().into();
    let s2 = screen.clone();
    let Ok(windows) = tokio::task::spawn_blocking(move || s2.windows()).await else { return Ok(None) };
    let target = match &named_app {
        // "…in Notepad": that app's window, wherever it is, brought to the front so the person sees what is being pointed at.
        Some(name) => match super::uia::pick_named(&windows, name) {
            Some(t) => t.clone(),
            None if COMMON_APPS.contains(&name.as_str()) => {
                super::activity(app, "Guided help", false, &format!("{name} is not open"));
                return Ok(Some((SourceBook::default(), format!("The user asked how to do something in {name}, but {name} is not open. Tell them that in one short sentence and offer to open it (they can say \"open {name}\"), then answer briefly from general knowledge."))));
            }
            None => return Ok(None),
        },
        None => match super::uia::pick_target(&windows).cloned() {
            Some(t) => t,
            None => {
                super::activity(app, "Guided help", false, "no window to look at");
                return Ok(Some((SourceBook::default(), "BYTE could not find a window to look at (only BYTE itself is open). Tell the user in one short sentence, then answer from general knowledge if you can.".to_string())));
            }
        },
    };
    if let Err(r) = allowed(&target.app, &target.title, &settings.screen_exclude_apps) {
        super::activity(app, "Guided help", false, &r);
        return Ok(Some((SourceBook::default(), format!("BYTE did not look at the window: {r} Tell the user that in one short sentence, then answer the question from general knowledge if you can."))));
    }
    if named_app.is_some() {
        let (s4, title) = (screen.clone(), target.title.clone());
        let _ = tokio::task::spawn_blocking(move || s4.focus_window(&title)).await;
        tokio::time::sleep(std::time::Duration::from_millis(500)).await;
    }
    let (s3, id) = (screen.clone(), target.id);
    let snap = match tokio::task::spawn_blocking(move || s3.snapshot_window(id)).await {
        Ok(Ok(s)) => s,
        _ => return Ok(None),
    };
    // The reading's own title may differ from the list's: judge that one too, before anything is used.
    if let Err(r) = allowed(&snap.app, &snap.title, &settings.screen_exclude_apps) {
        super::activity(app, "Guided help", false, &r);
        return Ok(Some((SourceBook::default(), format!("BYTE did not look at the window: {r} Tell the user that in one short sentence, then answer the question from general knowledge if you can."))));
    }
    let plan = match make_plan(app, &goal, &snap).await {
        Ok(mut p) => {
            p.hwnd = target.id;
            p
        }
        Err(e) => {
            super::activity(app, "Guided help", false, &e);
            // A model that is too small is the usual reason: say so, with the way to get a better one, instead of answering as if nothing happened.
            if let Some(say) = super::small_model_say(app).await {
                return Ok(Some((SourceBook::default(), format!("BYTE tried to plan the steps on the person's screen but could not. {say} Tell the user that in two short sentences, then answer their question briefly from general knowledge."))));
            }
            return Ok(None);
        }
    };
    super::activity(app, "Guided help: planned", true, &format!("{} steps for {}", plan.steps.len(), plan.app));
    send(ChatEvent::Guide(plan.offer()))?;
    let notes = notes_for(&plan);
    store(plan);
    Ok(Some((SourceBook::default(), notes)))
}

// ------------------------------------------------------------------ keys and flags from outside

static CMD: Mutex<Option<Cmd>> = Mutex::new(None);
static RUNNING: AtomicBool = AtomicBool::new(false);

pub fn running() -> bool {
    RUNNING.load(Ordering::SeqCst)
}

/// A spoken or button command for the walk-through (taken by the loop at its next look).
pub fn send_cmd(c: Cmd) {
    if let Ok(mut s) = CMD.lock() {
        *s = Some(c);
    }
}

fn take_cmd() -> Option<Cmd> {
    CMD.lock().ok().and_then(|mut s| s.take())
}

/// A walk-through ends by itself after this long, whatever else happens.
pub const MAX_WALK_SECS: u64 = 600;

pub fn walk_expired(elapsed_secs: u64) -> bool {
    elapsed_secs > MAX_WALK_SECS
}

/// The screen `(left, top, right, bottom)` that holds a point, among all screens; the first one when none does.
pub fn pick_monitor(screens: &[(i32, i32, i32, i32)], p: (i32, i32)) -> Option<(i32, i32, i32, i32)> {
    screens.iter().copied().find(|&(l, t, r, b)| p.0 >= l && p.0 < r && p.1 >= t && p.1 < b).or_else(|| screens.first().copied())
}

// ------------------------------------------------------------------ Windows: the overlay and the loop

#[cfg(windows)]
mod win {
    use super::*;
    use std::time::{Duration, Instant};
    use tauri::{Manager, PhysicalPosition, PhysicalSize, WebviewUrl, WebviewWindowBuilder};
    use windows::Win32::Foundation::POINT;
    use windows::Win32::UI::Input::KeyboardAndMouse::{GetAsyncKeyState, VK_ESCAPE, VK_F8, VK_F9, VK_LBUTTON};
    use windows::Win32::UI::WindowsAndMessaging::GetCursorPos;

    const LABEL: &str = "guidering";
    const LOOK_EVERY: Duration = Duration::from_millis(800);
    const TICK: Duration = Duration::from_millis(100);

    fn down(vk: i32) -> bool {
        (unsafe { GetAsyncKeyState(vk) } as u16) & 0x8000 != 0
    }

    fn cursor() -> Option<(i32, i32)> {
        let mut p = POINT::default();
        unsafe { GetCursorPos(&mut p) }.ok().map(|_| (p.x, p.y))
    }

    fn overlay(app: &AppHandle) -> Option<tauri::WebviewWindow> {
        if let Some(w) = app.get_webview_window(LABEL) {
            return Some(w);
        }
        let w = WebviewWindowBuilder::new(app, LABEL, WebviewUrl::App("guide.html".into()))
            .title("BYTE guide")
            .inner_size(340.0, 90.0)
            .decorations(false)
            .transparent(true)
            .shadow(false)
            .resizable(false)
            .always_on_top(true)
            .skip_taskbar(true)
            .focusable(false)
            .focused(false)
            .visible(false)
            .build()
            .map_err(|e| log::warn!("guide overlay: {e}"))
            .ok()?;
        // Clicks go through it, always.
        let _ = w.set_ignore_cursor_events(true);
        Some(w)
    }

    /// The screen that holds `at` (the element), or the primary one.
    fn screen_bounds(app: &AppHandle, at: Option<(i32, i32)>) -> (i32, i32, i32, i32) {
        let rect = |m: &tauri::Monitor| (m.position().x, m.position().y, m.position().x + m.size().width as i32, m.position().y + m.size().height as i32);
        let all: Vec<_> = app.available_monitors().unwrap_or_default().iter().map(rect).collect();
        if let (Some(p), Some(b)) = (at, at.and_then(|p| pick_monitor(&all, p))) {
            let _ = p;
            return b;
        }
        match app.primary_monitor() {
            Ok(Some(m)) => rect(&m),
            _ => (0, 0, 1920, 1080),
        }
    }

    fn show(app: &AppHandle, rect: Option<Rect>, text: &str, hint: &str, n: usize, total: usize) {
        let Some(w) = overlay(app) else { return };
        let l = layout(rect, screen_bounds(app, rect.map(|r| r.center())));
        // Move first: the window's scale is the scale of the screen it is on.
        let _ = w.set_position(PhysicalPosition::new(l.x, l.y));
        let _ = w.set_size(PhysicalSize::new(l.w.max(1) as u32, l.h.max(1) as u32));
        let scale = w.scale_factor().unwrap_or(1.0);
        let ring = l.ring.map(|(x, y, rw, rh)| json!({ "x": x as f64 / scale, "y": y as f64 / scale, "w": rw as f64 / scale, "h": rh as f64 / scale }));
        let payload = json!({ "ring": ring, "below": l.caption_below, "text": text, "hint": hint, "n": n, "total": total });
        let _ = w.show();
        let _ = w.eval(format!("window.setGuide && window.setGuide({payload})"));
    }

    fn hide(app: &AppHandle) {
        if let Some(w) = app.get_webview_window(LABEL) {
            let _ = w.eval("window.setGuide && window.setGuide(null)");
            let _ = w.hide();
            let _ = w.close();
        }
    }

    /// Hides the overlay and frees the "running" flag however the walk-through ends, a panic included.
    struct Cleanup {
        app: AppHandle,
    }

    impl Drop for Cleanup {
        fn drop(&mut self) {
            hide(&self.app);
            RUNNING.store(false, Ordering::SeqCst);
            if std::thread::panicking() {
                use tauri::Emitter;
                let _ = self.app.emit("guide://state", json!({ "running": false, "done": false }));
            }
        }
    }

    /// A fresh reading of the guided window, only if it may still be read (the never-look list, kids mode).
    /// `Err((true, why))` ends the walk-through (off, kids mode, the window is gone or may not be read); `Err((false, why))` is a
    /// reading that failed this time and is tried again.
    fn reread(app: &AppHandle, screen: &dyn super::super::uia::Screen, plan: &GuidePlan) -> Result<Snapshot, (bool, String)> {
        use super::super::uia::ScreenError;
        let s = super::super::settings_now(app);
        if s.as_ref().is_some_and(|s| s.kids_mode || !s.guided_help) {
            return Err((true, "guided help is off".into()));
        }
        let excludes = s.map(|s| s.screen_exclude_apps).unwrap_or_else(crate::screen::policy::default_excludes);
        let snap = screen.snapshot_window(plan.hwnd).map_err(|e| (!matches!(e, ScreenError::Failed(_)), e.to_string()))?;
        allowed(&snap.app, &snap.title, &excludes).map_err(|e| (true, e))?;
        Ok(snap)
    }

    /// Runs one walk-through to its end. Blocking: call it from a thread.
    pub fn run(app: AppHandle, plan: GuidePlan) {
        if RUNNING.swap(true, Ordering::SeqCst) {
            return;
        }
        let _cleanup = Cleanup { app: app.clone() };
        let started = Instant::now();
        super::super::activity(&app, "Guided help: show me", true, &plan.goal);
        let screen = super::super::real_screen();
        let _ = screen.focus_window(&plan.title);
        let mut sess = Session::new(plan.steps.clone());
        let mut total = plan.steps.len();
        let mut last_look = Instant::now() - LOOK_EVERY;
        let mut was_down = down(VK_LBUTTON.0 as i32);
        let mut esc_was = down(VK_ESCAPE.0 as i32);
        let mut f8_was = down(VK_F8.0 as i32);
        let mut f9_was = down(VK_F9.0 as i32);
        let mut ring: Option<Rect> = None;
        let mut shown = usize::MAX;
        let mut paused = false;
        take_cmd();
        let mut cur: Option<usize> = Some(0);
        let mut missing = false;
        let end = loop {
            std::thread::sleep(TICK);
            if walk_expired(started.elapsed().as_secs()) {
                super::super::activity(&app, "Guided help: ended after ten minutes", true, "");
                break Phase::Stopped;
            }
            let mut o = Obs::default();
            // Keys: Esc stops, F8 is Next. Edges only.
            let (esc, f8, f9, lb) = (down(VK_ESCAPE.0 as i32), down(VK_F8.0 as i32), down(VK_F9.0 as i32), down(VK_LBUTTON.0 as i32));
            if esc && !esc_was {
                o.cmd = Some(Cmd::Stop);
            } else if f8 && !f8_was {
                o.cmd = Some(Cmd::Next);
            } else if f9 && !f9_was {
                o.cmd = Some(Cmd::Replan);
            }
            (esc_was, f8_was, f9_was) = (esc, f8, f9);
            if o.cmd.is_none() {
                o.cmd = take_cmd();
            }
            // Which program is in front (cheap).
            if let Ok((app_name, _)) = screen.foreground() {
                let mine = app_name.eq_ignore_ascii_case(&plan.app);
                let byte = app_name.eq_ignore_ascii_case("byte");
                o.program_changed = !mine && !byte && !app_name.is_empty();
                if (!mine) != paused {
                    paused = !mine;
                    if paused {
                        hide(&app);
                        shown = usize::MAX;
                    }
                }
            }
            if lb && !was_down {
                if let (Some(r), Some((x, y))) = (ring, cursor()) {
                    o.clicked_in_target = x >= r.left && x <= r.right && y >= r.top && y <= r.bottom;
                }
            }
            was_down = lb;
            // Look at the window now and then, and find the element again.
            let idx = match &sess.phase {
                Phase::At(i) | Phase::Missing(i) => Some(*i),
                _ => None,
            };
            if !paused && idx.is_some() && last_look.elapsed() >= LOOK_EVERY {
                last_look = Instant::now();
                let read = idx.map(|_| reread(&app, &*screen, &plan));
                if let Some(Err((true, why))) = &read {
                    super::super::activity(&app, "Guided help: stopped", false, why);
                    break Phase::Left;
                }
                if let (Some(i), Some(Ok(snap))) = (idx, read) {
                    if let Some(t) = &sess.steps[i].target {
                        match refind(&snap, t) {
                            Some(e) => {
                                o.target = Found::Yes(e.rect);
                                ring = Some(e.rect);
                            }
                            None => o.target = Found::No,
                        }
                    }
                }
            }
            let out = sess.observe(&o);
            match out {
                Out::Finished(p) => break p,
                Out::Show(i) => {
                    cur = Some(i);
                    missing = false;
                    shown = usize::MAX;
                    ring = None;
                    last_look = Instant::now() - LOOK_EVERY;
                }
                Out::Missing(i) => {
                    cur = Some(i);
                    missing = true;
                    shown = usize::MAX;
                }
                Out::Replan => {
                    // Look again: a fresh reading of the window in front and a new plan for the same question.
                    super::super::activity(&app, "Guided help: looking again", true, "");
                    show(&app, None, "Looking again…", "Esc stop", 0, total);
                    let fresh = reread(&app, &*screen, &plan).map_err(|e| e.1).and_then(|snap| tauri::async_runtime::block_on(make_plan(&app, &plan.goal, &snap)).map(|mut p| {
                        p.hwnd = plan.hwnd;
                        p
                    }));
                    match fresh {
                        Ok(p) if !p.steps.is_empty() => {
                            total = p.steps.len();
                            sess = Session::new(p.steps.clone());
                            store(p);
                            (cur, missing, ring, shown) = (Some(0), false, None, usize::MAX);
                            last_look = Instant::now() - LOOK_EVERY;
                        }
                        other => {
                            super::super::activity(&app, "Guided help: could not plan again", false, &other.err().unwrap_or_default());
                            break Phase::Stopped;
                        }
                    }
                }
                Out::Nothing => {
                    if matches!(o.target, Found::Yes(_)) && missing {
                        missing = false;
                        shown = usize::MAX;
                    }
                }
            }
            if let (Some(i), false) = (cur, paused) {
                let step = &sess.steps[i.min(total - 1)];
                let want = i * 2 + usize::from(missing) + if ring.is_some() { 1000 } else { 0 };
                if shown != want {
                    shown = want;
                    let text = if missing { format!("I can't find “{}” here now. Press F8 to skip it, or F9 (or say “look again”) to plan again.", step.target.as_ref().map(|t| t.name.as_str()).unwrap_or("it")) } else { caption(step) };
                    show(&app, if missing { None } else { ring.or(step.target.as_ref().map(|t| t.rect)) }, &text, "F8 next · F9 look again · Esc stop", i + 1, total);
                }
            }
        };
        hide(&app);
        super::super::activity(&app, "Guided help: finished", end == Phase::Done, &format!("{end:?}"));
        use tauri::Emitter;
        let _ = app.emit("guide://state", json!({ "running": false, "done": end == Phase::Done }));
    }
}

#[cfg(windows)]
pub fn run_walkthrough(app: AppHandle, plan: GuidePlan) {
    win::run(app, plan);
}

#[cfg(not(windows))]
pub fn run_walkthrough(_app: AppHandle, _plan: GuidePlan) {}

// ------------------------------------------------------------------ commands

/// "Show me as I go".
#[tauri::command]
pub async fn guide_start(app: AppHandle, id: String) -> AppResult<()> {
    if !super::settings_now(&app).is_some_and(|s| s.guided_help) {
        return Err(AppError::msg("Guided help is off (Settings → PC control)."));
    }
    if !cfg!(windows) {
        return Err(AppError::msg("Guided help needs Windows."));
    }
    if super::settings_now(&app).is_some_and(|s| s.kids_mode) {
        return Err(AppError::msg("Guided help is not available in kids mode."));
    }
    let plan = get(&id).ok_or_else(|| AppError::msg("That walk-through is gone. Ask again and BYTE will make a new one."))?;
    if running() {
        return Err(AppError::msg("A walk-through is already on. Press Esc to end it."));
    }
    std::thread::spawn(move || run_walkthrough(app, plan));
    Ok(())
}

/// The steps "Do it for me" runs: a switch to the window first (the button was pressed in BYTE's own window), then the plan.
pub fn steps_for_do(plan: &GuidePlan) -> Vec<Step> {
    let title: String = plan.title.chars().take(200).collect();
    let mut v = Vec::new();
    if !title.trim().is_empty() {
        v.push(Step { action: Action::Focus { title }, expect: actions::Expect::default() });
    }
    v.extend(plan.steps.iter().map(|s| s.step.clone()));
    v
}

/// A native question for an approval card when there is no chat to show it in.
fn ask_native(app: &AppHandle, ask: &crate::web_agent::ApprovalAsk) -> bool {
    use tauri_plugin_dialog::{DialogExt, MessageDialogButtons, MessageDialogKind};
    let body = ask.fields.iter().map(|f| format!("{}: {}", f.label, f.value)).collect::<Vec<_>>().join("\n");
    app.dialog().message(format!("{}\n\n{body}", ask.title)).title("BYTE").kind(MessageDialogKind::Warning).buttons(MessageDialogButtons::OkCancelCustom("Allow".into(), "Don't allow".into())).blocking_show()
}

/// "Do it for me": the same plan through the executor, with its approval cards (as dialogs here), log, limits and Stop banner.
#[tauri::command]
pub async fn guide_do(app: AppHandle, id: String) -> AppResult<String> {
    let settings = super::settings_now(&app).ok_or_else(|| AppError::msg("Settings are busy, try again."))?;
    if !settings.guided_help {
        return Err(AppError::msg("Guided help is off (Settings → PC control)."));
    }
    if !settings.pc_control {
        return Err(AppError::msg("Turn on “Let BYTE use this PC for you” in Settings → PC control first, so BYTE may do it."));
    }
    if settings.kids_mode {
        return Err(AppError::msg("Guided help is not available in kids mode."));
    }
    let plan = get(&id).ok_or_else(|| AppError::msg("That walk-through is gone. Ask again and BYTE will make a new one."))?;
    // The window must still be one BYTE may read (its title or program may have changed since the plan was made).
    {
        let screen: std::sync::Arc<dyn super::uia::Screen> = super::real_screen().into();
        let (id, excludes) = (plan.hwnd, settings.screen_exclude_apps.clone());
        let ok = tokio::task::spawn_blocking(move || screen.snapshot_window(id).map_err(|e| e.to_string()).and_then(|s| allowed(&s.app, &s.title, &excludes))).await.map_err(|e| AppError::msg(e.to_string()))?;
        if let Err(e) = ok {
            super::activity(&app, "Guided help: do it for me refused", false, &e);
            return Err(AppError::msg(format!("BYTE did not do it: {e}")));
        }
    }
    super::activity(&app, "Guided help: do it for me", true, &plan.goal);
    let cancel = CancellationToken::new();
    let app2 = app.clone();
    let send = move |e: ChatEvent| -> AppResult<()> {
        if let ChatEvent::Approval(ask) = e {
            let (a, id) = (app2.clone(), ask.id.clone());
            std::thread::spawn(move || {
                let ok = ask_native(&a, &ask);
                crate::web_agent::answer(&id, ok);
            });
        }
        Ok(())
    };
    let report = super::run_plan_steps(&app, steps_for_do(&plan), &plan.goal, &cancel, &send).await;
    Ok(report.summary)
}

/// "No thanks" (forgets the plan) .
#[tauri::command]
pub fn guide_dismiss(id: String) {
    forget(&id);
}

/// The Stop, Next and Skip buttons of the card.
#[tauri::command]
pub fn guide_command(what: String) {
    match what.as_str() {
        "next" => send_cmd(Cmd::Next),
        "skip" => send_cmd(Cmd::Skip),
        "stop" => send_cmd(Cmd::Stop),
        "replan" => send_cmd(Cmd::Replan),
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn a_named_app_is_found_in_a_how_to() {
        use super::howto_named_app as n;
        assert_eq!(n("How do I change the font in Notepad?").map(|x| x.1), Some("notepad".into()));
        assert_eq!(n("hey byte, how can I turn on dark mode in the Settings app").map(|x| x.1), Some("settings".into()));
        assert_eq!(n("how do I add a tab using Edge").map(|x| x.1), Some("edge".into()));
        assert!(n("what is the capital of France").is_none(), "not a how-to");
        assert!(n("how do I make pasta").is_none(), "names no app");
    }

    use super::*;
    use crate::control::uia::fake::{el, snap};

    #[test]
    fn how_to_questions_about_the_screen_are_recognised() {
        for q in [
            "how do I change my shipping address on this site",
            "Hey BYTE, how do i turn on dark mode in this app?",
            "where is the setting for notifications",
            "where's the button to download my invoice",
            "I can't find the button to log out",
            "i can't figure out how to do this on this page",
            "how can I add a payee on this website",
            "I'm stuck on this page, where do I click",
            "where do i find the privacy settings",
        ] {
            assert!(howto_ask(q).is_some(), "{q}");
        }
    }

    #[test]
    fn ordinary_chat_is_not_taken() {
        for q in [
            "how do I make pasta",
            "how to tie a tie",
            "where is the Eiffel tower",
            "how do I install python on windows",
            "where can I find a good pizza near me",
            "how do I get to the airport",
            "tell me about the history of buttons",
            "I can't find my keys",
            "what's the weather",
            "open notepad",
            "how do I beat this boss in this game",
            "where is the nearest button shop",
            "how are you",
        ] {
            assert!(howto_ask(q).is_none(), "{q}: {:?}", howto_ask(q));
        }
    }

    #[test]
    fn spoken_commands_get_short_answers() {
        assert_eq!(voice_command("look again"), Some(Cmd::Replan));
        assert_eq!(voice_reply(Cmd::Next), "Next.");
        assert!(voice_reply(Cmd::Stop).contains("stopped"));
        // Spoken only while a walk-through is on, so media "next" is untouched otherwise.
        assert!(!running());
    }

    #[test]
    fn walk_through_words() {
        assert_eq!(voice_command("next"), Some(Cmd::Next));
        assert_eq!(voice_command("Hey BYTE, skip"), Some(Cmd::Skip));
        assert_eq!(voice_command("stop."), Some(Cmd::Stop));
        assert_eq!(voice_command("re-plan"), Some(Cmd::Replan));
        assert_eq!(voice_command("next to the door"), None);
    }

    fn account_page() -> Snapshot {
        let mut s = snap("My account - Shop", vec![el(1, "Account", Kind::Link), el(2, "Addresses", Kind::Link), el(3, "Password", Kind::Edit), el(4, "Save", Kind::Button)]);
        s.elements[2].sensitive = true;
        s
    }

    const REPLY: &str = "{\"steps\":[{\"action\":\"click\",\"id\":1,\"reason\":\"Click Account\"},{\"action\":\"click\",\"id\":2,\"reason\":\"Open Addresses\",\"expect\":{\"window\":\"Addresses\"}},{\"action\":\"type\",\"text\":\"12 Oak Street\",\"reason\":\"Type your new street\"}]}";

    #[test]
    fn the_plan_keeps_reasons_and_targets() {
        let p = plan_from_reply(REPLY, "change my address", &account_page(), 12).unwrap();
        assert_eq!(p.steps.len(), 3);
        assert_eq!(p.steps[0].reason, "Click Account");
        assert_eq!(p.steps[0].target.as_ref().unwrap().name, "Account");
        assert!(p.steps[2].target.is_none());
        assert_eq!(p.steps[1].step.expect.window.as_deref(), Some("Addresses"));
        assert_eq!(p.app, "notepad");
        let o = p.offer();
        assert_eq!(o.steps[2].text, "Type: 12 Oak Street", "typed words are shown before they are typed");
        assert_eq!(o.steps[0].reason, "Click Account");
    }

    #[test]
    fn a_missing_reason_is_made_from_the_element() {
        let p = plan_from_reply("[{\"action\":\"click\",\"id\":2},{\"action\":\"key\",\"combo\":\"ctrl+s\"}]", "g", &account_page(), 12).unwrap();
        assert_eq!(p.steps[0].reason, "Click Addresses");
        assert_eq!(p.steps[1].reason, "Press ctrl+s");
    }

    #[test]
    fn plans_with_secret_missing_or_refused_things_are_errors() {
        let s = account_page();
        assert!(plan_from_reply("[{\"action\":\"click\",\"id\":3}]", "g", &s, 12).is_err(), "a password field");
        assert!(plan_from_reply("[{\"action\":\"click\",\"id\":9}]", "g", &s, 12).is_err(), "not there");
        assert!(plan_from_reply("[{\"action\":\"open\",\"target\":\"cmd\"}]", "g", &s, 12).is_err(), "refused stays refused");
        assert!(plan_from_reply("[{\"action\":\"power\",\"kind\":\"shutdown\"}]", "g", &s, 12).is_err());
        assert!(plan_from_reply("[{\"action\":\"click\",\"id\":1},{\"action\":\"click\",\"id\":2}]", "g", &s, 1).is_err(), "the step limit");
        assert!(plan_from_reply("not json", "g", &s, 12).is_err());
    }

    #[test]
    fn plans_are_kept_in_memory_by_id_and_capped() {
        let p = plan_from_reply(REPLY, "g", &account_page(), 12).unwrap();
        let id = p.id.clone();
        store(p.clone());
        assert_eq!(get(&id).unwrap().goal, "g");
        forget(&id);
        assert!(get(&id).is_none());
        let mut ids = vec![];
        for _ in 0..MAX_SAVED + 3 {
            let mut q = p.clone();
            q.id = uuid::Uuid::new_v4().simple().to_string();
            ids.push(q.id.clone());
            store(q);
        }
        assert!(get(&ids[0]).is_none(), "the oldest went");
        assert!(get(ids.last().unwrap()).is_some());
    }

    fn session() -> Session {
        Session::new(plan_from_reply(REPLY, "g", &account_page(), 12).unwrap().steps)
    }

    fn rect() -> Rect {
        Rect { left: 10, top: 10, right: 50, bottom: 30 }
    }

    #[test]
    fn next_and_skip_advance_and_the_last_one_finishes() {
        let mut s = session();
        assert_eq!(s.phase, Phase::At(0));
        assert_eq!(s.observe(&Obs { cmd: Some(Cmd::Next), ..Default::default() }), Out::Show(1));
        assert_eq!(s.observe(&Obs { cmd: Some(Cmd::Skip), ..Default::default() }), Out::Show(2));
        assert_eq!(s.observe(&Obs { cmd: Some(Cmd::Next), ..Default::default() }), Out::Finished(Phase::Done));
        assert!(s.finished());
        assert_eq!(s.observe(&Obs { cmd: Some(Cmd::Next), ..Default::default() }), Out::Nothing);
    }

    #[test]
    fn clicking_the_ringed_element_advances_but_not_for_a_typing_step() {
        let mut s = session();
        assert_eq!(s.observe(&Obs { clicked_in_target: true, ..Default::default() }), Out::Show(1));
        assert_eq!(s.observe(&Obs { clicked_in_target: true, ..Default::default() }), Out::Show(2));
        assert_eq!(s.observe(&Obs { clicked_in_target: true, ..Default::default() }), Out::Nothing, "a typing step has no element to click");
    }

    #[test]
    fn stop_ends_at_once_from_any_state() {
        let mut s = session();
        assert_eq!(s.observe(&Obs { cmd: Some(Cmd::Stop), ..Default::default() }), Out::Finished(Phase::Stopped));
        let mut m = session();
        m.observe(&Obs { target: Found::No, ..Default::default() });
        assert_eq!(m.observe(&Obs { cmd: Some(Cmd::Stop), ..Default::default() }), Out::Finished(Phase::Stopped));
    }

    #[test]
    fn a_missing_element_is_reported_once_and_recovers_when_it_returns() {
        let mut s = session();
        assert_eq!(s.observe(&Obs { target: Found::No, ..Default::default() }), Out::Missing(0));
        assert_eq!(s.observe(&Obs { target: Found::No, ..Default::default() }), Out::Nothing);
        assert_eq!(s.observe(&Obs { target: Found::Yes(rect()), ..Default::default() }), Out::Show(0));
        assert_eq!(s.phase, Phase::At(0));
    }

    #[test]
    fn a_missing_element_can_be_skipped_or_replanned() {
        let mut s = session();
        s.observe(&Obs { target: Found::No, ..Default::default() });
        assert_eq!(s.observe(&Obs { cmd: Some(Cmd::Replan), ..Default::default() }), Out::Replan);
        assert_eq!(s.observe(&Obs { cmd: Some(Cmd::Skip), ..Default::default() }), Out::Show(1));
    }

    #[test]
    fn going_to_another_program_ends_it() {
        let mut s = session();
        assert_eq!(s.observe(&Obs { program_changed: true, ..Default::default() }), Out::Finished(Phase::Left));
    }

    #[test]
    fn an_unlooked_at_tick_changes_nothing() {
        let mut s = session();
        assert_eq!(s.observe(&Obs::default()), Out::Nothing);
        assert_eq!(s.phase, Phase::At(0));
        assert!(Session::new(vec![]).finished());
    }

    fn target(id: u32, name: &str, kind: Kind) -> TargetRef {
        TargetRef { id, name: name.into(), kind, rect: rect() }
    }

    #[test]
    fn an_element_is_found_again_when_the_numbers_moved() {
        let now = snap("W", vec![el(1, "Home", Kind::Link), el(2, "Account", Kind::Link), el(3, "Account settings", Kind::Link), el(4, "Account", Kind::Button)]);
        // Same id, same name: found there.
        assert_eq!(refind(&now, &target(2, "Account", Kind::Link)).unwrap().id, 2);
        // The id moved: found by name and kind.
        assert_eq!(refind(&now, &target(7, "account", Kind::Button)).unwrap().id, 4);
        assert_eq!(refind(&now, &target(9, "Account", Kind::Link)).unwrap().id, 2);
        // Only a partial name: the one element that contains it.
        assert_eq!(refind(&now, &target(9, "Account sett", Kind::Link)).unwrap().id, 3);
        // Two partial matches: not guessed.
        assert!(refind(&now, &target(9, "Acc", Kind::Link)).is_none());
        // Gone.
        assert!(refind(&now, &target(2, "Orders", Kind::Link)).is_none());
        assert!(refind(&now, &target(2, "", Kind::Link)).is_none());
    }

    #[test]
    fn a_secret_field_is_never_found() {
        let mut s = snap("W", vec![el(1, "Password", Kind::Edit)]);
        s.elements[0].sensitive = true;
        assert!(refind(&s, &target(1, "Password", Kind::Edit)).is_none());
    }

    #[test]
    fn the_overlay_stays_on_the_screen_and_puts_the_caption_where_there_is_room() {
        let screen = (0, 0, 1920, 1080);
        let mid = layout(Some(Rect { left: 400, top: 300, right: 520, bottom: 340 }), screen);
        assert!(mid.caption_below);
        assert_eq!((mid.x, mid.y), (388, 288));
        assert_eq!(mid.ring, Some((12, 12, 120, 40)));
        assert!(mid.w >= MIN_W && mid.y + mid.h <= 1080);
        let low = layout(Some(Rect { left: 400, top: 1030, right: 520, bottom: 1070 }), screen);
        assert!(!low.caption_below, "no room under it");
        assert!(low.y >= 0 && low.y + low.h <= 1080, "{low:?}");
        let right = layout(Some(Rect { left: 1880, top: 100, right: 1915, bottom: 130 }), screen);
        assert!(right.x + right.w <= 1920 && right.x >= 0, "{right:?}");
        assert!(right.ring.unwrap().0 > 0);
        let none = layout(None, screen);
        assert!(none.ring.is_none() && none.x >= 0 && none.x + none.w <= 1920);
        assert!(layout(Some(Rect::default()), screen).ring.is_none(), "an empty rect has no ring");
    }

    #[test]
    fn do_it_for_me_switches_to_the_window_first() {
        let p = plan_from_reply(REPLY, "g", &account_page(), 12).unwrap();
        let steps = steps_for_do(&p);
        assert_eq!(steps.len(), 4);
        assert!(matches!(&steps[0].action, Action::Focus { title } if title == "My account - Shop"));
        assert_eq!(steps[1], p.steps[0].step);
    }

    #[test]
    fn the_overlay_goes_to_the_screen_the_element_is_on() {
        let screens = [(0, 0, 1920, 1080), (1920, 0, 3840, 1080), (-1280, 0, 0, 1024)];
        assert_eq!(pick_monitor(&screens, (2000, 500)), Some((1920, 0, 3840, 1080)));
        assert_eq!(pick_monitor(&screens, (-10, 10)), Some((-1280, 0, 0, 1024)));
        assert_eq!(pick_monitor(&screens, (100, 100)), Some((0, 0, 1920, 1080)));
        assert_eq!(pick_monitor(&screens, (9000, 9000)), Some((0, 0, 1920, 1080)), "off every screen: the first");
        assert_eq!(pick_monitor(&[], (1, 1)), None);
    }

    #[test]
    fn a_walk_through_ends_by_itself() {
        assert!(!walk_expired(0) && !walk_expired(MAX_WALK_SECS));
        assert!(walk_expired(MAX_WALK_SECS + 1));
    }

    #[test]
    fn the_cloud_gets_nothing_from_the_window() {
        let p = plan_from_reply(REPLY, "g", &account_page(), 12).unwrap();
        let local = notes_for(&p);
        assert!(local.contains("My account - Shop") && local.contains("Click Account"));
        let cloud = notes_for_cloud("guide", local.clone());
        for leak in ["My account", "notepad", "Click Account", "Addresses", "12 Oak Street"] {
            assert!(!cloud.contains(leak), "{leak}");
        }
        assert_eq!(notes_for_cloud("timer", "x".into()), "x", "other routes are untouched");
    }

    #[test]
    fn the_window_policy_is_the_screen_tracks() {
        let ex = crate::screen::policy::default_excludes();
        assert!(allowed("chrome", "Shop", &ex).is_ok());
        assert!(allowed("byte", "BYTE", &ex).is_err(), "BYTE's own window");
        assert!(allowed("1password", "Vault", &ex).is_err());
        assert!(allowed("chrome", "Chase Online - Sign in", &ex).is_err());
        assert!(allowed("firefox", "Private Browsing", &ex).is_err());
    }

    #[test]
    fn the_answer_notes_ask_whether_to_help_and_do_not_claim_anything() {
        let p = plan_from_reply(REPLY, "g", &account_page(), 12).unwrap();
        let n = notes_for(&p);
        assert!(n.contains("Click Account") && n.contains("Show me as I go") && n.contains("Do not say you did anything"));
        assert!(!n.starts_with("Done:"));
    }

    #[test]
    #[ignore = "needs an interactive desktop: shows the real overlay for two seconds"]
    fn live_overlay_shows_and_hides() {
        // Run by hand with a window in front; checks nothing but that the loop starts and ends without panicking.
    }
}
