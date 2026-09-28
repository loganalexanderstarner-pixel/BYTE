//! Scholarly search without accounts or keys: Crossref (every field),
//! Europe PMC (medicine and life sciences, with abstracts) and arXiv
//! (physics, maths, computer science preprints), asked in parallel. Results
//! carry authors, year, venue and DOI so answers can be cited properly
//! (APA, MLA, … in the UI, `lib/citations.ts`).
//!
//! Any source can fail (rate limits, a network that blocks it); the others
//! still answer. OpenAlex and Semantic Scholar aren't used: without a key
//! they refuse requests from shared networks.

use std::time::Duration;

use serde_json::Value;

use crate::error::{AppError, AppResult};

#[derive(Debug, Clone, PartialEq, Default)]
pub struct Paper {
    pub title: String,
    /// "Given Family" names, in order.
    pub authors: Vec<String>,
    pub year: Option<i32>,
    /// Journal, conference or "arXiv".
    pub venue: String,
    pub doi: Option<String>,
    /// Where to read it (the DOI link when there is one).
    pub url: String,
    /// Plain-text abstract ("" when the source has none).
    pub abstract_text: String,
    /// Which service found it.
    pub from: &'static str,
}

const TIMEOUT: Duration = Duration::from_secs(15);

/// Searches all three services at once and merges the results, alternating
/// between them so one source can't crowd out the others. Papers with an
/// abstract come first; duplicates (same DOI or title) are dropped.
pub async fn search(net: &reqwest::Client, query: &str, max: usize) -> AppResult<Vec<Paper>> {
    let per = max.clamp(3, 10);
    let (cr, pmc, ax) = tokio::join!(crossref(net, query, per), europe_pmc(net, query, per), arxiv(net, query, per));
    let mut errors = Vec::new();
    let lists: Vec<Vec<Paper>> = [cr, pmc, ax]
        .into_iter()
        .filter_map(|r| r.map_err(|e| errors.push(e.to_string())).ok())
        .map(|l| l.into_iter().filter(|p| on_topic(p, query)).collect())
        .collect();
    if lists.iter().all(Vec::is_empty) && !errors.is_empty() {
        return Err(AppError::msg(format!("paper search failed ({})", errors.join("; "))));
    }
    Ok(merge(lists, max))
}

fn merge(lists: Vec<Vec<Paper>>, max: usize) -> Vec<Paper> {
    let mut out: Vec<Paper> = Vec::new();
    let longest = lists.iter().map(Vec::len).max().unwrap_or(0);
    for i in 0..longest {
        for l in &lists {
            let Some(p) = l.get(i) else { continue };
            if !out.iter().any(|q| same_paper(p, q)) {
                out.push(p.clone());
            }
        }
    }
    // Stable: keeps the alternating order within each group.
    out.sort_by_key(|p| p.abstract_text.is_empty());
    out.truncate(max);
    out
}

fn norm_title(t: &str) -> String {
    t.to_lowercase().chars().filter(|c| c.is_alphanumeric()).collect()
}

fn same_paper(a: &Paper, b: &Paper) -> bool {
    matches!((&a.doi, &b.doi), (Some(x), Some(y)) if x.eq_ignore_ascii_case(y)) || norm_title(&a.title) == norm_title(&b.title)
}

/// Crossref's relevance search matches single words anywhere; keep papers
/// whose title or abstract share at least half of the query's key words.
fn on_topic(p: &Paper, query: &str) -> bool {
    let words: Vec<String> = query
        .to_lowercase()
        .split(|c: char| !c.is_alphanumeric())
        .filter(|w| w.len() > 3 && !["what", "does", "with", "from", "about", "that", "this", "which", "study", "studies", "research", "evidence", "paper", "papers"].contains(w))
        .map(str::to_string)
        .collect();
    if words.is_empty() {
        return true;
    }
    let text = format!("{} {}", p.title, p.abstract_text).to_lowercase();
    let hits = words.iter().filter(|w| text.contains(&w[..w.len().min(6)])).count();
    hits * 2 >= words.len()
}

async fn get(net: &reqwest::Client, url: reqwest::Url) -> AppResult<String> {
    let r = net.get(url).timeout(TIMEOUT).send().await?;
    if !r.status().is_success() {
        return Err(AppError::msg(format!("{} answered {}", r.url().host_str().unwrap_or("?"), r.status())));
    }
    Ok(r.text().await?)
}

