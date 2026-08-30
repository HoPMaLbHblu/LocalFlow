use axum::{
    extract::{Path, State},
    response::{IntoResponse, Response},
};
use minijinja::context;

use crate::{
    db::models::RunView,
    errors::{AppError, AppResult},
    state::AppState,
};

/// Execution history for one automation.
pub async fn list_runs(State(state): State<AppState>, Path(id): Path<i64>) -> AppResult<Response> {
    let automation = state.repo.get_automation(id).await?.ok_or(AppError::NotFound)?;
    let runs: Vec<RunView> = state.repo.list_runs(id, 100).await?.into_iter().map(Into::into).collect();

    Ok(state.render("runs.html", context! { automation, runs })?.into_response())
}

/// Log viewer for one automation. The page polls itself via HTMX to stay current.
pub async fn list_logs(State(state): State<AppState>, Path(id): Path<i64>) -> AppResult<Response> {
    let automation = state.repo.get_automation(id).await?.ok_or(AppError::NotFound)?;
    let logs = state.repo.list_logs(id, 500).await?;

    Ok(state.render("logs.html", context! { automation, logs })?.into_response())
}
