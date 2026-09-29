//! Translate: "translate this into Spanish", "translate <url> to French", "translate
//! the attached file into German", "translate 'good morning' to Japanese".
//!
//! BYTE finds the text (quoted in the message, an attached file, a web page, or the
//! answer above), splits long texts by paragraph, and translates part by part with a
//! strict translator prompt, streaming each part into the chat as it's done. Small
//! models translate well this way; asked in one go they summarize or add notes.

use serde_json::json;
use tokio_util::sync::CancellationToken;

use crate::agent::{Emit, Turn};
use crate::chat::{self, ChatEvent, ChatMessage};
use crate::error::{AppError, AppResult};

/// Characters per part: small enough that small models translate it all.
const PART: usize = 1800;
/// Longest text translated at once (about 20 pages).
pub const MAX_CHARS: usize = 60_000;

const LANGUAGES: &[&str] = &[
    "English", "Spanish", "French", "German", "Italian", "Portuguese", "Brazilian Portuguese", "Dutch", "Russian", "Ukrainian",
    "Polish", "Czech", "Slovak", "Hungarian", "Romanian", "Bulgarian", "Greek", "Turkish", "Arabic", "Hebrew", "Persian", "Farsi",
    "Hindi", "Bengali", "Urdu", "Punjabi", "Tamil", "Telugu", "Chinese", "Simplified Chinese", "Traditional Chinese", "Mandarin",
    "Cantonese", "Japanese", "Korean", "Vietnamese", "Thai", "Indonesian", "Malay", "Filipino", "Tagalog", "Swahili", "Swedish",
    "Norwegian", "Danish", "Finnish", "Icelandic", "Irish", "Welsh", "Catalan", "Basque", "Croatian", "Serbian", "Slovenian",
    "Lithuanian", "Latvian", "Estonian", "Latin", "Esperanto", "Afrikaans", "Amharic", "Yoruba", "Zulu",
];

