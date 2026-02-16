#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use serde::Deserialize;
use std::sync::{Arc, Mutex};
use tauri::{AppHandle, Emitter, State};
use futures_util::StreamExt;
use reqwest::Client;

#[derive(Clone)]
struct AppState {
    model: Arc<Mutex<String>>,
    mode: Arc<Mutex<String>>,
}

#[derive(Deserialize)]
struct OllamaChunk {
    response: Option<String>,
    done: Option<bool>,
}

#[tauri::command]
async fn set_model(state: State<'_, AppState>, model: String) -> Result<(), String> {
    *state.model.lock().map_err(|_| "lock error")? = model;
    Ok(())
}

#[tauri::command]
async fn set_mode(state: State<'_, AppState>, mode: String) -> Result<(), String> {
    *state.mode.lock().map_err(|_| "lock error")? = mode;
    Ok(())
}

#[tauri::command]
async fn list_models() -> Result<Vec<String>, String> {
    Ok(vec![
        "llama3:latest".into(),
        "llama3.1:8b".into(),
        "llama3.1:70b".into(),
        "mistral:latest".into(),
    ])
}

#[tauri::command]
async fn stream_generate(
    app: AppHandle,
    state: State<'_, AppState>,
    prompt: String,
) -> Result<(), String> {
    let model = state.model.lock().unwrap().clone();
    let mode = state.mode.lock().unwrap().clone();

    tauri::async_runtime::spawn(async move {
        let client = Client::new();
        let url = "http://localhost:11434/api/generate";

        let body = serde_json::json!({
            "model": model,
            "prompt": prompt,
            "stream": true,
            "options": {
                "temperature": if mode == "fast" { 0.2 } else { 0.7 }
            }
        });

        let response = client.post(url).json(&body).send().await;

        if response.is_err() {
            let _ = app.emit("stream", "BYTE encountered a connection error.");
            let _ = app.emit("stream_done", "");
            return;
        }

        let mut stream = response.unwrap().bytes_stream();
        let mut last_response = String::new();

        while let Some(chunk) = stream.next().await {
            if let Ok(bytes) = chunk {
                let text = String::from_utf8_lossy(&bytes);

                for line in text.lines() {
                    if let Ok(parsed) = serde_json::from_str::<OllamaChunk>(line) {
                        if let Some(resp) = parsed.response {
                            // Ollama sends the full accumulated text each time.
                            // We only want to emit the *new* part.
                            let new_part = if let Some(stripped) = resp.strip_prefix(&last_response) {
                                stripped.to_string()
                            } else {
                                // Fallback: if it doesn't start with last_response, emit whole thing.
                                resp.clone()
                            };

                            if !new_part.is_empty() {
                                let _ = app.emit("stream", new_part);
                            }

                            last_response = resp;
                        }

                        if parsed.done == Some(true) {
                            let _ = app.emit("stream_done", "");
                        }
                    }
                }
            }
        }
    });

    Ok(())
}

fn main() {
    tauri::Builder::default()
        .manage(AppState {
            model: Arc::new(Mutex::new("llama3:latest".into())),
            mode: Arc::new(Mutex::new("smart".into())),
        })
        .invoke_handler(tauri::generate_handler![
            set_model,
            set_mode,
            list_models,
            stream_generate
        ])
        .run(tauri::generate_context!())
        .expect("error while running BYTE");
}
