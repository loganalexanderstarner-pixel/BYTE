//! BYTE's help articles (src/help/*.md, the same ones the help center shows),
//! offered to the model when the user asks how to do something in BYTE, so even
//! small models answer correctly about BYTE's own features.

const ARTICLES: &[&str] = &[
    include_str!("../../src/help/01-getting-started.md"),
    include_str!("../../src/help/02-models.md"),
    include_str!("../../src/help/03-modes-and-web.md"),
    include_str!("../../src/help/04-files.md"),
    include_str!("../../src/help/05-documents.md"),
    include_str!("../../src/help/06-voice.md"),
    include_str!("../../src/help/07-mac-control.md"),
    include_str!("../../src/help/08-everyday.md"),
    include_str!("../../src/help/09-notes.md"),
    include_str!("../../src/help/10-cloud.md"),
    include_str!("../../src/help/11-privacy.md"),
    include_str!("../../src/help/12-personality.md"),
    include_str!("../../src/help/13-shortcuts.md"),
    include_str!("../../src/help/14-troubleshooting.md"),
];

/// A question about using BYTE itself ("how do I change BYTE's voice?", "can you read my files?").
pub fn wants_app_help(q: &str) -> bool {
    let l = q.to_lowercase();
    let about_byte = l.contains("byte") || l.contains(" you ") || l.starts_with("can you") || l.starts_with("do you");
    let how = ["how do i", "how can i", "how to ", "where is", "where do i", "where can i", "can you", "do you", "is there a way", "what can you do", "how does", "turn on", "turn off", "set up", "change "]
        .iter()
        .any(|w| l.contains(w));
    let feature = ["setting", "voice", "model", "notes", "clip", "mind map", "board", "shortcut", "hey byte", "dark", "theme", "memory", "private", "cloud", "download", "knowledge base", "my files", "document", "deck", "personality", "permission", "open anyway", "automation", "reminder", "help", "dictate", "microphone", "quick ask", "palette"]
        .iter()
        .any(|w| l.contains(w));
    about_byte && how && feature
}

fn words(s: &str) -> Vec<String> {
    s.to_lowercase().split(|c: char| !c.is_alphanumeric()).filter(|w| w.len() > 3).map(str::to_string).collect()
}

/// The (at most `n`) articles that best match the question, by shared words (title words count double).
pub fn best(q: &str, n: usize) -> Vec<&'static str> {
    // Words in most articles ("BYTE", "open") say nothing about which one fits.
    let common = |w: &String| ARTICLES.iter().filter(|a| words(a).contains(w)).count() * 2 > ARTICLES.len();
    let qw: Vec<String> = words(q).into_iter().filter(|w| !common(w)).collect();
    let mut scored: Vec<(usize, &str)> = ARTICLES
        .iter()
        .map(|a| {
            let title = words(a.lines().next().unwrap_or(""));
            let body = words(a);
            let s = qw.iter().map(|w| body.iter().filter(|b| *b == w).count() + if title.contains(w) { 3 } else { 0 }).sum();
            (s, *a)
        })
        .filter(|(s, _)| *s > 0)
        .collect();
    scored.sort_by(|a, b| b.0.cmp(&a.0));
    scored.into_iter().take(n).map(|(_, a)| a).collect()
}

/// The system prompt addition for an app-help question ("" when it isn't one).
pub fn section(q: &str) -> String {
    if !wants_app_help(q) {
        return String::new();
    }
    let found = best(q, 2);
    if found.is_empty() {
        return String::new();
    }
    let links = |a: &str| a.replace("](byte-setting:", "](#").replace("](help:", "](#");
    format!(
        "\n\nThe user is asking how to use BYTE (you). Answer from BYTE's help below; don't invent buttons or settings that aren't in it. \
If it doesn't cover the question, say so and suggest the help center (⌘?).\n\n<<<\n{}\n>>>",
        found.iter().map(|a| links(a)).collect::<Vec<_>>().join("\n\n---\n\n")
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn knows_app_questions() {
        for yes in ["How do I change BYTE's voice?", "can you read my files?", "Where is the setting for dark mode in BYTE?", "how do I turn on Hey BYTE"] {
            assert!(wants_app_help(yes), "{yes}");
        }
        for no in ["How do I make banana bread?", "What's the weather?", "can you write a poem about the sea", "How does a mortgage work?"] {
            assert!(!wants_app_help(no), "{no}");
        }
    }

    #[test]
    fn finds_the_right_article() {
        assert!(best("how do I change BYTE's voice", 1)[0].starts_with("# Voice"));
        assert!(best("macOS says BYTE can't be opened, open anyway", 1)[0].starts_with("# Troubleshooting"));
        let s = section("How do I turn on Hey BYTE?");
        assert!(s.contains("Hey BYTE") && !s.contains("byte-setting:"), "{s}");
        assert_eq!(section("How do I make pancakes?"), "");
    }
}