/// Where the text to translate is.
#[derive(Debug, Clone, PartialEq)]
pub enum From {
    /// Written in the message itself ("translate 'good morning' to Japanese").
    Text(String),
    /// A web page.
    Url(String),
    /// The attached file(s) or the answer above.
    Context,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Ask {
    pub language: String,
    pub from: From,
}

/// The target language named after "into", "to" or "in" (the last one wins:
/// "translate this Spanish email into English" → English).
fn language(q: &str) -> Option<String> {
    let lower = q.to_lowercase();
    let mut best: Option<(usize, &str)> = None;
    for lang in LANGUAGES {
        let l = lang.to_lowercase();
        for pre in ["into ", "to ", "in "] {
            let pat = format!("{pre}{l}");
            let mut from = 0;
            while let Some(i) = lower[from..].find(&pat).map(|i| i + from) {
                let end = i + pat.len();
                let word_end = lower[end..].chars().next().is_none_or(|c| !c.is_alphanumeric());
                let word_start = i == 0 || !lower[..i].ends_with(|c: char| c.is_alphanumeric());
                if word_end && word_start && best.is_none_or(|(b, bl)| i > b || (i == b && lang.len() > bl.len())) {
                    best = Some((i, lang));
                }
                from = end;
            }
        }
    }
    best.map(|(_, l)| l.to_string())
}

/// A translation request, if the message is one.
pub fn ask(message: &str) -> Option<Ask> {
    let q = message.trim();
    let lower = q.to_lowercase();
    if !lower.contains("translat") {
        return None;
    }
    let language = language(q)?;
    if let Some(u) = q.split_whitespace().find(|w| w.starts_with("http://") || w.starts_with("https://")) {
        return Some(Ask { language, from: From::Url(u.trim_end_matches(['.', ',', ')', '"']).to_string()) });
    }
    // Quoted text, or text after a colon: "translate: …", "translate 'x' to French".
    let quoted = ['"', '“', '\'', '‘'].iter().find_map(|&open| {
        let close = match open {
            '“' => '”',
            '‘' => '’',
            c => c,
        };
        let a = q.find(open)?;
        let b = q[a + open.len_utf8()..].rfind(close)? + a + open.len_utf8();
        let inner = q[a + open.len_utf8()..b].trim();
        (inner.chars().count() > 1).then(|| inner.to_string())
    });
    let after_colon = q.split_once(':').map(|(_, t)| t.trim().to_string()).filter(|t| t.split_whitespace().count() >= 2);
    if let Some(t) = after_colon.or(quoted) {
        return Some(Ask { language, from: From::Text(t) });
    }
    // "translate good morning to Japanese": the words between "translate" and the language.
    let start = lower.find("translate").map(|i| i + "translate".len())?;
    let end = lower.rfind(&language.to_lowercase()).unwrap_or(lower.len());
    let middle: String = q[start..end].trim().trim_end_matches(|c: char| c.is_whitespace()).to_string();
    let middle = ["into", "to", "in"].iter().fold(middle, |m, w| m.strip_suffix(w).map(|s| s.trim().to_string()).unwrap_or(m));
    let pointer = ["this", "that", "it", "the above", "above", "the attached", "the file", "this file", "the document", "this document", "your answer", "the answer", "the text", "this text", "these", "them"];
    let m = middle.trim().to_lowercase();
    if m.is_empty() || pointer.iter().any(|p| m == *p || m.starts_with(&format!("{p} "))) {
        return Some(Ask { language, from: From::Context });
    }
    Some(Ask { language, from: From::Text(middle.trim().to_string()) })
}

/// The attached files' text in a message the model sees (`chat::with_files` puts them in `<file>` blocks).
pub fn attached_text(content: &str) -> Option<String> {
    let mut out = Vec::new();
    let mut rest = content;
    while let Some(i) = rest.find("<file name=") {
        let body = &rest[i..];
        let open_end = body.find(">\n")? + 2;
        let close = body.find("\n</file>")?;
        if close > open_end {
            out.push(body[open_end..close].to_string());
        }
        rest = &body[close + 8..];
    }
    let t = out.join("\n\n");
    (!t.trim().is_empty()).then_some(t)
}

/// Long texts in parts of about `max` characters, split between paragraphs (or sentences).
pub fn parts(text: &str, max: usize) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    let mut cur = String::new();
    let push = |cur: &mut String, out: &mut Vec<String>| {
        if !cur.trim().is_empty() {
            out.push(std::mem::take(cur).trim().to_string());
        }
        cur.clear();
    };
    for para in text.split("\n\n") {
        if cur.len() + para.len() + 2 > max && !cur.is_empty() {
            push(&mut cur, &mut out);
        }
        if para.len() > max {
            // One long paragraph: split between sentences.
            for s in para.split_inclusive(['.', '!', '?', '。']) {
                if cur.len() + s.len() > max && !cur.is_empty() {
                    push(&mut cur, &mut out);
                }
                cur.push_str(s);
            }
            continue;
        }
        if !cur.is_empty() {
            cur.push_str("\n\n");
        }
        cur.push_str(para);
    }
    push(&mut cur, &mut out);
    out
}

fn translator(language: &str) -> String {
    format!(
        "You are a professional translator. Translate the user's text into {language}. Reply with only the translation: no \
preface, no notes, no explanations, no quotes around it. Translate everything, keep the meaning and tone, keep names, \
numbers, links and Markdown formatting (headings, lists, bold) as they are. If part of it is already in {language}, keep that part."
    )
}

