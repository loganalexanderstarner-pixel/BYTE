//! Self-check: after a cited answer (Deep, Extended, fact-check), BYTE checks
//! each cited sentence against the passage it cites, and flags the ones the
//! source doesn't clearly back, so the user knows what to double-check.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::chat;
use crate::error::AppResult;
use crate::research;

/// A claim its source doesn't clearly back.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Issue {
    pub claim: String,
    pub sources: Vec<u32>,
    /// "partly" or "no"
    pub verdict: String,
    pub note: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SelfCheck {
    pub checked: u32,
    pub issues: Vec<Issue>,
}

/// At most this many claims are checked (one short JSON reply).
pub const MAX_CLAIMS: usize = 8;

/// The answer's sentences that cite sources, with the numbers they cite.
pub fn cited_sentences(answer: &str) -> Vec<(String, Vec<u32>)> {
    let mut out: Vec<(String, Vec<u32>)> = Vec::new();
    for line in answer.lines() {
        let line = line.trim().trim_start_matches(['-', '*', '>', '#', ' ']).trim();
        // Sentences end at ". " or at the end of a line (bullets).
        for sentence in line.split_inclusive(". ") {
            let mut cites = Vec::new();
            let mut rest = sentence;
            while let Some(i) = rest.find('[') {
                let tail = &rest[i + 1..];
                let Some(j) = tail.find(']') else { break };
                for part in tail[..j].split([',', ' ']) {
                    if let Ok(n) = part.trim().parse::<u32>() {
                        if !cites.contains(&n) {
                            cites.push(n);
                        }
                    }
                }
                rest = &tail[j + 1..];
            }
            if cites.is_empty() {
                continue;
            }
            // The claim without its citation marks, bold, or links.
            let text: String = strip_marks(sentence);
            if text.split_whitespace().count() >= 4 {
                out.push((text, cites));
            }
            if out.len() >= MAX_CLAIMS {
                return out;
            }
        }
    }
    out
}

fn strip_marks(s: &str) -> String {
    let mut out = String::new();
    let mut skip = false;
    for ch in s.chars() {
        match ch {
            '[' => skip = true,
            ']' => skip = false,
            '*' | '`' => {}
            _ if !skip => out.push(ch),
            _ => {}
        }
    }
    out.split_whitespace().collect::<Vec<_>>().join(" ").trim_end_matches(['.', ',', ' ']).trim().to_string()
}

/// The passages each source number stands for, from the notes and tool
/// results the model was given ("[n] Title …" blocks).
pub fn source_blocks(contents: &[&str]) -> HashMap<u32, String> {
    let mut map: HashMap<u32, String> = HashMap::new();
    for c in contents {
        let mut current: Option<u32> = None;
        for line in c.lines() {
            let t = line.trim_start();
            if let Some(n) = t.strip_prefix('[').and_then(|r| r.split_once(']')).and_then(|(n, _)| n.parse::<u32>().ok()) {
                current = Some(n);
            }
            if let Some(n) = current {
                let e = map.entry(n).or_default();
                if e.len() < 2500 {
                    e.push_str(line);
                    e.push('\n');
                }
            }
        }
    }
    map
}

fn schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "checks": { "type": "array", "items": { "type": "object", "properties": {
                "claim": { "type": "integer" },
                "supported": { "type": "string", "enum": ["yes", "partly", "no"] },
                "note": { "type": "string" }
            }, "required": ["claim", "supported"] } }
        },
        "required": ["checks"]
    })
}

