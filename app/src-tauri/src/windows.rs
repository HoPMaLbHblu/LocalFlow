//! The separate guide window, so the guide can sit next to the app.

use tauri::{AppHandle, Manager, WebviewUrl, WebviewWindowBuilder};

use crate::{commands::CommandError, show_main_window};

pub const GUIDE_LABEL: &str = "guide";

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

/// Bring the main window forward (used by "Open in editor" in the guide window).
#[tauri::command]
pub fn show_main(app: AppHandle) {
    show_main_window(&app);
}
