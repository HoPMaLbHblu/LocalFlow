pub mod models;
pub mod repository;

use std::{str::FromStr, time::Duration};

use sha2::{Digest, Sha384};
use sqlx::{
    migrate::Migrator,
    sqlite::{SqliteConnectOptions, SqliteJournalMode, SqlitePoolOptions, SqliteSynchronous},
    SqlitePool,
};

static MIGRATOR: Migrator = sqlx::migrate!("./migrations");

/// Open (or create) the SQLite database and apply migrations from `migrations/`.
pub async fn connect(database_url: &str) -> Result<SqlitePool, sqlx::Error> {
    let pool = open(database_url).await?;
    migrate(&pool).await?;
    Ok(pool)
}

/// Open the database without changing its schema.
pub async fn open(database_url: &str) -> Result<SqlitePool, sqlx::Error> {
    let in_memory = database_url.contains(":memory:");
    let mut options = SqliteConnectOptions::from_str(database_url)?
        .create_if_missing(true)
        .foreign_keys(true)
        .busy_timeout(Duration::from_secs(10));
    if !in_memory {
        // WAL + FULL sync: a crash or power cut can't corrupt the database or
        // lose a change that was reported as saved.
        options = options.journal_mode(SqliteJournalMode::Wal).synchronous(SqliteSynchronous::Full);
    }

    // An in-memory database lives only as long as its connection, so keep exactly one open.
    let pool_options = if in_memory {
        SqlitePoolOptions::new()
            .max_connections(1)
            .idle_timeout(None)
            .max_lifetime(None)
    } else {
        SqlitePoolOptions::new().max_connections(5)
    };
    pool_options.connect_with(options).await
}

/// True if this version of LocalFlow will change the database's schema.
pub async fn needs_migration(pool: &SqlitePool) -> bool {
    let latest = MIGRATOR.iter().map(|m| m.version).max().unwrap_or(0);
    let applied: Option<i64> = sqlx::query_scalar("SELECT MAX(version) FROM _sqlx_migrations")
        .fetch_one(pool)
        .await
        .unwrap_or(None);
    applied.is_some_and(|v| v < latest)
}

/// True if the database has been set up at all (it isn't brand new).
pub async fn is_initialized(pool: &SqlitePool) -> bool {
    sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM _sqlx_migrations")
        .fetch_one(pool)
        .await
        .is_ok_and(|n| n > 0)
}

/// Bring the schema up to date.
pub async fn migrate(pool: &SqlitePool) -> Result<(), sqlx::Error> {
    repair_line_ending_checksums(pool).await?;
    MIGRATOR.run(pool).await?;
    Ok(())
}

/// SQLite's own consistency check. `Ok(())` if the database is healthy.
pub async fn check_integrity(pool: &SqlitePool) -> Result<(), String> {
    let result: String = sqlx::query_scalar("PRAGMA quick_check")
        .fetch_one(pool)
        .await
        .map_err(|e| e.to_string())?;
    if result == "ok" {
        Ok(())
    } else {
        Err(result)
    }
}

/// sqlx refuses to start if an applied migration's file "changed". A build made
/// from a checkout with Windows line endings (CRLF) and one with Unix line
/// endings (LF) produce different checksums for the very same SQL, which made
/// an early LocalFlow build crash on databases created by earlier builds. If the only
/// difference is line endings, record the current checksum instead.
async fn repair_line_ending_checksums(pool: &SqlitePool) -> Result<(), sqlx::Error> {
    let applied: Vec<(i64, Vec<u8>)> = match sqlx::query_as("SELECT version, checksum FROM _sqlx_migrations")
        .fetch_all(pool)
        .await
    {
        Ok(rows) => rows,
        // A new database has no migrations table yet: nothing to repair.
        Err(_) => return Ok(()),
    };

    for migration in MIGRATOR.iter() {
        let Some((_, stored)) = applied.iter().find(|(v, _)| *v == migration.version) else { continue };
        if stored.as_slice() == migration.checksum.as_ref() {
            continue;
        }
        let lf = migration.sql.replace("\r\n", "\n");
        let crlf = lf.replace('\n', "\r\n");
        let same_sql = [lf, crlf]
            .iter()
            .any(|variant| Sha384::digest(variant.as_bytes()).as_slice() == stored.as_slice());
        if same_sql {
            tracing::info!(version = migration.version, "repairing migration checksum (line endings only)");
            sqlx::query("UPDATE _sqlx_migrations SET checksum = ? WHERE version = ?")
                .bind(migration.checksum.as_ref())
                .bind(migration.version)
                .execute(pool)
                .await?;
        }
    }
    Ok(())
}
