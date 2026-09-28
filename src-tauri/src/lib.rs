mod agent;
mod answer_cache;
mod backend;
mod chat;
mod chip;
mod cloud;
mod commands;
mod db;
mod decide;
mod docs;
mod embed;
mod kb;
mod kitchen;
mod engine;
mod export;
mod factcheck;
mod files;
mod ocr;
mod memory;
mod error;
mod modelcfg;
mod models;
mod paths;
mod profiles;
mod prompt;
mod research;
mod router;
mod settings;
mod speed;
mod state;
mod summarize;
mod system;
mod tools;
mod trip;
mod tune;

use tauri::{Manager, RunEvent};

use crate::state::AppState;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let app = tauri::Builder::default()
        .plugin(
            tauri_plugin_log::Builder::new()
                .level(log::LevelFilter::Info)
                .max_file_size(5_000_000)
                .build(),
        )
        .plugin(tauri_plugin_shell::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_os::init())
        .plugin(tauri_plugin_process::init())
        .plugin(tauri_plugin_opener::init())
        .setup(|app| {
            let paths = paths::Paths::resolve(app.handle())?;
            let state = AppState::new(paths);
            let active = {
                let s = state.settings.blocking_lock();
                s.active_model.clone().map(|m| (m, s.context_size))
            };
            let engine = state.engine.clone();
            engine.reap_stale();
            state.extras.reap_stale();
            state.embedder.reap_stale();
            let _ = state.app.set(app.handle().clone());
            let catalog = state.catalog.get();
            let models_dir = state.paths.models.clone();
            app.manage(state);

            // Logout, shutdown and `kill` send signals rather than quitting
            // through the menu; stop the engine so it can't outlive BYTE.
            #[cfg(unix)]
            {
                let engine = engine.clone();
                let handle = app.handle().clone();
                tauri::async_runtime::spawn(async move {
                    use tokio::signal::unix::{signal, SignalKind};
                    let (Ok(mut term), Ok(mut int), Ok(mut hup)) =
                        (signal(SignalKind::terminate()), signal(SignalKind::interrupt()), signal(SignalKind::hangup()))
                    else {
                        return;
                    };
                    tokio::select! {
                        _ = term.recv() => {}
                        _ = int.recv() => {}
                        _ = hup.recv() => {}
                    }
                    log::info!("termination signal received; stopping engine");
                    engine.kill_now();
                    handle.state::<AppState>().extras.kill_all_now();
                    handle.state::<AppState>().embedder.kill_now();
                    handle.exit(0);
                });
            }

            // Keep the knowledge base's folders up to date.
            kb::schedule(app.handle().clone());

            // Check for a newer model catalog in the background.
            {
                let handle = app.handle().clone();
                tauri::async_runtime::spawn(async move {
                    let state = handle.state::<AppState>();
                    let url = state.settings.lock().await.catalog_url.clone().unwrap_or_else(|| models::DEFAULT_CATALOG_URL.to_string());
                    match state.catalog.refresh(&state.net, &url).await {
                        Ok(true) => log::info!("model catalog updated from {url}"),
                        Ok(false) => {}
                        Err(e) => log::info!("model catalog not refreshed ({e}); using the built-in one"),
                    }
                });
            }

            // Preload the model at launch so the first answer is fast.
            let handle = app.handle().clone();
            tauri::async_runtime::spawn(async move {
                match active {
                    Some((model, ctx)) => {
                        let opts = tune::launch_opts(&handle.state::<AppState>(), &catalog, &model).await;
                        if let Err(e) = engine.start(&handle, models_dir, &catalog, &model, ctx, 0, opts).await {
                            log::warn!("engine did not start at launch: {e}");
                            return;
                        }
                        // Bring back the models that were loaded alongside it.
                        let state = handle.state::<AppState>();
                        let extra = state.settings.lock().await.loaded_alongside.clone();
                        for key in extra {
                            if let Err(e) = commands::load_extra(&handle, &state, &key).await {
                                log::warn!("couldn't reload {key} alongside the main model: {e}");
                            }
                        }
                        // First time this model runs on this Mac: find its fastest settings.
                        commands::auto_tune(&handle);
                    }
                    None => {
                        let _ = tauri::Emitter::emit(&handle, engine::STATUS_EVENT, engine::EngineStatus::NoModel);
                    }
                }
            });
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::system_info,
            commands::memory_report,
            commands::file_ingest,
            commands::kb_status,
            commands::kb_add,
            commands::kb_remove,
            commands::kb_reindex,
            commands::kb_search,
            commands::answer_cache_put,
            commands::answer_cache_clear,
            commands::doc_outline,
            commands::doc_write,
            commands::doc_save,
            commands::calendar_open,
            commands::recipes_list,
            commands::recipe_save,
            commands::recipe_delete,
            commands::app_quit,
            commands::settings_get,
            commands::settings_update,
            commands::models_list,
            commands::model_recommend,
            commands::catalog_refresh,
            commands::model_download,
            commands::model_pause,
            commands::model_delete,
            commands::model_activate,
            commands::models_loaded,
            commands::model_load,
            commands::model_unload,
            commands::chats_list,
            commands::chat_load,
            commands::chat_save,
            commands::chat_delete,
            commands::chat_update,
            commands::chats_search,
            commands::chats_import,
            commands::chats_export,
            commands::memories_list,
            commands::memory_add,
            commands::memory_update,
            commands::memory_delete,
            commands::data_wipe,
            commands::chat_autotitle,
            commands::speed_boost_info,
            commands::gpu_share_info,
            cloud::cmd::cloud_status,
            cloud::cmd::cloud_connect,
            cloud::cmd::cloud_disconnect,
            cloud::cmd::cloud_refresh,
            cloud::cmd::cloud_action,
            cloud::cmd::cloud_delete_message,
            cloud::cmd::cloud_conversations,
            cloud::cmd::cloud_import,
            cloud::cmd::cloud_get,
            cloud::cmd::cloud_post,
            cloud::cmd::cloud_delete,
            cloud::cmd::cloud_image,
            cloud::cmd::cloud_download,
            cloud::cmd::cloud_upload,
            cloud::cmd::cloud_attach,
            commands::gpu_share_set,
            commands::engine_tune,
            commands::engine_tune_all,
            commands::projects_list,
            commands::project_save,
            commands::project_delete,
            commands::profiles_list,
            commands::profile_create,
            commands::profile_rename,
            commands::profile_delete,
            commands::profile_switch,
            commands::engine_status,
            commands::engine_restart,
            commands::engine_log,
            commands::chat_send,
            commands::chat_cancel,
        ])
        .build(tauri::generate_context!())
        .expect("error while building BYTE");

    app.run(|handle, event| {
        if let RunEvent::Exit = event {
            if let Some(state) = handle.try_state::<AppState>() {
                state.engine.kill_now();
                state.extras.kill_all_now();
                state.embedder.kill_now();
            }
        }
    });
}
