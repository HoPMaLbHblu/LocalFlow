use std::{path::PathBuf, time::Duration};

/// Settings shared by every front-end.
#[derive(Debug, Clone)]
pub struct CoreConfig {
    /// SQLite connection string, e.g. `sqlite://localflow.db`.
    pub database_url: String,
    /// Directories Lua scripts are allowed to touch.
    pub allowed_dirs: Vec<PathBuf>,
    /// Maximum run time for a single script.
    pub script_timeout: Duration,
}

impl CoreConfig {
    /// Defaults: the given database, the home folder, a 30 second timeout.
    pub fn new(database_url: impl Into<String>) -> Self {
        CoreConfig {
            database_url: database_url.into(),
            allowed_dirs: dirs::home_dir().into_iter().collect(),
            script_timeout: Duration::from_secs(30),
        }
    }
}
