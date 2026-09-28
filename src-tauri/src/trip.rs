//! Trip planner (Phase 6, v0.6.2): "plan 3 days in Lisbon in May for $1,500".
//!
//! 1. Reads the trip from the question (JSON): destination, days, dates or
//!    month, budget, travellers, interests.
//! 2. Weather: the forecast when the trip starts within a week; otherwise last
//!    year's weather for the same dates (Open-Meteo archive), labelled typical.
//! 3. Researches things to do, where to stay, food, getting around and costs
//!    (plus each interest), reads pages, and lists top sights from the map.
//! 4. Asks the model for a structured plan (days → items with times, places,
//!    costs and sources; a budget; a packing list; tips), sent to the UI as a
//!    `Trip` card (Save as PDF, Add to Calendar), then a short cited summary.

use chrono::{Datelike, NaiveDate};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tokio_util::sync::CancellationToken;

use crate::agent::Turn;
use crate::chat::{self, ChatEvent};
use crate::error::AppResult;
use crate::research::{self, Ctx, Emit, Gathered};
use crate::settings::Mode;
use crate::tools::{places, weather, SourceBook};

/// What the user asked for.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct TripAsk {
    pub destination: String,
    pub days: u32,
    /// First day, when a date was given (None when only a month is known).
    pub start: Option<NaiveDate>,
    /// "May" when only a month was given.
    pub month: Option<String>,
    pub budget: Option<f64>,
    pub currency: String,
    pub travelers: u32,
    pub interests: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TripItem {
    /// "09:00", or "Morning" / "Afternoon" / "Evening".
    pub time: String,
    pub title: String,
    #[serde(default)]
    pub place: String,
    #[serde(default)]
    pub note: String,
    /// Estimated cost for the group, in the trip's currency.
    #[serde(default)]
    pub cost: Option<f64>,
    #[serde(default)]
    pub sources: Vec<u32>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TripDay {
    pub title: String,
    /// "2027-05-10" when the dates are known.
    #[serde(default)]
    pub date: Option<String>,
    pub items: Vec<TripItem>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BudgetLine {
    pub category: String,
    pub amount: f64,
}

/// The plan shown as a card in the chat.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TripPlan {
    pub destination: String,
    pub currency: String,
    /// The user's budget, if they gave one.
    pub budget: Option<f64>,
    pub travelers: u32,
    /// "May" when only a month is known.
    pub month: Option<String>,
    pub days: Vec<TripDay>,
    pub costs: Vec<BudgetLine>,
    pub packing: Vec<String>,
    pub tips: Vec<String>,
    /// A line about the weather ("Typical for May 10–12 (last year): 18–24 °C, 1 rainy day").
    pub weather: String,
}

pub const TRIP_RULES: &str = "The user sees the day-by-day plan above as a card (with the budget and packing list), so \
don't repeat it in full. Write a short companion: a one-line **TL;DR:** blockquote, then `##` sections on where to stay \
(which area and why), getting around, and what to book ahead, citing sources like [3]. Mention the weather and anything \
seasonal. Say that prices and opening hours are estimates to check before booking. Use only source numbers listed \
above and don't add a list of sources. End with one line exactly like `**Confidence:** Likely — <why, in one short \
sentence>` using Verified, Likely or Unsure.";

/// Whether this question gets the trip planner.
pub fn applies(web: bool, question: &str) -> bool {
    web && crate::router::wants_trip(question)
}

fn ask_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "destination": { "type": "string" },
            "days": { "type": "integer", "minimum": 1, "maximum": 14 },
            "startDate": { "type": "string", "description": "YYYY-MM-DD, or empty" },
            "month": { "type": "string", "description": "Month name if only a month is given, or empty" },
            "budget": { "type": "number", "description": "Total budget, 0 if not given" },
            "currency": { "type": "string", "description": "ISO code like USD or EUR" },
            "travelers": { "type": "integer", "minimum": 1, "maximum": 12 },
            "interests": { "type": "array", "maxItems": 5, "items": { "type": "string" } }
        },
        "required": ["destination", "days", "startDate", "month", "budget", "currency", "travelers", "interests"]
    })
}

