//! Shell, processes, windows, the "Allow system control" permission, and event triggers.
//! Only harmless actions are exercised: nothing here locks the PC, presses keys or shuts down.

use std::time::Duration;

use localflow_core::{
    sharing::Risk, triggers::ExtraTriggers, AutomationInput, CoreConfig, CoreError, LocalFlow, TestRunResult,
};
use tempfile::TempDir;

async fn flow(dir: &TempDir) -> LocalFlow {
    let config = CoreConfig {
        database_url: "sqlite::memory:".into(),
        allowed_dirs: vec![dir.path().to_path_buf()],
        // Generous: the first PowerShell start on a fresh CI machine can take 20+ seconds.
        script_timeout: Duration::from_secs(90),
    };
    LocalFlow::open(config, None).await.unwrap()
}

fn messages(r: &TestRunResult) -> Vec<String> {
    r.logs.iter().map(|l| l.message.clone()).collect()
}

#[tokio::test]
async fn powerful_functions_need_permission() {
    let dir = TempDir::new().unwrap();
    let flow = flow(&dir).await;
    for code in [
        "shell.run('echo hi')",
        "process.kill('notepad')",
        "keyboard.press('ctrl+c')",
        "mouse.click(1, 1)",
        "system.lock()",
        "system.shutdown()",
        "window.close('nothing')",
    ] {
        let r = flow.test_run_with(code.into(), "t".into(), false).await;
        let error = r.error.unwrap_or_default();
        assert!(error.contains("Allow system control"), "{code}: {error}");
    }
    // Reading information is always allowed.
    let r = flow
        .test_run_with("log(#process.list() > 0) log(type(window.list())) log(system.idle_seconds() >= 0)".into(), "t".into(), false)
        .await;
    assert!(r.success, "{:?}", r.error);
    assert_eq!(messages(&r), ["true", "table", "true"]);
}

#[tokio::test]
async fn shell_commands_run_with_utf8_output() {
    let dir = TempDir::new().unwrap();
    let flow = flow(&dir).await;
    let code = r#"
        local r = shell.run("echo hello")
        log(r.output .. " " .. r.code .. " " .. tostring(r.ok))
        local p = shell.powershell("Write-Output 'Привет'")
        log(p.output)
        local bad = shell.run("exit 3")
        log(bad.code .. " " .. tostring(bad.ok))
    "#;
    let r = flow.test_run_with(code.into(), "t".into(), true).await;
    assert!(r.success, "{:?}", r.error);
    assert_eq!(messages(&r), ["hello 0 true", "Привет", "3 false"]);
}

#[tokio::test]
async fn slow_commands_are_stopped_at_their_timeout() {
    let dir = TempDir::new().unwrap();
    let flow = flow(&dir).await;
    let r = flow
        .test_run_with("shell.run('waitfor /t 30 LocalFlowTestSignal', { timeout = 1 })".into(), "t".into(), true)
        .await;
    assert!(r.error.unwrap().contains("stopped after"));
}

#[tokio::test]
async fn windows_processes_are_protected() {
    let dir = TempDir::new().unwrap();
    let flow = flow(&dir).await;
    let r = flow.test_run_with("process.kill('svchost')".into(), "t".into(), true).await;
    assert!(r.error.unwrap().contains("protected"));
    let r = flow
        .test_run_with("log(process.kill('definitely-not-running-3f9c'))".into(), "t".into(), true)
        .await;
    assert_eq!(messages(&r), ["0"]);
}

#[tokio::test]
async fn read_only_information() {
    let dir = TempDir::new().unwrap();
    let flow = flow(&dir).await;
    let code = r#"
        assert(process.running("definitely-not-running-3f9c") == false)
        assert(window.find("no window has this title 3f9c") == nil)
        assert(process.wait_for("definitely-not-running-3f9c", 0.5) == false)
        network.wake_on_lan("AA:BB:CC:DD:EE:FF", "127.0.0.1")
        log("ok")
    "#;
    let r = flow.test_run(code.into(), "t".into()).await;
    assert!(r.success, "{:?}", r.error);
}

#[tokio::test]
async fn app_start_trigger_runs_with_the_app_name() {
    let dir = TempDir::new().unwrap();
    let flow = flow(&dir).await;
    let a = flow
        .create(&AutomationInput {
            name: "When ping starts".into(),
            lua_code: "log(ctx.trigger .. ':' .. ctx.app)".into(),
            enabled: true,
            triggers: ExtraTriggers { app_start: Some("PING.EXE".into()), ..Default::default() },
            ..Default::default()
        })
        .await
        .unwrap();
    flow.start().await.unwrap();

    // Let the monitor take its first snapshot of running programs (slow in debug
    // builds), then start a harmless program.
    tokio::time::sleep(Duration::from_secs(8)).await;
    let mut child = std::process::Command::new("ping").args(["-n", "15", "127.0.0.1"]).spawn().unwrap();

    let mut logs = Vec::new();
    for _ in 0..60 {
        logs = flow.logs(a.id, 10).await.unwrap();
        if !logs.is_empty() {
            break;
        }
        tokio::time::sleep(Duration::from_millis(250)).await;
    }
    let _ = child.kill();
    assert_eq!(logs.first().map(|l| l.message.as_str()), Some("app_start:ping"));
}

#[tokio::test]
async fn bad_hotkeys_are_rejected() {
    let dir = TempDir::new().unwrap();
    let flow = flow(&dir).await;
    let input = AutomationInput {
        name: "Hotkey".into(),
        lua_code: "log(1)".into(),
        triggers: ExtraTriggers { hotkey: Some("K".into()), ..Default::default() },
        ..Default::default()
    };
    assert!(matches!(flow.create(&input).await, Err(CoreError::Validation(e)) if e[0].contains("Hotkey")));

    let good = AutomationInput {
        triggers: ExtraTriggers { hotkey: Some("ctrl + alt + k".into()), ..Default::default() },
        ..input
    };
    let a = flow.create(&good).await.unwrap();
    assert!(a.triggers.unwrap().contains("Ctrl+Alt+K"));
}

#[tokio::test]
async fn imports_never_get_system_control() {
    let dir = TempDir::new().unwrap();
    let flow = flow(&dir).await;
    let original = flow
        .create(&AutomationInput {
            name: "Powerful".into(),
            lua_code: "shell.run('echo hi')\nkeyboard.type('x')\nsystem.lock()".into(),
            allow_system: true,
            triggers: ExtraTriggers { idle_minutes: Some(10), ..Default::default() },
            ..Default::default()
        })
        .await
        .unwrap();
    let file = flow.export(original.id).await.unwrap();

    let preview = flow.preview_import(&file).unwrap();
    for risk in [Risk::RunsCommands, Risk::ControlsInput, Risk::ControlsPower, Risk::NeedsSystemControl, Risk::RunsOnEvents] {
        assert!(preview.risks.contains(&risk), "missing {risk:?} in {:?}", preview.risks);
    }

    let imported = flow.import(&file).await.unwrap();
    assert!(!imported.allow_system, "system control is never granted by a file");
    assert!(!imported.enabled);
    assert!(imported.triggers.unwrap().contains("idle_minutes"));
}
