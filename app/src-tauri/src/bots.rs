//! Settings › Telegram and Discord. The bot token and the webhook address are secrets:
//! they're kept in Windows Credential Manager or the macOS Keychain, never in the
//! database, and never sent to the window. Your chat id and the switches are settings.

use std::time::Duration;

use localflow_core::{messaging, remote, LocalFlow};
use serde::Serialize;
use tauri::State;

use crate::{commands::CommandError, AppState};

const KEYRING_SERVICE: &str = "LocalFlow";
const TOKEN_USER: &str = "Telegram bot token";
const WEBHOOK_USER: &str = "Discord webhook";
const CHAT: &str = "telegram_chat";
const REMOTE: &str = "telegram_remote";
const REMOTE_POWER: &str = "telegram_remote_power";

fn entry(user: &str) -> Result<keyring::Entry, String> {
    keyring::Entry::new(KEYRING_SERVICE, user).map_err(|e| e.to_string())
}

fn secret(user: &str) -> Option<String> {
    entry(user).ok()?.get_password().ok().filter(|s| !s.trim().is_empty())
}

fn save_secret(user: &str, value: &str) -> Result<(), CommandError> {
    entry(user)
        .and_then(|e| e.set_password(value).map_err(|e| e.to_string()))
        .map_err(|e| error(format!("Could not save it in the system's password store: {e}")))
}

fn forget(user: &str) {
    if let Ok(entry) = entry(user) {
        let _ = entry.delete_credential();
    }
}

fn error(message: impl std::fmt::Display) -> CommandError {
    CommandError::Error { message: message.to_string() }
}

async fn setting(flow: &LocalFlow, key: &str) -> Option<String> {
    flow.repo().get_setting(key).await.ok().flatten()
}

/// Hand the saved secrets and switches to the engine. Called at startup and after changes.
pub async fn load(flow: &LocalFlow) {
    let chat = setting(flow, CHAT).await.and_then(|c| c.parse().ok());
    messaging::configure(messaging::Bots {
        telegram_token: secret(TOKEN_USER),
        telegram_chat: chat,
        discord_webhook: secret(WEBHOOK_USER),
    });
    let on = |v: Option<String>| v.as_deref() == Some("true");
    remote::configure(on(setting(flow, REMOTE).await), on(setting(flow, REMOTE_POWER).await));
}

#[derive(Serialize)]
pub struct BotSettings {
    telegram_token: bool,
    telegram_chat: Option<String>,
    discord_webhook: bool,
    remote: bool,
    remote_power: bool,
}

#[tauri::command]
pub async fn get_bot_settings(state: State<'_, AppState>) -> Result<BotSettings, CommandError> {
    let flow = &state.flow;
    Ok(BotSettings {
        telegram_token: secret(TOKEN_USER).is_some(),
        telegram_chat: setting(flow, CHAT).await,
        discord_webhook: secret(WEBHOOK_USER).is_some(),
        remote: setting(flow, REMOTE).await.as_deref() == Some("true"),
        remote_power: setting(flow, REMOTE_POWER).await.as_deref() == Some("true"),
    })
}

/// Save what was given. Empty secrets keep the saved ones.
#[tauri::command]
pub async fn set_bot_settings(
    state: State<'_, AppState>,
    token: Option<String>,
    chat: Option<String>,
    webhook: Option<String>,
    remote: bool,
    remote_power: bool,
) -> Result<(), CommandError> {
    if let Some(token) = token.map(|t| t.trim().to_string()).filter(|t| !t.is_empty()) {
        let valid = token.split_once(':').is_some_and(|(id, rest)| id.chars().all(|c| c.is_ascii_digit()) && rest.len() > 20);
        if !valid {
            return Err(error("That doesn't look like a bot token. It looks like 123456789:AAE..., from @BotFather."));
        }
        save_secret(TOKEN_USER, &token)?;
    }
    if let Some(webhook) = webhook.map(|w| w.trim().to_string()).filter(|w| !w.is_empty()) {
        if !messaging::is_discord_webhook(&webhook) {
            return Err(error("That isn't a Discord webhook address. It starts with https://discord.com/api/webhooks/."));
        }
        save_secret(WEBHOOK_USER, &webhook)?;
    }
    let repo = state.flow.repo();
    match chat.map(|c| c.trim().to_string()).filter(|c| !c.is_empty()) {
        Some(chat) if chat.parse::<i64>().is_ok() => repo.set_setting(CHAT, &chat).await.map_err(error)?,
        Some(_) => return Err(error("The chat id is a number, like 123456789.")),
        None => {}
    }
    if remote && (secret(TOKEN_USER).is_none() || setting(&state.flow, CHAT).await.is_none()) {
        return Err(error("Set up the bot token and your chat first."));
    }
    repo.set_setting(REMOTE, if remote { "true" } else { "false" }).await.map_err(error)?;
    repo.set_setting(REMOTE_POWER, if remote && remote_power { "true" } else { "false" }).await.map_err(error)?;
    load(&state.flow).await;
    Ok(())
}

/// Forget "telegram" (token, chat, remote control) or "discord" (the webhook).
#[tauri::command]
pub async fn clear_bot(state: State<'_, AppState>, which: String) -> Result<(), CommandError> {
    let repo = state.flow.repo();
    if which == "telegram" {
        forget(TOKEN_USER);
        repo.set_setting(CHAT, "").await.map_err(error)?;
        repo.set_setting(REMOTE, "false").await.map_err(error)?;
        repo.set_setting(REMOTE_POWER, "false").await.map_err(error)?;
    } else {
        forget(WEBHOOK_USER);
    }
    load(&state.flow).await;
    Ok(())
}

#[derive(Serialize)]
pub struct FoundChat {
    id: String,
    name: String,
}

/// The people who wrote to the bot lately, so the owner can pick their own chat.
/// Also returns the bot's @name.
#[tauri::command]
pub async fn find_telegram_chats(token: Option<String>) -> Result<(String, Vec<FoundChat>), CommandError> {
    let token = token.map(|t| t.trim().to_string()).filter(|t| !t.is_empty()).or_else(|| secret(TOKEN_USER));
    let Some(token) = token else { return Err(error("Paste the bot token first.")) };
    tauri::async_runtime::spawn_blocking(move || {
        let name = messaging::telegram_bot_name(&token, Duration::from_secs(20))?;
        let chats = messaging::telegram_recent_chats(&token, Duration::from_secs(20))?;
        Ok::<_, String>((name, chats.into_iter().map(|(id, name)| FoundChat { id: id.to_string(), name }).collect()))
    })
    .await
    .map_err(error)?
    .map_err(error)
}

/// Send a test message to Telegram and/or Discord (whatever is set up).
#[tauri::command]
pub async fn test_bots() -> Result<String, CommandError> {
    tauri::async_runtime::spawn_blocking(|| {
        let mut sent = Vec::new();
        let text = "LocalFlow is connected. Send /help to see what I can do.";
        if messaging::telegram_ready() {
            messaging::telegram_send(text, Duration::from_secs(20)).map_err(|e| format!("Telegram: {e}"))?;
            sent.push("Telegram");
        }
        if messaging::discord_ready() {
            messaging::discord_send("LocalFlow is connected.", Duration::from_secs(20)).map_err(|e| format!("Discord: {e}"))?;
            sent.push("Discord");
        }
        if sent.is_empty() {
            return Err("Nothing is set up yet.".to_string());
        }
        Ok(sent.join(", "))
    })
    .await
    .map_err(error)?
    .map_err(error)
}
