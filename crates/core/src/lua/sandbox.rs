//! The restricted Lua environment and the rules for which paths scripts may touch.

use std::{
    path::{Component, Path, PathBuf},
    sync::{atomic::{AtomicBool, Ordering}, Arc},
    time::{Duration, Instant},
};

use mlua::{HookTriggers, Lua, LuaOptions, StdLib, Value, VmState};

/// Scripts may not allocate more than this much memory.
const MEMORY_LIMIT: usize = 64 * 1024 * 1024;

/// The error text of a run that was stopped on request.
pub const STOPPED: &str = "stopped by the user";

thread_local! {
    /// The stop flag of the run executing on this thread. A script (and its nested
    /// steps) runs on one blocking thread, so deadline-aware helpers can look here
    /// without every function having to carry the flag.
    static CANCEL: std::cell::RefCell<Option<Arc<AtomicBool>>> = const { std::cell::RefCell::new(None) };
}

/// Makes `flag` the current thread's stop flag until dropped (then restores the previous one).
pub struct CancelScope(Option<Arc<AtomicBool>>);

impl CancelScope {
    pub fn enter(flag: Arc<AtomicBool>) -> Self {
        CancelScope(CANCEL.with(|c| c.borrow_mut().replace(flag)))
    }
}

impl Drop for CancelScope {
    fn drop(&mut self) {
        let previous = self.0.take();
        CANCEL.with(|c| *c.borrow_mut() = previous);
    }
}

/// True if a stop was requested for the run on this thread.
pub fn cancelled() -> bool {
    CANCEL.with(|c| c.borrow().as_ref().is_some_and(|f| f.load(Ordering::Relaxed)))
}

/// The current thread's stop flag (so nested steps share it).
pub fn current_cancel() -> Option<Arc<AtomicBool>> {
    CANCEL.with(|c| c.borrow().clone())
}

/// `Err("stopped by the user")` if a stop was requested.
pub fn check_cancelled() -> mlua::Result<()> {
    if cancelled() {
        Err(mlua::Error::runtime(STOPPED))
    } else {
        Ok(())
    }
}

/// Create a Lua state with only safe standard libraries loaded.
///
/// `io`, `os`, `package` and `debug` are never loaded, and the base functions
/// that can load code from disk or strings are removed. The only way a script
/// can reach the outside world is through the functions in [`super::api`].
pub fn new_lua(timeout: Duration) -> mlua::Result<Lua> {
    let libs = StdLib::TABLE | StdLib::STRING | StdLib::MATH | StdLib::UTF8 | StdLib::COROUTINE;
    let lua = Lua::new_with(libs, LuaOptions::default())?;
    lua.set_memory_limit(MEMORY_LIMIT)?;

    let globals = lua.globals();
    for name in ["dofile", "loadfile", "load", "require", "collectgarbage"] {
        globals.set(name, Value::Nil)?;
    }

    // Stop runaway scripts (e.g. `while true do end`).
    let started = Instant::now();
    lua.set_hook(
        HookTriggers::new().every_nth_instruction(10_000),
        move |_lua, _debug| {
            if cancelled() {
                Err(mlua::Error::runtime(STOPPED))
            } else if started.elapsed() > timeout {
                Err(mlua::Error::runtime(format!(
                    "script timed out after {} seconds",
                    timeout.as_secs()
                )))
            } else {
                Ok(VmState::Continue)
            }
        },
    );

    Ok(lua)
}

/// Tests set this so `~` points at a temporary folder instead of the real home folder.
const TEST_HOME: &str = "LOCALFLOW_TEST_HOME";

fn test_home() -> Option<PathBuf> {
    std::env::var_os(TEST_HOME).map(PathBuf::from)
}

/// Decides which filesystem paths scripts are allowed to use.
#[derive(Debug, Clone)]
pub struct PathPolicy {
    roots: Vec<PathBuf>,
    home: PathBuf,
}

