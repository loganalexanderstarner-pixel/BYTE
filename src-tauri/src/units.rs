//! Kitchen units: recipes in US measures (teaspoons, tablespoons, cups,
//! ounces, pounds, °F) or metric (mL, g, °C), whichever the user chose. The
//! model is asked for the right units; this converts whatever slipped
//! through (web recipes are often metric), so the card is always consistent.

use crate::kitchen::{Ingredient, Recipe};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Unit {
    Ml,
    L,
    G,
    Kg,
    Tsp,
    Tbsp,
    Cup,
    FlOz,
    Oz,
    Lb,
}

fn unit_of(s: &str) -> Option<Unit> {
    let u = s.trim().trim_end_matches('.').to_lowercase();
    Some(match u.as_str() {
        "ml" | "milliliter" | "milliliters" | "millilitre" | "millilitres" => Unit::Ml,
        "l" | "liter" | "liters" | "litre" | "litres" => Unit::L,
        "g" | "gr" | "gram" | "grams" | "gramme" | "grammes" => Unit::G,
        "kg" | "kilogram" | "kilograms" | "kilo" | "kilos" => Unit::Kg,
        "tsp" | "teaspoon" | "teaspoons" | "t" => Unit::Tsp,
        "tbsp" | "tablespoon" | "tablespoons" | "tbs" | "tbl" => Unit::Tbsp,
        "cup" | "cups" | "c" => Unit::Cup,
        "fl oz" | "fl. oz" | "fluid ounce" | "fluid ounces" => Unit::FlOz,
        "oz" | "ounce" | "ounces" => Unit::Oz,
        "lb" | "lbs" | "pound" | "pounds" => Unit::Lb,
        _ => return None,
    })
}

/// Grams in one US cup, for ingredients usually measured by volume in US recipes.
const DENSITY: &[(&str, f64)] = &[
    ("powdered sugar", 120.0),
    ("icing sugar", 120.0),
    ("confectioners", 120.0),
    ("brown sugar", 213.0),
    ("sugar", 200.0),
    ("bread flour", 127.0),
    ("whole wheat flour", 120.0),
    ("almond flour", 96.0),
    ("flour", 120.0),
    ("cocoa", 85.0),
    ("oats", 90.0),
    ("rice", 185.0),
    ("honey", 340.0),
    ("maple syrup", 320.0),
    ("syrup", 340.0),
    ("chocolate chips", 170.0),
    ("cornstarch", 128.0),
    ("cornmeal", 138.0),
    ("breadcrumbs", 108.0),
    ("parmesan", 100.0),
    ("shredded cheese", 113.0),
    ("nuts", 120.0),
    ("walnuts", 120.0),
    ("pecans", 110.0),
    ("raisins", 150.0),
    ("peanut butter", 258.0),
    ("yogurt", 245.0),
    ("sour cream", 230.0),
];

fn density(item: &str) -> Option<f64> {
    let i = item.to_lowercase();
    DENSITY.iter().find(|(k, _)| i.contains(k)).map(|(_, d)| *d)
}

fn is_butter(item: &str) -> bool {
    let i = item.to_lowercase();
    i.contains("butter") && !i.contains("peanut") && !i.contains("almond butter") && !i.contains("buttermilk")
}

/// To the nearest 1/8 (cups, spoons) with at least 1/8.
fn eighth(x: f64) -> f64 {
    ((x * 8.0).round() / 8.0).max(0.125)
}

fn nice_metric(x: f64) -> f64 {
    if x >= 100.0 {
        (x / 5.0).round() * 5.0
    } else if x >= 10.0 {
        x.round()
    } else {
        (x * 10.0).round() / 10.0
    }
}

fn num(x: f64) -> String {
    if x.fract() == 0.0 {
        format!("{}", x as i64)
    } else {
        format!("{}", (x * 100.0).round() / 100.0)
    }
}

fn add_note(ing: &mut Ingredient, text: String) {
    if ing.note.is_empty() {
        ing.note = text;
    } else if !ing.note.contains(&text) {
        ing.note = format!("{text}; {}", ing.note);
    }
}

/// A volume in mL as the most natural US spoon or cup amount.
fn ml_to_us(ml: f64) -> (f64, &'static str) {
    if ml < 14.0 {
        (eighth(ml / 4.93), "tsp")
    } else if ml < 59.0 {
        (eighth(ml / 14.79), "tbsp")
    } else {
        (eighth(ml / 236.6), "cup")
    }
}

