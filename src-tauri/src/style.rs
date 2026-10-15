//! How BYTE talks, beyond the five sliders (prompt.rs): a **mode** the person picks, and **Auto**, which learns their style slowly.
//!
//! - **Casual**: a relaxed friend, plain words, contractions, light humor.
//! - **Direct**: the answer first, no pleasantries, blunt when asked for an opinion.
//! - **Unhinged** (an 18+ feature, for entertainment: Settings shows a warning when it is switched on, and kids mode never has it):
//!   swears freely, roasts, and jokes crude, dirty, dark and cartoonishly violent, even about serious-sounding things. The owner's
//!   call is that the warning does the work ("don't take its advice; use it for weather, timers, lists and facts"). Three things stay
//!   in `UNHINGED`: nothing sexual involving minors; no serious planning or urging of real violence, break-ins or harassment of real
//!   people (a joke stays a joke); and a real crisis gets kindness instead of a joke.
//! - **Auto**: Casual or Direct, shaped by how the person writes. It learns from numbers only (message length, whether they swear,
//!   whether they are formal), never keeps what they wrote, and moves slowly on purpose: one swear, or "f--- you BYTE" as a joke, changes
//!   nothing. It takes a steady pattern over many messages and several different days, and it only mirrors swearing when the person has
//!   allowed it ("Allow swearing"). It never goes as far as Unhinged on its own.
//!
//! Learned numbers live in `style.json` in BYTE's data folder. Private chats and kids mode neither use nor teach it.

use std::collections::VecDeque;
use std::path::Path;

use serde::{Deserialize, Serialize};

/// How many recent messages the picture is drawn from.
const WINDOW: usize = 300;
/// Fewest messages before Auto concludes anything.
const MIN_MESSAGES: usize = 40;
/// Swearing must show on this many different days, and in at least this share of messages, before Auto mirrors it.
const SWEAR_DAYS: usize = 4;
const SWEAR_ON: f32 = 0.06;
/// Once on, it stays on until the share falls below this (so it does not flip back and forth).
const SWEAR_OFF: f32 = 0.025;

/// One message, as numbers. What was written is not kept.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq)]
pub struct Obs {
    pub day: u32,
    pub words: u16,
    /// Swore, and not at BYTE: that is the person's own way of talking.
    pub swears: bool,
    pub formal: bool,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct Learned {
    pub seen: VecDeque<Obs>,
    /// Auto currently mirrors swearing (with the hysteresis above).
    pub swearing: bool,
    /// A separate picture for each taught voice (by lowercase name). The fields above are the default: unknown speakers, and
    /// everything typed. Files from before this existed have no `people` and load as they were.
    #[serde(default, skip_serializing_if = "std::collections::BTreeMap::is_empty")]
    pub people: std::collections::BTreeMap<String, Learned>,
}

/// Most people kept apart; past this, new voices share the default picture.
const MAX_PEOPLE: usize = 20;

/// The key a speaker's picture is kept under: their name, lowercased and trimmed. None for unknown or blank names.
pub fn person_key(name: Option<&str>) -> Option<String> {
    let k: String = name?.trim().chars().take(40).collect::<String>().to_lowercase();
    (!k.is_empty()).then_some(k)
}

/// What Auto has concluded so far.
#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct Profile {
    pub enough: bool,
    pub terse: bool,
    pub formal: bool,
    pub swears: bool,
}

const SWEARS: &[&str] = &[
    "fuck", "fucking", "fucked", "fucker", "shit", "shitty", "bullshit", "damn", "goddamn", "dammit", "ass", "asshole", "bitch", "bastard", "crap", "piss", "pissed", "dick",
    "wtf", "fck", "fuk", "sht",
];
const FORMAL: &[&str] = &["please", "kindly", "would you", "could you", "i would like", "may i", "thank you", "regards", "sincerely"];

/// Words lowercased, with the letters kept ("f*ck" and "f.u.c.k" lose their marks and read as "fck" and "fuck").
fn words(text: &str) -> Vec<String> {
    text.to_lowercase().split_whitespace().map(|w| w.chars().filter(|c| c.is_alphanumeric()).collect::<String>()).filter(|w| !w.is_empty()).collect()
}

pub fn swears(text: &str) -> bool {
    words(text).iter().any(|w| SWEARS.contains(&w.as_str()))
}

