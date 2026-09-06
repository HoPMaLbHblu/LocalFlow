//! Stopping runs, the running list and the "already running" guard.

use std::time::{Duration, Instant};

use localflow_core::{AutomationInput, CoreConfig, LocalFlow};
use serde_json::json;
use tempfile::TempDir;

async fn flow_in(dir: &TempDir, timeout: u64) -> LocalFlow {
    let config = CoreConfig {
        database_url: "sqlite::memory:".into(),
        allowed_dirs: vec![dir.path().to_path_buf()],
        script_timeout: Duration::from_secs(timeout),
    };
    LocalFlow::open(config, None).await.unwrap()
}

async fn add(flow: &LocalFlow, name: &str, code: &str, triggers: serde_json::Value) -> i64 {
    let input: AutomationInput =
        serde_json::from_value(json!({ "name": name, "lua_code": code, "enabled": true, "triggers": triggers })).unwrap();
    flow.create(&input).await.unwrap().id
}

async fn until_running(flow: &LocalFlow, count: usize) {
    for _ in 0..100 {
        if flow.running().len() == count {
            return;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    panic!("running() never reached {count}: {:?}", flow.running());
}

#[tokio::test]
async fn an_endless_loop_is_stopped_quickly() {
    let dir = TempDir::new().unwrap();
    let flow = flow_in(&dir, 30).await;
    let id = add(&flow, "Loop", "while true do end", json!({})).await;
    let task = tokio::spawn({
        let flow = flow.clone();
        async move { flow.run(id, "manual").await.unwrap() }
    });
    until_running(&flow, 1).await;
    let list = flow.running();
    assert_eq!((list[0].automation_id, list[0].name.as_str()), (id, "Loop"));
    assert!(flow.is_running(id));
    let t = Instant::now();
    assert!(flow.stop_run(list[0].run_id));
    let run = task.await.unwrap();
    assert!(t.elapsed() < Duration::from_secs(2), "{:?}", t.elapsed());
    assert_eq!(run.status, "failed");
    assert_eq!(run.error.as_deref(), Some("stopped by the user"));
    assert!(flow.running().is_empty());
    assert!(!flow.is_running(id));
}

#[tokio::test]
async fn wait_wakes_early() {
    let dir = TempDir::new().unwrap();
    let flow = flow_in(&dir, 60).await;
    let id = add(&flow, "Sleeper", "wait(30)", json!({})).await;
    let task = tokio::spawn({
        let flow = flow.clone();
        async move { flow.run(id, "manual").await.unwrap() }
    });
    until_running(&flow, 1).await;
    let t = Instant::now();
    assert_eq!(flow.stop_automation(id), 1);
    let run = task.await.unwrap();
    assert!(t.elapsed() < Duration::from_secs(2));
    assert_eq!(run.error.as_deref(), Some("stopped by the user"));
}

#[tokio::test]
async fn no_entries_leak_and_unknown_ids_are_refused() {
    let dir = TempDir::new().unwrap();
    let flow = flow_in(&dir, 1).await;
    let bad = add(&flow, "Bad", "error('boom')", json!({})).await;
    let slow = add(&flow, "Timeout", "while true do end", json!({})).await;
    let ok = add(&flow, "Ok", "log('hi')", json!({})).await;
    assert_eq!(flow.run(bad, "manual").await.unwrap().status, "failed");
    let timed_out = flow.run(slow, "manual").await.unwrap();
    assert!(timed_out.error.unwrap().contains("timed out"));
    assert_eq!(flow.run(ok, "manual").await.unwrap().status, "success");
    assert!(flow.running().is_empty());
    assert!(!flow.stop_run(9999));
    assert_eq!(flow.stop_automation(9999), 0);
    assert_eq!(flow.stop_all(), 0);
}

#[tokio::test]
async fn run_guarded_skips_a_second_start_but_allows_one_later() {
    let dir = TempDir::new().unwrap();
    let flow = flow_in(&dir, 30).await;
    let id = add(&flow, "Once", "wait(1)", json!({})).await;
    let (a, b) = tokio::join!(flow.run_guarded(id, "voice"), flow.run_guarded(id, "voice"));
    let (a, b) = (a.unwrap(), b.unwrap());
    assert_eq!(a.is_some() as u8 + b.is_some() as u8, 1, "exactly one should start");
    assert!(flow.running().is_empty());
    assert!(flow.run_guarded(id, "voice").await.unwrap().is_some());
    assert!(flow.run_guarded(424242, "voice").await.is_err());
    assert!(flow.running().is_empty());
}

#[tokio::test]
async fn a_stopped_run_does_not_start_followers() {
    let dir = TempDir::new().unwrap();
    let flow = flow_in(&dir, 60).await;
    let root = dir.path().to_string_lossy().replace(std::path::MAIN_SEPARATOR, "/");
    let first = add(&flow, "First", "wait(30)", json!({})).await;
    add(
        &flow,
        "Follower",
        &format!(r#"fs.write("{root}/f.txt", "x")"#),
        json!({ "after": { "automation_id": first, "when": "failure" } }),
    )
    .await;
    let task = tokio::spawn({
        let flow = flow.clone();
        async move { flow.run(first, "manual").await.unwrap() }
    });
    until_running(&flow, 1).await;
    flow.stop_all();
    task.await.unwrap();
    tokio::time::sleep(Duration::from_millis(600)).await;
    assert!(!dir.path().join("f.txt").exists());
    assert!(flow.running().is_empty());
}

#[tokio::test]
async fn a_nested_step_is_stopped_too() {
    let dir = TempDir::new().unwrap();
    let flow = flow_in(&dir, 30).await;
    add(&flow, "Inner", "while true do end", json!({})).await;
    let outer = add(&flow, "Outer", r#"automations.call("Inner")"#, json!({})).await;
    let task = tokio::spawn({
        let flow = flow.clone();
        async move { flow.run(outer, "manual").await.unwrap() }
    });
    until_running(&flow, 1).await;
    tokio::time::sleep(Duration::from_millis(200)).await;
    let t = Instant::now();
    assert_eq!(flow.stop_automation(outer), 1);
    let run = task.await.unwrap();
    assert!(t.elapsed() < Duration::from_secs(2));
    assert_eq!(run.status, "failed");
    assert_eq!(run.error.as_deref(), Some("stopped by the user"));
    assert!(flow.running().is_empty());
}

#[tokio::test]
async fn normal_runs_are_unchanged() {
    let dir = TempDir::new().unwrap();
    let flow = flow_in(&dir, 5).await;
    let id = add(&flow, "Ok", "log('a') wait(0.2) log('b')", json!({})).await;
    let run = flow.run(id, "manual").await.unwrap();
    assert_eq!(run.status, "success");
    assert!(run.error.is_none());
}
