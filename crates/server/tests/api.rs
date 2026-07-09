use std::time::Duration;

use axum::{
    body::Body,
    http::{header, HeaderMap, Method, Request, StatusCode},
    Router,
};
use http_body_util::BodyExt;
use localflow::{api::routes::router, state::AppState};
use localflow_core::{CoreConfig, LocalFlow};
use tempfile::TempDir;
use tower::ServiceExt;

struct TestApp {
    router: Router,
    state: AppState,
    _dir: TempDir,
}

async fn app() -> TestApp {
    let dir = TempDir::new().unwrap();
    let config = CoreConfig {
        database_url: "sqlite::memory:".into(),
        allowed_dirs: vec![dir.path().to_path_buf()],
        script_timeout: Duration::from_secs(5),
    };
    let flow = LocalFlow::open(config, None).await.unwrap();
    let state = AppState::new(flow);
    TestApp { router: router(state.clone()), state, _dir: dir }
}

struct TestResponse {
    status: StatusCode,
    headers: HeaderMap,
    body: String,
}

impl TestApp {
    async fn send(&self, method: Method, uri: &str, form: Option<&str>, htmx: bool) -> TestResponse {
        let mut request = Request::builder().method(method).uri(uri);
        if form.is_some() {
            request = request.header(header::CONTENT_TYPE, "application/x-www-form-urlencoded");
        }
        if htmx {
            request = request.header("HX-Request", "true");
        }
        let request = request.body(Body::from(form.unwrap_or("").to_string())).unwrap();

        let response = self.router.clone().oneshot(request).await.unwrap();
        let status = response.status();
        let headers = response.headers().clone();
        let bytes = response.into_body().collect().await.unwrap().to_bytes();
        TestResponse { status, headers, body: String::from_utf8_lossy(&bytes).into_owned() }
    }

    async fn get(&self, uri: &str) -> TestResponse {
        self.send(Method::GET, uri, None, false).await
    }

    async fn post_form(&self, uri: &str, form: &str) -> TestResponse {
        self.send(Method::POST, uri, Some(form), false).await
    }

    /// Create an automation through the API and return its id.
    async fn create(&self, form: &str) -> i64 {
        let response = self.post_form("/automations", form).await;
        assert_eq!(response.status, StatusCode::SEE_OTHER, "{}", response.body);
        let location = response.headers[header::LOCATION].to_str().unwrap();
        location.trim_start_matches("/automations/").parse().unwrap()
    }
}

const VALID: &str = "name=Greeter&description=says+hi&lua_code=log%28%27hello%27%29&schedule=&enabled=on";

#[tokio::test]
async fn dashboard_and_new_form_render() {
    let app = app().await;

    let response = app.get("/").await;
    assert_eq!(response.status, StatusCode::OK);
    assert!(response.body.contains("No automations yet"));

    let response = app.get("/automations/new?template=organize-pdfs").await;
    assert_eq!(response.status, StatusCode::OK);
    assert!(response.body.contains("Organize PDF files"));
    assert!(response.body.contains("fs.list"));

    assert_eq!(app.get("/static/style.css").await.status, StatusCode::OK);
}

#[tokio::test]
async fn create_view_and_list() {
    let app = app().await;
    let id = app.create(VALID).await;

    let detail = app.get(&format!("/automations/{id}")).await;
    assert_eq!(detail.status, StatusCode::OK);
    assert!(detail.body.contains("Greeter"));
    assert!(detail.body.contains("says hi"));

    let list = app.get("/automations").await;
    assert!(list.body.contains("Greeter"));

    let edit = app.get(&format!("/automations/{id}/edit")).await;
    assert_eq!(edit.status, StatusCode::OK);
    assert!(edit.body.contains("Save changes"));
}

#[tokio::test]
async fn invalid_lua_is_rejected() {
    let app = app().await;
    let response = app
        .post_form("/automations", "name=Broken&lua_code=if+then+end&schedule=")
        .await;

    assert_eq!(response.status, StatusCode::UNPROCESSABLE_ENTITY);
    assert!(response.body.contains("Lua syntax error"));
    assert!(app.state.flow.repo().list_automations().await.unwrap().is_empty());
}

