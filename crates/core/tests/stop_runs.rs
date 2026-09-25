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

// ---- reservations ------------------------------------------------------------------------------------

#[tokio::test]
async fn a_run_that_is_being_started_is_listed_and_stoppable() {
    let dir = TempDir::new().unwrap();
    let flow = flow_in(&dir, 60).await;
    let id = add(&flow, "Slow", "wait(30)", json!({})).await;
    let task = tokio::spawn({
        let flow = flow.clone();
        async move { flow.run_guarded(id, "voice").await.unwrap() }
    });
    // From the moment it is reserved until it is registered, running() never omits it.
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        let reserved_or_running = flow.is_running(id);
        let list = flow.running();
        if reserved_or_running {
            assert!(list.iter().any(|r| r.automation_id == id), "is_running() but not in running(): {list:?}");
            if list.iter().any(|r| r.run_id > 0) {
                break;
            }
        }
        assert!(Instant::now() < deadline, "the run never started");
        tokio::time::sleep(Duration::from_millis(1)).await;
    }
    assert_eq!(flow.running().len(), 1);
    assert_eq!(flow.stop_automation(id), 1);
    let run = task.await.unwrap().unwrap();
    assert_eq!(run.error.as_deref(), Some("stopped by the user"));
    assert!(flow.running().is_empty() && !flow.is_running(id));
}

#[tokio::test]
async fn stopping_by_the_id_that_running_shows_works_for_reservations_too() {
    let dir = TempDir::new().unwrap();
    let flow = flow_in(&dir, 60).await;
    let id = add(&flow, "Slow", "wait(30)", json!({})).await;
    let task = tokio::spawn({
        let flow = flow.clone();
        async move { flow.run_guarded(id, "voice").await.unwrap() }
    });
    // Whatever id the entry has (temporary or real), stop_run accepts what running() shows.
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        if let Some(r) = flow.running().first() {
            if flow.stop_run(r.run_id) {
                break;
            }
        }
        assert!(Instant::now() < deadline, "could not stop it");
        tokio::time::sleep(Duration::from_millis(1)).await;
    }
    let run = task.await.unwrap().unwrap();
    assert_eq!(run.error.as_deref(), Some("stopped by the user"));
    assert!(flow.running().is_empty());
}

// ---- voice origin: system steps and followers need a confirmation ------------------------------------------

async fn add_sys(flow: &LocalFlow, name: &str, code: &str, triggers: serde_json::Value) -> i64 {
    let input: AutomationInput =
        serde_json::from_value(json!({ "name": name, "lua_code": code, "enabled": true, "allow_system": true, "triggers": triggers })).unwrap();
    flow.create(&input).await.unwrap().id
}

fn root_of(dir: &TempDir) -> String {
    dir.path().to_string_lossy().replace(std::path::MAIN_SEPARATOR, "/")
}

async fn settle() {
    tokio::time::sleep(Duration::from_millis(900)).await;
}

