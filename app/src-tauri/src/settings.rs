//! User preferences, stored in the `settings` table.

use std::{
    path::PathBuf,
    sync::atomic::{AtomicBool, Ordering},
};

use localflow_core::{CoreResult, LocalFlow};
use serde::Serialize;
use tauri::{AppHandle, Manager, State};
use tauri_plugin_autostart::ManagerExt;

use crate::{commands::CommandError, AppState};

const NOTIFICATIONS: &str = "notifications";
const ALLOWED_DIRS: &str = "allowed_dirs";

#[derive(Serialize)]
pub struct Settings {
    autostart: bool,
    notifications: bool,
    allowed_dirs: Vec<String>,
    data_dir: String,
    version: String,
}

/// Load saved preferences into the running app. Called once at startup.
pub async fn apply_saved(flow: &LocalFlow, notifications: &AtomicBool) -> CoreResult<()> {
    let repo = flow.repo();
    if let Some(value) = repo.get_setting(NOTIFICATIONS).await? {
        notifications.store(value == "true", Ordering::Relaxed);
    }
    if let Some(value) = repo.get_setting(ALLOWED_DIRS).await? {
        let dirs = parse_dirs(&value);
        if !dirs.is_empty() {
            flow.set_allowed_dirs(&dirs);
        }
    }
    Ok(())
}

/// Stored as one folder per line.
fn parse_dirs(value: &str) -> Vec<PathBuf> {
    value.lines().map(str::trim).filter(|l| !l.is_empty()).map(PathBuf::from).collect()
}

fn error(message: impl ToString) -> CommandError {
    CommandError::Error { message: message.to_string() }
}

#[tauri::command]
pub async fn get_settings(app: AppHandle, state: State<'_, AppState>) -> Result<Settings, CommandError> {
    let allowed_dirs = state
        .flow
        .path_policy()
        .roots()
        .iter()
        .map(|p| p.to_string_lossy().trim_start_matches(r"\\?\").to_string())
        .collect();

    Ok(Settings {
        autostart: app.autolaunch().is_enabled().unwrap_or(false),
        notifications: state.notifications.load(Ordering::Relaxed),
        allowed_dirs,
        data_dir: app.path().app_data_dir().map(|p| p.display().to_string()).unwrap_or_default(),
        version: app.package_info().version.to_string(),
    })
}

#[tauri::command]
pub fn set_autostart(app: AppHandle, enabled: bool) -> Result<(), CommandError> {
    let autolaunch = app.autolaunch();
    let result = if enabled { autolaunch.enable() } else { autolaunch.disable() };
    result.map_err(error)
}

#[tauri::command]
pub async fn set_notifications(state: State<'_, AppState>, enabled: bool) -> Result<(), CommandError> {
    state.notifications.store(enabled, Ordering::Relaxed);
    state
        .flow
        .repo()
        .set_setting(NOTIFICATIONS, if enabled { "true" } else { "false" })
        .await
        .map_err(error)
}

/// Replace the folders scripts may access. Every folder must exist.
#[tauri::command]
pub async fn set_allowed_dirs(state: State<'_, AppState>, dirs: Vec<String>) -> Result<(), CommandError> {
    let mut paths = Vec::new();
    let mut problems = Vec::new();
    for dir in dirs.iter().map(|d| d.trim()).filter(|d| !d.is_empty()) {
        let path = PathBuf::from(dir);
        if path.is_dir() {
            paths.push(path);
        } else {
            problems.push(format!("Folder not found: {dir}"));
        }
    }
    if paths.is_empty() && problems.is_empty() {
        problems.push("Add at least one folder.".into());
    }
    if !problems.is_empty() {
        return Err(CommandError::Validation { messages: problems });
    }

    let value = paths.iter().map(|p| p.to_string_lossy()).collect::<Vec<_>>().join("\n");
    state.flow.repo().set_setting(ALLOWED_DIRS, &value).await.map_err(error)?;
    state.flow.set_allowed_dirs(&paths);
    Ok(())
}