/// One ingredient in US measures. The metric amount it came from goes in the note.
pub fn ingredient_to_us(ing: &mut Ingredient) {
    let (Some(q), Some(u)) = (ing.qty, unit_of(&ing.unit)) else { return };
    let original = format!("{} {}", num(q), ing.unit.trim());
    match u {
        Unit::Ml | Unit::L => {
            let ml = if u == Unit::L { q * 1000.0 } else { q };
            let (v, unit) = ml_to_us(ml);
            ing.qty = Some(v);
            ing.unit = unit.into();
            add_note(ing, original);
        }
        Unit::G | Unit::Kg => {
            let g = if u == Unit::Kg { q * 1000.0 } else { q };
            if is_butter(&ing.item) {
                let tbsp = g / 14.2;
                if tbsp < 16.0 {
                    (ing.qty, ing.unit) = (Some(((tbsp * 2.0).round() / 2.0).max(0.5)), "tbsp".into());
                } else {
                    (ing.qty, ing.unit) = (Some(eighth(g / 227.0)), "cup".into());
                }
            } else if let Some(d) = density(&ing.item) {
                let cups = g / d;
                if cups < 0.25 {
                    (ing.qty, ing.unit) = (Some(eighth(cups * 16.0)), "tbsp".into());
                } else {
                    (ing.qty, ing.unit) = (Some(eighth(cups)), "cup".into());
                }
            } else if g < 454.0 {
                (ing.qty, ing.unit) = (Some(((g / 28.35) * 2.0).round().max(1.0) / 2.0), "oz".into());
            } else {
                (ing.qty, ing.unit) = (Some(((g / 453.6) * 4.0).round() / 4.0), "lb".into());
            }
            add_note(ing, original);
        }
        _ => {}
    }
}

/// One ingredient in metric (mL for liquids, g for weights and dry baking goods).
pub fn ingredient_to_metric(ing: &mut Ingredient) {
    let (Some(q), Some(u)) = (ing.qty, unit_of(&ing.unit)) else { return };
    // The exact metric amount is often already in the note ("80 ml"): use it.
    if !matches!(u, Unit::G | Unit::Kg | Unit::Ml | Unit::L) {
        if let Some((qty, unit, rest)) = metric_in_note(&ing.note) {
            (ing.qty, ing.unit, ing.note) = (Some(qty), unit, rest);
            return;
        }
    }
    let ml = match u {
        Unit::Tsp => Some(q * 5.0),
        Unit::Tbsp => Some(q * 15.0),
        Unit::Cup => Some(q * 240.0),
        Unit::FlOz => Some(q * 30.0),
        _ => None,
    };
    if let Some(ml) = ml {
        // Butter and dry baking goods are weighed in metric kitchens.
        let grams = if is_butter(&ing.item) { Some(ml / 15.0 * 14.2) } else { density(&ing.item).map(|d| ml / 240.0 * d) };
        match grams {
            Some(g) if ml >= 15.0 => (ing.qty, ing.unit) = (Some(nice_metric(g)), "g".into()),
            _ => (ing.qty, ing.unit) = (Some(nice_metric(ml)), "ml".into()),
        }
        return;
    }
    match u {
        Unit::Oz => (ing.qty, ing.unit) = (Some(nice_metric(q * 28.35)), "g".into()),
        Unit::Lb => (ing.qty, ing.unit) = (Some(nice_metric(q * 453.6)), "g".into()),
        _ => {}
    }
}

/// "80 ml", "140 g; washed" at the start of a note → (80, "ml", rest).
fn metric_in_note(note: &str) -> Option<(f64, String, String)> {
    let t = note.trim_start();
    let end = t.find(|c: char| !(c.is_ascii_digit() || c == '.'))?;
    let qty: f64 = t[..end].parse().ok().filter(|q: &f64| *q > 0.0)?;
    let rest = t[end..].trim_start();
    let word_end = rest.find(|c: char| !c.is_ascii_alphabetic()).unwrap_or(rest.len());
    let unit = rest[..word_end].to_lowercase();
    if !matches!(unit.as_str(), "g" | "kg" | "ml" | "l") {
        return None;
    }
    let after = rest[word_end..].trim_start_matches([';', ',']).trim().to_string();
    Some((qty, unit, after))
}

/// Oven and cooking temperatures in text: "Bake at 180°C" → "Bake at 350°F" (US)
/// or back (metric). Text that already gives both is left alone.
pub fn temps(text: &str, metric: bool) -> String {
    let (from, to) = if metric { ('F', 'C') } else { ('C', 'F') };
    if text.contains(&format!("°{to}")) {
        return text.to_string();
    }
    let chars: Vec<char> = text.chars().collect();
    let mut out = String::with_capacity(text.len());
    let mut i = 0;
    while i < chars.len() {
        if chars[i].is_ascii_digit() && (i == 0 || !chars[i - 1].is_ascii_digit()) {
            let start = i;
            while i < chars.len() && chars[i].is_ascii_digit() {
                i += 1;
            }
            let mut j = i;
            while j < chars.len() && chars[j] == ' ' {
                j += 1;
            }
            if j + 1 < chars.len() && chars[j] == '°' && chars[j + 1] == from {
                let v: f64 = chars[start..i].iter().collect::<String>().parse().unwrap_or(0.0);
                let c = if metric { (v - 32.0) * 5.0 / 9.0 } else { v * 9.0 / 5.0 + 32.0 };
                // Ovens in steps of 25 °F / 10 °C; the rest to 5.
                let step = if metric { if c >= 100.0 { 10.0 } else { 5.0 } } else if c >= 200.0 { 25.0 } else { 5.0 };
                let rounded = (c / step).round() * step;
                out.push_str(&format!("{}°{to}", rounded as i64));
                i = j + 2;
                continue;
            }
            out.extend(&chars[start..i]);
            continue;
        }
        out.push(chars[i]);
        i += 1;
    }
    out
}

