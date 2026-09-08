//! Files, zip, security scan, system info, json/http/store, and sharing.

use std::time::Duration;

use localflow_core::{sharing::Risk, AutomationInput, CoreConfig, LocalFlow, TestRunResult};
use tempfile::TempDir;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

async fn flow_in(dir: &TempDir) -> LocalFlow {
    let config = CoreConfig {
        database_url: "sqlite::memory:".into(),
        allowed_dirs: vec![dir.path().to_path_buf()],
        script_timeout: Duration::from_secs(10),
    };
    LocalFlow::open(config, None).await.unwrap()
}

fn messages(r: &TestRunResult) -> Vec<String> {
    r.logs.iter().map(|l| l.message.clone()).collect()
}

async fn run(flow: &LocalFlow, code: String) -> TestRunResult {
    let result = flow.test_run(code, "t".into()).await;
    assert!(result.success, "script failed: {:?}", result.error);
    result
}

#[tokio::test]
async fn read_write_append_rename_and_list_dirs() {
    let dir = TempDir::new().unwrap();
    let flow = flow_in(&dir).await;
    let code = format!(
        r#"
        local root = [[{}]]
        local file = fs.join(root, "notes", "a.txt")
        fs.write(file, "hello")
        fs.append(file, " world")
        log(fs.read(file))
        local renamed = fs.rename(file, "b.txt")
        log(fs.basename(renamed))
        fs.mkdir(fs.join(root, "other"))
        for _, d in ipairs(fs.list_dirs(root)) do log(fs.basename(d)) end
        "#,
        dir.path().display()
    );
    let r = run(&flow, code).await;
    assert_eq!(messages(&r), ["hello world", "b.txt", "notes", "other"]);

    let bad = flow.test_run(r#"fs.rename("x", "../y")"#.into(), "t".into()).await;
    assert!(bad.error.unwrap().contains("plain file name"));
}

#[tokio::test]
async fn find_largest_and_hash() {
    let dir = TempDir::new().unwrap();
    let root = dir.path();
    std::fs::create_dir_all(root.join("a/b")).unwrap();
    std::fs::write(root.join("small.pdf"), vec![0u8; 10]).unwrap();
    std::fs::write(root.join("a/medium.pdf"), vec![0u8; 500]).unwrap();
    std::fs::write(root.join("a/b/big.bin"), vec![0u8; 5000]).unwrap();
    std::fs::write(root.join("hello.txt"), "hello").unwrap();
    let flow = flow_in(&dir).await;

    let code = format!(
        r#"
        local root = [[{}]]
        local pdfs, complete = fs.find(root, "*.pdf")
        log(#pdfs .. " " .. tostring(complete))
        local top = fs.largest(root, 2)
        log(fs.basename(top[1].path) .. "=" .. top[1].size)
        log(fs.basename(top[2].path) .. "=" .. top[2].size)
        log(fs.hash(fs.join(root, "hello.txt")))
        "#,
        root.display()
    );
    let r = run(&flow, code).await;
    assert_eq!(
        messages(&r),
        [
            "2 true",
            "big.bin=5000",
            "medium.pdf=500",
            // sha256("hello")
            "2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824",
        ]
    );
}

#[tokio::test]
async fn zip_round_trip_and_zip_slip_protection() {
    let dir = TempDir::new().unwrap();
    let root = dir.path();
    std::fs::create_dir_all(root.join("docs/sub")).unwrap();
    std::fs::write(root.join("docs/one.txt"), "1").unwrap();
    std::fs::write(root.join("docs/sub/two.txt"), "2").unwrap();
    let flow = flow_in(&dir).await;

    let code = format!(
        r#"
        local root = [[{0}]]
        log(zip.create(fs.join(root, "backup.zip"), fs.join(root, "docs")))
        log(zip.extract(fs.join(root, "backup.zip"), fs.join(root, "restored")))
        "#,
        root.display()
    );
    let r = run(&flow, code).await;
    assert_eq!(messages(&r), ["2", "2"]);
    assert_eq!(std::fs::read_to_string(root.join("restored/docs/sub/two.txt")).unwrap(), "2");

    // An archive entry pointing outside the target folder must be refused.
    {
        let file = std::fs::File::create(root.join("evil.zip")).unwrap();
        let mut writer = zip::ZipWriter::new(file);
        writer.start_file("../escaped.txt", zip::write::SimpleFileOptions::default()).unwrap();
        std::io::Write::write_all(&mut writer, b"x").unwrap();
        writer.finish().unwrap();
    }
    let code = format!(r#"zip.extract([[{0}/evil.zip]], [[{0}/out]])"#, root.display());
    let bad = flow.test_run(code, "t".into()).await;
    assert!(bad.error.unwrap().contains("unsafe path"));
    assert!(!root.join("escaped.txt").exists());
}

#[tokio::test]
async fn security_scan_flags_suspicious_names() {
    let dir = TempDir::new().unwrap();
    let root = dir.path();
    std::fs::create_dir_all(root.join("deep")).unwrap();
    for name in ["report.pdf", "invoice.pdf.exe", "deep/free_trojan.exe", "deep/svchost.exe", "photo.jpg"] {
        std::fs::write(root.join(name), "x").unwrap();
    }
    let flow = flow_in(&dir).await;
    let code = format!(
        r#"
        local hits = security.scan([[{}]])
        table.sort(hits, function(a, b) return a.path < b.path end)
        for _, hit in ipairs(hits) do log(fs.basename(hit.path) .. ": " .. hit.reason) end
        "#,
        root.display()
    );
    let r = run(&flow, code).await;
    let found = messages(&r);
    assert_eq!(found.len(), 3, "{found:?}");
    assert!(found.iter().any(|m| m.starts_with("free_trojan.exe: suspicious name (trojan)")));
    assert!(found.iter().any(|m| m.starts_with("invoice.pdf.exe: double extension")));
    assert!(found.iter().any(|m| m.starts_with("svchost.exe: Windows system program")));
}

#[tokio::test]
async fn system_information() {
    let dir = TempDir::new().unwrap();
    let flow = flow_in(&dir).await;
    let code = r#"
        assert(#system.computer_name() > 0)
        assert(system.uptime() > 0)
        local mem = system.memory()
        assert(mem.total > 0 and mem.free <= mem.total)
        local cpu = system.cpu()
        assert(cpu >= 0 and cpu <= 100)
        local disks = system.disks()
        assert(#disks > 0 and disks[1].total > 0)
        assert(system.disk_free("~") > 0)
        local battery = system.battery()   -- nil on desktops
        assert(battery == nil or (battery.percent >= 0 and battery.percent <= 100))
        log(system.os())
    "#;
    run(&flow, code.into()).await;
}

#[tokio::test]
async fn json_encode_and_decode() {
    let dir = TempDir::new().unwrap();
    let flow = flow_in(&dir).await;
    let code = r#"
        local data = json.decode('{"name":"LocalFlow","tags":["a","b"],"count":3,"none":null}')
        log(data.name .. " " .. #data.tags .. " " .. data.count .. " " .. tostring(data.none))
        log(json.encode({ ok = true }))
    "#;
    let r = run(&flow, code.into()).await;
    assert_eq!(messages(&r), ["LocalFlow 2 3 nil", r#"{"ok":true}"#]);

    let bad = flow.test_run(r#"json.decode("{nope")"#.into(), "t".into()).await;
    assert!(bad.error.unwrap().contains("not valid JSON"));
}

/// A one-request HTTP server that answers with the request body, upper-cased.
async fn echo_server() -> String {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    tokio::spawn(async move {
        for _ in 0..2 {
            let (mut socket, _) = listener.accept().await.unwrap();
            // Read until the headers and the whole body (Content-Length) have arrived.
            let mut data = Vec::new();
            let mut chunk = vec![0u8; 8192];
            let request = loop {
                let read = socket.read(&mut chunk).await.unwrap();
                data.extend_from_slice(&chunk[..read]);
                let text = String::from_utf8_lossy(&data).to_string();
                if let Some(end) = text.find("\r\n\r\n") {
                    let length = text[..end]
                        .lines()
                        .find_map(|l| l.to_lowercase().strip_prefix("content-length:").map(|v| v.trim().parse::<usize>().unwrap()))
                        .unwrap_or(0);
                    if data.len() >= end + 4 + length || read == 0 {
                        break text;
                    }
                }
            };
            let body = request.split("\r\n\r\n").nth(1).unwrap_or("").to_uppercase();
            let status = if request.starts_with("GET /missing") { "404 Not Found" } else { "200 OK" };
            let response = format!("HTTP/1.1 {status}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len());
            socket.write_all(response.as_bytes()).await.unwrap();
        }
    });
    format!("http://{address}")
}

#[tokio::test]
async fn http_requests() {
    let dir = TempDir::new().unwrap();
    let flow = flow_in(&dir).await;
    let url = echo_server().await;
    let code = format!(
        r#"
        local r = http.post("{url}/echo", {{ json = {{ msg = "hi" }} }})
        log(r.status .. " " .. tostring(r.ok) .. " " .. r.body)
        local missing = http.get("{url}/missing")
        log(missing.status .. " " .. tostring(missing.ok))
        "#
    );
    let r = run(&flow, code).await;
    assert_eq!(messages(&r), [r#"200 true {"MSG":"HI"}"#, "404 false"]);

    let bad = flow.test_run(r#"http.get("file:///etc/passwd")"#.into(), "t".into()).await;
    assert!(bad.error.unwrap().contains("http://"));
}

#[tokio::test]
async fn store_remembers_between_runs() {
    let dir = TempDir::new().unwrap();
    let flow = flow_in(&dir).await;
    let code = r#"
        local count = store.get("count", 0) + 1
        store.set("count", count)
        store.set("last", { when = "now", list = { 1, 2 } })
        log("run " .. count)
    "#;
    let a = flow
        .create(&AutomationInput { name: "Counter".into(), lua_code: code.into(), enabled: true, ..Default::default() })
        .await
        .unwrap();
    flow.run(a.id, "manual").await.unwrap();
    flow.run(a.id, "manual").await.unwrap();
    let logs: Vec<String> = flow.logs(a.id, 10).await.unwrap().into_iter().map(|l| l.message).collect();
    assert_eq!(logs, ["run 2", "run 1"]);

    // Test runs don't see or change the saved values.
    let r = run(&flow, "log(tostring(store.get('count')))".into()).await;
    assert_eq!(messages(&r), ["nil"]);
}

#[tokio::test]
async fn export_and_import_arrive_disabled_with_risks() {
    let dir = TempDir::new().unwrap();
    let flow = flow_in(&dir).await;
    let original = flow
        .create(&AutomationInput {
            name: "Cleaner".into(),
            description: "Deletes old stuff".into(),
            lua_code: "fs.delete('~/x')\napp.open('notepad')".into(),
            schedule: Some("0 0 * * * *".into()),
            enabled: true,
            // A folder that won't exist on the other computer.
            watch_path: Some(dir.path().display().to_string()),
            ..Default::default()
        })
        .await
        .unwrap();

    let file = flow.export(original.id).await.unwrap();
    assert!(file.contains("\"format\": \"localflow\""));

    let preview = flow.preview_import(&file).unwrap();
    assert_eq!(preview.automation.name, "Cleaner");
    assert!(preview.problems.is_empty());
    assert_eq!(
        preview.risks,
        [Risk::DeletesFiles, Risk::OpensApps, Risk::WatchesFolder, Risk::RunsOnSchedule]
    );

    // Import on "another computer" where the watched folder doesn't exist.
    let other_dir = TempDir::new().unwrap();
    let other = flow_in(&other_dir).await;
    let imported = other.import(&file.replace(&dir.path().display().to_string().replace('\\', "\\\\"), "C:/nowhere")).await.unwrap();
    assert!(!imported.enabled, "imports must start disabled");
    assert_eq!(imported.schedule.as_deref(), Some("0 0 * * * *"));
    assert!(!other.is_scheduled(imported.id).await);

    assert!(flow.preview_import("not a localflow file").is_err());
}

#[tokio::test]
async fn script_time_limit_can_change() {
    let dir = TempDir::new().unwrap();
    let flow = flow_in(&dir).await;
    flow.set_script_timeout(Duration::from_millis(300));
    let r = flow.test_run("while true do end".into(), "t".into()).await;
    assert!(r.error.unwrap().contains("timed out"));
    assert_eq!(flow.script_timeout(), Duration::from_millis(300));
}
