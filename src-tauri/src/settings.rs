use std::collections::HashMap;
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::error::AppResult;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Mode {
    Fast,
    Auto,
    Deep,
    Extended,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ThinkingPref {
    Auto,
    On,
    Off,
}

/// What BYTE favours when it recommends a model version.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SpeedPref {
    /// Faster answers; accepts a smaller or more compressed model.
    Speed,
    #[default]
    Balanced,
    /// The smartest model that fits, even if it's slower.
    Quality,
}

/// Engine settings measured to be fastest for one model on this Mac.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(default, rename_all = "camelCase")]
pub struct Tuning {
    /// Use the Speed boost helper.
    pub boost: bool,
    /// Keep the conversation memory (KV cache) at full precision instead of 8-bit.
    pub kv_f16: bool,
    /// Tokens processed per GPU batch while reading the prompt.
    pub ubatch: u32,
    /// Measured speeds with these settings.
    pub tokens_per_sec: f64,
    pub prompt_per_sec: f64,
    /// Chip it was measured on (re-tune on another Mac).
    pub chip: String,
    pub tested_at: i64,
    pub flash_attn: bool,
    pub draft_n_max: u32,
    pub draft_p_min: f32,
    /// Whether the thorough tune (more settings, ~5 minutes) was run.
    pub thorough: bool,
    /// Which kind of helper the look-ahead was tuned for.
    pub helper_kind: crate::models::HelperKind,
    /// Repeated-text guessing (llama.cpp ngram-mod) won.
    pub ngram: bool,
}

impl Default for Tuning {
    fn default() -> Self {
        Tuning {
            boost: false,
            kv_f16: false,
            ubatch: 512,
            tokens_per_sec: 0.0,
            prompt_per_sec: 0.0,
            chip: String::new(),
            tested_at: 0,
            flash_attn: true,
            draft_n_max: 16,
            draft_p_min: 0.75,
            thorough: false,
            helper_kind: crate::models::HelperKind::Draft,
            ngram: false,
        }
    }
}

