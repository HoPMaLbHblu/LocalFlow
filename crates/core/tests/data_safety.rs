//! Nothing the user made is ever lost: trash, versions, backups, recycle bin.

use std::time::Duration;

use localflow_core::{AutomationInput, CoreConfig, CoreError, LocalFlow};
use tempfile::TempDir;

fn file_url(dir: &TempDir) -> String {
    format!("sqlite://{}", dir.path().join("localflow.db").display().to_string().replace('\\', "/"))
}

async fn flow_at(url: &str, allowed: &TempDir) -> LocalFlow {
    let config = CoreConfig {
        database_url: url.into(),
        allowed_dirs: vec![allowed.path().to_path_buf()],
        script_timeout: Duration::from_secs(5),
    };
    LocalFlow::open(config, None).await.unwrap()
}

fn input(name: &str, code: &str) -> AutomationInput {
    AutomationInput { name: name.into(), lua_code: code.into(), enabled: true, ..Default::default() }
}

#[tokio::test]
async fn deleted_automations_go_to_the_trash_and_come_back() {
    let dir = TempDir::new().unwrap();
    let flow = flow_at("sqlite::memory:", &dir).await;
    let a = flow.create(&input("Keep me", "log('hi')")).await.unwrap();
    flow.run(a.id, "manual").await.unwrap();

    flow.delete(a.id).await.unwrap();
    assert!(flow.list().await.unwrap().is_empty());
    assert!(matches!(flow.get(a.id).await, Err(CoreError::NotFound)));
    assert_eq!(flow.trash().await.unwrap().len(), 1);

    let restored = flow.restore(a.id).await.unwrap();
    assert_eq!(restored.name, "Keep me");
    assert_eq!(flow.runs(a.id, 10).await.unwrap().len(), 1, "history survives the trash");
    assert!(flow.trash().await.unwrap().is_empty());
}

#[tokio::test]
async fn only_trashed_automations_can_be_deleted_forever() {
    let dir = TempDir::new().unwrap();
    let flow = flow_at("sqlite::memory:", &dir).await;
    let a = flow.create(&input("A", "log(1)")).await.unwrap();
    assert!(flow.delete_forever(a.id).await.is_err());
    flow.delete(a.id).await.unwrap();
    flow.delete_forever(a.id).await.unwrap();
    assert!(flow.trash().await.unwrap().is_empty());
}

#[tokio::test]
async fn every_save_keeps_the_previous_version() {
    let dir = TempDir::new().unwrap();
    let flow = flow_at("sqlite::memory:", &dir).await;
    let a = flow.create(&input("V", "log('one')")).await.unwrap();
    flow.update(a.id, &input("V", "log('two')")).await.unwrap();
    // Toggling on/off is not a new version.
    flow.set_enabled(a.id, false).await.unwrap();
    flow.update(a.id, &AutomationInput { enabled: false, ..input("V", "log('three')") }).await.unwrap();

    let versions = flow.versions(a.id).await.unwrap();
    let codes: Vec<&str> = versions.iter().map(|v| v.lua_code.as_str()).collect();
    assert_eq!(codes, ["log('two')", "log('one')"]);

    let oldest = versions.last().unwrap().id;
    let restored = flow.restore_version(a.id, oldest).await.unwrap();
    assert_eq!(restored.lua_code, "log('one')");
    assert!(!restored.enabled, "restoring a version keeps the on/off switch");
    // The state before restoring became a version too.
    assert_eq!(flow.versions(a.id).await.unwrap()[0].lua_code, "log('three')");
}

#[tokio::test]
async fn backups_are_made_and_can_be_restored() {
    let data = TempDir::new().unwrap();
    let url = file_url(&data);
    let flow = flow_at(&url, &data).await;
    flow.start().await.unwrap();
    flow.create(&input("Before backup", "log(1)")).await.unwrap();

    // A daily backup appears on start.
    tokio::time::sleep(Duration::from_millis(500)).await;
    assert!(flow.list_backups().iter().any(|b| b.kind == "daily"));

    let manual = flow.backup_now().await.unwrap();
    assert_eq!(manual.kind, "manual");
    flow.create(&input("After backup", "log(2)")).await.unwrap();

    flow.schedule_restore(&manual.file_name).await.unwrap();
    assert!(flow.list_backups().iter().any(|b| b.kind == "before-restore"));
    flow.close().await;

    // "Restart": the chosen backup replaces the database.
    let flow = flow_at(&url, &data).await;
    let names: Vec<String> = flow.list().await.unwrap().into_iter().map(|a| a.automation.name).collect();
    assert_eq!(names, ["Before backup"]);
    assert!(flow.startup_notice().is_some());

    assert!(flow.schedule_restore("../../evil.db").await.is_err());
}

#[tokio::test]
async fn damaged_database_is_replaced_by_the_newest_backup() {
    let data = TempDir::new().unwrap();
    let url = file_url(&data);
    let flow = flow_at(&url, &data).await;
    flow.create(&input("Precious", "log(1)")).await.unwrap();
    flow.backup_now().await.unwrap();
    flow.close().await;
    tokio::time::sleep(Duration::from_millis(300)).await;

    // Scribble over the middle of the database file.
    let db = data.path().join("localflow.db");
    let _ = std::fs::remove_file(data.path().join("localflow.db-wal"));
    let _ = std::fs::remove_file(data.path().join("localflow.db-shm"));
    let mut bytes = std::fs::read(&db).unwrap();
    for b in bytes.iter_mut().skip(4096).take(4096) {
        *b = 0xAB;
    }
    std::fs::write(&db, bytes).unwrap();

    let flow = flow_at(&url, &data).await;
    let names: Vec<String> = flow.list().await.unwrap().into_iter().map(|a| a.automation.name).collect();
    assert_eq!(names, ["Precious"]);
    assert!(flow.list_backups().iter().any(|b| b.kind == "damaged"), "the damaged file is kept, not deleted");
}

#[tokio::test]
async fn scripts_send_files_to_the_recycle_bin() {
    let dir = TempDir::new().unwrap();
    let bin = TempDir::new().unwrap();
    std::env::set_var("LOCALFLOW_TEST_RECYCLE_DIR", bin.path());
    let flow = flow_at("sqlite::memory:", &dir).await;
    let root = dir.path();
    std::fs::create_dir_all(root.join("folder/sub")).unwrap();
    std::fs::write(root.join("folder/sub/a.txt"), "a").unwrap();
    std::fs::write(root.join("note.txt"), "first").unwrap();

    let code = format!(
        r#"
        local root = [[{}]]
        fs.delete(fs.join(root, "folder"))          -- a whole folder, not only empty ones
        fs.write(fs.join(root, "note.txt"), "second") -- the old text is kept
        "#,
        root.display()
    );
    let result = flow.test_run(code, "t".into()).await;
    assert!(result.success, "{:?}", result.error);

    assert!(!root.join("folder").exists());
    assert_eq!(std::fs::read_to_string(root.join("note.txt")).unwrap(), "second");
    assert!(bin.path().join("folder/sub/a.txt").exists());
    assert_eq!(std::fs::read_to_string(bin.path().join("note.txt")).unwrap(), "first");
}
