//! What the model is told when it is given pictures of the screen. Honest by construction: it must say what it can and
//! cannot see, and never invent rules or numbers.

use super::policy::WindowInfo;

fn where_from(w: &WindowInfo) -> String {
    match (w.program.trim(), w.title.trim()) {
        ("", "") => "the window in front".into(),
        (p, "") => format!("the {p} window"),
        ("", t) => format!("the window \"{t}\""),
        (p, t) => format!("the {p} window (\"{t}\")"),
    }
}

/// "What do you think about this?"
pub fn ask(w: &WindowInfo, picture: bool, has_text: bool) -> String {
    let mut s = format!(
        "\n\nThe person asked about what is on their screen. BYTE took ONE picture of {} just now{}. Answer from it: say what it is, then answer their question. \
Only describe what you can really see or read; if something is too small, cut off or not shown, say so instead of guessing.",
        where_from(w),
        if picture { "" } else { " (you can't see pictures, so you are given a description of it)" }
    );
    if has_text {
        s.push_str(" The text BYTE read from the window is included; trust it over your own reading for exact words, numbers and error codes.");
    }
    s
}

/// "How's my city doing?" (any game that shows a state: city builders, colony sims, tycoons, strategy).
pub fn advisor(w: &WindowInfo, picture: bool, has_text: bool) -> String {
    let mut s = format!(
        "\n\nThe person is playing a game and asked how it is going. BYTE took ONE picture of {} just now{}. Act as a calm, practical advisor. \
First say in one line what you think this is (the game and what the screen shows) and what you CAN and CAN'T see (for example: money, population, happiness, power and water are readable; traffic or the \
map outside the view are not). Then name the one to three most likely problems, using only numbers, icons and warnings that are actually visible, and give one concrete step for each. \
Never invent the game's rules, prices, item names, hidden numbers or buttons. If you are not sure which game it is, say so and keep the advice general.",
        where_from(w),
        if picture { "" } else { " (you can't see pictures, so you are given a description of it)" }
    );
    if has_text {
        s.push_str(" The text BYTE read from the screen (UI numbers and labels) is included; use it for the figures.");
    }
    s
}

/// "Where did I go wrong?": `times` are the seconds before the end for each frame, oldest first; `per_picture` how many frames are in each picture.
pub fn coach(w: &WindowInfo, seconds_covered: u32, times: &[f32], per_picture: usize, picture: bool) -> String {
    let list = times.iter().map(|t| if *t < 0.5 { "the end".to_string() } else { format!("{t:.0} s before the end") }).collect::<Vec<_>>().join(", ");
    let layout = if picture {
        format!(
            "They come as pictures; each picture holds up to {per_picture} frames, read left to right then top to bottom, and the pictures run in order from the earliest to the latest. "
        )
    } else {
        "You can't see pictures, so you are given a description of each group of frames, in order from the earliest to the latest. ".to_string()
    };
    format!(
        "\n\nThe person asked how they could have done better, or what went wrong. BYTE kept the last {seconds_covered} seconds of {} in memory only, and picked {} still frames from it \
(about one a second at most, with the last moments closer together): at {list}. {layout}\n\
Be an honest coach. 1) Say in a sentence or two what seems to be happening. 2) Name the most likely mistake and WHICH moment shows it. 3) Give one or two concrete things to try next time. \
You only see still frames, not their aim, timing or controls, so say plainly when the frames don't show enough to know (a death that happens between two frames, a fast shooter, something off screen). \
Never invent the game's rules, weapons, item names, numbers or controls that you can't read on screen. If you can't tell what the game is or what happened, say so and ask one short question. Keep it short and kind.",
        where_from(w),
        times.len(),
    )
}

/// What the photo helper is asked about each picture of frames when the main model can't see.
pub fn frames_question(labels: &[String]) -> String {
    format!(
        "These are still frames from a screen recording, in time order ({}), laid out left to right then top to bottom. For each frame in order say what the screen shows and what changed since the one before: \
positions, health or score bars, text on screen (copy it exactly), enemies or obstacles, and anything that looks like a mistake. Say \"not visible\" for what you can't see.",
        labels.join("; ")
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn win() -> WindowInfo {
        WindowInfo { program: "CitiesSkylines".into(), title: "Cities: Skylines".into() }
    }

    #[test]
    fn every_prompt_forbids_guessing_and_names_the_window() {
        let a = ask(&win(), true, true);
        assert!(a.contains("CitiesSkylines") && a.contains("say so instead of guessing"));
        let g = advisor(&win(), true, true);
        assert!(g.contains("CAN'T see") && g.contains("Never invent"));
        let c = coach(&win(), 30, &[29.0, 20.0, 0.0], 4, true);
        assert!(c.contains("29 s before the end") && c.contains("the end"));
        assert!(c.contains("Never invent") && c.contains("don't show enough"));
    }

    #[test]
    fn a_model_that_cannot_see_is_told_it_gets_descriptions() {
        assert!(ask(&win(), false, false).contains("you can't see pictures"));
        assert!(coach(&win(), 30, &[1.0], 4, false).contains("description"));
    }

    #[test]
    fn no_other_platform_wording() {
        for t in [ask(&win(), true, true), advisor(&win(), true, true), coach(&win(), 30, &[0.0], 4, true), frames_question(&["a".into()])] {
            for bad in ["Mac", "macOS", "Finder", "Keychain", "Spotlight"] {
                assert!(!t.contains(bad), "{bad}");
            }
        }
    }
}
