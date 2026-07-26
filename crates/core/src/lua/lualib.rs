//! The built-in helper library, written in Lua itself (see `crates/core/lualib/`).
//!
//! ```lua
//! local strings = require("lf.strings")
//! log(strings.title("hello world"))   -- "Hello World"
//! ```
//!
//! `require` only loads these built-in modules. It can't load files from disk,
//! so the sandbox stays closed.

use mlua::{Lua, Table, Value};

/// Every module scripts can `require`, with its Lua source.
pub const MODULES: &[(&str, &str)] = &[
    ("lf.strings", include_str!("../../lualib/lf/strings.lua")),
    ("lf.tables", include_str!("../../lualib/lf/tables.lua")),
    ("lf.paths", include_str!("../../lualib/lf/paths.lua")),
    ("lf.dates", include_str!("../../lualib/lf/dates.lua")),
    ("lf.retry", include_str!("../../lualib/lf/retry.lua")),
    ("lf.template", include_str!("../../lualib/lf/template.lua")),
    ("lf.report", include_str!("../../lualib/lf/report.lua")),
    ("lf.test", include_str!("../../lualib/lf/test.lua")),
];

/// Registry key of the table holding modules that were already loaded in this run.
const LOADED_KEY: &str = "localflow.loaded";

pub fn register(lua: &Lua) -> mlua::Result<()> {
    lua.set_named_registry_value(LOADED_KEY, lua.create_table()?)?;
    lua.globals().set(
        "require",
        lua.create_function(|lua, name: String| {
            let loaded: Table = lua.named_registry_value(LOADED_KEY)?;
            if let Some(module) = loaded.get::<Option<Value>>(name.as_str())? {
                return Ok(module);
            }
            let Some((_, source)) = MODULES.iter().find(|(n, _)| *n == name) else {
                let names: Vec<&str> = MODULES.iter().map(|(n, _)| *n).collect();
                return Err(mlua::Error::runtime(format!(
                    "require: there is no module \"{name}\". Built-in modules: {}",
                    names.join(", ")
                )));
            };
            // Mark it first, so two modules that need each other don't loop.
            loaded.set(name.as_str(), true)?;
            let module: Value = lua.load(*source).set_name(format!("={name}")).call(())?;
            let module = if module.is_nil() { Value::Boolean(true) } else { module };
            loaded.set(name.as_str(), module.clone())?;
            Ok(module)
        })?,
    )
}