/// "3 days", "a weekend", "a week" in the question.
pub fn days_in(question: &str) -> Option<u32> {
    let q = question.to_lowercase();
    let words: Vec<&str> = q.split(|c: char| !c.is_alphanumeric() && c != '-').filter(|w| !w.is_empty()).collect();
    for (i, w) in words.iter().enumerate() {
        let unit = words.get(i + 1).copied().unwrap_or("");
        let n = match *w {
            "one" | "a" => Some(1),
            "two" => Some(2),
            "three" => Some(3),
            "four" => Some(4),
            "five" => Some(5),
            "six" => Some(6),
            "seven" => Some(7),
            "ten" => Some(10),
            x => x.trim_end_matches("-day").parse::<u32>().ok(),
        };
        if let Some(n) = n {
            if w.ends_with("-day") || unit.starts_with("day") || unit == "nights" || unit == "night" {
                return Some(n.clamp(1, 14));
            }
            if unit == "week" || unit == "weeks" {
                return Some((n * 7).clamp(1, 14));
            }
        }
    }
    if q.contains("weekend") {
        return Some(2);
    }
    None
}

/// The trip from the model's reply, with the question's own numbers winning.
pub fn parse_ask(reply: &str, question: &str, today: NaiveDate) -> Option<TripAsk> {
    let v = research::lenient_json(reply);
    let destination: String = v["destination"].as_str().unwrap_or("").trim().chars().take(80).collect();
    if destination.is_empty() {
        return None;
    }
    let days = days_in(question).or_else(|| v["days"].as_u64().map(|d| d as u32)).unwrap_or(3).clamp(1, 14);
    // Only future dates make sense; a past one is a misread.
    let start = v["startDate"].as_str().and_then(|s| NaiveDate::parse_from_str(s.trim(), "%Y-%m-%d").ok()).filter(|d| *d >= today);
    let month = v["month"].as_str().map(|m| m.trim().to_string()).filter(|m| !m.is_empty() && start.is_none());
    let budget = v["budget"].as_f64().filter(|b| *b > 0.0);
    let currency = v["currency"].as_str().map(|c| c.trim().to_uppercase()).filter(|c| c.len() == 3).unwrap_or_else(|| if question.contains('€') { "EUR".into() } else if question.contains('£') { "GBP".into() } else { "USD".into() });
    let travelers = v["travelers"].as_u64().unwrap_or(1).clamp(1, 12) as u32;
    let interests = v["interests"].as_array().into_iter().flatten().filter_map(Value::as_str).map(|s| s.trim().chars().take(40).collect::<String>()).filter(|s| !s.is_empty()).take(5).collect();
    Some(TripAsk { destination, days, start, month, budget, currency, travelers, interests })
}

/// Month number for "May", "sept", "December".
fn month_number(name: &str) -> Option<u32> {
    let n = name.trim().to_lowercase();
    ["jan", "feb", "mar", "apr", "may", "jun", "jul", "aug", "sep", "oct", "nov", "dec"].iter().position(|m| n.starts_with(m)).map(|i| i as u32 + 1)
}

/// The dates to look up typical weather for: last year's same dates (or the
/// middle of the month).
pub fn weather_window(ask: &TripAsk, today: NaiveDate) -> Option<(NaiveDate, NaiveDate)> {
    let first = match (ask.start, &ask.month) {
        (Some(d), _) => d,
        (None, Some(m)) => {
            let mo = month_number(m)?;
            let year = if mo >= today.month() { today.year() } else { today.year() + 1 };
            NaiveDate::from_ymd_opt(year, mo, 10)?
        }
        _ => return None,
    };
    let last_year = NaiveDate::from_ymd_opt(first.year() - 1, first.month(), first.day().min(28))?;
    // The archive lags a few days; a trip within a year of today still has last year's data.
    let end = last_year + chrono::Days::new(ask.days.max(1) as u64 - 1);
    (end < today).then_some((last_year, end))
}

