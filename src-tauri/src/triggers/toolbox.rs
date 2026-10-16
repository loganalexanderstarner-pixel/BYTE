//! A tool allow-list for unattended work (jobs, answers sent back to a phone). While a list is set around a turn, the agent
//! offers and runs only the tools in the named groups; everything else is refused. Without a list nothing changes.
//!
//! Groups: `web` (search, reading pages, weather, places, papers), `browser` (the hidden browser), `files` (search my files),
//! `memory` (remember), `pc` (PC control). The calculator is always allowed.

use std::future::Future;

use serde_json::Value;

tokio::task_local! {
    static GROUPS: Vec<String>;
}

pub const ALL_GROUPS: &[&str] = &["web", "browser", "files", "memory", "pc"];

/// The group a tool belongs to (None: always allowed).
pub fn group_of(tool: &str) -> Option<&'static str> {
    use crate::tools::{ACADEMIC_SEARCH, FIND_PLACES, READ_PAGE, REMEMBER, SEARCH_FILES, WEATHER, WEB_SEARCH};
    if [WEB_SEARCH, READ_PAGE, WEATHER, FIND_PLACES, ACADEMIC_SEARCH].contains(&tool) {
        Some("web")
    } else if tool == SEARCH_FILES {
        Some("files")
    } else if tool == REMEMBER {
        Some("memory")
    } else if crate::control::TOOLS.contains(&tool) {
        Some("pc")
    } else if crate::web_agent::TOOLS.contains(&tool) {
        Some("browser")
    } else {
        None
    }
}

/// Runs `f` with only these tool groups.
pub async fn with_groups<F: Future>(groups: Vec<String>, f: F) -> F::Output {
    GROUPS.scope(groups, f).await
}

/// May this tool be used right now?
pub fn allowed(tool: &str) -> bool {
    match (GROUPS.try_with(|g| g.clone()), group_of(tool)) {
        (Ok(g), Some(group)) => g.iter().any(|x| x == group),
        _ => true,
    }
}

/// Is a list in force?
pub fn limited() -> bool {
    GROUPS.try_with(|_| ()).is_ok()
}

/// Drops the tools that aren't allowed from the specs sent to the model.
pub fn filter_specs(specs: &mut Vec<Value>) {
    if limited() {
        specs.retain(|s| allowed(s["function"]["name"].as_str().unwrap_or("")));
    }
}

/// What the model is told when it asks for a tool it may not use.
pub const REFUSED: &str = "That tool isn't available for this task. Answer with what you have.";

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn without_a_list_everything_is_allowed() {
        assert!(!limited());
        assert!(allowed("web_search") && allowed("pc_control"));
    }

    #[tokio::test]
    async fn a_list_allows_only_its_groups() {
        with_groups(vec!["web".into()], async {
            assert!(limited());
            assert!(allowed("web_search") && allowed("read_page"));
            assert!(!allowed("pc_control"), "PC control is a group of its own");
            assert!(!allowed("remember") && !allowed("search_my_files"));
            assert!(allowed("calculate"), "the calculator is always on");
            let mut specs = vec![serde_json::json!({"function": {"name": "web_search"}}), serde_json::json!({"function": {"name": "pc_control"}})];
            filter_specs(&mut specs);
            assert_eq!(specs.len(), 1);
        })
        .await;
        with_groups(vec![], async {
            assert!(!allowed("web_search") && allowed("calculate"));
        })
        .await;
    }
}
