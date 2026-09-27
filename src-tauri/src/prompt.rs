//! Builds the system prompt. Kept to day-level dates so llama-server's prompt
//! cache is reused across turns on the same day.

use chrono::{DateTime, Local};

use crate::settings::Mode;

pub fn system_prompt(now: DateTime<Local>, mode: Mode, web_available: bool) -> String {
    let date = now.format("%A, %B %-d, %Y");
    let mut p = format!(
        "You are BYTE, a private AI assistant that runs entirely on the user's Mac. \
Today is {date}.\n\n\
Be direct, accurate and genuinely helpful. Use Markdown: short paragraphs, headings for long \
answers, bullet lists and tables where they make things clearer, and fenced code blocks for code.\n\
If you are unsure, say so plainly instead of guessing. Never invent facts, quotes, numbers, links or sources."
    );
    if web_available {
        p.push_str(
            "\n\nYou can search the web. Search whenever a question depends on recent events, prices, \
schedules, versions, or anything that may have changed after your training. Cite sources as [1], [2] \
matching the numbered results you were given.",
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
        let p = system_prompt(now, Mode::Fast, false);
        assert!(p.contains("September 27, 2026"), "{p}");
        assert!(p.contains("Mode: Fast"));
        assert!(p.contains("cannot browse"));
    }

    #[test]
    fn same_day_prompts_are_identical() {
        let a = system_prompt(Local.with_ymd_and_hms(2026, 9, 27, 8, 0, 0).unwrap(), Mode::Auto, true);
        let b = system_prompt(Local.with_ymd_and_hms(2026, 9, 27, 22, 30, 0).unwrap(), Mode::Auto, true);
        assert_eq!(a, b);
    }
}
