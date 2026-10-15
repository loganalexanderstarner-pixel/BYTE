//! Game guides with spoiler-free hints: "I'm stuck on the water temple in
//! Ocarina of Time", "how do I beat Malenia in Elden Ring". BYTE reads game
//! wikis and guides, then gives hints that get stronger one at a time (the
//! card keeps each one hidden until tapped) and the full solution last. Nothing
//! past the point the player is at.

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tokio_util::sync::CancellationToken;

use crate::agent::Turn;
use crate::chat::{self, ChatEvent};
use crate::error::AppResult;
use crate::research::{self, Ctx, Emit, Gathered};
use crate::tools::SourceBook;

/// The hints card.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Hints {
    pub game: String,
    /// Where the player is stuck ("the Water Temple, the locked door on floor 2").
    pub spot: String,
    /// From a gentle nudge to nearly the answer.
    pub hints: Vec<String>,
    /// Step-by-step, shown only when asked for.
    pub solution: String,
    pub sources: Vec<u32>,
}

/// Words that mean "I'm stuck in a game".
const STUCK: &[&str] = &[
    "stuck on", "stuck at", "stuck in", "can't beat", "cant beat", "can't get past", "cant get past", "how do i beat", "how to beat", "how do you beat",
    "how do i solve", "how to solve the", "how do i get past", "how do i unlock", "how to unlock", "how do i get to", "hint for", "hints for", "walkthrough",
    "how do i find the", "where do i find the", "where is the", "boss fight", "puzzle in",
];

/// Words that show it's about a game.
const GAME_WORDS: &[&str] = &[
    "boss", "level", "dungeon", "temple", "quest", "puzzle", "shrine", "chapter", "mission", "achievement", "trophy", "secret", "game", "stage", "world ",
    "raid", "zelda", "mario", "pokemon", "pokémon", "elden ring", "dark souls", "minecraft", "hollow knight", "skyrim", "baldur", "metroid", "portal",
    "celeste", "hades", "silksong", "sekiro", "bloodborne", "fortnite", "animal crossing", "god of war", "final fantasy", "persona", "witcher", "cyberpunk 2077",
    "tears of the kingdom", "breath of the wild", "ocarina",
];

/// "I'm stuck on the water temple in Ocarina of Time", "how do I beat Malenia in Elden Ring".
pub fn wants_game_help(question: &str) -> bool {
    let q = question.to_lowercase();
    let stuck = STUCK.iter().any(|s| q.contains(s));
    let game = GAME_WORDS.iter().any(|w| q.contains(w));
    // "how do I solve this equation", "where is the nearest pharmacy": not games.
    let not_game = ["equation", "integral", "math", "homework", "nearest", "near me", "error", "bug", "exception", "rubik"].iter().any(|w| q.contains(w));
    stuck && game && !not_game
}

pub fn applies(enabled: bool, web: bool, question: &str) -> bool {
    enabled && web && wants_game_help(question)
}

fn schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "game": { "type": "string" },
            "spot": { "type": "string" },
            "hints": { "type": "array", "minItems": 1, "maxItems": 3, "items": { "type": "string" } },
            "solution": { "type": "string" },
            "sources": { "type": "array", "items": { "type": "integer" } }
        },
        "required": ["game", "hints", "solution"]
    })
}

pub fn parse_hints(reply: &str, valid: &[u32]) -> Option<Hints> {
    let v = research::lenient_json(reply);
    let s = |k: &str| v[k].as_str().unwrap_or("").trim().to_string();
    let mut hints: Vec<String> = Vec::new();
    for h in v["hints"].as_array().into_iter().flatten().filter_map(|h| h.as_str()) {
        let h = h.trim().to_string();
        if h.len() > 3 && !hints.iter().any(|x| x.eq_ignore_ascii_case(&h)) {
            hints.push(h);
        }
    }
    hints.truncate(3);
    let solution = s("solution");
    if hints.is_empty() && solution.is_empty() {
        return None;
    }
    let mut sources: Vec<u32> = v["sources"].as_array().into_iter().flatten().filter_map(|n| n.as_u64()).map(|n| n as u32).filter(|n| valid.contains(n)).collect();
    sources.sort_unstable();
    sources.dedup();
    Some(Hints { game: s("game"), spot: s("spot"), hints, solution, sources })
}

