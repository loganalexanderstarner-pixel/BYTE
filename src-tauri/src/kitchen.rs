//! Kitchen (Phase 6, v0.6.4): recipes, "what can I make with…", and weekly
//! meal plans, written like a professional chef would teach a home cook
//! working by hand: exact amounts (US and metric; grams for baking), mise en
//! place, temperatures, times with what to look, smell and listen for, what
//! can go wrong and how to fix it. Coffee drinks get ratio, grind, water
//! temperature and brew time.
//!
//! With the web on, a recipe starts from real recipe pages: BYTE searches,
//! reads the pages' structured recipe data (schema.org Recipe JSON-LD, which
//! most recipe sites publish, including a photo), and the model writes one
//! best version for the user, citing where it came from. Offline, the model
//! writes it from what it knows (no photo).
//!
//! Results are cards in the chat (`ChatEvent::Recipe`, `RecipeIdeas`,
//! `MealPlan`); recipes can be saved to the recipe box (DB table `recipes`).

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tokio_util::sync::CancellationToken;

use crate::agent::Turn;
use crate::chat::{self, ChatEvent};
use crate::error::AppResult;
use crate::research::{self, Ctx, Emit};
use crate::tools::{self, SourceBook};

// ---------- what the user asked ----------

#[derive(Debug, Clone, PartialEq)]
pub enum KitchenAsk {
    /// A recipe for a dish ("how do I make a flat white").
    Recipe(String),
    /// Ideas from what the user has ("what can I make with eggs, spinach and feta").
    Ideas(Vec<String>),
    /// A meal plan ("plan dinners for the week with chicken and rice").
    Plan,
}

const PLAN_CUES: &[&str] = &["meal plan", "plan my meals", "plan meals", "plan our meals", "meals for the week", "weekly meal", "week of meals", "week of dinners", "dinners for the week", "dinners this week", "meals this week", "meal prep plan", "plan dinners", "plan breakfasts", "plan lunches", "plan this week's"];
const IDEA_CUES: &[&str] = &["what can i make with", "what can i cook with", "what should i make with", "what should i cook with", "what can we make with", "what can i bake with", "what to make with", "what to cook with", "recipes using", "recipes with", "ideas using", "ideas with", "something to make with", "use up"];
const RECIPE_CUES: &[&str] = &["recipe for", "recipe to", "recipe", "how do i make", "how do you make", "how to make", "how can i make", "how do i bake", "how to bake", "how do i brew", "how to brew", "teach me to make", "teach me how to make", "i want to make", "i want to bake", "let's make", "help me make", "help me bake"];

/// Kitchen words that make "how do I make…" about food (not "make money").
const FOOD_HINTS: &[&str] = &[
    "coffee", "latte", "cappuccino", "espresso", "americano", "flat white", "cold brew", "pour over", "pour-over", "mocha", "macchiato", "matcha", "chai", "tea", "cocktail", "smoothie",
    "bread", "cake", "cookie", "cookies", "pie", "muffin", "brownie", "pizza", "pasta", "sauce", "soup", "stew", "curry", "salad", "chicken", "beef", "pork", "fish", "salmon", "shrimp", "tofu", "egg", "eggs", "rice", "noodles", "tacos", "burger", "steak", "lasagna", "lasagne", "risotto", "pancakes", "waffles", "omelette", "omelet", "frittata", "scones", "croissant", "biscuits", "sourdough", "dough", "cheesecake", "dessert", "breakfast", "lunch", "dinner", "sandwich", "dumplings", "chili", "roast", "casserole", "quiche", "granola", "dressing", "marinade", "gravy", "frosting", "icing", "bagels", "focaccia", "ramen", "pho", "biryani", "paella",
];

fn has_food_word(m: &str) -> bool {
    let w = format!(" {} ", m.replace(|c: char| !c.is_alphanumeric() && c != '-', " "));
    FOOD_HINTS.iter().any(|f| w.contains(&format!(" {f} ")) || w.contains(&format!(" {f}s ")))
}

/// Items in a list like "eggs, spinach and feta cheese".
pub fn split_items(text: &str) -> Vec<String> {
    text.split([',', ';', '\n'])
        .flat_map(|p| p.split(" and "))
        .flat_map(|p| p.split(" & "))
        .map(|p| {
            let p = p.trim().trim_end_matches(['?', '.', '!']).trim();
            let l = p.to_lowercase();
            let p = ["some ", "a few ", "a ", "an ", "the ", "leftover ", "and "].iter().find(|a| l.starts_with(**a)).map(|a| &p[a.len()..]).unwrap_or(p);
            p.trim().to_string()
        })
        .filter(|p| !p.is_empty() && p.split_whitespace().count() <= 5)
        .take(20)
        .collect()
}

/// Reads a kitchen request, or None when it isn't one.
pub fn kitchen_ask(message: &str) -> Option<KitchenAsk> {
    let m = message.trim().replace('\u{2019}', "'");
    let lower = m.to_lowercase();
    if lower.contains("```") || lower.lines().count() > 15 {
        return None;
    }
    if PLAN_CUES.iter().any(|c| lower.contains(c)) {
        return Some(KitchenAsk::Plan);
    }
    if let Some((cue, i)) = IDEA_CUES.iter().filter_map(|c| lower.find(c).map(|i| (*c, i))).min_by_key(|(_, i)| *i) {
        let items = split_items(&m[i + cue.len()..]);
        if !items.is_empty() {
            return Some(KitchenAsk::Ideas(items));
        }
    }
    // "I have chicken, rice and broccoli, what can I make?"
    if (lower.starts_with("i have ") || lower.starts_with("i've got ") || lower.starts_with("we have ")) && (lower.contains("what can") || lower.contains("what should") || lower.contains("ideas")) {
        let start = m.find(' ').map(|i| i + 1).unwrap_or(0);
        let body = &m[start..];
        let body = body.strip_prefix("have ").or_else(|| body.strip_prefix("got ")).unwrap_or(body);
        let end = body.to_lowercase().find(" what").or_else(|| body.find('?')).unwrap_or(body.len());
        let items = split_items(&body[..end]);
        if !items.is_empty() {
            return Some(KitchenAsk::Ideas(items));
        }
    }
    let (cue, i) = RECIPE_CUES.iter().filter_map(|c| lower.find(c).map(|i| (*c, i))).min_by_key(|(_, i)| *i)?;
    // "how to make money" isn't a recipe; "recipe" alone is.
    if !cue.starts_with("recipe") && !has_food_word(&lower) {
        return None;
    }
    let after = &m[i + cue.len()..];
    let dish: String = after
        .trim_start_matches([':', ' '])
        .split(['?', '.', '!'])
        .next()
        .unwrap_or("")
        .trim()
        .trim_start_matches("a ")
        .trim_start_matches("an ")
        .trim_start_matches("some ")
        .trim_start_matches("the ")
        .to_string();
    let dish = if dish.split_whitespace().count() < 1 || dish.len() > 120 {
        // "What's a good recipe?": use the whole message.
        m.trim_end_matches(['?', '.', '!']).to_string()
    } else {
        dish
    };
    Some(KitchenAsk::Recipe(dish))
}

