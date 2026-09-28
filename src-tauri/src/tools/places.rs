//! Places nearby from OpenStreetMap (no key, no account): "coffee near me",
//! "pharmacy open now in Shadyside", "museums in Lisbon". The place is found
//! with the weather module's geocoder (or the user's town from Settings for
//! "near me"; BYTE never guesses where the user is), then the Overpass API
//! lists matching places around it with their address, opening hours,
//! website and distance.

use std::time::Duration;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::weather::{self, Place};
use crate::error::{AppError, AppResult};

/// Overpass servers, tried in order (the main one is often busy).
const OVERPASS: &[&str] = &[
    "https://overpass-api.de/api/interpreter",
    "https://maps.mail.ru/osm/tools/overpass/api/interpreter",
    "https://overpass.kumi.systems/api/interpreter",
];

/// Most places listed.
pub const MAX_SPOTS: usize = 12;

/// A kind of place and the OpenStreetMap tags that mean it.
pub struct Category {
    pub label: &'static str,
    /// Words people use for it (singular and plural).
    pub words: &'static [&'static str],
    /// Overpass tag filters; a place matching any of them counts.
    pub filters: &'static [&'static str],
}

pub const CATEGORIES: &[Category] = &[
    Category { label: "coffee shops", words: &["coffee", "cafe", "cafes", "café", "cafés", "coffee shop", "coffee shops", "espresso"], filters: &[r#"["amenity"="cafe"]"#] },
    Category { label: "restaurants", words: &["restaurant", "restaurants", "food", "dinner", "lunch", "places to eat", "somewhere to eat", "eat"], filters: &[r#"["amenity"="restaurant"]"#] },
    Category { label: "pizza places", words: &["pizza", "pizzeria"], filters: &[r#"["amenity"~"restaurant|fast_food"]["cuisine"~"pizza"]"#] },
    Category { label: "sushi places", words: &["sushi"], filters: &[r#"["amenity"="restaurant"]["cuisine"~"sushi|japanese"]"#] },
    Category { label: "fast food", words: &["fast food", "burger", "burgers"], filters: &[r#"["amenity"="fast_food"]"#] },
    Category { label: "bars", words: &["bar", "bars", "pub", "pubs", "brewery", "breweries", "drinks"], filters: &[r#"["amenity"~"^(bar|pub|biergarten)$"]"#, r#"["craft"="brewery"]"#] },
    Category { label: "bakeries", words: &["bakery", "bakeries", "bread", "pastries"], filters: &[r#"["shop"="bakery"]"#] },
    Category { label: "pharmacies", words: &["pharmacy", "pharmacies", "drugstore", "drug store", "chemist"], filters: &[r#"["amenity"="pharmacy"]"#] },
    Category { label: "grocery stores", words: &["grocery", "groceries", "supermarket", "supermarkets", "grocery store"], filters: &[r#"["shop"~"^(supermarket|grocery|greengrocer)$"]"#] },
    Category { label: "gas stations", words: &["gas", "gas station", "gas stations", "petrol", "fuel"], filters: &[r#"["amenity"="fuel"]"#] },
    Category { label: "EV chargers", words: &["ev charger", "ev chargers", "charging station", "charging stations", "charger", "tesla supercharger"], filters: &[r#"["amenity"="charging_station"]"#] },
    Category { label: "ATMs", words: &["atm", "atms", "cash machine"], filters: &[r#"["amenity"="atm"]"#, r#"["atm"="yes"]"#] },
    Category { label: "banks", words: &["bank", "banks"], filters: &[r#"["amenity"="bank"]"#] },
    Category { label: "hospitals", words: &["hospital", "hospitals", "emergency room", "er"], filters: &[r#"["amenity"="hospital"]"#] },
    Category { label: "urgent care and clinics", words: &["urgent care", "clinic", "clinics", "doctor", "doctors"], filters: &[r#"["amenity"~"^(clinic|doctors)$"]"#] },
    Category { label: "dentists", words: &["dentist", "dentists"], filters: &[r#"["amenity"="dentist"]"#] },
    Category { label: "vets", words: &["vet", "vets", "veterinarian"], filters: &[r#"["amenity"="veterinary"]"#] },
    Category { label: "parks", words: &["park", "parks", "playground", "playgrounds"], filters: &[r#"["leisure"~"^(park|playground)$"]"#] },
    Category { label: "gyms", words: &["gym", "gyms", "fitness"], filters: &[r#"["leisure"="fitness_centre"]"#] },
    Category { label: "libraries", words: &["library", "libraries"], filters: &[r#"["amenity"="library"]"#] },
    Category { label: "post offices", words: &["post office", "post offices"], filters: &[r#"["amenity"="post_office"]"#] },
    Category { label: "hotels", words: &["hotel", "hotels", "motel", "hostel", "places to stay", "stay"], filters: &[r#"["tourism"~"^(hotel|motel|hostel|guest_house)$"]"#] },
    Category { label: "museums", words: &["museum", "museums", "gallery", "galleries"], filters: &[r#"["tourism"~"^(museum|gallery)$"]"#] },
    Category { label: "sights", words: &["attraction", "attractions", "sights", "things to see", "landmarks", "viewpoint", "viewpoints"], filters: &[r#"["tourism"~"^(attraction|viewpoint|museum)$"]"#, r#"["historic"~"^(monument|castle|memorial)$"]"#] },
    Category { label: "hardware stores", words: &["hardware store", "hardware stores"], filters: &[r#"["shop"~"^(hardware|doityourself)$"]"#] },
    Category { label: "bookstores", words: &["bookstore", "bookstores", "book shop", "books"], filters: &[r#"["shop"="books"]"#] },
    Category { label: "laundromats", words: &["laundromat", "laundromats", "laundry"], filters: &[r#"["shop"="laundry"]"#] },
    Category { label: "parking", words: &["parking", "parking garage", "parking lot"], filters: &[r#"["amenity"="parking"]"#] },
    Category { label: "car repair shops", words: &["mechanic", "mechanics", "car repair", "auto repair"], filters: &[r#"["shop"="car_repair"]"#] },
];

/// The category a request names ("good coffee near me" → coffee shops).
/// Longer phrases win over single words ("coffee shop" over "shop").
pub fn category_for(what: &str) -> Option<&'static Category> {
    let w = format!(" {} ", what.to_lowercase().replace(|c: char| !c.is_alphanumeric() && c != 'é', " "));
    CATEGORIES
        .iter()
        .flat_map(|c| c.words.iter().map(move |word| (c, *word)))
        .filter(|(_, word)| w.contains(&format!(" {word} ")))
        .max_by_key(|(_, word)| word.len())
        .map(|(c, _)| c)
}

/// The Overpass query for `filters` within `radius` metres of a point.
pub fn overpass_query(filters: &[String], lat: f64, lon: f64, radius: u32) -> String {
    let parts: String = filters.iter().map(|f| format!("nwr{f}(around:{radius},{lat:.5},{lon:.5});")).collect();
    format!("[out:json][timeout:20];({parts});out center tags 60;")
}

/// Filters for a request: the category's, or a name search for anything else
/// ("Trader Joe's near me").
pub fn filters_for(what: &str) -> Vec<String> {
    if let Some(c) = category_for(what) {
        return c.filters.iter().map(|f| f.to_string()).collect();
    }
    let name: String = what.chars().filter(|c| c.is_alphanumeric() || *c == ' ' || *c == '\'' || *c == '&').collect::<String>().trim().to_string();
    // Overpass regex: escape nothing risky (only letters, digits, spaces, ' and & remain).
    vec![format!(r#"["name"~"{}",i]"#, name.replace('\'', "."))]
}

/// A place found nearby.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Spot {
    pub name: String,
    /// What it is ("cafe", "pharmacy"), from its main tag.
    pub kind: String,
    pub address: String,
    pub lat: f64,
    pub lon: f64,
    pub distance_m: u32,
    /// Raw OpenStreetMap opening hours ("Mo-Fr 07:00-15:00").
    pub hours: String,
    /// Open right now by those hours (None: unknown).
    pub open_now: Option<bool>,
    pub website: String,
    pub phone: String,
    pub cuisine: String,
    /// The place on openstreetmap.org (the citation link).
    pub osm_url: String,
}

/// Great-circle distance in metres.
pub fn distance_m(lat1: f64, lon1: f64, lat2: f64, lon2: f64) -> f64 {
    let r = 6_371_000.0;
    let (p1, p2) = (lat1.to_radians(), lat2.to_radians());
    let dp = (lat2 - lat1).to_radians();
    let dl = (lon2 - lon1).to_radians();
    let a = (dp / 2.0).sin().powi(2) + p1.cos() * p2.cos() * (dl / 2.0).sin().powi(2);
    2.0 * r * a.sqrt().asin()
}

const KIND_KEYS: &[&str] = &["amenity", "shop", "tourism", "leisure", "historic", "craft"];

/// Places from an Overpass reply, named ones only, closest first.
pub fn parse_overpass(v: &Value, lat: f64, lon: f64, now: chrono::NaiveDateTime) -> Vec<Spot> {
    let mut out: Vec<Spot> = v["elements"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|e| {
            let tags = &e["tags"];
            let name = tags["name"].as_str()?.trim().to_string();
            if name.is_empty() {
                return None;
            }
            let (plat, plon) = match (e["lat"].as_f64(), e["lon"].as_f64()) {
                (Some(a), Some(b)) => (a, b),
                _ => (e["center"]["lat"].as_f64()?, e["center"]["lon"].as_f64()?),
            };
            let tag = |k: &str| tags[k].as_str().unwrap_or("").trim().to_string();
            let street = [tag("addr:housenumber"), tag("addr:street")].iter().filter(|s| !s.is_empty()).cloned().collect::<Vec<_>>().join(" ");
            let address = [street, tag("addr:city")].into_iter().filter(|s| !s.is_empty()).collect::<Vec<_>>().join(", ");
            let hours = tag("opening_hours");
            let kind = KIND_KEYS.iter().map(|k| tag(k)).find(|v| !v.is_empty() && v != "yes").unwrap_or_default().replace('_', " ");
            let website = [tag("website"), tag("contact:website")].into_iter().find(|s| !s.is_empty()).unwrap_or_default();
            let phone = [tag("phone"), tag("contact:phone")].into_iter().find(|s| !s.is_empty()).unwrap_or_default();
            Some(Spot {
                open_now: open_at(&hours, now),
                distance_m: distance_m(lat, lon, plat, plon).round() as u32,
                osm_url: format!("https://www.openstreetmap.org/{}/{}", e["type"].as_str().unwrap_or("node"), e["id"].as_u64().unwrap_or(0)),
                name,
                kind,
                address,
                lat: plat,
                lon: plon,
                hours,
                website,
                phone,
                cuisine: tag("cuisine").replace(['_', ';'], " "),
            })
        })
        .collect();
    out.sort_by_key(|s| s.distance_m);
    // The same place mapped twice (a node and a building).
    let mut seen = std::collections::HashSet::new();
    out.retain(|s| seen.insert(s.name.to_lowercase()) || s.distance_m > 300);
    out
}

const DAYS: [&str; 7] = ["Mo", "Tu", "We", "Th", "Fr", "Sa", "Su"];

fn day_index(d: &str) -> Option<usize> {
    DAYS.iter().position(|x| x.eq_ignore_ascii_case(d.trim()))
}

/// Days a selector covers: "Mo-Fr", "Sa,Su", "Mo-Fr,Su", "Tu".
fn days_of(sel: &str) -> Option<Vec<usize>> {
    let mut out = Vec::new();
    for part in sel.split(',') {
        match part.split_once('-') {
            Some((a, b)) => {
                let (a, b) = (day_index(a)?, day_index(b)?);
                let mut d = a;
                loop {
                    out.push(d);
                    if d == b {
                        break;
                    }
                    d = (d + 1) % 7;
                }
            }
            None => out.push(day_index(part)?),
        }
    }
    Some(out)
}

fn minutes(t: &str) -> Option<u32> {
    let (h, m) = t.trim().split_once(':')?;
    let (h, m): (u32, u32) = (h.parse().ok()?, m.parse().ok()?);
    (h <= 24 && m < 60).then_some(h * 60 + m)
}

/// Whether a place is open at `now` by its OpenStreetMap opening hours.
/// Handles the common forms ("24/7", "Mo-Fr 08:00-18:00; Sa 10:00-14:00",
/// "Mo-Su 11:00-14:00,17:00-22:00", "Su off", times past midnight); None for
/// anything else (public holidays, months, "sunrise"…).
pub fn open_at(hours: &str, now: chrono::NaiveDateTime) -> Option<bool> {
    use chrono::{Datelike, Timelike};
    let h = hours.trim();
    if h.is_empty() {
        return None;
    }
    if h == "24/7" {
        return Some(true);
    }
    let today = now.weekday().num_days_from_monday() as usize;
    let yesterday = (today + 6) % 7;
    let t = now.hour() * 60 + now.minute();
    // Later rules override earlier ones for the days they name.
    let mut today_rule: Option<Vec<(u32, u32)>> = None;
    let mut spill: Option<Vec<(u32, u32)>> = None;
    for rule in h.split(';').map(str::trim).filter(|r| !r.is_empty()) {
        let (sel, times) = match rule.split_once(' ') {
            Some((a, b)) if a.chars().next().is_some_and(|c| c.is_ascii_uppercase()) => (a, b.trim()),
            // "08:00-18:00" alone: every day.
            _ if rule.chars().next().is_some_and(|c| c.is_ascii_digit()) => ("Mo-Su", rule),
            _ => return None,
        };
        let days = days_of(sel)?;
        let ranges: Vec<(u32, u32)> = if times.eq_ignore_ascii_case("off") || times.eq_ignore_ascii_case("closed") {
            vec![]
        } else {
            times
                .split(',')
                .map(|r| {
                    let (a, b) = r.split_once('-')?;
                    Some((minutes(a)?, minutes(b.trim_end_matches('+'))?))
                })
                .collect::<Option<Vec<_>>>()?
        };
        if days.contains(&today) {
            today_rule = Some(ranges.clone());
        }
        if days.contains(&yesterday) {
            spill = Some(ranges);
        }
    }
    let open_today = today_rule.as_ref().is_some_and(|r| r.iter().any(|&(a, b)| if b > a { t >= a && t < b } else { t >= a }));
    // Yesterday's hours running past midnight ("Fr 18:00-02:00" on Saturday 01:00).
    let open_from_yesterday = spill.as_ref().is_some_and(|r| r.iter().any(|&(a, b)| b <= a && t < b));
    if today_rule.is_none() && spill.is_none() {
        return Some(false);
    }
    Some(open_today || open_from_yesterday)
}

async fn overpass(net: &reqwest::Client, query: &str) -> AppResult<Value> {
    let mut last = String::new();
    for url in OVERPASS {
        match net.post(*url).form(&[("data", query)]).timeout(Duration::from_secs(30)).send().await {
            Ok(r) if r.status().is_success() => {
                let text = r.text().await?;
                match serde_json::from_str::<Value>(&text) {
                    Ok(v) => return Ok(v),
                    Err(_) => last = format!("{url} was busy"),
                }
            }
            Ok(r) => last = format!("{url} answered {}", r.status()),
            Err(e) => last = e.to_string(),
        }
    }
    Err(AppError::msg(format!("the map service is busy ({last}); try again in a minute")))
}

/// Places matching `what` around `near`: the closest, within 1.5 km, or
/// 5 km when there are fewer than three that close.
pub async fn find(net: &reqwest::Client, what: &str, near: &str) -> AppResult<(Place, Vec<Spot>)> {
    let place = weather::geocode(net, near).await?;
    let spots = find_at(net, what, &place).await?;
    Ok((place, spots))
}

pub async fn find_at(net: &reqwest::Client, what: &str, place: &Place) -> AppResult<Vec<Spot>> {
    let filters = filters_for(what);
    let now = chrono::Local::now().naive_local();
    let mut spots = Vec::new();
    for radius in [1500, 5000] {
        let v = overpass(net, &overpass_query(&filters, place.lat, place.lon, radius)).await?;
        spots = parse_overpass(&v, place.lat, place.lon, now);
        if spots.len() >= 3 {
            break;
        }
    }
    spots.truncate(MAX_SPOTS);
    Ok(spots)
}

/// Distance in words ("350 m", "1.2 km", or miles in the US).
pub fn distance_text(m: u32, imperial: bool) -> String {
    if imperial {
        let mi = m as f64 / 1609.34;
        if mi < 0.1 {
            format!("{} ft", (m as f64 * 3.281 / 10.0).round() * 10.0)
        } else {
            format!("{mi:.1} mi")
        }
    } else if m < 1000 {
        format!("{} m", (m as f64 / 10.0).round() * 10.0)
    } else {
        format!("{:.1} km", m as f64 / 1000.0)
    }
}

/// The places as the model sees them, numbered as sources.
pub fn spots_text(spots: &[Spot], numbers: &[u32], what: &str, place: &Place) -> String {
    let imperial = weather::uses_imperial(place);
    let mut out = format!("{} near {} (from OpenStreetMap, closest first):\n", category_for(what).map(|c| c.label).unwrap_or(what), place.label());
    for (s, n) in spots.iter().zip(numbers) {
        let open = match s.open_now {
            Some(true) => " · open now",
            Some(false) => " · closed now",
            None => "",
        };
        let mut line = format!("\n[{n}] {} ({}{}) · {}{open}", s.name, s.kind, if s.cuisine.is_empty() { String::new() } else { format!(", {}", s.cuisine) }, distance_text(s.distance_m, imperial));
        if !s.address.is_empty() {
            line.push_str(&format!("\n{}", s.address));
        }
        if !s.hours.is_empty() {
            line.push_str(&format!("\nHours: {}", s.hours));
        }
        if !s.website.is_empty() {
            line.push_str(&format!("\n{}", s.website));
        }
        out.push_str(&line);
        out.push('\n');
    }
    out.push_str("\nList the best few for the user with distance and whether they're open, citing [n]. Say that hours come from OpenStreetMap and may be out of date. Don't invent ratings or places not listed.");
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::NaiveDate;

    fn at(day: u32, h: u32, m: u32) -> chrono::NaiveDateTime {
        // 2026-09-28 is a Monday.
        (NaiveDate::from_ymd_opt(2026, 9, 28).unwrap() + chrono::Days::new(day as u64)).and_hms_opt(h, m, 0).unwrap()
    }

    #[test]
    fn categories_are_found() {
        assert_eq!(category_for("good coffee near me").unwrap().label, "coffee shops");
        assert_eq!(category_for("a pharmacy open now").unwrap().label, "pharmacies");
        assert_eq!(category_for("Where can I charge my car, any EV chargers?").unwrap().label, "EV chargers");
        assert!(category_for("Trader Joe's").is_none());
        assert_eq!(filters_for("Trader Joe's"), vec![r#"["name"~"Trader Joe.s",i]"#.to_string()]);
        let q = overpass_query(&filters_for("coffee"), 40.44, -79.99, 1500);
        assert_eq!(q, r#"[out:json][timeout:20];(nwr["amenity"="cafe"](around:1500,40.44000,-79.99000););out center tags 60;"#);
    }

    #[test]
    fn opening_hours() {
        let h = "Mo-Fr 07:00-15:00; Sa,Su 07:00-15:00";
        assert_eq!(open_at(h, at(0, 8, 0)), Some(true));
        assert_eq!(open_at(h, at(0, 15, 0)), Some(false));
        assert_eq!(open_at("Mo-Fr 07:00-14:00", at(5, 9, 0)), Some(false)); // Saturday
        assert_eq!(open_at("24/7", at(3, 3, 0)), Some(true));
        assert_eq!(open_at("Mo-Su 11:00-14:00,17:00-22:00", at(2, 15, 0)), Some(false));
        assert_eq!(open_at("Mo-Su 11:00-14:00,17:00-22:00", at(2, 18, 0)), Some(true));
        // Later rules override: closed Sundays.
        assert_eq!(open_at("Mo-Su 09:00-17:00; Su off", at(6, 10, 0)), Some(false));
        // Past midnight: Friday's bar hours still open early Saturday.
        assert_eq!(open_at("Fr 18:00-02:00", at(5, 1, 0)), Some(true));
        assert_eq!(open_at("Fr 18:00-02:00", at(4, 23, 0)), Some(true));
        assert_eq!(open_at("08:00-18:00", at(1, 9, 0)), Some(true));
        assert_eq!(open_at("Mo-Fr 08:00-18:00; PH off", at(0, 9, 0)), None);
        assert_eq!(open_at("sunrise-sunset", at(0, 9, 0)), None);
        assert_eq!(open_at("", at(0, 9, 0)), None);
    }

    #[test]
    fn reads_overpass_replies() {
        let v: Value = serde_json::from_str(&std::fs::read_to_string(format!("{}/tests/fixtures/overpass_cafes.json", env!("CARGO_MANIFEST_DIR"))).unwrap()).unwrap();
        let spots = parse_overpass(&v, 40.4406, -79.9959, at(0, 10, 0));
        assert_eq!(spots.len(), 6);
        assert!(spots.windows(2).all(|w| w[0].distance_m <= w[1].distance_m));
        let rock = spots.iter().find(|s| s.name == "Rock'n Joe").unwrap();
        assert_eq!(rock.address, "524 Penn Avenue, Pittsburgh");
        assert_eq!(rock.open_now, Some(true));
        assert_eq!(rock.kind, "cafe");
        assert!(rock.osm_url.starts_with("https://www.openstreetmap.org/node/"));
        assert_eq!(spots.iter().find(|s| s.name == "Fernando's Cafe").unwrap().open_now, None);
        // Two Starbucks far apart are both kept.
        assert_eq!(spots.iter().filter(|s| s.name == "Starbucks").count(), 2);
    }

    #[test]
    fn distances_read_naturally() {
        assert!((distance_m(40.4406, -79.9959, 40.4406, -79.9841) - 1000.0).abs() < 20.0);
        assert_eq!(distance_text(350, false), "350 m");
        assert_eq!(distance_text(1234, false), "1.2 km");
        assert_eq!(distance_text(2414, true), "1.5 mi");
        assert_eq!(distance_text(60, true), "200 ft");
    }

    /// Live: `BYTE_TEST_WEB=1 cargo test live_places -- --ignored --nocapture`
    #[tokio::test]
    #[ignore]
    async fn live_places() {
        if std::env::var("BYTE_TEST_WEB").is_err() {
            return;
        }
        let net = crate::tools::fetch::web_client();
        let (place, spots) = find(&net, "coffee", "Pittsburgh, PA").await.expect("places");
        eprintln!("{}: {spots:#?}", place.label());
        assert!(!spots.is_empty());
    }
}