fn url_with(base: &str, params: &[(&str, &str)]) -> reqwest::Url {
    reqwest::Url::parse_with_params(base, params).expect("static base URL")
}

async fn crossref(net: &reqwest::Client, query: &str, n: usize) -> AppResult<Vec<Paper>> {
    let rows = n.to_string();
    let url = url_with(
        "https://api.crossref.org/works",
        &[("query.bibliographic", query), ("rows", &rows), ("select", "title,author,issued,container-title,DOI,URL,abstract,type")],
    );
    let v: Value = serde_json::from_str(&get(net, url).await?).map_err(|e| AppError::msg(format!("crossref: {e}")))?;
    Ok(parse_crossref(&v))
}

async fn europe_pmc(net: &reqwest::Client, query: &str, n: usize) -> AppResult<Vec<Paper>> {
    let size = n.to_string();
    let url = url_with(
        "https://www.ebi.ac.uk/europepmc/webservices/rest/search",
        &[("query", query), ("format", "json"), ("resultType", "core"), ("pageSize", &size)],
    );
    let v: Value = serde_json::from_str(&get(net, url).await?).map_err(|e| AppError::msg(format!("europe pmc: {e}")))?;
    Ok(parse_europe_pmc(&v))
}

async fn arxiv(net: &reqwest::Client, query: &str, n: usize) -> AppResult<Vec<Paper>> {
    let size = n.to_string();
    let q = format!("all:{}", query.split_whitespace().collect::<Vec<_>>().join(" AND all:"));
    let url = url_with("https://export.arxiv.org/api/query", &[("search_query", &q), ("max_results", &size), ("sortBy", "relevance")]);
    Ok(parse_arxiv(&get(net, url).await?))
}

/// Removes markup (JATS in Crossref abstracts, HTML in Europe PMC's) and
/// squeezes whitespace.
pub fn plain(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut in_tag = false;
    for c in s.chars() {
        match c {
            '<' => {
                in_tag = true;
                out.push(' ');
            }
            '>' if in_tag => in_tag = false,
            _ if !in_tag => out.push(c),
            _ => {}
        }
    }
    let out = out.replace("&amp;", "&").replace("&lt;", "<").replace("&gt;", ">").replace("&quot;", "\"").replace("&#39;", "'");
    let squeezed = out.split_whitespace().collect::<Vec<_>>().join(" ");
    // "Abstract" headings left at the start.
    squeezed.strip_prefix("Abstract ").map(str::to_string).unwrap_or(squeezed)
}

fn first_str(v: &Value) -> String {
    match v {
        Value::Array(a) => a.first().and_then(Value::as_str).unwrap_or("").to_string(),
        Value::String(s) => s.clone(),
        _ => String::new(),
    }
}

pub fn parse_crossref(v: &Value) -> Vec<Paper> {
    let Some(items) = v["message"]["items"].as_array() else { return Vec::new() };
    items
        .iter()
        .filter_map(|it| {
            let title = plain(&first_str(&it["title"]));
            if title.is_empty() {
                return None;
            }
            let authors = it["author"]
                .as_array()
                .map(|a| {
                    a.iter()
                        .filter_map(|p| {
                            let given = p["given"].as_str().unwrap_or("");
                            let family = p["family"].as_str().or(p["name"].as_str())?;
                            Some(format!("{given} {family}").trim().to_string())
                        })
                        .collect()
                })
                .unwrap_or_default();
            let year = it["issued"]["date-parts"][0][0].as_i64().map(|y| y as i32);
            let doi = it["DOI"].as_str().map(str::to_string);
            let url = doi.as_ref().map(|d| format!("https://doi.org/{d}")).or_else(|| it["URL"].as_str().map(str::to_string))?;
            Some(Paper {
                title,
                authors,
                year,
                venue: plain(&first_str(&it["container-title"])),
                doi,
                url,
                abstract_text: plain(it["abstract"].as_str().unwrap_or("")),
                from: "Crossref",
            })
        })
        .collect()
}