/// A one-line weather summary from an Open-Meteo daily reply.
pub fn weather_line(v: &Value, label: &str, imperial: bool) -> Option<String> {
    let d = &v["daily"];
    let nums = |k: &str| -> Vec<f64> { d[k].as_array().into_iter().flatten().filter_map(Value::as_f64).collect() };
    let (hi, lo, rain) = (nums("temperature_2m_max"), nums("temperature_2m_min"), nums("precipitation_sum"));
    if hi.is_empty() || lo.is_empty() {
        return None;
    }
    let conv = |c: f64| if imperial { c * 9.0 / 5.0 + 32.0 } else { c };
    let unit = if imperial { "°F" } else { "°C" };
    let lo_min = lo.iter().cloned().fold(f64::MAX, f64::min);
    let hi_max = hi.iter().cloned().fold(f64::MIN, f64::max);
    let wet = rain.iter().filter(|r| **r >= 1.0).count();
    Some(format!("{label}: lows around {:.0}{unit}, highs up to {:.0}{unit}, {wet} of {} days with rain.", conv(lo_min), conv(hi_max), hi.len()))
}

async fn typical_weather(net: &reqwest::Client, place: &weather::Place, from: NaiveDate, to: NaiveDate) -> Option<String> {
    let v: Value = net
        .get("https://archive-api.open-meteo.com/v1/archive")
        .query(&[
            ("latitude", place.lat.to_string()),
            ("longitude", place.lon.to_string()),
            ("start_date", from.to_string()),
            ("end_date", to.to_string()),
            ("daily", "temperature_2m_max,temperature_2m_min,precipitation_sum".into()),
            ("timezone", "auto".into()),
        ])
        .timeout(std::time::Duration::from_secs(15))
        .send()
        .await
        .ok()?
        .json()
        .await
        .ok()?;
    let label = format!("Typical weather ({} to {}, last year)", from.format("%b %-d"), to.format("%b %-d"));
    weather_line(&v, &label, weather::uses_imperial(place))
}

fn plan_schema(days: u32) -> Value {
    json!({
        "type": "object",
        "properties": {
            "days": { "type": "array", "minItems": days, "maxItems": days, "items": {
                "type": "object",
                "properties": {
                    "title": { "type": "string" },
                    "items": { "type": "array", "minItems": 2, "maxItems": 6, "items": {
                        "type": "object",
                        "properties": {
                            "time": { "type": "string" },
                            "title": { "type": "string" },
                            "place": { "type": "string" },
                            "note": { "type": "string" },
                            "cost": { "type": "number" },
                            "sources": { "type": "array", "items": { "type": "integer" } }
                        },
                        "required": ["time", "title", "place", "note", "cost", "sources"]
                    }}
                },
                "required": ["title", "items"]
            }},
            "costs": { "type": "array", "maxItems": 8, "items": {
                "type": "object",
                "properties": { "category": { "type": "string" }, "amount": { "type": "number" } },
                "required": ["category", "amount"]
            }},
            "packing": { "type": "array", "maxItems": 15, "items": { "type": "string" } },
            "tips": { "type": "array", "maxItems": 6, "items": { "type": "string" } }
        },
        "required": ["days", "costs", "packing", "tips"]
    })
}

