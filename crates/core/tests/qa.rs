//! Whole-app checks with hand-written automations (no templates): the normal
//! things people do, and the things scripts and users must NOT be able to do.

use std::{path::Path, time::Duration};

use localflow_core::{AutomationInput, CoreConfig, CoreError, LocalFlow};
use serde_json::json;
use tempfile::TempDir;

struct Env {
    flow: LocalFlow,
    dir: TempDir,
    _outside: TempDir,
    outside: String,
}

async fn env() -> Env {
    env_with_timeout(Duration::from_secs(5)).await
}

async fn env_with_timeout(timeout: Duration) -> Env {
    std::env::set_var("LOCALFLOW_TEST_RECYCLE_DIR", std::env::temp_dir().join("localflow-qa-recycle"));
    let dir = TempDir::new().unwrap();
    let outside_dir = TempDir::new().unwrap();
    std::fs::write(outside_dir.path().join("secret.txt"), "top secret").unwrap();
    let flow = LocalFlow::open(
        CoreConfig {
            database_url: "sqlite::memory:".into(),
            allowed_dirs: vec![dir.path().to_path_buf()],
            script_timeout: timeout,
        },
        None,
    )
    .await
    .unwrap();
    let outside = lua_path(outside_dir.path());
    Env { flow, dir, _outside: outside_dir, outside }
}

fn lua_path(path: &Path) -> String {
    path.to_string_lossy().replace('\\', "/")
}

impl Env {
    fn root(&self) -> String {
        lua_path(self.dir.path())
    }

    async fn test(&self, code: &str) -> localflow_core::TestRunResult {
        self.flow.test_run(code.to_string(), "QA".into()).await
    }

    async fn ok(&self, code: &str) -> Vec<String> {
        let r = self.test(code).await;
        assert!(r.success, "expected success, got {:?}\ncode: {code}", r.error);
        r.logs.into_iter().map(|l| l.message).collect()
    }

    async fn fails(&self, code: &str, contains: &str) -> String {
        let r = self.test(code).await;
        let error = r.error.unwrap_or_else(|| panic!("expected an error mentioning {contains:?}\ncode: {code}"));
        assert!(error.contains(contains), "error {error:?} should mention {contains:?}\ncode: {code}");
        error
    }
}

fn input(value: serde_json::Value) -> AutomationInput {
    serde_json::from_value(value).unwrap()
}

fn validation(err: CoreError) -> Vec<String> {
    match err {
        CoreError::Validation(list) => list,
        other => panic!("expected a validation error, got {other:?}"),
    }
}

// ---- creating and editing ---------------------------------------------------------