/// Persistent user settings. Unknown or missing fields fall back to defaults so
/// older settings files keep loading after upgrades.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct Settings {
    pub onboarding_complete: bool,
    pub active_model: Option<String>,
    /// Context window override in tokens. `None` lets the RAM planner decide.
    pub context_size: Option<u32>,
    pub default_mode: Mode,
    pub thinking: ThinkingPref,
    pub theme: String,
    pub accent: Option<String>,
    pub font_scale: f32,
    pub density: String,
    pub show_stats: bool,
    /// Let BYTE search and read the web when a question needs it.
    pub web_search: bool,
    /// With web on: "auto" (search when a question needs it) or "always"
    /// (search for every real question).
    #[serde(default = "default_web_mode")]
    pub web_mode: String,
    /// What BYTE calls the user (asked during setup).
    pub user_name: Option<String>,
    /// The user's town, for "near me" questions ("Pittsburgh, PA"). Never guessed.
    #[serde(default)]
    pub home_place: Option<String>,
    /// How far research goes: 0 Normal, 1 More, 2 Max (more pages and searches).
    #[serde(default)]
    pub research_depth: u8,
    /// Where to fetch model catalog updates (default: the BYTE repository).
    pub catalog_url: Option<String>,
    /// Free-form "About me" the user writes; included in every conversation.
    pub about_me: Option<String>,
    /// Use saved memories in answers and let BYTE suggest new ones.
    pub memory_enabled: bool,
    /// Knowledge base module: index the chosen folders and let BYTE search
    /// them ("My files" in the chat box). Off = no indexing, no tool.
    pub kb_enabled: bool,
    /// Kitchen module: recipes, meal plans and the recipe box.
    #[serde(default = "yes")]
    pub kitchen_enabled: bool,
    /// Recipe measures: "us" (cups, spoons, °F; the default) or "metric" (g, mL, °C).
    #[serde(default = "default_units")]
    pub measure_units: String,
    /// Web agent module: BYTE may use a hidden browser for the user (open, click, fill forms with approval).
    #[serde(default = "yes")]
    pub web_agent_enabled: bool,
    // --- track:browser begin --- Settings → Browser (browserroutes.rs, browserpolicy.rs); all off or empty by default
    /// Watch BYTE browse: the web agent's window is shown with a "BYTE is browsing" strip and a Stop button.
    pub browser_visible: bool,
    /// Drive Edge/Chrome (a separate profile folder BYTE makes) through the DevTools protocol instead of the built-in window.
    pub browser_drive_real: bool,
    /// "Look up the latest on X" opens several sources live in the browser (explicit "in the browser" asks always do).
    pub browser_live_research: bool,
    /// Sites the browser may open (empty = any public site) and sites it never opens (always wins).
    pub browser_sites: crate::browserpolicy::SitePolicy,
    /// Most browser steps in one answer; 0 = the built-in default.
    pub browser_max_steps: u32,
    // --- track:browser end ---
    /// Review summaries ("reviews of X", "is X worth it").
    #[serde(default = "yes")]
    pub reviews_enabled: bool,
    /// Price compare ("cheapest X", "where to buy X").
    #[serde(default = "yes")]
    pub prices_enabled: bool,
    /// Spoiler-free game hints ("stuck on … in <game>").
    #[serde(default = "yes")]
    pub game_hints_enabled: bool,
    /// Check cited answers against their sources (Deep, Extended, fact-check).
    #[serde(default = "yes")]
    pub self_check: bool,
    /// Three drafts and a majority vote for hard questions (Deep, Extended).
    #[serde(default = "yes")]
    pub best_of_three: bool,
    /// Study tools: flashcards, quizzes, tutor mode, the Study panel.
    #[serde(default = "yes")]
    pub study_enabled: bool,
    /// Photo helper: a small vision model describes photos for models that can't see them.
    #[serde(default = "yes")]
    pub photo_helper: bool,
    // --- track:screen begin --- (screen/): everything that looks at the screen is off until the person switches it on.
    /// "What do you think about this?": look at the window in front once, when asked.
    #[serde(default)]
    pub screen_ask: bool,
    /// Programs and window titles (parts of their names) that are never looked at.
    #[serde(default = "crate::screen::policy::default_excludes")]
    pub screen_exclude_apps: Vec<String>,
    /// Keep the last few seconds of the screen in memory only (never saved), for "where did I go wrong?".
    #[serde(default)]
    pub replay_buffer: bool,
    #[serde(default = "crate::screen::config::default_seconds")]
    pub replay_seconds: u32,
    #[serde(default = "crate::screen::config::default_fps")]
    pub replay_fps: u32,
    /// Game advisor: "how's my city doing?" looks at the screen and says what is wrong.
    #[serde(default)]
    pub game_advisor: bool,
    /// "Read this page" / "read the selected text" aloud.
    #[serde(default)]
    pub read_it: bool,
    // --- track:screen end ---
    // --- track:helpers begin --- (helpers/): everyday helpers. Each has its own switch, off until switched on, and is device-local.
    /// Where the helpers' questions go: "local" (this computer, the default), "cloud" or "auto". Text from the screen is
    /// never sent to the cloud unless this says "cloud".
    #[serde(default = "crate::helpers::default_ask_where")]
    pub helper_ask_where: String,
    /// Region screenshot: a hotkey draws a box; only that box is read, in memory.
    #[serde(default)]
    pub helper_region: bool,
    #[serde(default = "crate::helpers::default_region_keys")]
    pub helper_region_keys: String,
    /// "Explain this": a hotkey over selected text shows a short explanation at the pointer.
    #[serde(default)]
    pub helper_explain: bool,
    #[serde(default = "crate::helpers::default_explain_keys")]
    pub helper_explain_keys: String,
    /// A new error or installer window shows "BYTE can explain this" on the tray icon (Windows).
    #[serde(default)]
    pub helper_dialogs: bool,
    /// A toast offers the useful action when a tracking number, address, link or code is copied.
    #[serde(default)]
    pub helper_smart_paste: bool,
    /// Dictate anywhere: hold a key, speak, the words are typed into the focused app (Windows).
    #[serde(default)]
    pub helper_dictate: bool,
    #[serde(default = "crate::helpers::dictate::default_keys")]
    pub helper_dictate_keys: String,
    /// "Look at this page and help" in the tray menu, and the address-bar reader.
    #[serde(default)]
    pub helper_page: bool,
    #[serde(default = "crate::helpers::page::default_keys")]
    pub helper_page_keys: String,
    /// Meeting assistant: "Record a meeting" in the tray menu (only when clicked), written up as a note.
    #[serde(default)]
    pub helper_meeting: bool,
    /// Longest meeting recording in minutes; 0 means no limit.
    #[serde(default = "crate::helpers::meeting::default_minutes")]
    pub helper_meeting_minutes: u32,
    /// Focus mode: a timed session with notifications off (tray menu).
    #[serde(default)]
    pub helper_focus: bool,
    /// Session length in minutes; 0 means no limit.
    #[serde(default = "crate::helpers::focus::default_minutes")]
    pub helper_focus_minutes: u32,
    /// Minutes between break nudges; 0 means none.
    #[serde(default = "crate::helpers::focus::default_break")]
    pub helper_focus_break: u32,
    /// Say the plan out loud when a session starts and ends.
    #[serde(default = "yes")]
    pub helper_focus_speak: bool,
    /// Downloads tidy-up: offers to sort loose files into folders, with Undo.
    #[serde(default)]
    pub helper_tidy: bool,
    /// Recipes: export a routine as a file, import one after a safety review.
    #[serde(default)]
    pub helper_recipes: bool,
    /// Home Assistant (local): lights and plugs by voice. The access token is in the credential store, not here.
    #[serde(default)]
    pub helper_ha: bool,
    #[serde(default)]
    pub helper_ha_url: String,
    /// Game companion: "Review my last play" and "How is my game going?" in the tray menu (local only).
    #[serde(default)]
    pub helper_game: bool,
    /// Voice navigation (accessibility): "go back", "next tab", "click Send" (needs PC control).
    #[serde(default)]
    pub helper_nav: bool,
    /// Read aloud with follow-along highlighting (selected text, a shortcut).
    #[serde(default)]
    pub helper_read: bool,
    #[serde(default = "crate::helpers::readaloud::default_keys")]
    pub helper_read_keys: String,
    /// "slow", "normal" or "fast".
    #[serde(default = "crate::helpers::readaloud::default_speed")]
    pub helper_read_speed: String,
    /// Live captions and "what did they just say" (the sound the computer plays; last 30 s, memory only).
    #[serde(default)]
    pub helper_captions: bool,
    /// Your own wake phrase instead of "Hey BYTE" (empty = "Hey BYTE").
    #[serde(default)]
    pub helper_wake_phrase: String,
    /// Page tools from the tray menu: to-do, note, tables, compare tabs.
    #[serde(default)]
    pub helper_pagetools: bool,
    /// Email helper: summary and three drafts for a selected thread.
    #[serde(default)]
    pub helper_email: bool,
    /// Product helper: price and review research with sources.
    #[serde(default)]
    pub helper_product: bool,
    /// Form helper: fill name/email/phone/address boxes from a saved profile (needs PC control). Never passwords or cards.
    #[serde(default)]
    pub helper_form: bool,
    /// Fix my writing in place, explain code, show maths steps, translate (selected text, a shortcut).
    #[serde(default)]
    pub helper_rewrite: bool,
    #[serde(default = "crate::helpers::rewrite::default_keys")]
    pub helper_rewrite_keys: String,
    /// Explorer right-click "Ask BYTE" on files (adds a per-user entry under HKCU).
    #[serde(default)]
    pub helper_explorer: bool,
    /// Searchable memory of what was on screen: text only, local only, off by default.
    #[serde(default)]
    pub helper_screenmem: bool,
    /// Minutes between looks.
    #[serde(default = "crate::helpers::screenmem::default_every")]
    pub helper_screenmem_every: u32,
    /// Days kept; 0 means until cleared.
    #[serde(default = "crate::helpers::screenmem::default_days")]
    pub helper_screenmem_days: u32,
    /// Programs and window titles this memory never reads (on top of the never-look-at list).
    #[serde(default)]
    pub helper_screenmem_exclude: Vec<String>,
    // --- track:helpers end ---
    /// The writing studio (✍️): rewrite, expand, shorten, tone, grammar.
    #[serde(default = "yes")]
    pub writing_enabled: bool,
    /// The user's writing style for "Write like me" (learned from their samples; editable).
    #[serde(default)]
    pub writing_style: String,
    /// Advanced tuning per model key ("id:quant").
    #[serde(default)]
    pub model_overrides: std::collections::HashMap<String, ModelOverride>,
    /// Below 20% battery and unplugged: Deep/Extended answer like Auto, and thinking is short.
    #[serde(default = "yes")]
    pub battery_saver: bool,
    /// ⌥⌘B: select text in any app and open it in the writing studio (Mac control).
    #[serde(default = "yes")]
    pub selection_hotkey: bool,
    /// Keep a history of copied text (off until switched on; secrets skipped).
    #[serde(default)]
    pub clipboard_history: bool,
    /// Mac control: BYTE uses Notes, Reminders, Calendar, Music and settings when asked (macOS).
    #[serde(default = "yes")]
    pub mac_control: bool,
    /// Windows: BYTE may propose a PowerShell command for a question about the PC, and runs it only
    /// after the person presses "Do it" (terminal.rs, terminal_ps.rs). Off until switched on. (On a
    /// Mac the same helper is part of Mac control.)
    #[serde(default)]
    pub terminal_enabled: bool,
    /// BYTE's to-do list, reminders and scheduled questions (tasks.rs, scheduler.rs).
    #[serde(default = "yes")]
    pub tasks_enabled: bool,
    /// Topics the daily briefing follows (a few headlines each; needs the web).
    #[serde(default)]
    pub briefing_topics: Vec<String>,
    /// News feeds and page watchers (feeds.rs, watchers.rs; needs the web).
    #[serde(default = "yes")]
    pub watch_enabled: bool,
    /// Automations and multi-step runs (automations.rs).
    #[serde(default = "yes")]
    pub automations_enabled: bool,
    /// Trackers: packages, bills and subscriptions, birthdays, maintenance (trackers.rs).
    #[serde(default = "yes")]
    pub trackers_enabled: bool,
    /// Connectors (connectors/): each is also off until set up.
    #[serde(default = "yes")]
    pub connectors_enabled: bool,
    /// The Obsidian vault folder (connectors/obsidian.rs).
    #[serde(default)]
    pub obsidian_vault: Option<String>,
    /// The Notion page new pages go under (its id; the secret is in the Keychain).
    #[serde(default)]
    pub notion_parent: Option<String>,
    /// Open BYTE (in the background) when the user logs in.
    #[serde(default)]
    pub open_at_login: bool,
    /// Closing the window keeps BYTE running (schedules and watchers go on); ⌘Q quits.
    #[serde(default = "yes")]
    pub keep_running: bool,
    /// Quick Ask: a small window from anywhere on the global shortcut below (quick.rs).
    #[serde(default = "yes")]
    pub quick_ask: bool,
    /// Quick Ask's shortcut, in Tauri's syntax ("Alt+Space" is ⌥Space).
    #[serde(default = "default_quick_keys")]
    pub quick_ask_keys: String,
    /// The selection hotkey's keys ("Alt+Super+KeyB" is ⌥⌘B).
    #[serde(default = "default_selection_keys")]
    pub selection_keys: String,
    /// BYTE's icon in the menu bar (click: Quick Ask; menu: show BYTE, quit).
    #[serde(default = "yes")]
    pub menu_bar_icon: bool,
    /// Voice input: the mic button, holding Space, and transcribing audio files (voice.rs).
    #[serde(default = "yes")]
    pub voice_enabled: bool,
    /// Which speech model to use ("turbo" or "base-en", voice::MODELS).
    #[serde(default = "default_voice_model")]
    pub voice_model: String,
    /// Read answers aloud (speech.rs, macOS voices).
    #[serde(default)]
    pub read_aloud: bool,
    /// BYTE's own voice (voices.rs: "<package>/<speaker>"), used once that voice is downloaded.
    #[serde(default = "default_byte_voice")]
    pub byte_voice: String,
    /// How BYTE talks (prompt.rs personality sliders).
    #[serde(default)]
    pub personality: crate::prompt::Personality,
    /// Notes (notes.rs) and the web clipper.
    #[serde(default = "yes")]
    pub notes_enabled: bool,
    /// Your own themes (Settings → Appearance): `{name, colors: {bg, panel, border, text, accent}}`, checked in the UI.
    #[serde(default)]
    pub custom_themes: Vec<serde_json::Value>,
    /// "auto" follows macOS's Reduce motion; "reduce" turns animations down everywhere.
    #[serde(default = "default_motion")]
    pub reduce_motion: String,
    /// Check once a day whether a newer BYTE is out (updater.rs).
    #[serde(default = "yes")]
    pub update_check: bool,
    /// The Messages inbox (messages.rs): read texts you receive (needs Full Disk Access). Off by default.
    #[serde(default)]
    pub messages_inbox: bool,
    /// A notification when a text arrives (while the inbox is on).
    #[serde(default = "yes")]
    pub messages_notify: bool,
    /// Soft sounds when an answer is ready and for reminders.
    #[serde(default)]
    pub sounds: bool,
    /// Kids mode (kids.rs): a simple BYTE with no web, Mac control or files; leaving needs the PIN.
    #[serde(default)]
    pub kids_mode: bool,
    /// The kids-mode PIN, salted and hashed (kids.rs). Never the PIN itself.
    #[serde(default)]
    pub kids_pin: Option<String>,
    /// Backups (backup.rs): a folder other than iCloud Drive's BYTE Backups.
    #[serde(default)]
    pub backup_dir: Option<String>,
    /// Back up every week (with the passphrase remembered in the Keychain).
    #[serde(default)]
    pub backup_auto: bool,
    /// Include the notes folder in backups.
    #[serde(default = "yes")]
    pub backup_include_notes: bool,
    /// When the last backup was made (Unix ms).
    #[serde(default)]
    pub last_backup: Option<i64>,
    /// Delete unpinned chats not used for this many days (0 = keep them).
    #[serde(default)]
    pub auto_delete_days: u32,
    /// The offline switch (offline.rs): nothing reaches the internet while on.
    #[serde(default)]
    pub offline: bool,
    /// Lock BYTE (lock.rs): Touch ID or the Mac password to open it.
    #[serde(default)]
    pub lock_enabled: bool,
    /// Lock after this many idle minutes (0 = only when BYTE opens).
    #[serde(default = "default_lock_after")]
    pub lock_after_minutes: u32,
    /// Where notes are kept (None = ~/Documents/BYTE/Notes).
    #[serde(default)]
    pub notes_dir: Option<String>,
    /// How BYTE speaks: "calm", "natural" or "lively" (pace and pauses).
    #[serde(default = "default_speech_style")]
    pub speech_style: String,
    /// Where BYTE's voice is made: "mac", "cloud" or "auto" (the cloud when it's connected and has voices).
    #[serde(default = "default_voice_where")]
    pub voice_where: String,
    /// The BYTE Cloud voice to use (from the cloud's voice list; "" = its default).
    #[serde(default)]
    pub cloud_voice: String,
    /// The macOS voice ("" = the system's default voice).
    #[serde(default)]
    pub speech_voice: String,
    /// "slow", "normal" or "fast".
    #[serde(default = "default_speech_speed")]
    pub speech_speed: String,
    /// Listen for "Hey BYTE" (wake.rs; off until switched on).
    #[serde(default)]
    pub wake_word: bool,
    // --- track:voice begin ---
    /// A small always-on-top pill shows when BYTE has heard the phrase and is listening / thinking / speaking (voicectl.rs).
    #[serde(default = "default_true_voice")]
    pub wake_indicator: bool,
    /// The sound after "Hey BYTE": "system" (the PC's own), "soft" (a short quiet tone) or "off".
    #[serde(default = "default_wake_chime")]
    pub wake_chime: String,
    /// A sentence that clearly isn't finished ("and then", "um") gets about one more second before the turn ends.
    #[serde(default = "default_true_voice")]
    pub wait_until_done: bool,
    /// Talking over BYTE: "off", "headphones" (any speech stops BYTE) or "speakers" (echo cancelled first, stricter).
    #[serde(default = "default_barge_in")]
    pub barge_in: String,
    /// After "Hey BYTE" only that voice is followed up on: "off", "normal" or "strict".
    #[serde(default = "default_voice_focus")]
    pub voice_focus: String,
    /// A global key that starts a listening turn without the phrase ("" = none; Ctrl+Alt+Space is the suggestion).
    #[serde(default)]
    pub talk_hotkey: String,
    // --- track:voice end ---
    // --- track:wakemodel begin ---
    /// Who spots "Hey BYTE" first: "quick" (a tiny detector hears every moment of audio and Whisper only confirms its guesses) or
    /// "careful" (Whisper listens to every speech burst, as before). A phrase of your own always uses careful (wakemodel.rs).
    #[serde(default = "default_wake_detector")]
    pub wake_detector: String,
    // --- track:wakemodel end ---
    /// Where the model's weights live: "auto", "gpu" (graphics memory only), "gpuRam" (graphics + system memory) or "cpu" (processor only).
    /// (see system::Placement)
    /// Show the Quick Ask window when "Hey BYTE" is heard. Off: BYTE listens and answers out loud with nothing on screen.
    #[serde(default = "default_memory_mode")]
    pub memory_mode: String,
    #[serde(default = "default_wake_show_window")]
    pub wake_show_window: bool,
    /// How BYTE talks beyond the sliders: "" (just the sliders), "casual", "direct", "unhinged" or "auto" (style.rs).
    #[serde(default)]
    pub talk_mode: String,
    /// Who may ask BYTE for things by voice once voices have been taught (voiceprints.rs): "off", "actions" or "all".
    #[serde(default = "default_voice_lock")]
    pub voice_lock: String,
    /// The person (an adult) allows swearing: required for Unhinged, and for Auto to mirror their own swearing (style.rs).
    #[serde(default)]
    pub allow_swearing: bool,
    /// Where "Hey BYTE" answers come from: "local" (the model on this PC), "cloud" (BYTE Cloud) or "auto" (the cloud while
    /// a game is running or the graphics card is busy, this PC otherwise; busy.rs).
    #[serde(default = "default_hey_engine")]
    pub hey_byte_engine: String,
    /// Label who's speaking in recordings (speakers.rs; needs its small models).
    #[serde(default = "yes")]
    pub voice_speakers: bool,
    /// The spoken language ("auto", or a code like "en", "es").
    #[serde(default = "default_voice_language")]
    pub voice_language: String,
    /// Mac upkeep: storage clean-up, health checks, uninstalling apps, login items (macOS).
    #[serde(default = "yes")]
    pub mac_upkeep: bool,
    /// "Translate … into …" in chat: part by part, for long texts, files and pages.
    #[serde(default = "yes")]
    pub translate_enabled: bool,
    /// The job search tracker (💼).
    #[serde(default = "yes")]
    pub jobs_enabled: bool,
    /// Custom assistants (🤖).
    #[serde(default = "yes")]
    pub assistants_enabled: bool,
    /// Reuse the answer to a question asked (almost exactly) in the last week
    /// (needs the search-by-meaning model; see answer_cache.rs).
    pub answer_cache: bool,
    /// Models loaded alongside the main one; reloaded at launch.
    pub loaded_alongside: Vec<String>,
    /// Models BYTE already said "a better model fits your Mac" about (said once per model).
    #[serde(default)]
    pub better_model_hint_for: Vec<String>,
    /// Speculative decoding with a small same-family helper model.
    pub speed_boost: bool,
    pub speed_pref: SpeedPref,
    /// Measure and apply the fastest engine settings the first time a model loads.
    pub auto_tune: bool,
    /// Measured best settings per model key ("id:quant").
    pub tuning: HashMap<String, Tuning>,
    /// A BYTE cloud key is saved in the Keychain (the key itself is never here).
    pub cloud_connected: bool,
    /// Cloud address; `None` = the default in `cloud::DEFAULT_BASE`.
    pub cloud_base_url: Option<String>,
    /// The account as `GET /api/auth/me` last described it (tier, modes, budgets).
    pub cloud_account: Option<serde_json::Value>,
    /// Answer with the BYTE cloud instead of the model on this Mac.
    pub use_cloud: bool,
    /// Cloud mode id last chosen (one of the account's modes).
    pub cloud_mode: Option<String>,
    /// Which workspace the sidebar shows: "local" (this Mac), "cloud" or "both".
    pub workspace: String,
    /// How BYTE handles commands: "smart" (default), "always" or "chat" (intentgate.rs).
    #[serde(default = "default_action_mode")]
    pub action_mode: String,
    // --- track:house begin ---
    /// Plain-words disk hints before a download and when the disk is low (house.rs: diskhints).
    pub disk_hints: bool,
    /// While a game runs: answer from the cloud, stay quiet, start nothing heavy (gamepolicy.rs).
    pub game_aware: bool,
    /// "Hey BYTE" stays off while another program is using the microphone for a call (meeting.rs).
    pub meeting_quiet: bool,
    /// Say "your model finished downloading" and long jobs finishing out loud (spokennotice.rs).
    pub spoken_notices: bool,
    /// "Where did I leave off" recap and voice notes (houseroutes.rs).
    pub recap: bool,
    /// A daily spoken morning briefing at `scheduled_briefing_time` (housebrief.rs).
    pub scheduled_briefing: bool,
    pub scheduled_briefing_time: String,
    // --- track:house end ---
    // --- track:control begin ---
    /// Let BYTE operate this PC by voice (control/): read the front window, click, type, open and close programs. Off until chosen.
    pub pc_control: bool,
    /// A downloaded model key ("id:quant") that does the controlling; empty = the model chosen for chat.
    pub control_model: String,
    /// While a game runs, the control model works on the processor and system memory only, so the game keeps the card.
    #[serde(default = "yes")]
    pub control_game_ram: bool,
    /// Programs BYTE may operate without asking (lower-case names). Empty = ask each time.
    pub control_allowed_apps: Vec<String>,
    /// Ask before risky steps (shutting down, closing unsaved work, deleting, sending, buying).
    #[serde(default = "yes")]
    pub control_confirm_risky: bool,
    /// How long one PC-control run may take: "90s" (default), "3m", "10m", "30m", "1h" or "unlimited" (control/executor.rs `run_limit_ms`).
    #[serde(default = "default_control_run_limit")]
    pub control_run_limit: String,
    /// Never show an approval card for PC control (off by default; program files and other refused things stay refused).
    #[serde(default)]
    pub control_never_ask: bool,
    /// Steps allowed in one plan: "12" (default), "25", "50", "100" or "unlimited".
    #[serde(default = "default_control_max_steps")]
    pub control_max_steps: String,
    /// Times a run may re-plan after a step fails: "2" (default), "5", "10" or "unlimited".
    #[serde(default = "default_control_max_replans")]
    pub control_max_replans: String,
    /// Voice routines: a spoken phrase that runs a list of PC steps (control/routines.rs).
    pub routines_enabled: bool,
    /// "Find the invoice from last week": search the chosen folders and show the file in File Explorer.
    pub find_files: bool,
    /// The folders "find files" may search (nothing else is read).
    pub find_folders: Vec<String>,
    /// "How's my PC?": a spoken status from the PC's own numbers.
    #[serde(default = "yes")]
    pub pc_status: bool,
    /// Guided help (control/guide.rs): a how-to question about the window in front gets an offer to ring each button in turn, or do it.
    pub guided_help: bool,
    // --- track:control end ---
    // --- track:accuracy begin ---
    /// Where PC-control plans are made: "local" (default), "cloud" or "auto" (this PC's model when it is big enough, else BYTE Cloud if allowed below).
    #[serde(default = "default_control_plan_where")]
    pub control_plan_where: String,
    /// Let planning use BYTE Cloud (sends the goal and the names of the buttons in the window). Off until chosen.
    pub control_cloud_planning: bool,
    /// The person confirmed, once, what cloud planning sends.
    pub control_cloud_confirmed: bool,
    /// Per-program notes: after a run works, remember which buttons did it and use that as a hint next time. Off until chosen.
    pub control_app_notes: bool,
    /// List what BYTE would do instead of doing it.
    pub control_dry_run: bool,
    /// After a stopped run, offer to save what the person did instead as a note.
    pub control_learn: bool,
    // --- track:accuracy end ---
    /// Test log: record what "Hey BYTE" heard (words, verdict, timing) in the log, for troubleshooting. Off by default; device-local.
    pub test_log: bool,
    /// After a "Hey BYTE" answer: "off" (no follow-up listening), "once" (one short follow-up, then stop: the default) or "chain" (keep listening until you say thanks).
    pub hey_byte_follow_up: String,
    // --- track:found begin --- all off by default, device-local (not in the profile sync)
    /// Laptop mode: on battery, pause "Hey BYTE" listening and the screen replay, and suggest a smaller model (found/power.rs).
    pub laptop_mode: bool,
    /// Show a small always-on-top popup for an approval when BYTE's window is not in front (found/approval.rs).
    pub approval_popup: bool,
    /// Also speak the approval question and accept a spoken yes or no (needs Hey BYTE on). Off by default.
    pub approval_voice: bool,
    /// Keep the visible BYTE browsing window open after a task and list the pages in the chat as links (found/research.rs).
    pub browser_keep_pages: bool,
    /// "Keep researching until I'm back": "off" (default), "15m", "1h", "3h", "8h" or "unlimited".
    #[serde(default = "default_until_back")]
    pub browser_research_until_back: String,
    /// Drive the person's own Edge profile instead of BYTE's separate one (strong warning, off by default).
    pub browser_own_edge: bool,
    /// A short summary of what BYTE did yesterday (counts and times only), in the briefing and in Settings (found/summary.rs).
    pub daily_summary: bool,
    /// When a "keep researching" job ends, close its Edge tabs instead of leaving them for the person (off by default).
    pub browser_close_tabs_when_back: bool,
    // --- track:found end ---
    // --- track:autonomy begin ---
    /// How much BYTE asks while controlling the PC: "ask" (risky steps), "contract" (only outside a run contract) or "never".
    #[serde(default = "default_control_autonomy")]
    pub control_autonomy: String,
    /// Autonomous mode (jobs that run stage after stage). Off until the person has read and accepted the notice.
    pub auto_mode: bool,
    /// The person read and accepted the autonomous-mode notice.
    pub auto_accepted: bool,
    /// The global key that stops a running job.
    #[serde(default = "default_auto_stop_key")]
    pub auto_stop_key: String,
    /// Pause a job when the person uses the mouse or keyboard (best effort).
    pub auto_pause_on_input: bool,
    /// Keep the PC awake while a job runs.
    #[serde(default = "yes")]
    pub auto_keep_awake: bool,
    /// A notification when a job waits for an answer or ends.
    #[serde(default = "yes")]
    pub auto_notify: bool,
    /// Read the morning report aloud when a job ends.
    pub auto_speak_report: bool,
    /// Stages in one job: "10" (default), "25", "50" or "unlimited".
    #[serde(default = "default_auto_max_stages")]
    pub auto_max_stages: String,
    /// "Watch me do this once": record the person's own steps as a routine. Off until chosen.
    pub macro_recorder: bool,
    // --- track:autonomy end ---
}

