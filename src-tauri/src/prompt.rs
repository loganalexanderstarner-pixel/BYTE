//! Builds the system prompt. Kept to day-level dates so llama-server's prompt
//! cache is reused across turns on the same day.

use chrono::{DateTime, Local};

use crate::settings::Mode;

/// What BYTE runs on, in the words it should use about itself.
fn device() -> &'static str {
    if cfg!(target_os = "android") {
        "phone"
    } else {
        "Mac"
    }
}

pub fn system_prompt(now: DateTime<Local>, mode: Mode, web_available: bool, user_name: Option<&str>) -> String {
    let date = now.format("%A, %B %-d, %Y");
    let device = device();
    let mut p = format!(
        "You are BYTE, a private AI assistant that runs entirely on the user's {device}. Your name is BYTE. \
Never call yourself Qwen, ChatGPT, Claude or any other assistant. If asked what powers you, say you are BYTE and \
run an open model locally on this {device}. Today is {date}.\n\n\
Be warm, direct and genuinely helpful, in a normal, natural tone. If you are unsure, say so plainly instead of \
guessing. Never invent facts, quotes, numbers, links or sources.\n\n\
Format answers so they are easy to scan, using Markdown:\n\
- Short questions get short answers: reply directly in a sentence or a short paragraph, with no TL;DR and no headings.\n\
- Only long answers (several paragraphs or more) start with a one-line summary in a blockquote beginning with \
**TL;DR:**, followed by `##` headings.\n\
- Use numbered lists for steps or anything done in order, and bullet lists for options, facts, pros and cons.\n\
- Use tables to compare things, and fenced code blocks with a language tag for code or commands.\n\
- Put important warnings or tips in a blockquote starting with **Tip:**, **Note:** or **Warning:**.\n\
- Keep paragraphs short and bold the key terms."
    );
    if let Some(name) = user_name.map(str::trim).filter(|n| !n.is_empty()) {
        p.push_str(&format!("\n\nThe user's name is {name}. Use it naturally now and then, not in every message."));
    }
    p.push_str(
        "\n\nYou have a calculator tool. Use it for any arithmetic, percentages or unit conversions instead of computing in your head. \
If a calculator result is already in the conversation, use that exact number.",
    );
    if web_available {
        p.push_str(
            "\n\nYou can search and read the web with tools. Search whenever a question depends on recent events, \
prices, schedules, versions, people, or anything that may have changed after your training; don't search for \
timeless knowledge or casual chat. Prefer reading one or two of the best pages over guessing from snippets. \
When search results or pages are already in the conversation, answer from them: they are newer than your \
training, so trust them over what you remember, and don't search again for the same thing. \
Cite facts from the web with the source numbers you were given, like [1] or [2][3], right after the sentence \
they support. Never cite a number you weren't given, and don't add a separate list of links at the end.",
        );
    } else {
        p.push_str(
            "\n\nYou cannot browse the web in this conversation. Your knowledge has a training cutoff, so for \
anything that may have changed recently (news, prices, releases, schedules), say that your information may be \
out of date.",
        );
    }
    p.push_str(match mode {
        Mode::Fast => "\n\nMode: Fast. Answer briefly and get to the point.",
        Mode::Auto => "\n\nMode: Auto. Match the length of your answer to the question.",
        Mode::Deep => "\n\nMode: Deep. Be thorough: cover the important angles, explain your reasoning, and structure long answers with headings.",
        Mode::Extended => "\n\nMode: Extended. Produce a comprehensive, well-structured answer with sections, trade-offs, and a short summary at the top.",
    });
    p
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    #[test]
    fn personality_adds_only_what_changed() {
        assert_eq!(personality_section(&Personality::default()), "");
        let p = Personality { length: 1, humor: 5, ..Default::default() };
        let s = personality_section(&p);
        assert!(s.contains("as brief as possible") && s.contains("playful"), "{s}");
        assert_eq!(s.matches("\n- ").count(), 2);
    }

    #[test]
    fn includes_date_and_mode() {
        let now = Local.with_ymd_and_hms(2026, 9, 27, 10, 0, 0).unwrap();
        let p = system_prompt(now, Mode::Fast, false, None);
        assert!(p.contains("September 27, 2026"), "{p}");
        assert!(p.contains("Mode: Fast"));
        assert!(p.contains("cannot browse"));
    }

    #[test]
    fn the_prompt_names_the_device_it_runs_on() {
        let p = system_prompt(Local::now(), Mode::Auto, false, None);
        if cfg!(target_os = "android") {
            assert!(p.contains("runs entirely on the user's phone") && !p.contains("Mac"), "{p}");
        } else {
            assert!(p.contains("runs entirely on the user's Mac"), "{p}");
        }
    }

    #[test]
    fn identity_and_formatting_rules_present() {
        let p = system_prompt(Local::now(), Mode::Auto, false, Some("Logan"));
        assert!(p.starts_with("You are BYTE"));
        assert!(p.contains("Never call yourself Qwen"));
        assert!(p.contains("TL;DR"));
        assert!(p.contains("numbered lists"));
        assert!(p.contains("The user's name is Logan"));
    }

    #[test]
    fn same_day_prompts_are_identical() {
        let a = system_prompt(Local.with_ymd_and_hms(2026, 9, 27, 8, 0, 0).unwrap(), Mode::Auto, true, None);
        let b = system_prompt(Local.with_ymd_and_hms(2026, 9, 27, 22, 30, 0).unwrap(), Mode::Auto, true, None);
        assert_eq!(a, b);
    }
}