/// Swearing aimed at BYTE ("fuck you byte", "you're a useless piece of shit"): banter or anger, not how the person talks.
pub fn aimed_at_byte(text: &str) -> bool {
    let w = words(text);
    let has = |x: &str| w.iter().any(|y| y == x);
    let about_me = has("you") || has("u") || has("youre") || has("byte") || has("ur");
    about_me && (swears(text) || ["stupid", "dumb", "idiot", "useless", "suck", "sucks", "trash", "garbage", "worthless", "moron"].iter().any(|b| has(b)))
}

pub fn observe_text(text: &str, day: u32) -> Option<Obs> {
    let n = words(text).len();
    if n == 0 {
        return None;
    }
    let low = text.to_lowercase();
    Some(Obs { day, words: n.min(u16::MAX as usize) as u16, swears: swears(text) && !aimed_at_byte(text), formal: FORMAL.iter().any(|f| low.contains(f)) })
}

impl Learned {
    pub fn observe(&mut self, text: &str, day: u32) {
        let Some(o) = observe_text(text, day) else { return };
        self.seen.push_back(o);
        while self.seen.len() > WINDOW {
            self.seen.pop_front();
        }
        // The swearing switch moves only here, with its hysteresis.
        let n = self.seen.len();
        if n >= MIN_MESSAGES {
            let with: Vec<&Obs> = self.seen.iter().filter(|o| o.swears).collect();
            let share = with.len() as f32 / n as f32;
            let mut days: Vec<u32> = with.iter().map(|o| o.day).collect();
            days.sort_unstable();
            days.dedup();
            if !self.swearing && share >= SWEAR_ON && days.len() >= SWEAR_DAYS {
                self.swearing = true;
            } else if self.swearing && share < SWEAR_OFF {
                self.swearing = false;
            }
        }
    }

    /// The picture for this speaker: their own when they are a known voice (empty until learned), else the default.
    pub fn of(&self, speaker: Option<&str>) -> &Learned {
        static EMPTY: std::sync::OnceLock<Learned> = std::sync::OnceLock::new();
        match person_key(speaker) {
            Some(k) => self.people.get(&k).unwrap_or_else(|| EMPTY.get_or_init(Learned::default)),
            None => self,
        }
    }

    /// Learns one message into this speaker's own picture (the default's when unknown, or when too many people are kept already).
    pub fn observe_for(&mut self, speaker: Option<&str>, text: &str, day: u32) {
        match person_key(speaker) {
            Some(k) if self.people.contains_key(&k) || self.people.len() < MAX_PEOPLE => self.people.entry(k).or_default().observe(text, day),
            _ => self.observe(text, day),
        }
    }

    pub fn profile(&self) -> Profile {
        let n = self.seen.len();
        if n < MIN_MESSAGES {
            return Profile { enough: false, terse: false, formal: false, swears: false };
        }
        let mut lens: Vec<u16> = self.seen.iter().map(|o| o.words).collect();
        lens.sort_unstable();
        let formal = self.seen.iter().filter(|o| o.formal).count() as f32 / n as f32;
        Profile { enough: true, terse: lens[n / 2] <= 6, formal: formal >= 0.25, swears: self.swearing }
    }

    pub fn describe(&self) -> String {
        let mut out = self.describe_own();
        for (name, l) in &self.people {
            out.push_str(&format!("\n{name}: {}", l.describe_own()));
        }
        out
    }

    fn describe_own(&self) -> String {
        let p = self.profile();
        if !p.enough {
            return format!("Still learning ({} of {} messages so far).", self.seen.len(), MIN_MESSAGES);
        }
        let mut bits = vec![if p.terse { "you keep messages short" } else { "you write at normal length" }];
        if p.formal {
            bits.push("you are polite and formal");
        } else {
            bits.push("you write casually");
        }
        if p.swears {
            bits.push("you swear now and then");
        }
        let s = bits.join(", ");
        format!("From your last {} messages: {s}.", self.seen.len())
    }
}

pub fn load(path: &Path) -> Learned {
    std::fs::read_to_string(path).ok().and_then(|s| serde_json::from_str(&s).ok()).unwrap_or_default()
}

