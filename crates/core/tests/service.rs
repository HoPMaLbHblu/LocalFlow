use std::{
    sync::{Arc, Mutex},
    time::Duration,
};

use localflow_core::{AutomationInput, CoreConfig, CoreError, CoreEvent, LocalFlow};
use tempfile::TempDir;

struct Fixture {
    flow: LocalFlow,
    events: Arc<Mutex<Vec<CoreEvent>>>,
    dir: TempDir,
}

async fn fixture() -> Fixture {
    let dir = TempDir::new().unwrap();
    let config = CoreConfig {
        database_url: "sqlite::memory:".into(),
        allowed_dirs: vec![dir.path().to_path_buf()],
        script_timeout: Duration::from_secs(5),
    };
    let events = Arc::new(Mutex::new(Vec::new()));
    let sink = events.clone();
    let flow = LocalFlow::open(config, Some(Arc::new(move |e| sink.lock().unwrap().push(e))))
        .await
        .unwrap();
    Fixture { flow, events, dir }
}

fn input(code: &str, schedule: Option<&str>) -> AutomationInput {
    AutomationInput {
        name: "Test".into(),
        description: String::new(),
        lua_code: code.into(),
        schedule: schedule.map(String::from),
        enabled: true,
        ..Default::default()
    }
}

#[tokio::test]
async fn create_validates_every_field() {
    let f = fixture().await;
    let bad = AutomationInput {
        name: " ".into(),
        lua_code: "if then".into(),
        schedule: Some("nope".into()),
        ..Default::default()
    };
    match f.flow.create(&bad).await {
        Err(CoreError::Validation(errors)) => assert_eq!(errors.len(), 3, "{errors:?}"),
        other => panic!("expected validation error, got {other:?}"),
    }
}

#[tokio::test]
async fn schedule_follows_enabled_state() {
    let f = fixture().await;
    let a = f.flow.create(&input("log(1)", Some("0 0 * * * *"))).await.unwrap();
    assert!(f.flow.is_scheduled(a.id).await);

    f.flow.toggle(a.id).await.unwrap();
    assert!(!f.flow.is_scheduled(a.id).await);

    f.flow.set_enabled(a.id, true).await.unwrap();
    assert!(f.flow.is_scheduled(a.id).await);

    // Clearing the schedule unschedules it.
    f.flow.update(a.id, &input("log(1)", None)).await.unwrap();
    assert!(!f.flow.is_scheduled(a.id).await);
}

#[tokio::test]
async fn run_records_history_and_emits_live_events() {
    let f = fixture().await;
    let a = f.flow.create(&input("log('one')\nnotify('two')", None)).await.unwrap();

    let run = f.flow.run(a.id, "manual").await.unwrap();
    assert_eq!(run.status, "success");
    assert_eq!(f.flow.logs(a.id, 10).await.unwrap().len(), 2);

    let events = f.events.lock().unwrap();
    let kinds: Vec<&str> = events
        .iter()
        .map(|e| match e {
            CoreEvent::RunStarted { .. } => "started",
            CoreEvent::Log { .. } => "log",
            CoreEvent::RunFinished { .. } => "finished",
            CoreEvent::AutomationsChanged => "changed",
            CoreEvent::Notice { .. } => "notice",
        })
        .collect();
    assert_eq!(kinds, ["changed", "started", "log", "log", "finished"]);
    assert!(matches!(&events[3], CoreEvent::Log { level, run_id: Some(_), .. } if level == "notify"));
}

#[tokio::test]
async fn test_run_saves_nothing() {
    let f = fixture().await;
    let result = f.flow.test_run("log('hi')".into(), "Draft".into()).await;
    assert!(result.success);
    assert_eq!(result.logs[0].message, "hi");
    assert!(f.flow.list().await.unwrap().is_empty());

    let result = f.flow.test_run("error('bad')".into(), "Draft".into()).await;
    assert!(!result.success);
    assert!(result.error.unwrap().contains("bad"));
}

#[tokio::test]
async fn list_includes_last_and_next_run() {
    let f = fixture().await;
    let a = f.flow.create(&input("log(1)", Some("0 0 * * * *"))).await.unwrap();
    f.flow.start().await.unwrap();
    f.flow.run(a.id, "manual").await.unwrap();

    let list = f.flow.list().await.unwrap();
    assert_eq!(list[0].last_run.as_ref().unwrap().status, "success");
    assert!(list[0].next_run.is_some());
}

#[tokio::test]
async fn allowed_dirs_can_change_at_runtime() {
    let f = fixture().await;
    let other = TempDir::new().unwrap();
    let code = format!("log(tostring(fs.exists([[{}]])))", other.path().display());

    assert!(!f.flow.test_run(code.clone(), "x".into()).await.success);
    f.flow.set_allowed_dirs(&[f.dir.path().to_path_buf(), other.path().to_path_buf()]);
    assert!(f.flow.test_run(code, "x".into()).await.success);
}

#[tokio::test]
async fn settings_round_trip() {
    let f = fixture().await;
    let repo = f.flow.repo();
    assert_eq!(repo.get_setting("k").await.unwrap(), None);
    repo.set_setting("k", "1").await.unwrap();
    repo.set_setting("k", "2").await.unwrap();
    assert_eq!(repo.get_setting("k").await.unwrap().as_deref(), Some("2"));
}