#[tokio::test]
async fn create_edit_run_and_history() {
    let e = env().await;
    let code = format!(r#"fs.write("{}/out.txt", "run " .. ctx.trigger) log("done")"#, e.root());
    let a = e.flow.create(&input(json!({ "name": "  My automation  ", "lua_code": code, "enabled": true }))).await.unwrap();
    assert_eq!(a.name, "My automation", "names are trimmed");

    let run = e.flow.run(a.id, "manual").await.unwrap();
    assert_eq!(run.status, "success");
    assert_eq!(std::fs::read_to_string(e.dir.path().join("out.txt")).unwrap(), "run manual");
    assert!(run.output.unwrap().contains("done"));

    let runs = e.flow.runs(a.id, 10).await.unwrap();
    assert_eq!(runs.len(), 1);
    let logs = e.flow.logs(a.id, 10).await.unwrap();
    assert!(logs.iter().any(|l| l.message == "done"));

    // Editing keeps the old version.
    let edited = e
        .flow
        .update(a.id, &input(json!({ "name": "Renamed", "lua_code": "log('v2')", "enabled": true })))
        .await
        .unwrap();
    assert_eq!(edited.name, "Renamed");
    let versions = e.flow.versions(a.id).await.unwrap();
    assert_eq!(versions.len(), 1);
    assert_eq!(versions[0].name, "My automation");
    let back = e.flow.restore_version(a.id, versions[0].id).await.unwrap();
    assert_eq!(back.name, "My automation");

    // Toggling.
    assert!(!e.flow.toggle(a.id).await.unwrap().enabled);
    assert!(e.flow.toggle(a.id).await.unwrap().enabled);
    assert_eq!(e.flow.list().await.unwrap().len(), 1);
}

#[tokio::test]
async fn invalid_input_is_refused_with_every_problem_listed() {
    let e = env().await;
    let problems = validation(
        e.flow
            .create(&input(json!({
                "name": "   ",
                "lua_code": "if then",
                "schedule": "every day at noon",
                "triggers": { "hotkey": "K", "idle_minutes": 5000, "after": { "automation_id": 1, "when": "sometimes" } }
            })))
            .await
            .unwrap_err(),
    );
    let text = problems.join("\n");
    for expected in ["Name is required", "syntax error", "Hotkey", "Idle time", "unknown choice"] {
        assert!(text.contains(expected), "missing {expected:?} in:\n{text}");
    }
    assert!(problems.len() >= 6, "{problems:?}");

    let long = "x".repeat(101);
    let problems = validation(e.flow.create(&input(json!({ "name": long, "lua_code": "log(1)" }))).await.unwrap_err());
    assert!(problems[0].contains("at most 100"));

    let problems = validation(e.flow.create(&input(json!({ "name": "Empty", "lua_code": "  \n " }))).await.unwrap_err());
    assert!(problems[0].contains("must not be empty"));

    assert!(e.flow.list().await.unwrap().is_empty(), "nothing was saved");
}

#[tokio::test]
async fn unusual_but_valid_names_and_text_work() {
    let e = env().await;
    for name in ["Привет мир", "Grüße 🎉", "'; DROP TABLE automations; --", "<script>alert(1)</script>", "a"] {
        let a = e.flow.create(&input(json!({ "name": name, "lua_code": "log('ok')" }))).await.unwrap();
        assert_eq!(e.flow.get(a.id).await.unwrap().name, name);
    }
    assert_eq!(e.flow.list().await.unwrap().len(), 5);
    let logs = e.ok(r#"log("Юникод ✓ 日本語") log(nil) log(123) print("a", 1, true)"#).await;
    assert_eq!(logs, vec!["Юникод ✓ 日本語", "nil", "123", "a\t1\ttrue"]);
}

#[tokio::test]
async fn missing_automations_are_not_found() {
    let e = env().await;
    assert!(matches!(e.flow.get(999).await, Err(CoreError::NotFound)));
    assert!(e.flow.run(999, "manual").await.is_err());
    assert!(e.flow.update(999, &input(json!({ "name": "x", "lua_code": "log(1)" }))).await.is_err());
    assert!(e.flow.delete(999).await.is_err());
    assert!(e.flow.export(999).await.is_err());
}

// ---- trash and data safety ----------------------------------------------------------

#[tokio::test]
async fn trash_restore_and_delete_forever() {
    let e = env().await;
    let a = e.flow.create(&input(json!({ "name": "Keep me", "lua_code": "log(1)", "enabled": true }))).await.unwrap();
    e.flow.run(a.id, "manual").await.unwrap();

    // Deleting forever is only possible from the trash.
    assert!(e.flow.delete_forever(a.id).await.is_err());
    e.flow.delete(a.id).await.unwrap();
    assert!(e.flow.list().await.unwrap().is_empty());
    assert_eq!(e.flow.trash().await.unwrap().len(), 1);
    assert!(e.flow.run(a.id, "manual").await.is_err(), "trashed automations don't run");

    let restored = e.flow.restore(a.id).await.unwrap();
    assert_eq!(restored.name, "Keep me");
    assert_eq!(e.flow.runs(a.id, 10).await.unwrap().len(), 1, "history survives the trash");

    e.flow.delete(a.id).await.unwrap();
    e.flow.delete_forever(a.id).await.unwrap();
    assert!(e.flow.trash().await.unwrap().is_empty());
    assert!(e.flow.restore(a.id).await.is_err());
}

// ---- running scripts ----------------------------------------------------------------

#[tokio::test]
async fn errors_are_reported_with_line_numbers() {
    let e = env().await;
    e.fails("log('a')\nlocal x = nil\nx.y = 1", "automation:3").await;
    e.fails("error('custom problem')", "custom problem").await;
    e.fails("fs.lsit('.')", "lsit").await;
    e.fails("automation { name = 'no run' }", "run = function").await;
    e.fails("local t = {} .. 'x'", "concatenate").await;

    // A failed run is saved as failed, with its output so far.
    let a = e.flow.create(&input(json!({ "name": "Fails", "lua_code": "log('before') error('boom')" }))).await.unwrap();
    let run = e.flow.run(a.id, "manual").await.unwrap();
    assert_eq!(run.status, "failed");
    assert!(run.output.unwrap().contains("before"));
    assert!(run.error.unwrap().contains("boom"));
}

#[tokio::test]
async fn runaway_scripts_are_stopped() {
    let e = env_with_timeout(Duration::from_secs(1)).await;
    let started = std::time::Instant::now();
    e.fails("while true do end", "timed out").await;
    // Endless recursion hits the memory limit (or the stack limit) and is stopped.
    let error = e.test("local function f() return f() + 1 end f()").await.error.unwrap();
    assert!(error.contains("memory") || error.contains("stack"), "{error}");
    e.fails("local t = {} for i = 1, 1e9 do t[i] = string.rep('x', 1000) end", "memory").await;
    e.fails("wait(30)", "time limit").await;
    assert!(started.elapsed() < Duration::from_secs(15), "stopping took {:?}", started.elapsed());
    // After all that, normal scripts still work.
    e.ok("log('still fine')").await;
}

#[tokio::test]
async fn the_sandbox_has_no_dangerous_lua_functions() {
    let e = env().await;
    for code in [
        "os.execute('calc')",
        "io.open('C:/Windows/win.ini')",
        "load('return 1')()",
        "loadfile('x.lua')",
        "dofile('x.lua')",
        "require('os')",
        "package.loadlib('x.dll', 'f')",
        "debug.getinfo(1)",
        "collectgarbage('count')",
        "string.dump(print)",
    ] {
        let r = e.test(code).await;
        assert!(!r.success, "{code} should not work");
    }
    // Globals can't be used to smuggle anything back in between runs.
    e.ok("secret_global = 42").await;
    e.ok("assert(secret_global == nil, 'globals leaked between runs')").await;
}

#[tokio::test]
async fn scripts_cannot_leave_the_allowed_folders() {
    let e = env().await;
    let root = e.root();
    let outside = &e.outside;
    std::fs::write(e.dir.path().join("x.txt"), "mine").unwrap();
    for code in [
        format!(r#"fs.read("{outside}/secret.txt")"#),
        format!(r#"fs.write("{outside}/new.txt", "x")"#),
        format!(r#"fs.list("{outside}", "*")"#),
        format!(r#"fs.copy("{outside}/secret.txt", "{root}/stolen.txt")"#),
        format!(r#"fs.move("{root}/x.txt", "{outside}/x.txt")"#),
        format!(r#"fs.delete("{outside}/secret.txt")"#),
        format!(r#"fs.read("{root}/../{}/secret.txt")"#, Path::new(outside).file_name().unwrap().to_string_lossy()),
        r#"fs.read("C:/Windows/win.ini")"#.to_string(),
        r#"fs.list("C:/Windows/System32", "*.dll")"#.to_string(),
        format!(r#"zip.create("{outside}", "{root}/out.zip")"#),
        format!(r#"image.convert("{outside}/secret.txt", "{root}/a.png")"#),
        format!(r#"csv.read("{outside}/secret.txt")"#),
        format!(r#"crypto.encrypt("{outside}/secret.txt", "{root}/s.locked", "password123")"#),
        format!(r#"fs.duplicates("{outside}")"#),
        format!(r#"fs.hash("{outside}/secret.txt")"#),
    ] {
        let r = e.test(&code).await;
        assert!(!r.success, "should be refused: {code}");
        let error = r.error.unwrap();
        assert!(error.contains("outside the allowed") || error.contains("access denied"), "{code}: {error}");
    }
    assert_eq!(std::fs::read_to_string(Path::new(outside).join("secret.txt")).unwrap(), "top secret");
    assert!(!e.dir.path().join("stolen.txt").exists());
}

#[tokio::test]
async fn powerful_functions_need_permission() {
    let e = env().await;
    for code in [
        "shell.run('echo hi')",
        "shell.powershell('Get-Date')",
        "process.kill('notepad')",
        "app.close('notepad')",
        "keyboard.type('x')",
        "keyboard.press('ctrl+s')",
        "mouse.move(1, 1)",
        "system.lock()",
        "system.shutdown(60)",
        "system.mute()",
        "system.wake_at('07:30')",
    ] {
        e.fails(code, "Allow system control").await;
    }
    // Looking is always fine.
    e.ok("log(#process.list()) log(#window.list()) log(system.idle_seconds())").await;
}

#[tokio::test]
async fn files_are_never_destroyed() {
    let e = env().await;
    let root = e.root();
    std::fs::write(e.dir.path().join("a.txt"), "original").unwrap();
    std::fs::write(e.dir.path().join("b.txt"), "other").unwrap();
    // Moving onto an existing file is refused.
    e.fails(&format!(r#"fs.move("{root}/a.txt", "{root}/b.txt")"#), "exists").await;
    assert_eq!(std::fs::read_to_string(e.dir.path().join("b.txt")).unwrap(), "other");
    // Overwriting and deleting send the old file to the Recycle Bin.
    e.ok(&format!(r#"fs.write("{root}/a.txt", "new") fs.delete("{root}/b.txt")"#)).await;
    assert_eq!(std::fs::read_to_string(e.dir.path().join("a.txt")).unwrap(), "new");
    assert!(!e.dir.path().join("b.txt").exists());
    let recycle = std::env::temp_dir().join("localflow-qa-recycle");
    assert!(std::fs::read_dir(&recycle).unwrap().count() >= 2);
    // Renaming can't move files to another folder.
    e.fails(&format!(r#"fs.rename("{root}/a.txt", "../a.txt")"#), "plain file name").await;
}

#[tokio::test]
async fn zip_archives_cannot_write_outside_their_folder() {
    let e = env().await;
    let root = e.root();
    // A hand-made archive with a "../evil.txt" entry (a "zip slip").
    let file = std::fs::File::create(e.dir.path().join("evil.zip")).unwrap();
    let mut zip = zip::ZipWriter::new(file);
    zip.start_file("../evil.txt", zip::write::SimpleFileOptions::default()).unwrap();
    std::io::Write::write_all(&mut zip, b"gotcha").unwrap();
    zip.finish().unwrap();
    let r = e.test(&format!(r#"zip.extract("{root}/evil.zip", "{root}/unpacked")"#)).await;
    assert!(!e.dir.path().parent().unwrap().join("evil.txt").exists(), "zip slip escaped: {r:?}");
}

#[tokio::test]
async fn store_json_and_values_round_trip() {
    let e = env().await;
    let code = r#"
        local n = store.get("n", 0) + 1
        store.set("n", n)
        store.set("t", { list = { 1, 2 }, name = "x" })
        log(n .. " " .. json.encode(store.get("t")))
    "#;
    let a = e.flow.create(&input(json!({ "name": "Counter", "lua_code": code }))).await.unwrap();
    e.flow.run(a.id, "manual").await.unwrap();
    let run = e.flow.run(a.id, "manual").await.unwrap();
    assert!(run.output.unwrap().contains("2 {"), "store kept the count");
    e.fails("json.decode('{not json')", "json.decode").await;
    e.fails("store.set('f', function() end)", "store.set").await;
    e.ok(r#"assert(json.decode("null") == nil) assert(json.decode("[1,2]")[2] == 2)"#).await;
}

#[tokio::test]
async fn automation_table_form_and_return_values() {
    let e = env().await;
    let logs = e
        .ok(r#"automation { name = "x", run = function(ctx) log(ctx.trigger .. " " .. ctx.name .. " " .. ctx.id) end }"#)
        .await;
    assert_eq!(logs, vec!["test QA 0"]);
    e.fails("automation { run = 5 }", "").await;
    e.fails("automation('not a table')", "").await;
}

// ---- schedules, watching and triggers ------------------------------------------------

#[tokio::test]
async fn schedules_start_runs() {
    let e = env().await;
    e.flow.start().await.unwrap();
    let file = format!("{}/tick.txt", e.root());
    let a = e
        .flow
        .create(&input(json!({ "name": "Every second", "lua_code": format!(r#"fs.append("{file}", "x")"#), "schedule": "* * * * * *", "enabled": true })))
        .await
        .unwrap();
    assert!(e.flow.is_scheduled(a.id).await);
    assert!(e.flow.next_run(a.id).await.is_some());
    tokio::time::sleep(Duration::from_millis(3500)).await;
    let ticks = std::fs::read_to_string(e.dir.path().join("tick.txt")).unwrap_or_default().len();
    assert!(ticks >= 2, "ran {ticks} times");

    // Disabling stops the schedule.
    e.flow.set_enabled(a.id, false).await.unwrap();
    assert!(!e.flow.is_scheduled(a.id).await);
    tokio::time::sleep(Duration::from_millis(1200)).await;
    let after = std::fs::read_to_string(e.dir.path().join("tick.txt")).unwrap().len();
    tokio::time::sleep(Duration::from_millis(2000)).await;
    assert_eq!(std::fs::read_to_string(e.dir.path().join("tick.txt")).unwrap().len(), after);

    // Deleting also stops it.
    e.flow.set_enabled(a.id, true).await.unwrap();
    e.flow.delete(a.id).await.unwrap();
    assert!(!e.flow.is_scheduled(a.id).await);
}

#[tokio::test]
async fn watched_folders_start_runs_with_the_new_file() {
    let e = env().await;
    e.flow.start().await.unwrap();
    let inbox = e.dir.path().join("inbox");
    std::fs::create_dir_all(&inbox).unwrap();
    let log_file = format!("{}/seen.txt", e.root());
    let a = e
        .flow
        .create(&input(json!({
            "name": "Watcher",
            "lua_code": format!(r#"fs.append("{log_file}", fs.basename(ctx.file) .. "\n")"#),
            "watch_path": lua_path(&inbox),
            "watch_pattern": "*.pdf",
            "enabled": true
        })))
        .await
        .unwrap();
    assert!(e.flow.is_watching(a.id).await);
    tokio::time::sleep(Duration::from_millis(300)).await;
    std::fs::write(inbox.join("doc.pdf"), "x").unwrap();
    std::fs::write(inbox.join("ignore.txt"), "x").unwrap();
    let seen = e.dir.path().join("seen.txt");
    for _ in 0..50 {
        if seen.exists() {
            break;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    tokio::time::sleep(Duration::from_millis(500)).await;
    let text = std::fs::read_to_string(&seen).unwrap_or_default();
    assert!(text.contains("doc.pdf"), "{text:?}");
    assert!(!text.contains("ignore.txt"), "pattern not applied: {text:?}");
}

#[tokio::test]
async fn watching_a_folder_outside_the_allowed_ones_is_harmless() {
    let e = env().await;
    e.flow.start().await.unwrap();
    // Saving works (the folder might be allowed later), but runs can't touch it.
    let result = e
        .flow
        .create(&input(json!({ "name": "Outside", "lua_code": "fs.read(ctx.file)", "watch_path": e.outside, "enabled": true })))
        .await;
    if let Ok(a) = result {
        std::fs::write(Path::new(&e.outside).join("new.txt"), "x").unwrap();
        tokio::time::sleep(Duration::from_millis(1500)).await;
        for run in e.flow.runs(a.id, 10).await.unwrap() {
            assert_eq!(run.status, "failed", "a run read a file outside the allowed folders");
        }
    }
}

// ---- sharing ------------------------------------------------------------------------

#[tokio::test]
async fn export_and_import() {
    let e = env().await;
    let a = e
        .flow
        .create(&input(json!({
            "name": "Share me", "description": "d", "lua_code": "shell.run('echo')",
            "schedule": "0 0 9 * * *", "enabled": true, "allow_system": true,
            "triggers": { "hotkey": "Ctrl+Alt+Q", "after": { "automation_id": 1, "when": "success" } }
        })))
        .await
        .unwrap();
    let text = e.flow.export(a.id).await.unwrap();
    assert!(!text.contains("\"after\""), "run-after links are not shared");
    let preview = e.flow.preview_import(&text).unwrap();
    let json_preview = serde_json::to_string(&preview).unwrap();
    assert!(json_preview.contains("Share me"));

    let imported = e.flow.import(&text).await.unwrap();
    assert_eq!(imported.name, "Share me");
    assert!(!imported.enabled, "imports arrive switched off");
    assert!(!imported.allow_system, "imports never get system control");

    // Broken or hostile files.
    for bad in ["", "not json", "{}", r#"{"format":"something-else"}"#, &"x".repeat(5_000_000)] {
        assert!(e.flow.preview_import(bad).is_err(), "accepted: {}", &bad[..bad.len().min(40)]);
        assert!(e.flow.import(bad).await.is_err());
    }
    let broken_code = text.replace("shell.run('echo')", "if then");
    assert!(e.flow.import(&broken_code).await.is_err(), "invalid code must not be imported");
}

// ---- settings ------------------------------------------------------------------------

#[tokio::test]
async fn allowed_folders_and_time_limit_apply_immediately() {
    let e = env().await;
    let other = TempDir::new().unwrap();
    let other_path = lua_path(other.path());
    e.fails(&format!(r#"fs.write("{other_path}/x.txt", "x")"#), "outside the allowed").await;
    e.flow.set_allowed_dirs(&[e.dir.path().to_path_buf(), other.path().to_path_buf()]);
    e.ok(&format!(r#"fs.write("{other_path}/x.txt", "x")"#)).await;
    e.flow.set_allowed_dirs(&[e.dir.path().to_path_buf()]);
    e.fails(&format!(r#"fs.read("{other_path}/x.txt")"#), "outside the allowed").await;

    e.flow.set_script_timeout(Duration::from_secs(1));
    e.fails("while true do end", "1 seconds").await;
}

// ---- combining automations ------------------------------------------------------------

#[tokio::test]
async fn steps_with_bad_input_fail_cleanly() {
    let e = env().await;
    e.flow.create(&input(json!({ "name": "Step", "lua_code": "return ctx.input" }))).await.unwrap();
    e.fails("automations.call('Step', { f = function() end })", "bad input").await;
    e.fails("automations.call()", "no automation").await;
    let logs = e.ok("log(json.encode(automations.call('Step', { a = { 1, 2 } })))").await;
    assert_eq!(logs, vec![r#"{"a":[1,2]}"#]);
}

// ---- many runs at once ---------------------------------------------------------------

#[tokio::test]
async fn many_runs_at_the_same_time_are_all_recorded() {
    let e = env().await;
    let a = e.flow.create(&input(json!({ "name": "Busy", "lua_code": "log('x')", "enabled": true }))).await.unwrap();
    let mut tasks = Vec::new();
    for _ in 0..30 {
        let flow = e.flow.clone();
        tasks.push(tokio::spawn(async move { flow.run(a.id, "manual").await }));
    }
    for t in tasks {
        assert_eq!(t.await.unwrap().unwrap().status, "success");
    }
    assert_eq!(e.flow.runs(a.id, 100).await.unwrap().len(), 30);
}

#[tokio::test]
async fn huge_output_is_handled() {
    let e = env().await;
    let r = e.test("for i = 1, 20000 do log(string.rep('x', 100)) end").await;
    assert!(r.success);
    assert_eq!(r.logs.len(), 20000);
    let a = e.flow.create(&input(json!({ "name": "Chatty", "lua_code": "for i = 1, 5000 do log(i) end" }))).await.unwrap();
    assert_eq!(e.flow.run(a.id, "manual").await.unwrap().status, "success");
}
