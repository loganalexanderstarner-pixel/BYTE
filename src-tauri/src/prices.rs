//! Price compare: "cheapest AirPods Pro", "price of a Dyson V15", "where to
//! buy X". BYTE searches shopping pages, reads the prices they publish as
//! schema.org Product/Offer data (or `product:price` meta tags), keeps the
//! pages that are about the product, and shows the offers sorted by price.
//! Prices are only ever read from pages, never written by the model.

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tokio_util::sync::CancellationToken;

use crate::agent::Turn;
use crate::chat::ChatEvent;
use crate::error::AppResult;
use crate::research::{self, Ctx, Emit, Gathered};
use crate::tools::{self, SourceBook};

/// One store's offer.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Offer {
    pub store: String,
    pub title: String,
    pub price: f64,
    pub currency: String,
    /// None: the page didn't say.
    pub in_stock: Option<bool>,
    /// "new", "used", "refurbished" (empty: not said).
    pub condition: String,
    pub url: String,
    /// Source number, for citations.
    pub n: u32,
}

/// The price card.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Prices {
    pub product: String,
    pub offers: Vec<Offer>,
    /// When the pages were read (RFC 3339), since prices change.
    pub checked_at: String,
}

const CUES: &[&str] = &[
    "cheapest", "lowest price", "best price", "best deal", "price of", "prices for", "price for", "how much is", "how much does",
    "how much do", "where to buy", "where can i buy", "where can i get", "compare prices", "price compare", "price comparison", "on sale",
];

/// Shopping questions about a specific thing's price.
pub fn wants_prices(question: &str) -> bool {
    let q = question.to_lowercase();
    if !CUES.iter().any(|c| q.contains(c)) {
        return false;
    }
    // "How much is 5 + 5", "how much does the earth weigh", "how much do I owe": not shopping.
    // "How much is a Nintendo Switch 2": a named product (capitals or a model number) counts too.
    let named = q.starts_with("how much is") && product_of(question).chars().any(|c| c.is_uppercase() || c.is_ascii_digit());
    let money_word = named || ["cheapest", "price", "buy", "deal", "sale", "cost", "$", "€", "£"].iter().any(|w| q.contains(w));
    let not_shopping = ["weigh", "calories", "owe", "tax", "salary", "rent", "tip", "per hour", "worth to", "does it take", "how much time", "sleep", "water"].iter().any(|w| q.contains(w))
        || crate::router::math_expression(question).is_some()
        // "A bat and a ball cost $1.10 … how much does the ball cost?": a puzzle.
        || crate::drafts::is_reasoning(question);
    money_word && !not_shopping && !product_of(question).is_empty()
}

pub fn applies(enabled: bool, web: bool, question: &str) -> bool {
    enabled && web && wants_prices(question)
}

/// The product named in the question ("cheapest AirPods Pro 3 in the US" → "AirPods Pro 3").
pub fn product_of(question: &str) -> String {
    crate::reviews::subject(question, CUES)
}

/// A number from "1,299.00", "$1 299", "1299" (None for 0 or nonsense).
pub fn price_value(v: &Value) -> Option<f64> {
    let p = match v {
        Value::Number(n) => n.as_f64(),
        Value::String(s) => {
            let cleaned: String = s.chars().filter(|c| c.is_ascii_digit() || *c == '.' || *c == ',').collect();
            // "1.299,00" (European) vs "1,299.00".
            let normal = match (cleaned.rfind(','), cleaned.rfind('.')) {
                (Some(c), Some(d)) if c > d => cleaned.replace('.', "").replace(',', "."),
                (Some(c), None) if cleaned.len() - c == 3 => cleaned.replace(',', "."),
                _ => cleaned.replace(',', ""),
            };
            normal.parse().ok()
        }
        _ => None,
    }?;
    (p > 0.0 && p < 1_000_000.0).then_some(p)
}

