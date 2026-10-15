//! Notion, with the user's own internal-integration secret (made at
//! notion.so/my-integrations and shared with the pages BYTE may use). The
//! secret lives only in the macOS Keychain. BYTE searches pages, reads them
//! as sources, and creates pages after the user's OK.

use serde_json::{json, Value};

use crate::error::{AppError, AppResult};

pub const API: &str = "https://api.notion.com";
const VERSION: &str = "2022-06-28";

pub struct Notion<'a> {
    pub http: &'a reqwest::Client,
    pub base: &'a str,
    pub token: &'a str,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Page {
    pub id: String,
    pub title: String,
    pub url: String,
}

fn plain(rich: &Value) -> String {
    rich.as_array().map(|a| a.iter().filter_map(|t| t["plain_text"].as_str()).collect::<String>()).unwrap_or_default()
}

/// A page's title (whichever property is the title).
fn title_of(page: &Value) -> String {
    page["properties"]
        .as_object()
        .and_then(|props| props.values().find(|p| p["type"] == "title").map(|p| plain(&p["title"])))
        .filter(|t| !t.trim().is_empty())
        .unwrap_or_else(|| "Untitled".into())
}

/// A page id from a Notion link or a bare id ("…-2f8a…" with or without dashes).
pub fn page_id(text: &str) -> Option<String> {
    let t = text.trim().split(['?', '#']).next().unwrap_or("");
    let last = t.rsplit(['/', '-']).next().unwrap_or(t);
    let hex: String = if last.len() == 32 { last.to_string() } else { t.chars().filter(|c| c.is_ascii_hexdigit()).collect::<String>() };
    let hex = if hex.len() >= 32 { hex[hex.len() - 32..].to_string() } else { return None };
    hex.chars().all(|c| c.is_ascii_hexdigit()).then(|| format!("{}-{}-{}-{}-{}", &hex[..8], &hex[8..12], &hex[12..16], &hex[16..20], &hex[20..])).map(|s| s.to_lowercase())
}

