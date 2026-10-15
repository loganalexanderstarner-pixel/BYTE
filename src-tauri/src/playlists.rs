//! Spotify playlists by name: "my gaming playlist is <share link>" once, then "open Spotify to my gaming playlist", "play my
//! workout playlist", "put on my road trip playlist".
//!
//! Spotify's desktop app starts a playlist from `spotify:playlist:<id>:play`; BYTE keeps the names and links in
//! `playlists.json` (no Spotify login, nothing leaves the PC). A name BYTE does not know opens Spotify's search for those words
//! and says how to teach it. Reading the real playlists of an account would need the owner to sign in to Spotify; later.

use std::path::Path;

use serde::{Deserialize, Serialize};
use serde_json::json;
use tauri::Manager;
use tokio_util::sync::CancellationToken;

use crate::agent::{Emit, Turn};
use crate::chat::ChatEvent;
use crate::error::AppResult;
use crate::state::AppState;
use crate::tools::SourceBook;

const FILE: &str = "playlists.json";
const KINDS: &[&str] = &["playlist", "album", "artist", "track"];

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Saved {
    pub name: String,
    pub kind: String,
    pub id: String,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Ask {
    Save(Saved),
    Play { name: String },
    List,
    Forget { name: String },
}

/// `playlist` and the id from a Spotify share link or URI, wherever it is in the text.
pub fn find_link(text: &str) -> Option<(String, String)> {
    for word in text.split_whitespace() {
        let w = word.trim_matches(|c: char| matches!(c, '<' | '>' | '(' | ')' | '"' | '\'' | ',' | '.' | '!' | '?' | ';'));
        if let Some(rest) = w.strip_prefix("spotify:") {
            let parts: Vec<&str> = rest.split(':').collect();
            if let [kind, id, ..] = parts.as_slice() {
                if KINDS.contains(kind) && id_ok(id) {
                    return Some((kind.to_string(), id.to_string()));
                }
            }
        }
        if let Some(i) = w.find("open.spotify.com/") {
            let path = w[i + "open.spotify.com/".len()..].split(['?', '#']).next().unwrap_or("");
            let segs: Vec<&str> = path.split('/').filter(|s| !s.is_empty()).collect();
            for pair in segs.windows(2) {
                if KINDS.contains(&pair[0]) && id_ok(pair[1]) {
                    return Some((pair[0].to_string(), pair[1].to_string()));
                }
            }
        }
    }
    None
}

fn id_ok(id: &str) -> bool {
    (10..=40).contains(&id.len()) && id.chars().all(|c| c.is_ascii_alphanumeric())
}

/// The URI that starts it playing.
pub fn uri(s: &Saved) -> String {
    format!("spotify:{}:{}:play", s.kind, s.id)
}

pub fn search_uri(words: &str) -> String {
    let enc: String = words.bytes().map(|b| if b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.' | b'~') { (b as char).to_string() } else { format!("%{b:02X}") }).collect();
    format!("spotify:search:{enc}")
}

/// "my gaming playlist" and "gaming" are the same name.
fn norm(name: &str) -> String {
    const DROP: &[&str] = &["my", "the", "a", "an", "playlist", "playlists", "spotify", "on", "in", "please"];
    name.to_lowercase().split(|c: char| !c.is_alphanumeric() && c != '\'').filter(|w| !w.is_empty() && !DROP.contains(w)).collect::<Vec<_>>().join(" ")
}

pub fn find<'a>(list: &'a [Saved], name: &str) -> Option<&'a Saved> {
    let n = norm(name);
    if n.is_empty() {
        return None;
    }
    list.iter().find(|s| norm(&s.name) == n).or_else(|| list.iter().find(|s| norm(&s.name).contains(&n) || n.contains(&norm(&s.name))))
}

pub fn applies(q: &str) -> bool {
    ask(q).is_some()
}