fn text(v: &Value) -> String {
    match v {
        Value::String(s) => s.trim().to_string(),
        Value::Object(o) => o.get("name").map(text).unwrap_or_default(),
        Value::Array(a) => a.first().map(text).unwrap_or_default(),
        _ => String::new(),
    }
}

fn availability(v: &Value) -> Option<bool> {
    let s = text(v).to_lowercase();
    if s.is_empty() {
        return None;
    }
    let yes = ["instock", "in_stock", "in stock", "limitedavailability", "onlineonly", "presale"].iter().any(|w| s.contains(w));
    Some(yes && !s.contains("outofstock"))
}

fn condition(v: &Value) -> String {
    let s = text(v).to_lowercase();
    ["refurbished", "used", "new", "damaged"].iter().find(|c| s.contains(**c)).map(|c| c.to_string()).unwrap_or_default()
}

/// Offers a page publishes: (title, price, currency, in stock, condition).
pub fn offers_on_page(html: &str) -> Vec<(String, f64, String, Option<bool>, String)> {
    let mut out = Vec::new();
    for v in tools::fetch::json_ld(html) {
        if !tools::fetch::ld_is(&v, "Product") && !tools::fetch::ld_is(&v, "ProductGroup") {
            continue;
        }
        let title = text(&v["name"]);
        let offers = match &v["offers"] {
            Value::Array(a) => a.clone(),
            o @ Value::Object(_) => vec![o.clone()],
            _ => vec![],
        };
        for o in offers {
            // AggregateOffer: the lowest price.
            let price = price_value(&o["price"]).or_else(|| price_value(&o["lowPrice"])).or_else(|| price_value(&o["priceSpecification"]["price"]));
            let Some(price) = price else { continue };
            let mut currency = text(&o["priceCurrency"]);
            if currency.is_empty() {
                currency = text(&o["priceSpecification"]["priceCurrency"]);
            }
            out.push((title.clone(), price, currency.to_uppercase(), availability(&o["availability"]), condition(&o["itemCondition"])));
            if out.len() >= 3 {
                break;
            }
        }
    }
    if out.is_empty() {
        // Open Graph / Facebook product tags.
        if let Some(p) = tools::fetch::meta_content(html, &["product:price:amount", "og:price:amount", "price"]).and_then(|s| price_value(&Value::String(s))) {
            let cur = tools::fetch::meta_content(html, &["product:price:currency", "og:price:currency", "priceCurrency"]).unwrap_or_default();
            let title = tools::fetch::meta_content(html, &["og:title"]).unwrap_or_default();
            let stock = tools::fetch::meta_content(html, &["product:availability", "og:availability"]).map(|s| availability(&Value::String(s)).unwrap_or(false));
            out.push((title, p, cur.to_uppercase(), stock, String::new()));
        }
    }
    out
}

/// Words that tell products apart ("pro", "3", "max"); not filler.
fn key_words(s: &str) -> Vec<String> {
    const SKIP: &[&str] = &["the", "a", "an", "for", "with", "and", "in", "on", "of", "new", "buy", "price", "sale", "deal", "best", "cheap", "online", "us", "uk", "store", "shop"];
    s.to_lowercase().split(|c: char| !c.is_alphanumeric()).filter(|w| !w.is_empty() && !SKIP.contains(w)).map(str::to_string).collect()
}

/// Whether an offer's title is the product asked about (most of its words appear).
pub fn matches_product(title: &str, product: &str) -> bool {
    let want = key_words(product);
    if want.is_empty() || title.trim().is_empty() {
        return true;
    }
    let have = key_words(title);
    let hits = want.iter().filter(|w| have.contains(w)).count();
    hits * 3 >= want.len() * 2
}