fn valid_theme(t: &serde_json::Value) -> bool {
    let hex = |v: &serde_json::Value| {
        v.as_str().is_some_and(|s| {
            let h = s.strip_prefix('#').unwrap_or("");
            matches!(h.len(), 3 | 6) && h.chars().all(|c| c.is_ascii_hexdigit())
        })
    };
    let name_ok = t["name"].as_str().is_some_and(|n| !n.trim().is_empty() && n.chars().count() <= 40);
    name_ok && ["bg", "panel", "border", "text", "accent"].iter().all(|k| hex(&t["colors"][*k]))
}

// --- track:found begin ---
fn default_until_back() -> String {
    "off".into()
}
// --- track:found end ---

fn default_motion() -> String {
    "auto".into()
}

fn default_lock_after() -> u32 {
    15
}

impl Default for Settings {
    fn default() -> Self {
        Settings {
            onboarding_complete: false,
            active_model: None,
            context_size: None,
            default_mode: Mode::Auto,
            thinking: ThinkingPref::Auto,
            theme: "midnight".into(),
            accent: None,
            font_scale: 1.0,
            density: "comfortable".into(),
            show_stats: true,
            web_search: true,
            web_mode: default_web_mode(),
            user_name: None,
            home_place: None,
            research_depth: 0,
            catalog_url: None,
            about_me: None,
            memory_enabled: true,
            kb_enabled: true,
            kitchen_enabled: true,
            measure_units: default_units(),
            web_agent_enabled: true,
            // --- track:browser begin ---
            browser_visible: false,
            browser_drive_real: false,
            browser_live_research: false,
            browser_sites: Default::default(),
            browser_max_steps: 0,
            // --- track:browser end ---
            reviews_enabled: true,
            prices_enabled: true,
            game_hints_enabled: true,
            self_check: true,
            best_of_three: true,
            study_enabled: true,
            photo_helper: true,
            // --- track:screen begin ---
            screen_ask: false,
            screen_exclude_apps: crate::screen::policy::default_excludes(),
            replay_buffer: false,
            replay_seconds: crate::screen::config::default_seconds(),
            replay_fps: crate::screen::config::default_fps(),
            game_advisor: false,
            read_it: false,
            // --- track:screen end ---
            // --- track:helpers begin ---
            helper_ask_where: crate::helpers::default_ask_where(),
            helper_region: false,
            helper_region_keys: crate::helpers::default_region_keys(),
            helper_explain: false,
            helper_explain_keys: crate::helpers::default_explain_keys(),
            helper_dialogs: false,
            helper_smart_paste: false,
            helper_dictate: false,
            helper_dictate_keys: crate::helpers::dictate::default_keys(),
            helper_page: false,
            helper_page_keys: crate::helpers::page::default_keys(),
            helper_meeting: false,
            helper_meeting_minutes: crate::helpers::meeting::default_minutes(),
            helper_focus: false,
            helper_focus_minutes: crate::helpers::focus::default_minutes(),
            helper_focus_break: crate::helpers::focus::default_break(),
            helper_focus_speak: true,
            helper_tidy: false,
            helper_recipes: false,
            helper_ha: false,
            helper_ha_url: String::new(),
            helper_game: false,
            helper_nav: false,
            helper_read: false,
            helper_read_keys: crate::helpers::readaloud::default_keys(),
            helper_read_speed: crate::helpers::readaloud::default_speed(),
            helper_captions: false,
            helper_wake_phrase: String::new(),
            helper_pagetools: false,
            helper_email: false,
            helper_product: false,
            helper_form: false,
            helper_rewrite: false,
            helper_rewrite_keys: crate::helpers::rewrite::default_keys(),
            helper_explorer: false,
            helper_screenmem: false,
            helper_screenmem_every: crate::helpers::screenmem::default_every(),
            helper_screenmem_days: crate::helpers::screenmem::default_days(),
            helper_screenmem_exclude: Vec::new(),
            // --- track:helpers end ---
            writing_enabled: true,
            writing_style: String::new(),
            model_overrides: Default::default(),
            battery_saver: true,
            translate_enabled: true,
            mac_control: true,
            terminal_enabled: false,
            mac_upkeep: true,
            tasks_enabled: true,
            briefing_topics: Vec::new(),
            watch_enabled: true,
            automations_enabled: true,
            trackers_enabled: true,
            connectors_enabled: true,
            obsidian_vault: None,
            notion_parent: None,
            open_at_login: false,
            keep_running: true,
            quick_ask: true,
            quick_ask_keys: default_quick_keys(),
            selection_keys: default_selection_keys(),
            menu_bar_icon: true,
            voice_enabled: true,
            voice_model: default_voice_model(),
            voice_language: default_voice_language(),
            voice_speakers: true,
            read_aloud: false,
            speech_voice: String::new(),
            byte_voice: default_byte_voice(),
            speech_style: default_speech_style(),
            personality: Default::default(),
            notes_enabled: true,
            offline: false,
            custom_themes: Vec::new(),
            reduce_motion: default_motion(),
            sounds: false,
            update_check: true,
            messages_inbox: false,
            messages_notify: true,
            kids_mode: false,
            kids_pin: None,
            backup_dir: None,
            backup_auto: false,
            backup_include_notes: true,
            last_backup: None,
            auto_delete_days: 0,
            lock_enabled: false,
            lock_after_minutes: default_lock_after(),
            notes_dir: None,
            voice_where: default_voice_where(),
            cloud_voice: String::new(),
            speech_speed: default_speech_speed(),
            wake_word: false,
            // --- track:voice begin ---
            wake_indicator: true,
            wake_chime: default_wake_chime(),
            wait_until_done: true,
            barge_in: default_barge_in(),
            voice_focus: default_voice_focus(),
            talk_hotkey: String::new(),
            // --- track:voice end ---
            // --- track:wakemodel begin ---
            wake_detector: default_wake_detector(),
            // --- track:wakemodel end ---
            wake_show_window: default_wake_show_window(),
            memory_mode: default_memory_mode(),
            hey_byte_engine: default_hey_engine(),
            talk_mode: String::new(),
            voice_lock: default_voice_lock(),
            allow_swearing: false,
            selection_hotkey: true,
            clipboard_history: false,
            jobs_enabled: true,
            assistants_enabled: true,
            answer_cache: true,
            loaded_alongside: Vec::new(),
            better_model_hint_for: Vec::new(),
            speed_boost: true,
            speed_pref: SpeedPref::Balanced,
            auto_tune: true,
            tuning: HashMap::new(),
            cloud_connected: false,
            cloud_base_url: None,
            cloud_account: None,
            use_cloud: false,
            cloud_mode: None,
            workspace: "local".into(),
            action_mode: default_action_mode(),
            // --- track:house begin ---
            disk_hints: true,
            game_aware: true,
            meeting_quiet: true,
            spoken_notices: false,
            recap: true,
            scheduled_briefing: false,
            scheduled_briefing_time: "07:30".into(),
            // --- track:house end ---
            // --- track:control begin ---
            pc_control: false,
            control_model: String::new(),
            control_game_ram: true,
            control_allowed_apps: Vec::new(),
            control_confirm_risky: true,
            control_run_limit: default_control_run_limit(),
            control_never_ask: false,
            control_max_steps: default_control_max_steps(),
            control_max_replans: default_control_max_replans(),
            routines_enabled: false,
            find_files: false,
            find_folders: Vec::new(),
            pc_status: true,
            guided_help: false,
            // --- track:accuracy begin ---
            control_plan_where: default_control_plan_where(),
            control_cloud_planning: false,
            control_cloud_confirmed: false,
            control_app_notes: false,
            control_dry_run: false,
            control_learn: false,
            // --- track:accuracy end ---
            // --- track:control end ---
            // --- track:found begin ---
            laptop_mode: false,
            test_log: false,
            hey_byte_follow_up: "once".into(),
            approval_popup: true,
            approval_voice: false,
            browser_keep_pages: false,
            browser_research_until_back: default_until_back(),
            browser_own_edge: false,
            daily_summary: false,
            browser_close_tabs_when_back: false,
            // --- track:found end ---
            // --- track:autonomy begin ---
            control_autonomy: default_control_autonomy(),
            auto_mode: false,
            auto_accepted: false,
            auto_stop_key: default_auto_stop_key(),
            auto_pause_on_input: false,
            auto_keep_awake: true,
            auto_notify: true,
            auto_speak_report: false,
            auto_max_stages: default_auto_max_stages(),
            macro_recorder: false,
            // --- track:autonomy end ---
        }
    }
}