/// "Oselinsky KM, Hill EB" → ["K. M. Oselinsky", …] would guess at names;
/// Europe PMC's `authorList` has full names, so it's used when present.
fn pmc_authors(r: &Value) -> Vec<String> {
    if let Some(list) = r["authorList"]["author"].as_array() {
        let names: Vec<String> = list
            .iter()
            .filter_map(|a| match (a["firstName"].as_str(), a["lastName"].as_str()) {
                (Some(f), Some(l)) => Some(format!("{f} {l}")),
                _ => a["fullName"].as_str().or(a["collectiveName"].as_str()).map(str::to_string),
            })
            .collect();
        if !names.is_empty() {
            return names;
        }
    }
    r["authorString"]
        .as_str()
        .unwrap_or("")
        .trim_end_matches('.')
        .split(", ")
        .filter(|s| !s.trim().is_empty())
        .map(|s| {
            // "Hill EB" → "E. B. Hill"
            match s.rsplit_once(' ') {
                Some((last, initials)) if initials.chars().all(|c| c.is_ascii_uppercase()) && initials.len() <= 3 => {
                    let i: Vec<String> = initials.chars().map(|c| format!("{c}.")).collect();
                    format!("{} {last}", i.join(" "))
                }
                _ => s.to_string(),
            }
        })
        .collect()
}

pub fn parse_europe_pmc(v: &Value) -> Vec<Paper> {
    let Some(results) = v["resultList"]["result"].as_array() else { return Vec::new() };
    results
        .iter()
        .filter_map(|r| {
            let title = plain(r["title"].as_str().unwrap_or("")).trim_end_matches('.').to_string();
            if title.is_empty() {
                return None;
            }
            let doi = r["doi"].as_str().map(str::to_string);
            let url = match (&doi, r["pmcid"].as_str(), r["pmid"].as_str()) {
                (Some(d), _, _) => format!("https://doi.org/{d}"),
                (None, Some(pmc), _) => format!("https://europepmc.org/article/PMC/{pmc}"),
                (None, None, Some(pm)) => format!("https://pubmed.ncbi.nlm.nih.gov/{pm}/"),
                _ => format!("https://europepmc.org/article/{}/{}", r["source"].as_str().unwrap_or("MED"), r["id"].as_str().unwrap_or("")),
            };
            Some(Paper {
                title,
                authors: pmc_authors(r),
                year: r["pubYear"].as_str().and_then(|y| y.parse().ok()),
                venue: r["journalInfo"]["journal"]["title"].as_str().map(str::to_string).unwrap_or_else(|| {
                    if r["source"] == "PPR" {
                        "Preprint".into()
                    } else {
                        String::new()
                    }
                }),
                doi,
                url,
                abstract_text: plain(r["abstractText"].as_str().unwrap_or("")),
                from: "Europe PMC",
            })
        })
        .collect()
}

