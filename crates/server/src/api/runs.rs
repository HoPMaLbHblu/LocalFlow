use axum::{
    extract::{Path, State},
    response::{IntoResponse, Response},
};
use localflow_core::db::models::RunView;
use minijinja::context;

use crate::{errors::AppResult, state::AppState};

/// Execution history for one automation.
pub async fn list_runs(State(state): State<AppState>, Path(id): Path<i64>) -> AppResult<Response> {
    let automation = state.flow.get(id).await?;
    let runs: Vec<RunView> = state.flow.runs(id, 100).await?.into_iter().map(Into::into).collect();

    Ok(state.render("runs.html", context! { automation, runs })?.into_response())
}

/// Log viewer for one automation. The page polls itself via HTMX to stay current.
pub async fn list_logs(State(state): State<AppState>, Path(id): Path<i64>) -> AppResult<Response> {
    let automation = state.flow.get(id).await?;
    let logs = state.flow.logs(id, 500).await?;

    Ok(state.render("logs.html", context! { automation, logs })?.into_response())
}
