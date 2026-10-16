//! Scheduled jobs and triggers. A job is a saved, named instruction that runs on a schedule (daily, weekdays, every N hours)
//! or when something happens (a download finished, a file appears in a folder, a new message in an allowed channel, a watched
//! page changed, BYTE starts). Everything is per profile and off until the person turns it on; a master switch pauses all jobs.
//!
//! - `model`: the saved jobs and their run history (database tables of their own).
//! - `run`: running one job through the normal answer pipeline, with its own tool allow-list (`toolbox`), and the loop that
//!   notices what is due.
//! - `folders`: finding new and finished files. `digest`: the morning digest template. `commands`: the app's side.

pub mod commands;
pub mod digest;
pub mod folders;
pub mod model;
pub mod run;
pub mod toolbox;

/// Jobs never reach the internet while the offline switch is on.
pub fn net_allowed() -> bool {
    !crate::offline::is_offline()
}
