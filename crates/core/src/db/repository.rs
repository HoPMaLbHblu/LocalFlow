use chrono::{SecondsFormat, Utc};
use sqlx::SqlitePool;

use super::models::{Automation, AutomationRun, AutomationVersion, LogEntry, NewAutomation};

/// Current time as an RFC 3339 string, the format every timestamp column uses.
pub fn now() -> String {
    Utc::now().to_rfc3339_opts(SecondsFormat::Millis, true)
}

/// All database access goes through this type.
#[derive(Clone)]
pub struct Repository {
    pool: SqlitePool,
}

impl Repository {
    pub fn new(pool: SqlitePool) -> Self {
        Repository { pool }
    }

    /// The connection pool, for backups (`VACUUM INTO`).
    pub fn pool(&self) -> &SqlitePool {
        &self.pool
    }

    // ---- automations -------------------------------------------------------

    /// Automations that are not in the trash.
    pub async fn list_automations(&self) -> sqlx::Result<Vec<Automation>> {
        sqlx::query_as("SELECT * FROM automations WHERE deleted_at IS NULL ORDER BY name COLLATE NOCASE, id")
            .fetch_all(&self.pool)
            .await
    }

    /// Automations in the trash, most recently deleted first.
    pub async fn list_trash(&self) -> sqlx::Result<Vec<Automation>> {
        sqlx::query_as("SELECT * FROM automations WHERE deleted_at IS NOT NULL ORDER BY deleted_at DESC")
            .fetch_all(&self.pool)
            .await
    }

    /// Move to the trash (or back out of it with `deleted = false`).
    /// Returns `false` if there is no such automation.
    pub async fn set_deleted(&self, id: i64, deleted: bool) -> sqlx::Result<bool> {
        let deleted_at = deleted.then(now);
        let result = sqlx::query("UPDATE automations SET deleted_at = ? WHERE id = ?")
            .bind(deleted_at)
            .bind(id)
            .execute(&self.pool)
            .await?;
        Ok(result.rows_affected() > 0)
    }

    /// Earlier versions of an automation, newest first.
    pub async fn list_versions(&self, automation_id: i64) -> sqlx::Result<Vec<AutomationVersion>> {
        sqlx::query_as("SELECT * FROM automation_versions WHERE automation_id = ? ORDER BY id DESC")
            .bind(automation_id)
            .fetch_all(&self.pool)
            .await
    }

    pub async fn get_version(&self, version_id: i64) -> sqlx::Result<Option<AutomationVersion>> {
        sqlx::query_as("SELECT * FROM automation_versions WHERE id = ?")
            .bind(version_id)
            .fetch_optional(&self.pool)
            .await
    }

    pub async fn get_automation(&self, id: i64) -> sqlx::Result<Option<Automation>> {
        sqlx::query_as("SELECT * FROM automations WHERE id = ?")
            .bind(id)
            .fetch_optional(&self.pool)
            .await
    }

    pub async fn create_automation(&self, new: &NewAutomation) -> sqlx::Result<Automation> {
        let now = now();
        let id = sqlx::query(
            "INSERT INTO automations
                (name, description, lua_code, schedule, enabled, run_on_startup, watch_path, watch_pattern, created_at, updated_at)
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(&new.name)
        .bind(&new.description)
        .bind(&new.lua_code)
        .bind(&new.schedule)
        .bind(new.enabled)
        .bind(new.run_on_startup)
        .bind(&new.watch_path)
        .bind(&new.watch_pattern)
        .bind(&now)
        .bind(&now)
        .execute(&self.pool)
        .await?
        .last_insert_rowid();

        self.get_automation(id).await?.ok_or(sqlx::Error::RowNotFound)
    }

    /// Returns `None` if no automation has this id. The previous state is kept
    /// in `automation_versions` whenever the code, name, description or triggers change.
    pub async fn update_automation(
        &self,
        id: i64,
        new: &NewAutomation,
    ) -> sqlx::Result<Option<Automation>> {
        let mut tx = self.pool.begin().await?;

        let Some(old): Option<Automation> = sqlx::query_as("SELECT * FROM automations WHERE id = ?")
            .bind(id)
            .fetch_optional(&mut *tx)
            .await?
        else {
            return Ok(None);
        };
        let changed = old.name != new.name
            || old.description != new.description
            || old.lua_code != new.lua_code
            || old.schedule != new.schedule
            || old.run_on_startup != new.run_on_startup
            || old.watch_path != new.watch_path
            || old.watch_pattern != new.watch_pattern;
        if changed {
            sqlx::query(
                "INSERT INTO automation_versions
                    (automation_id, name, description, lua_code, schedule, run_on_startup, watch_path, watch_pattern, saved_at)
                 VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)",
            )
            .bind(id)
            .bind(&old.name)
            .bind(&old.description)
            .bind(&old.lua_code)
            .bind(&old.schedule)
            .bind(old.run_on_startup)
            .bind(&old.watch_path)
            .bind(&old.watch_pattern)
            .bind(now())
            .execute(&mut *tx)
            .await?;
        }

        let result = sqlx::query(
            "UPDATE automations
             SET name = ?, description = ?, lua_code = ?, schedule = ?, enabled = ?,
                 run_on_startup = ?, watch_path = ?, watch_pattern = ?, updated_at = ?
             WHERE id = ?",
        )
        .bind(&new.name)
        .bind(&new.description)
        .bind(&new.lua_code)
        .bind(&new.schedule)
        .bind(new.enabled)
        .bind(new.run_on_startup)
        .bind(&new.watch_path)
        .bind(&new.watch_pattern)
        .bind(now())
        .bind(id)
        .execute(&mut *tx)
        .await?;
        tx.commit().await?;

        if result.rows_affected() == 0 {
            return Ok(None);
        }
        self.get_automation(id).await
    }