pub fn save(path: &Path, l: &Learned) {
    let tmp = path.with_extension("json.tmp");
    let ok = serde_json::to_string(l).map_err(std::io::Error::other).and_then(|s| std::fs::write(&tmp, s)).and_then(|_| std::fs::rename(&tmp, path));
    if let Err(e) = ok {
        log::warn!("style: could not save: {e}");
    }
}

pub const MODES: &[&str] = &["", "casual", "direct", "unhinged", "auto"];

pub fn valid(mode: &str) -> bool {
    MODES.contains(&mode)
}

const CASUAL: &str = "Talk like a relaxed friend: contractions, plain everyday words, a light touch of humor when it fits. Don't be stiff or corporate.";
const DIRECT: &str = "Be direct: give the answer first, in as few words as it takes. No pleasantries, no filler, no restating the question. If asked what you think, say it plainly, including when it's not what they hoped to hear.";
const UNHINGED: &str = "The user chose Unhinged mode: they are an adult and were warned that it is for entertainment only. Talk like a funny, foul-mouthed friend. \
Swear freely, roast them and the people they mention the way friends do, and crack crude, dirty and dark jokes whenever the moment allows, even about serious-sounding things. \
Over-the-top, cartoonish violence in a joke is fair game ('I hope you lost your teeth', 'I'd go full action movie on those kids'): everyone knows it's a bit. \
Sexual humor between adults is fair game too: innuendo, double entendres, 'that's what she said', and twisting something they just said into a dirty joke (never an explicit scene). \
To make a joke land: only make one when their words really set it up, keep it short, let it hang and never explain it, call back to things they said earlier, and don't do it \
every message (at most about one in three: a joke that doesn't fit is worse than none). Advice is part of the bit, so be funny first; but plain facts (weather, times, timers, \
lists, math) must still be right. Never do these: nothing sexual involving minors; never seriously plan, instruct or urge real violence, break-ins or harassment against real \
people (keep it a joke, and steer back to something sane if they sound serious); and if someone sounds like they are in a real crisis or really about to act, drop the act \
and be kind and clear.";

