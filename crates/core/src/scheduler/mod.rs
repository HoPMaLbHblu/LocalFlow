//! Runs enabled automations on their cron schedule.

use std::{collections::HashMap, future::Future, pin::Pin, sync::Arc};

use chrono::{DateTime, Local, Utc};
use tokio::sync::Mutex;
use tokio_cron_scheduler::{Job, JobScheduler, JobSchedulerError};
use uuid::Uuid;

use crate::{
    db::models::Automation,
    errors::{CoreError, CoreResult},
};

/// Explains the expected cron format in error messages.
pub const CRON_HELP: &str =
    "Use 6 fields: second minute hour day-of-month month day-of-week, e.g. \"0 */5 * * * *\" (every 5 minutes)";

/// What a scheduled job does when it fires: run automation `id`.
pub type JobAction = Arc<dyn Fn(i64) -> Pin<Box<dyn Future<Output = ()> + Send>> + Send + Sync>;

pub struct Scheduler {
    inner: JobScheduler,
    /// Automation id -> scheduled job id.
    jobs: Mutex<HashMap<i64, Uuid>>,
}

fn internal(e: JobSchedulerError) -> CoreError {
    CoreError::Scheduler(e.to_string())
}

fn invalid(expr: &str) -> String {
    format!("Invalid schedule \"{expr}\". {CRON_HELP}")
}

/// Check a cron expression without scheduling anything.
pub fn validate_cron(expr: &str) -> Result<(), String> {
    Job::new_async_tz(expr, Local, |_, _| Box::pin(async {}))
        .map(|_| ())
        .map_err(|_| invalid(expr))
}

impl Scheduler {
    pub async fn new() -> CoreResult<Self> {
        Ok(Scheduler {
            inner: JobScheduler::new().await.map_err(internal)?,
            jobs: Mutex::new(HashMap::new()),
        })
    }

    pub async fn start(&self) -> CoreResult<()> {
        self.inner.start().await.map_err(internal)
    }

    /// Make the scheduler match an automation's current settings.
    /// Call after every create, update or toggle.
    pub async fn sync(&self, automation: &Automation, action: JobAction) -> CoreResult<()> {
        self.remove(automation.id).await?;

        let schedule = automation.schedule.as_deref().map(str::trim).unwrap_or("");
        if schedule.is_empty() || !automation.enabled {
            return Ok(());
        }

        let id = automation.id;
        let job = Job::new_async_tz(schedule, Local, move |_job_id, _scheduler| action(id))
            .map_err(|_| CoreError::Validation(vec![invalid(schedule)]))?;

        let job_id = self.inner.add(job).await.map_err(internal)?;
        self.jobs.lock().await.insert(id, job_id);
        tracing::info!(automation_id = id, schedule, "scheduled automation");
        Ok(())
    }

    /// Stop scheduling an automation (no-op if it was not scheduled).
    pub async fn remove(&self, automation_id: i64) -> CoreResult<()> {
        let job_id = self.jobs.lock().await.remove(&automation_id);
        if let Some(job_id) = job_id {
            self.inner.remove(&job_id).await.map_err(internal)?;
        }
        Ok(())
    }

    pub async fn is_scheduled(&self, automation_id: i64) -> bool {
        self.jobs.lock().await.contains_key(&automation_id)
    }

    /// When the automation will next run, if it is scheduled.
    pub async fn next_run(&self, automation_id: i64) -> Option<DateTime<Utc>> {
        let job_id = *self.jobs.lock().await.get(&automation_id)?;
        self.inner.clone().next_tick_for_job(job_id).await.ok().flatten()
    }
}
