mod chat;
mod commands;
mod engine;
mod error;
mod models;
mod paths;
mod prompt;
mod router;
mod settings;
mod state;
mod system;

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
                    handle.exit(0);
                });
            }

            // Preload the model at launch so the first answer is fast.
            let handle = app.handle().clone();
            tauri::async_runtime::spawn(async move {
                match active {
                    Some((model, ctx)) => {
                        if let Err(e) = engine.start(&handle, models_dir, &model, ctx).await {
                            log::warn!("engine did not start at launch: {e}");
                        }
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
            commands::settings_get,
            commands::settings_update,
            commands::models_list,
            commands::model_download,
            commands::model_pause,
            commands::model_delete,
            commands::model_activate,
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
            }
        }
    });
}
