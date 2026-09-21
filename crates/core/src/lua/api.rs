//! The functions user scripts can call. This is the *only* bridge from Lua to the system.
//!
//! ```lua
//! fs.list(path, pattern)        -- files in a directory matching a wildcard, e.g. "*.pdf"
//! fs.move(source, destination)  -- move/rename a file; returns the new path
//! fs.copy(source, destination)  -- copy a file (a replaced file goes to the Recycle Bin)
//! fs.exists(path)               -- true if the path exists
//! fs.delete(path)               -- move a file or folder to the Recycle Bin; false if it did not exist
//! fs.mkdir(path)                -- create a directory (and parents)
//! fs.is_dir(path)               -- true if the path is a folder
//! fs.size(path)                 -- file size in bytes
//! fs.modified(path)             -- when the file last changed, as a timestamp (see time.*)
//! fs.basename(path)             -- "report.pdf" for "~/Downloads/report.pdf"
//! fs.join(a, b, ...)            -- join path parts
//! log(message)                  -- write to the automation's log
//! notify(message)               -- a highlighted log line meant for the user
//! automation { name = ..., run = function(ctx) ... end }
//! ```

use std::{
    cell::RefCell,
    fmt::Display,
    path::{Path, PathBuf},
    rc::Rc,
    sync::Arc,
};

use mlua::{Lua, Table, Value, Variadic};

use super::{
    engine::LogLine,
    sandbox::{wildcard_match, PathPolicy},
};

/// Registry key under which `automation { ... }` stores its definition.
pub const AUTOMATION_KEY: &str = "localflow.automation";

/// Collects log lines from a script and forwards each one as it is written,
/// so front-ends can show output live.
pub struct LogCollector {
    lines: RefCell<Vec<LogLine>>,
    on_log: Box<dyn Fn(&LogLine)>,
}

impl LogCollector {
    pub fn new(on_log: impl Fn(&LogLine) + 'static) -> Self {
        LogCollector { lines: RefCell::new(Vec::new()), on_log: Box::new(on_log) }
    }

    pub fn lines(&self) -> Vec<LogLine> {
        self.lines.borrow().clone()
    }
}

pub type LogSink = Rc<LogCollector>;

pub fn register(
    lua: &Lua,
    policy: Arc<PathPolicy>,
    logs: LogSink,
    deadline: std::time::Instant,
    store: super::data::SharedStore,
    allow_system: bool,
) -> mlua::Result<()> {
    let globals = lua.globals();
    super::system::register(lua, policy.clone(), deadline)?;
    super::control::register(lua, allow_system, policy.clone(), deadline)?;
    super::desktop::register(lua, allow_system, policy.clone())?;
    super::data::register(lua, store, deadline)?;

    let sink = logs.clone();
    globals.set(
        "log",
        lua.create_function(move |_, message: Value| {
            push(&sink, "info", message.to_string()?);
            Ok(())
        })?,
    )?;

    // `print` behaves like `log` so beginners see their output.
    let sink = logs.clone();
    globals.set(
        "print",
        lua.create_function(move |_, values: Variadic<Value>| {
            let parts = values.iter().map(|v| v.to_string()).collect::<mlua::Result<Vec<_>>>()?;
            push(&sink, "info", parts.join("\t"));
            Ok(())
        })?,
    )?;

    let sink = logs.clone();
    globals.set(
        "notify",
        lua.create_function(move |_, message: Value| {
            let message = message.to_string()?;
            tracing::info!(target: "localflow::notify", "{message}");
            push(&sink, "notify", message);
            Ok(())
        })?,
    )?;

    globals.set(
        "automation",
        lua.create_function(|lua, definition: Table| {
            lua.set_named_registry_value(AUTOMATION_KEY, definition)
        })?,
    )?;

    let fs = fs_table(lua, policy.clone())?;
    super::files::register(lua, &fs, policy.clone(), deadline)?;
    super::media::register(lua, &fs, policy, deadline)?;
    crate::metrics::register(lua)?;
    crate::ai::register(lua, deadline)?;
    super::lualib::register(lua)?;
    globals.set("fs", fs)?;
    Ok(())
}

pub(crate) fn push(sink: &LogSink, level: &str, message: String) {
    let line = LogLine { level: level.to_string(), message };
    (sink.on_log)(&line);
    sink.lines.borrow_mut().push(line);
}

fn fs_err(function: &str, error: impl Display) -> mlua::Error {
    mlua::Error::runtime(format!("{function}: {error}"))
}

fn path_string(path: &Path) -> String {
    path.to_string_lossy().into_owned()
}

/// Moving or copying into an existing directory keeps the file name, like `mv`/`cp`.
fn target_path(source: &Path, destination: PathBuf) -> PathBuf {
    match source.file_name() {
        Some(name) if destination.is_dir() => destination.join(name),
        _ => destination,
    }
}

