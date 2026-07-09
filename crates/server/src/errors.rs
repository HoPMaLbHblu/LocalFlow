use axum::{
    http::StatusCode,
    response::{Html, IntoResponse, Response},
};
use localflow_core::CoreError;

/// Every error a request handler can return.
#[derive(Debug, thiserror::Error)]
pub enum AppError {
    #[error(transparent)]
    Core(#[from] CoreError),

    #[error("Template error: {0}")]
    Template(#[from] minijinja::Error),
}

pub type AppResult<T> = Result<T, AppError>;

impl AppError {
    pub fn status(&self) -> StatusCode {
        match self {
            AppError::Core(CoreError::NotFound) => StatusCode::NOT_FOUND,
            AppError::Core(CoreError::Validation(_)) => StatusCode::BAD_REQUEST,
            _ => StatusCode::INTERNAL_SERVER_ERROR,
        }
    }
}

impl IntoResponse for AppError {
    fn into_response(self) -> Response {
        let status = self.status();
        if status.is_server_error() {
            tracing::error!(error = %self, "request failed");
        }

        // Kept independent of the template engine so errors still render if templates fail.
        let body = format!(
            "<!doctype html><html><head><meta charset=\"utf-8\"><title>Error</title>\
             <link rel=\"stylesheet\" href=\"/static/style.css\"></head>\
             <body><main class=\"container\"><h1>{status}</h1><p class=\"error-box\">{}</p>\
             <p><a href=\"/\">Back to dashboard</a></p></main></body></html>",
            escape_html(&self.to_string())
        );
        (status, Html(body)).into_response()
    }
}

fn escape_html(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}
