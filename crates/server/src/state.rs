use std::{
    net::{IpAddr, Ipv4Addr},
    sync::Arc,
    time::Duration,
};

use axum::response::Html;
use localflow_core::{CoreConfig, LocalFlow};
use minijinja::Environment;
use serde::Serialize;

use crate::errors::AppResult;

/// Settings read from environment variables (or a `.env` file).
#[derive(Debug, Clone)]
pub struct Config {
    pub host: IpAddr,
    pub port: u16,
    pub core: CoreConfig,
}

impl Config {
    pub fn from_env() -> Self {
        let localhost = IpAddr::V4(Ipv4Addr::LOCALHOST);

        let mut host = env_or("LOCALFLOW_HOST", "127.0.0.1").parse().unwrap_or_else(|_| {
            tracing::warn!("LOCALFLOW_HOST is not a valid IP address, using 127.0.0.1");
            localhost
        });

        // Local-only mode: refuse to listen on a public interface unless explicitly allowed.
        let allow_remote = env_or("LOCALFLOW_ALLOW_REMOTE", "false").eq_ignore_ascii_case("true");
        if !host.is_loopback() && !allow_remote {
            tracing::warn!(
                "{host} is not a loopback address; binding to 127.0.0.1 instead \
                 (set LOCALFLOW_ALLOW_REMOTE=true to override)"
            );
            host = localhost;
        }

        let port = env_or("LOCALFLOW_PORT", "3000").parse().unwrap_or(3000);

        let mut core = CoreConfig::new(env_or("DATABASE_URL", "sqlite://localflow.db"));
        if let Some(value) = std::env::var_os("LOCALFLOW_ALLOWED_DIRS").filter(|v| !v.is_empty()) {
            core.allowed_dirs = std::env::split_paths(&value).collect();
        }
        if let Ok(secs) = env_or("LOCALFLOW_SCRIPT_TIMEOUT_SECS", "30").parse() {
            core.script_timeout = Duration::from_secs(secs);
        }

        Config { host, port, core }
    }
}

fn env_or(key: &str, default: &str) -> String {
    std::env::var(key).unwrap_or_else(|_| default.to_string())
}

/// Shared state handed to every request handler.
#[derive(Clone)]
pub struct AppState {
    pub flow: LocalFlow,
    pub templates: Arc<Environment<'static>>,
}

impl AppState {
    pub fn new(flow: LocalFlow) -> Self {
        AppState { flow, templates: Arc::new(templates()) }
    }

    /// Render one of the HTML templates in `templates/`.
    pub fn render(&self, name: &str, ctx: impl Serialize) -> AppResult<Html<String>> {
        let template = self.templates.get_template(name)?;
        Ok(Html(template.render(ctx)?))
    }
}

/// Templates are compiled into the binary so `cargo run` works from any directory.
fn templates() -> Environment<'static> {
    let mut env = Environment::new();
    let files = [
        ("base.html", include_str!("../templates/base.html")),
        ("macros.html", include_str!("../templates/macros.html")),
        ("dashboard.html", include_str!("../templates/dashboard.html")),
        ("form.html", include_str!("../templates/form.html")),
        ("detail.html", include_str!("../templates/detail.html")),
        ("runs.html", include_str!("../templates/runs.html")),
        ("logs.html", include_str!("../templates/logs.html")),
        ("run_result.html", include_str!("../templates/run_result.html")),
    ];
    for (name, source) in files {
        env.add_template(name, source)
            .unwrap_or_else(|e| panic!("template {name} is invalid: {e}"));
    }
    env
}
