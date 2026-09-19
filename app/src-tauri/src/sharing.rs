//! Exporting and importing `.localflow` files, including files opened by
//! double-clicking them in Explorer.

use std::{path::PathBuf, sync::Mutex};

use localflow_core::{
    db::models::Automation,
    sharing::{ImportPreview, EXTENSION},
};
use tauri::{AppHandle, Emitter, Manager, State};

use crate::{commands::CommandError, show_main_window, AppState};

/// Event telling the frontend to show the import screen for a file.
pub const OPEN_FILE_EVENT: &str = "localflow://open-file";

/// A file LocalFlow was started with, waiting for the frontend to ask for it.
#[derive(Default)]
pub struct PendingImport(pub Mutex<Option<String>>);

/// The first `.localflow` file among command-line arguments, if any.
pub fn file_argument<I: IntoIterator<Item = String>>(args: I) -> Option<String> {
    args.into_iter().skip(1).find(|a| {
        PathBuf::from(a)
            .extension()
            .is_some_and(|e| e.eq_ignore_ascii_case(EXTENSION))
    })
}

/// Called when a second copy of LocalFlow is started with a file: show it in the running one.
pub fn open_from_second_instance(app: &AppHandle, args: Vec<String>) {
    show_main_window(app);
    if let Some(path) = file_argument(args) {
        let _ = app.emit(OPEN_FILE_EVENT, path);
    }
}

/// A file opened on macOS (double-click, or "Open With"): remember it for a
/// window that is still starting, and tell a window that is already running.
#[allow(dead_code)]
pub fn open_file(app: &AppHandle, path: String) {
    if !PathBuf::from(&path).extension().is_some_and(|e| e.eq_ignore_ascii_case(EXTENSION)) {
        return;
    }
    if let Some(pending) = app.try_state::<PendingImport>() {
        if let Ok(mut slot) = pending.0.lock() {
            *slot = Some(path.clone());
        }
    }
    show_main_window(app);
    let _ = app.emit(OPEN_FILE_EVENT, path);
}

fn read_file(path: &str) -> Result<String, CommandError> {
    std::fs::read_to_string(path).map_err(|e| CommandError::Error {
        message: format!("Could not read {path}: {e}"),
    })
}

/// Save an automation as a `.localflow` file at `path` (chosen by the user).
#[tauri::command]
pub async fn export_automation(
    state: State<'_, AppState>,
    id: i64,
    path: String,
) -> Result<(), CommandError> {
    let contents = state.flow.export(id).await?;
    std::fs::write(&path, contents).map_err(|e| CommandError::Error {
        message: format!("Could not save {path}: {e}"),
    })
}

/// Read a `.localflow` file and describe it without importing it.
#[tauri::command]
pub fn preview_import(
    state: State<'_, AppState>,
    path: String,
) -> Result<ImportPreview, CommandError> {
    Ok(state.flow.preview_import(&read_file(&path)?)?)
}

/// Import a `.localflow` file. The automation starts disabled.
#[tauri::command]
pub async fn import_automation(
    state: State<'_, AppState>,
    path: String,
) -> Result<Automation, CommandError> {
    Ok(state.flow.import(&read_file(&path)?).await?)
}

/// The file LocalFlow was opened with (by double-clicking it), once.
#[tauri::command]
pub fn take_pending_import(app: AppHandle) -> Option<String> {
    app.state::<PendingImport>().0.lock().ok()?.take()
}
