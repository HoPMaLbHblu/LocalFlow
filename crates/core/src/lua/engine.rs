//! Validating and executing Lua scripts. Pure functions: no database, no async.

use std::{rc::Rc, sync::Arc, time::Duration};

use mlua::{Function, Table};
use serde::Serialize;

use super::{
    api::{self, LogCollector},
    sandbox::{self, PathPolicy},
};

/// Name used for the script in error messages, e.g. `automation:3: attempt to call a nil value`.
const CHUNK_NAME: &str = "=automation";

/// Information passed to `run(ctx)`.
#[derive(Debug, Clone)]
pub struct RunContext {
    pub automation_id: i64,
    pub automation_name: String,
    /// `"manual"`, `"schedule"`, `"startup"`, `"watch"` or `"test"`.
    pub trigger: String,
    /// For `"watch"` runs: the file that appeared.
    pub file: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
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
    pub fn failed(error: String) -> Self {
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
pub fn execute(
    code: &str,
    ctx: &RunContext,
    policy: Arc<PathPolicy>,
    timeout: Duration,
) -> ExecutionResult {
    execute_with(code, ctx, policy, timeout, |_| {})
}

/// Like [`execute`], calling `on_log` for every line as soon as the script writes it.
///
/// Scripts either call `automation { run = function(ctx) ... end }`, in which
/// case `run` is called after the file loads, or are plain top-level code.
pub fn execute_with(
    code: &str,
    ctx: &RunContext,
    policy: Arc<PathPolicy>,
    timeout: Duration,
    on_log: impl Fn(&LogLine) + 'static,
) -> ExecutionResult {
    let logs = Rc::new(LogCollector::new(on_log));

    let result = (|| -> mlua::Result<()> {
        let lua = sandbox::new_lua(timeout)?;
        api::register(&lua, policy, logs.clone(), std::time::Instant::now() + timeout)?;

        // Available both as the `run(ctx)` argument and as a global for plain scripts.
        let ctx_table = lua.create_table()?;
        ctx_table.set("id", ctx.automation_id)?;
        ctx_table.set("name", ctx.automation_name.as_str())?;
        ctx_table.set("trigger", ctx.trigger.as_str())?;
        ctx_table.set("file", ctx.file.as_deref())?;
        lua.globals().set("ctx", &ctx_table)?;

        lua.load(code).set_name(CHUNK_NAME).exec()?;

        if let Some(definition) = lua.named_registry_value::<Option<Table>>(api::AUTOMATION_KEY)? {
            let run: Option<Function> = definition.get("run")?;
            let run = run.ok_or_else(|| {
                mlua::Error::runtime("automation { ... } must define a `run = function(ctx) ... end`")
            })?;
            run.call::<()>(ctx_table)?;
        }
        Ok(())
    })();

    let logs = logs.lines();
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
