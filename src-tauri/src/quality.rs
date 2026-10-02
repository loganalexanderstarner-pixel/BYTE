//! Card quality checks: plain problems a small model's card can have (a flashcard
//! front that just repeats the topic, a recipe that isn't the dish asked for, a meal
//! plan with the same dinner every night…), and one targeted repair.
//!
//! Only for small models (`Modules::small_model`), and only when a problem is found,
//! so big models and good cards pay nothing. The repaired card is kept only if it has
//! fewer problems than the first one.

use std::collections::HashSet;

use serde_json::Value;

use crate::agent::Turn;
use crate::chat;
use crate::decide::Decision;
use crate::error::AppResult;
use crate::kitchen::{MealPlan, Recipe, RecipeIdeas};
use crate::study::{Flashcards, Quiz};
use crate::trip::TripPlan;

/// Models at or below this size (billions of parameters) get their cards checked.
pub const SMALL_B: f32 = 4.0;

/// Words that don't tell dishes, topics or titles apart.
const FILLER: &[&str] = &[
    "a", "an", "the", "and", "or", "of", "for", "with", "to", "in", "on", "at", "my", "your", "our", "some", "how", "make",
    "made", "do", "i", "me", "at", "home", "easy", "best", "quick", "simple", "recipe", "homemade", "classic", "perfect",
];

fn words(s: &str) -> HashSet<String> {
    s.to_lowercase()
        .split(|c: char| !c.is_alphanumeric())
        .filter(|w| w.len() > 1 && !FILLER.contains(w))
        // "cookies" and "cookie" are the same word here.
        .map(|w| w.strip_suffix("es").filter(|s| s.len() > 3).or_else(|| w.strip_suffix('s').filter(|s| s.len() > 2)).unwrap_or(w).to_string())
        .collect()
}

fn same(a: &str, b: &str) -> bool {
    let n = |s: &str| s.to_lowercase().chars().filter(|c| c.is_alphanumeric()).collect::<String>();
    n(a) == n(b)
}

pub fn flashcards(f: &Flashcards, topic: &str, asked: usize) -> Vec<String> {
    let mut out = Vec::new();
    let generic = f.cards.iter().filter(|c| same(&c.front, topic) || same(&c.front, &f.title)).count();
    if generic > 0 {
        out.push(format!("{generic} card fronts just repeat the topic (\"{topic}\"); each front must ask about one specific idea"));
    }
    let thin = f.cards.iter().filter(|c| c.back.split_whitespace().count() < 3).count();
    if thin > 0 {
        out.push(format!("{thin} card backs are too short to learn from; give a complete answer"));
    }
    if f.cards.len() * 2 < asked {
        out.push(format!("only {} cards; {asked} were asked for", f.cards.len()));
    }
    out
}

pub fn quiz(q: &Quiz) -> Vec<String> {
    let mut out = Vec::new();
    for (i, x) in q.questions.iter().enumerate() {
        let mut seen = HashSet::new();
        if x.choices.iter().any(|c| !seen.insert(c.to_lowercase())) {
            out.push(format!("question {} repeats a choice", i + 1));
        }
        if x.choices.len() < 3 {
            out.push(format!("question {} needs 4 choices", i + 1));
        }
        if x.explanation.trim().is_empty() {
            out.push(format!("question {} has no explanation", i + 1));
        }
    }
    out
}

/// Kinds of dish: a title that adds one the request didn't name ("Flat White Lentil Soup"
/// for a flat white) has drifted to a different dish.
const DISH_KINDS: &[&str] = &[
    "soup", "stew", "salad", "pasta", "curry", "cake", "bread", "pie", "sandwich", "pizza", "stir", "taco", "burger",
    "smoothie", "risotto", "omelet", "omelette", "frittata", "muffin", "cookie", "brownie", "chili", "noodle", "latte",
    "cappuccino", "espresso", "tea", "cocktail",
];

