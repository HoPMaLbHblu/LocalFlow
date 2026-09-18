//! Copies of the database, so automations, history and settings are never lost.
//!
//! Backups live in a `backups` folder next to the database:
//!
//! - `daily`          once a day, automatically (the 30 newest are kept)
//! - `before-update`  right before a new LocalFlow version changes the database (kept)
//! - `manual`         "Back up now" in Settings (kept)
//! - `before-restore` the current state, taken before restoring an older backup (kept)
//! - `damaged`        a database that failed its integrity check, set aside (kept)
//!
//! Only `daily` backups are ever cleaned up automatically.

use std::path::{Path, PathBuf};

use chrono::{DateTime, Local};
use serde::Serialize;
use sqlx::SqlitePool;

const PREFIX: &str = "localflow-";
/// Written when the user picks a backup to restore; applied on the next start.
const RESTORE_MARKER: &str = "restore-on-next-start.txt";
/// How many automatic daily backups to keep.
const KEEP_DAILY: usize = 30;

#[derive(Debug, Clone, Serialize)]
pub struct BackupInfo {
    pub file_name: String,
    /// RFC 3339, local time.
    pub created_at: String,
    pub size: u64,
    /// "daily", "before-update", "manual", "before-restore" or "damaged".
    pub kind: String,
}

#[derive(Debug, Clone)]
pub struct Backups {
    database: PathBuf,
    dir: PathBuf,
}

impl Backups {
    /// `None` for in-memory databases, which have nothing to back up.
    pub fn for_database_url(url: &str) -> Option<Self> {
        if url.contains(":memory:") {
            return None;
        }
        let path = url.strip_prefix("sqlite://").or_else(|| url.strip_prefix("sqlite:")).unwrap_or(url);
        let path = path.split('?').next().unwrap_or(path);
        let database = PathBuf::from(path);
        let dir = database.parent().map(|p| p.join("backups")).unwrap_or_else(|| PathBuf::from("backups"));
        Some(Backups { database, dir })
    }

    pub fn dir(&self) -> &Path {
        &self.dir
    }

    fn sidecars(path: &Path) -> [PathBuf; 2] {
        let name = path.file_name().unwrap_or_default().to_string_lossy();
        [path.with_file_name(format!("{name}-wal")), path.with_file_name(format!("{name}-shm"))]
    }

    fn new_file_name(kind: &str) -> String {
        format!("{PREFIX}{}-{kind}.db", Local::now().format("%Y-%m-%d_%H-%M-%S"))
    }

    /// A consistent copy of the live database (SQLite `VACUUM INTO`).
    pub async fn create(&self, pool: &SqlitePool, kind: &str) -> Result<BackupInfo, String> {
        std::fs::create_dir_all(&self.dir).map_err(|e| format!("could not create {}: {e}", self.dir.display()))?;
        let mut target = self.dir.join(Self::new_file_name(kind));
        // Two backups within the same second: add a counter.
        let mut n = 1;
        while target.exists() {
            n += 1;
            target = self.dir.join(format!("{PREFIX}{}-{kind}-{n}.db", Local::now().format("%Y-%m-%d_%H-%M-%S")));
        }
        sqlx::query("VACUUM INTO ?")
            .bind(target.to_string_lossy().into_owned())
            .execute(pool)
            .await
            .map_err(|e| format!("backup failed: {e}"))?;
        tracing::info!(kind, file = %target.display(), "database backed up");
        info(&target).ok_or_else(|| "backup file missing after writing it".to_string())
    }

    /// All backups, newest first.
    pub fn list(&self) -> Vec<BackupInfo> {
        let Ok(entries) = std::fs::read_dir(&self.dir) else { return Vec::new() };
        let mut list: Vec<BackupInfo> = entries.flatten().filter_map(|e| info(&e.path())).collect();
        list.sort_by(|a, b| b.file_name.cmp(&a.file_name));
        list
    }

    pub fn has_daily_backup_today(&self) -> bool {
        let today = format!("{PREFIX}{}", Local::now().format("%Y-%m-%d"));
        self.list().iter().any(|b| b.kind == "daily" && b.file_name.starts_with(&today))
    }

    /// Remove old *daily* backups beyond the newest 30. Nothing else is ever removed.
    pub fn prune(&self) {
        for old in self.list().into_iter().filter(|b| b.kind == "daily").skip(KEEP_DAILY) {
            let _ = std::fs::remove_file(self.dir.join(&old.file_name));
        }
    }