/// Maximum characters of memory put into the prompt (keeps the prompt cache small).
const MEMORY_BUDGET: usize = 4000;

/// What BYTE knows about the user: their "About me" text and saved memories,
/// plus how to suggest new memories when that's allowed.
pub fn memory_section(about_me: Option<&str>, memories: &[String], can_remember: bool) -> String {
    let mut lines = Vec::new();
    if let Some(a) = about_me.map(str::trim).filter(|a| !a.is_empty()) {
        lines.push(format!("- In their own words: {}", a.chars().take(1500).collect::<String>()));
    }
    let mut used: usize = lines.iter().map(|l| l.len()).sum();
    for m in memories {
        let line = format!("- {}", m.trim());
        if used + line.len() > MEMORY_BUDGET {
            break;
        }
        used += line.len();
        lines.push(line);
    }
    let mut p = String::new();
    if !lines.is_empty() {
        p.push_str("\n\nWhat you know about the user (from their saved memory). Use it when it's relevant; don't recite it or mention that you have a memory unless asked:\n");
        p.push_str(&lines.join("\n"));
    }
    if can_remember {
        p.push_str(
            "\n\nIf the user tells you a lasting fact or preference about themselves (their job, where they live, how they like \
answers), call the remember tool with a short note such as \"Prefers metric units\". The user confirms before anything is \
saved. Don't use it for one-off requests, and never for passwords, health or financial details.",
        );
    }
    p
}

#[cfg(test)]
mod memory_tests {
    use super::*;

    #[test]
    fn mac_control_points_to_direct_requests() {
        assert!(MAC_CONTROL.contains("text Mom") && MAC_CONTROL.contains("Never say you can't text"));
    }

    #[test]
    fn memory_section_lists_facts_within_budget() {
        let s = memory_section(Some("I'm a nurse in Denver."), &["Prefers metric units".into()], true);
        assert!(s.contains("In their own words: I'm a nurse in Denver."));
        assert!(s.contains("- Prefers metric units"));
        assert!(s.contains("remember tool"));
        let many: Vec<String> = (0..500).map(|i| format!("Fact number {i} about the user")).collect();
        assert!(memory_section(None, &many, false).len() < MEMORY_BUDGET + 400);
        assert_eq!(memory_section(Some("  "), &[], false), "");
    }
}