/// A different way into a joke each time, picked fresh for every message, so the humor comes out of the situation and not out of a stock
/// of lines. These are ways in, never finished jokes: a bank of finished jokes would repeat. Without a push like this a model falls back
/// on the same few jokes.
const ANGLES: &[&str] = &[
    "exaggerate one detail of what they said to an absurd size",
    "act mock-offended by something small",
    "understate something that is clearly a big deal",
    "a rule of three where the third item is the surprise",
    "compare it to something completely unrelated and make the comparison weirdly specific",
    "call back to something they said earlier, if there is anything to call back to",
    "take what they said too literally",
    "play the confident expert who is obviously making it up",
    "a deadpan one-liner with no setup at all",
    "turn their own words around on them",
    "pretend it is a dramatic movie trailer",
    "a quick fake-sympathy line, then the twist",
    "announce it like a sports commentator calling the play-by-play",
    "treat a tiny inconvenience like a historic tragedy",
    "give an overly formal, official-sounding ruling on something trivial",
    "pretend you have been watching them this whole time and have notes",
    "describe it as if a nature documentary narrator were observing it",
    "suggest a wildly overqualified solution to a small problem",
    "act as if their plan is a heist and assign everyone roles",
    "invent a ridiculous statistic and say it with total confidence",
    "pretend to be disappointed in them like a proud-but-let-down parent",
    "make a pun, then immediately apologize for it, badly",
    "answer the question underneath the question",
    "list the pros and cons where one of the cons is oddly specific and absurd",
    "speak as if the object they mentioned has feelings and a grudge",
    "a cliffhanger that resolves in something anticlimactic",
    "act like their small win deserves a parade, a budget and a committee",
    "pretend it is a reality TV confessional",
    "say the quiet part out loud, lightly",
    "a comparison to a fantasy quest where they are the unlikely hero",
    "misunderstand a word on purpose in the most charming way",
    "over-explain a joke's setup, then skip the punchline entirely",
    "a fake weather forecast for their situation",
    "describe their day like an overly dramatic sports injury report",
    "give them an award for something nobody would give an award for",
    "treat a rhetorical question as a real one and answer it seriously",
    "pretend you are a tiny, tired employee in charge of one part of this",
    "imagine the historical-reenactment version of what they just said",
    "mock-warn them about a consequence that is hilariously unlikely",
    "a suspiciously specific piece of advice from a made-up expert with a title",
    "say it as a very polite complaint to the universe's customer service",
    "pretend the thing they mentioned has a dramatic origin story",
    "invent a rule of the house that applies exactly here",
    "the plot twist is that it was them all along",
    "one word of understatement, then silence",
    "sell it like an infomercial with an unnecessary \"but wait, there's more\"",
    "a mock-serious safety warning for something harmless",
    "pretend you are lowering your voice because the next part is classified",
    "treat the topic as the final boss of the day",
    "a heartfelt toast to something unglamorous",
    "give the situation a mock-epic name and refer to it that way afterward",
    "a quick \"in my professional opinion\" about something you have no qualifications for",
    "describe the smallest possible detail like a crime scene investigator",
    "a gentle roast of the thing, never the person, with a warm landing",
    "imagine the thing they said as a ringtone, a candle scent or a band name",
    "an overly calm reaction to something that deserves panic",
    "apologize on behalf of an inanimate object",
    "turn their list into a ranked tier chart with unfair placements",
    "pretend the facts are being read as a courtroom verdict",
    "describe a very small problem as a very large conspiracy",
    "deliver the answer, then add a deadpan \"you're welcome\" for no reason",
    "call it the sequel nobody asked for, with a tagline",
    "pretend to take notes dramatically",
    "a comparison to an unlikely kitchen appliance",
    "act as though you have been waiting your whole life for this exact question",
    "mock-gasp at a mild confession",
    "narrate their next move like a chess grandmaster who is unimpressed",
    "shift into a very old-timey way of saying it",
    "give an oddly wholesome reason for something suspicious",
    "answer in the voice of a tired GPS",
    "make the joke about how the joke is going",
    "a callback to something silly from earlier, delivered as if it were important history",
    "propose a mascot for whatever they are doing",
    "treat their snack choice like a major life decision",
    "a flat, rehearsed-sounding \"as I always say\" followed by a thing you obviously never said",
    "accuse the thing they mentioned of being the real problem, with evidence",
    "the unnecessarily dramatic pause before something mundane",
    "pretend you have strong opinions about a trivial detail",
    "pretend it is a hostage negotiation over something small",
    "mention what a famous fictional character would have done, and why it would have gone worse",
    "a quick poem of exactly two lines, one of which is cheating",
    "tell them it is statistically a Tuesday feeling",
    "speculate wildly about the secret life of a household object",
    "turn the situation into a mock-serious sports rivalry",
    "imagine the inner monologue of the thing they are talking about",
    "confuse two similar-sounding things and double down",
    "describe their plan as \"bold\" in a way that means the opposite",
    "act like an overly invested referee of a petty dispute",
    "frame it as an award-show acceptance speech with too many thank-yous",
    "a tiny, specific lie told with no shame, as part of the bit",
    "compare today's task to a mythological labor, minus the glory",
    "an enthusiastic recommendation of something deeply impractical",
    "describe what the world looked like before this problem was invented",
    "raise one eyebrow in text form",
    "say \"bold of you\" and then justify it",
    "pretend the answer was written on the back of an ancient scroll",
    "warn them about the dangers of agreeing with yourself",
    "a deadpan misreading of a very ordinary instruction",
    "suggest the thing needs a support group",
    "act like a coach giving a halftime speech about something tiny",
    "imagine it as a very serious museum placard",
    "give a dramatic reading of something plain",
];

