//! Mind maps (Phase 11): any answer (or note) as a tree to look at. Most
//! answers already have structure, so the tree comes straight from the
//! Markdown (headings → branches, bullets → leaves); only unstructured text
//! asks the model, for a small JSON outline. The UI lays it out and draws it.

use serde::{Deserialize, Serialize};
use serde_json::json;
use tauri::State;

use crate::error::{AppError, AppResult};
use crate::state::AppState;

const MAX_BRANCHES: usize = 7;
const MAX_CHILDREN: usize = 6;
const MAX_DEPTH: usize = 3;
const MAX_WORDS: usize = 8;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
#[serde(rename_all = "camelCase")]
pub struct MapNode {
    pub label: String,
    #[serde(default)]
    pub children: Vec<MapNode>,
}

/// A short label: no markdown, citation marks or trailing punctuation, at most `MAX_WORDS` words.
pub fn label(text: &str) -> String {
    let plain = crate::speech::speakable(text).replace("\n\n", " ");
    let plain = plain.trim().trim_end_matches(['.', ':', ';', ',']).to_string();
    // "Term: explanation" → "Term" when the term is short.
    let plain = match plain.split_once(": ") {
        Some((head, _)) if head.split_whitespace().count() <= 5 => head.to_string(),
        _ => plain,
    };
    let words: Vec<&str> = plain.split_whitespace().collect();
    if words.len() <= MAX_WORDS { words.join(" ") } else { format!("{}…", words[..MAX_WORDS].join(" ")) }
}

/// Keeps the tree a readable size: branches, children and depth limited, empty labels dropped.
pub fn clamp(mut n: MapNode, depth: usize) -> MapNode {
    n.label = n.label.trim().to_string();
    let limit = if depth == 0 { MAX_BRANCHES } else { MAX_CHILDREN };
    n.children = if depth >= MAX_DEPTH {
        vec![]
    } else {
        n.children.into_iter().filter(|c| !c.label.trim().is_empty()).take(limit).map(|c| clamp(c, depth + 1)).collect()
    };
    n
}

/// The tree in an answer's Markdown: the first H1 (or `center`) in the middle, H2/H3 as branches, bullets
/// (nested by indent) under them. None when there isn't enough structure (fewer than 2 branches).
pub fn from_markdown(md: &str, center: &str) -> Option<MapNode> {
    let mut root = MapNode { label: label(center), children: vec![] };
    // The path of open nodes: (level, index path). Levels: heading 2 → 1, heading 3 → 2, bullet → 10 + indent.
    let mut stack: Vec<(usize, Vec<usize>)> = vec![];
    let mut in_code = false;
    for raw in md.lines() {
        let line = raw.trim_end();
        if line.trim_start().starts_with("```") {
            in_code = !in_code;
            continue;
        }
        if in_code || line.trim().is_empty() || line.trim_start().starts_with('|') {
            continue;
        }
        let t = line.trim_start();
        let indent = line.len() - t.len();
        let (level, text) = if let Some(h) = t.strip_prefix("# ") {
            if root.children.is_empty() && stack.is_empty() {
                root.label = label(h);
            }
            continue;
        } else if let Some(h) = t.strip_prefix("## ") {
            (1, h)
        } else if let Some(h) = t.strip_prefix("### ").or_else(|| t.strip_prefix("#### ")) {
            (2, h)
        } else if let Some(b) = t.strip_prefix("- ").or_else(|| t.strip_prefix("* ")).or_else(|| {
            t.split_once(". ").filter(|(n, _)| !n.is_empty() && n.len() <= 2 && n.chars().all(|c| c.is_ascii_digit())).map(|(_, r)| r)
        }) {
            (10 + indent / 2, b)
        } else {
            continue;
        };
        let lbl = label(text);
        if lbl.is_empty() || lbl.eq_ignore_ascii_case("tl;dr") {
            continue;
        }
        while stack.last().is_some_and(|(l, _)| *l >= level) {
            stack.pop();
        }
        let parent_path = stack.last().map(|(_, p)| p.clone()).unwrap_or_default();
        let parent = node_at(&mut root, &parent_path);
        parent.children.push(MapNode { label: lbl, children: vec![] });
        let mut path = parent_path;
        path.push(parent.children.len() - 1);
        stack.push((level, path));
    }
    // A single heading with everything under it: make its children the branches.
    if root.children.len() == 1 && root.children[0].children.len() >= 2 {
        let only = root.children.remove(0);
        root.children = only.children;
    }
    (root.children.len() >= 2).then(|| clamp(root, 0))
}

/// The node at an index path (indexes of children pushed while reading).
fn node_at<'a>(root: &'a mut MapNode, path: &[usize]) -> &'a mut MapNode {
    path.iter().fold(root, |n, &i| &mut n.children[i])
}

fn schema() -> serde_json::Value {
    let leaf = json!({ "type": "object", "properties": { "label": { "type": "string" } }, "required": ["label"] });
    let mid = json!({ "type": "object", "properties": { "label": { "type": "string" }, "children": { "type": "array", "items": leaf } }, "required": ["label", "children"] });
    json!({ "type": "object", "properties": { "label": { "type": "string" }, "children": { "type": "array", "items": mid } }, "required": ["label", "children"] })
}

