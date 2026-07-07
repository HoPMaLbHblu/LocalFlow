use chrono::{SecondsFormat, Utc};
use sqlx::SqlitePool;

use super::models::{Automation, AutomationRun, LogEntry, NewAutomation};

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

    // ---- automations -------------------------------------------------------

    pub async fn list_automations(&self) -> sqlx::Result<Vec<Automation>> {
        sqlx::query_as("SELECT * FROM automations ORDER BY name COLLATE NOCASE, id")
            .fetch_all(&self.pool)
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
            "INSERT INTO automations (name, description, lua_code, schedule, enabled, created_at, updated_at)
             VALUES (?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(&new.name)
        .bind(&new.description)
        .bind(&new.lua_code)
        .bind(&new.schedule)
        .bind(new.enabled)
        .bind(&now)
        .bind(&now)
        .execute(&self.pool)
        .await?
        .last_insert_rowid();

        self.get_automation(id).await?.ok_or(sqlx::Error::RowNotFound)
    }

    /// Returns `None` if no automation has this id.
    pub async fn update_automation(
        &self,
        id: i64,
        new: &NewAutomation,
    ) -> sqlx::Result<Option<Automation>> {
        let result = sqlx::query(
            "UPDATE automations
             SET name = ?, description = ?, lua_code = ?, schedule = ?, enabled = ?, updated_at = ?
             WHERE id = ?",
        )
        .bind(&new.name)
        .bind(&new.description)
        .bind(&new.lua_code)
        .bind(&new.schedule)
        .bind(new.enabled)
        .bind(now())
        .bind(id)
        .execute(&self.pool)
        .await?;

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

    /// Deletes the automation and (via `ON DELETE CASCADE`) its runs and logs.
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

    /// Most recent log lines first.
    pub async fn list_logs(&self, automation_id: i64, limit: i64) -> sqlx::Result<Vec<LogEntry>> {
        sqlx::query_as("SELECT * FROM logs WHERE automation_id = ? ORDER BY id DESC LIMIT ?")
            .bind(automation_id)
            .bind(limit)
            .fetch_all(&self.pool)
            .await
    }
}
