//! Runs enabled automations on their cron schedule.

use std::collections::HashMap;

use chrono::Local;
use tokio::sync::Mutex;
use tokio_cron_scheduler::{Job, JobScheduler, JobSchedulerError};
use uuid::Uuid;

use crate::{
    db::models::Automation,
    errors::{AppError, AppResult},
    lua::engine::run_automation,
    state::AppState,
};

/// Explains the expected cron format in error messages.
pub const CRON_HELP: &str =
    "Use 6 fields: second minute hour day-of-month month day-of-week, e.g. \"0 */5 * * * *\" (every 5 minutes)";

pub struct Scheduler {
    inner: JobScheduler,
    /// Automation id -> scheduled job id.
    jobs: Mutex<HashMap<i64, Uuid>>,
}

fn internal(e: JobSchedulerError) -> AppError {
    AppError::Internal(format!("scheduler: {e}"))
}

/// Check a cron expression without scheduling anything.
pub fn validate_cron(expr: &str) -> Result<(), String> {
    Job::new_async_tz(expr, Local, |_, _| Box::pin(async {}))
        .map(|_| ())
        .map_err(|_| format!("Invalid schedule \"{expr}\". {CRON_HELP}"))
}

impl Scheduler {
    pub async fn new() -> AppResult<Self> {
        Ok(Scheduler {
            inner: JobScheduler::new().await.map_err(internal)?,
            jobs: Mutex::new(HashMap::new()),
        })
    }

    pub async fn start(&self) -> AppResult<()> {
        self.inner.start().await.map_err(internal)
    }

    /// Schedule every enabled automation that has a schedule. Called at startup.
    pub async fn load_all(&self, state: &AppState) -> AppResult<()> {
        for automation in state.repo.list_automations().await? {
            if let Err(e) = self.sync(state, &automation).await {
                tracing::warn!(automation_id = automation.id, "not scheduled: {e}");
            }
        }
        Ok(())
    }

    /// Make the scheduler match an automation's current settings.
    /// Call after every create, update or toggle.
    pub async fn sync(&self, state: &AppState, automation: &Automation) -> AppResult<()> {
        self.remove(automation.id).await?;

        let schedule = automation.schedule.as_deref().map(str::trim).unwrap_or("");
        if schedule.is_empty() || !automation.enabled {
            return Ok(());
        }

        let id = automation.id;
        let job_state = state.clone();
        let job = Job::new_async_tz(schedule, Local, move |_job_id, _scheduler| {
            let state = job_state.clone();
            Box::pin(async move {
                if let Err(e) = run_automation(&state, id, "schedule").await {
                    tracing::error!(automation_id = id, "scheduled run failed: {e}");
                }
            })
        })
        .map_err(|_| AppError::BadRequest(format!("Invalid schedule \"{schedule}\". {CRON_HELP}")))?;

        let job_id = self.inner.add(job).await.map_err(internal)?;
        self.jobs.lock().await.insert(id, job_id);
        tracing::info!(automation_id = id, schedule, "scheduled automation");
        Ok(())
    }

    /// Stop scheduling an automation (no-op if it was not scheduled).
    pub async fn remove(&self, automation_id: i64) -> AppResult<()> {
        let job_id = self.jobs.lock().await.remove(&automation_id);
        if let Some(job_id) = job_id {
            self.inner.remove(&job_id).await.map_err(internal)?;
        }
        Ok(())
    }

    pub async fn is_scheduled(&self, automation_id: i64) -> bool {
        self.jobs.lock().await.contains_key(&automation_id)
    }
}