pub fn ask(q: &str) -> Option<Ask> {
    let raw = crate::router::without_wake_phrase(q);
    let l = crate::macctl::clean(raw);
    if l.is_empty() || ["how do i", "how can i", "how to", "what is ", "explain", "why "].iter().any(|p| l.starts_with(p)) {
        return None;
    }
    // Spotify's ids are case-sensitive, so the link is read from the words as they were written.
    let link = find_link(raw);
    let has_word = |w: &str| l.split(|c: char| !c.is_alphanumeric()).any(|x| x == w);
    // Teaching a name: "my gaming playlist is <link>", "save <link> as my gaming playlist".
    if let Some((kind, id)) = link {
        let before_link = l.split("http").next().unwrap_or(&l).split("spotify:").next().unwrap_or(&l);
        let name = if let Some((_, after)) = l.split_once(" as ") {
            norm(after.split("http").next().unwrap_or(after))
        } else {
            norm(&before_link.replace(" is ", " ").replace(" are ", " ").replace("save", " ").replace("remember", " ").replace("add", " "))
        };
        return (!name.is_empty() && (has_word("playlist") || has_word("album") || has_word("save") || has_word("remember") || l.contains(" as ") || l.contains(" is "))).then_some(Ask::Save(Saved { name, kind, id }));
    }
    if !has_word("playlist") && !has_word("playlists") {
        return None;
    }
    if ["what playlists", "which playlists", "list my playlists", "show my playlists", "playlists do you know", "playlists have i saved", "playlists do i have"].iter().any(|p| l.contains(p)) {
        return Some(Ask::List);
    }
    if l.starts_with("forget ") || l.starts_with("delete ") || l.starts_with("remove ") {
        let name = norm(l.split_once(' ').map(|x| x.1).unwrap_or(""));
        return (!name.is_empty()).then_some(Ask::Forget { name });
    }
    if !["open", "play", "put on", "start", "queue", "launch", "listen", "load", "run", "shuffle", "turn on"].iter().any(|v| l.starts_with(v)) {
        return None;
    }
    // "my playlist road trip" (the name after the word) or "my road trip playlist" (before it).
    let (before, after) = l.split_once("playlist").map(|(b, a)| (b.to_string(), a.trim_start_matches('s').to_string())).unwrap_or_default();
    let after = after.trim();
    let name = if !after.is_empty() && !after.starts_with("on ") && !after.starts_with("in ") && !after.starts_with("please") {
        norm(after)
    } else {
        const VERBS: &[&str] = &["open", "play", "put", "start", "queue", "launch", "listen", "to", "load", "run", "shuffle", "turn", "up", "spotify", "on", "in"];
        norm(&before.split_whitespace().filter(|w| !VERBS.contains(w)).collect::<Vec<_>>().join(" "))
    };
    (!name.is_empty()).then_some(Ask::Play { name })
}

pub fn load(path: &Path) -> Vec<Saved> {
    std::fs::read_to_string(path).ok().and_then(|s| serde_json::from_str(&s).ok()).unwrap_or_default()
}

pub fn save(path: &Path, list: &[Saved]) {
    let tmp = path.with_extension("json.tmp");
    let write = serde_json::to_string_pretty(list).map_err(std::io::Error::other).and_then(|s| std::fs::write(&tmp, s)).and_then(|_| std::fs::rename(&tmp, path));
    if let Err(e) = write {
        log::warn!("playlists: could not save: {e}");
    }
}

/// What a request did: the card, the notes for the model, and the address to open (if any).
#[derive(Debug, PartialEq)]
pub struct Done {
    pub card: String,
    pub detail: String,
    pub notes: String,
    pub open: Option<String>,
}

