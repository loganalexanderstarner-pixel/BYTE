//! Weather from Open-Meteo (free, no key): the place is found with its
//! geocoder, then the current conditions and a 7-day forecast are fetched.
//! Far more reliable than reading weather websites, which are mostly
//! JavaScript apps that return no text.

use std::time::Duration;

use serde_json::Value;

use crate::error::{AppError, AppResult};

/// A place Open-Meteo knows.
#[derive(Debug, Clone, PartialEq)]
pub struct Place {
    pub name: String,
    pub region: String,
    pub country: String,
    pub country_code: String,
    pub lat: f64,
    pub lon: f64,
}

impl Place {
    pub fn label(&self) -> String {
        [self.name.as_str(), self.region.as_str(), self.country.as_str()].iter().filter(|s| !s.is_empty()).cloned().collect::<Vec<_>>().join(", ")
    }
}

/// Countries that use °F and mph.
fn imperial(country_code: &str) -> bool {
    matches!(country_code, "US" | "LR" | "MM" | "PR" | "GU" | "VI" | "AS" | "MP")
}

/// US state abbreviations, so "Pittsburgh, PA" picks Pennsylvania's Pittsburgh.
const US_STATES: &[(&str, &str)] = &[
    ("al", "alabama"), ("ak", "alaska"), ("az", "arizona"), ("ar", "arkansas"), ("ca", "california"), ("co", "colorado"),
    ("ct", "connecticut"), ("de", "delaware"), ("fl", "florida"), ("ga", "georgia"), ("hi", "hawaii"), ("id", "idaho"),
    ("il", "illinois"), ("in", "indiana"), ("ia", "iowa"), ("ks", "kansas"), ("ky", "kentucky"), ("la", "louisiana"),
    ("me", "maine"), ("md", "maryland"), ("ma", "massachusetts"), ("mi", "michigan"), ("mn", "minnesota"),
    ("ms", "mississippi"), ("mo", "missouri"), ("mt", "montana"), ("ne", "nebraska"), ("nv", "nevada"),
    ("nh", "new hampshire"), ("nj", "new jersey"), ("nm", "new mexico"), ("ny", "new york"), ("nc", "north carolina"),
    ("nd", "north dakota"), ("oh", "ohio"), ("ok", "oklahoma"), ("or", "oregon"), ("pa", "pennsylvania"),
    ("ri", "rhode island"), ("sc", "south carolina"), ("sd", "south dakota"), ("tn", "tennessee"), ("tx", "texas"),
    ("ut", "utah"), ("vt", "vermont"), ("va", "virginia"), ("wa", "washington"), ("wv", "west virginia"),
    ("wi", "wisconsin"), ("wy", "wyoming"), ("dc", "district of columbia"),
];

/// Picks the best geocoder match: the one whose region or country matches
/// what came after a comma ("Portland, Maine"), else the first (most populous).
pub fn pick_place(v: &Value, qualifier: &str) -> Option<Place> {
    let q = qualifier.trim().to_lowercase();
    let q = US_STATES.iter().find(|(abbr, _)| *abbr == q).map(|(_, name)| name.to_string()).unwrap_or(q);
    let places: Vec<Place> = v["results"]
        .as_array()?
        .iter()
        .filter_map(|r| {
            Some(Place {
                name: r["name"].as_str()?.to_string(),
                region: r["admin1"].as_str().unwrap_or("").to_string(),
                country: r["country"].as_str().unwrap_or("").to_string(),
                country_code: r["country_code"].as_str().unwrap_or("").to_string(),
                lat: r["latitude"].as_f64()?,
                lon: r["longitude"].as_f64()?,
            })
        })
        .collect();
    if !q.is_empty() {
        if let Some(p) = places.iter().find(|p| {
            p.region.to_lowercase() == q || p.country.to_lowercase() == q || p.country_code.to_lowercase() == q || (q == "usa" && p.country_code == "US") || (q == "uk" && p.country_code == "GB")
        }) {
            return Some(p.clone());
        }
    }
    places.into_iter().next()
}

