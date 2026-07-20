//! The `LocalFlow` service: the one API both front-ends use.

use std::{
    collections::HashMap,
    future::Future,
    pin::Pin,
    sync::{Arc, RwLock},
    time::{Duration, Instant},
};

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::{
    backup::{BackupInfo, Backups},
    config::CoreConfig,
    db::{
        self,
        models::{Automation, AutomationRun, AutomationVersion, LogEntry, NewAutomation},
        repository::Repository,
    },
    errors::{CoreError, CoreResult},
    lua::{
        chain::{self, Library, Step},
        engine::{self, ExecutionResult, LogLine, RunContext},
        sandbox::PathPolicy,
    },
    scheduler::{validate_cron, JobAction, Scheduler},
    triggers::ExtraTriggers,
    watcher::{WatchAction, Watchers},
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
    /// Run every time LocalFlow starts.
    #[serde(default)]
    pub run_on_startup: bool,
    /// Run when a new file appears in this folder; empty or `None` means don't watch.
    #[serde(default)]
    pub watch_path: Option<String>,
    /// Only react to files matching this pattern, e.g. `*.pdf`. Defaults to `*`.
    #[serde(default)]
    pub watch_pattern: Option<String>,
    /// Let the script use powerful functions (commands, keystrokes, shutdown, ...).
    #[serde(default)]
    pub allow_system: bool,
    /// Hotkey, app start/exit, idle and USB triggers.
    #[serde(default)]
    pub triggers: ExtraTriggers,
}

fn non_empty(value: &Option<String>) -> Option<String> {
    value.as_deref().map(str::trim).filter(|v| !v.is_empty()).map(String::from)
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
        errors.extend(self.triggers.problems());

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
            run_on_startup: self.run_on_startup,
            watch_path: non_empty(&self.watch_path),
            watch_pattern: non_empty(&self.watch_pattern).filter(|_| non_empty(&self.watch_path).is_some()),
            allow_system: self.allow_system,
            triggers: self.triggers.normalized().to_json(),
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
    watchers: Watchers,
    path_policy: RwLock<Arc<PathPolicy>>,
    script_timeout: RwLock<Duration>,
    events: Option<EventHandler>,
    backups: Option<Backups>,
    /// What happened while opening the database, for the UI to report.
    startup_notice: Option<StartupNotice>,
}

/// Something the user should hear about after startup.
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum StartupNotice {
    /// A backup chosen in Settings was restored.
    Restored { backup: String },
    /// The database was damaged; the newest backup was restored and the damaged copy kept.
    RecoveredFromDamage { backup: String },
}

/// Cheap to clone; all clones share the same database, scheduler and settings.
#[derive(Clone)]
pub struct LocalFlow {
    inner: Arc<Inner>,
}

impl LocalFlow {
    /// Open the database (creating and migrating it if needed). Call [`LocalFlow::start`] afterwards.
    ///
    /// Protects the data on the way: applies a restore chosen in Settings,
    /// recovers from a damaged database using the newest backup (keeping the
    /// damaged copy), and backs up before a new version changes the database.
    pub async fn open(config: CoreConfig, events: Option<EventHandler>) -> CoreResult<Self> {
        let backups = Backups::for_database_url(&config.database_url);
        let backup_error = |e: String| CoreError::Validation(vec![e]);
        let mut startup_notice = None;

        if let Some(backups) = &backups {
            if let Some(backup) = backups.apply_pending_restore().map_err(backup_error)? {
                startup_notice = Some(StartupNotice::Restored { backup });
            }
        }

        let mut pool = db::open(&config.database_url).await?;
        if let (Some(backups), Err(problem)) = (&backups, db::check_integrity(&pool).await) {
            tracing::error!("database integrity check failed: {problem}");
            pool.close().await;
            let backup = backups.recover_from_damage().map_err(backup_error)?;
            startup_notice = Some(StartupNotice::RecoveredFromDamage { backup });
            pool = db::open(&config.database_url).await?;
        }

        if let Some(backups) = &backups {
            if db::is_initialized(&pool).await && db::needs_migration(&pool).await {
                backups.create(&pool, "before-update").await.map_err(backup_error)?;
            }
        }
        db::migrate(&pool).await?;
        let repo = Repository::new(pool);

        let interrupted = repo.fail_interrupted_runs().await?;
        if interrupted > 0 {
            tracing::warn!(count = interrupted, "marked runs interrupted by the last shutdown as failed");
        }

        Ok(LocalFlow {
            inner: Arc::new(Inner {
                repo,
                scheduler: Scheduler::new().await?,
                watchers: Watchers::new(),
                path_policy: RwLock::new(Arc::new(PathPolicy::new(&config.allowed_dirs))),
                script_timeout: RwLock::new(config.script_timeout),
                events,
                backups,
                startup_notice,
            }),
        })
    }

