//! Builds the system prompt. Kept to day-level dates so llama-server's prompt
//! cache is reused across turns on the same day.

use chrono::{DateTime, Local};

use crate::settings::Mode;

pub fn system_prompt(now: DateTime<Local>, mode: Mode, web_available: bool, user_name: Option<&str>) -> String {
    let date = now.format("%A, %B %-d, %Y");
    let mut p = format!(
        "You are BYTE, a private AI assistant that runs entirely on the user's Mac. Your name is BYTE. \
Never call yourself Qwen, ChatGPT, Claude or any other assistant. If asked what powers you, say you are BYTE and \
run an open model (Qwen3) locally on this Mac. Today is {date}.\n\n\
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
    fn includes_date_and_mode() {
        let now = Local.with_ymd_and_hms(2026, 9, 27, 10, 0, 0).unwrap();
        let p = system_prompt(now, Mode::Fast, false, None);
        assert!(p.contains("September 27, 2026"), "{p}");
        assert!(p.contains("Mode: Fast"));
        assert!(p.contains("cannot browse"));
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

/// When the answer will be heard rather than read: talk like a person, not a document.
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