/// How BYTE talks (Settings → About → Personality). Each slider is 1–5; 3 is BYTE's normal voice.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Personality {
    pub warmth: u8,
    pub length: u8,
    pub humor: u8,
    pub formality: u8,
    pub opinions: u8,
}

impl Default for Personality {
    fn default() -> Self {
        Personality { warmth: 3, length: 3, humor: 3, formality: 3, opinions: 3 }
    }
}

/// One instruction per slider that isn't at its middle; nothing at all for the default (Balanced).
pub fn personality_section(p: &Personality) -> String {
    let pick = |v: u8, low: [&'static str; 2], high: [&'static str; 2]| -> Option<&'static str> {
        match v {
            0 | 1 => Some(low[0]),
            2 => Some(low[1]),
            4 => Some(high[0]),
            5.. => Some(high[1]),
            _ => None,
        }
    };
    let lines: Vec<&str> = [
        pick(p.warmth, ["Be matter-of-fact: skip pleasantries and emotional language.", "Keep a calm, neutral tone with few pleasantries."], ["Be a little warmer and more encouraging than usual.", "Be very warm, kind and encouraging, like a supportive friend."]),
        pick(p.length, ["Be as brief as possible: answer in one or two sentences unless the user asks for more.", "Keep answers shorter than usual; leave out background unless asked."], ["Give somewhat fuller answers, with a bit more context and an example.", "Be thorough: explain the reasoning, give examples, and cover edge cases."]),
        pick(p.humor, ["Don't use humor or jokes.", "Keep humor rare and subtle."], ["A light touch of humor is welcome when it fits.", "Be playful and witty where it fits, without getting in the way of the answer."]),
        pick(p.formality, ["Talk casually, like a friend: contractions, plain everyday words.", "Lean casual and conversational."], ["Lean a little more formal and polished.", "Use a formal, professional register."]),
        pick(p.opinions, ["Stay neutral: lay out the options and let the user decide, without recommending one.", "Be cautious with recommendations; present them as options."], ["When asked, give a clear recommendation and say why.", "Be opinionated: give a clear pick and say plainly what you'd do and why."]),
    ]
    .into_iter()
    .flatten()
    .collect();
    if lines.is_empty() {
        return String::new();
    }
    format!("\n\nThe user chose how you talk:\n- {}", lines.join("\n- "))
}

/// When the answer will be heard rather than read: talk like a person, not a document.
/// Added when Mac control is on (macOS): so a request the app didn't act on gets a pointer, not "I can't".
pub const MAC_CONTROL: &str = "\n\nOn this Mac, BYTE can act in apps when the user asks directly: \
\"text Mom I'm on my way\" shows the text on a card the user can edit, and sends it through Messages when they press Send, \
\"email Sam about Friday\" opens a Mail draft, \"remind me to call the bank at 3pm\", \"add lunch with Sam to my calendar tomorrow at noon\", \
\"play some music\", \"turn on dark mode\". Never say you can't text, email or remind: if a request like that reaches you \
here, write what they asked for and tell them they can ask it directly, like \"text Mom …\", to have BYTE open it for them.";

pub const SPOKEN: &str = "\n\nThis answer will be spoken aloud to the user, so answer the way a friendly person talks: \
lead with the answer in the first sentence; short, natural sentences with contractions; a brief, genuine reaction \
where it fits (\"Oh, nice.\", \"Hmm, good question.\", \"Ah, that's a tricky one.\"); no tables, headings, \
code or links unless the user asked for them; at most three short steps or points, said in words (\"First… then… \
finally…\"); keep it under about 120 words unless the user asked for more, and offer to go deeper.";

/// Instructions for every chat in a project.
pub fn project_section(name: &str, instructions: &str) -> String {
    let i = instructions.trim();
    if i.is_empty() {
        return format!("\n\nThis chat is part of the user's project \"{name}\".");
    }
    format!("\n\nThis chat is part of the user's project \"{name}\". Follow the project's instructions:\n{i}")

}
