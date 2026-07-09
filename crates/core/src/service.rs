//! The `LocalFlow` service: the one API both front-ends use.

use std::{
    future::Future,
    pin::Pin,
    sync::{Arc, RwLock},
    time::{Duration, Instant},
};

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::{
    config::CoreConfig,
    db::{
        self,
        models::{Automation, AutomationRun, LogEntry, NewAutomation},
        repository::Repository,
    },
    errors::{CoreError, CoreResult},
    lua::{
        engine::{self, ExecutionResult, LogLine, RunContext},
        sandbox::PathPolicy,
    },
    scheduler::{validate_cron, JobAction, Scheduler},
};

/// Something that happened, for front-ends that show live updates.
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum CoreEvent {
    RunStarted { automation_id: i64, run_id: i64, name: String, trigger: String },
    /// A line written by a script. Both ids are `None` for test runs.
    Log { automation_id: Option<i64>, run_id: Option<i64>, level: String, message: String },
    RunFinished { automation_id: i64, name: String, trigger: String, run: AutomationRun },
    /// Automations were created, changed or deleted.
    AutomationsChanged,
}

pub type EventHandler = Arc<dyn Fn(CoreEvent) + Send + Sync>;

/// The user-editable fields of an automation.
#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct AutomationInput {
    pub name: String,
    #[serde(default)]
    pub description: String,
    pub lua_code: String,
    /// Cron expression; empty or `None` means manual only.
    #[serde(default)]
    pub schedule: Option<String>,
    #[serde(default)]
    pub enabled: bool,
}

impl AutomationInput {
    /// Check every field and collect all problems, so the user can fix them in one go.
    pub fn validate(&self) -> Result<NewAutomation, Vec<String>> {
        let mut errors = Vec::new();

        let name = self.name.trim();
        if name.is_empty() {
            errors.push("Name is required.".to_string());
        } else if name.chars().count() > 100 {
            errors.push("Name must be at most 100 characters.".to_string());
        }

        if let Err(e) = engine::validate(&self.lua_code) {
            errors.push(e);
        }

        let schedule = self.schedule.as_deref().map(str::trim).unwrap_or("");
        if !schedule.is_empty() {
            if let Err(e) = validate_cron(schedule) {
                errors.push(e);
            }
        }

        if !errors.is_empty() {
            return Err(errors);
        }
        Ok(NewAutomation {
            name: name.to_string(),
            description: self.description.trim().to_string(),
            lua_code: self.lua_code.clone(),
            schedule: (!schedule.is_empty()).then(|| schedule.to_string()),
            enabled: self.enabled,
        })
    }
}

/// An automation plus what a list view needs to show about it.
#[derive(Debug, Clone, Serialize)]
pub struct AutomationSummary {
    #[serde(flatten)]
    pub automation: Automation,
    pub last_run: Option<AutomationRun>,
    pub next_run: Option<DateTime<Utc>>,
}

/// Result of running unsaved code from the editor.
#[derive(Debug, Clone, Serialize)]
pub struct TestRunResult {
    pub success: bool,
    pub logs: Vec<LogLine>,
    pub error: Option<String>,
    pub duration_ms: u64,
}

struct Inner {
    repo: Repository,
    scheduler: Scheduler,
    path_policy: RwLock<Arc<PathPolicy>>,
    script_timeout: Duration,
    events: Option<EventHandler>,
}

/// Cheap to clone; all clones share the same database, scheduler and settings.
#[derive(Clone)]
pub struct LocalFlow {
    inner: Arc<Inner>,
}

impl LocalFlow {
    /// Open the database (creating and migrating it if needed). Call [`LocalFlow::start`] afterwards.
    pub async fn open(config: CoreConfig, events: Option<EventHandler>) -> CoreResult<Self> {
        let pool = db::connect(&config.database_url).await?;
        let repo = Repository::new(pool);

        let interrupted = repo.fail_interrupted_runs().await?;
        if interrupted > 0 {
            tracing::warn!(count = interrupted, "marked runs interrupted by the last shutdown as failed");
        }

        Ok(LocalFlow {
            inner: Arc::new(Inner {
                repo,
                scheduler: Scheduler::new().await?,
                path_policy: RwLock::new(Arc::new(PathPolicy::new(&config.allowed_dirs))),
                script_timeout: config.script_timeout,
                events,
            }),
        })
    }