    fn backup_path(&self, file_name: &str) -> Result<PathBuf, String> {
        // Only plain file names from our own folder.
        if file_name.contains(['/', '\\']) || !file_name.starts_with(PREFIX) || !file_name.ends_with(".db") {
            return Err(format!("not a LocalFlow backup: {file_name}"));
        }
        let path = self.dir.join(file_name);
        if !path.is_file() {
            return Err(format!("backup not found: {file_name}"));
        }
        Ok(path)
    }

    /// Ask for `file_name` to replace the database the next time LocalFlow starts.
    /// The current database is backed up first ("before-restore").
    pub async fn schedule_restore(&self, pool: &SqlitePool, file_name: &str) -> Result<(), String> {
        self.backup_path(file_name)?;
        self.create(pool, "before-restore").await?;
        std::fs::write(self.dir.join(RESTORE_MARKER), file_name).map_err(|e| e.to_string())
    }

    /// Run before the database is opened. Returns the restored backup's name, if any.
    pub fn apply_pending_restore(&self) -> Result<Option<String>, String> {
        let marker = self.dir.join(RESTORE_MARKER);
        let Ok(file_name) = std::fs::read_to_string(&marker) else { return Ok(None) };
        let file_name = file_name.trim().to_string();
        let source = self.backup_path(&file_name)?;
        self.replace_database_with(&source)?;
        std::fs::remove_file(&marker).map_err(|e| e.to_string())?;
        tracing::warn!(backup = %file_name, "restored database from backup");
        Ok(Some(file_name))
    }

    fn replace_database_with(&self, source: &Path) -> Result<(), String> {
        for sidecar in Self::sidecars(&self.database) {
            if sidecar.exists() {
                remove_with_retry(&sidecar)?;
            }
        }
        std::fs::copy(source, &self.database).map_err(|e| format!("could not restore backup: {e}"))?;
        Ok(())
    }

    /// The database is damaged: set it aside (never delete it) and put the newest
    /// healthy backup in its place. Returns the backup used, or an error if there is none.
    pub fn recover_from_damage(&self) -> Result<String, String> {
        let newest = self
            .list()
            .into_iter()
            .find(|b| b.kind != "damaged")
            .ok_or("the database is damaged and there is no backup to restore")?;

        std::fs::create_dir_all(&self.dir).map_err(|e| e.to_string())?;
        let aside = self.dir.join(Self::new_file_name("damaged"));
        std::fs::copy(&self.database, &aside).map_err(|e| format!("could not keep the damaged database: {e}"))?;
        for (i, sidecar) in Self::sidecars(&self.database).iter().enumerate() {
            if sidecar.exists() {
                let suffix = if i == 0 { "-wal" } else { "-shm" };
                let _ = std::fs::copy(sidecar, aside.with_file_name(format!("{}{suffix}", aside.file_name().unwrap().to_string_lossy())));
            }
        }
        self.replace_database_with(&self.dir.join(&newest.file_name))?;
        tracing::error!(damaged_copy = %aside.display(), restored = %newest.file_name, "database was damaged; restored the newest backup");
        Ok(newest.file_name)
    }
}

fn info(path: &Path) -> Option<BackupInfo> {
    let file_name = path.file_name()?.to_string_lossy().into_owned();
    if !file_name.starts_with(PREFIX) || !file_name.ends_with(".db") {
        return None;
    }
    let meta = std::fs::metadata(path).ok()?;
    let created: DateTime<Local> = meta.modified().ok()?.into();
    // localflow-YYYY-MM-DD_HH-MM-SS-<kind>[-n].db
    let stem = file_name.trim_end_matches(".db");
    let kind = stem
        .get(PREFIX.len() + 20..)
        .unwrap_or("")
        .trim_end_matches(|c: char| c.is_ascii_digit() || c == '-')
        .to_string();
    Some(BackupInfo {
        file_name,
        created_at: created.to_rfc3339(),
        size: meta.len(),
        kind: if kind.is_empty() { "manual".into() } else { kind },
    })
}

/// Remove a file, retrying for a few seconds: right after LocalFlow closes the
/// database, Windows (or an antivirus scan) can still hold it for a moment.
fn remove_with_retry(path: &Path) -> Result<(), String> {
    let mut last = None;
    for _ in 0..40 {
        match std::fs::remove_file(path) {
            Ok(()) => return Ok(()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(()),
            Err(e) => last = Some(e),
        }
        std::thread::sleep(std::time::Duration::from_millis(100));
    }
    Err(format!("could not remove {}: {}", path.display(), last.map(|e| e.to_string()).unwrap_or_default()))
}
