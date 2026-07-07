pub mod automations;
pub mod routes;
pub mod runs;

use axum::{
    http::HeaderMap,
    response::{IntoResponse, Redirect, Response},
};

/// True when the request was sent by HTMX rather than a plain form or link.
pub fn is_htmx(headers: &HeaderMap) -> bool {
    headers.contains_key("hx-request")
}

/// Redirect in a way that works for both HTMX requests and plain HTML forms.
pub fn redirect(headers: &HeaderMap, to: &str) -> Response {
    if is_htmx(headers) {
        ([("HX-Redirect", to.to_string())], "").into_response()
    } else {
        Redirect::to(to).into_response()
    }
}