/// WMO weather codes in words.
pub fn describe(code: i64) -> &'static str {
    match code {
        0 => "clear",
        1 => "mostly clear",
        2 => "partly cloudy",
        3 => "cloudy",
        45 | 48 => "fog",
        51 | 53 | 55 => "drizzle",
        56 | 57 => "freezing drizzle",
        61 => "light rain",
        63 => "rain",
        65 => "heavy rain",
        66 | 67 => "freezing rain",
        71 => "light snow",
        73 => "snow",
        75 => "heavy snow",
        77 => "snow grains",
        80 => "light showers",
        81 => "showers",
        82 => "heavy showers",
        85 | 86 => "snow showers",
        95 => "thunderstorms",
        96 | 99 => "thunderstorms with hail",
        _ => "mixed conditions",
    }
}

/// Turns Open-Meteo's forecast into lines the model can quote.
pub fn format_forecast(place: &Place, v: &Value) -> Option<String> {
    let (t, w) = if imperial(&place.country_code) { ("°F", "mph") } else { ("°C", "km/h") };
    let cur = &v["current"];
    let mut out = format!("Weather for {} (Open-Meteo forecast, local time {}):\n", place.label(), cur["time"].as_str().unwrap_or("now").replace('T', " "));
    if let Some(temp) = cur["temperature_2m"].as_f64() {
        out.push_str(&format!(
            "Now: {:.0}{t} (feels like {:.0}{t}), {}, wind {:.0} {w}\n",
            temp,
            cur["apparent_temperature"].as_f64().unwrap_or(temp),
            describe(cur["weather_code"].as_i64().unwrap_or(-1)),
            cur["wind_speed_10m"].as_f64().unwrap_or(0.0)
        ));
    }
    let d = &v["daily"];
    let days = d["time"].as_array()?;
    for (i, day) in days.iter().enumerate() {
        let date = chrono::NaiveDate::parse_from_str(day.as_str()?, "%Y-%m-%d").ok()?;
        let num = |k: &str| d[k][i].as_f64();
        out.push_str(&format!(
            "{}: {}, high {:.0}{t}, low {:.0}{t}, {:.0}% chance of rain{}\n",
            date.format("%A %b %-d"),
            describe(d["weather_code"][i].as_i64().unwrap_or(-1)),
            num("temperature_2m_max").unwrap_or(f64::NAN),
            num("temperature_2m_min").unwrap_or(f64::NAN),
            num("precipitation_probability_max").unwrap_or(0.0),
            num("wind_speed_10m_max").map(|x| format!(", wind up to {x:.0} {w}")).unwrap_or_default()
        ));
    }
    Some(out)
}

/// Finds a place by name ("Pittsburgh, PA", "Lisbon", "Shadyside, Pittsburgh").
pub async fn geocode(client: &reqwest::Client, place: &str) -> AppResult<Place> {
    let (name, qualifier) = place.split_once(',').unwrap_or((place, ""));
    let name = name.trim();
    if name.is_empty() {
        return Err(AppError::msg("no place given"));
    }
    let geo: Value = client
        .get("https://geocoding-api.open-meteo.com/v1/search")
        .query(&[("name", name), ("count", "10"), ("language", "en"), ("format", "json")])
        .timeout(Duration::from_secs(10))
        .send()
        .await?
        .json()
        .await?;
    pick_place(&geo, qualifier).ok_or_else(|| AppError::msg(format!("couldn't find a place called {name}")))
}

/// Uses °F and mph for this place.
pub fn uses_imperial(place: &Place) -> bool {
    imperial(&place.country_code)
}

/// Current conditions and a 7-day forecast for a place name ("Pittsburgh, PA").
pub async fn forecast(client: &reqwest::Client, place: &str) -> AppResult<(Place, String)> {
    let place = geocode(client, place).await?;
    forecast_at(client, place).await
}

