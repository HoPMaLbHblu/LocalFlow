use chrono::DateTime;
use serde::Serialize;

/// A row in the `automations` table.
#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
pub struct Automation {
    pub id: i64,
    pub name: String,
    pub description: String,
    pub lua_code: String,
    pub schedule: Option<String>,
    pub enabled: bool,
    pub created_at: String,
    pub updated_at: String,
    /// Run once every time LocalFlow starts.
    pub run_on_startup: bool,
    /// Run whenever a new file appears in this folder.
    pub watch_path: Option<String>,
    /// With `watch_path`: only files whose names match, e.g. `*.pdf`.
    pub watch_pattern: Option<String>,
    /// Set when the automation is in the trash.
    pub deleted_at: Option<String>,
}

/// An earlier saved state of an automation (from `automation_versions`).
#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
pub struct AutomationVersion {
    pub id: i64,
    pub automation_id: i64,
    pub name: String,
    pub description: String,
    pub lua_code: String,
    pub schedule: Option<String>,
    pub run_on_startup: bool,
    pub watch_path: Option<String>,
    pub watch_pattern: Option<String>,
    /// When this version was replaced by a newer one.
    pub saved_at: String,
}

/// The user-editable fields of an automation, used for create and update.
#[derive(Debug, Clone)]
pub struct NewAutomation {
    pub name: String,
    pub description: String,
    pub lua_code: String,
    pub schedule: Option<String>,
    pub enabled: bool,
    pub run_on_startup: bool,
    pub watch_path: Option<String>,
    pub watch_pattern: Option<String>,
}

/// A row in the `automation_runs` table.
#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
pub struct AutomationRun {
    pub id: i64,
    pub automation_id: i64,
    pub status: String,
    pub output: Option<String>,
    pub error: Option<String>,
    pub started_at: String,
    pub finished_at: Option<String>,
}

impl AutomationRun {
    pub fn duration_ms(&self) -> Option<i64> {
        let started = DateTime::parse_from_rfc3339(&self.started_at).ok()?;
        let finished = DateTime::parse_from_rfc3339(self.finished_at.as_deref()?).ok()?;
        Some((finished - started).num_milliseconds())
    }
}

/// A run plus a human-readable duration, for templates.
#[derive(Debug, Clone, Serialize)]
pub struct RunView {
    #[serde(flatten)]
    pub run: AutomationRun,
    pub duration: Option<String>,
}

impl From<AutomationRun> for RunView {
    fn from(run: AutomationRun) -> Self {
        let duration = run.duration_ms().map(|ms| {
            if ms < 1000 {
                format!("{ms} ms")
            } else {
                format!("{:.2} s", ms as f64 / 1000.0)
            }
        });
        RunView { run, duration }
    }
}

/// A row in the `logs` table.
#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
pub struct LogEntry {
    pub id: i64,
    pub automation_id: i64,
    pub level: String,
    pub message: String,
    pub created_at: String,
}
