//! Data helpers: JSON, web requests, and a small memory that survives between runs.
//!
//! ```lua
//! json.encode(value, pretty)         -- a Lua value as JSON text
//! json.decode(text)                  -- JSON text as a Lua value
//! http.get(url, { headers = {...} })  -- { status, ok, body }
//! http.post(url, { json = {...} })   -- or { body = "text" }; same result
//! store.get(key, default)            -- a value saved by an earlier run
//! store.set(key, value)              -- save text, numbers, booleans or tables
//! store.delete(key)
//! ```

use std::{
    cell::RefCell,
    collections::HashMap,
    rc::Rc,
    time::{Duration, Instant},
};

use mlua::{Lua, LuaSerdeExt, SerializeOptions, Table, Value};

fn err(function: &str, error: impl std::fmt::Display) -> mlua::Error {
    mlua::Error::runtime(format!("{function}: {error}"))
}

/// JSON `null` becomes `nil` instead of a special marker value.
pub(crate) fn to_lua(lua: &Lua, value: &serde_json::Value) -> mlua::Result<Value> {
    lua.to_value_with(value, SerializeOptions::new().serialize_none_to_null(false).serialize_unit_to_null(false))
}

fn from_lua(lua: &Lua, value: Value) -> mlua::Result<serde_json::Value> {
    lua.from_value(value)
}

/// Values the script has saved with `store`, as JSON text by key.
#[derive(Debug, Default)]
pub struct StoreState {
    pub values: HashMap<String, String>,
    pub changed: bool,
}

pub type SharedStore = Rc<RefCell<StoreState>>;

pub fn register(lua: &Lua, store: SharedStore, deadline: Instant) -> mlua::Result<()> {
    let globals = lua.globals();
    globals.set("json", json_table(lua)?)?;
    globals.set("http", http_table(lua, deadline)?)?;
    globals.set("store", store_table(lua, store)?)?;
    Ok(())
}

fn json_table(lua: &Lua) -> mlua::Result<Table> {
    let json = lua.create_table()?;
    json.set(
        "encode",
        lua.create_function(|lua, (value, pretty): (Value, Option<bool>)| {
            let value = from_lua(lua, value).map_err(|e| err("json.encode", e))?;
            let text = if pretty.unwrap_or(false) {
                serde_json::to_string_pretty(&value)
            } else {
                serde_json::to_string(&value)
            };
            text.map_err(|e| err("json.encode", e))
        })?,
    )?;
    json.set(
        "decode",
        lua.create_function(|lua, text: String| {
            let value: serde_json::Value = serde_json::from_str(&text).map_err(|e| err("json.decode", format!("not valid JSON: {e}")))?;
            to_lua(lua, &value)
        })?,
    )?;
    Ok(json)
}

// ---- http ----------------------------------------------------------------------

fn http_table(lua: &Lua, deadline: Instant) -> mlua::Result<Table> {
    let http = lua.create_table()?;
    http.set(
        "get",
        lua.create_function(move |lua, (url, options): (String, Option<Table>)| {
            request(lua, "GET", &url, options, deadline).map_err(|e| err("http.get", e))
        })?,
    )?;
    http.set(
        "post",
        lua.create_function(move |lua, (url, options): (String, Option<Table>)| {
            request(lua, "POST", &url, options, deadline).map_err(|e| err("http.post", e))
        })?,
    )?;
    Ok(http)
}

fn request(lua: &Lua, method: &str, url: &str, options: Option<Table>, deadline: Instant) -> Result<Table, String> {
    let lower = url.to_lowercase();
    if !lower.starts_with("http://") && !lower.starts_with("https://") {
        return Err("the address must start with http:// or https://".into());
    }
    let remaining = deadline.saturating_duration_since(Instant::now());
    if remaining.is_zero() {
        return Err("no time left before the script's time limit".into());
    }

    let agent = ureq::AgentBuilder::new()
        .timeout(remaining.min(Duration::from_secs(60)))
        .user_agent(concat!("LocalFlow/", env!("CARGO_PKG_VERSION")))
        .build();
    let mut req = agent.request(method, url);

    let (mut body, mut json_body) = (None::<String>, None::<serde_json::Value>);
    if let Some(options) = &options {
        if let Some(headers) = options.get::<Option<Table>>("headers").map_err(|e| e.to_string())? {
            for pair in headers.pairs::<String, String>() {
                let (name, value) = pair.map_err(|e| e.to_string())?;
                req = req.set(&name, &value);
            }
        }
        body = options.get::<Option<String>>("body").map_err(|e| e.to_string())?;
        if let Some(value) = options.get::<Option<Value>>("json").map_err(|e| e.to_string())? {
            json_body = Some(from_lua(lua, value).map_err(|e| e.to_string())?);
        }
    }

    let result = match (json_body, body) {
        (Some(json), _) => req.send_json(json),
        (None, Some(text)) => req.send_string(&text),
        (None, None) => req.call(),
    };
    // Error status codes still come back as a response, so scripts can check `ok`.
    let response = match result {
        Ok(response) | Err(ureq::Error::Status(_, response)) => response,
        Err(ureq::Error::Transport(e)) => return Err(e.to_string()),
    };

    let status = response.status();
    let text = response.into_string().map_err(|e| format!("could not read the response: {e}"))?;
    let table = lua.create_table().map_err(|e| e.to_string())?;
    table.set("status", status).map_err(|e| e.to_string())?;
    table.set("ok", (200..300).contains(&status)).map_err(|e| e.to_string())?;
    table.set("body", text).map_err(|e| e.to_string())?;
    Ok(table)
}

// ---- store ---------------------------------------------------------------------

fn store_table(lua: &Lua, store: SharedStore) -> mlua::Result<Table> {
    let table = lua.create_table()?;

    let s = store.clone();
    table.set(
        "get",
        lua.create_function(move |lua, (key, default): (String, Value)| {
            match s.borrow().values.get(&key) {
                Some(text) => {
                    let value: serde_json::Value = serde_json::from_str(text).map_err(|e| err("store.get", e))?;
                    to_lua(lua, &value)
                }
                None => Ok(default),
            }
        })?,
    )?;

    let s = store.clone();
    table.set(
        "set",
        lua.create_function(move |lua, (key, value): (String, Value)| {
            if value.is_nil() {
                let mut state = s.borrow_mut();
                state.changed |= state.values.remove(&key).is_some();
                return Ok(());
            }
            let json = from_lua(lua, value).map_err(|e| err("store.set", e))?;
            let text = serde_json::to_string(&json).map_err(|e| err("store.set", e))?;
            if text.len() > 1024 * 1024 {
                return Err(err("store.set", "value is too large (limit 1 MB)"));
            }
            let mut state = s.borrow_mut();
            state.values.insert(key, text);
            state.changed = true;
            Ok(())
        })?,
    )?;

    table.set(
        "delete",
        lua.create_function(move |_, key: String| {
            let mut state = store.borrow_mut();
            state.changed |= state.values.remove(&key).is_some();
            Ok(())
        })?,
    )?;

    Ok(table)
}