pub fn recipe(r: &Recipe, dish: &str) -> Vec<String> {
    let mut out = Vec::new();
    let want = words(dish);
    let title = words(&r.title);
    let drifted = DISH_KINDS.iter().find(|k| title.contains(**k) && !want.contains(**k) && !dish.to_lowercase().contains(**k));
    if !want.is_empty() && (title.is_disjoint(&want) || drifted.is_some()) {
        out.push(format!("the recipe (\"{}\") isn't the dish that was asked for (\"{dish}\"); write a recipe for {dish}", r.title));
    }
    let vague = r.ingredients.iter().filter(|i| i.qty.is_none() && !i.note.to_lowercase().contains("taste") && !i.unit.to_lowercase().contains("taste")).count();
    if vague > 1 {
        out.push(format!("{vague} ingredients have no amount"));
    }
    if r.steps.len() < 3 {
        out.push("fewer than 3 steps; give every step a home cook needs".into());
    }
    out
}

pub fn ideas(i: &RecipeIdeas) -> Vec<String> {
    let mut out = Vec::new();
    let mut seen = HashSet::new();
    if i.ideas.iter().any(|x| !seen.insert(x.title.to_lowercase())) {
        out.push("two ideas are the same dish".into());
    }
    // "What you'd need" shouldn't list what they said they have.
    let have: HashSet<String> = i.have.iter().flat_map(|h| words(h)).collect();
    if i.ideas.iter().any(|x| x.missing.iter().any(|m| !words(m).is_disjoint(&have))) {
        out.push("the missing-ingredients lists include things the user already has".into());
    }
    out
}

pub fn meal_plan(p: &MealPlan, days: usize) -> Vec<String> {
    let mut out = Vec::new();
    if p.days.len() != days {
        out.push(format!("{} days planned; {days} were asked for", p.days.len()));
    }
    let mut count = std::collections::HashMap::new();
    for m in p.days.iter().flat_map(|d| &d.meals) {
        *count.entry(m.title.to_lowercase()).or_insert(0) += 1;
    }
    if let Some((t, _)) = count.iter().find(|(_, n)| **n >= 3) {
        out.push(format!("\"{t}\" is planned three or more times; vary the meals"));
    }
    if p.grocery.iter().all(|a| a.items.is_empty()) {
        out.push("the grocery list is empty".into());
    }
    out
}

pub fn decision(d: &Decision) -> Vec<String> {
    let cells: Vec<_> = d.scores.iter().flatten().flatten().collect();
    let mut out = Vec::new();
    if cells.len() > 2 && cells.iter().all(|c| c.score == cells[0].score) {
        out.push("every score is the same; score each option on its merits".into());
    }
    let bare = cells.iter().filter(|c| c.reason.trim().is_empty()).count();
    if bare > 0 {
        out.push(format!("{bare} scores have no reason"));
    }
    out
}

pub fn trip(t: &TripPlan, days: usize) -> Vec<String> {
    let mut out = Vec::new();
    if t.days.len() != days {
        out.push(format!("{} days planned; {days} were asked for", t.days.len()));
    }
    let empty = t.days.iter().filter(|d| d.items.is_empty()).count();
    if empty > 0 {
        out.push(format!("{empty} days have nothing planned"));
    }
    out
}

/// What the model is asked when its card has problems.
pub fn repair_prompt(user: &str, reply: &str, problems: &[String]) -> String {
    format!(
        "{user}\n\n---\nYour previous JSON had these problems:\n{}\n\nHere it is:\n{}\n\nReply with the corrected JSON. Fix only these problems and keep everything else.",
        problems.iter().map(|p| format!("- {p}")).collect::<Vec<_>>().join("\n"),
        reply.chars().take(12_000).collect::<String>()
    )
}

