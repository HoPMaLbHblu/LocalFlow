//! Settings › AI: the GigaChat key (kept in Windows Credential Manager or the macOS
//! Keychain, never in the database), the model, and "Write with AI" in the editor.

use std::time::Duration;

use localflow_core::{ai, LocalFlow};
use serde::Serialize;
use tauri::State;

use crate::{commands::CommandError, AppState};

const KEYRING_SERVICE: &str = "LocalFlow";
const KEYRING_USER: &str = "GigaChat authorization key";
const SCOPE: &str = "ai_scope";
const MODEL: &str = "ai_model";

fn entry() -> Result<keyring::Entry, String> {
    keyring::Entry::new(KEYRING_SERVICE, KEYRING_USER).map_err(|e| e.to_string())
}

fn error(message: impl std::fmt::Display) -> CommandError {
    CommandError::Error { message: message.to_string() }
}

async fn saved(flow: &LocalFlow, key: &str, default: &str, allowed: &[&str]) -> String {
    flow.repo()
        .get_setting(key)
        .await
        .ok()
        .flatten()
        .filter(|v| allowed.contains(&v.as_str()))
        .unwrap_or_else(|| default.to_string())
}

/// Hand the saved key to the engine. Called at startup and after changes.
pub async fn load(flow: &LocalFlow) {
    let key = entry().and_then(|e| e.get_password().map_err(|e| e.to_string())).ok();
    let credentials = match key {
        Some(key) if !key.trim().is_empty() => Some(ai::Credentials {
            key,
            scope: saved(flow, SCOPE, ai::SCOPES[0], ai::SCOPES).await,
            model: saved(flow, MODEL, ai::DEFAULT_MODEL, ai::MODELS).await,
        }),
        _ => None,
    };
    ai::configure(credentials);
}

#[derive(Serialize)]
pub struct AiSettings {
    /// A key is saved (the key itself is never sent to the window).
    configured: bool,
    scope: String,
    model: String,
    scopes: Vec<&'static str>,
    models: Vec<&'static str>,
}

#[tauri::command]
pub async fn get_ai_settings(state: State<'_, AppState>) -> Result<AiSettings, CommandError> {
    Ok(AiSettings {
        configured: ai::is_configured(),
        scope: saved(&state.flow, SCOPE, ai::SCOPES[0], ai::SCOPES).await,
        model: saved(&state.flow, MODEL, ai::DEFAULT_MODEL, ai::MODELS).await,
        scopes: ai::SCOPES.to_vec(),
        models: ai::MODELS.to_vec(),
    })
}

/// Save the key (if given), scope and model. An empty key keeps the saved one.
#[tauri::command]
pub async fn set_ai_settings(
    state: State<'_, AppState>,
    key: Option<String>,
    scope: String,
    model: String,
) -> Result<(), CommandError> {
    if !ai::SCOPES.contains(&scope.as_str()) || !ai::MODELS.contains(&model.as_str()) {
        return Err(error("Unknown GigaChat scope or model."));
    }
    if let Some(key) = key.map(|k| k.trim().to_string()).filter(|k| !k.is_empty()) {
        entry().and_then(|e| e.set_password(&key).map_err(|e| e.to_string())).map_err(|e| {
            error(format!("Could not save the key in the system's password store: {e}"))
        })?;
    }
    let repo = state.flow.repo();
    repo.set_setting(SCOPE, &scope).await.map_err(error)?;
    repo.set_setting(MODEL, &model).await.map_err(error)?;
    load(&state.flow).await;
    Ok(())
}

/// Forget the key.
#[tauri::command]
pub async fn clear_ai_key(state: State<'_, AppState>) -> Result<(), CommandError> {
    if let Ok(entry) = entry() {
        // Already gone is fine.
        let _ = entry.delete_credential();
    }
    load(&state.flow).await;
    Ok(())
}

/// A tiny request, to check the key works. Returns GigaChat's answer.
#[tauri::command]
pub async fn test_ai(language: String) -> Result<String, CommandError> {
    let question = match language.as_str() {
        "ru" => "Поздоровайся одним коротким предложением.",
        "de" => "Sag in einem kurzen Satz Hallo.",
        _ => "Say hello in one short sentence.",
    };
    tauri::async_runtime::spawn_blocking(move || {
        ai::ask(question, None, &ai::AskOptions { max_tokens: Some(60), ..Default::default() }, Duration::from_secs(60))
    })
    .await
    .map_err(error)?
    .map_err(error)
}

/// "Write with AI": Lua code for a description, for the user to review and test.
#[tauri::command]
pub async fn ai_write_automation(description: String, language: String) -> Result<ai::WrittenCode, CommandError> {
    tauri::async_runtime::spawn_blocking(move || ai::write_automation(&description, &language, Duration::from_secs(120)))
        .await
        .map_err(error)?
        .map_err(error)
}