impl Notion<'_> {
    fn req(&self, method: reqwest::Method, path: &str) -> reqwest::RequestBuilder {
        self.http
            .request(method, format!("{}{path}", self.base))
            .bearer_auth(self.token)
            .header("Notion-Version", VERSION)
            .timeout(std::time::Duration::from_secs(20))
    }

    async fn json(&self, rb: reqwest::RequestBuilder) -> AppResult<Value> {
        let resp = rb.send().await.map_err(|e| AppError::msg(format!("Couldn't reach Notion: {e}")))?;
        let status = resp.status();
        let v: Value = resp.json().await.unwrap_or(Value::Null);
        if status == reqwest::StatusCode::UNAUTHORIZED {
            return Err(AppError::msg("Notion didn't accept the secret. Make a new one at notion.so/my-integrations and connect again."));
        }
        if !status.is_success() {
            let msg = v["message"].as_str().unwrap_or("something went wrong");
            return Err(AppError::msg(format!("Notion said: {msg}")));
        }
        Ok(v)
    }

    /// Checks the secret; returns the integration's name.
    pub async fn me(&self) -> AppResult<String> {
        let v = self.json(self.req(reqwest::Method::GET, "/v1/users/me")).await?;
        Ok(v["name"].as_str().unwrap_or("Notion").to_string())
    }

    /// Pages matching words (only pages shared with the integration).
    pub async fn search(&self, query: &str, limit: usize) -> AppResult<Vec<Page>> {
        let body = json!({ "query": query, "page_size": limit.min(20), "filter": { "property": "object", "value": "page" } });
        let v = self.json(self.req(reqwest::Method::POST, "/v1/search").json(&body)).await?;
        Ok(v["results"]
            .as_array()
            .map(|a| a.iter().map(|p| Page { id: p["id"].as_str().unwrap_or("").to_string(), title: title_of(p), url: p["url"].as_str().unwrap_or("").to_string() }).filter(|p| !p.id.is_empty()).collect())
            .unwrap_or_default())
    }

    /// A page's text (its first 100 blocks), up to `max` characters.
    pub async fn text(&self, id: &str, max: usize) -> AppResult<String> {
        let v = self.json(self.req(reqwest::Method::GET, &format!("/v1/blocks/{id}/children?page_size=100"))).await?;
        let mut out = String::new();
        for b in v["results"].as_array().into_iter().flatten() {
            let kind = b["type"].as_str().unwrap_or("");
            let text = plain(&b[kind]["rich_text"]);
            if text.trim().is_empty() {
                continue;
            }
            let line = match kind {
                "heading_1" => format!("# {text}"),
                "heading_2" => format!("## {text}"),
                "heading_3" => format!("### {text}"),
                "bulleted_list_item" => format!("- {text}"),
                "numbered_list_item" => format!("1. {text}"),
                "to_do" => format!("- [{}] {text}", if b["to_do"]["checked"] == true { "x" } else { " " }),
                "quote" | "callout" => format!("> {text}"),
                _ => text,
            };
            out.push_str(&line);
            out.push('\n');
            if out.len() >= max {
                break;
            }
        }
        Ok(out.chars().take(max).collect())
    }

    /// Creates a page under `parent` with a title and paragraphs; returns its link.
    pub async fn create(&self, parent: &str, title: &str, body: &str) -> AppResult<Page> {
        let children: Vec<Value> = body
            .split("\n\n")
            .flat_map(|p| {
                let chars: Vec<char> = p.trim().chars().collect();
                chars.chunks(1900).map(|c| c.iter().collect::<String>()).collect::<Vec<_>>()
            })
            .filter(|p| !p.trim().is_empty())
            .take(100)
            .map(|p| json!({ "object": "block", "type": "paragraph", "paragraph": { "rich_text": [{ "type": "text", "text": { "content": p } }] } }))
            .collect();
        let body = json!({
            "parent": { "page_id": parent },
            "properties": { "title": { "title": [{ "type": "text", "text": { "content": title.chars().take(200).collect::<String>() } }] } },
            "children": children,
        });
        let v = self.json(self.req(reqwest::Method::POST, "/v1/pages").json(&body)).await?;
        Ok(Page { id: v["id"].as_str().unwrap_or("").to_string(), title: title.to_string(), url: v["url"].as_str().unwrap_or("").to_string() })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use wiremock::matchers::{body_partial_json, header, method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    #[test]
    fn page_ids_come_from_links() {
        assert_eq!(page_id("https://www.notion.so/myspace/Reading-list-2f8a1b3c4d5e6f708192a3b4c5d6e7f8?pvs=4").as_deref(), Some("2f8a1b3c-4d5e-6f70-8192-a3b4c5d6e7f8"));
        assert_eq!(page_id("2f8a1b3c-4d5e-6f70-8192-a3b4c5d6e7f8").as_deref(), Some("2f8a1b3c-4d5e-6f70-8192-a3b4c5d6e7f8"));
        assert_eq!(page_id("not a page"), None);
    }

    #[tokio::test]
    async fn search_read_and_create_talk_to_notion() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/v1/search"))
            .and(header("Notion-Version", VERSION))
            .and(header("Authorization", "Bearer secret_test_x"))
            .and(body_partial_json(json!({ "query": "trip" })))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({ "results": [
                { "object": "page", "id": "p1", "url": "https://www.notion.so/Lisbon-trip-p1", "properties": { "Name": { "type": "title", "title": [{ "plain_text": "Lisbon " }, { "plain_text": "trip" }] } } }
            ] })))
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/v1/blocks/p1/children"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({ "results": [
                { "type": "heading_2", "heading_2": { "rich_text": [{ "plain_text": "Flights" }] } },
                { "type": "to_do", "to_do": { "checked": true, "rich_text": [{ "plain_text": "Book TAP flight" }] } },
                { "type": "paragraph", "paragraph": { "rich_text": [] } },
                { "type": "bulleted_list_item", "bulleted_list_item": { "rich_text": [{ "plain_text": "Hotel in Alfama" }] } }
            ] })))
            .mount(&server)
            .await;
        Mock::given(method("POST"))
            .and(path("/v1/pages"))
            .and(body_partial_json(json!({ "parent": { "page_id": "parent-1" } })))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({ "id": "new-1", "url": "https://www.notion.so/new-1" })))
            .mount(&server)
            .await;
        Mock::given(method("GET")).and(path("/v1/users/me")).respond_with(ResponseTemplate::new(401).set_body_json(json!({ "message": "API token is invalid." }))).mount(&server).await;

        let http = reqwest::Client::new();
        let base = server.uri();
        let n = Notion { http: &http, base: &base, token: "secret_test_x" };
        let pages = n.search("trip", 5).await.unwrap();
        assert_eq!(pages, vec![Page { id: "p1".into(), title: "Lisbon trip".into(), url: "https://www.notion.so/Lisbon-trip-p1".into() }]);
        assert_eq!(n.text("p1", 1000).await.unwrap(), "## Flights\n- [x] Book TAP flight\n- Hotel in Alfama\n");
        let made = n.create("parent-1", "Packing list", "Passport\n\nCharger").await.unwrap();
        assert_eq!(made.url, "https://www.notion.so/new-1");
        assert!(n.me().await.unwrap_err().to_string().contains("didn't accept the secret"));
    }
}
