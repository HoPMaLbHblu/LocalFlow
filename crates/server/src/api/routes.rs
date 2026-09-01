use axum::{
    http::header,
    response::IntoResponse,
    routing::{get, post},
    Router,
};

use super::{automations, runs};
use crate::state::AppState;

pub fn router(state: AppState) -> Router {
    Router::new()
        .route("/", get(automations::dashboard))
        .route("/automations", get(automations::dashboard).post(automations::create))
        .route("/automations/new", get(automations::new_form))
        .route(
            "/automations/{id}",
            get(automations::detail)
                .put(automations::update)
                .delete(automations::delete)
                // Fallback so the edit form still works if HTMX fails to load.
                .post(automations::update),
        )
        .route("/automations/{id}/edit", get(automations::edit_form))
        .route("/automations/{id}/run", post(automations::run_now))
        .route("/automations/{id}/toggle", post(automations::toggle))
        // Fallback for deleting without HTMX.
        .route("/automations/{id}/delete", post(automations::delete))
        .route("/automations/{id}/runs", get(runs::list_runs))
        .route("/automations/{id}/logs", get(runs::list_logs))
        .route("/static/style.css", get(stylesheet))
        .route("/health", get(|| async { "ok" }))
        .with_state(state)
}

async fn stylesheet() -> impl IntoResponse {
    (
        [(header::CONTENT_TYPE, "text/css; charset=utf-8")],
        include_str!("../../static/style.css"),
    )
}
