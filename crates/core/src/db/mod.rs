pub mod models;
pub mod repository;

use std::str::FromStr;

use sha2::{Digest, Sha384};
use sqlx::{
    migrate::Migrator,
    sqlite::{SqliteConnectOptions, SqlitePoolOptions},
    SqlitePool,
};

static MIGRATOR: Migrator = sqlx::migrate!("./migrations");

/// Open (or create) the SQLite database and apply migrations from `migrations/`.
pub async fn connect(database_url: &str) -> Result<SqlitePool, sqlx::Error> {
    let options = SqliteConnectOptions::from_str(database_url)?
        .create_if_missing(true)
        .foreign_keys(true);

    // An in-memory database lives only as long as its connection, so keep exactly one open.
    let pool_options = if database_url.contains(":memory:") {
        SqlitePoolOptions::new()
            .max_connections(1)
            .idle_timeout(None)
            .max_lifetime(None)
    } else {
        SqlitePoolOptions::new().max_connections(5)
    };

    let pool = pool_options.connect_with(options).await?;
    repair_line_ending_checksums(&pool).await?;
    MIGRATOR.run(&pool).await?;
    Ok(pool)
}

/// sqlx refuses to start if an applied migration's file "changed". A build made
/// from a checkout with Windows line endings (CRLF) and one with Unix line
/// endings (LF) produce different checksums for the very same SQL, which made
/// LocalFlow 2.3.0 crash on databases created by earlier builds. If the only
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