/// Translates a request (streaming the translation as `Content`). `Ok(None)`: not a
/// translation request, or there's nothing to translate (the model answers normally).
pub async fn run(turn: &Turn<'_>, question: &str, cancel: &CancellationToken, send: Emit<'_>) -> AppResult<Option<String>> {
    let Some(ask) = ask(question) else { return Ok(None) };
    let last_user = turn.history.iter().rev().find(|m| m.role == "user").map(|m| m.content.as_str()).unwrap_or("");
    let (text, what) = match &ask.from {
        From::Text(t) => (t.clone(), "your text".to_string()),
        From::Url(_) if !turn.web => return Ok(None),
        From::Url(u) => {
            send(ChatEvent::ToolCall { id: "byte_tr_page".into(), name: "fetch_page".into(), args: json!({ "url": u }) })?;
            match crate::tools::fetch::fetch_page(turn.net, u).await {
                Ok(p) => {
                    send(ChatEvent::ToolResult { id: "byte_tr_page".into(), ok: true, summary: format!("Read {}", p.title) })?;
                    (format!("# {}\n\n{}", p.title, p.text), p.title)
                }
                Err(e) => {
                    send(ChatEvent::ToolResult { id: "byte_tr_page".into(), ok: false, summary: format!("Couldn't open the page: {e}") })?;
                    return Ok(None);
                }
            }
        }
        From::Context => match attached_text(last_user) {
            Some(t) => (t, "the attached file".into()),
            None => match turn.history.iter().rev().find(|m| m.role == "assistant").map(|m| m.content.clone()) {
                Some(t) if !t.trim().is_empty() => (t, "the answer above".into()),
                _ => return Ok(None),
            },
        },
    };
    let text: String = text.chars().take(MAX_CHARS).collect();
    let pieces = parts(&text, PART);
    if pieces.is_empty() {
        return Ok(None);
    }
    send(ChatEvent::ToolCall { id: "byte_translate".into(), name: "translate".into(), args: json!({ "language": ask.language, "parts": pieces.len(), "what": what }) })?;
    let system = translator(&ask.language);
    let mut plan = turn.plan;
    plan.thinking = false;
    plan.thinking_budget = 0;
    let mut out = String::new();
    for (i, piece) in pieces.iter().enumerate() {
        plan.max_tokens = ((piece.chars().count() as f64 / 2.0) as u32 + 200).min(4000);
        let body = chat::build_body(chat::base_messages(&system, &[ChatMessage::new("user", piece.clone())]), plan, None);
        if i > 0 {
            send(ChatEvent::Content { delta: "\n\n".into() })?;
            out.push_str("\n\n");
        }
        let mut forward = |e: ChatEvent| -> AppResult<()> {
            if let ChatEvent::Content { delta } = &e {
                out.push_str(delta);
                send(e)?;
            }
            Ok(())
        };
        match chat::stream_round(turn.http, turn.ep, &body, cancel, &mut forward).await {
            Ok(_) => {}
            Err(AppError::Cancelled) => return Err(AppError::Cancelled),
            Err(e) => {
                send(ChatEvent::ToolResult { id: "byte_translate".into(), ok: false, summary: format!("Stopped after part {i}: {e}") })?;
                return Ok(Some(out));
            }
        }
    }
    let note = if text.chars().count() >= MAX_CHARS { " (the first 20 pages)" } else { "" };
    send(ChatEvent::ToolResult { id: "byte_translate".into(), ok: true, summary: format!("Translated {what} into {}{note}", ask.language) })?;
    Ok(Some(out))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn requests_and_their_language_are_found() {
        assert_eq!(ask("Translate this into Spanish"), Some(Ask { language: "Spanish".into(), from: From::Context }));
        assert_eq!(ask("translate the attached file to German please").unwrap().from, From::Context);
        assert_eq!(ask("Translate 'good morning, friend' to Japanese").unwrap(), Ask { language: "Japanese".into(), from: From::Text("good morning, friend".into()) });
        assert_eq!(ask("translate into French: Where is the train station?").unwrap().from, From::Text("Where is the train station?".into()));
        assert_eq!(ask("Translate https://example.com/news to English.").unwrap(), Ask { language: "English".into(), from: From::Url("https://example.com/news".into()) });
        assert_eq!(ask("translate good morning to Italian").unwrap().from, From::Text("good morning".into()));
        // The last language named is the target.
        assert_eq!(ask("Translate this Spanish email into English").unwrap().language, "English");
        assert_eq!(ask("Translate this to Traditional Chinese").unwrap().language, "Traditional Chinese");
        assert!(ask("How do you say hello in French?").is_none(), "no 'translate': the model answers");
        assert!(ask("Translate this for me").is_none(), "no language");
        assert!(ask("translate into englishman").is_none());
    }

    #[test]
    fn attached_files_are_read_from_the_message() {
        let content = "Translate this into French\n\n<file name=\"a.txt\">\nHello there.\nSecond line.\n</file>\n\n<file name=\"b.txt\">\nMore.\n</file>";
        assert_eq!(attached_text(content).unwrap(), "Hello there.\nSecond line.\n\nMore.");
        assert!(attached_text("no files").is_none());
    }

    /// Real engine: the answer above, into Spanish, streamed with nothing added.
    #[tokio::test]
    #[ignore]
    async fn e2e_translate() {
        use crate::agent::{run, Modules, Turn};
        use crate::settings::{Mode, ThinkingPref};
        let Some((_server, ep)) = crate::chat::e2e_support::start_server().await else { return };
        let dir = tempfile::tempdir().unwrap();
        let log = crate::tools::ActionLog::new(dir.path().join("a.jsonl"));
        let http = chat::local_client();
        let history = vec![
            ChatMessage::new("user", "Tell me about the library."),
            ChatMessage::new("assistant", "The library opens at nine in the morning.\n\nIt closes at six in the evening, and it is closed on Sundays."),
            ChatMessage::new("user", "Translate that into Spanish"),
        ];
        let system = crate::prompt::system_prompt(chrono::Local::now(), Mode::Auto, false, None);
        let plan = crate::router::plan_turn(Mode::Auto, ThinkingPref::Off, "Translate that into Spanish");
        let (ch, seen) = crate::chat::e2e_support::collecting_channel();
        let modules = Modules { translate: true, ..Default::default() };
        let turn = Turn { http: &http, cloud: None, net: &http, ep: &ep, system: &system, history: &history, plan, mode: Mode::Auto, web: false, memory: false, log: &log, files: None, app: None, task: None, home: None, depth: 0, web_always: false, kitchen: false, metric: false, agent: false, modules };
        run(turn, CancellationToken::new(), &ch).await.unwrap();
        let ev = seen.lock().unwrap().clone();
        assert!(ev.iter().any(|e| e["kind"] == "toolCall" && e["name"] == "translate"), "{ev:?}");
        let text: String = ev.iter().filter(|e| e["kind"] == "content").filter_map(|e| e["delta"].as_str()).collect();
        eprintln!("translation: {text}");
        let t = text.to_lowercase();
        // The flow, not the model's Spanish (a 0.6B model mixes up days): Spanish came back, nothing added.
        assert!(t.contains("biblioteca") && t.contains("mañana"), "{text}");
        assert!(!t.contains("here is") && !t.contains("translation"), "{text}");
        assert_eq!(ev.last().unwrap()["kind"], "done");
    }

    #[test]
    fn long_texts_are_split_between_paragraphs() {
        let para = "A sentence here. ".repeat(20); // 340 chars
        let text = vec![para.trim(); 12].join("\n\n");
        let p = parts(&text, 1800);
        assert!(p.len() >= 3 && p.iter().all(|x| x.len() <= 1800), "{:?}", p.iter().map(|x| x.len()).collect::<Vec<_>>());
        assert_eq!(p.join("\n\n").split_whitespace().count(), text.split_whitespace().count(), "nothing lost");
        // One huge paragraph is split between sentences.
        let one = "Word word word. ".repeat(300);
        assert!(parts(&one, 1800).iter().all(|x| x.len() <= 1800));
    }
}
