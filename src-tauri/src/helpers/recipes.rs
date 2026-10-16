//! Recipes: a routine (control/routines.rs) saved as a small file that can be shared, and a safety review before one from someone
//! else is added. A recipe is plain data, never a script: it can only hold the same few steps routines have (open a program, close
//! a program, set the volume, notifications on/off, open a web address, run one of your own automations), each checked by the
//! same rules. An imported recipe never brings a schedule, and nothing runs until the person has read the review and clicks Import.

use serde::{Deserialize, Serialize};

use crate::control::routines::{self, RStep, Routine, MAX_STEPS};

pub const FORMAT: &str = "byte-recipe";
pub const VERSION: u32 = 1;
/// A recipe file is small: anything bigger is not one.
pub const MAX_BYTES: usize = 20_000;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Recipe {
    pub format: String,
    pub version: u32,
    /// What is said to run it, "game time".
    pub phrase: String,
    pub steps: Vec<RStep>,
}

/// One line of the review.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ReviewStep {
    pub text: String,
    /// "ok", or "look" when the person should read it twice.
    pub level: &'static str,
    pub note: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct Review {
    pub phrase: String,
    pub steps: Vec<ReviewStep>,
    /// Why it cannot be added at all (empty when it can).
    pub problems: Vec<String>,
    pub can_import: bool,
}

pub fn export(r: &Routine) -> String {
    let recipe = Recipe { format: FORMAT.into(), version: VERSION, phrase: r.phrase.clone(), steps: r.steps.clone() };
    serde_json::to_string_pretty(&recipe).unwrap_or_default()
}

fn host_of(url: &str) -> String {
    url::Url::parse(url).ok().and_then(|u| u.host_str().map(str::to_string)).unwrap_or_else(|| url.to_string())
}

/// How much attention a step needs.
pub fn judge(step: &RStep) -> (&'static str, String) {
    match step {
        RStep::OpenUrl { url } => {
            let secure = url.to_lowercase().starts_with("https://");
            ("look", format!("Opens a page on {}{}", host_of(url), if secure { "" } else { " (not a secure link)" }))
        }
        RStep::CloseApp { name } => ("look", format!("Asks {name} to close; unsaved work in it could be lost")),
        RStep::RunAutomation { name } => ("look", format!("Runs your automation \"{name}\" (only works if you have one with that name)")),
        RStep::DoNotDisturb { on: true } => ("look", "Turns notification pop-ups off until they are turned on again, or BYTE restarts".into()),
        RStep::Volume { percent } if *percent >= 80 => ("look", "Sets the volume loud".into()),
        _ => ("ok", String::new()),
    }
}

/// Reads and checks a recipe. Never returns a recipe that fails the routine rules.
pub fn parse(text: &str) -> Result<Recipe, String> {
    if text.len() > MAX_BYTES {
        return Err("That is too big to be a recipe.".into());
    }
    let r: Recipe = serde_json::from_str(text.trim()).map_err(|e| format!("That isn't a BYTE recipe ({e})."))?;
    if r.format != FORMAT {
        return Err("That isn't a BYTE recipe.".into());
    }
    if r.version != VERSION {
        return Err(format!("This recipe is version {}; this BYTE reads version {VERSION}.", r.version));
    }
    Ok(r)
}

/// The review shown before importing. `others`: the routines already saved (for the phrase check).
pub fn review(text: &str, others: &[Routine]) -> Review {
    let r = match parse(text) {
        Ok(r) => r,
        Err(e) => return Review { problems: vec![e], ..Default::default() },
    };
    let phrase = routines::normalize(&r.phrase);
    let steps: Vec<ReviewStep> = r.steps.iter().map(|s| {
        let (level, note) = judge(s);
        ReviewStep { text: s.label(), level, note }
    }).collect();
    let mut problems = Vec::new();
    if r.steps.len() > MAX_STEPS {
        problems.push(format!("It has {} steps; a routine can have at most {MAX_STEPS}.", r.steps.len()));
    }
    let candidate = Routine { id: "new".into(), phrase: phrase.clone(), steps: r.steps, schedule: String::new(), enabled: true };
    if let Err(e) = routines::check(&candidate, others) {
        problems.push(e);
    }
    Review { phrase, steps, can_import: problems.is_empty(), problems }
}

/// The routine to save from a recipe the person accepted, under `phrase` when they changed it. Never scheduled.
pub fn to_routine(text: &str, phrase: Option<&str>, others: &[Routine]) -> Result<Routine, String> {
    let r = parse(text)?;
    let routine = Routine { id: String::new(), phrase: routines::normalize(phrase.filter(|p| !p.trim().is_empty()).unwrap_or(&r.phrase)), steps: r.steps, schedule: String::new(), enabled: true };
    routines::check(&Routine { id: "new".into(), ..routine.clone() }, others)?;
    Ok(routine)
}

fn file_name(phrase: &str) -> String {
    let n: String = routines::normalize(phrase).replace(' ', "-");
    format!("{}.byte-recipe.json", if n.is_empty() { "recipe" } else { &n })
}