/// Keeps the cheapest offer per store, the one currency most offers use, sorted by price.
pub fn tidy(mut offers: Vec<Offer>) -> Vec<Offer> {
    let mut counts: std::collections::HashMap<String, usize> = std::collections::HashMap::new();
    for o in &offers {
        *counts.entry(o.currency.clone()).or_default() += 1;
    }
    let main = counts.into_iter().filter(|(c, _)| !c.is_empty()).max_by_key(|(_, k)| *k).map(|(c, _)| c);
    if let Some(cur) = &main {
        offers.retain(|o| o.currency.is_empty() || &o.currency == cur);
        for o in offers.iter_mut() {
            if o.currency.is_empty() {
                o.currency = cur.clone();
            }
        }
    }
    offers.sort_by(|a, b| a.price.total_cmp(&b.price));
    let mut seen = std::collections::HashSet::new();
    offers.retain(|o| seen.insert(o.store.clone()));
    offers.truncate(12);
    offers
}

/// Rules for the written answer (the card shows the table).
pub const PRICE_RULES: &str = "The user sees the offers above as a table sorted by price, with links. In your answer: \
say where it's cheapest and for how much, point out anything to watch for (used or refurbished, out of stock, a \
different model or bundle, shipping or membership prices) and cite the stores as [n]. Only use prices from the offers \
and notes; never guess a price. Mention that prices change and were checked just now. Keep it short.";

