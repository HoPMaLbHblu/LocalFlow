//! Validating and executing Lua automations.

use std::{cell::RefCell, rc::Rc, sync::Arc, time::Duration};

use mlua::{Function, Table};

use super::{api, sandbox::{self, PathPolicy}};
use crate::{
    db::models::AutomationRun,
    errors::{AppError, AppResult},
    state::AppState,
};

/// Name used for the script in error messages, e.g. `automation:3: attempt to call a nil value`.
const CHUNK_NAME: &str = "=automation";

/// Information passed to `run(ctx)`.
#[derive(Debug, Clone)]
pub struct RunContext {
    pub automation_id: i64,
    pub automation_name: String,
    /// `"manual"` or `"schedule"`.
    pub trigger: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct LogLine {
    pub level: String,
    pub message: String,
}

#[derive(Debug)]
pub struct ExecutionResult {
    pub success: bool,
    pub logs: Vec<LogLine>,
    pub error: Option<String>,
}

impl ExecutionResult {
    fn failed(error: String) -> Self {
        ExecutionResult { success: false, logs: Vec::new(), error: Some(error) }
    }

    /// Everything the script logged, one line per message.
    pub fn output(&self) -> String {
        self.logs
            .iter()
            .map(|l| format!("[{}] {}", l.level, l.message))
            .collect::<Vec<_>>()
            .join("\n")
    }
}

/// Check that `code` compiles, without running it.
pub fn validate(code: &str) -> Result<(), String> {
    if code.trim().is_empty() {
        return Err("Lua code must not be empty".into());
    }
    let lua = sandbox::new_lua(Duration::from_secs(1)).map_err(|e| describe(&e))?;
    lua.load(code)
        .set_name(CHUNK_NAME)
        .into_function()
        .map(|_| ())
        .map_err(|e| format!("Lua syntax error: {}", describe(&e)))
}

/// Run a script to completion. This blocks, so call it from `spawn_blocking`.
///
/// Scripts either call `automation { run = function(ctx) ... end }`, in which
/// case `run` is called after the file loads, or are plain top-level code.
pub fn execute(
    code: &str,
    ctx: &RunContext,
    policy: Arc<PathPolicy>,
    timeout: Duration,
) -> ExecutionResult {
    let logs: api::LogSink = Rc::new(RefCell::new(Vec::new()));

    let result = (|| -> mlua::Result<()> {
        let lua = sandbox::new_lua(timeout)?;
        api::register(&lua, policy, logs.clone())?;

        lua.load(code).set_name(CHUNK_NAME).exec()?;

        if let Some(definition) = lua.named_registry_value::<Option<Table>>(api::AUTOMATION_KEY)? {
            let run: Option<Function> = definition.get("run")?;
            let run = run.ok_or_else(|| {
                mlua::Error::runtime("automation { ... } must define a `run = function(ctx) ... end`")
            })?;

            let ctx_table = lua.create_table()?;
            ctx_table.set("id", ctx.automation_id)?;
            ctx_table.set("name", ctx.automation_name.as_str())?;
            ctx_table.set("trigger", ctx.trigger.as_str())?;
            run.call::<()>(ctx_table)?;
        }
        Ok(())
    })();

    let logs = logs.borrow().clone();
    match result {
        Ok(()) => ExecutionResult { success: true, logs, error: None },
        Err(e) => ExecutionResult { success: false, logs, error: Some(describe(&e)) },
    }
}

/// Turn an mlua error into a short, readable message (without Rust-side noise).
fn describe(error: &mlua::Error) -> String {
    match error {
        mlua::Error::CallbackError { cause, .. } => describe(cause),
        mlua::Error::RuntimeError(message) => message.clone(),
        mlua::Error::SyntaxError { message, .. } => message.clone(),
        mlua::Error::MemoryError(message) => format!("out of memory: {message}"),
        mlua::Error::FromLuaConversionError { from, to, message } => format!(
            "bad argument: expected {to}, got {from}{}",
            message.as_deref().map(|m| format!(" ({m})")).unwrap_or_default()
        ),
        mlua::Error::BadArgument { pos, name, cause, .. } => format!(
            "bad argument #{pos}{}: {}",
            name.as_deref().map(|n| format!(" ({n})")).unwrap_or_default(),
            describe(cause)
        ),
        other => other.to_string(),
    }
}

/// Run a stored automation end-to-end: record the run, execute the script,
/// save its logs and result.
pub async fn run_automation(state: &AppState, id: i64, trigger: &str) -> AppResult<AutomationRun> {
    let automation = state.repo.get_automation(id).await?.ok_or(AppError::NotFound)?;
    let run_id = state.repo.start_run(id).await?;
    tracing::info!(automation_id = id, run_id, trigger, "running automation '{}'", automation.name);

    let ctx = RunContext {
        automation_id: id,
        automation_name: automation.name.clone(),
        trigger: trigger.to_string(),
    };
    let code = automation.lua_code;
    let policy = state.path_policy.clone();
    let timeout = state.config.script_timeout;

    let result = tokio::task::spawn_blocking(move || execute(&code, &ctx, policy, timeout))
        .await
        .unwrap_or_else(|e| ExecutionResult::failed(format!("script crashed: {e}")));

    for line in &result.logs {
        state.repo.add_log(id, &line.level, &line.message).await?;
    }
    if let Some(error) = &result.error {
        state.repo.add_log(id, "error", error).await?;
        tracing::warn!(automation_id = id, run_id, "automation failed: {error}");
    }

    let status = if result.success { "success" } else { "failed" };
    state
        .repo
        .finish_run(run_id, status, &result.output(), result.error.as_deref())
        .await?;

    state.repo.get_run(run_id).await?.ok_or(AppError::NotFound)
}