pub fn parse_arxiv(xml: &str) -> Vec<Paper> {
    use quick_xml::events::Event;
    let mut reader = quick_xml::Reader::from_str(xml);
    let mut out = Vec::new();
    let mut cur: Option<Paper> = None;
    let mut field: Option<&'static str> = None;
    let mut text = String::new();
    loop {
        match reader.read_event() {
            Ok(Event::Start(e)) => {
                let name = e.name();
                match name.as_ref() {
                    b"entry" => cur = Some(Paper { venue: "arXiv".into(), from: "arXiv", ..Default::default() }),
                    b"title" | b"summary" | b"name" | b"published" | b"id" | b"arxiv:doi" if cur.is_some() => {
                        field = Some(match name.as_ref() {
                            b"title" => "title",
                            b"summary" => "summary",
                            b"name" => "name",
                            b"published" => "published",
                            b"id" => "id",
                            _ => "doi",
                        });
                        text.clear();
                    }
                    _ => {}
                }
            }
            Ok(Event::Text(t)) if field.is_some() => {
                if let Ok(s) = t.unescape() {
                    text.push_str(&s);
                }
            }
            Ok(Event::End(e)) => {
                if e.name().as_ref() == b"entry" {
                    if let Some(p) = cur.take().filter(|p| !p.title.is_empty() && !p.url.is_empty()) {
                        out.push(p);
                    }
                } else if let (Some(f), Some(p)) = (field.take(), cur.as_mut()) {
                    let t = text.split_whitespace().collect::<Vec<_>>().join(" ");
                    match f {
                        "title" => p.title = t,
                        "summary" => p.abstract_text = t,
                        "name" => p.authors.push(t),
                        "published" => p.year = t.get(..4).and_then(|y| y.parse().ok()),
                        "id" => p.url = t.replacen("http://", "https://", 1),
                        _ => p.doi = Some(t),
                    }
                }
            }
            Ok(Event::Eof) | Err(_) => break,
            _ => {}
        }
    }
    // arXiv gives every paper a DOI of the form 10.48550/arXiv.<id>.
    for p in &mut out {
        if p.doi.is_none() {
            if let Some(id) = p.url.rsplit("/abs/").next().filter(|_| p.url.contains("/abs/")) {
                let id = id.split('v').next().unwrap_or(id);
                p.doi = Some(format!("10.48550/arXiv.{id}"));
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture(name: &str) -> String {
        std::fs::read_to_string(format!("{}/tests/fixtures/{name}", env!("CARGO_MANIFEST_DIR"))).unwrap()
    }

    #[test]
    fn reads_crossref() {
        let v: Value = serde_json::from_str(&fixture("crossref.json")).unwrap();
        let papers = parse_crossref(&v);
        assert_eq!(papers.len(), 3);
        let p = &papers[0];
        assert_eq!(p.title, "Confidence-Modulated Speculative Decoding for Large Language Models");
        assert_eq!(p.authors[0], "Jaydip Sen");
        assert_eq!(p.year, Some(2025));
        assert!(p.venue.contains("INDISCON"));
        assert_eq!(p.url, "https://doi.org/10.1109/indiscon66021.2025.11254640");
        // JATS markup and the "Abstract" heading are removed.
        assert!(papers[1].abstract_text.starts_with("Speculative decoding speeds up"), "{}", papers[1].abstract_text);
    }

    #[test]
    fn reads_europe_pmc() {
        let v: Value = serde_json::from_str(&fixture("europepmc.json")).unwrap();
        let papers = parse_europe_pmc(&v);
        assert_eq!(papers.len(), 3);
        let p = &papers[0];
        assert!(p.title.starts_with("Exploring the Feasibility") && !p.title.ends_with('.'));
        assert_eq!(p.authors[0], "K. M. Oselinsky");
        assert_eq!(p.year, Some(2026));
        assert_eq!(p.doi.as_deref(), Some("10.2196/86281"));
        assert!(!p.abstract_text.contains('<'));
        assert!(p.abstract_text.starts_with("Background"), "{}", &p.abstract_text[..40]);
    }

    #[test]
    fn reads_arxiv() {
        let papers = parse_arxiv(&fixture("arxiv.xml"));
        assert_eq!(papers.len(), 2);
        assert_eq!(papers[0].title, "Fast Inference from Transformers via Speculative Decoding");
        assert_eq!(papers[0].authors, vec!["Yaniv Leviathan", "Matan Kalman", "Yossi Matias"]);
        assert_eq!(papers[0].year, Some(2022));
        assert_eq!(papers[0].doi.as_deref(), Some("10.48550/arXiv.2211.17192"));
        assert_eq!(papers[0].url, "https://arxiv.org/abs/2211.17192v2");
        assert!(papers[1].abstract_text.contains("speculative sampling & show"));
        assert_eq!(papers[1].doi.as_deref(), Some("10.48550/arXiv.2302.01318"));
    }

    #[test]
    fn merging_alternates_and_drops_duplicates() {
        let p = |t: &str, doi: Option<&str>, abs: &str, from| Paper { title: t.into(), doi: doi.map(Into::into), abstract_text: abs.into(), from, ..Default::default() };
        let a = vec![p("One", Some("10.1/a"), "x", "A"), p("Two", None, "", "A")];
        let b = vec![p("one", Some("10.1/A"), "y", "B"), p("Three", None, "z", "B")];
        let m = merge(vec![a, b], 10);
        let titles: Vec<&str> = m.iter().map(|p| p.title.as_str()).collect();
        assert_eq!(titles, vec!["One", "Three", "Two"]);
        assert_eq!(merge(vec![m.clone()], 1).len(), 1);
    }

    #[test]
    fn off_topic_papers_are_dropped() {
        let p = Paper { title: "Speculative decoding for transformers".into(), ..Default::default() };
        assert!(on_topic(&p, "speculative decoding speedups"));
        assert!(!on_topic(&p, "intermittent fasting weight loss"));
        assert!(on_topic(&p, "what does research say"));
    }

    /// Live check: `BYTE_TEST_WEB=1 cargo test live_papers -- --ignored --nocapture`
    #[tokio::test]
    #[ignore]
    async fn live_papers() {
        if std::env::var("BYTE_TEST_WEB").is_err() {
            return;
        }
        let net = crate::tools::fetch::web_client();
        let papers = search(&net, "intermittent fasting weight loss", 8).await.expect("search");
        for p in &papers {
            eprintln!("{} ({:?}) {} [{}] abstract {} chars", p.title, p.year, p.venue, p.from, p.abstract_text.len());
        }
        assert!(!papers.is_empty());
    }
}
