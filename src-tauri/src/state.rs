use tokio::sync::Mutex;

use crate::chat::Generations;
use crate::engine::{Engine, Extras};
use crate::models::Downloads;
use crate::paths::Paths;
use crate::settings::Settings;

pub struct AppState {
    pub paths: Paths,
    pub settings: Mutex<Settings>,
    pub engine: Engine,
    /// Models loaded alongside the main one.
    pub extras: Extras,
    pub downloads: Downloads,
    pub generations: Generations,
    /// Client for the internet (downloads, later web search).
    pub net: reqwest::Client,
    /// Client for the local engine: no proxy, no timeout on streaming bodies.
    pub local_http: reqwest::Client,
    /// Log of every tool call (web searches, pages read, calculations).
    pub actions: crate::tools::ActionLog,
    /// Encrypted database: chats, search index, memories.
    pub db: crate::db::Db,
    /// Model catalog (built in, refreshed from the web).
    pub catalog: crate::models::CatalogStore,
}

impl AppState {
    pub fn new(paths: Paths) -> Self {
        let settings = Settings::load(&paths.settings_file);
        AppState {
            settings: Mutex::new(settings),
            engine: Engine::new(paths.root.join("engine.pid")),
            extras: Extras::new(paths.root.clone()),
            downloads: Downloads::default(),
            generations: Generations::default(),
            net: crate::tools::fetch::web_client(),
            local_http: crate::chat::local_client(),
            actions: crate::tools::ActionLog::new(paths.data.join("actions.jsonl")),
            catalog: crate::models::CatalogStore::load(paths.root.join("catalog.json")),
            db: crate::db::Db::open(&paths.data).unwrap_or_else(|e| {
                // Chats still work for this session; they just aren't kept.
                log::error!("database unavailable, chats won't be saved this session: {e}");
                crate::db::Db::open_with_key(std::path::Path::new(":memory:"), "x'00'").expect("in-memory database")
            }),
            paths,
        }
    }
}
