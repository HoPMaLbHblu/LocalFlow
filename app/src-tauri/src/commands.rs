//! Commands the frontend calls with `invoke(...)`.

use chrono::{DateTime, Utc};
use localflow_core::{
    db::models::{Automation, AutomationRun, LogEntry},
    lua::{engine, Example, EXAMPLES},
    scheduler::validate_cron,
    AutomationInput, AutomationSummary, CoreError, TestRunResult,
};
use serde::Serialize;
use tauri::State;

use crate::AppState;

/// Errors as the frontend sees them.
#[derive(Debug, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum CommandError {
    /// Input was rejected; show each message next to the form.
    Validation {
        messages: Vec<String>,
    },
    NotFound,
    Error {
        message: String,
    },
}

impl From<CoreError> for CommandError {
    fn from(e: CoreError) -> Self {
        match e {
            CoreError::Validation(messages) => CommandError::Validation { messages },
            CoreError::NotFound => CommandError::NotFound,
            other => {
                tracing::error!("command failed: {other}");
                CommandError::Error {
                    message: other.to_string(),
                }
            }
        }
    }
}

type CommandResult<T> = Result<T, CommandError>;

#[derive(Serialize)]
pub struct AutomationDetail {
    #[serde(flatten)]
    automation: Automation,
    scheduled: bool,
    watching: bool,
    next_run: Option<DateTime<Utc>>,
}

#[tauri::command]
pub async fn list_automations(state: State<'_, AppState>) -> CommandResult<Vec<AutomationSummary>> {
    Ok(state.flow.list().await?)
}

#[tauri::command]
pub async fn get_automation(
    state: State<'_, AppState>,
    id: i64,
) -> CommandResult<AutomationDetail> {
    let automation = state.flow.get(id).await?;
    Ok(AutomationDetail {
        automation,
        scheduled: state.flow.is_scheduled(id).await,
        watching: state.flow.is_watching(id).await,
        next_run: state.flow.next_run(id).await,
    })
}

#[tauri::command]
pub async fn create_automation(
    state: State<'_, AppState>,
    input: AutomationInput,
) -> CommandResult<Automation> {
    Ok(state.flow.create(&input).await?)
}

#[tauri::command]
pub async fn update_automation(
    state: State<'_, AppState>,
    id: i64,
    input: AutomationInput,
) -> CommandResult<Automation> {
    Ok(state.flow.update(id, &input).await?)
}

#[tauri::command]
pub async fn set_enabled(
    state: State<'_, AppState>,
    id: i64,
    enabled: bool,
) -> CommandResult<Automation> {
    Ok(state.flow.set_enabled(id, enabled).await?)
}

#[tauri::command]
pub async fn delete_automation(state: State<'_, AppState>, id: i64) -> CommandResult<()> {
    Ok(state.flow.delete(id).await?)
}

#[tauri::command]
pub async fn run_automation(state: State<'_, AppState>, id: i64) -> CommandResult<AutomationRun> {
    Ok(state.flow.run(id, "manual").await?)
}

/// Run code straight from the editor without saving it.
#[tauri::command]
pub async fn test_run(
    state: State<'_, AppState>,
    code: String,
    name: String,
) -> CommandResult<TestRunResult> {
    Ok(state.flow.test_run(code, name).await)
}

/// `None` if the code compiles, otherwise the syntax error.
#[tauri::command]
pub fn validate_code(code: String) -> Option<String> {
    if code.trim().is_empty() {
        return None;
    }
    engine::validate(&code).err()
}

/// `None` if the cron expression is valid (or empty), otherwise why not.
#[tauri::command]
pub fn validate_schedule(schedule: String) -> Option<String> {
    let schedule = schedule.trim();
    if schedule.is_empty() {
        return None;
    }
    validate_cron(schedule).err()
}

#[tauri::command]
pub async fn list_runs(
    state: State<'_, AppState>,
    id: i64,
    limit: i64,
) -> CommandResult<Vec<AutomationRun>> {
    Ok(state.flow.runs(id, limit).await?)
}

#[tauri::command]
pub async fn list_logs(
    state: State<'_, AppState>,
    id: i64,
    limit: i64,
) -> CommandResult<Vec<LogEntry>> {
    Ok(state.flow.logs(id, limit).await?)
}

#[tauri::command]
pub fn get_templates() -> &'static [Example] {
    EXAMPLES
}