/// Whether this turn is for the kitchen (module on and a kitchen request).
pub fn applies(enabled: bool, question: &str) -> bool {
    enabled && kitchen_ask(question).is_some()
}

// ---------- recipes from web pages (schema.org JSON-LD) ----------

/// A recipe published on a web page.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct SourceRecipe {
    pub name: String,
    pub image: String,
    pub servings: String,
    pub total_min: Option<u32>,
    pub ingredients: Vec<String>,
    pub steps: Vec<String>,
}

/// Minutes in an ISO 8601 duration ("PT1H15M", "P0DT0H30M", "PT90M").
pub fn iso_minutes(d: &str) -> Option<u32> {
    let d = d.trim().to_uppercase();
    let t = d.strip_prefix('P')?;
    let (days, time) = match t.split_once('T') {
        Some((a, b)) => (a, b),
        None => (t, ""),
    };
    let num = |s: &str, unit: char| -> u32 {
        s.split_inclusive(|c: char| c.is_ascii_alphabetic()).find(|p| p.ends_with(unit)).and_then(|p| p[..p.len() - 1].parse::<f64>().ok()).unwrap_or(0.0) as u32
    };
    let total = num(days, 'D') * 1440 + num(time, 'H') * 60 + num(time, 'M');
    (total > 0).then_some(total)
}

fn text_of(v: &Value) -> String {
    match v {
        Value::String(s) => tools::academic::plain(s),
        Value::Number(n) => n.to_string(),
        Value::Array(a) => a.first().map(text_of).unwrap_or_default(),
        Value::Object(o) => o.get("text").or_else(|| o.get("name")).map(text_of).unwrap_or_default(),
        _ => String::new(),
    }
}

fn image_of(v: &Value) -> String {
    match v {
        Value::String(s) => s.clone(),
        // Several sizes: the last is usually the largest; the first is fine too.
        Value::Array(a) => a.iter().map(image_of).find(|s| !s.is_empty()).unwrap_or_default(),
        Value::Object(o) => o.get("url").or_else(|| o.get("contentUrl")).map(image_of).unwrap_or_default(),
        _ => String::new(),
    }
}

fn steps_of(v: &Value, out: &mut Vec<String>) {
    match v {
        Value::String(s) => out.extend(s.split('\n').map(|l| tools::academic::plain(l)).filter(|l| !l.is_empty())),
        Value::Array(a) => a.iter().for_each(|x| steps_of(x, out)),
        Value::Object(o) => {
            if let Some(items) = o.get("itemListElement") {
                steps_of(items, out);
            } else if let Some(t) = o.get("text").or_else(|| o.get("name")) {
                let t = text_of(t);
                if !t.is_empty() {
                    out.push(t);
                }
            }
        }
        _ => {}
    }
}

fn is_recipe(v: &Value) -> bool {
    match &v["@type"] {
        Value::String(t) => t == "Recipe",
        Value::Array(a) => a.iter().any(|t| t == "Recipe"),
        _ => false,
    }
}

fn find_recipe(v: &Value) -> Option<&Value> {
    if is_recipe(v) {
        return Some(v);
    }
    match v {
        Value::Array(a) => a.iter().find_map(find_recipe),
        Value::Object(o) => o.get("@graph").and_then(find_recipe),
        _ => None,
    }
}

