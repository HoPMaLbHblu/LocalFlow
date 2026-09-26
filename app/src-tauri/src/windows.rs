//! Separate windows next to the app: the guide, the AI chat and the Dota 2 companion.

use tauri::{AppHandle, Manager, WebviewUrl, WebviewWindowBuilder};

use crate::{commands::CommandError, show_main_window};

pub const GUIDE_LABEL: &str = "guide";
pub const AI_CHAT_LABEL: &str = "aichat";
pub const DOTA_LABEL: &str = "dota";

// Must be async: on Windows, creating a window inside a synchronous command deadlocks the app.
/// Open the guide window, or bring it to the front if it's already open.
#[tauri::command]
pub async fn open_guide(app: AppHandle, title: String) -> Result<(), CommandError> {
    if let Some(window) = app.get_webview_window(GUIDE_LABEL) {
        let _ = window.unminimize();
        let _ = window.show();
        let _ = window.set_focus();
        return Ok(());
    }
    WebviewWindowBuilder::new(&app, GUIDE_LABEL, WebviewUrl::App("index.html".into()))
        .title(title)
        .inner_size(760.0, 820.0)
        .min_inner_size(480.0, 400.0)
        .build()
        .map_err(|e| CommandError::Error { message: format!("could not open the guide window: {e}") })?;
    Ok(())
}

/// Open the AI chat window, or bring it to the front if it's already open.
#[tauri::command]
pub async fn open_ai_chat(app: AppHandle, title: String) -> Result<(), CommandError> {
    if let Some(window) = app.get_webview_window(AI_CHAT_LABEL) {
        let _ = window.unminimize();
        let _ = window.show();
        let _ = window.set_focus();
        return Ok(());
    }
    WebviewWindowBuilder::new(&app, AI_CHAT_LABEL, WebviewUrl::App("index.html".into()))
        .title(title)
        .inner_size(440.0, 720.0)
        .min_inner_size(340.0, 420.0)
        .build()
        .map_err(|e| CommandError::Error { message: format!("could not open the AI chat window: {e}") })?;
    Ok(())
}

/// Open the Dota 2 companion window, or bring it to the front if it's already open.
#[tauri::command]
pub async fn open_dota(app: AppHandle, title: String) -> Result<(), CommandError> {
    if let Some(window) = app.get_webview_window(DOTA_LABEL) {
        let _ = window.unminimize();
        let _ = window.show();
        let _ = window.set_focus();
        return Ok(());
    }
    WebviewWindowBuilder::new(&app, DOTA_LABEL, WebviewUrl::App("index.html".into()))
        .title(title)
        .inner_size(460.0, 780.0)
        .min_inner_size(380.0, 480.0)
        .build()
        .map_err(|e| CommandError::Error { message: format!("could not open the Dota 2 window: {e}") })?;
    Ok(())
}

/// Bring the main window forward (used by "Open in editor" in the guide window).
#[tauri::command]
pub fn show_main(app: AppHandle) {
    show_main_window(&app);
}
