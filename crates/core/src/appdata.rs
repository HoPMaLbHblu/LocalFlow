//! LocalFlow's own data folder for small files that aren't in the database (link sets, ...).
//! The desktop app sets it to its app-data folder; tests use `LOCALFLOW_DATA_DIR`.

use std::{path::PathBuf, sync::RwLock};

static DIR: RwLock<Option<PathBuf>> = RwLock::new(None);

pub fn set_dir(dir: PathBuf) {
    *DIR.write().unwrap() = Some(dir);
}

pub fn dir() -> PathBuf {
    if let Some(dir) = std::env::var_os("LOCALFLOW_DATA_DIR") {
        return PathBuf::from(dir);
    }
    DIR.read().unwrap().clone().unwrap_or_else(|| std::env::temp_dir().join("localflow-data"))
}

/// Write via a temporary file and a rename, keeping the previous version as `<name>.bak`,
/// so a crash or a mistake never loses the file.
pub fn write_safely(path: &std::path::Path, bytes: &[u8]) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    let tmp = path.with_extension("tmp");
    std::fs::write(&tmp, bytes).map_err(|e| e.to_string())?;
    if path.exists() {
        let _ = std::fs::copy(path, path.with_extension("bak"));
    }
    std::fs::rename(&tmp, path).map_err(|e| e.to_string())
}