/// The recipe a page publishes as JSON-LD, if any.
pub fn recipe_ld(html: &str) -> Option<SourceRecipe> {
    let doc = scraper::Html::parse_document(html);
    let sel = scraper::Selector::parse(r#"script[type="application/ld+json"]"#).ok()?;
    for script in doc.select(&sel) {
        let raw: String = script.text().collect();
        let Ok(v) = serde_json::from_str::<Value>(raw.trim()) else { continue };
        let Some(r) = find_recipe(&v) else { continue };
        let ingredients: Vec<String> = r["recipeIngredient"].as_array().or(r["ingredients"].as_array()).into_iter().flatten().map(text_of).filter(|s| !s.is_empty()).take(60).collect();
        let mut steps = Vec::new();
        steps_of(&r["recipeInstructions"], &mut steps);
        steps.truncate(40);
        if ingredients.is_empty() && steps.is_empty() {
            continue;
        }
        let total = r["totalTime"].as_str().and_then(iso_minutes).or_else(|| Some(r["prepTime"].as_str().and_then(iso_minutes).unwrap_or(0) + r["cookTime"].as_str().and_then(iso_minutes).unwrap_or(0)).filter(|m| *m > 0));
        return Some(SourceRecipe { name: text_of(&r["name"]), image: image_of(&r["image"]), servings: text_of(&r["recipeYield"]), total_min: total, ingredients, steps });
    }
    None
}

/// A published recipe as notes for the model.
pub fn source_notes(n: u32, site: &str, r: &SourceRecipe) -> String {
    let mut out = format!("[{n}] {} ({site})", r.name);
    if !r.servings.is_empty() {
        out.push_str(&format!(" · serves {}", r.servings));
    }
    if let Some(t) = r.total_min {
        out.push_str(&format!(" · {t} min"));
    }
    out.push_str("\nIngredients:\n");
    for i in &r.ingredients {
        out.push_str(&format!("- {i}\n"));
    }
    out.push_str("Method:\n");
    for (k, s) in r.steps.iter().enumerate() {
        out.push_str(&format!("{}. {s}\n", k + 1));
    }
    out.chars().take(3500).collect()
}

// ---------- the cards ----------

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct Ingredient {
    /// Amount for the recipe's servings (None: "to taste", "a pinch").
    #[serde(default)]
    pub qty: Option<f64>,
    #[serde(default)]
    pub unit: String,
    pub item: String,
    /// "finely chopped", "room temperature", or the metric amount ("240 ml").
    #[serde(default)]
    pub note: String,
    /// The user said they have it.
    #[serde(default)]
    pub have: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct RecipeStep {
    pub text: String,
    /// A timer for this step, in minutes.
    #[serde(default)]
    pub minutes: Option<u32>,
    /// How to tell it's done ("golden at the edges, smells nutty").
    #[serde(default)]
    pub cue: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct Recipe {
    pub title: String,
    pub description: String,
    /// "dinner", "baking", "coffee", "breakfast", "dessert", "drink", …
    pub category: String,
    #[serde(default)]
    pub cuisine: String,
    pub servings: u32,
    #[serde(default)]
    pub prep_min: Option<u32>,
    #[serde(default)]
    pub cook_min: Option<u32>,
    /// "Easy", "Medium" or "Advanced".
    #[serde(default)]
    pub difficulty: String,
    #[serde(default)]
    pub equipment: Vec<String>,
    pub ingredients: Vec<Ingredient>,
    pub steps: Vec<RecipeStep>,
    #[serde(default)]
    pub tips: Vec<String>,
    #[serde(default)]
    pub substitutions: Vec<String>,
    #[serde(default)]
    pub storage: String,
    /// A photo of the dish (from the recipe page it's based on).
    #[serde(default)]
    pub image: String,
    #[serde(default)]
    pub source_url: String,
    #[serde(default)]
    pub source_name: String,
    /// An emoji for the dish, shown when there's no photo.
    #[serde(default)]
    pub emoji: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct RecipeIdea {
    pub title: String,
    pub description: String,
    #[serde(default)]
    pub minutes: Option<u32>,
    /// Things it needs that the user didn't mention.
    #[serde(default)]
    pub missing: Vec<String>,
    #[serde(default)]
    pub emoji: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RecipeIdeas {
    pub have: Vec<String>,
    pub ideas: Vec<RecipeIdea>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct PlanMeal {
    /// "Breakfast", "Lunch", "Dinner".
    pub meal: String,
    pub title: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub minutes: Option<u32>,
    #[serde(default)]
    pub emoji: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct PlanDay {
    pub day: String,
    pub meals: Vec<PlanMeal>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct GroceryAisle {
    pub aisle: String,
    pub items: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct MealPlan {
    pub days: Vec<PlanDay>,
    pub grocery: Vec<GroceryAisle>,
    /// What the user said they already have (left off the grocery list).
    pub have: Vec<String>,
}

pub const CHEF: &str = "You are BYTE, cooking like a professional chef who teaches home cooks. Recipes must work \
perfectly for someone cooking by hand in an ordinary home kitchen. Give exact amounts: US cups/spoons with the metric \
amount in the note (grams for baking, which should be weighed), oven temperatures in °F and °C, and times with a sense \
cue for doneness (what it should look, smell, sound or feel like). Start with mise en place (what to prep first). Warn \
about the step people get wrong and how to fix it. For coffee and tea give the dose, ratio, grind size, water \
temperature and brew time. Never invent a source. Reply only with JSON.";

fn recipe_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "title": { "type": "string" },
            "description": { "type": "string" },
            "category": { "type": "string" },
            "cuisine": { "type": "string" },
            "emoji": { "type": "string" },
            "servings": { "type": "integer", "minimum": 1, "maximum": 24 },
            "prepMin": { "type": "integer" },
            "cookMin": { "type": "integer" },
            "difficulty": { "enum": ["Easy", "Medium", "Advanced"] },
            "equipment": { "type": "array", "maxItems": 10, "items": { "type": "string" } },
            "ingredients": { "type": "array", "minItems": 1, "maxItems": 30, "items": {
                "type": "object",
                "properties": { "qty": { "type": "number" }, "unit": { "type": "string" }, "item": { "type": "string" }, "note": { "type": "string" } },
                "required": ["qty", "unit", "item", "note"]
            }},
            "steps": { "type": "array", "minItems": 1, "maxItems": 20, "items": {
                "type": "object",
                "properties": { "text": { "type": "string" }, "minutes": { "type": "integer" }, "cue": { "type": "string" } },
                "required": ["text", "minutes", "cue"]
            }},
            "tips": { "type": "array", "maxItems": 5, "items": { "type": "string" } },
            "substitutions": { "type": "array", "maxItems": 5, "items": { "type": "string" } },
            "storage": { "type": "string" },
            "basedOn": { "type": "integer", "description": "Number of the source recipe this is mostly based on, 0 if none" }
        },
        "required": ["title", "description", "category", "emoji", "servings", "prepMin", "cookMin", "difficulty", "equipment", "ingredients", "steps", "tips", "substitutions", "storage", "basedOn"]
    })
}

fn clean(v: &Value, max: usize) -> String {
    v.as_str().unwrap_or("").trim().chars().take(max).collect()
}

fn list(v: &Value, n: usize, max: usize) -> Vec<String> {
    v.as_array().into_iter().flatten().map(|x| clean(x, max)).filter(|s| !s.is_empty()).take(n).collect()
}

fn minutes(v: &Value) -> Option<u32> {
    v.as_f64().filter(|m| *m > 0.0 && *m < 60.0 * 24.0 * 3.0).map(|m| m.round() as u32)
}

/// Words that mean the same ingredient in "what you have" and a recipe line.
fn matches_have(item: &str, have: &[String]) -> bool {
    let item = item.to_lowercase();
    have.iter().any(|h| {
        let h = h.to_lowercase();
        let h = h.trim_end_matches('s');
        !h.is_empty() && item.contains(h)
    })
}

/// The recipe card from the model's reply (cleaned), plus which source it's based on.
pub fn parse_recipe(reply: &str, have: &[String]) -> Option<(Recipe, u32)> {
    let v = research::lenient_json(reply);
    let title = clean(&v["title"], 100);
    let ingredients: Vec<Ingredient> = v["ingredients"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|i| {
            let item = clean(&i["item"], 100);
            (!item.is_empty()).then(|| Ingredient {
                qty: i["qty"].as_f64().filter(|q| *q > 0.0 && q.is_finite()),
                unit: clean(&i["unit"], 20),
                note: clean(&i["note"], 100),
                have: matches_have(&item, have),
                item,
            })
        })
        .take(30)
        .collect();
    let steps: Vec<RecipeStep> = v["steps"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|s| {
            let text = clean(&s["text"], 600);
            (!text.is_empty()).then(|| RecipeStep { text, minutes: minutes(&s["minutes"]), cue: clean(&s["cue"], 200) })
        })
        .take(20)
        .collect();
    if title.is_empty() || ingredients.is_empty() || steps.is_empty() {
        return None;
    }
    let difficulty = match clean(&v["difficulty"], 20).to_lowercase().as_str() {
        "easy" => "Easy",
        "advanced" | "hard" => "Advanced",
        _ => "Medium",
    };
    let emoji = clean(&v["emoji"], 8);
    let based_on = v["basedOn"].as_u64().unwrap_or(0) as u32;
    Some((
        Recipe {
            title,
            description: clean(&v["description"], 400),
            category: clean(&v["category"], 30).to_lowercase(),
            cuisine: clean(&v["cuisine"], 30),
            servings: v["servings"].as_u64().unwrap_or(2).clamp(1, 24) as u32,
            prep_min: minutes(&v["prepMin"]),
            cook_min: minutes(&v["cookMin"]),
            difficulty: difficulty.into(),
            equipment: list(&v["equipment"], 10, 60),
            ingredients,
            steps,
            tips: list(&v["tips"], 5, 300),
            substitutions: list(&v["substitutions"], 5, 200),
            storage: clean(&v["storage"], 300),
            emoji: if emoji.chars().count() <= 4 { emoji } else { String::new() },
            ..Default::default()
        },
        based_on,
    ))
}

fn ideas_schema() -> Value {
    json!({
        "type": "object",
        "properties": { "ideas": { "type": "array", "minItems": 3, "maxItems": 4, "items": {
            "type": "object",
            "properties": {
                "title": { "type": "string" }, "description": { "type": "string" }, "minutes": { "type": "integer" },
                "missing": { "type": "array", "maxItems": 5, "items": { "type": "string" } }, "emoji": { "type": "string" }
            },
            "required": ["title", "description", "minutes", "missing", "emoji"]
        }}},
        "required": ["ideas"]
    })
}

pub fn parse_ideas(reply: &str, have: &[String]) -> Option<RecipeIdeas> {
    let v = research::lenient_json(reply);
    let ideas: Vec<RecipeIdea> = v["ideas"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|i| {
            let title = clean(&i["title"], 80);
            (!title.is_empty()).then(|| RecipeIdea {
                title,
                description: clean(&i["description"], 240),
                minutes: minutes(&i["minutes"]),
                // Staples everyone has don't count as missing.
                missing: list(&i["missing"], 5, 40).into_iter().filter(|m| !matches_have(m, have) && !["salt", "pepper", "water", "oil"].contains(&m.to_lowercase().as_str())).collect(),
                emoji: clean(&i["emoji"], 8),
            })
        })
        .take(4)
        .collect();
    (!ideas.is_empty()).then(|| RecipeIdeas { have: have.to_vec(), ideas })
}

/// Days and meals asked for: "5 dinners", "breakfast and lunch for the week".
pub fn plan_shape(question: &str) -> (u32, Vec<&'static str>) {
    let q = question.to_lowercase();
    let mut meals: Vec<&'static str> = ["breakfast", "lunch", "dinner"].into_iter().filter(|m| q.contains(m)).collect();
    if meals.is_empty() {
        meals.push("dinner");
    }
    let mut days = 7;
    for w in q.split(|c: char| !c.is_alphanumeric()) {
        let n = match w {
            "three" => 3,
            "four" => 4,
            "five" => 5,
            "six" => 6,
            x => x.parse().unwrap_or(0),
        };
        if (2..=14).contains(&n) {
            days = n;
            break;
        }
    }
    if q.contains("weekday") || q.contains("work week") {
        days = 5;
    }
    (days, meals.into_iter().map(|m| match m { "breakfast" => "Breakfast", "lunch" => "Lunch", _ => "Dinner" }).collect())
}

fn plan_schema(days: u32, meals: usize) -> Value {
    json!({
        "type": "object",
        "properties": {
            "days": { "type": "array", "minItems": days, "maxItems": days, "items": {
                "type": "object",
                "properties": {
                    "day": { "type": "string" },
                    "meals": { "type": "array", "minItems": meals, "maxItems": meals, "items": {
                        "type": "object",
                        "properties": { "meal": { "type": "string" }, "title": { "type": "string" }, "description": { "type": "string" }, "minutes": { "type": "integer" }, "emoji": { "type": "string" } },
                        "required": ["meal", "title", "description", "minutes", "emoji"]
                    }}
                },
                "required": ["day", "meals"]
            }},
            "grocery": { "type": "array", "maxItems": 10, "items": {
                "type": "object",
                "properties": { "aisle": { "type": "string" }, "items": { "type": "array", "items": { "type": "string" } } },
                "required": ["aisle", "items"]
            }}
        },
        "required": ["days", "grocery"]
    })
}

pub fn parse_plan(reply: &str, days: u32, have: &[String]) -> Option<MealPlan> {
    let v = research::lenient_json(reply);
    let plan_days: Vec<PlanDay> = v["days"]
        .as_array()
        .into_iter()
        .flatten()
        .take(days as usize)
        .enumerate()
        .filter_map(|(i, d)| {
            let meals: Vec<PlanMeal> = d["meals"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(|m| {
                    let title = clean(&m["title"], 80);
                    (!title.is_empty()).then(|| PlanMeal { meal: clean(&m["meal"], 20), title, description: clean(&m["description"], 200), minutes: minutes(&m["minutes"]), emoji: clean(&m["emoji"], 8) })
                })
                .take(3)
                .collect();
            let day = match clean(&d["day"], 20) {
                x if x.is_empty() => format!("Day {}", i + 1),
                x => x,
            };
            (!meals.is_empty()).then_some(PlanDay { day, meals })
        })
        .collect();
    if plan_days.is_empty() {
        return None;
    }
    let grocery = v["grocery"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|a| {
            let items: Vec<String> = list(&a["items"], 30, 60).into_iter().filter(|i| !matches_have(i, have)).collect();
            let aisle = clean(&a["aisle"], 30);
            (!items.is_empty() && !aisle.is_empty()).then_some(GroceryAisle { aisle, items })
        })
        .collect();
    Some(MealPlan { days: plan_days, grocery, have: have.to_vec() })
}

/// What the user says they have, anywhere in the message ("I have …", "using …", "with …").
pub fn have_from(question: &str) -> Vec<String> {
    let q = question.replace('\u{2019}', "'");
    let lower = q.to_lowercase();
    for cue in ["i have ", "i've got ", "we have ", "using ", "with "] {
        if let Some(i) = lower.find(cue) {
            let rest = &q[i + cue.len()..];
            let end = rest.to_lowercase().find(|c: char| c == '?' || c == '.').unwrap_or(rest.len());
            let items = split_items(&rest[..end]);
            if !items.is_empty() {
                return items;
            }
        }
    }
    Vec::new()
}

// ---------- the pipeline ----------

pub const RECIPE_RULES: &str = "The user sees the full recipe above as a card (ingredients, steps, timers, tips), so \
don't repeat it. Write 2 to 4 short sentences: what makes this version good, the one thing to get right, and a \
serving idea. If it's based on a numbered source, cite it like [1]. No headings.";

pub const IDEAS_RULES: &str = "The user sees these dishes as cards they can pick. Write one or two friendly sentences: \
which one you'd pick for them and why, and that they can tap a card for the full recipe. Don't list them again.";

pub const PLAN_RULES: &str = "The user sees the plan above as a week card with a grocery list. Write two or three \
sentences: how the week is balanced, which evenings are quickest, and one prep-ahead tip (e.g. cook the rice twice). \
Don't list the meals again.";

/// Runs a kitchen request. `Ok(None)` when it isn't one or nothing usable came back.
pub async fn run(turn: &Turn<'_>, question: &str, cancel: &CancellationToken, send: Emit<'_>) -> AppResult<Option<(SourceBook, String)>> {
    let Some(ask) = kitchen_ask(question) else { return Ok(None) };
    let c = Ctx { turn, cancel, send };
    match ask {
        KitchenAsk::Recipe(dish) => recipe(&c, question, &dish).await,
        KitchenAsk::Ideas(have) => {
            c.call("byte_kideas", "recipe_ideas", json!({ "have": have }))?;
            let user = format!("The user has: {}.\nMessage: {question}\n\nSuggest 3 or 4 different dishes they could make now, mostly from what they have (pantry staples are fine). For each: a title, a one-sentence mouth-watering description, total minutes, what they'd need that they didn't mention, and one food emoji.", have.join(", "));
            let reply = c.cancellable(chat::complete_json(turn.http, turn.ep, CHEF, &user, ideas_schema(), 700)).await?.unwrap_or_default();
            let Some(ideas) = parse_ideas(&reply, &have) else {
                c.result("byte_kideas", false, "No ideas came back")?;
                return Ok(None);
            };
            c.result("byte_kideas", true, format!("{} ideas", ideas.ideas.len()))?;
            send(ChatEvent::RecipeIdeas(ideas.clone()))?;
            let list: Vec<String> = ideas.ideas.iter().map(|i| i.title.clone()).collect();
            Ok(Some((SourceBook::default(), format!("Dishes shown: {}.\n\n{IDEAS_RULES}", list.join("; ")))))
        }
        KitchenAsk::Plan => {
            let (days, meals) = plan_shape(question);
            let have = have_from(question);
            c.call("byte_kplan", "meal_plan", json!({ "days": days, "meals": meals }))?;
            let user = format!(
                "Message: {question}\n\nPlan {days} days of {} for this household. Vary proteins and cuisines, keep weeknights quick (under 40 minutes), reuse ingredients across days to cut waste, and use what they have{}. Name each day (Monday…), give each meal a title, a one-line description, total minutes and a food emoji. Then a grocery list grouped by aisle (Produce, Meat & fish, Dairy & eggs, Pantry, Bakery, Frozen, Spices) with amounts, leaving out what they already have.",
                meals.join(", ").to_lowercase(),
                if have.is_empty() { String::new() } else { format!(": {}", have.join(", ")) }
            );
            let reply = c.cancellable(chat::complete_json(turn.http, turn.ep, CHEF, &user, plan_schema(days, meals.len()), 400 + days * meals.len() as u32 * 110 + 500)).await?.unwrap_or_default();
            let Some(plan) = parse_plan(&reply, days, &have) else {
                c.result("byte_kplan", false, "The plan couldn't be read")?;
                return Ok(None);
            };
            c.result("byte_kplan", true, format!("{} days, {} meals", plan.days.len(), plan.days.iter().map(|d| d.meals.len()).sum::<usize>()))?;
            send(ChatEvent::MealPlan(plan.clone()))?;
            let summary: Vec<String> = plan.days.iter().map(|d| format!("{}: {}", d.day, d.meals.iter().map(|m| m.title.as_str()).collect::<Vec<_>>().join(", "))).collect();
            Ok(Some((SourceBook::default(), format!("Meal plan shown:\n{}\n\n{PLAN_RULES}", summary.join("\n")))))
        }
    }
}

/// A saved recipe matching the request ("make my saved lasagna").
fn saved_match(turn: &Turn<'_>, dish: &str) -> Option<Recipe> {
    use tauri::Manager;
    let app = turn.app?;
    let state = app.try_state::<crate::state::AppState>()?;
    recipes_list(&state.db, Some(dish)).ok()?.into_iter().next().and_then(|s| serde_json::from_value(s.recipe).ok())
}

async fn recipe(c: &Ctx<'_, '_>, question: &str, dish: &str) -> AppResult<Option<(SourceBook, String)>> {
    let turn = c.turn;
    let have = have_from(question);
    let mut book = SourceBook::default();
    let mut notes = String::new();
    let mut images: Vec<(u32, String, String, String)> = Vec::new(); // (n, image, url, site)

    let lower = question.to_lowercase();
    if lower.contains("my saved") || lower.contains("saved recipe") || lower.contains("my recipe for") {
        if let Some(r) = saved_match(turn, dish) {
            notes.push_str(&format!("The user's saved recipe:\n{}\n\n", serde_json::to_string(&r).unwrap_or_default()));
        }
    }

    // With the web on, start from real recipes (and their photos).
    if turn.web && notes.is_empty() {
        let query = format!("{dish} recipe");
        let id = "byte_ksearch";
        c.call(id, tools::WEB_SEARCH, json!({ "query": query }))?;
        let found = c.cancellable(tools::search::search(turn.net, turn.cloud, &query, 8)).await?;
        let results = match found {
            Ok(s) => {
                c.result(id, true, format!("{} results", s.results.len()))?;
                s.results
            }
            Err(e) => {
                c.result(id, false, e.to_string())?;
                Vec::new()
            }
        };
        let candidates: Vec<_> = results.iter().filter(|r| tools::fetch::worth_reading(&r.url)).take(6).collect();
        let ids: Vec<String> = (0..candidates.len()).map(|i| format!("byte_kread_{i}")).collect();
        for (id, r) in ids.iter().zip(&candidates) {
            c.call(id, tools::READ_PAGE, json!({ "url": r.url }))?;
        }
        let pages = c.cancellable(futures_util::future::join_all(candidates.iter().map(|r| tools::fetch::fetch_html(turn.net, &r.url)))).await?;
        let mut used = 0;
        for ((id, r), page) in ids.iter().zip(&candidates).zip(pages) {
            match page {
                Ok((p, html)) if used < 4 => {
                    let site = tools::host_of(&p.url);
                    let n = book.mark_read(&r.url, if p.title.is_empty() { &r.title } else { &p.title });
                    if let Some(src) = recipe_ld(&html) {
                        notes.push_str(&source_notes(n, &site, &src));
                        notes.push_str("\n\n");
                        if !src.image.is_empty() {
                            images.push((n, src.image.clone(), p.url.clone(), site.clone()));
                        }
                        c.result(id, true, format!("recipe: {}", src.name.chars().take(50).collect::<String>()))?;
                    } else {
                        notes.push_str(&format!("[{n}] {} ({site})\n{}\n\n", p.title, tools::fetch::relevant_passages(&p.text, dish, 1500)));
                        c.result(id, true, p.title.chars().take(60).collect::<String>())?;
                    }
                    used += 1;
                }
                Ok(_) => c.result(id, true, "not needed")?,
                Err(e) => c.result(id, false, e.to_string())?,
            }
        }
        if !book.sources.is_empty() {
            (c.send)(ChatEvent::Sources { sources: book.sources.clone() })?;
        }
    }

    c.call("byte_krecipe", "write_recipe", json!({ "dish": dish }))?;
    let user = format!(
        "Request: {question}\n{}\n{}\nWrite the best version of this recipe for the user as a JSON recipe card. {}If they \
mentioned how many people, use that for servings. Each step is one clear action with its time in minutes (0 if none) \
and a doneness cue. Give 2 to 5 chef's tips and a few substitutions.",
        if have.is_empty() { String::new() } else { format!("They have: {}. Use these where it makes sense.\n", have.join(", ")) },
        if notes.is_empty() { String::new() } else { format!("\nRecipes and notes to work from:\n{}", notes.chars().take(12_000).collect::<String>()) },
        if images.is_empty() && notes.is_empty() { "" } else { "Base it on the most reliable source above (set basedOn to its number) and improve the method with your own expertise. " },
    );
    let reply = c.cancellable(chat::complete_json(turn.http, turn.ep, CHEF, &user, recipe_schema(), 3000)).await?.unwrap_or_default();
    let Some((mut recipe, based_on)) = parse_recipe(&reply, &have) else {
        c.result("byte_krecipe", false, "The recipe couldn't be read")?;
        return Ok(None);
    };
    // The photo and credit from the source it's based on (else the first with a photo).
    if let Some((n, image, url, site)) = images.iter().find(|(n, ..)| *n == based_on).or(images.first()).cloned() {
        recipe.image = image;
        recipe.source_url = url;
        recipe.source_name = site;
        let _ = n;
    }
    c.result("byte_krecipe", true, format!("{} ingredients, {} steps", recipe.ingredients.len(), recipe.steps.len()))?;
    (c.send)(ChatEvent::Recipe(recipe.clone()))?;
    let cite = if based_on > 0 && book.sources.iter().any(|s| s.n == based_on) { format!("It's based on source [{based_on}].\n") } else { String::new() };
    Ok(Some((book, format!("Recipe shown: {} ({}).\n{cite}\n{RECIPE_RULES}", recipe.title, recipe.description))))
}

// ---------- the recipe box (DB table `recipes`) ----------

/// A saved recipe.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SavedRecipe {
    pub id: i64,
    pub title: String,
    pub category: String,
    pub image: String,
    pub saved_at: i64,
    pub recipe: Value,
}

pub fn recipe_save(db: &crate::db::Db, recipe: &Value) -> AppResult<i64> {
    let r: Recipe = serde_json::from_value(recipe.clone()).map_err(|e| crate::error::AppError::msg(format!("not a recipe: {e}")))?;
    let conn = db.conn();
    // Saving the same recipe twice updates it.
    let existing: Option<i64> = conn.query_row("SELECT id FROM recipes WHERE title = ?1 AND source_url = ?2", rusqlite::params![r.title, r.source_url], |row| row.get(0)).ok();
    let data = serde_json::to_string(&r)?;
    let now = chrono::Utc::now().timestamp_millis();
    match existing {
        Some(id) => {
            conn.execute("UPDATE recipes SET data = ?1, category = ?2, image = ?3, saved_at = ?4 WHERE id = ?5", rusqlite::params![data, r.category, r.image, now, id])?;
            Ok(id)
        }
        None => {
            conn.execute(
                "INSERT INTO recipes (title, category, image, source_url, data, saved_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                rusqlite::params![r.title, r.category, r.image, r.source_url, data, now],
            )?;
            Ok(conn.last_insert_rowid())
        }
    }
}

/// Saved recipes, newest first; `query` matches the title, category or ingredients.
pub fn recipes_list(db: &crate::db::Db, query: Option<&str>) -> AppResult<Vec<SavedRecipe>> {
    let conn = db.conn();
    let words: Vec<String> = query
        .unwrap_or("")
        .to_lowercase()
        .split(|c: char| !c.is_alphanumeric())
        .filter(|w| w.len() > 2 && !["recipe", "saved", "the", "and", "for", "make"].contains(w))
        .map(|w| format!("%{w}%"))
        .collect();
    let mut st = conn.prepare("SELECT id, title, category, image, saved_at, data FROM recipes ORDER BY saved_at DESC")?;
    let rows = st.query_map([], |r| Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?, r.get::<_, String>(2)?, r.get::<_, String>(3)?, r.get::<_, i64>(4)?, r.get::<_, String>(5)?)))?;
    let mut out = Vec::new();
    for row in rows {
        let (id, title, category, image, saved_at, data) = row?;
        let hay = format!("{} {} {}", title, category, data).to_lowercase();
        if !words.iter().all(|w| hay.contains(w.trim_matches('%'))) {
            continue;
        }
        out.push(SavedRecipe { id, title, category, image, saved_at, recipe: serde_json::from_str(&data).unwrap_or(Value::Null) });
    }
    Ok(out)
}

pub fn recipe_delete(db: &crate::db::Db, id: i64) -> AppResult<()> {
    db.conn().execute("DELETE FROM recipes WHERE id = ?1", [id])?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn kitchen_requests_are_understood() {
        assert_eq!(kitchen_ask("What can I make with eggs, spinach and feta?"), Some(KitchenAsk::Ideas(vec!["eggs".into(), "spinach".into(), "feta".into()])));
        assert_eq!(kitchen_ask("I have chicken thighs, rice and broccoli, what can I make?"), Some(KitchenAsk::Ideas(vec!["chicken thighs".into(), "rice".into(), "broccoli".into()])));
        assert_eq!(kitchen_ask("How do I make a flat white at home?"), Some(KitchenAsk::Recipe("flat white at home".into())));
        assert_eq!(kitchen_ask("Recipe for banana bread"), Some(KitchenAsk::Recipe("banana bread".into())));
        assert_eq!(kitchen_ask("Plan our dinners for the week, we have ground beef"), Some(KitchenAsk::Plan));
        assert_eq!(kitchen_ask("Can you make a meal plan for 5 weekdays"), Some(KitchenAsk::Plan));
        assert_eq!(kitchen_ask("How do I make money online?"), None);
        assert_eq!(kitchen_ask("How to make a pivot table in Excel"), None);
        assert_eq!(kitchen_ask("Who won the game?"), None);
        assert!(applies(true, "how to bake sourdough bread"));
        assert!(!applies(false, "how to bake sourdough bread"));
    }

    #[test]
    fn meal_plan_shape_and_pantry() {
        assert_eq!(plan_shape("plan dinners for the week"), (7, vec!["Dinner"]));
        assert_eq!(plan_shape("meal plan for 5 days, breakfast and dinner"), (5, vec!["Breakfast", "Dinner"]));
        assert_eq!(plan_shape("weekday lunches meal plan").0, 5);
        assert_eq!(have_from("Plan dinners, we have ground beef, rice and a bag of spinach."), vec!["ground beef", "rice", "bag of spinach"]);
        assert!(have_from("plan my meals").is_empty());
    }

    #[test]
    fn durations_and_published_recipes() {
        assert_eq!(iso_minutes("PT1H15M"), Some(75));
        assert_eq!(iso_minutes("PT90M"), Some(90));
        assert_eq!(iso_minutes("P0DT0H30M"), Some(30));
        assert_eq!(iso_minutes("P1D"), Some(1440));
        assert_eq!(iso_minutes("PT0M"), None);
        assert_eq!(iso_minutes("soon"), None);
        let html = std::fs::read_to_string(format!("{}/tests/fixtures/recipe_bbc.html", env!("CARGO_MANIFEST_DIR"))).unwrap();
        let r = recipe_ld(&html).expect("recipe");
        assert_eq!(r.name, "Classic lasagne");
        assert_eq!(r.servings, "6");
        assert_eq!(r.total_min, Some(120));
        assert!(r.image.starts_with("https://images.immediate.co.uk/"));
        assert!(r.ingredients.iter().any(|i| i.contains("750g lean beef mince")));
        assert!(r.steps[0].starts_with("To make the meat sauce"));
        let notes = source_notes(3, "bbcgoodfood.com", &r);
        assert!(notes.starts_with("[3] Classic lasagne (bbcgoodfood.com) · serves 6 · 120 min"));
        // Other shapes: a list at the top, @type as a list, sections of steps, a string image.
        let other = r#"<script type="application/ld+json">[{"@type":["Recipe"],"name":"Cold brew","image":"https://x/cb.jpg","recipeIngredient":["100 g coffee"],"recipeInstructions":[{"@type":"HowToSection","name":"Brew","itemListElement":[{"@type":"HowToStep","text":"Mix &amp; steep 16 hours."}]}]}]</script>"#;
        let r = recipe_ld(other).unwrap();
        assert_eq!((r.image.as_str(), r.steps[0].as_str()), ("https://x/cb.jpg", "Mix & steep 16 hours."));
        assert!(recipe_ld("<html><body>no recipe</body></html>").is_none());
    }

    #[test]
    fn recipe_cards_are_cleaned() {
        let reply = r#"{"title":"Spinach & Feta Frittata","description":"Fluffy, golden and ready in 20 minutes.","category":"Breakfast","cuisine":"Greek","emoji":"🍳","servings":40,"prepMin":10,"cookMin":12,"difficulty":"easy",
            "equipment":["10-inch oven-safe skillet"],
            "ingredients":[{"qty":8,"unit":"","item":"large eggs","note":"room temperature"},{"qty":0,"unit":"","item":"salt","note":"to taste"},{"qty":2,"unit":"cups","item":"baby spinach","note":"60 g"},{"qty":1,"unit":"","item":"","note":""}],
            "steps":[{"text":"Heat the oven to 400°F (200°C).","minutes":0,"cue":""},{"text":"Bake until just set.","minutes":12,"cue":"The center jiggles only slightly"},{"text":"","minutes":5,"cue":""}],
            "tips":["Take it out while the middle still wobbles."],"substitutions":[],"storage":"Fridge up to 3 days.","basedOn":2}"#;
        let (r, based) = parse_recipe(reply, &["eggs".into(), "spinach".into()]).unwrap();
        assert_eq!(based, 2);
        assert_eq!(r.servings, 24);
        assert_eq!(r.difficulty, "Easy");
        assert_eq!(r.category, "breakfast");
        assert_eq!(r.ingredients.len(), 3);
        assert!(r.ingredients[0].have && r.ingredients[2].have && !r.ingredients[1].have);
        assert_eq!(r.ingredients[1].qty, None);
        assert_eq!(r.steps.len(), 2);
        assert_eq!(r.steps[0].minutes, None);
        assert_eq!(r.steps[1].minutes, Some(12));
        assert!(parse_recipe(r#"{"title":"x","ingredients":[],"steps":[]}"#, &[]).is_none());
        let v = serde_json::to_value(&r).unwrap();
        assert_eq!(v["prepMin"], 10);
        assert_eq!(v["ingredients"][0]["have"], true);
    }

    #[test]
    fn ideas_and_plans_are_cleaned() {
        let ideas = parse_ideas(r#"{"ideas":[{"title":"Shakshuka","description":"Eggs in spicy tomato","minutes":25,"missing":["canned tomatoes","eggs","salt"],"emoji":"🍳"},{"title":"","description":"x","minutes":1,"missing":[],"emoji":""}]}"#, &["eggs".into()]).unwrap();
        assert_eq!(ideas.ideas.len(), 1);
        assert_eq!(ideas.ideas[0].missing, vec!["canned tomatoes"]);
        let plan = parse_plan(
            r#"{"days":[{"day":"Monday","meals":[{"meal":"Dinner","title":"Beef tacos","description":"","minutes":25,"emoji":"🌮"}]},{"day":"","meals":[{"meal":"Dinner","title":"Fried rice","description":"","minutes":20,"emoji":""}]},{"day":"Extra","meals":[{"meal":"Dinner","title":"x","description":"","minutes":1,"emoji":""}]}],
               "grocery":[{"aisle":"Produce","items":["2 limes","1 bag spinach"]},{"aisle":"Meat","items":["1 lb ground beef"]}]}"#,
            2,
            &["ground beef".into(), "spinach".into()],
        )
        .unwrap();
        assert_eq!(plan.days.len(), 2);
        assert_eq!(plan.days[1].day, "Day 2");
        assert_eq!(plan.grocery, vec![GroceryAisle { aisle: "Produce".into(), items: vec!["2 limes".into()] }]);
    }

    #[test]
    fn the_recipe_box_saves_finds_and_deletes() {
        let dir = tempfile::tempdir().unwrap();
        let db = crate::db::Db::open(dir.path()).unwrap();
        let r = Recipe { title: "Classic lasagne".into(), category: "dinner".into(), servings: 6, ingredients: vec![Ingredient { item: "beef mince".into(), ..Default::default() }], steps: vec![RecipeStep { text: "Bake".into(), ..Default::default() }], ..Default::default() };
        let v = serde_json::to_value(&r).unwrap();
        let id = recipe_save(&db, &v).unwrap();
        // Saving again updates the same entry.
        assert_eq!(recipe_save(&db, &v).unwrap(), id);
        assert_eq!(recipes_list(&db, None).unwrap().len(), 1);
        assert_eq!(recipes_list(&db, Some("my saved lasagne")).unwrap()[0].title, "Classic lasagne");
        assert_eq!(recipes_list(&db, Some("mince")).unwrap().len(), 1);
        assert!(recipes_list(&db, Some("pancakes")).unwrap().is_empty());
        recipe_delete(&db, id).unwrap();
        assert!(recipes_list(&db, None).unwrap().is_empty());
        assert!(recipe_save(&db, &json!({"nope": 1})).is_err());
    }

    /// Live: `BYTE_TEST_WEB=1 cargo test live_recipe_page -- --ignored --nocapture`
    #[tokio::test]
    #[ignore]
    async fn live_recipe_page() {
        if std::env::var("BYTE_TEST_WEB").is_err() {
            return;
        }
        let net = crate::tools::fetch::web_client();
        let (page, html) = crate::tools::fetch::fetch_html(&net, "https://www.bbcgoodfood.com/recipes/classic-lasagne-0").await.expect("page");
        let r = recipe_ld(&html).expect("recipe data");
        eprintln!("{} · {} · image {} · {} ingredients · {} steps", page.title, r.name, r.image, r.ingredients.len(), r.steps.len());
        assert!(!r.image.is_empty() && r.ingredients.len() > 3 && r.steps.len() > 3);
    }

    /// Real engine (+ web when BYTE_TEST_WEB=1). Needs BYTE_TEST_LLAMA_SERVER and BYTE_TEST_MODEL.
    #[tokio::test]
    #[ignore]
    async fn e2e_kitchen() {
        let Ok(model) = std::env::var("BYTE_TEST_MODEL") else { return };
        let Some((_server, mut ep)) = crate::chat::e2e_support::start_server_with(&model, &["-c".into(), "16384".into()], None).await else { return };
        ep.context = 16384;
        let web = std::env::var("BYTE_TEST_WEB").is_ok();
        let dir = tempfile::tempdir().unwrap();
        let log = crate::tools::ActionLog::new(dir.path().join("a.jsonl"));
        let http = chat::local_client();
        let net = crate::tools::fetch::web_client();
        for (q, kind) in [("What can I make with eggs, spinach and feta?", "recipeIdeas"), ("How do I make a flat white at home?", "recipe"), ("Plan dinners for 3 days, we have chicken and rice", "mealPlan")] {
            let history = vec![chat::ChatMessage::new("user", q)];
            let system = crate::prompt::system_prompt(chrono::Local::now(), crate::settings::Mode::Auto, web, None);
            let plan = crate::router::plan_turn(crate::settings::Mode::Auto, crate::settings::ThinkingPref::Off, q);
            let (ch, seen) = crate::chat::e2e_support::collecting_channel();
            let turn = Turn { http: &http, cloud: None, net: &net, ep: &ep, system: &system, history: &history, plan, mode: crate::settings::Mode::Auto, web, memory: false, log: &log, files: None, app: None, task: None, home: None, depth: 0, web_always: false, kitchen: true, agent: false };
            let t = std::time::Instant::now();
            crate::agent::run(turn, CancellationToken::new(), &ch).await.unwrap();
            let ev = seen.lock().unwrap().clone();
            let card = ev.iter().find(|e| e["kind"] == kind).unwrap_or_else(|| panic!("no {kind} card for {q}: {ev:?}"));
            eprintln!("\n=== {q} ({:.0}s)\n{}", t.elapsed().as_secs_f64(), serde_json::to_string_pretty(card).unwrap().chars().take(1800).collect::<String>());
            let content: String = ev.iter().filter(|e| e["kind"] == "content").filter_map(|e| e["delta"].as_str()).collect();
            eprintln!("--- {content}");
        }
    }
}