/// The model's verdicts, matched back to the claims.
pub fn parse_checks(reply: &str, claims: &[(String, Vec<u32>)]) -> Option<SelfCheck> {
    let v = research::lenient_json(reply);
    let checks = v["checks"].as_array()?;
    let mut issues = Vec::new();
    let mut seen = std::collections::HashSet::new();
    for c in checks {
        let Some(i) = c["claim"].as_u64().map(|i| i as usize).filter(|i| (1..=claims.len()).contains(i)) else { continue };
        if !seen.insert(i) {
            continue;
        }
        let verdict = c["supported"].as_str().unwrap_or("yes").to_lowercase();
        if verdict == "partly" || verdict == "no" {
            let (claim, sources) = &claims[i - 1];
            issues.push(Issue { claim: claim.clone(), sources: sources.clone(), verdict, note: c["note"].as_str().unwrap_or("").trim().to_string() });
        }
    }
    (!seen.is_empty()).then_some(SelfCheck { checked: seen.len() as u32, issues })
}

/// Checks the answer's cited claims against their sources. None when there's
/// nothing to check or the model's reply was unusable.
pub async fn check(http: &reqwest::Client, ep: &crate::engine::Endpoint, answer: &str, blocks: &HashMap<u32, String>) -> AppResult<Option<SelfCheck>> {
    let claims: Vec<(String, Vec<u32>)> = cited_sentences(answer).into_iter().filter(|(_, c)| c.iter().any(|n| blocks.contains_key(n))).collect();
    if claims.len() < 2 {
        return Ok(None);
    }
    let mut cited: Vec<u32> = claims.iter().flat_map(|(_, c)| c.iter().copied()).collect();
    cited.sort_unstable();
    cited.dedup();
    let sources: String = cited.iter().filter_map(|n| blocks.get(n)).map(|b| b.chars().take(1500).collect::<String>()).collect::<Vec<_>>().join("\n");
    let list: String = claims.iter().enumerate().map(|(i, (c, n))| format!("{}. {c} (cites {})\n", i + 1, n.iter().map(|x| format!("[{x}]")).collect::<String>())).collect();
    let user = format!(
        "Sources:\n{sources}\n\nClaims from an answer:\n{list}\nFor each claim, does the source it cites support it? \"yes\" if the \
source says it, \"partly\" if the source says something weaker or only part of it, \"no\" if the source doesn't say it. \
Give a short note for partly/no."
    );
    let reply = chat::complete_json(http, ep, "You check whether sources support claims, strictly. Reply only with JSON.", &user, schema(), 600).await?;
    Ok(parse_checks(&reply, &claims))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cited_sentences_are_found() {
        let answer = "## Summary\nThe brain uses **all** regions over a day [1][3]. It weighs about 1.4 kg [2, 4]. No cite here.\n- Imaging shows activity everywhere [3]\n- Short [1]\n";
        let got = cited_sentences(answer);
        assert_eq!(got, vec![
            ("The brain uses all regions over a day".to_string(), vec![1, 3]),
            ("It weighs about 1.4 kg".to_string(), vec![2, 4]),
            ("Imaging shows activity everywhere".to_string(), vec![3]),
        ]);
    }

    #[test]
    fn blocks_and_checks() {
        let notes = "Research notes:\n[1] Brain myths (sciam.com)\nThe 10% figure is a myth.\nPET scans show…\n[2] Anatomy (wiki)\nThe brain weighs 1.3–1.4 kg.";
        let b = source_blocks(&[notes]);
        assert!(b[&1].contains("10% figure is a myth"));
        assert!(b[&2].contains("1.4 kg"));
        assert!(!b[&1].contains("1.4 kg"));
        let claims = vec![("A".to_string(), vec![1]), ("B".to_string(), vec![2]), ("C".to_string(), vec![1])];
        let sc = parse_checks(r#"{"checks":[{"claim":1,"supported":"yes"},{"claim":2,"supported":"partly","note":"says 1.3 to 1.4"},{"claim":2,"supported":"no"},{"claim":9,"supported":"no"}]}"#, &claims).unwrap();
        assert_eq!(sc.checked, 2);
        assert_eq!(sc.issues, vec![Issue { claim: "B".into(), sources: vec![2], verdict: "partly".into(), note: "says 1.3 to 1.4".into() }]);
        assert!(parse_checks("oops", &claims).is_none());
    }
}