/// The better of `reply` and one repair of it. `problems_of` reads a reply and lists
/// its problems (`None`: it can't be read at all, which the caller handles as before).
pub async fn improve(
    turn: &Turn<'_>,
    system: &str,
    user: &str,
    schema: Value,
    max_tokens: u32,
    reply: String,
    problems_of: impl Fn(&str) -> Option<Vec<String>>,
) -> AppResult<String> {
    if !turn.modules.small_model || turn.ep.cloud.is_some() {
        return Ok(reply);
    }
    let Some(problems) = problems_of(&reply).filter(|p| !p.is_empty()) else { return Ok(reply) };
    log::info!("card problems, asking for a repair: {problems:?}");
    // A failed repair keeps the first card: it was readable, just not great.
    let fixed = match chat::complete_json(turn.http, turn.ep, system, &repair_prompt(user, &reply, &problems), schema, max_tokens).await {
        Ok(f) => f,
        Err(e) => {
            log::warn!("card repair failed: {e}");
            return Ok(reply);
        }
    };
    Ok(match problems_of(&fixed) {
        Some(left) if left.len() < problems.len() => fixed,
        _ => reply,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::kitchen::{Ingredient, RecipeStep};
    use crate::study::Card;

    fn card(f: &str, b: &str) -> Card {
        Card { front: f.into(), back: b.into() }
    }

    #[test]
    fn flashcards_that_repeat_the_topic_are_caught() {
        let bad = Flashcards { title: "Photosynthesis".into(), cards: (0..6).map(|i| card("Photosynthesis", &format!("Fact number {i} about plants."))).collect() };
        assert_eq!(flashcards(&bad, "photosynthesis", 6).len(), 1);
        let good = Flashcards { title: "Photosynthesis".into(), cards: vec![card("What gas do plants take in?", "Carbon dioxide from the air."), card("Where does it happen?", "In the chloroplasts of leaf cells.")] };
        assert!(flashcards(&good, "photosynthesis", 2).is_empty());
        assert_eq!(flashcards(&good, "photosynthesis", 10).len(), 1, "too few");
    }

    #[test]
    fn a_recipe_for_the_wrong_dish_is_caught() {
        let step = |t: &str| RecipeStep { text: t.into(), minutes: None, cue: String::new() };
        let ing = |q: Option<f64>, item: &str| Ingredient { qty: q, unit: "cup".into(), item: item.into(), note: String::new(), have: false };
        let mut r = Recipe { title: "Flat White Lentil Soup".into(), steps: vec![step("a"), step("b"), step("c")], ingredients: vec![ing(Some(1.0), "lentils")], ..Default::default() };
        assert_eq!(recipe(&r, "flat white at home").len(), 1, "a soup isn't a flat white");
        r.title = "Velvety Flat White".into();
        assert!(recipe(&r, "flat white at home").is_empty());
        r.title = "Creamy Lentil Soup".into();
        assert_eq!(recipe(&r, "flat white").len(), 1);
        r.title = "Chocolate Chip Cookie".into();
        assert!(recipe(&r, "chocolate chip cookies").is_empty(), "plural is the same dish");
        r.ingredients = vec![ing(None, "milk"), ing(None, "espresso")];
        r.steps.truncate(1);
        assert_eq!(recipe(&r, "chocolate chip cookies").len(), 2);
    }

    #[test]
    fn repeated_meals_and_equal_scores_are_caught() {
        use crate::kitchen::{GroceryAisle, PlanDay, PlanMeal};
        let meal = |t: &str| PlanMeal { meal: "Dinner".into(), title: t.into(), description: String::new(), minutes: None, emoji: String::new() };
        let p = MealPlan {
            days: (0..3).map(|i| PlanDay { day: format!("Day {i}"), meals: vec![meal("Chicken and Rice")] }).collect(),
            grocery: vec![GroceryAisle { aisle: "Produce".into(), items: vec![] }],
            have: vec![],
        };
        assert_eq!(meal_plan(&p, 3).len(), 2, "same dinner 3 times, empty grocery list");
        assert_eq!(meal_plan(&p, 5).len(), 3);
        let cell = |s: u8, r: &str| Some(crate::decide::Cell { score: s, reason: r.into(), sources: vec![] });
        let d = Decision { options: vec!["A".into(), "B".into()], criteria: vec![], scores: vec![vec![cell(7, "ok"), cell(7, "")], vec![cell(7, "ok"), cell(7, "ok")]] };
        assert_eq!(decision(&d).len(), 2);
    }

    #[test]
    fn the_repair_prompt_lists_the_problems_and_the_old_reply() {
        let p = repair_prompt("Write cards.", "{\"cards\":[]}", &["too few".into()]);
        assert!(p.starts_with("Write cards."));
        assert!(p.contains("- too few") && p.contains("{\"cards\":[]}") && p.contains("Fix only these problems"));
    }
}