#[cfg(desktop)]
fn enabled(app: &tauri::AppHandle) -> Result<(), crate::error::AppError> {
    let on = crate::control::settings_now(app).is_some_and(|s| s.helper_recipes && !s.kids_mode);
    if on { Ok(()) } else { Err(crate::error::AppError::msg("Recipes are off (Settings > Control and screen > Everyday helpers).")) }
}

/// Saves a routine as a recipe file in Documents/BYTE/Recipes and returns its path.
#[tauri::command]
pub fn recipe_export(app: tauri::AppHandle, id: String) -> Result<String, crate::error::AppError> {
    enabled(&app)?;
    let r = routines::routines_list(app.clone()).into_iter().find(|r| r.id == id).ok_or_else(|| crate::error::AppError::msg("That routine isn't there any more."))?;
    let dir = crate::paths::user_documents_dir(&app)?.join("BYTE").join("Recipes");
    std::fs::create_dir_all(&dir)?;
    let path = dir.join(file_name(&r.phrase));
    std::fs::write(&path, export(&r))?;
    Ok(path.display().to_string())
}

/// What importing this recipe would add, for the person to read first.
#[tauri::command]
pub fn recipe_review(app: tauri::AppHandle, text: String) -> Result<Review, crate::error::AppError> {
    enabled(&app)?;
    Ok(review(&text, &routines::routines_list(app.clone())))
}

/// Adds the recipe as a routine (after the review). It has no schedule.
#[tauri::command]
pub fn recipe_import(app: tauri::AppHandle, text: String, phrase: Option<String>) -> Result<Routine, crate::error::AppError> {
    enabled(&app)?;
    let routine = to_routine(&text, phrase.as_deref(), &routines::routines_list(app.clone())).map_err(crate::error::AppError::msg)?;
    routines::routine_save(app, routine)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn routine() -> Routine {
        Routine {
            id: "r1".into(),
            phrase: "game time".into(),
            steps: vec![RStep::OpenApp { name: "notepad".into() }, RStep::DoNotDisturb { on: true }, RStep::Volume { percent: 40 }],
            schedule: "weekdays 18:00".into(),
            enabled: true,
        }
    }

    #[test]
    fn a_routine_exports_without_its_schedule_and_reads_back() {
        let text = export(&routine());
        assert!(!text.contains("weekdays") && text.contains("byte-recipe"));
        let r = parse(&text).unwrap();
        assert_eq!(r.steps, routine().steps);
        let back = to_routine(&text, None, &[]).unwrap();
        assert!(back.schedule.is_empty() && back.phrase == "game time");
    }

    #[test]
    fn the_review_marks_steps_that_need_a_second_look() {
        let mut r = routine();
        r.steps.push(RStep::OpenUrl { url: "http://example.com/x".into() });
        r.steps.push(RStep::CloseApp { name: "chrome".into() });
        r.steps.push(RStep::RunAutomation { name: "backup".into() });
        let rev = review(&export(&r), &[]);
        assert!(rev.can_import, "{:?}", rev.problems);
        let levels: Vec<&str> = rev.steps.iter().map(|s| s.level).collect();
        assert_eq!(levels, vec!["ok", "look", "ok", "look", "look", "look"]);
        assert!(rev.steps[3].note.contains("example.com") && rev.steps[3].note.contains("not a secure link"));
    }

    #[test]
    fn anything_that_is_not_a_plain_recipe_is_refused() {
        for t in [
            "not json",
            r#"{"format":"other","version":1,"phrase":"x y z","steps":[]}"#,
            r#"{"format":"byte-recipe","version":9,"phrase":"game time","steps":[]}"#,
            // An unknown step type or field cannot sneak in.
            r#"{"format":"byte-recipe","version":1,"phrase":"game time","steps":[{"type":"runCommand","command":"del *"}]}"#,
            r#"{"format":"byte-recipe","version":1,"phrase":"game time","extra":1,"steps":[{"type":"volume","percent":10}]}"#,
        ] {
            let rev = review(t, &[]);
            assert!(!rev.can_import && !rev.problems.is_empty(), "{t}");
            assert!(to_routine(t, None, &[]).is_err(), "{t}");
        }
        assert!(parse(&"x".repeat(MAX_BYTES + 1)).is_err());
    }

    #[test]
    fn the_routine_rules_still_apply_to_every_step_and_the_phrase() {
        // BYTE will not close a part of Windows, whoever wrote the recipe.
        let bad = r#"{"format":"byte-recipe","version":1,"phrase":"clean up","steps":[{"type":"closeApp","name":"explorer"}]}"#;
        assert!(!review(bad, &[]).can_import);
        let local_url = r#"{"format":"byte-recipe","version":1,"phrase":"clean up","steps":[{"type":"openUrl","url":"file:///C:/Windows/system32/cmd.exe"}]}"#;
        assert!(!review(local_url, &[]).can_import);
        // A phrase that is already taken must be changed.
        let r = routine();
        let text = export(&r);
        assert!(!review(&text, &[r.clone()]).can_import);
        assert!(to_routine(&text, Some("movie night"), &[r]).is_ok());
    }

    #[test]
    fn file_names_are_plain() {
        assert_eq!(file_name("Game time!"), "game-time.byte-recipe.json");
        assert_eq!(file_name("../../x"), "x.byte-recipe.json");
        assert_eq!(file_name(""), "recipe.byte-recipe.json");
    }
}