impl Settings {
    pub fn load(path: &Path) -> Settings {
        match std::fs::read_to_string(path) {
            Ok(text) => serde_json::from_str(&text).unwrap_or_else(|e| {
                log::warn!("settings file unreadable, using defaults: {e}");
                Settings::default()
            }),
            Err(_) => Settings::default(),
        }
    }

    /// Writes atomically (temp file + rename) so a crash never leaves a
    /// half-written settings file behind.
    pub fn save(&self, path: &Path) -> AppResult<()> {
        let tmp = path.with_extension("json.tmp");
        // --- track:found begin --- in safe mode the file keeps what the person saved (found/safe.rs)
        std::fs::write(&tmp, serde_json::to_vec_pretty(&crate::found::safe::for_disk(self)?)?)?;
        // --- track:found end ---
        std::fs::rename(&tmp, path)?;
        Ok(())
    }

    /// Applies a partial JSON update from the UI.
    pub fn merged(&self, patch: serde_json::Value) -> AppResult<Settings> {
        let mut current = serde_json::to_value(self)?;
        if let (Some(obj), serde_json::Value::Object(p)) = (current.as_object_mut(), patch) {
            for (k, v) in p {
                obj.insert(k, v);
            }
        }
        let mut next: Settings = serde_json::from_value(current)?;
        // --- track:voice begin ---
        if !matches!(next.wake_chime.as_str(), "system" | "soft" | "off") {
            next.wake_chime = default_wake_chime();
        }
        // --- track:wakemodel begin ---
        if !matches!(next.wake_detector.as_str(), "quick" | "careful") {
            next.wake_detector = default_wake_detector();
        }
        // --- track:wakemodel end ---
        if !matches!(next.barge_in.as_str(), "off" | "headphones" | "speakers") {
            next.barge_in = default_barge_in();
        }
        if !matches!(next.voice_focus.as_str(), "off" | "normal" | "strict") {
            next.voice_focus = default_voice_focus();
        }
        // --- track:voice end ---
        // --- track:helpers begin ---
        if !matches!(next.helper_ask_where.as_str(), "local" | "cloud" | "auto") {
            next.helper_ask_where = crate::helpers::default_ask_where();
        }
        if !matches!(next.helper_read_speed.as_str(), "slow" | "normal" | "fast") {
            next.helper_read_speed = crate::helpers::readaloud::default_speed();
        }
        next.helper_wake_phrase = if next.helper_wake_phrase.trim().is_empty() { String::new() } else { crate::helpers::wakeword::valid(&next.helper_wake_phrase).unwrap_or_default() };
        next.helper_screenmem_every = next.helper_screenmem_every.clamp(1, 120);
        if !crate::helpers::dictate::valid_keys(&next.helper_dictate_keys) {
            next.helper_dictate_keys = crate::helpers::dictate::default_keys();
        }
        // --- track:helpers end ---
        if !matches!(next.voice_lock.as_str(), "off" | "actions" | "all") {
            next.voice_lock = default_voice_lock();
        }
        if !crate::style::valid(&next.talk_mode) {
            next.talk_mode = String::new();
        }
        if !matches!(next.hey_byte_engine.as_str(), "local" | "cloud" | "auto") {
            next.hey_byte_engine = default_hey_engine();
        }
        if !matches!(next.workspace.as_str(), "local" | "cloud" | "both") {
            next.workspace = "local".into();
        }
        if !matches!(next.reduce_motion.as_str(), "auto" | "reduce") {
            next.reduce_motion = default_motion();
        }
        // Only well-formed themes with hex colors are kept (they become CSS values), at most 10.
        next.custom_themes.retain(valid_theme);
        next.custom_themes.truncate(10);
        Ok(next)
    }
}