    pub async fn set_enabled(&self, id: i64, enabled: bool) -> sqlx::Result<Option<Automation>> {
        sqlx::query("UPDATE automations SET enabled = ?, updated_at = ? WHERE id = ?")
            .bind(enabled)
            .bind(now())
            .bind(id)
            .execute(&self.pool)
            .await?;
        self.get_automation(id).await
    }

    /// Permanently deletes the automation and (via `ON DELETE CASCADE`) its runs, logs,
    /// saved values and versions. Normally only used to empty the trash.
    /// Returns `false` if it did not exist.
    pub async fn delete_automation(&self, id: i64) -> sqlx::Result<bool> {
        let result = sqlx::query("DELETE FROM automations WHERE id = ?")
            .bind(id)
            .execute(&self.pool)
            .await?;
        Ok(result.rows_affected() > 0)
    }

    // ---- runs --------------------------------------------------------------

    /// Records a new run in the `running` state and returns its id.
    pub async fn start_run(&self, automation_id: i64) -> sqlx::Result<i64> {
        let id = sqlx::query(
            "INSERT INTO automation_runs (automation_id, status, started_at) VALUES (?, 'running', ?)",
        )
        .bind(automation_id)
        .bind(now())
        .execute(&self.pool)
        .await?
        .last_insert_rowid();
        Ok(id)
    }

    pub async fn finish_run(
        &self,
        run_id: i64,
        status: &str,
        output: &str,
        error: Option<&str>,
    ) -> sqlx::Result<()> {
        sqlx::query(
            "UPDATE automation_runs SET status = ?, output = ?, error = ?, finished_at = ? WHERE id = ?",
        )
        .bind(status)
        .bind(output)
        .bind(error)
        .bind(now())
        .bind(run_id)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    /// Runs still marked `running` at startup were cut off by a shutdown.
    pub async fn fail_interrupted_runs(&self) -> sqlx::Result<u64> {
        let result = sqlx::query(
            "UPDATE automation_runs
             SET status = 'failed', error = 'Interrupted: the server stopped during this run', finished_at = ?
             WHERE status = 'running'",
        )
        .bind(now())
        .execute(&self.pool)
        .await?;
        Ok(result.rows_affected())
    }

    pub async fn get_run(&self, id: i64) -> sqlx::Result<Option<AutomationRun>> {
        sqlx::query_as("SELECT * FROM automation_runs WHERE id = ?")
            .bind(id)
            .fetch_optional(&self.pool)
            .await
    }

    /// Most recent runs first.
    pub async fn list_runs(&self, automation_id: i64, limit: i64) -> sqlx::Result<Vec<AutomationRun>> {
        sqlx::query_as(
            "SELECT * FROM automation_runs WHERE automation_id = ? ORDER BY id DESC LIMIT ?",
        )
        .bind(automation_id)
        .bind(limit)
        .fetch_all(&self.pool)
        .await
    }

    pub async fn latest_run(&self, automation_id: i64) -> sqlx::Result<Option<AutomationRun>> {
        Ok(self.list_runs(automation_id, 1).await?.into_iter().next())
    }

    // ---- logs --------------------------------------------------------------

    pub async fn add_log(&self, automation_id: i64, level: &str, message: &str) -> sqlx::Result<()> {
        sqlx::query("INSERT INTO logs (automation_id, level, message, created_at) VALUES (?, ?, ?, ?)")
            .bind(automation_id)
            .bind(level)
            .bind(message)
            .bind(now())
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    // ---- store (script memory) ---------------------------------------------

    /// Everything an automation has saved with `store.set`, as JSON text by key.
    pub async fn load_store(&self, automation_id: i64) -> sqlx::Result<std::collections::HashMap<String, String>> {
        let rows: Vec<(String, String)> = sqlx::query_as("SELECT key, value FROM automation_store WHERE automation_id = ?")
            .bind(automation_id)
            .fetch_all(&self.pool)
            .await?;
        Ok(rows.into_iter().collect())
    }

    /// Replace everything an automation has saved.
    pub async fn save_store(
        &self,
        automation_id: i64,
        values: &std::collections::HashMap<String, String>,
    ) -> sqlx::Result<()> {
        let mut tx = self.pool.begin().await?;
        sqlx::query("DELETE FROM automation_store WHERE automation_id = ?")
            .bind(automation_id)
            .execute(&mut *tx)
            .await?;
        for (key, value) in values {
            sqlx::query("INSERT INTO automation_store (automation_id, key, value) VALUES (?, ?, ?)")
                .bind(automation_id)
                .bind(key)
                .bind(value)
                .execute(&mut *tx)
                .await?;
        }
        tx.commit().await
    }

    // ---- settings ----------------------------------------------------------

    pub async fn get_setting(&self, key: &str) -> sqlx::Result<Option<String>> {
        sqlx::query_scalar("SELECT value FROM settings WHERE key = ?")
            .bind(key)
            .fetch_optional(&self.pool)
            .await
    }

    pub async fn set_setting(&self, key: &str, value: &str) -> sqlx::Result<()> {
        sqlx::query(
            "INSERT INTO settings (key, value) VALUES (?, ?)
             ON CONFLICT(key) DO UPDATE SET value = excluded.value",
        )
        .bind(key)
        .bind(value)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    /// Most recent log lines first.
    pub async fn list_logs(&self, automation_id: i64, limit: i64) -> sqlx::Result<Vec<LogEntry>> {
        sqlx::query_as("SELECT * FROM logs WHERE automation_id = ? ORDER BY id DESC LIMIT ?")
            .bind(automation_id)
            .bind(limit)
            .fetch_all(&self.pool)
            .await
    }
}