fn fs_table(lua: &Lua, policy: Arc<PathPolicy>) -> mlua::Result<Table> {
    let fs = lua.create_table()?;

    let p = policy.clone();
    fs.set(
        "list",
        lua.create_function(move |_, (path, pattern): (String, Option<String>)| {
            let dir = p.resolve(&path).map_err(|e| fs_err("fs.list", e))?;
            if !dir.is_dir() {
                return Err(fs_err("fs.list", format!("directory not found: {path}")));
            }
            let pattern = pattern.unwrap_or_else(|| "*".to_string());

            let mut files = Vec::new();
            for entry in std::fs::read_dir(&dir).map_err(|e| fs_err("fs.list", e))? {
                let entry = entry.map_err(|e| fs_err("fs.list", e))?;
                let name = entry.file_name().to_string_lossy().into_owned();
                if entry.path().is_file() && wildcard_match(&pattern, &name) {
                    files.push(path_string(&entry.path()));
                }
            }
            files.sort();
            Ok(files)
        })?,
    )?;

    let p = policy.clone();
    fs.set(
        "move",
        lua.create_function(move |_, (source, destination): (String, String)| {
            let src = p.resolve(&source).map_err(|e| fs_err("fs.move", e))?;
            if !src.exists() {
                return Err(fs_err("fs.move", format!("source not found: {source}")));
            }
            let dst = target_path(&src, p.resolve(&destination).map_err(|e| fs_err("fs.move", e))?);
            if dst.exists() {
                return Err(fs_err("fs.move", format!("destination already exists: {}", dst.display())));
            }
            if let Some(parent) = dst.parent() {
                std::fs::create_dir_all(parent).map_err(|e| fs_err("fs.move", e))?;
            }
            // `rename` fails across drives, so fall back to copy + delete for files.
            if std::fs::rename(&src, &dst).is_err() {
                if !src.is_file() {
                    return Err(fs_err("fs.move", format!("could not move directory {source}")));
                }
                std::fs::copy(&src, &dst).map_err(|e| fs_err("fs.move", e))?;
                std::fs::remove_file(&src).map_err(|e| fs_err("fs.move", e))?;
            }
            Ok(path_string(&dst))
        })?,
    )?;

    let p = policy.clone();
    fs.set(
        "copy",
        lua.create_function(move |_, (source, destination): (String, String)| {
            let src = p.resolve(&source).map_err(|e| fs_err("fs.copy", e))?;
            if !src.is_file() {
                return Err(fs_err("fs.copy", format!("source file not found: {source}")));
            }
            let dst = target_path(&src, p.resolve(&destination).map_err(|e| fs_err("fs.copy", e))?);
            if let Some(parent) = dst.parent() {
                std::fs::create_dir_all(parent).map_err(|e| fs_err("fs.copy", e))?;
            }
            if dst != src {
                super::recycle::keep_old_version(&dst).map_err(|e| fs_err("fs.copy", e))?;
            }
            std::fs::copy(&src, &dst).map_err(|e| fs_err("fs.copy", e))?;
            Ok(path_string(&dst))
        })?,
    )?;

    let p = policy.clone();
    fs.set(
        "exists",
        lua.create_function(move |_, path: String| {
            let path = p.resolve(&path).map_err(|e| fs_err("fs.exists", e))?;
            Ok(path.exists())
        })?,
    )?;

    let p = policy.clone();
    fs.set(
        "delete",
        lua.create_function(move |_, path: String| {
            let resolved = p.resolve(&path).map_err(|e| fs_err("fs.delete", e))?;
            if !resolved.exists() {
                return Ok(false);
            }
            // Never permanent: files and folders go to the Recycle Bin.
            super::recycle::recycle(&resolved).map_err(|e| fs_err("fs.delete", e))?;
            Ok(true)
        })?,
    )?;

    let p = policy.clone();
    fs.set(
        "is_dir",
        lua.create_function(move |_, path: String| {
            Ok(p.resolve(&path).map_err(|e| fs_err("fs.is_dir", e))?.is_dir())
        })?,
    )?;

    let p = policy.clone();
    fs.set(
        "size",
        lua.create_function(move |_, path: String| {
            let resolved = p.resolve(&path).map_err(|e| fs_err("fs.size", e))?;
            let meta = std::fs::metadata(&resolved).map_err(|_| fs_err("fs.size", format!("not found: {path}")))?;
            Ok(meta.len())
        })?,
    )?;

    let p = policy.clone();
    fs.set(
        "modified",
        lua.create_function(move |_, path: String| {
            let resolved = p.resolve(&path).map_err(|e| fs_err("fs.modified", e))?;
            let modified = std::fs::metadata(&resolved)
                .and_then(|m| m.modified())
                .map_err(|_| fs_err("fs.modified", format!("not found: {path}")))?;
            let seconds = modified
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_secs() as i64)
                .unwrap_or(0);
            Ok(seconds)
        })?,
    )?;

    let p = policy;
    fs.set(
        "mkdir",
        lua.create_function(move |_, path: String| {
            let resolved = p.resolve(&path).map_err(|e| fs_err("fs.mkdir", e))?;
            std::fs::create_dir_all(&resolved).map_err(|e| fs_err("fs.mkdir", e))?;
            Ok(path_string(&resolved))
        })?,
    )?;

    // Pure string helpers: no filesystem access, so no policy check needed.
    fs.set(
        "basename",
        lua.create_function(|_, path: String| {
            Ok(Path::new(&path)
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default())
        })?,
    )?;

    fs.set(
        "join",
        lua.create_function(|_, parts: Variadic<String>| {
            let joined: PathBuf = parts.iter().collect();
            Ok(path_string(&joined))
        })?,
    )?;

    Ok(fs)
}