fn yes() -> bool {
    true
}

fn default_byte_voice() -> String {
    crate::voices::DEFAULT_VOICE.into()
}

fn default_speech_style() -> String {
    "natural".into()
}

fn default_voice_lock() -> String {
    "off".into()
}

fn default_hey_engine() -> String {
    "auto".into()
}

// --- track:voice begin ---
fn default_true_voice() -> bool {
    true
}
fn default_wake_chime() -> String {
    "system".into()
}
// --- track:wakemodel begin ---
fn default_wake_detector() -> String {
    "careful".into()
}
// --- track:wakemodel end ---

fn default_barge_in() -> String {
    crate::bargein::DEFAULT_MODE.into()
}
fn default_voice_focus() -> String {
    // Off until the voice matching has been tuned on real voices.
    "off".into()
}
// --- track:voice end ---

fn default_voice_where() -> String {
    "mac".into()
}

/// The Mac app always opened Quick Ask for "Hey BYTE" and keeps doing so; the Windows app answers with nothing on screen.
fn default_wake_show_window() -> bool {
    cfg!(target_os = "macos")
}

fn default_control_plan_where() -> String {
    "local".into()
}

// --- track:autonomy begin ---
fn default_control_autonomy() -> String {
    "ask".into()
}

fn default_auto_stop_key() -> String {
    "Ctrl+Alt+F12".into()
}

