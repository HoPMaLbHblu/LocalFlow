//! Trash, version history and backups: the commands that keep data from being lost.

use localflow_core::{
    backup::BackupInfo,
    db::models::{Automation, AutomationVersion},
    StartupNotice,
};
use tauri::{AppHandle, State};

use crate::{commands::CommandError, AppState};

type CommandResult<T> = Result<T, CommandError>;

#[tauri::command]
pub async fn list_trash(state: State<'_, AppState>) -> CommandResult<Vec<Automation>> {
    Ok(state.flow.trash().await?)
}

#[tauri::command]
pub async fn restore_automation(state: State<'_, AppState>, id: i64) -> CommandResult<Automation> {
    Ok(state.flow.restore(id).await?)
}

/// Only for automations already in the trash. A backup is made first.
#[tauri::command]
pub async fn delete_forever(state: State<'_, AppState>, id: i64) -> CommandResult<()> {
    Ok(state.flow.delete_forever(id).await?)
}

#[tauri::command]
pub async fn list_versions(state: State<'_, AppState>, id: i64) -> CommandResult<Vec<AutomationVersion>> {
    Ok(state.flow.versions(id).await?)
}

#[tauri::command]
pub async fn restore_version(state: State<'_, AppState>, id: i64, version_id: i64) -> CommandResult<Automation> {
    Ok(state.flow.restore_version(id, version_id).await?)
}

#[tauri::command]
pub fn list_backups(state: State<'_, AppState>) -> Vec<BackupInfo> {
    state.flow.list_backups()
}

#[tauri::command]
pub async fn backup_now(state: State<'_, AppState>) -> CommandResult<BackupInfo> {
    Ok(state.flow.backup_now().await?)
}

/// Restore a backup: the current state is backed up, then LocalFlow restarts
/// and swaps the database before opening it.
#[tauri::command]
pub async fn restore_backup(app: AppHandle, state: State<'_, AppState>, file_name: String) -> CommandResult<()> {
    state.flow.schedule_restore(&file_name).await?;
    state.flow.close().await;
    app.restart();
}

#[tauri::command]
pub fn open_backups_folder(state: State<'_, AppState>) -> CommandResult<()> {
    let dir = state
        .flow
        .backups_dir()
        .ok_or_else(|| CommandError::Error { message: "Backups are not available.".into() })?;
    std::fs::create_dir_all(&dir).map_err(|e| CommandError::Error { message: e.to_string() })?;
    std::process::Command::new("explorer")
        .arg(&dir)
        .spawn()
        .map_err(|e| CommandError::Error { message: e.to_string() })?;
    Ok(())
}

/// A restore or a recovery that happened while starting, to tell the user once.
#[tauri::command]
pub fn startup_notice(state: State<'_, AppState>) -> Option<StartupNotice> {
    state.flow.startup_notice()
}