pub const HINT_RULES: &str = "The user sees a hints card above: the hints are hidden until they tap each one, and the \
full solution is hidden too. In your answer, give ONLY the first, gentlest nudge (one or two sentences), then say the \
stronger hints and the full solution are in the card. Don't reveal anything from the later hints or the solution, and \
nothing about story, bosses, items or places past the point they're at. Cite the guides as [n].";

pub async fn run(turn: &Turn<'_>, question: &str, used_tokens: usize, cancel: &CancellationToken, send: Emit<'_>) -> AppResult<Option<(SourceBook, String)>> {
    let c = Ctx { turn, cancel, send };
    let mut g = Gathered::default();
    let q = crate::agent::search_query(question, None);
    let queries = vec![format!("{q} guide"), format!("{q} walkthrough"), format!("{q} reddit tips")];
    let lists = research::run_searches(&c, &mut g, &queries, "g").await?;
    let want = research::scale_pages(5, turn.depth);
    let candidates = research::interleave(&lists, &[], want * 2);
    research::read_pages(&c, &mut g, &candidates, want, "g").await?;

    c.call("byte_grank", "rank_passages", json!({}))?;
    let budget = research::notes_budget(turn, used_tokens, 0.4);
    let (picks, by_meaning) = c.cancellable(research::rank_texts(turn, &g.texts, question, "how to beat solve get past strategy", budget, 3)).await?;
    let notes = research::format_notes(&g.book, &picks);
    c.result("byte_grank", !picks.is_empty(), research::rank_summary(picks.len(), &notes, by_meaning))?;

    c.call("byte_ghints", "write_hints", json!({}))?;
    let user = format!(
        "The player asks: {question}\n\nGuides (numbered sources):\n{}\n\nWrite spoiler-free help for exactly where they're \
stuck: up to 3 hints, from a gentle nudge (what to pay attention to) to a strong hint (nearly the answer), then the full \
solution step by step. Never mention story events, bosses, items or areas beyond this point. Name the game and the spot, \
and list the numbers of the guides you used.",
        notes.chars().take(budget.min(10_000)).collect::<String>()
    );
    let reply = c.cancellable(chat::complete_json(turn.http, turn.ep, "You give careful, spoiler-free game hints. Reply only with JSON.", &user, schema(), 900)).await?.unwrap_or_default();
    let valid: Vec<u32> = g.book.sources.iter().map(|s| s.n).collect();
    let hints = parse_hints(&reply, &valid);
    c.result("byte_ghints", hints.is_some(), hints.as_ref().map(|h| format!("{} hints", h.hints.len())).unwrap_or_else(|| "Couldn't write hints".into()))?;
    let Some(hints) = hints else {
        return Ok(Some((g.book, format!("Game guide notes for: {question}\n\n{notes}\n\nGive a gentle hint first and ask if they want more; avoid spoilers."))));
    };
    let first = hints.hints.first().cloned().unwrap_or_default();
    send(ChatEvent::Hints(hints))?;
    // The model only sees the first hint, so it can't give the rest away.
    Ok(Some((g.book, format!("Game help for: {question}\n\nThe first, gentlest hint (the only one to mention): {first}\n\nSources read: {}\n\n{HINT_RULES}", valid.iter().map(|n| format!("[{n}]")).collect::<String>()))))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn which_questions_are_game_help() {
        for yes in ["I'm stuck on the Water Temple in Ocarina of Time", "How do I beat Malenia in Elden Ring?", "hint for the shrine puzzle near Lookout Landing", "can't get past the second boss in Hollow Knight"] {
            assert!(wants_game_help(yes), "{yes}");
        }
        for no in ["how do I solve this equation", "where is the nearest pharmacy", "I'm stuck on this bug in my game engine error", "how to beat procrastination", "Elden Ring review"] {
            assert!(!wants_game_help(no), "{no}");
        }
    }

    #[test]
    fn hints_are_cleaned() {
        let reply = r#"{"game":"Elden Ring","spot":"Malenia","hints":["Watch her stagger windows"," watch her stagger windows","Waterfowl Dance can be dodged by running away first",""],"solution":"1. Summon ...","sources":[2,7,3]}"#;
        let h = parse_hints(reply, &[2, 3]).unwrap();
        assert_eq!(h.hints.len(), 2);
        assert_eq!(h.sources, vec![2, 3]);
        assert_eq!(h.game, "Elden Ring");
        assert!(parse_hints("{}", &[1]).is_none());
    }
}