fn default_auto_max_stages() -> String {
    "10".into()
}
// --- track:autonomy end ---

fn default_control_max_steps() -> String {
    "12".into()
}

fn default_control_max_replans() -> String {
    "2".into()
}

fn default_control_run_limit() -> String {
    "90s".into()
}

fn default_action_mode() -> String {
    "smart".into()
}

fn default_memory_mode() -> String {
    "auto".into()
}

fn default_speech_speed() -> String {
    "normal".into()
}

fn default_voice_model() -> String {
    "turbo".into()
}

fn default_voice_language() -> String {
    "auto".into()
}

pub fn default_quick_keys() -> String {
    // Alt+Space is the window menu of every Windows app, and of GNOME, KDE and most Linux window managers;
    // registering it globally would take that away (or, on Linux, fail because the window manager has it).
    if cfg!(any(windows, target_os = "linux")) { "Control+Alt+Space".into() } else { "Alt+Space".into() }
}

pub fn default_selection_keys() -> String {
    crate::selection::HOTKEY.into()
}

fn default_units() -> String {
    "us".into()
}

fn default_web_mode() -> String {
    "auto".into()
}

#[cfg(test)]
mod tests {

    #[test]
    fn custom_themes_and_motion_are_checked() {
        let good = serde_json::json!({ "name": "Night", "colors": { "bg": "#000", "panel": "#111111", "border": "#222222", "text": "#eeeeee", "accent": "#4c8dff" } });
        let bad = serde_json::json!({ "name": "Evil", "colors": { "bg": "#000", "panel": "red;}", "border": "#222", "text": "#eee", "accent": "#fff" } });
        let s = Settings::default().merged(serde_json::json!({ "customThemes": [good, bad], "reduceMotion": "wild" })).unwrap();
        assert_eq!(s.custom_themes.len(), 1);
        assert_eq!(s.custom_themes[0]["name"], "Night");
        assert_eq!(s.reduce_motion, "auto");
    }