/// The plan from the model's reply, cleaned: at most `ask.days` days (extra
/// dropped), items need a title, costs non-negative, sources must exist.
pub fn parse_plan(reply: &str, ask: &TripAsk, weather: &str, valid: &[u32]) -> Option<TripPlan> {
    let v = research::lenient_json(reply);
    let text = |x: &Value, max: usize| -> String { x.as_str().unwrap_or("").trim().chars().take(max).collect() };
    let mut days = Vec::new();
    for (i, d) in v["days"].as_array().into_iter().flatten().take(ask.days as usize).enumerate() {
        let items: Vec<TripItem> = d["items"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|it| {
                let title = text(&it["title"], 120);
                (!title.is_empty()).then(|| TripItem {
                    time: text(&it["time"], 20),
                    title,
                    place: text(&it["place"], 100),
                    note: text(&it["note"], 240),
                    cost: it["cost"].as_f64().filter(|c| *c > 0.0 && c.is_finite()),
                    sources: it["sources"].as_array().into_iter().flatten().filter_map(Value::as_u64).map(|n| n as u32).filter(|n| valid.contains(n)).collect(),
                })
            })
            .take(6)
            .collect();
        if items.is_empty() {
            continue;
        }
        let date = ask.start.map(|s| (s + chrono::Days::new(i as u64)).to_string());
        let title = match text(&d["title"], 80) {
            t if t.is_empty() => format!("Day {}", i + 1),
            t => t,
        };
        days.push(TripDay { title, date, items });
    }
    if days.is_empty() {
        return None;
    }
    let costs = v["costs"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|c| Some(BudgetLine { category: text(&c["category"], 40), amount: c["amount"].as_f64().filter(|a| *a >= 0.0 && a.is_finite())? }))
        .filter(|c| !c.category.is_empty())
        .collect();
    let list = |k: &str, n: usize| -> Vec<String> { v[k].as_array().into_iter().flatten().map(|x| text(x, 160)).filter(|s| !s.is_empty()).take(n).collect() };
    Some(TripPlan {
        destination: ask.destination.clone(),
        currency: ask.currency.clone(),
        budget: ask.budget,
        travelers: ask.travelers,
        month: ask.month.clone(),
        days,
        costs,
        packing: list("packing", 15),
        tips: list("tips", 6),
        weather: weather.to_string(),
    })
}

/// Pages read, by mode.
fn pages(mode: Mode) -> usize {
    match mode {
        Mode::Fast => 4,
        Mode::Auto => 8,
        Mode::Deep | Mode::Extended => 14,
    }
}

