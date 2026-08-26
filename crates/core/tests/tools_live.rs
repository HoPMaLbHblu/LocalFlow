//! Tries the new tools on this PC. Read-only, except for files in a temp folder.
//! cargo test -p localflow-core --test tools_live -- --ignored --nocapture

use std::time::Duration;

use localflow_core::{CoreConfig, LocalFlow};
use tempfile::TempDir;

#[tokio::test]
#[ignore = "uses this PC's screen, network and winget"]
async fn tools_work_on_this_pc() {
    let dir = TempDir::new().unwrap();
    let home = dir.path().canonicalize().unwrap();
    let config = CoreConfig {
        database_url: "sqlite::memory:".into(),
        allowed_dirs: vec![home.clone()],
        script_timeout: Duration::from_secs(300),
    };
    let flow = LocalFlow::open(config, None).await.unwrap();
    let folder = home.to_string_lossy().replace(std::path::MAIN_SEPARATOR, "/").replace("//?/", "");
    let code = format!(
        r#"
        local shot = screen.capture("{folder}/shots/screen.png")
        local again = screen.capture("{folder}/shots/screen.png")
        log("shots: " .. fs.basename(shot) .. " " .. fs.basename(again) .. " " .. fs.size(shot))
        speak(" ")
        log("online: " .. tostring(network.online()))
        log("ping: " .. tostring(network.ping("example.com")))
        log("port 1 open: " .. tostring(network.port_open("127.0.0.1", 1)))
        log("has local ip: " .. tostring(network.local_ip() ~= nil))
        log("wifi known: " .. tostring(network.wifi() ~= nil))
        local file = http.download("https://example.com/", "{folder}/page.html")
        log("download: " .. fs.size(file))
        for _, p in ipairs(process.top(3, "memory")) do log("top: " .. p.name .. " " .. p.memory_mb) end
        log("env has PATH: " .. tostring(env.get("PATH") ~= nil))
        local s = service.status("Spooler")
        log("spooler: " .. (s and s.status or "none"))
        log("missing service: " .. tostring(service.status("NoSuchServiceLF")))
        log("services: " .. #service.list())
        log("updates: " .. #packages.updates())
        local ok, e = pcall(packages.install, "x")
        log("install blocked: " .. tostring(not ok))
        "#
    );
    let result = flow.test_run(code, "tools".into()).await;
    for l in &result.logs {
        println!("{}", l.message);
    }
    assert!(result.success, "{:?}", result.error);
}

/// The Telegram commands, with Telegram replaced by the log. Only commands that read.
#[tokio::test]
#[ignore = "uses this PC's screen and windows"]
async fn remote_commands_answer() {
    let dir = TempDir::new().unwrap();
    let home = dir.path().canonicalize().unwrap();
    std::env::set_var("LOCALFLOW_TEST_HOME", &home);
    let config = CoreConfig {
        database_url: "sqlite::memory:".into(),
        allowed_dirs: vec![home.clone()],
        script_timeout: Duration::from_secs(60),
    };
    let flow = LocalFlow::open(config, None).await.unwrap();
    for (command, args) in [("/help", ""), ("/status", ""), ("/top", ""), ("/apps", ""), ("/volume", ""), ("/screenshot", ""), ("/shutdown", ""), ("/nope", "")] {
        let code = format!(
            "telegram.send = function(t) log('REPLY ' .. t) end\ntelegram.send_photo = function(p) log('PHOTO ' .. fs.basename(p)) end\nCOMMAND = {}\nARGS = {}\nALLOW_POWER = false\n{}",
            localflow_core::remote::lua_string(command),
            localflow_core::remote::lua_string(args),
            localflow_core::remote::COMMANDS
        );
        let result = flow.test_run_with(code, "remote".into(), true).await;
        let out: Vec<_> = result.logs.iter().map(|l| l.message.clone()).collect();
        println!("== {command}\n{}", out.join("\n").chars().take(400).collect::<String>());
        assert!(result.success, "{command}: {:?}", result.error);
        assert!(out.iter().any(|l| l.starts_with("REPLY") || l.starts_with("PHOTO")), "{command}: no reply");
    }
}

/// Reading the volume (Core Audio, COM) and then opening a link on the same thread must still
/// open the link. Needs a counter at 127.0.0.1:8765 that records visits.
#[tokio::test]
#[ignore = "opens a browser tab to a local test page"]
async fn link_opens_after_reading_the_volume() {
    let dir = TempDir::new().unwrap();
    let config = CoreConfig { database_url: "sqlite::memory:".into(), allowed_dirs: vec![dir.path().to_path_buf()], script_timeout: Duration::from_secs(30) };
    let flow = LocalFlow::open(config, None).await.unwrap();
    let code = r#"log("volume " .. system.volume()); app.open("http://127.0.0.1:8765/after-volume")"#;
    let result = flow.test_run(code.into(), "com".into()).await;
    println!("{:?} {:?}", result.error, result.logs.iter().map(|l| &l.message).collect::<Vec<_>>());
    assert!(result.success);
}