    /// Schedule all enabled automations and start the scheduler.
    pub async fn start(&self) -> CoreResult<()> {
        for automation in self.inner.repo.list_automations().await? {
            if let Err(e) = self.sync_schedule(&automation).await {
                tracing::warn!(automation_id = automation.id, "not scheduled: {e}");
            }
        }
        self.inner.scheduler.start().await
    }

    pub fn repo(&self) -> &Repository {
        &self.inner.repo
    }

    pub fn path_policy(&self) -> Arc<PathPolicy> {
        self.inner.path_policy.read().expect("path policy lock").clone()
    }

    /// Change which folders scripts may access. Applies to the next run.
    pub fn set_allowed_dirs(&self, dirs: &[std::path::PathBuf]) {
        *self.inner.path_policy.write().expect("path policy lock") = Arc::new(PathPolicy::new(dirs));
    }

    fn emit(&self, event: CoreEvent) {
        if let Some(handler) = &self.inner.events {
            handler(event);
        }
    }

    async fn sync_schedule(&self, automation: &Automation) -> CoreResult<()> {
        let flow = self.clone();
        let action: JobAction = Arc::new(move |id| {
            let flow = flow.clone();
            let job: Pin<Box<dyn Future<Output = ()> + Send>> = Box::pin(async move {
                if let Err(e) = flow.run(id, "schedule").await {
                    tracing::error!(automation_id = id, "scheduled run failed: {e}");
                }
            });
            job
        });
        self.inner.scheduler.sync(automation, action).await
    }

    // ---- automations -------------------------------------------------------

    pub async fn list(&self) -> CoreResult<Vec<AutomationSummary>> {
        let mut out = Vec::new();
        for automation in self.inner.repo.list_automations().await? {
            let last_run = self.inner.repo.latest_run(automation.id).await?;
            let next_run = self.inner.scheduler.next_run(automation.id).await;
            out.push(AutomationSummary { automation, last_run, next_run });
        }
        Ok(out)
    }

    pub async fn get(&self, id: i64) -> CoreResult<Automation> {
        self.inner.repo.get_automation(id).await?.ok_or(CoreError::NotFound)
    }

    pub async fn create(&self, input: &AutomationInput) -> CoreResult<Automation> {
        let new = input.validate().map_err(CoreError::Validation)?;
        let automation = self.inner.repo.create_automation(&new).await?;
        self.sync_schedule(&automation).await?;
        tracing::info!(automation_id = automation.id, "created automation '{}'", automation.name);
        self.emit(CoreEvent::AutomationsChanged);
        Ok(automation)
    }

    pub async fn update(&self, id: i64, input: &AutomationInput) -> CoreResult<Automation> {
        let new = input.validate().map_err(CoreError::Validation)?;
        let automation = self
            .inner
            .repo
            .update_automation(id, &new)
            .await?
            .ok_or(CoreError::NotFound)?;
        self.sync_schedule(&automation).await?;
        tracing::info!(automation_id = id, "updated automation");
        self.emit(CoreEvent::AutomationsChanged);
        Ok(automation)
    }

    pub async fn set_enabled(&self, id: i64, enabled: bool) -> CoreResult<Automation> {
        let automation = self
            .inner
            .repo
            .set_enabled(id, enabled)
            .await?
            .ok_or(CoreError::NotFound)?;
        self.sync_schedule(&automation).await?;
        tracing::info!(automation_id = id, enabled, "changed enabled state");
        self.emit(CoreEvent::AutomationsChanged);
        Ok(automation)
    }