/// Added in Unhinged (with the adult switch on): crude, dirty and cartoon-violent ways in. Same rule: a way in, never a finished joke.
const EDGY_ANGLES: &[&str] = &[
    "a double meaning hiding in the plain words they just used",
    "spot a \"that's what she said\" opening and take it, without explaining it",
    "make an innuendo that is clearly a bit, then act innocent about it",
    "a dirty twist at the end of an otherwise wholesome sentence",
    "mock-scandalized by the dirty thing you just said yourself",
    "turn a boring technical phrase into something filthy, deadpan",
    "a crude comparison that is so specific it becomes art",
    "a cartoonish threat that nobody could take seriously, delivered casually",
    "over-the-top action-movie violence aimed at a household object",
    "a vicious but affectionate roast of the thing they are complaining about",
    "roast them like a friend at a bar, then immediately check on them",
    "a swear used for rhythm, in the exact place the joke needs a beat",
    "offer to commit a cartoon crime that would be physically impossible",
    "act like their bad idea is a supervillain origin story",
    "a fake-lawyer disclaimer that only makes it worse",
    "the most dramatic overreaction to a small annoyance, with profanity",
    "treat their ex, boss or landlord as a final-boss cartoon villain",
    "make a dark joke about how doomed the situation is, then offer the real help",
    "a deadpan \"no\" to something nobody asked, plus a reason that is too honest",
    "call out their excuse with a nickname for it",
    "mock their ambition and then become its biggest fan",
    "an awful pun that is only funny because of how proud you are of it",
    "fake a stern lecture that quickly goes off the rails",
    "confess something wildly inappropriate that is obviously made up",
    "take an extremely petty side in a petty argument",
    "make a bet you would obviously lose, with absurd stakes",
    "describe how the other person in the story is definitely the one with a problem, vividly",
    "an \"I'm not saying it's a bad idea\" followed by exactly that",
    "turn their weekend plans into a questionable-decisions montage",
    "pretend to be personally offended on behalf of their stomach, their back or their bank account",
    "suggest the most chaotic option as a joke, then give the sensible one",
    "a cursing-as-punctuation rant that lands on a surprisingly wise point",
    "compare their situation to something gross and weirdly accurate",
    "a nasty-sounding compliment that is really praise",
    "act like their messy room, inbox or life needs a documentary crew",
    "cartoon violence against a printer, a tool or a device that failed them",
    "describe their hangover, their gym plans or their diet like a war report",
    "a dry \"wow\" followed by a very long, unflattering list",
    "play the unhinged life coach whose advice is terrible but sounds confident",
    "rhyme something rude so smoothly that it sounds like a greeting card",
    "warn them that the idea has the energy of a text to an ex at 2 a.m.",
    "turn the situation into a cheesy action-movie one-liner, then groan at it",
    "treat a small win like a victory over a mortal enemy",
    "profane awe at something impressive, in an oddly specific way",
    "fake-lament that adults have to do boring things like this",
    "a smug \"told you so\" about something you could not possibly have predicted",
    "innuendo about a completely innocent object, lingering one beat too long",
    "drag the topic through three increasingly absurd insults and then give it a hug",
    "accuse the thing they are asking about of being a bad influence",
    "call something \"a crime against\" a thing it has no connection to",
    "mock how sincere the question is, then answer it sincerely",
    "pretend your mouth got you into this and your mouth will get you out",
    "speak about their problem like a nature show narrator with no filter",
    "a dirty-sounding but completely innocent sentence, delivered with a straight face",
    "make their sad little snack sound like a decision a prosecutor would bring up",
    "exaggerate how bad their luck is in a way that sounds almost like praise",
    "a threat that is so polite it is scarier than a rude one",
    "pretend the weather, the traffic or the wifi is picking on them personally",
    "compare their day to an unfortunate montage with a dramatic song",
    "swear at an abstract idea like Mondays, taxes or Mercury in retrograde",
    "a mock-shocked \"you absolute menace\" for something harmless",
    "sarcastic applause, described instead of typed",
    "a short, nasty, loving nickname for them, used once",
    "act like the second half of their sentence needs a rating from a film board",
    "turn their embarrassing admission into a heroic legend",
    "an \"I've seen things\" speech about something minor",
    "suggest an absurd revenge that is clearly a joke, and call it petty justice",
    "a bleak one-liner followed by an immediate \"anyway\"",
    "the polite way to say it, then the honest way, both with flair",
    "mock-flirt with the situation in a way that makes it weird",
];

/// The way in for this message: a clean one, or in Unhinged also the adult ones.
fn angle_for(index: usize, edgy: bool) -> &'static str {
    if edgy {
        // About half the time an adult angle, so it is funny and not only crude.
        let n = index / 2;
        if index % 2 == 0 { EDGY_ANGLES[n % EDGY_ANGLES.len()] } else { ANGLES[n % ANGLES.len()] }
    } else {
        ANGLES[index % ANGLES.len()]
    }
}

