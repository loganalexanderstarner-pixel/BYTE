use tokio::sync::Mutex;

use crate::error::{AppError, AppResult};

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
    /// True while "Tune for this Mac" is restarting the engine.
    pub tuning: std::sync::atomic::AtomicBool,
    /// Encrypted database: chats, search index, memories.
    pub db: crate::db::Db,
    /// Model catalog (built in, refreshed from the web).
    pub catalog: crate::models::CatalogStore,
    /// Where the BYTE cloud key is kept (the macOS Keychain).
    pub secrets: Box<dyn crate::cloud::keychain::SecretStore>,
    /// The cloud key once read from the Keychain this session.
    pub cloud_key: Mutex<Option<String>>,
    /// One client for the cloud, so its connection is reused between messages.
    pub cloud_http: reqwest::Client,
    /// Embedding engine for searching files by meaning (started on demand).
    pub embedder: crate::embed::Embedder,
    /// Photo helper: a small vision model that describes photos for models that can't see (on demand).
    pub looker: crate::looker::Looker,
    /// The app, set at startup; for work that starts engines from deep inside
    /// a turn (searching the knowledge base starts the embedding engine).
    pub app: std::sync::OnceLock<tauri::AppHandle>,
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
            tuning: std::sync::atomic::AtomicBool::new(false),
            secrets: Box::new(crate::cloud::keychain::Keychain),
            cloud_key: Mutex::new(None),
            cloud_http: crate::cloud::http_client(),
            embedder: crate::embed::Embedder::new(paths.root.join("embed.pid")),
            looker: crate::looker::Looker::new(paths.root.join("looker.pid")),
            app: std::sync::OnceLock::new(),
            db: crate::db::Db::open(&paths.data).unwrap_or_else(|e| {
                // Chats still work for this session; they just aren't kept.
                log::error!("database unavailable, chats won't be saved this session: {e}");
                crate::db::Db::open_with_key(std::path::Path::new(":memory:"), "x'00'").expect("in-memory database")
            }),
            paths,
        }
    }
}

impl AppState {
    /// Keychain account for this profile's cloud key.
    pub fn cloud_account(&self) -> String {
        match self.paths.data.strip_prefix(&self.paths.root) {
            Ok(p) if !p.as_os_str().is_empty() => p.to_string_lossy().replace('\\', "/"),
            _ => "default".into(),
        }
    }

    pub async fn cloud_base(&self) -> String {
        self.settings.lock().await.cloud_base_url.clone().filter(|u| !u.trim().is_empty()).unwrap_or_else(|| crate::cloud::DEFAULT_BASE.into())
    }

    /// A client with the saved key, or an error saying how to connect.
    pub async fn cloud_client(&self) -> AppResult<crate::cloud::CloudClient> {
        let mut cached = self.cloud_key.lock().await;
        if cached.is_none() {
            *cached = self.secrets.get(&self.cloud_account())?;
        }
        let key = cached.clone().ok_or_else(|| AppError::msg("Connect BYTE Cloud first: Settings → Cloud."))?;
        drop(cached);
        Ok(crate::cloud::CloudClient::with_http(self.cloud_http.clone(), &self.cloud_base().await, &key))
    }
}
