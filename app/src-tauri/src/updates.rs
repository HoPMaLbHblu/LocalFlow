//! "LocalFlow 1.4.0 is available": a check against GitHub's public releases shortly after
//! start and then once a day. A desktop notification appears once per new version; the
//! window shows a banner with a Download button until the user updates or dismisses it.
//! Settings › "Tell me about new versions" switches the check off.

use std::{sync::Mutex, time::Duration};

use localflow_core::{updates, LocalFlow};
use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager, State};

use crate::{commands::CommandError, AppState};

const ENABLED: &str = "update_check";
const NOTIFIED: &str = "update_notified";
const DISMISSED: &str = "update_dismissed";
pub const EVENT: &str = "localflow://update";

/// The last answer, so opening the window doesn't ask GitHub again.
#[derive(Default)]
pub struct UpdateCache(Mutex<Option<updates::Release>>);

#[derive(Clone, Serialize)]
pub struct UpdateInfo {
    current: String,
    latest: updates::Release,
}

async fn setting(flow: &LocalFlow, key: &str) -> Option<String> {
    flow.repo().get_setting(key).await.ok().flatten()
}

async fn enabled(flow: &LocalFlow) -> bool {
    setting(flow, ENABLED).await.as_deref() != Some("false")
}

/// Runs for the app's lifetime: the first check after a minute, then every 24 hours.
pub fn start(app: AppHandle) {
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(Duration::from_secs(60)).await;
        loop {
            check_and_notify(&app).await;
            tokio::time::sleep(Duration::from_secs(24 * 60 * 60)).await;
        }
    });
}

async fn check_and_notify(app: &AppHandle) {
    let state = app.state::<AppState>();
    if !enabled(&state.flow).await {
        return;
    }
    let result = tauri::async_runtime::spawn_blocking(|| updates::check(Duration::from_secs(20))).await;
    let release = match result {
        Ok(Ok(Some(release))) => release,
        Ok(Ok(None)) => return,
        Ok(Err(e)) => {
            tracing::info!("update check: {e}");
            return;
        }
        Err(_) => return,
    };
    *app.state::<UpdateCache>().0.lock().unwrap_or_else(|e| e.into_inner()) = Some(release.clone());
    let _ = app.emit(EVENT, UpdateInfo { current: updates::CURRENT.into(), latest: release.clone() });

    // One desktop notification per new version.
    if setting(&state.flow, NOTIFIED).await.as_deref() != Some(release.version.as_str()) {
        let texts = state.prefs.texts();
        let title = texts.update_title.replace("{version}", &release.version);
        let body = texts.update_body.replace("{current}", updates::CURRENT);
        crate::notify(app, &title, &body);
        let _ = state.flow.repo().set_setting(NOTIFIED, &release.version).await;
    }
}

/// The newer version, if there is one (and it wasn't dismissed). Uses the last check;
/// asks GitHub only when `refresh` is true.
#[tauri::command]
pub async fn update_status(state: State<'_, AppState>, cache: State<'_, UpdateCache>, refresh: Option<bool>) -> Result<Option<UpdateInfo>, CommandError> {
    if refresh.unwrap_or(false) {
        let release = tauri::async_runtime::spawn_blocking(|| updates::check(Duration::from_secs(20)))
            .await
            .map_err(|e| CommandError::Error { message: e.to_string() })?
            .map_err(|message| CommandError::Error { message })?;
        *cache.0.lock().unwrap_or_else(|e| e.into_inner()) = release;
    }
    let latest = cache.0.lock().unwrap_or_else(|e| e.into_inner()).clone();
    let Some(latest) = latest.filter(|r| updates::is_newer(&r.version, updates::CURRENT)) else { return Ok(None) };
    if !refresh.unwrap_or(false) && setting(&state.flow, DISMISSED).await.as_deref() == Some(latest.version.as_str()) {
        return Ok(None);
    }
    Ok(Some(UpdateInfo { current: updates::CURRENT.into(), latest }))
}

/// "Later": hide the banner for this version (a newer one shows it again).
#[tauri::command]
pub async fn dismiss_update(state: State<'_, AppState>, version: String) -> Result<(), CommandError> {
    state.flow.repo().set_setting(DISMISSED, &version).await.map_err(|e| CommandError::Error { message: e.to_string() })
}

#[tauri::command]
pub async fn get_update_check(state: State<'_, AppState>) -> Result<bool, CommandError> {
    Ok(enabled(&state.flow).await)
}

#[tauri::command]
pub async fn set_update_check(state: State<'_, AppState>, enabled: bool) -> Result<(), CommandError> {
    state
        .flow
        .repo()
        .set_setting(ENABLED, if enabled { "true" } else { "false" })
        .await
        .map_err(|e| CommandError::Error { message: e.to_string() })
}

/// Open the release page (only GitHub addresses come from the check).
#[tauri::command]
pub fn open_release_page(url: String) -> Result<(), CommandError> {
    if !url.starts_with("https://github.com/") {
        return Err(CommandError::Error { message: "not a release page".into() });
    }
    // On a fresh thread: the Windows shell can ignore links opened from a thread where COM
    // was already set up in multithreaded mode.
    std::thread::spawn(move || open::that_detached(url))
        .join()
        .map_err(|_| CommandError::Error { message: "could not open the browser".into() })?
        .map_err(|e| CommandError::Error { message: e.to_string() })
}
