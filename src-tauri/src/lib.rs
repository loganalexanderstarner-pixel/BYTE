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
mod looker;
mod engine;
mod export;
mod factcheck;
mod filectl;
mod files;
mod gguf;
mod ocr;
mod memory;
mod error;
mod modelcfg;
mod models;
mod paths;
mod profiles;
mod prompt;
mod quick;
mod media;
mod speakers;
mod speech;
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
        .plugin(tauri_plugin_deep_link::init())
        .plugin(tauri_plugin_autostart::Builder::new().args(["--background"]).build())
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
        // Global shortcuts (quick.rs): Quick Ask and the selection hotkey, keys from Settings.
        .plugin(
            tauri_plugin_global_shortcut::Builder::new()
                .with_handler(|app, shortcut, event| {
                    if event.state() == tauri_plugin_global_shortcut::ShortcutState::Pressed {
                        quick::pressed(app, shortcut);
                    }
                })
                .build(),
        )
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
            state.looker.reap_stale();
            // Models the user added (model lab) join the catalog.
            state.catalog.set_added(crate::lab::load(&state.paths.data.join("added_models.json")).iter().map(crate::lab::LabModel::to_catalog).collect());
            let _ = state.app.set(app.handle().clone());
            let settings_now = state.settings.blocking_lock().clone();
            let catalog = state.catalog.get();
            let models_dir = state.paths.models.clone();
            app.manage(state);
            // Global shortcuts (Quick Ask, the selection hotkey) and the menu-bar icon;
            // clipboard history checks its own setting.
            quick::apply_shortcuts(app.handle(), &settings_now);
            quick::apply_tray(app.handle(), settings_now.menu_bar_icon);
            wake::apply(app.handle(), settings_now.wake_word);
            clipboard::watch(app.handle().clone());
            // Reminders, the daily briefing and scheduled questions.
            scheduler::start(app.handle().clone());
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