/// Runs the planner. `Ok(None)`: the trip couldn't be read (research instead).
pub async fn run(turn: &Turn<'_>, question: &str, used_tokens: usize, cancel: &CancellationToken, send: Emit<'_>) -> AppResult<Option<(SourceBook, String)>> {
    let c = Ctx { turn, cancel, send };
    let mut g = Gathered::default();
    let today = chrono::Local::now().date_naive();

    c.call("byte_tplan", "plan_trip", json!({}))?;
    let user = format!(
        "Today is {today}. Trip request:\n{question}\n\nWhat is the destination (city and country), how many days, the start date \
if given (YYYY-MM-DD, else empty), the month if only a month is given (else empty), the total budget (0 if none), its \
currency, how many travellers, and their interests?"
    );
    let reply = c.cancellable(chat::complete_json(turn.http, turn.ep, "You read trip requests. Reply only with JSON.", &user, ask_schema(), 300)).await?.unwrap_or_default();
    let Some(ask) = parse_ask(&reply, question, today) else {
        c.result("byte_tplan", false, "Couldn't tell where the trip is")?;
        return Ok(None);
    };
    c.result("byte_tplan", true, format!("{} days in {}", ask.days, ask.destination))?;

    // Weather and the map.
    c.call("byte_tweather", "trip_weather", json!({ "place": ask.destination }))?;
    let place = c.cancellable(weather::geocode(turn.net, &ask.destination)).await?.ok();
    let mut weather_text = String::new();
    if let Some(p) = &place {
        let soon = ask.start.is_some_and(|s| s <= today + chrono::Days::new(6));
        weather_text = if soon {
            c.cancellable(weather::forecast_at(turn.net, p.clone())).await?.map(|(_, t)| t).unwrap_or_default()
        } else if let Some((from, to)) = weather_window(&ask, today) {
            c.cancellable(typical_weather(turn.net, p, from, to)).await?.unwrap_or_default()
        } else {
            String::new()
        };
    }
    c.result("byte_tweather", !weather_text.is_empty(), if weather_text.is_empty() { "No weather for those dates".to_string() } else { weather_text.lines().next().unwrap_or("").chars().take(80).collect() })?;

    // Research.
    let d = &ask.destination;
    let mut queries = vec![
        format!("best things to do in {d}"),
        format!("where to stay in {d} neighborhoods"),
        format!("{d} food what to eat"),
        format!("getting around {d} public transport"),
        format!("{d} travel costs budget"),
    ];
    queries.extend(ask.interests.iter().take(2).map(|i| format!("{d} {i}")));
    let lists = research::run_searches(&c, &mut g, &queries, "t").await?;
    let candidates = research::interleave(&lists, &[], research::scale_pages(pages(turn.mode), turn.depth) * 2);
    research::read_pages(&c, &mut g, &candidates, research::scale_pages(pages(turn.mode), turn.depth), "t").await?;

    // Top sights from the map, as sources the plan can cite.
    let mut sights_text = String::new();
    if let Some(p) = &place {
        c.call("byte_tsights", crate::tools::FIND_PLACES, json!({ "what": "sights", "near": p.label() }))?;
        match c.cancellable(places::find_at(turn.net, "sights", p)).await? {
            Ok(spots) => {
                let numbers: Vec<u32> = spots.iter().map(|s| g.book.mark_read(&s.osm_url, &s.name)).collect();
                sights_text = places::spots_text(&spots, &numbers, "sights", p).lines().filter(|l| !l.starts_with("List the best")).collect::<Vec<_>>().join("\n");
                c.result("byte_tsights", true, format!("{} sights on the map", spots.len()))?;
            }
            Err(e) => c.result("byte_tsights", false, e.to_string())?,
        }
    }
    if !g.book.sources.is_empty() {
        send(ChatEvent::Sources { sources: g.book.sources.clone() })?;
    }

    c.call("byte_trank", "rank_passages", json!({}))?;
    let budget = research::notes_budget(turn, used_tokens, 0.4);
    let (picks, by_meaning) = c.cancellable(research::rank_texts(turn, &g.texts, question, &queries.join(" "), budget, 3)).await?;
    let notes = research::format_notes(&g.book, &picks);
    c.result("byte_trank", !picks.is_empty(), research::rank_summary(picks.len(), &notes, by_meaning))?;

    // The plan itself.
    c.call("byte_titinerary", "write_itinerary", json!({}))?;
    let when = match (ask.start, &ask.month) {
        (Some(s), _) => format!("starting {}", s.format("%A, %B %-d, %Y")),
        (None, Some(m)) => format!("in {m}"),
        _ => "dates not set".into(),
    };
    let money = match ask.budget {
        Some(b) => format!("Total budget: {b:.0} {} for {} traveller(s).", ask.currency, ask.travelers),
        None => format!("No budget given; estimate typical mid-range costs in {} for {} traveller(s).", ask.currency, ask.travelers),
    };
    let user = format!(
        "Trip: {} days in {d}, {when}. {money} Interests: {}.\n{weather_text}\n\nResearch notes:\n{}\n\n{sights_text}\n\nPlan the trip day by day: \
2 to 5 items a day with a time (like 09:00, or Morning/Afternoon/Evening), a short title, the place, a one-line note, an \
estimated cost for the group ({}; 0 if free) and the source numbers it comes from. Group nearby places on the same day \
and keep a realistic pace. Then a budget split (lodging, food, transport, activities…), a packing list suited to the \
weather and activities, and up to 6 practical tips.",
        ask.days,
        if ask.interests.is_empty() { "general sightseeing".to_string() } else { ask.interests.join(", ") },
        notes.chars().take(budget.min(10_000)).collect::<String>(),
        ask.currency
    );
    let reply = c.cancellable(chat::complete_json(turn.http, turn.ep, "You are a careful travel planner. Reply only with JSON.", &user, plan_schema(ask.days), 700 + ask.days * 450)).await?.unwrap_or_default();
    let valid: Vec<u32> = g.book.sources.iter().map(|s| s.n).collect();
    let plan = parse_plan(&reply, &ask, &weather_text, &valid);
    c.result("byte_titinerary", plan.is_some(), match &plan { Some(p) => format!("{} days, {} stops", p.days.len(), p.days.iter().map(|d| d.items.len()).sum::<usize>()), None => "The plan couldn't be read".into() })?;
    if let Some(p) = &plan {
        send(ChatEvent::Trip(p.clone()))?;
    }

    let shown = match &plan {
        Some(p) => format!("Plan shown to the user:\n{}\n", serde_json::to_string(&p.days).unwrap_or_default().chars().take(4000).collect::<String>()),
        None => "The day-by-day plan couldn't be made; write it yourself as `##` sections per day.\n".into(),
    };
    let content = format!("Trip notes for: {question}\n{weather_text}\n\n{notes}\n{sights_text}\n\n{shown}\n{TRIP_RULES}");
    Ok(Some((g.book, content)))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn today() -> NaiveDate {
        NaiveDate::from_ymd_opt(2026, 9, 28).unwrap()
    }

    #[test]
    fn days_are_read_from_the_question() {
        assert_eq!(days_in("Plan 3 days in Lisbon"), Some(3));
        assert_eq!(days_in("a 5-day trip to Rome"), Some(5));
        assert_eq!(days_in("a weekend in Chicago"), Some(2));
        assert_eq!(days_in("two weeks in Japan"), Some(14));
        assert_eq!(days_in("one week in Paris"), Some(7));
        assert_eq!(days_in("trip to Paris"), None);
    }

    #[test]
    fn the_ask_is_read_leniently() {
        let a = parse_ask(r#"{"destination":"Lisbon, Portugal","days":5,"startDate":"","month":"May","budget":1500,"currency":"usd","travelers":2,"interests":["food","history",""]}"#, "Plan 3 days in Lisbon in May for $1500", today()).unwrap();
        assert_eq!(a.days, 3, "the question's number wins");
        assert_eq!(a.month.as_deref(), Some("May"));
        assert_eq!(a.budget, Some(1500.0));
        assert_eq!(a.currency, "USD");
        assert_eq!(a.interests, vec!["food", "history"]);
        // A date in the past is ignored; € means EUR.
        let b = parse_ask(r#"{"destination":"Rome","days":2,"startDate":"2020-01-01","budget":0,"currency":"x"}"#, "2 days in Rome for €800", today()).unwrap();
        assert_eq!(b.start, None);
        assert_eq!(b.currency, "EUR");
        assert_eq!(b.budget, None);
        assert!(parse_ask("no", "x", today()).is_none());
    }

    #[test]
    fn weather_uses_last_years_dates() {
        let ask = TripAsk { destination: "Lisbon".into(), days: 3, month: Some("May".into()), ..Default::default() };
        let (from, to) = weather_window(&ask, today()).unwrap();
        assert_eq!((from.to_string(), to.to_string()), ("2026-05-10".into(), "2026-05-12".into()));
        let ask = TripAsk { destination: "Lisbon".into(), days: 2, start: NaiveDate::from_ymd_opt(2026, 12, 30), ..Default::default() };
        assert_eq!(weather_window(&ask, today()).unwrap().0.to_string(), "2025-12-28");
        let v = json!({"daily": {"temperature_2m_max": [22.0, 24.6, 19.0], "temperature_2m_min": [14.0, 15.0, 13.2], "precipitation_sum": [0.0, 3.1, 0.2]}});
        assert_eq!(weather_line(&v, "Typical", false).unwrap(), "Typical: lows around 13°C, highs up to 25°C, 1 of 3 days with rain.");
        assert_eq!(weather_line(&v, "T", true).unwrap(), "T: lows around 56°F, highs up to 76°F, 1 of 3 days with rain.");
    }

    #[test]
    fn the_plan_is_cleaned() {
        let ask = TripAsk { destination: "Lisbon".into(), days: 2, start: NaiveDate::from_ymd_opt(2026, 10, 3), currency: "EUR".into(), travelers: 2, ..Default::default() };
        let reply = r#"{"days":[
            {"title":"Alfama and the castle","items":[{"time":"09:00","title":"Castelo de São Jorge","place":"Alfama","note":"Go early","cost":30,"sources":[2,99]},{"time":"","title":"","place":"","note":"","cost":0,"sources":[]}]},
            {"title":"","items":[{"time":"Evening","title":"Fado dinner","place":"Bairro Alto","note":"Book ahead","cost":-5,"sources":[]}]},
            {"title":"Extra","items":[{"time":"x","title":"y","place":"","note":"","cost":1,"sources":[]}]}
        ],"costs":[{"category":"Lodging","amount":240},{"category":"","amount":5},{"category":"Food","amount":-1}],"packing":["Walking shoes",""],"tips":["Buy a Viva Viagem card"]}"#;
        let p = parse_plan(reply, &ask, "Typical: mild", &[1, 2]).unwrap();
        assert_eq!(p.days.len(), 2);
        assert_eq!(p.days[0].items.len(), 1);
        assert_eq!(p.days[0].items[0].sources, vec![2]);
        assert_eq!(p.days[0].date.as_deref(), Some("2026-10-03"));
        assert_eq!(p.days[1].title, "Day 2");
        assert_eq!(p.days[1].date.as_deref(), Some("2026-10-04"));
        assert_eq!(p.days[1].items[0].cost, None);
        assert_eq!(p.costs, vec![BudgetLine { category: "Lodging".into(), amount: 240.0 }]);
        assert_eq!(p.packing, vec!["Walking shoes"]);
        let v = serde_json::to_value(&p).unwrap();
        assert_eq!(v["days"][0]["items"][0]["time"], "09:00");
        assert!(parse_plan(r#"{"days":[]}"#, &ask, "", &[]).is_none());
    }

    /// Real engine + real internet. Needs BYTE_TEST_LLAMA_SERVER, BYTE_TEST_MODEL and BYTE_TEST_WEB=1.
    #[tokio::test]
    #[ignore]
    async fn e2e_trip() {
        if std::env::var("BYTE_TEST_WEB").is_err() {
            return;
        }
        let Ok(model) = std::env::var("BYTE_TEST_MODEL") else { return };
        let Some((_server, mut ep)) = crate::chat::e2e_support::start_server_with(&model, &["-c".into(), "16384".into()], None).await else { return };
        ep.context = 16384;
        let dir = tempfile::tempdir().unwrap();
        let log = crate::tools::ActionLog::new(dir.path().join("a.jsonl"));
        let http = chat::local_client();
        let net = crate::tools::fetch::web_client();
        let q = "Plan 2 days in Pittsburgh in October for 2 people, we like food and museums";
        let history = vec![chat::ChatMessage::new("user", q)];
        let system = crate::prompt::system_prompt(chrono::Local::now(), Mode::Auto, true, None);
        let plan = crate::router::plan_turn(Mode::Auto, crate::settings::ThinkingPref::Off, q);
        let (ch, seen) = crate::chat::e2e_support::collecting_channel();
        let turn = Turn { http: &http, cloud: None, net: &net, ep: &ep, system: &system, history: &history, plan, mode: Mode::Auto, web: true, memory: false, log: &log, files: None, app: None, task: None, home: None, depth: 0, web_always: false, kitchen: false };
        crate::agent::run(turn, CancellationToken::new(), &ch).await.unwrap();
        let ev = seen.lock().unwrap().clone();
        for e in ev.iter().filter(|e| e["kind"] == "toolResult" || e["kind"] == "trip") {
            eprintln!("{}", e.to_string().chars().take(600).collect::<String>());
        }
        let content: String = ev.iter().filter(|e| e["kind"] == "content").filter_map(|e| e["delta"].as_str()).collect();
        eprintln!("{content}");
        let trip = ev.iter().find(|e| e["kind"] == "trip").expect("no trip card");
        assert_eq!(trip["days"].as_array().unwrap().len(), 2);
    }
}