impl PathPolicy {
    /// Allowed directories that do not exist are skipped with a warning.
    pub fn new(allowed_dirs: &[PathBuf]) -> Self {
        let roots = allowed_dirs
            .iter()
            .filter_map(|dir| match dir.canonicalize() {
                Ok(path) => Some(path),
                Err(e) => {
                    tracing::warn!("ignoring allowed directory {}: {e}", dir.display());
                    None
                }
            })
            .collect();

        PathPolicy {
            roots,
            home: test_home().or_else(dirs::home_dir).unwrap_or_else(|| PathBuf::from(".")),
        }
    }

    pub fn roots(&self) -> &[PathBuf] {
        &self.roots
    }

    /// The same rules, plus access to one more folder (e.g. a USB drive that
    /// was just plugged in, for the run it triggered).
    pub fn with_extra_root(&self, folder: &Path) -> Self {
        let mut policy = self.clone();
        if let Ok(root) = folder.canonicalize() {
            policy.roots.push(root);
        }
        policy
    }

    /// Turn a path written in a script into a real path, or explain why it is not allowed.
    ///
    /// `~` expands to the home directory and relative paths are relative to it.
    /// The result must be inside one of the allowed directories, even after
    /// resolving `..` and symlinks.
    pub fn resolve(&self, raw: &str) -> Result<PathBuf, String> {
        if raw.trim().is_empty() {
            return Err("path must not be empty".into());
        }

        let path = normalize(&self.expand(raw));
        let real = canonicalize_existing_prefix(&path);

        if self.roots.iter().any(|root| real.starts_with(root)) {
            Ok(path)
        } else {
            Err(format!("access denied: '{raw}' is outside the allowed directories"))
        }
    }

    fn expand(&self, raw: &str) -> PathBuf {
        if raw == "~" {
            return self.home.clone();
        }
        if let Some(rest) = raw.strip_prefix("~/").or_else(|| raw.strip_prefix("~\\")) {
            return self.home.join(rest);
        }
        let path = PathBuf::from(raw);
        if path.is_absolute() {
            path
        } else {
            self.home.join(path)
        }
    }
}

/// Remove `.` and `..` components without touching the filesystem.
fn normalize(path: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                out.pop();
            }
            other => out.push(other.as_os_str()),
        }
    }
    out
}

/// Canonicalize the deepest part of `path` that exists, so symlinks cannot be
/// used to escape an allowed directory, even when the final file does not exist yet.
fn canonicalize_existing_prefix(path: &Path) -> PathBuf {
    let mut existing = path.to_path_buf();
    let mut missing = Vec::new();

    loop {
        if let Ok(real) = existing.canonicalize() {
            let mut out = real;
            for part in missing.iter().rev() {
                out.push(part);
            }
            return out;
        }
        match (existing.file_name().map(|n| n.to_os_string()), existing.parent()) {
            (Some(name), Some(parent)) => {
                missing.push(name);
                existing = parent.to_path_buf();
            }
            _ => return path.to_path_buf(),
        }
    }
}

/// Case-insensitive filename matching with `*` (any run of characters) and `?` (one character).
pub fn wildcard_match(pattern: &str, name: &str) -> bool {
    let p: Vec<char> = pattern.to_lowercase().chars().collect();
    let n: Vec<char> = name.to_lowercase().chars().collect();

    let (mut pi, mut ni) = (0, 0);
    let mut star: Option<usize> = None;
    let mut star_match = 0;

    while ni < n.len() {
        if pi < p.len() && (p[pi] == '?' || p[pi] == n[ni]) {
            pi += 1;
            ni += 1;
        } else if pi < p.len() && p[pi] == '*' {
            star = Some(pi);
            star_match = ni;
            pi += 1;
        } else if let Some(s) = star {
            // Let the last `*` swallow one more character and retry.
            pi = s + 1;
            star_match += 1;
            ni = star_match;
        } else {
            return false;
        }
    }

    while pi < p.len() && p[pi] == '*' {
        pi += 1;
    }
    pi == p.len()
}
