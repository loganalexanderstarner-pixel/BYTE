use std::time::Duration;

use tokio::sync::Mutex;

use crate::chat::Generations;
use crate::engine::Engine;
use crate::models::Downloads;
use crate::paths::Paths;
use crate::settings::Settings;

pub struct AppState {
    pub paths: Paths,
    pub settings: Mutex<Settings>,
    pub engine: Engine,
    pub downloads: Downloads,
    pub generations: Generations,
    /// Client for the internet (downloads, later web search).
    pub net: reqwest::Client,
    /// Client for the local engine: no proxy, no timeout on streaming bodies.
    pub local_http: reqwest::Client,
}

impl AppState {
    pub fn new(paths: Paths) -> Self {
        let settings = Settings::load(&paths.settings_file);
        AppState {
            settings: Mutex::new(settings),
            engine: Engine::new(paths.data.join("engine.pid")),
            downloads: Downloads::default(),
            generations: Generations::default(),
            net: reqwest::Client::builder()
                .user_agent(concat!("BYTE/", env!("CARGO_PKG_VERSION"), " (macOS; local AI assistant)"))
                .connect_timeout(Duration::from_secs(20))
                .read_timeout(Duration::from_secs(60))
                .build()
                .expect("http client"),
            local_http: crate::chat::local_client(),
            paths,
        }
    }
}
