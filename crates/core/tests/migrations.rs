//! Databases created by a build with other line endings must still open.
//! (LocalFlow 2.3.0 crashed at startup because of this.)

use localflow_core::db;
use sha2::{Digest, Sha384};
use tempfile::TempDir;

fn url(dir: &TempDir) -> String {
    format!("sqlite://{}", dir.path().join("t.db").display().to_string().replace('\\', "/"))
}

#[tokio::test]
async fn database_from_a_build_with_other_line_endings_still_opens() {
    let dir = TempDir::new().unwrap();
    let pool = db::connect(&url(&dir)).await.unwrap();

    // Pretend every migration was applied by a build whose .sql files had the
    // opposite line endings: same SQL, different checksum.
    let rows: Vec<(i64,)> = sqlx::query_as("SELECT version FROM _sqlx_migrations").fetch_all(&pool).await.unwrap();
    assert!(!rows.is_empty());
    for (version,) in rows {
        let path = std::fs::read_dir("migrations")
            .unwrap()
            .map(|e| e.unwrap().path())
            .find(|p| p.file_name().unwrap().to_string_lossy().starts_with(&format!("{version:04}_")))
            .unwrap();
        let sql = std::fs::read_to_string(path).unwrap();
        let other = if sql.contains("\r\n") { sql.replace("\r\n", "\n") } else { sql.replace('\n', "\r\n") };
        sqlx::query("UPDATE _sqlx_migrations SET checksum = ? WHERE version = ?")
            .bind(Sha384::digest(other.as_bytes()).to_vec())
            .bind(version)
            .execute(&pool)
            .await
            .unwrap();
    }
    pool.close().await;

    // Opening again must repair the checksums instead of failing.
    let pool = db::connect(&url(&dir)).await.expect("database should open after repair");
    pool.close().await;
}

#[tokio::test]
async fn a_really_changed_migration_is_still_refused() {
    let dir = TempDir::new().unwrap();
    let pool = db::connect(&url(&dir)).await.unwrap();
    sqlx::query("UPDATE _sqlx_migrations SET checksum = ? WHERE version = 1")
        .bind(Sha384::digest(b"something else entirely").to_vec())
        .execute(&pool)
        .await
        .unwrap();
    pool.close().await;

    let error = db::connect(&url(&dir)).await.unwrap_err().to_string();
    assert!(error.contains("modified"), "{error}");
}

/// Opens a copy of a real database given in LOCALFLOW_DB_COPY (manual check).
#[tokio::test]
async fn real_database_copy_opens() {
    let Ok(path) = std::env::var("LOCALFLOW_DB_COPY") else { return };
    let pool = db::connect(&format!("sqlite://{}", path.replace('\\', "/"))).await.expect("real database copy should open");
    let count: (i64,) = sqlx::query_as("SELECT COUNT(*) FROM automations").fetch_one(&pool).await.unwrap();
    println!("real database copy opened, automations: {}", count.0);
}