#[tokio::test]
async fn an_unconfirmed_voice_run_cannot_start_system_steps() {
    let dir = TempDir::new().unwrap();
    let flow = flow_in(&dir, 20).await;
    let root = root_of(&dir);
    add_sys(&flow, "Sys step", &format!(r#"fs.write("{root}/sys.txt", "ran")"#), json!({})).await;
    let outer = add(
        &flow,
        "Outer",
        &format!(r#"local r = automations.run("Sys step") fs.write("{root}/result.txt", tostring(r.ok) .. "|" .. tostring(r.error))"#),
        json!({}),
    )
    .await;
    let caller = add(&flow, "Caller", r#"automations.call("Sys step")"#, json!({})).await;

    // unconfirmed: blocked with a clear message; `run` reports it, `call` fails the run
    let run = flow.run_voice(outer, false).await.unwrap().unwrap();
    assert_eq!(run.status, "success");
    assert!(!dir.path().join("sys.txt").exists());
    let result = std::fs::read_to_string(dir.path().join("result.txt")).unwrap();
    assert!(result.starts_with("false|") && result.contains("this step controls the PC; run it yourself or confirm it by voice"), "{result}");
    let run = flow.run_voice(caller, false).await.unwrap().unwrap();
    assert_eq!(run.status, "failed");
    assert!(run.error.unwrap().contains("controls the PC"));
    assert!(!dir.path().join("sys.txt").exists());

    // confirmed: as before
    std::fs::remove_file(dir.path().join("result.txt")).unwrap();
    assert_eq!(flow.run_voice(outer, true).await.unwrap().unwrap().status, "success");
    assert!(dir.path().join("sys.txt").exists());
    assert_eq!(std::fs::read_to_string(dir.path().join("result.txt")).unwrap(), "true|nil");
    std::fs::remove_file(dir.path().join("sys.txt")).unwrap();

    // every other way of starting is unaffected
    assert_eq!(flow.run(outer, "manual").await.unwrap().status, "success");
    assert!(dir.path().join("sys.txt").exists());
    std::fs::remove_file(dir.path().join("sys.txt")).unwrap();
    assert!(flow.run_guarded(outer, "hotkey").await.unwrap().is_some());
    assert!(dir.path().join("sys.txt").exists());
}

#[tokio::test]
async fn an_unconfirmed_voice_run_does_not_start_system_followers() {
    let dir = TempDir::new().unwrap();
    let flow = flow_in(&dir, 20).await;
    let root = root_of(&dir);
    let first = add(&flow, "First", "log(\"first\")", json!({})).await;
    // a follower that controls the PC
    add_sys(
        &flow,
        "Sys follower",
        &format!(r#"fs.write("{root}/sysf.txt", "x")"#),
        json!({ "after": { "automation_id": first, "when": "success" } }),
    )
    .await;
    // a harmless follower with a system step, and a system follower of its own
    let plain = add(
        &flow,
        "Plain follower",
        &format!(r#"fs.write("{root}/plain.txt", "x") local r = automations.run("Sys step") fs.write("{root}/plain_step.txt", tostring(r.ok))"#),
        json!({ "after": { "automation_id": first, "when": "success" } }),
    )
    .await;
    add_sys(&flow, "Sys step", &format!(r#"fs.write("{root}/sys.txt", "x")"#), json!({})).await;
    add_sys(
        &flow,
        "Deep sys follower",
        &format!(r#"fs.write("{root}/deep.txt", "x")"#),
        json!({ "after": { "automation_id": plain, "when": "success" } }),
    )
    .await;

    // unconfirmed: the harmless follower runs; everything that controls the PC is blocked down the chain
    flow.run_voice(first, false).await.unwrap().unwrap();
    settle().await;
    assert!(dir.path().join("plain.txt").exists(), "harmless followers still run");
    assert_eq!(std::fs::read_to_string(dir.path().join("plain_step.txt")).unwrap(), "false");
    for blocked in ["sysf.txt", "sys.txt", "deep.txt"] {
        assert!(!dir.path().join(blocked).exists(), "{blocked} must not have run");
    }
    let logs = flow.logs(first, 20).await.unwrap();
    assert!(logs.iter().any(|l| l.message.contains("Sys follower") && l.message.contains("not started")), "{logs:?}");

    // confirmed: the chain behaves as it always did
    for f in ["plain.txt", "plain_step.txt"] {
        std::fs::remove_file(dir.path().join(f)).unwrap();
    }
    flow.run_voice(first, true).await.unwrap().unwrap();
    settle().await;
    for ran in ["sysf.txt", "plain.txt", "plain_step.txt", "sys.txt", "deep.txt"] {
        assert!(dir.path().join(ran).exists(), "{ran} should have run");
    }
    assert_eq!(std::fs::read_to_string(dir.path().join("plain_step.txt")).unwrap(), "true");

    // started any other way: unchanged
    for f in ["sysf.txt", "plain.txt", "plain_step.txt", "sys.txt", "deep.txt"] {
        std::fs::remove_file(dir.path().join(f)).unwrap();
    }
    flow.run(first, "manual").await.unwrap();
    settle().await;
    assert!(dir.path().join("sysf.txt").exists() && dir.path().join("deep.txt").exists());
}

#[tokio::test]
async fn a_voice_run_reports_the_voice_trigger() {
    let dir = TempDir::new().unwrap();
    let config = CoreConfig {
        database_url: "sqlite::memory:".into(),
        allowed_dirs: vec![dir.path().to_path_buf()],
        script_timeout: Duration::from_secs(5),
    };
    let seen: std::sync::Arc<std::sync::Mutex<Vec<String>>> = Default::default();
    let sink = seen.clone();
    let handler: localflow_core::EventHandler = std::sync::Arc::new(move |e| {
        if let localflow_core::CoreEvent::RunStarted { trigger, .. } = e {
            sink.lock().unwrap().push(trigger);
        }
    });
    let flow = LocalFlow::open(config, Some(handler)).await.unwrap();
    let id = add(&flow, "Ok", "log(\"x\")", json!({})).await;
    flow.run_voice(id, false).await.unwrap().unwrap();
    flow.run_voice(id, true).await.unwrap().unwrap();
    assert_eq!(*seen.lock().unwrap(), vec!["voice".to_string(), "voice".to_string()]);
}