/// Current conditions and a 7-day forecast for a place already found.
pub async fn forecast_at(client: &reqwest::Client, place: Place) -> AppResult<(Place, String)> {
    let (temp, wind) = if imperial(&place.country_code) { ("fahrenheit", "mph") } else { ("celsius", "kmh") };
    let v: Value = client
        .get("https://api.open-meteo.com/v1/forecast")
        .query(&[
            ("latitude", place.lat.to_string()),
            ("longitude", place.lon.to_string()),
            ("current", "temperature_2m,apparent_temperature,weather_code,wind_speed_10m".into()),
            ("daily", "weather_code,temperature_2m_max,temperature_2m_min,precipitation_probability_max,wind_speed_10m_max".into()),
            ("timezone", "auto".into()),
            ("forecast_days", "7".into()),
            ("temperature_unit", temp.into()),
            ("wind_speed_unit", wind.into()),
        ])
        .timeout(Duration::from_secs(10))
        .send()
        .await?
        .json()
        .await?;
    let text = format_forecast(&place, &v).ok_or_else(|| AppError::msg("the weather service sent an unexpected reply"))?;
    Ok((place, text))
}

/// A link people can open for the same forecast.
pub fn source_url(place: &Place) -> String {
    if place.country_code == "US" {
        format!("https://forecast.weather.gov/MapClick.php?lat={:.4}&lon={:.4}", place.lat, place.lon)
    } else {
        format!("https://open-meteo.com/en/docs#latitude={:.4}&longitude={:.4}", place.lat, place.lon)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn geo() -> Value {
        json!({ "results": [
            { "name": "Portland", "latitude": 45.52, "longitude": -122.68, "country_code": "US", "country": "United States", "admin1": "Oregon" },
            { "name": "Portland", "latitude": 43.66, "longitude": -70.26, "country_code": "US", "country": "United States", "admin1": "Maine" }
        ]})
    }

    #[test]
    fn picks_the_place_named_after_the_comma() {
        assert_eq!(pick_place(&geo(), "").unwrap().region, "Oregon");
        assert_eq!(pick_place(&geo(), " ME").unwrap().region, "Maine");
        assert_eq!(pick_place(&geo(), "maine").unwrap().region, "Maine");
        assert!(pick_place(&json!({}), "").is_none());
    }

    #[test]
    fn formats_a_forecast_in_local_units() {
        let place = pick_place(&geo(), "").unwrap();
        let v = json!({
            "current": { "time": "2026-09-27T22:15", "temperature_2m": 60.5, "apparent_temperature": 60.0, "weather_code": 3, "wind_speed_10m": 4.5 },
            "daily": { "time": ["2026-09-26", "2026-09-27"], "weather_code": [61, 0], "temperature_2m_max": [72.9, 65.0], "temperature_2m_min": [51.7, 58.0], "precipitation_probability_max": [80, 5], "wind_speed_10m_max": [12.0, 6.0] }
        });
        let text = format_forecast(&place, &v).unwrap();
        assert!(text.contains("Portland, Oregon, United States"), "{text}");
        assert!(text.contains("Now: 60°F (feels like 60°F), cloudy, wind 4 mph") || text.contains("Now: 61°F"), "{text}");
        assert!(text.contains("Saturday Sep 26: light rain, high 73°F, low 52°F, 80% chance of rain"), "{text}");
        assert!(source_url(&place).starts_with("https://forecast.weather.gov/"));
    }

    #[test]
    fn metric_outside_the_us() {
        let place = Place { name: "Paris".into(), region: "Île-de-France".into(), country: "France".into(), country_code: "FR".into(), lat: 48.85, lon: 2.35 };
        let v = json!({ "current": {}, "daily": { "time": ["2026-09-26"], "weather_code": [0], "temperature_2m_max": [21.0], "temperature_2m_min": [12.0], "precipitation_probability_max": [0] } });
        let text = format_forecast(&place, &v).unwrap();
        assert!(text.contains("high 21°C, low 12°C"), "{text}");
    }

    /// Live: `BYTE_TEST_WEB=1 cargo test live_weather -- --ignored --nocapture`
    #[tokio::test]
    #[ignore]
    async fn live_weather() {
        if std::env::var("BYTE_TEST_WEB").is_err() {
            return;
        }
        let (place, text) = forecast(&crate::tools::fetch::web_client(), "Pittsburgh, PA").await.unwrap();
        eprintln!("{text}");
        assert_eq!(place.region, "Pennsylvania");
        assert!(text.contains("high"));
    }
}
