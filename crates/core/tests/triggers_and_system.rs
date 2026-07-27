//! Startup runs, folder watching, and the app/time/wait Lua functions.

use std::time::Duration;

use localflow_core::{AutomationInput, CoreConfig, CoreError, LocalFlow};
use tempfile::TempDir;

async fn flow_in(dir: &TempDir, database_url: &str) -> LocalFlow {
    let config = CoreConfig {
        database_url: database_url.into(),
        allowed_dirs: vec![dir.path().to_path_buf()],
        script_timeout: Duration::from_secs(5),
    };
    LocalFlow::open(config, None).await.unwrap()
}

fn input(code: &str) -> AutomationInput {
    AutomationInput { name: "Test".into(), lua_code: code.into(), enabled: true, ..Default::default() }
}

/// Wait up to `seconds` for `check` to become true.
async fn eventually(seconds: u64, mut check: impl AsyncFnMut() -> bool) -> bool {
    for _ in 0..seconds * 10 {
        if check().await {
            return true;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    false
}

#[tokio::test]
async fn time_functions() {
    let dir = TempDir::new().unwrap();
    let flow = flow_in(&dir, "sqlite::memory:").await;
    let code = r#"
        local now = time.now()
        assert(now > 1700000000, "now should be a unix timestamp")
        log(time.format("%Y-%m-%d", 0 + 86400 * 365))
        local d = time.date(now)
        assert(d.year >= 2024 and d.month >= 1 and d.weekday >= 1 and d.weekday <= 7)
        assert(time.today() == time.format("%Y-%m-%d"))
        assert(time.days(2) == 172800 and time.hours(1) == 3600 and time.minutes(1) == 60)
        log("ok")
    "#;
    let result = flow.test_run(code.into(), "t".into()).await;
    assert!(result.success, "{:?}", result.error);
    assert!(result.logs[0].message.starts_with("1971-01-0"), "{}", result.logs[0].message);

    let bad = flow.test_run(r#"time.format("%Q")"#.into(), "t".into()).await;
    assert!(bad.error.unwrap().contains("invalid format"));
}

#[tokio::test]
async fn file_info_functions() {
    let dir = TempDir::new().unwrap();
    std::fs::write(dir.path().join("a.txt"), "hello").unwrap();
    let flow = flow_in(&dir, "sqlite::memory:").await;
    let code = format!(
        r#"
        local root = [[{0}]]
        log(fs.size(fs.join(root, "a.txt")))
        log(tostring(fs.is_dir(root)))
        local age = time.now() - fs.modified(fs.join(root, "a.txt"))
        log(tostring(age >= 0 and age < 60))
        "#,
        dir.path().display()
    );
    let result = flow.test_run(code, "t".into()).await;
    assert!(result.success, "{:?}", result.error);
    let messages: Vec<_> = result.logs.iter().map(|l| l.message.as_str()).collect();
    assert_eq!(messages, ["5", "true", "true"]);
}

#[tokio::test]
async fn wait_respects_the_time_limit() {
    let dir = TempDir::new().unwrap();
    let flow = flow_in(&dir, "sqlite::memory:").await;
    assert!(flow.test_run("wait(0.1)".into(), "t".into()).await.success);
    let result = flow.test_run("wait(60)".into(), "t".into()).await;
    assert!(result.error.unwrap().contains("time limit"));
}

#[tokio::test]
async fn app_functions() {
    let dir = TempDir::new().unwrap();
    let flow = flow_in(&dir, "sqlite::memory:").await;
    let code = r#"
        assert(#app.list() > 0, "some processes are always running")
        assert(app.running("definitely-not-running-3f9c1a") == false)
        assert(type(app.shortcuts()) == "table")
        app.open("definitely-not-an-app-3f9c1a")
    "#;
    let result = flow.test_run(code.into(), "t".into()).await;
    assert!(result.error.unwrap().contains("could not find an app"));
}

#[tokio::test]
async fn startup_automations_run_when_localflow_starts() {
    let dir = TempDir::new().unwrap();
    let db = format!("sqlite://{}", dir.path().join("t.db").display().to_string().replace('\\', "/"));

    let flow = flow_in(&dir, &db).await;
    let mut on_start = input("log(ctx.trigger)");
    on_start.run_on_startup = true;
    let a = flow.create(&on_start).await.unwrap();
    let mut disabled = input("log('no')");
    disabled.run_on_startup = true;
    disabled.enabled = false;
    let b = flow.create(&disabled).await.unwrap();
    drop(flow);

    // "Restart" LocalFlow on the same database.
    let flow = flow_in(&dir, &db).await;
    flow.start().await.unwrap();

    // A run is recorded when it starts; wait until it has finished and written its log.
    let ran = eventually(10, async || {
        let runs = flow.runs(a.id, 10).await.unwrap();
        runs.len() == 1 && runs[0].status == "success" && !flow.logs(a.id, 10).await.unwrap().is_empty()
    })
    .await;
    assert!(ran, "startup automation should run");
    assert_eq!(flow.logs(a.id, 10).await.unwrap()[0].message, "startup");
    assert!(flow.runs(b.id, 10).await.unwrap().is_empty(), "disabled automations don't run");
}

#[tokio::test]
async fn watched_folder_runs_automation_with_the_new_file() {
    let dir = TempDir::new().unwrap();
    let inbox = dir.path().join("inbox");
    std::fs::create_dir(&inbox).unwrap();
    let flow = flow_in(&dir, "sqlite::memory:").await;

    let mut watch = input("log(fs.basename(ctx.file))");
    watch.watch_path = Some(inbox.display().to_string());
    watch.watch_pattern = Some("*.pdf".into());
    let a = flow.create(&watch).await.unwrap();
    assert!(flow.is_watching(a.id).await);

    std::fs::write(inbox.join("notes.txt"), "ignored").unwrap();
    std::fs::write(inbox.join("report.pdf"), "pdf").unwrap();

    let ran = eventually(10, async || !flow.runs(a.id, 10).await.unwrap().is_empty()).await;
    assert!(ran, "a new PDF should trigger a run");
    tokio::time::sleep(Duration::from_secs(2)).await;
    let logs = flow.logs(a.id, 10).await.unwrap();
    assert_eq!(logs.len(), 1, "only the PDF should trigger: {logs:?}");
    assert_eq!(logs[0].message, "report.pdf");

    // Disabling stops the watch.
    flow.set_enabled(a.id, false).await.unwrap();
    assert!(!flow.is_watching(a.id).await);
}

#[tokio::test]
async fn watch_folder_must_be_allowed_and_exist() {
    let dir = TempDir::new().unwrap();
    let outside = TempDir::new().unwrap();
    let flow = flow_in(&dir, "sqlite::memory:").await;

    let mut watch = input("log(1)");
    watch.watch_path = Some(outside.path().display().to_string());
    assert!(matches!(flow.create(&watch).await, Err(CoreError::Validation(e)) if e[0].contains("access denied")));

    watch.watch_path = Some(dir.path().join("missing").display().to_string());
    assert!(matches!(flow.create(&watch).await, Err(CoreError::Validation(e)) if e[0].contains("not found")));
}
