//! Combining automations: steps (`automations.run` / `automations.call`) and "run after".

use std::time::Duration;

use localflow_core::{AutomationInput, CoreConfig, LocalFlow};
use serde_json::json;
use tempfile::TempDir;

async fn flow_in(dir: &TempDir) -> LocalFlow {
    let config = CoreConfig {
        database_url: "sqlite::memory:".into(),
        allowed_dirs: vec![dir.path().to_path_buf()],
        script_timeout: Duration::from_secs(10),
    };
    LocalFlow::open(config, None).await.unwrap()
}

fn lua_path(dir: &TempDir) -> String {
    dir.path().to_string_lossy().replace('\\', "/")
}

async fn add(flow: &LocalFlow, name: &str, code: &str, triggers: serde_json::Value) -> i64 {
    let input: AutomationInput = serde_json::from_value(json!({
        "name": name,
        "lua_code": code,
        "enabled": true,
        "triggers": triggers,
    }))
    .unwrap();
    flow.create(&input).await.unwrap().id
}

#[tokio::test]
async fn one_automation_runs_others_as_steps_and_passes_data() {
    let dir = TempDir::new().unwrap();
    let flow = flow_in(&dir).await;
    let root = lua_path(&dir);
    // Step 1 makes a folder and a file, and returns the file's path.
    add(
        &flow,
        "Make project",
        &format!(
            r#"automation {{ run = function(ctx)
                local folder = fs.join("{root}", ctx.input.name)
                fs.mkdir(folder)
                local file = fs.join(folder, "notes.txt")
                fs.write(file, "")
                log("made " .. ctx.input.name)
                return {{ file = file }}
            end }}"#
        ),
        json!({}),
    )
    .await;
    // Step 2 writes into it.
    add(
        &flow,
        "Write notes",
        r#"automation { run = function(ctx) fs.write(ctx.input.file, "hello from step 2") return "done" end }"#,
        json!({}),
    )
    .await;

    let main = r#"
        local made = automations.call("make project", { name = "Report" })
        local status = automations.call("Write notes", made)
        log("status: " .. status)
    "#;
    let result = flow.test_run(main.into(), "Main".into()).await;
    assert!(result.success, "{:?}", result.error);
    let text = std::fs::read_to_string(dir.path().join("Report").join("notes.txt")).unwrap();
    assert_eq!(text, "hello from step 2");
    let messages: Vec<_> = result.logs.iter().map(|l| l.message.as_str()).collect();
    assert!(messages.contains(&"[Make project] made Report"), "{messages:?}");
    assert!(messages.contains(&"status: done"), "{messages:?}");
}

#[tokio::test]
async fn failed_steps_and_missing_names_are_reported() {
    let dir = TempDir::new().unwrap();
    let flow = flow_in(&dir).await;
    add(&flow, "Broken", r#"error("VPN app not found")"#, json!({})).await;

    // `run` reports the failure and the script carries on.
    let r = flow
        .test_run(
            r#"local r = automations.run("Broken") assert(r.ok == false) log(r.error)"#.into(),
            "Main".into(),
        )
        .await;
    assert!(r.success, "{:?}", r.error);
    assert!(r.logs.iter().any(|l| l.message.contains("VPN app not found")));

    // `call` stops the script.
    let r = flow.test_run(r#"automations.call("Broken")"#.into(), "Main".into()).await;
    assert!(r.error.unwrap().contains("step \"Broken\" failed"));

    let r = flow.test_run(r#"automations.call("Nope")"#.into(), "Main".into()).await;
    assert!(r.error.unwrap().contains("no automation named \"Nope\""));
}

#[tokio::test]
async fn automations_calling_each_other_in_a_loop_are_stopped() {
    let dir = TempDir::new().unwrap();
    let flow = flow_in(&dir).await;
    let a = add(&flow, "A", r#"automations.call("B")"#, json!({})).await;
    add(&flow, "B", r#"automations.call("A")"#, json!({})).await;
    let run = flow.run(a, "manual").await.unwrap();
    assert_eq!(run.status, "failed");
    assert!(run.error.unwrap().contains("would call itself forever"));
}

#[tokio::test]
async fn steps_keep_their_own_saved_values_and_permissions() {
    let dir = TempDir::new().unwrap();
    let flow = flow_in(&dir).await;
    let counter = add(
        &flow,
        "Counter",
        r#"local n = store.get("n", 0) + 1 store.set("n", n) return n"#,
        json!({}),
    )
    .await;
    // Without "Allow system control" on the step, its powerful functions stay off.
    add(&flow, "Needs system", r#"shell.run("echo hi")"#, json!({})).await;

    let main = add(
        &flow,
        "Main",
        r#"
        automations.call("Counter")
        log("second: " .. automations.call("Counter"))
        local r = automations.run("Needs system")
        log("system: " .. tostring(r.ok))
        "#,
        json!({}),
    )
    .await;
    let run = flow.run(main, "manual").await.unwrap();
    assert_eq!(run.status, "success", "{:?}", run.error);
    let output = run.output.unwrap();
    assert!(output.contains("second: 2"), "{output}");
    assert!(output.contains("system: false"), "{output}");

    // The counter's value was saved for its next run.
    let again = flow.run(counter, "manual").await.unwrap();
    assert_eq!(again.status, "success");
    let r = flow.test_run(r#"log(automations.call("Counter"))"#.into(), "t".into()).await;
    assert_eq!(r.logs.last().unwrap().message, "4");
}

#[tokio::test]
async fn run_after_starts_the_next_automation_with_the_result() {
    let dir = TempDir::new().unwrap();
    let flow = flow_in(&dir).await;
    let root = lua_path(&dir);
    let first = add(&flow, "Connect VPN", r#"return { country = "Germany" }"#, json!({})).await;
    add(
        &flow,
        "After connect",
        &format!(
            r#"fs.write("{root}/after.txt", ctx.trigger .. " " .. ctx.previous .. " " .. ctx.input.country)"#
        ),
        json!({ "after": { "automation_id": first, "when": "success" } }),
    )
    .await;
    add(
        &flow,
        "Only on failure",
        &format!(r#"fs.write("{root}/failure.txt", "x")"#),
        json!({ "after": { "automation_id": first, "when": "failure" } }),
    )
    .await;

    flow.run(first, "manual").await.unwrap();
    let path = dir.path().join("after.txt");
    for _ in 0..50 {
        if path.exists() {
            break;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    assert_eq!(std::fs::read_to_string(&path).unwrap(), "after Connect VPN Germany");
    assert!(!dir.path().join("failure.txt").exists());
}

#[tokio::test]
async fn a_loop_of_run_after_links_stops() {
    let dir = TempDir::new().unwrap();
    let flow = flow_in(&dir).await;
    let root = lua_path(&dir);
    let code = format!(r#"fs.append("{root}/count.txt", "x")"#);
    let a = add(&flow, "Ping", &code, json!({})).await;
    let b = add(&flow, "Pong", &code, json!({ "after": { "automation_id": a, "when": "always" } })).await;
    let mut input: AutomationInput = serde_json::from_value(json!({
        "name": "Ping", "lua_code": code, "enabled": true,
        "triggers": { "after": { "automation_id": b, "when": "always" } }
    }))
    .unwrap();
    input.enabled = true;
    flow.update(a, &input).await.unwrap();

    flow.run(a, "manual").await.unwrap();
    tokio::time::sleep(Duration::from_secs(3)).await;
    let count = std::fs::read_to_string(dir.path().join("count.txt")).unwrap().len();
    assert_eq!(count, localflow_core::lua::chain::MAX_DEPTH, "the loop ran {count} times");
}