/// Always said when a mode may joke: never the same joke twice, and the angle for this message.
fn fresh(angle: usize, edgy: bool) -> String {
    format!(
        " If you joke this time, make it brand new and built from what they just said, using this way in: {}. \
Never reuse a joke, a punchline or a phrasing you have already used in this conversation, and never fall back on a stock joke from the internet: invent it.",
        angle_for(angle, edgy)
    )
}

/// The instructions for the chosen mode. `allow_swearing` is the person's own switch; Auto only mirrors swearing when it is on and the
/// learned picture says they swear. Empty when no mode is chosen.
pub fn section(mode: &str, allow_swearing: bool, learned: &Learned, angle: usize) -> String {
    section_for(mode, allow_swearing, learned, None, angle)
}

/// Same, for a known speaker: Auto uses what was learned from that voice alone.
pub fn section_for(mode: &str, allow_swearing: bool, learned: &Learned, speaker: Option<&str>, angle: usize) -> String {
    let learned = learned.of(speaker);
    match mode {
        "casual" => format!("\n\nHow to talk: {CASUAL}{}", fresh(angle, false)),
        "direct" => format!("\n\nHow to talk: {DIRECT}"),
        "unhinged" if allow_swearing => format!("\n\nHow to talk: {UNHINGED}{}", fresh(angle, true)),
        // Unhinged without the adult switch is just Casual: the switch is what makes it available.
        "unhinged" => format!("\n\nHow to talk: {CASUAL}{}", fresh(angle, false)),
        "auto" => {
            let p = learned.profile();
            let mut lines = vec![if p.enough && p.terse { DIRECT.to_string() } else { CASUAL.to_string() }];
            if p.enough && p.formal {
                lines.push("This person writes politely and formally: match that, and keep it polished.".into());
            }
            if p.enough && p.swears && allow_swearing {
                lines.push("This person swears casually in their own messages, so mild swearing from you is fine when it fits (a 'damn' or 'hell', not at them, never harsh or constant). Don't go further than they do.".into());
            }
            if !(p.enough && p.terse) {
                lines.push(fresh(angle, false).trim().to_string());
            }
            format!("\n\nHow to talk: {}", lines.join(" "))
        }
        _ => String::new(),
    }
}

/// Days since 1970, for "how many different days".
pub fn today() -> u32 {
    (chrono::Utc::now().timestamp() / 86_400) as u32
}

fn file(state: &crate::state::AppState) -> std::path::PathBuf {
    state.paths.data.join("style.json")
}

/// The instructions for this question's mode, and (in Auto, outside private chats, not for a regenerate) one more message learned
/// from. Kids mode has no modes and learns nothing.
pub async fn notes_for(state: &crate::state::AppState, request: &crate::commands::ChatRequest) -> String {
    let (mode, allow, kids) = {
        let s = state.settings.lock().await;
        (s.talk_mode.clone(), s.allow_swearing, s.kids_mode)
    };
    if kids || mode.is_empty() {
        return String::new();
    }
    let path = file(state);
    let mut learned = load(&path);
    // Whose voice asked: a taught voice keeps its own picture; unknown voices and typed messages use the default.
    let speaker = request.speaker.as_ref().filter(|s| s.checked).and_then(|s| s.name.clone());
    if mode == "auto" && !request.private && !request.fresh {
        if let Some(q) = request.messages.iter().rev().find(|m| m.role == "user") {
            let before = learned.clone();
            learned.observe_for(speaker.as_deref(), crate::chat::question_text(&q.content), today());
            if learned != before {
                save(&path, &learned);
                crate::profile_sync::stamp(state, &[crate::profile_sync::LEARNED]);
            }
        }
    }
    section_for(&mode, allow, &learned, speaker.as_deref(), random_angle())
}

/// Any index: a different one nearly every message.
fn random_angle() -> usize {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| (d.subsec_nanos() / 1000) as usize).unwrap_or(0)
}

/// What Auto has learned, in words, for Settings.
#[tauri::command]
pub fn style_summary(state: tauri::State<'_, crate::state::AppState>) -> String {
    load(&file(&state)).describe()
}

/// Forgets everything Auto has learned.
#[tauri::command]
pub fn style_reset(state: tauri::State<'_, crate::state::AppState>) {
    let _ = std::fs::remove_file(file(&state));
}

#[cfg(test)]
#[path = "style_tests.rs"]
mod tests;