    use super::*;

    #[test]
    fn merge_keeps_unrelated_fields() {
        let s = Settings::default();
        let m = s
            .merged(serde_json::json!({ "theme": "paper", "defaultMode": "fast" }))
            .unwrap();
        assert_eq!(m.theme, "paper");
        assert_eq!(m.default_mode, Mode::Fast);
        assert_eq!(m.thinking, ThinkingPref::Auto);
    }

    #[test]
    fn save_and_load_roundtrip() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("settings.json");
        let mut s = Settings::default();
        s.active_model = Some("qwen3-14b".into());
        s.save(&path).unwrap();
        assert_eq!(Settings::load(&path).active_model.as_deref(), Some("qwen3-14b"));
    }

    #[test]
    fn corrupt_file_falls_back_to_defaults() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("settings.json");
        std::fs::write(&path, "{not json").unwrap();
        assert!(!Settings::load(&path).onboarding_complete);
    }
}

/// Advanced tuning for one model (None/empty: the model's recommended value).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct ModelOverride {
    pub temperature: Option<f32>,
    pub top_p: Option<f32>,
    /// Tokens the model may think (-1 = no limit).
    pub thinking_budget: Option<i32>,
    /// Extra instructions added to every chat with this model.
    pub system_extra: String,
}

impl ModelOverride {
    /// Applies the sampling and thinking overrides to a turn's plan (values clamped to sane ranges).
    pub fn apply(&self, plan: &mut crate::router::TurnPlan) {
        for s in [&mut plan.profile.think, &mut plan.profile.plain] {
            if let Some(t) = self.temperature {
                s.temperature = t.clamp(0.0, 2.0);
            }
            if let Some(p) = self.top_p {
                s.top_p = p.clamp(0.05, 1.0);
            }
        }
        if let (true, Some(b)) = (plan.thinking, self.thinking_budget) {
            plan.thinking_budget = if b < 0 { -1 } else { b.clamp(64, 32_768) };
        }
    }
}