pub async fn run(turn: &Turn<'_>, question: &str, used_tokens: usize, cancel: &CancellationToken, send: Emit<'_>) -> AppResult<Option<(SourceBook, String)>> {
    let product = product_of(question);
    if product.is_empty() {
        return Ok(None);
    }
    let c = Ctx { turn, cancel, send };
    let mut g = Gathered::default();
    let queries = vec![format!("{product} price"), format!("buy {product}"), format!("{product} deal")];
    let lists = research::run_searches(&c, &mut g, &queries, "p").await?;
    let want = research::scale_pages(8, turn.depth);
    let candidates = research::interleave(&lists, &[], want * 2);
    let pages = research::read_html_pages(&c, &mut g, &candidates, want, "p").await?;

    c.call("byte_pfind", "find_prices", json!({ "product": product }))?;
    let mut offers = Vec::new();
    for (n, url, html) in &pages {
        let store = tools::host_of(url);
        for (title, price, currency, in_stock, cond) in offers_on_page(html) {
            if matches_product(&title, &product) {
                offers.push(Offer { store: store.clone(), title: if title.is_empty() { product.clone() } else { title }, price, currency, in_stock, condition: cond, url: url.clone(), n: *n });
            }
        }
    }
    let offers = tidy(offers);
    c.result("byte_pfind", !offers.is_empty(), if offers.is_empty() { "No prices published on those pages".to_string() } else { format!("{} prices", offers.len()) })?;
    let prices = Prices { product: product.clone(), offers, checked_at: chrono::Local::now().to_rfc3339() };
    if !prices.offers.is_empty() {
        send(ChatEvent::Prices(prices.clone()))?;
    }

    let budget = research::notes_budget(turn, used_tokens, 0.3);
    let (picks, _) = c.cancellable(research::rank_texts(turn, &g.texts, question, &format!("{product} price"), budget, 2)).await?;
    let notes = research::format_notes(&g.book, &picks);
    let table = if prices.offers.is_empty() {
        "NO PRICES WERE FOUND: no store page BYTE read published a price. Do not state any price, and do not say any store is \
cheapest; you don't know. Say that BYTE couldn't read current prices, and suggest where to check (the stores in the notes, or \
the maker's own store)."
            .to_string()
    } else {
        prices.offers.iter().map(|o| format!("- [{}] {}: {:.2} {}{}{}", o.n, o.store, o.price, o.currency, o.in_stock.map(|s| if s { ", in stock" } else { ", out of stock" }).unwrap_or(""), if o.condition.is_empty() || o.condition == "new" { String::new() } else { format!(", {}", o.condition) })).collect::<Vec<_>>().join("\n")
    };
    let rules = if prices.offers.is_empty() { "Cite the notes as [n]. Keep it short." } else { PRICE_RULES };
    Ok(Some((g.book, format!("Price check for: {product}\n\nOffers found:\n{table}\n\nNotes:\n{notes}\n\n{rules}"))))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn which_questions_are_about_prices() {
        for yes in ["What's the cheapest place to buy AirPods Pro 3?", "price of a Dyson V15 Detect", "Where can I buy a Steam Deck OLED?", "best deal on a 65 inch LG C4 TV", "how much is a Nintendo Switch 2 right now"] {
            assert!(wants_prices(yes), "{yes}");
        }
        for no in ["how much does the earth weigh", "how much is 12 * 15", "how much do I tip in Italy", "what is a price index", "cheapest", "how much sleep do I need", "A bat and a ball cost $1.10 in total. The bat costs $1.00 more than the ball. How much does the ball cost?"] {
            assert!(!wants_prices(no), "{no}");
        }
        assert_eq!(product_of("What's the cheapest place to buy AirPods Pro 3?"), "AirPods Pro 3");
        assert_eq!(product_of("best deal on a 65 inch LG C4 TV"), "65 inch LG C4 TV");
    }

    #[test]
    fn prices_are_read_from_product_data() {
        let html = include_str!("../tests/fixtures/product_offer.html");
        let offers = offers_on_page(html);
        assert_eq!(offers.len(), 1);
        let (title, price, cur, stock, cond) = &offers[0];
        assert_eq!(title, "Apple AirPods Pro 3");
        assert_eq!(*price, 229.99);
        assert_eq!(cur, "USD");
        assert_eq!(*stock, Some(true));
        assert_eq!(cond, "new");
        // Aggregate offers, @graph, and meta tags.
        let agg = r#"<script type="application/ld+json">{"@graph":[{"@type":"WebPage"},{"@type":["Product"],"name":"Steam Deck OLED 512GB","offers":{"@type":"AggregateOffer","lowPrice":"549.00","priceCurrency":"usd","availability":"https://schema.org/OutOfStock"}}]}</script>"#;
        assert_eq!(offers_on_page(agg), vec![("Steam Deck OLED 512GB".to_string(), 549.0, "USD".to_string(), Some(false), String::new())]);
        let meta = r#"<meta property="og:title" content="Dyson V15"><meta property="product:price:amount" content="1.299,00"><meta property="product:price:currency" content="EUR">"#;
        assert_eq!(offers_on_page(meta)[0].1, 1299.0);
        assert!(offers_on_page("<p>no data</p>").is_empty());
    }

    #[test]
    fn numbers_and_matching() {
        assert_eq!(price_value(&json!("$1,299.00")), Some(1299.0));
        assert_eq!(price_value(&json!("19,99")), Some(19.99));
        assert_eq!(price_value(&json!(0)), None);
        assert_eq!(price_value(&json!("free")), None);
        assert!(matches_product("Apple AirPods Pro 3 Wireless Earbuds", "AirPods Pro 3"));
        assert!(!matches_product("AirPods 4", "AirPods Pro 3"));
        assert!(matches_product("", "AirPods Pro 3"));
    }

    #[test]
    fn offers_are_tidied() {
        let o = |store: &str, price: f64, cur: &str| Offer { store: store.into(), title: "x".into(), price, currency: cur.into(), in_stock: None, condition: String::new(), url: String::new(), n: 1 };
        let t = tidy(vec![o("a.com", 30.0, "USD"), o("b.com", 20.0, "USD"), o("a.com", 25.0, "USD"), o("c.co.uk", 10.0, "GBP"), o("d.com", 40.0, "")]);
        let got: Vec<(String, f64)> = t.iter().map(|x| (x.store.clone(), x.price)).collect();
        assert_eq!(got, vec![("b.com".to_string(), 20.0), ("a.com".to_string(), 25.0), ("d.com".to_string(), 40.0)]);
        assert_eq!(t[2].currency, "USD");
    }
}