    /// Close the database cleanly (all changes are written first).
    pub async fn close(&self) {
        self.inner.repo.pool().close().await;
    }

    /// Anything that happened to the database at startup that the user should know about.
    pub fn startup_notice(&self) -> Option<StartupNotice> {
        self.inner.startup_notice.clone()
    }

    /// Start schedules and folder watches, then run the "when LocalFlow starts" automations.
    pub async fn start(&self) -> CoreResult<()> {
        let automations = self.inner.repo.list_automations().await?;
        for automation in &automations {
            if let Err(e) = self.sync_triggers(automation).await {
                tracing::warn!(automation_id = automation.id, "trigger not set up: {e}");
            }
        }
        self.inner.scheduler.start().await?;

        // App start/exit, idle and USB triggers.
        tokio::spawn(crate::triggers::monitor(self.clone()));

        // A backup every day, also when LocalFlow runs for weeks without a restart.
        if self.inner.backups.is_some() {
            let flow = self.clone();
            tokio::spawn(async move {
                loop {
                    flow.daily_backup().await;
                    tokio::time::sleep(Duration::from_secs(60 * 60)).await;
                }
            });
        }

        let startup: Vec<i64> = automations
            .iter()
            .filter(|a| a.enabled && a.run_on_startup)
            .map(|a| a.id)
            .collect();
        if !startup.is_empty() {
            let flow = self.clone();
            // One after another, in name order, without delaying startup.
            tokio::spawn(async move {
                for id in startup {
                    if let Err(e) = flow.run(id, "startup").await {
                        tracing::error!(automation_id = id, "startup run failed: {e}");
                    }
                }
            });
        }
        Ok(())
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

    pub fn script_timeout(&self) -> Duration {
        *self.inner.script_timeout.read().expect("timeout lock")
    }

    /// Change how long a script may run. Applies to the next run.
    pub fn set_script_timeout(&self, timeout: Duration) {
        *self.inner.script_timeout.write().expect("timeout lock") = timeout;
    }

    fn emit(&self, event: CoreEvent) {
        if let Some(handler) = &self.inner.events {
            handler(event);
        }
    }

    /// Make schedules and folder watches match an automation's current settings.
    async fn sync_triggers(&self, automation: &Automation) -> CoreResult<()> {
        self.sync_schedule(automation).await?;

        let folder = match (&automation.watch_path, automation.enabled) {
            (Some(path), true) => Some(self.resolve_watch_folder(path).map_err(|e| CoreError::Validation(vec![e]))?),
            _ => None,
        };
        let flow = self.clone();
        let action: WatchAction = Arc::new(move |id, file| {
            let flow = flow.clone();
            let job: Pin<Box<dyn Future<Output = ()> + Send>> = Box::pin(async move {
                if let Err(e) = flow.run_with_file(id, "watch", Some(file)).await {
                    tracing::error!(automation_id = id, "watch run failed: {e}");
                }
            });
            job
        });
        self.inner
            .watchers
            .sync(automation.id, folder, automation.watch_pattern.clone(), action)
            .await
    }

    /// A watch folder must exist and be inside the allowed folders.
    fn resolve_watch_folder(&self, path: &str) -> Result<std::path::PathBuf, String> {
        let resolved = self
            .path_policy()
            .resolve(path)
            .map_err(|e| format!("Watch folder: {e}"))?;
        if !resolved.is_dir() {
            return Err(format!("Watch folder not found: {path}"));
        }
        Ok(resolved)
    }

    fn check_watch(&self, new: &NewAutomation) -> CoreResult<()> {
        if let (Some(path), true) = (&new.watch_path, new.enabled) {
            self.resolve_watch_folder(path).map_err(|e| CoreError::Validation(vec![e]))?;
        }
        Ok(())
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

    /// An automation that is not in the trash.
    pub async fn get(&self, id: i64) -> CoreResult<Automation> {
        let automation = self.get_any(id).await?;
        if automation.deleted_at.is_some() {
            return Err(CoreError::NotFound);
        }
        Ok(automation)
    }

    /// An automation, even if it is in the trash.
    pub async fn get_any(&self, id: i64) -> CoreResult<Automation> {
        self.inner.repo.get_automation(id).await?.ok_or(CoreError::NotFound)
    }

    pub async fn create(&self, input: &AutomationInput) -> CoreResult<Automation> {
        let new = input.validate().map_err(CoreError::Validation)?;
        self.check_watch(&new)?;
        let automation = self.inner.repo.create_automation(&new).await?;
        self.sync_triggers(&automation).await?;
        tracing::info!(automation_id = automation.id, "created automation '{}'", automation.name);
        self.emit(CoreEvent::AutomationsChanged);
        Ok(automation)
    }

    pub async fn update(&self, id: i64, input: &AutomationInput) -> CoreResult<Automation> {
        let new = input.validate().map_err(CoreError::Validation)?;
        self.check_watch(&new)?;
        let automation = self
            .inner
            .repo
            .update_automation(id, &new)
            .await?
            .ok_or(CoreError::NotFound)?;
        self.sync_triggers(&automation).await?;
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
        self.sync_triggers(&automation).await?;
        tracing::info!(automation_id = id, enabled, "changed enabled state");
        self.emit(CoreEvent::AutomationsChanged);
        Ok(automation)
    }

    pub async fn toggle(&self, id: i64) -> CoreResult<Automation> {
        let current = self.get(id).await?;
        self.set_enabled(id, !current.enabled).await
    }

    /// Move an automation to the trash. Its code, history, logs and saved
    /// values are all kept, and [`LocalFlow::restore`] brings it back.
    pub async fn delete(&self, id: i64) -> CoreResult<()> {
        self.get(id).await?; // already in the trash counts as not found
        self.inner.scheduler.remove(id).await?;
        self.inner.watchers.remove(id).await;
        if !self.inner.repo.set_deleted(id, true).await? {
            return Err(CoreError::NotFound);
        }
        tracing::info!(automation_id = id, "moved automation to the trash");
        self.emit(CoreEvent::AutomationsChanged);
        Ok(())
    }

    /// Take an automation out of the trash, with its triggers as they were.
    pub async fn restore(&self, id: i64) -> CoreResult<Automation> {
        if !self.inner.repo.set_deleted(id, false).await? {
            return Err(CoreError::NotFound);
        }
        let automation = self.get(id).await?;
        if let Err(e) = self.sync_triggers(&automation).await {
            // e.g. its watch folder no longer exists: restore it, but switched off.
            tracing::warn!(automation_id = id, "restored without triggers: {e}");
            return self.set_enabled(id, false).await;
        }
        tracing::info!(automation_id = id, "restored automation from the trash");
        self.emit(CoreEvent::AutomationsChanged);
        Ok(automation)
    }

    pub async fn trash(&self) -> CoreResult<Vec<Automation>> {
        Ok(self.inner.repo.list_trash().await?)
    }

    /// Permanently delete an automation that is already in the trash.
    /// A backup is taken first, so even this can be undone from Settings › Backups.
    pub async fn delete_forever(&self, id: i64) -> CoreResult<()> {
        let automation = self.get_any(id).await?;
        if automation.deleted_at.is_none() {
            return Err(CoreError::Validation(vec!["Only automations in the trash can be deleted permanently.".into()]));
        }
        if let Some(backups) = &self.inner.backups {
            backups
                .create(self.inner.repo.pool(), "before-delete")
                .await
                .map_err(|e| CoreError::Validation(vec![e]))?;
        }
        self.inner.repo.delete_automation(id).await?;
        tracing::info!(automation_id = id, "permanently deleted automation");
        self.emit(CoreEvent::AutomationsChanged);
        Ok(())
    }

    // ---- versions ------------------------------------------------------------

    pub async fn versions(&self, id: i64) -> CoreResult<Vec<AutomationVersion>> {
        Ok(self.inner.repo.list_versions(id).await?)
    }

    /// Put an earlier version back. The current state becomes a version itself,
    /// so this can be undone too. The enabled switch is left as it is.
    pub async fn restore_version(&self, id: i64, version_id: i64) -> CoreResult<Automation> {
        let version = self
            .inner
            .repo
            .get_version(version_id)
            .await?
            .filter(|v| v.automation_id == id)
            .ok_or(CoreError::NotFound)?;
        let current = self.get(id).await?;
        let input = AutomationInput {
            name: version.name,
            description: version.description,
            lua_code: version.lua_code,
            schedule: version.schedule,
            enabled: current.enabled,
            run_on_startup: version.run_on_startup,
            watch_path: version.watch_path,
            watch_pattern: version.watch_pattern,
            allow_system: current.allow_system,
            triggers: ExtraTriggers::from_json(version.triggers.as_deref()),
        };
        self.update(id, &input).await
    }

    // ---- backups -------------------------------------------------------------

    pub fn backups_dir(&self) -> Option<std::path::PathBuf> {
        self.inner.backups.as_ref().map(|b| b.dir().to_path_buf())
    }

    pub fn list_backups(&self) -> Vec<BackupInfo> {
        self.inner.backups.as_ref().map(|b| b.list()).unwrap_or_default()
    }

    pub async fn backup_now(&self) -> CoreResult<BackupInfo> {
        let backups = self.inner.backups.as_ref().ok_or_else(|| CoreError::Validation(vec!["Backups are not available.".into()]))?;
        backups.create(self.inner.repo.pool(), "manual").await.map_err(|e| CoreError::Validation(vec![e]))
    }

    /// Replace everything with a backup the next time LocalFlow starts.
    /// The current state is backed up first.
    pub async fn schedule_restore(&self, file_name: &str) -> CoreResult<()> {
        let backups = self.inner.backups.as_ref().ok_or_else(|| CoreError::Validation(vec!["Backups are not available.".into()]))?;
        backups
            .schedule_restore(self.inner.repo.pool(), file_name)
            .await
            .map_err(|e| CoreError::Validation(vec![e]))
    }

    /// Make today's automatic backup if there isn't one yet.
    async fn daily_backup(&self) {
        let Some(backups) = &self.inner.backups else { return };
        if backups.has_daily_backup_today() {
            return;
        }
        match backups.create(self.inner.repo.pool(), "daily").await {
            Ok(_) => backups.prune(),
            Err(e) => tracing::error!("daily backup failed: {e}"),
        }
    }

    pub async fn is_scheduled(&self, id: i64) -> bool {
        self.inner.scheduler.is_scheduled(id).await
    }

    pub async fn is_watching(&self, id: i64) -> bool {
        self.inner.watchers.is_watching(id).await
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

    // ---- sharing -----------------------------------------------------------

    /// The contents of a `.localflow` file for this automation.
    pub async fn export(&self, id: i64) -> CoreResult<String> {
        let automation = self.get(id).await?;
        Ok(crate::sharing::SharedAutomation::from_automation(&automation).to_json())
    }

    /// Check a `.localflow` file and describe it, without saving anything.
    pub fn preview_import(&self, text: &str) -> CoreResult<crate::sharing::ImportPreview> {
        crate::sharing::preview(text).map_err(|e| CoreError::Validation(vec![e]))
    }

    /// Create an automation from a `.localflow` file. It starts disabled.
    pub async fn import(&self, text: &str) -> CoreResult<Automation> {
        let shared = crate::sharing::SharedAutomation::parse(text).map_err(|e| CoreError::Validation(vec![e]))?;
        let automation = self.create(&shared.to_input()).await?;
        tracing::info!(automation_id = automation.id, "imported automation '{}'", automation.name);
        Ok(automation)
    }

    // ---- running -----------------------------------------------------------

    /// Run a stored automation end-to-end: record the run, execute the script,
    /// save its logs and result.
    pub async fn run(&self, id: i64, trigger: &str) -> CoreResult<AutomationRun> {
        self.run_with_details(id, trigger, None, HashMap::new()).await
    }

    /// Like [`LocalFlow::run`], passing a file to the script as `ctx.file`.
    pub async fn run_with_file(&self, id: i64, trigger: &str, file: Option<String>) -> CoreResult<AutomationRun> {
        self.run_with_details(id, trigger, file, HashMap::new()).await
    }

    /// Like [`LocalFlow::run`], with `ctx.file` and more `ctx` values (e.g. `ctx.app`).
    pub async fn run_with_details(
        &self,
        id: i64,
        trigger: &str,
        file: Option<String>,
        details: HashMap<String, String>,
    ) -> CoreResult<AutomationRun> {
        self.run_full(id, trigger.to_string(), file, details, None, 0).await
    }

    /// Everything a run can be given. `depth` counts "run after" links, so a loop of
    /// automations starting each other stops instead of running forever.
    fn run_full(
        &self,
        id: i64,
        trigger: String,
        file: Option<String>,
        details: HashMap<String, String>,
        input: Option<serde_json::Value>,
        depth: usize,
    ) -> Pin<Box<dyn Future<Output = CoreResult<AutomationRun>> + Send + 'static>> {
        let this = self.clone();
        Box::pin(async move { this.run_inner(id, &trigger, file, details, input, depth).await })
    }

    async fn run_inner(
        &self,
        id: i64,
        trigger: &str,
        file: Option<String>,
        details: HashMap<String, String>,
        input: Option<serde_json::Value>,
        depth: usize,
    ) -> CoreResult<AutomationRun> {
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

        // A run started by a USB drive may use that drive.
        let extra_root = details.get("drive").map(std::path::PathBuf::from);
        let ctx = RunContext {
            file,
            details,
            input,
            library: self.library_for(&automation.lua_code).await?,
            ..RunContext::new(id, automation.name.clone(), trigger, automation.allow_system)
        };
        let store = repo.load_store(id).await?;
        let result = self
            .execute(automation.lua_code, ctx, Some(id), Some(run_id), store, extra_root)
            .await;
        if let Some(store) = &result.store {
            repo.save_store(id, store).await?;
        }
        for (step_id, store) in &result.step_stores {
            repo.save_store(*step_id, store).await?;
        }

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
            name: automation.name.clone(),
            trigger: trigger.to_string(),
            run: run.clone(),
        });
        self.start_followers(id, &automation.name, result.success, result.result, depth).await;
        Ok(run)
    }