const SYSTEM: &str = "You turn text into a mind map. Reply with JSON only: {\"label\": central topic (2-4 words), \"children\": \
[{\"label\": main idea (1-5 words), \"children\": [{\"label\": detail (1-6 words)}]}]}. 3 to 6 main ideas, 2 to 5 details each. \
Use only what the text says. Labels are short phrases, not sentences.";

/// A mind map of `text`: from its structure when it has one, else from the loaded model.
#[tauri::command]
pub async fn mindmap_make(state: State<'_, AppState>, text: String, title: String) -> AppResult<MapNode> {
    let center = if title.trim().is_empty() { "Topic" } else { title.trim() };
    if let Some(m) = from_markdown(&text, center) {
        return Ok(m);
    }
    let ep = match state.engine.endpoint().await {
        Some(ep) => ep,
        None => crate::backend::cloud_cards(&state).await.ok_or_else(|| AppError::msg("Making a mind map of this needs a model: load one in Settings → Models."))?,
    };
    from_model(&state.local_http, &ep, &text, center).await
}

/// A mind map written by the model (for text without headings or lists).
pub async fn from_model(http: &reqwest::Client, ep: &crate::engine::Endpoint, text: &str, center: &str) -> AppResult<MapNode> {
    let body: String = text.chars().take(12_000).collect();
    let raw = crate::chat::complete_json(http, ep, SYSTEM, &body, schema(), 900).await?;
    let mut m: MapNode = serde_json::from_value(crate::research::lenient_json(&raw)).map_err(|_| AppError::msg("The model's mind map didn't come out right; try again."))?;
    if m.label.trim().is_empty() {
        m.label = label(center);
    }
    m.label = label(&m.label);
    for c in m.children.iter_mut() {
        c.label = label(&c.label);
        for g in c.children.iter_mut() {
            g.label = label(&g.label);
        }
    }
    let m = clamp(m, 0);
    if m.children.is_empty() {
        return Err(AppError::msg("There wasn't enough in that answer for a mind map."));
    }
    Ok(m)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn labels_are_short_and_plain() {
        assert_eq!(label("**Battery life**: lasts about 18 hours on a charge [2]."), "Battery life");
        assert_eq!(label("This is a much longer sentence that goes on and on about things"), "This is a much longer sentence that goes…");
    }

    #[test]
    fn headings_and_bullets_become_a_tree() {
        let md = "> **TL;DR:** Get the Air.\n\n# MacBook Air vs Pro\n\n## Speed\n- M4 chip in both\n- Pro has fans\n  - steady under load\n\n## Battery\n- Air: 18 hours\n- Pro: 22 hours\n\n```\ncode\n```\n\n## Price\n1. Air from $999\n2. Pro from $1,599\n";
        let m = from_markdown(md, "Answer").unwrap();
        assert_eq!(m.label, "MacBook Air vs Pro");
        let names: Vec<&str> = m.children.iter().map(|c| c.label.as_str()).collect();
        assert_eq!(names, ["Speed", "Battery", "Price"]);
        assert_eq!(m.children[0].children[1].label, "Pro has fans");
        assert_eq!(m.children[0].children[1].children[0].label, "steady under load");
        assert_eq!(m.children[2].children.len(), 2);
    }

    #[test]
    fn a_plain_list_works_and_prose_does_not() {
        let m = from_markdown("Pack these:\n- Passport\n- Charger\n- Snacks", "Trip packing").unwrap();
        assert_eq!((m.label.as_str(), m.children.len()), ("Trip packing", 3));
        assert!(from_markdown("Just one paragraph of text with no structure at all.", "x").is_none());
    }

    #[test]
    fn big_trees_are_clamped() {
        let leaf = |i: usize| MapNode { label: format!("l{i}"), children: vec![MapNode { label: "deep".into(), children: vec![MapNode { label: "deeper".into(), children: vec![MapNode { label: "too deep".into(), children: vec![] }] }] }] };
        let m = clamp(MapNode { label: "c".into(), children: (0..20).map(leaf).collect() }, 0);
        assert_eq!(m.children.len(), MAX_BRANCHES);
        assert!(m.children[0].children[0].children[0].children.is_empty());
    }

    /// Real engine (BYTE_TEST_LLAMA_SERVER + BYTE_TEST_MODEL): prose with no structure becomes a usable map.
    #[tokio::test]
    #[ignore]
    async fn e2e_mindmap_from_prose() {
        let Some((_server, ep)) = crate::chat::e2e_support::start_server().await else { return };
        let text = "Coffee starts as the seed of a cherry grown in places like Ethiopia, Colombia and Brazil. After picking, the beans are \
washed or dried in the sun, then roasted: light roasts taste fruity and bright, dark roasts taste bitter and chocolatey. At home, \
grind size matters most; espresso needs a fine grind, a French press a coarse one, and pour-over sits in the middle. Water just \
below boiling and fresh beans make the biggest difference to taste.";
        let m = from_model(&crate::chat::local_client(), &ep, text, "Coffee").await.unwrap();
        println!("{m:#?}");
        assert!(m.children.len() >= 2, "{m:?}");
        assert!(m.children.iter().all(|c| !c.label.is_empty() && c.label.split_whitespace().count() <= 9));
    }
}
