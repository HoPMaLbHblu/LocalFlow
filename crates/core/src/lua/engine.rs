//! Validating and executing Lua scripts. Pure functions: no database, no async.

use std::{cell::RefCell, collections::HashMap, rc::Rc, sync::Arc, time::Duration};

use mlua::{Function, LuaSerdeExt, Table, Value};
use serde::Serialize;

use super::{
    api::{self, LogCollector},
    chain::{self, ChainEnv, Library, StepStores},
    data::{self, SharedStore, StoreState},
    sandbox::{self, PathPolicy},
};

/// Name used for the script in error messages, e.g. `automation:3: attempt to call a nil value`.
const CHUNK_NAME: &str = "=automation";

/// Information passed to `run(ctx)`.
#[derive(Debug, Clone)]
pub struct RunContext {
    pub automation_id: i64,
    pub automation_name: String,
    /// `"manual"`, `"schedule"`, `"startup"`, `"watch"`, `"after"`, `"step"` or `"test"`.
    pub trigger: String,
    /// For `"watch"` runs: the file that appeared.
    pub file: Option<String>,
    /// More values for `ctx`, e.g. `app` for app triggers or `drive` for USB.
    pub details: HashMap<String, String>,
    /// Whether powerful functions (commands, keystrokes, shutdown, ...) may run.
    pub allow_system: bool,
    /// `ctx.input`: data from the automation that started this one.
    pub input: Option<serde_json::Value>,
    /// Automations this run may start with `automations.run`.
    pub library: Option<Arc<Library>>,
    /// Automations already running in this chain (for steps), outermost first.
    pub stack: Vec<i64>,
    /// Set to true to stop the run (see `LocalFlow::stop_run`). Shared with steps.
    /// Limit: a blocking OS call already in flight (long `shell.run`, `ask()`, `speak`)
    /// is not interrupted; the stop takes effect when it returns or at the next check.
    pub cancel: Arc<std::sync::atomic::AtomicBool>,
}

impl RunContext {
    /// A context with no input, steps or extra details.
    pub fn new(automation_id: i64, automation_name: impl Into<String>, trigger: impl Into<String>, allow_system: bool) -> Self {
        RunContext {
            automation_id,
            automation_name: automation_name.into(),
            trigger: trigger.into(),
            file: None,
            details: HashMap::new(),
            allow_system,
            input: None,
            library: None,
            stack: Vec::new(),
            cancel: Arc::default(),
        }
    }
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
    /// The script's saved values, if it changed any with `store.set`.
    pub store: Option<HashMap<String, String>>,
    /// What `run(ctx)` (or the script) returned, if it can be stored as JSON.
    pub result: Option<serde_json::Value>,
    /// Saved values changed by steps (`automations.run`), by automation id.
    pub step_stores: HashMap<i64, HashMap<String, String>>,
}

impl ExecutionResult {
    pub fn failed(error: String) -> Self {
        ExecutionResult {
            success: false,
            logs: Vec::new(),
            error: Some(error),
            store: None,
            result: None,
            step_stores: HashMap::new(),
        }
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
    execute_with(code, ctx, policy, timeout, HashMap::new(), |_| {})
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
    store: HashMap<String, String>,
    on_log: impl Fn(&LogLine) + 'static,
) -> ExecutionResult {
    let _cancel_scope = sandbox::CancelScope::enter(ctx.cancel.clone());
    let logs = Rc::new(LogCollector::new(on_log));
    let store: SharedStore = Rc::new(RefCell::new(StoreState { values: store, changed: false }));
    let step_stores: StepStores = Rc::default();

    let result = (|| -> mlua::Result<Option<serde_json::Value>> {
        let lua = sandbox::new_lua(timeout)?;
        let deadline = std::time::Instant::now() + timeout;
        api::register(&lua, policy.clone(), logs.clone(), deadline, store.clone(), ctx.allow_system)?;
        let mut stack = ctx.stack.clone();
        if stack.is_empty() && ctx.automation_id != 0 {
            stack.push(ctx.automation_id);
        }
        chain::register(
            &lua,
            ChainEnv {
                library: ctx.library.clone().unwrap_or_default(),
                stack,
                caller: ctx.automation_name.clone(),
                policy,
                deadline,
                logs: logs.clone(),
                stores: step_stores.clone(),
            },
        )?;

        // Available both as the `run(ctx)` argument and as a global for plain scripts.
        let ctx_table = lua.create_table()?;
        ctx_table.set("id", ctx.automation_id)?;
        ctx_table.set("name", ctx.automation_name.as_str())?;
        ctx_table.set("trigger", ctx.trigger.as_str())?;
        ctx_table.set("file", ctx.file.as_deref())?;
        for (key, value) in &ctx.details {
            ctx_table.set(key.as_str(), value.as_str())?;
        }
        if let Some(input) = &ctx.input {
            ctx_table.set("input", data::to_lua(&lua, input)?)?;
        }
        lua.globals().set("ctx", &ctx_table)?;

        let mut returned: Value = lua.load(code).set_name(CHUNK_NAME).call(())?;

        if let Some(definition) = lua.named_registry_value::<Option<Table>>(api::AUTOMATION_KEY)? {
            let run: Option<Function> = definition.get("run")?;
            let run = run.ok_or_else(|| {
                mlua::Error::runtime("automation { ... } must define a `run = function(ctx) ... end`")
            })?;
            returned = run.call(ctx_table)?;
        }
        // Functions and other things JSON can't hold are simply not passed on.
        Ok(match returned {
            Value::Nil => None,
            value => lua.from_value::<serde_json::Value>(value).ok(),
        })
    })();

    let logs = logs.lines();
    let store = {
        let state = store.borrow();
        state.changed.then(|| state.values.clone())
    };
    let step_stores = step_stores.take();
    // A script can swallow the stop error with pcall; a stopped run is still a stopped run.
    let result = match result {
        Ok(_) if sandbox::cancelled() => Err(mlua::Error::runtime(sandbox::STOPPED)),
        other => other,
    };
    let result = match result {
        Err(_) if sandbox::cancelled() => Err(mlua::Error::runtime(sandbox::STOPPED)),
        other => other,
    };
    match result {
        Ok(result) => ExecutionResult { success: true, logs, error: None, store, result, step_stores },
        Err(e) => ExecutionResult { success: false, logs, error: Some(describe(&e)), store, result: None, step_stores },
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