/// The whole recipe in the chosen units.
pub fn convert_recipe(r: &mut Recipe, metric: bool) {
    for ing in r.ingredients.iter_mut() {
        if metric {
            ingredient_to_metric(ing);
        } else {
            ingredient_to_us(ing);
        }
    }
    for s in r.steps.iter_mut() {
        s.text = temps(&s.text, metric);
        s.cue = temps(&s.cue, metric);
    }
    r.tips = r.tips.iter().map(|t| temps(t, metric)).collect();
}

/// What the model is told about units.
pub fn prompt_rule(metric: bool) -> &'static str {
    if metric {
        "Use metric measures: grams for solids and baking, mL for liquids, °C for temperatures."
    } else {
        "Use US measures only: tsp, tbsp, cup, fl oz, oz and lb for amounts (butter in tablespoons or sticks), and °F for \
temperatures. Never write mL, L, g or kg in the amounts; convert metric recipes (1 cup = 240 mL, 1 tbsp = 15 mL, 1 tsp = 5 mL)."
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ing(qty: f64, unit: &str, item: &str) -> Ingredient {
        Ingredient { qty: Some(qty), unit: unit.into(), item: item.into(), ..Default::default() }
    }

    fn us(qty: f64, unit: &str, item: &str) -> (Option<f64>, String, String) {
        let mut i = ing(qty, unit, item);
        ingredient_to_us(&mut i);
        (i.qty, i.unit, i.note)
    }

    #[test]
    fn metric_amounts_become_cups_and_spoons() {
        assert_eq!(us(250.0, "ml", "milk"), (Some(1.0), "cup".into(), "250 ml".into()));
        assert_eq!(us(15.0, "mL", "olive oil"), (Some(1.0), "tbsp".into(), "15 mL".into()));
        assert_eq!(us(5.0, "ml", "vanilla extract"), (Some(1.0), "tsp".into(), "5 ml".into()));
        assert_eq!(us(250.0, "g", "all-purpose flour").0, Some(2.125));
        assert_eq!(us(240.0, "g", "all-purpose flour").1, "cup");
        assert_eq!(us(115.0, "g", "unsalted butter"), (Some(8.0), "tbsp".into(), "115 g".into()));
        assert_eq!(us(1.0, "kg", "potatoes"), (Some(2.25), "lb".into(), "1 kg".into()));
        assert_eq!(us(200.0, "g", "chicken thighs"), (Some(7.0), "oz".into(), "200 g".into()));
        assert_eq!(us(10.0, "g", "sugar"), (Some(0.75), "tbsp".into(), "10 g".into()));
        // Already US, or no unit: untouched.
        assert_eq!(us(2.0, "cups", "water"), (Some(2.0), "cups".into(), String::new()));
        assert_eq!(us(3.0, "", "eggs"), (Some(3.0), String::new(), String::new()));
    }

    #[test]
    fn us_amounts_become_metric() {
        let m = |q: f64, u: &str, item: &str| {
            let mut i = ing(q, u, item);
            ingredient_to_metric(&mut i);
            (i.qty, i.unit)
        };
        assert_eq!(m(1.0, "cup", "milk"), (Some(240.0), "ml".into()));
        assert_eq!(m(2.0, "cups", "flour"), (Some(240.0), "g".into()));
        assert_eq!(m(1.0, "tsp", "salt"), (Some(5.0), "ml".into()));
        assert_eq!(m(4.0, "tbsp", "butter"), (Some(57.0), "g".into()));
        assert_eq!(m(1.0, "lb", "ground beef"), (Some(455.0), "g".into()));
        let mut milk = Ingredient { note: "80 ml".into(), ..ing(0.33, "cup", "whole milk") };
        ingredient_to_metric(&mut milk);
        assert_eq!((milk.qty, milk.unit.as_str(), milk.note.as_str()), (Some(80.0), "ml", ""));
        let mut spinach = Ingredient { note: "140 g; washed".into(), ..ing(5.0, "oz", "spinach") };
        ingredient_to_metric(&mut spinach);
        assert_eq!((spinach.qty, spinach.unit.as_str(), spinach.note.as_str()), (Some(140.0), "g", "washed"));
    }

    #[test]
    fn temperatures_follow_the_units() {
        assert_eq!(temps("Bake at 180°C for 25 minutes.", false), "Bake at 350°F for 25 minutes.");
        assert_eq!(temps("Heat the oven to 220 °C", false), "Heat the oven to 425°F");
        assert_eq!(temps("Heat to 200°C (400°F).", false), "Heat to 200°C (400°F).");
        assert_eq!(temps("Bake at 350°F", true), "Bake at 180°C");
        assert_eq!(temps("Steep 4 minutes at 93°C", false), "Steep 4 minutes at 200°F");
        assert_eq!(temps("No temperatures here, 2 eggs.", false), "No temperatures here, 2 eggs.");
    }

    #[test]
    fn prompts_differ() {
        assert!(prompt_rule(false).contains("tbsp"));
        assert!(prompt_rule(true).contains("grams"));
    }
}
