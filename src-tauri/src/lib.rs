mod agent;
mod assistants;
mod answer_cache;
mod backend;
mod chat;
mod chip;
mod clipboard;
mod cloud;
mod commands;
mod db;
mod decide;
mod docs;
mod drafts;
mod games;
mod prices;
mod reviews;
mod selfcheck;
mod embed;
mod kb;
mod jobs;
mod kitchen;
mod lab;
mod macctl;
mod messages;
mod looker;
mod engine;
mod export;
mod factcheck;
mod filectl;
mod androidfs;
mod files;
mod gguf;
mod ocr;
mod memory;
mod error;
mod offline;
mod lock;
mod privacy;
mod kids;
mod backup;
#[cfg(desktop)]
mod updater;
#[cfg(mobile)]
#[path = "updater_mobile.rs"]
mod updater;
mod modelcfg;
mod models;
mod paths;
mod profiles;
mod prompt;
#[cfg(desktop)]
mod quick;
#[cfg(mobile)]
#[path = "quick_mobile.rs"]
mod quick;
mod media;
mod speakers;
mod speech;
mod tts;
mod notes;
mod mindmap;
mod board;
mod help;
mod voices;
mod wake;
mod voice;
mod quality;
mod research;
mod router;
mod selection;
mod settings;
mod speed;
mod state;
mod study;
mod terminal;
mod summarize;
mod system;
mod tools;
mod translate;
mod trip;
mod briefing;
mod scheduler;
mod tasks;
mod feeds;
mod watchers;
mod upkeep;
mod web_agent;
mod writing;
mod youtube;
mod tune;
mod units;
mod automations;
mod shortcut_make;
mod background;
mod trackers;
mod connectors;
mod dashboard;
mod bundled;
mod diagnostics;

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
        .plugin(tauri_plugin_notification::init())
        // byte:// links (Shortcuts start automations) and opening at login (background.rs).
        .plugin(tauri_plugin_deep_link::init());
    // Android: opens the content:// links the file picker returns (androidfs.rs).
    #[cfg(target_os = "android")]
    let app = app.plugin(tauri_plugin_fs::init());
    // Desktop only: signed updates, opening at login and global shortcuts. Android
    // gets its own versions (docs/ANDROID.md: in-app APK updater, Ask BYTE).
    #[cfg(desktop)]
    let app = app
        .plugin(tauri_plugin_updater::Builder::new().build())
        .plugin(tauri_plugin_autostart::Builder::new().args(["--background"]).build())
        // Global shortcuts (quick.rs): Quick Ask and the selection hotkey, keys from Settings.
        .plugin(
            tauri_plugin_global_shortcut::Builder::new()
                .with_handler(|app, shortcut, event| {
                    if event.state() == tauri_plugin_global_shortcut::ShortcutState::Pressed {
                        quick::pressed(app, shortcut);
                    }
                })
                .build(),
        );
    let app = app
        // Keep running (macOS): closing the window hides it; ⌘Q quits.
        .on_window_event(|window, event| {
            if let tauri::WindowEvent::Focused(false) = event {
                quick::on_blur(window);
            }
            if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                let keep = cfg!(target_os = "macos")
                    && window.label() == "main"
                    && window.app_handle().try_state::<AppState>().is_some_and(|s| s.settings.try_lock().map(|s| s.keep_running).unwrap_or(true));
                if keep {
                    api.prevent_close();
                    let _ = window.hide();
                }
            }
        })
        .setup(|app| {
            let paths = paths::Paths::resolve(app.handle())?;
            // A restore or "erase everything" from last time finishes before the database opens.
            if let Err(e) = backup::apply_pending(&paths.data) {
                log::error!("couldn't finish the restore/erase: {e}");
            }
            let state = AppState::new(paths);
            let engine = state.engine.clone();
            engine.reap_stale();
            state.extras.reap_stale();
            state.embedder.reap_stale();
            state.looker.reap_stale();
            // Models the user added (model lab) join the catalog.
            state.catalog.set_added(crate::lab::load(&state.paths.data.join("added_models.json")).iter().map(crate::lab::LabModel::to_catalog).collect());
            let _ = state.app.set(app.handle().clone());
            let settings_now = state.settings.blocking_lock().clone();
            offline::set(settings_now.offline);
            messages::set_enabled(settings_now.messages_inbox);
            kids::set(settings_now.kids_mode);
            let catalog = state.catalog.get();
            let models_dir = state.paths.models.clone();
            app.manage(state);
            // Global shortcuts (Quick Ask, the selection hotkey) and the menu-bar icon;
            // clipboard history checks its own setting.
            quick::apply_shortcuts(app.handle(), &settings_now);
            quick::apply_tray(app.handle(), settings_now.menu_bar_icon);
            wake::apply(app.handle(), settings_now.wake_word);
            lock::start(app.handle().clone(), settings_now.lock_enabled);
            clipboard::watch(app.handle().clone());
            // Reminders, the daily briefing and scheduled questions.
            scheduler::start(app.handle().clone());
            messages::start(app.handle().clone());
            // The window stays hidden when the login item started BYTE.
            if !background::launched_in_background() {
                background::show_main(app.handle());
            }
            {
                use tauri_plugin_deep_link::DeepLinkExt;
                let handle = app.handle().clone();
                app.deep_link().on_open_url(move |e| background::open_links(&handle, e.urls()));
                if let Ok(Some(urls)) = app.deep_link().get_current() {
                    background::open_links(app.handle(), urls);
                }
                // The login item follows the setting (it may have been removed in System Settings).
                let login = app.state::<AppState>().settings.blocking_lock().open_at_login;
                if let Err(e) = background::apply_login(app.handle(), login) {
                    log::warn!("{e}");
                }
            }

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
                    handle.state::<AppState>().looker.kill_now();
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
                let state = handle.state::<AppState>();
                let (chosen, ctx, tuned) = {
                    let s = state.settings.lock().await;
                    (s.active_model.clone(), s.context_size, s.tuning.keys().cloned().collect::<Vec<_>>())
                };
                // Use the chosen model, or another downloaded one when the choice is lost or its file is gone,
                // so "No model" only ever means nothing is downloaded (an Android restart showed it wrongly).
                let model = match models::choose_active(&catalog, &models_dir, chosen.as_deref(), &tuned) {
                    models::Startup::Use(key) => {
                        engine.set_note(format!("launch: using {key}")).await;
                        key
                    }
                    models::Startup::Switched { key, from } => {
                        engine.set_note(format!("launch: {} is not usable, switched to {key}", from.as_deref().unwrap_or("(no model chosen)"))).await;
                        log::warn!("the chosen model {from:?} is not usable; starting {key} instead");
                        let mut s = state.settings.lock().await;
                        let mut next = s.clone();
                        next.active_model = Some(key.clone());
                        if next.save(&state.paths.settings_file).is_ok() {
                            *s = next;
                        }
                        key
                    }
                    models::Startup::Missing(key) => {
                        engine.set_note(format!("launch: {key} is chosen but its file is missing or differs from the catalog; nothing else is downloaded")).await;
                        engine
                            .set_status(&handle, engine::EngineStatus::Error { message: "The model file is missing or doesn't match the catalog. Download it again in Settings → Models.".into() })
                            .await;
                        return;
                    }
                    models::Startup::NothingDownloaded => {
                        engine.set_note("launch: no chat model is downloaded").await;
                        let _ = tauri::Emitter::emit(&handle, engine::STATUS_EVENT, engine::EngineStatus::NoModel);
                        return;
                    }
                };
                let opts = tune::launch_opts(&state, &catalog, &model).await;
                if let Err(e) = engine.start(&handle, models_dir, &catalog, &model, ctx, 0, opts).await {
                    log::warn!("engine did not start at launch: {e}");
                    engine.set_note(format!("launch: {model} did not start: {e}")).await;
                    return;
                }
                // Bring back the models that were loaded alongside it.
                let extra = state.settings.lock().await.loaded_alongside.clone();
                for key in extra {
                    if let Err(e) = commands::load_extra(&handle, &state, &key).await {
                        log::warn!("couldn't reload {key} alongside the main model: {e}");
                    }
                }
                // First time this model runs on this Mac: find its fastest settings.
                commands::auto_tune(&handle);
            });
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::system_info,
            commands::memory_report,
            commands::file_ingest,
            commands::kb_status,
            commands::looker_status,
            commands::engine_live,
            lab::lab_inspect,
            lab::lab_inspect_url,
            lab::lab_add,
            lab::lab_list,
            lab::lab_remove,
            macctl::mac_undo,
            upkeep::upkeep_trash,
            upkeep::upkeep_quit,
            upkeep::upkeep_reveal,
            upkeep::upkeep_open_settings,
            tasks::tasks_list,
            tasks::task_save,
            tasks::task_done,
            tasks::task_delete,
            scheduler::schedules_list,
            scheduler::schedule_save,
            scheduler::schedule_delete,
            scheduler::schedule_run,
            scheduler::schedule_parse,
            feeds::feeds_list,
            feeds::feed_follow,
            feeds::feed_delete,
            watchers::watchers_list,
            watchers::watcher_save,
            watchers::watcher_delete,
            watchers::watcher_events,
            watchers::watcher_check,
            selection::selection_paste,
            quick::quick_toggle,
            quick::quick_hide,
            quick::quick_open,
            voice::voice_status,
            voice::voice_download,
            voice::voice_delete,
            voice::voice_transcribe,
            voice::voice_transcribe_file,
            speakers::speakers_status,
            speakers::speakers_download,
            speakers::speakers_delete,
            media::media_status,
            media::media_download,
            media::media_delete,
            speech::speech_say,
            speech::speech_stop,
            speech::speech_feed,
            notes::notes_list,
            notes::note_get,
            notes::note_save,
            notes::note_delete,
            notes::notes_info,
            notes::note_clip,
            mindmap::mindmap_make,
            board::boards_list,
            board::board_get,
            board::board_save,
            board::board_delete,
            board::board_assist,
            tts::voices_catalog,
            tts::cloud_voices,
            tts::voices_status,
            tts::tts_voice_download,
            tts::tts_voice_unpack,
            tts::tts_voice_delete,
            speech::speech_voices,
            wake::wake_pause,
            wake::wake_ready,
            clipboard::clip_list,
            clipboard::clip_copy,
            clipboard::clip_delete,
            clipboard::clip_clear,
            writing::writing_run,
            writing::writing_outline,
            writing::writing_section,
            writing::style_learn,
            jobs::jobs_list,
            assistants::assistants_list,
            assistants::assistant_presets,
            assistants::assistant_save,
            assistants::assistant_delete,
            jobs::job_save,
            jobs::job_delete,
            jobs::job_from_url,
            jobs::job_prep_prompt,
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
            commands::agent_approve,
            commands::decks_list,
            commands::deck_save,
            commands::deck_cards,
            commands::study_queue,
            commands::card_review,
            commands::deck_delete,
            commands::card_delete,
            commands::deck_export,
            commands::agent_show,
            commands::agent_file,
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
            commands::diagnostics_report,
            automations::automations_list,
            automations::automation_save,
            automations::automation_delete,
            automations::automation_run,
            automations::automation_run_status,
            automations::automation_trigger_parse,
            shortcut_make::automation_shortcut,
            trackers::trackers_list,
            trackers::tracker_save,
            trackers::tracker_done,
            trackers::tracker_delete,
            trackers::tracker_date_parse,
            trackers::tracker_carrier,
            connectors::connectors_status,
            connectors::obsidian_set,
            connectors::notion_connect,
            connectors::notion_disconnect,
            connectors::calendar_link_add,
            connectors::calendar_link_remove,
            dashboard::dashboard_summary,
            dashboard::dashboard_today,
            dashboard::dashboard_usage,
            dashboard::research_library,
            commands::chat_send,
            lock::lock_status,
            lock::lock_touch,
            lock::lock_now,
            lock::lock_unlock,
            lock::lock_verify,
            privacy::privacy_permissions,
            privacy::actions_list,
            privacy::actions_clear,
            kids::kids_enter,
            kids::kids_exit,
            backup::backup_info,
            backup::backup_now,
            backup::backup_forget,
            backup::backup_restore,
            backup::erase_everything,
            updater::update_configured,
            updater::update_check,
            messages::messages_status,
            messages::messages_threads,
            messages::messages_thread,
            messages::messages_send,
            updater::update_install,
            commands::chat_cancel,
        ])
        .build(tauri::generate_context!())
        .expect("error while building BYTE");

    app.run(|handle, event| {
        // Clicking BYTE in the Dock shows the window again (it may be hidden).
        #[cfg(target_os = "macos")]
        if let RunEvent::Reopen { .. } = event {
            background::show_main(handle);
        }
        if let RunEvent::Exit = event {
            if let Some(state) = handle.try_state::<AppState>() {
                state.engine.kill_now();
                state.extras.kill_all_now();
                state.embedder.kill_now();
                state.looker.kill_now();
            }
        }
    });
}
