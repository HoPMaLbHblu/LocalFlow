//! Running saved automations from a script, so small automations can be combined
//! into bigger ones ("steps").
//!
//! ```lua
//! automations.list()                 -- { { id, name, enabled }, ... }
//! automations.run(name_or_id, input) -- { ok, error, result }; never stops the script
//! automations.call(name_or_id, input) -- the step's result; stops the script if the step fails
//! ```
//!
//! A step gets `input` as `ctx.input`, and whatever its `run(ctx)` returns comes back as `result`.
//! Steps run one after another in the same run, share its time limit, and keep their own
//! "Allow system control" setting and saved values. Disabled automations can still be used as steps.

use std::{
    cell::RefCell,
    collections::HashMap,
    rc::Rc,
    sync::Arc,
    time::Instant,
};

use mlua::{Lua, LuaSerdeExt, Value};

use super::{
    api::{self, LogSink},
    data,
    engine::{self, RunContext},
    sandbox::PathPolicy,
};

/// How many automations may be running inside each other at once.
pub const MAX_DEPTH: usize = 8;

/// A saved automation that scripts can run as a step.
#[derive(Debug, Clone)]
pub struct Step {
    pub id: i64,
    pub name: String,
    pub code: String,
    pub enabled: bool,
    pub allow_system: bool,
    /// Its saved values (`store`) when the run started.
    pub store: HashMap<String, String>,
}

/// All automations a run may use as steps.
#[derive(Debug, Default)]
pub struct Library {
    pub steps: Vec<Step>,
}

impl Library {
    /// By id, or by name (ignoring case and spaces around it).
    pub fn find(&self, key: &Value) -> Option<&Step> {
        match key {
            Value::Integer(id) => self.steps.iter().find(|s| s.id == *id),
            Value::Number(id) => self.steps.iter().find(|s| s.id as f64 == *id),
            Value::String(name) => {
                let name = name.to_string_lossy();
                let name = name.trim().to_lowercase();
                self.steps.iter().find(|s| s.name.trim().to_lowercase() == name)
            }
            _ => None,
        }
    }
}

/// Saved values changed by steps during this run, by automation id.
pub type StepStores = Rc<RefCell<HashMap<i64, HashMap<String, String>>>>;

/// What a step needs from the run that starts it.
pub struct ChainEnv {
    pub library: Arc<Library>,
    /// Automations already running in this chain, outermost first.
    pub stack: Vec<i64>,
    pub caller: String,
    pub policy: Arc<PathPolicy>,
    pub deadline: Instant,
    pub logs: LogSink,
    pub stores: StepStores,
}

fn err(function: &str, message: impl std::fmt::Display) -> mlua::Error {
    mlua::Error::runtime(format!("{function}: {message}"))
}

pub fn register(lua: &Lua, env: ChainEnv) -> mlua::Result<()> {
    let env = Rc::new(env);
    let automations = lua.create_table()?;

    let e = env.clone();
    automations.set(
        "list",
        lua.create_function(move |lua, ()| {
            let list = lua.create_table()?;
            for step in &e.library.steps {
                let item = lua.create_table()?;
                item.set("id", step.id)?;
                item.set("name", step.name.as_str())?;
                item.set("enabled", step.enabled)?;
                list.push(item)?;
            }
            Ok(list)
        })?,
    )?;

    let e = env.clone();
    automations.set(
        "run",
        lua.create_function(move |lua, (key, input): (Value, Value)| {
            let outcome = run_step(lua, &e, "automations.run", &key, input)?;
            let table = lua.create_table()?;
            table.set("ok", outcome.success)?;
            table.set("error", outcome.error)?;
            table.set("result", outcome.result)?;
            Ok(table)
        })?,
    )?;

    let e = env.clone();
    automations.set(
        "call",
        lua.create_function(move |lua, (key, input): (Value, Value)| {
            let outcome = run_step(lua, &e, "automations.call", &key, input)?;
            match outcome.error {
                None => Ok(outcome.result),
                Some(error) => Err(err("automations.call", format!("step \"{}\" failed: {error}", outcome.name))),
            }
        })?,
    )?;

    lua.globals().set("automations", automations)?;
    Ok(())
}

struct Outcome {
    name: String,
    success: bool,
    error: Option<String>,
    result: Value,
}

fn run_step(lua: &Lua, env: &ChainEnv, function: &str, key: &Value, input: Value) -> mlua::Result<Outcome> {
    let step = env.library.find(key).ok_or_else(|| {
        let shown = match key {
            Value::String(s) => format!("\"{}\"", s.to_string_lossy()),
            other => other.to_string().unwrap_or_else(|_| "?".into()),
        };
        err(function, format!("there is no automation named {shown} (check the name in the sidebar)"))
    })?;
    if env.stack.contains(&step.id) {
        return Err(err(
            function,
            format!("\"{}\" is already running in this chain, so it would call itself forever", step.name),
        ));
    }
    if env.stack.len() >= MAX_DEPTH {
        return Err(err(function, format!("steps can go at most {MAX_DEPTH} levels deep")));
    }
    let remaining = env.deadline.saturating_duration_since(Instant::now());
    if remaining.is_zero() {
        return Err(err(function, "the time limit was reached"));
    }
    let input = match input {
        Value::Nil => None,
        value => Some(lua.from_value::<serde_json::Value>(value).map_err(|e| err(function, format!("bad input: {e}")))?),
    };

    let mut stack = env.stack.clone();
    stack.push(step.id);
    let ctx = RunContext {
        automation_id: step.id,
        automation_name: step.name.clone(),
        trigger: "step".into(),
        file: None,
        details: HashMap::from([("caller".to_string(), env.caller.clone())]),
        allow_system: step.allow_system,
        input,
        library: Some(env.library.clone()),
        stack,
    };
    let store = env.stores.borrow().get(&step.id).cloned().unwrap_or_else(|| step.store.clone());

    // The step's lines appear in this run's log, marked with the step's name.
    let sink = env.logs.clone();
    let prefix = step.name.clone();
    let result = engine::execute_with(&step.code, &ctx, env.policy.clone(), remaining, store, move |line| {
        api::push(&sink, &line.level, format!("[{prefix}] {}", line.message));
    });

    {
        let mut stores = env.stores.borrow_mut();
        stores.extend(result.step_stores);
        if let Some(store) = result.store {
            stores.insert(step.id, store);
        }
    }
    let value = match &result.result {
        Some(json) => data::to_lua(lua, json)?,
        None => Value::Nil,
    };
    Ok(Outcome { name: step.name.clone(), success: result.success, error: result.error, result: value })
}

/// Just for tests and callers that want to build a library by hand.
pub fn library(steps: Vec<Step>) -> Arc<Library> {
    Arc::new(Library { steps })
}