    pub async fn toggle(&self, id: i64) -> CoreResult<Automation> {
        let current = self.get(id).await?;
        self.set_enabled(id, !current.enabled).await
    }

    pub async fn delete(&self, id: i64) -> CoreResult<()> {
        self.inner.scheduler.remove(id).await?;
        if !self.inner.repo.delete_automation(id).await? {
            return Err(CoreError::NotFound);
        }
        tracing::info!(automation_id = id, "deleted automation");
        self.emit(CoreEvent::AutomationsChanged);
        Ok(())
    }

    pub async fn is_scheduled(&self, id: i64) -> bool {
        self.inner.scheduler.is_scheduled(id).await
    }

    pub async fn next_run(&self, id: i64) -> Option<DateTime<Utc>> {
        self.inner.scheduler.next_run(id).await
    }

    pub async fn runs(&self, id: i64, limit: i64) -> CoreResult<Vec<AutomationRun>> {
        Ok(self.inner.repo.list_runs(id, limit).await?)
    }

    pub async fn logs(&self, id: i64, limit: i64) -> CoreResult<Vec<LogEntry>> {
        Ok(self.inner.repo.list_logs(id, limit).await?)
    }

    // ---- running -----------------------------------------------------------

    /// Run a stored automation end-to-end: record the run, execute the script,
    /// save its logs and result.
    pub async fn run(&self, id: i64, trigger: &str) -> CoreResult<AutomationRun> {
        let repo = &self.inner.repo;
        let automation = self.get(id).await?;
        let run_id = repo.start_run(id).await?;
        tracing::info!(automation_id = id, run_id, trigger, "running automation '{}'", automation.name);
        self.emit(CoreEvent::RunStarted {
            automation_id: id,
            run_id,
            name: automation.name.clone(),
            trigger: trigger.to_string(),
        });

        let ctx = RunContext {
            automation_id: id,
            automation_name: automation.name.clone(),
            trigger: trigger.to_string(),
        };
        let result = self
            .execute(automation.lua_code, ctx, Some(id), Some(run_id))
            .await;

        for line in &result.logs {
            repo.add_log(id, &line.level, &line.message).await?;
        }
        if let Some(error) = &result.error {
            repo.add_log(id, "error", error).await?;
            tracing::warn!(automation_id = id, run_id, "automation failed: {error}");
        }

        let status = if result.success { "success" } else { "failed" };
        repo.finish_run(run_id, status, &result.output(), result.error.as_deref())
            .await?;

        let run = repo.get_run(run_id).await?.ok_or(CoreError::NotFound)?;
        self.emit(CoreEvent::RunFinished {
            automation_id: id,
            name: automation.name,
            trigger: trigger.to_string(),
            run: run.clone(),
        });
        Ok(run)
    }

    /// Run code that has not been saved. Nothing is written to the database,
    /// but file operations are real.
    pub async fn test_run(&self, code: String, name: String) -> TestRunResult {
        let started = Instant::now();
        let ctx = RunContext { automation_id: 0, automation_name: name, trigger: "test".into() };
        let result = self.execute(code, ctx, None, None).await;
        TestRunResult {
            success: result.success,
            logs: result.logs,
            error: result.error,
            duration_ms: started.elapsed().as_millis() as u64,
        }
    }

    async fn execute(
        &self,
        code: String,
        ctx: RunContext,
        automation_id: Option<i64>,
        run_id: Option<i64>,
    ) -> ExecutionResult {
        let policy = self.path_policy();
        let timeout = self.inner.script_timeout;
        let events = self.inner.events.clone();

        tokio::task::spawn_blocking(move || {
            engine::execute_with(&code, &ctx, policy, timeout, move |line| {
                if let Some(events) = &events {
                    events(CoreEvent::Log {
                        automation_id,
                        run_id,
                        level: line.level.clone(),
                        message: line.message.clone(),
                    });
                }
            })
        })
        .await
        .unwrap_or_else(|e| ExecutionResult::failed(format!("script crashed: {e}")))
    }
}