#[tokio::test]
async fn invalid_schedule_and_missing_name_are_rejected() {
    let app = app().await;
    let response = app
        .post_form("/automations", "name=&lua_code=log%281%29&schedule=not+a+cron")
        .await;

    assert_eq!(response.status, StatusCode::UNPROCESSABLE_ENTITY);
    assert!(response.body.contains("Name is required"));
    assert!(response.body.contains("Invalid schedule"));
}

#[tokio::test]
async fn scheduled_automation_is_registered_and_toggle_pauses_it() {
    let app = app().await;
    let id = app
        .create("name=Timed&lua_code=log%281%29&schedule=0+*%2F5+*+*+*+*&enabled=on")
        .await;
    assert!(app.state.flow.is_scheduled(id).await);

    let response = app.post_form(&format!("/automations/{id}/toggle"), "").await;
    assert_eq!(response.status, StatusCode::SEE_OTHER);
    assert!(!app.state.flow.repo().get_automation(id).await.unwrap().unwrap().enabled);
    assert!(!app.state.flow.is_scheduled(id).await);

    app.post_form(&format!("/automations/{id}/toggle"), "next=%2F").await;
    assert!(app.state.flow.is_scheduled(id).await);
}

#[tokio::test]
async fn manual_run_records_history_and_logs() {
    let app = app().await;
    let id = app.create(VALID).await;

    let response = app
        .send(Method::POST, &format!("/automations/{id}/run"), None, true)
        .await;
    assert_eq!(response.status, StatusCode::OK);
    assert!(response.body.contains("success"));
    assert!(response.body.contains("hello"));

    let runs = app.get(&format!("/automations/{id}/runs")).await;
    assert_eq!(runs.status, StatusCode::OK);
    assert!(runs.body.contains("success"));

    let logs = app.get(&format!("/automations/{id}/logs")).await;
    assert!(logs.body.contains("hello"));
}

#[tokio::test]
async fn failed_run_is_recorded_as_failed() {
    let app = app().await;
    let id = app
        .create("name=Fails&lua_code=error%28%27nope%27%29&schedule=&enabled=on")
        .await;

    let response = app
        .send(Method::POST, &format!("/automations/{id}/run"), None, true)
        .await;
    assert!(response.body.contains("failed"));
    assert!(response.body.contains("nope"));

    let run = app.state.flow.repo().latest_run(id).await.unwrap().unwrap();
    assert_eq!(run.status, "failed");
}

#[tokio::test]
async fn update_with_htmx_put() {
    let app = app().await;
    let id = app.create(VALID).await;

    let response = app
        .send(
            Method::PUT,
            &format!("/automations/{id}"),
            Some("name=Renamed&lua_code=log%282%29&schedule="),
            true,
        )
        .await;
    assert_eq!(response.status, StatusCode::OK);
    assert_eq!(response.headers["hx-redirect"], format!("/automations/{id}").as_str());

    let automation = app.state.flow.repo().get_automation(id).await.unwrap().unwrap();
    assert_eq!(automation.name, "Renamed");
    assert!(!automation.enabled, "unchecked checkbox should disable");
}

#[tokio::test]
async fn delete_removes_automation() {
    let app = app().await;
    let id = app.create(VALID).await;

    let response = app
        .send(Method::DELETE, &format!("/automations/{id}"), None, true)
        .await;
    assert_eq!(response.headers["hx-redirect"], "/");

    assert_eq!(app.get(&format!("/automations/{id}")).await.status, StatusCode::NOT_FOUND);
    let again = app.send(Method::DELETE, &format!("/automations/{id}"), None, true).await;
    assert_eq!(again.status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn unknown_automation_is_404() {
    let app = app().await;
    for uri in ["/automations/42", "/automations/42/edit", "/automations/42/runs", "/automations/42/logs"] {
        assert_eq!(app.get(uri).await.status, StatusCode::NOT_FOUND, "{uri}");
    }
    let response = app.post_form("/automations/42/run", "").await;
    assert_eq!(response.status, StatusCode::NOT_FOUND);
}