#[cfg(test)]
mod override_tests {
    use super::*;

    #[test]
    fn overrides_change_sampling_and_thinking() {
        let mut plan = crate::router::plan_turn(Mode::Deep, ThinkingPref::Auto, "Explain why the sky is blue");
        assert!(plan.thinking);
        let o = ModelOverride { temperature: Some(3.0), top_p: Some(0.5), thinking_budget: Some(10), system_extra: String::new() };
        o.apply(&mut plan);
        assert_eq!((plan.profile.think.temperature, plan.profile.plain.top_p), (2.0, 0.5), "clamped");
        assert_eq!(plan.thinking_budget, 64, "at least 64 tokens");
        let mut unlimited = crate::router::plan_turn(Mode::Auto, ThinkingPref::On, "x");
        ModelOverride { thinking_budget: Some(-1), ..Default::default() }.apply(&mut unlimited);
        assert_eq!(unlimited.thinking_budget, -1);
        let mut off = crate::router::plan_turn(Mode::Fast, ThinkingPref::Off, "x");
        let before = off;
        ModelOverride { thinking_budget: Some(900), ..Default::default() }.apply(&mut off);
        assert_eq!(off, before, "no thinking, nothing to budget");
    }

    #[test]
    fn overrides_are_saved_in_camel_case() {
        let mut s = Settings::default();
        s.model_overrides.insert("qwen3-8b:Q4_K_M".into(), ModelOverride { temperature: Some(0.3), system_extra: "Be brief.".into(), ..Default::default() });
        let v = serde_json::to_value(&s).unwrap();
        assert_eq!(v["modelOverrides"]["qwen3-8b:Q4_K_M"]["temperature"], serde_json::json!(0.3f32));
        assert_eq!(v["batterySaver"], true);
        let back: Settings = serde_json::from_value(v).unwrap();
        assert_eq!(back.model_overrides["qwen3-8b:Q4_K_M"].system_extra, "Be brief.");
    }
}