pub fn apply(list: &mut Vec<Saved>, a: Ask) -> Done {
    match a {
        Ask::Save(s) => {
            let replaced = list.iter().position(|x| norm(&x.name) == norm(&s.name));
            let card = format!("Saved \"{}\"", s.name);
            let notes = format!("Done: BYTE saved the {} link as \"{}\". The user can now say \"open Spotify to my {}\". Confirm in one short sentence.", s.kind, s.name, s.name);
            match replaced {
                Some(i) => list[i] = s,
                None => list.push(s),
            }
            Done { card, detail: "Say its name any time to play it".into(), notes, open: None }
        }
        Ask::Play { name } => match find(list, &name) {
            Some(s) => Done { card: format!("Playing \"{}\" in Spotify", s.name), detail: String::new(), notes: format!("Done: Spotify is opening \"{}\" and it should start playing. Confirm in one short sentence.", s.name), open: Some(uri(s)) },
            None => Done {
                card: format!("Searched Spotify for \"{name}\""),
                detail: "Not saved yet".into(),
                notes: format!("BYTE has no saved playlist called \"{name}\", so it opened Spotify's search for it. Tell the user to press play on the one they want, and that if they say \"my {name} playlist is\" and paste the playlist's share link, BYTE will play it by name from then on. Keep it short."),
                open: Some(search_uri(&name)),
            },
        },
        Ask::List => {
            if list.is_empty() {
                return Done { card: "No playlists saved".into(), detail: String::new(), notes: "BYTE has no saved Spotify playlists. Say how: \"my gaming playlist is\" and paste the share link. Keep it short.".into(), open: None };
            }
            let names: Vec<String> = list.iter().map(|s| s.name.clone()).collect();
            Done { card: format!("{} saved", names.len()), detail: names.join(", "), notes: format!("Saved Spotify playlists: {}. Say them out loud briefly.", names.join(", ")), open: None }
        }
        Ask::Forget { name } => {
            let before = list.len();
            list.retain(|s| norm(&s.name) != norm(&name));
            if list.len() < before {
                Done { card: format!("Forgot \"{name}\""), detail: String::new(), notes: format!("Done: BYTE forgot the playlist \"{name}\". Confirm in one short sentence."), open: None }
            } else {
                Done { card: "Not found".into(), detail: String::new(), notes: format!("BYTE has no saved playlist called \"{name}\". Say so in one short sentence."), open: None }
            }
        }
    }
}

/// Opens a `spotify:` address with whatever the system uses for it.
pub fn open_uri(uri: &str) -> Result<(), String> {
    #[cfg(windows)]
    {
        crate::pcctl::run_op(&crate::pcctl::WinOp::Open(uri.to_string())).map(|_| ()).map_err(|e| format!("{e:?}"))
    }
    #[cfg(target_os = "macos")]
    {
        std::process::Command::new("open").arg(uri).status().map_err(|e| e.to_string()).and_then(|s| if s.success() { Ok(()) } else { Err(format!("open exited with {s}")) })
    }
    #[cfg(not(any(windows, target_os = "macos")))]
    {
        std::process::Command::new("xdg-open").arg(uri).status().map_err(|e| e.to_string()).and_then(|s| if s.success() { Ok(()) } else { Err(format!("xdg-open exited with {s}")) })
    }
}

pub async fn run(turn: &Turn<'_>, question: &str, _cancel: &CancellationToken, send: Emit<'_>) -> AppResult<Option<(SourceBook, String)>> {
    let Some(a) = ask(question) else { return Ok(None) };
    let path = turn.app.map(|app| app.state::<AppState>().paths.data.join(FILE));
    let mut list = path.as_deref().map(load).unwrap_or_default();
    let before = list.clone();
    let id = format!("byte_playlist_{}", uuid::Uuid::new_v4().simple());
    let done = apply(&mut list, a);
    send(ChatEvent::ToolCall { id: id.clone(), name: "spotify_playlist".into(), args: json!({ "request": question }) })?;
    if list != before {
        if let Some(p) = &path {
            save(p, &list);
        }
    }
    let mut ok = true;
    let mut notes = done.notes.clone();
    if let Some(u) = &done.open {
        if let Err(e) = open_uri(u) {
            ok = false;
            notes = format!("BYTE could not open Spotify ({e}). Tell the user to check that Spotify is installed. Keep it short.");
        }
    }
    send(ChatEvent::ToolResult { id, ok, summary: done.card.clone() })?;
    turn.log.record("spotify_playlist", &json!({ "request": question }), ok, &done.card);
    send(ChatEvent::MacDone(crate::macctl::MacDone { app: "Spotify".into(), title: done.card.clone(), detail: done.detail.clone(), ok, undo: None }))?;
    Ok(Some((SourceBook::default(), notes)))
}

#[cfg(test)]
#[path = "playlists_tests.rs"]
mod tests;