    /// Start the automations set to "run after" this one. Each gets what this one
    /// returned as `ctx.input`.
    async fn start_followers(&self, id: i64, name: &str, success: bool, result: Option<serde_json::Value>, depth: usize) {
        let Ok(list) = self.inner.repo.list_automations().await else { return };
        for follower in list.into_iter().filter(|a| a.enabled && a.id != id) {
            let Some(after) = ExtraTriggers::from_json(follower.triggers.as_deref()).after else { continue };
            if after.automation_id != id || !after.matches(success) {
                continue;
            }
            if depth + 1 >= chain::MAX_DEPTH {
                tracing::warn!(automation_id = follower.id, "not started: too many automations in a row");
                continue;
            }
            let details = HashMap::from([
                ("previous".to_string(), name.to_string()),
                ("previous_ok".to_string(), success.to_string()),
            ]);
            let run = self.run_full(follower.id, "after".into(), None, details, result.clone(), depth + 1);
            tokio::spawn(async move {
                if let Err(e) = run.await {
                    tracing::error!("\"run after\" failed: {e}");
                }
            });
        }
    }

    /// The automations a script may run as steps. Only loaded when the script uses them.
    async fn library_for(&self, code: &str) -> CoreResult<Option<Arc<Library>>> {
        if !code.contains("automations.") {
            return Ok(None);
        }
        let repo = &self.inner.repo;
        let mut steps = Vec::new();
        for a in repo.list_automations().await? {
            let store = repo.load_store(a.id).await?;
            steps.push(Step {
                id: a.id,
                name: a.name,
                code: a.lua_code,
                enabled: a.enabled,
                allow_system: a.allow_system,
                store,
            });
        }
        Ok(Some(Arc::new(Library { steps })))
    }

    /// Run code that has not been saved. Nothing is written to the database,
    /// but file operations are real.
    pub async fn test_run(&self, code: String, name: String) -> TestRunResult {
        self.test_run_with(code, name, false).await
    }

    /// A test run that may use powerful functions if `allow_system` is on.
    pub async fn test_run_with(&self, code: String, name: String, allow_system: bool) -> TestRunResult {
        let started = Instant::now();
        let ctx = RunContext {
            // A missing library only means steps can't be found.
            library: self.library_for(&code).await.unwrap_or_default(),
            ..RunContext::new(0, name, "test", allow_system)
        };
        // Test runs start with an empty store and don't save it.
        let result = self.execute(code, ctx, None, None, Default::default(), None).await;
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
        store: std::collections::HashMap<String, String>,
        extra_root: Option<std::path::PathBuf>,
    ) -> ExecutionResult {
        let policy = match extra_root {
            Some(folder) => Arc::new(self.path_policy().with_extra_root(&folder)),
            None => self.path_policy(),
        };
        let timeout = self.script_timeout();
        let events = self.inner.events.clone();

        tokio::task::spawn_blocking(move || {
            engine::execute_with(&code, &ctx, policy, timeout, store, move |line| {
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
