//! More tools: screenshots, speech, internet checks, downloads, busy programs,
//! Windows services and app updates (winget).
//!
//! ```lua
//! screen.capture(path)            -- saves a screenshot (.png or .jpg), returns the path
//! speak(text)                     -- reads the text aloud
//! network.online()                -- is the internet reachable?
//! network.ping(host, port)        -- milliseconds to connect, or nil
//! network.port_open(host, port)   -- true / false
//! network.local_ip()              -- this PC's address on the home network
//! network.wifi()                  -- Wi-Fi name, or nil
//! http.download(url, path)        -- saves a file, returns the path
//! process.top(count, "cpu" | "memory")  -- { { pid, name, cpu, memory_mb } }
//! env.get(name)                   -- an environment variable
//! service.list() / service.status(name)   🔒 service.start / stop / restart(name)
//! packages.updates()              -- apps with updates (winget)   🔒 packages.install(id) / packages.upgrade(id | "all")
//! ```
//!
//! Nothing is overwritten: if a screenshot or download's file exists, " (2)" is added.

use std::{
    io::Read,
    net::{TcpStream, ToSocketAddrs, UdpSocket},
    path::{Path, PathBuf},
    process::Command,
    sync::Arc,
    time::{Duration, Instant},
};

use mlua::{Lua, Table};

use super::{
    control::{powershell_command, run_program},
    sandbox::PathPolicy,
};

fn err(function: &str, error: impl std::fmt::Display) -> mlua::Error {
    mlua::Error::runtime(format!("{function}: {error}"))
}

fn require(allowed: bool, function: &str) -> mlua::Result<()> {
    if allowed {
        Ok(())
    } else {
        Err(mlua::Error::runtime(format!(
            "{function} needs \"Allow system control\". Switch it on for this automation in the editor, \
             and only for scripts you trust."
        )))
    }
}

fn budget(deadline: Instant, wanted: Duration) -> Result<Duration, String> {
    if super::sandbox::cancelled() {
        return Err(super::sandbox::STOPPED.into());
    }
    let left = deadline.saturating_duration_since(Instant::now()).min(wanted);
    if left.is_zero() {
        Err("no time left before the script's time limit".into())
    } else {
        Ok(left)
    }
}

/// `path`, or "name (2).ext", "name (3).ext", ... if it exists, so nothing is overwritten.
pub fn unused_path(path: &Path) -> PathBuf {
    if !path.exists() {
        return path.to_path_buf();
    }
    let stem = path.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
    let ext = path.extension().map(|e| format!(".{}", e.to_string_lossy())).unwrap_or_default();
    (2..)
        .map(|n| path.with_file_name(format!("{stem} ({n}){ext}")))
        .find(|p| !p.exists())
        .expect("some number is free")
}

fn prepare_output(policy: &PathPolicy, raw: &str) -> Result<PathBuf, String> {
    let path = policy.resolve(raw)?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| format!("could not make the folder: {e}"))?;
    }
    Ok(unused_path(&path))
}

fn run(mut command: Command, timeout: Duration, env: &[(&str, &str)]) -> Result<String, String> {
    for (k, v) in env {
        command.env(k, v);
    }
    let (code, out, error) = run_program(command, timeout)?;
    if code != 0 {
        let message = if error.trim().is_empty() { out.trim() } else { error.trim() };
        return Err(message.lines().take(3).collect::<Vec<_>>().join(" "));
    }
    Ok(out)
}

// ---- screenshots and speech -------------------------------------------------------------

pub(crate) fn capture(path: &Path, timeout: Duration) -> Result<(), String> {
    let text = path.to_string_lossy();
    if cfg!(target_os = "macos") {
        let mut command = Command::new("screencapture");
        command.args(["-x", &text]);
        return run(command, timeout, &[]).map(|_| ());
    }
    let jpeg = matches!(path.extension().and_then(|e| e.to_str()).map(str::to_lowercase).as_deref(), Some("jpg" | "jpeg"));
    let script = format!(
        "Add-Type -MemberDefinition '[DllImport(\"user32.dll\")] public static extern bool SetProcessDPIAware();' -Name Dpi -Namespace LocalFlow
[LocalFlow.Dpi]::SetProcessDPIAware() | Out-Null
Add-Type -AssemblyName System.Windows.Forms, System.Drawing
$b = [System.Windows.Forms.SystemInformation]::VirtualScreen
$bmp = New-Object System.Drawing.Bitmap $b.Width, $b.Height
$g = [System.Drawing.Graphics]::FromImage($bmp)
$g.CopyFromScreen($b.Left, $b.Top, 0, 0, $bmp.Size)
$bmp.Save($env:LF_PATH, [System.Drawing.Imaging.ImageFormat]::{})
$g.Dispose(); $bmp.Dispose()",
        if jpeg { "Jpeg" } else { "Png" }
    );
    run(powershell_command(&script), timeout, &[("LF_PATH", &text)]).map(|_| ())
}

fn speak(text: &str, timeout: Duration) -> Result<(), String> {
    if cfg!(target_os = "macos") {
        let mut command = Command::new("say");
        command.arg(text);
        return run(command, timeout, &[]).map(|_| ());
    }
    let script = "Add-Type -AssemblyName System.Speech
$s = New-Object System.Speech.Synthesis.SpeechSynthesizer
$s.Speak($env:LF_TEXT)";
    run(powershell_command(script), timeout, &[("LF_TEXT", text)]).map(|_| ())
}

// ---- network ----------------------------------------------------------------------------

/// Milliseconds to open a connection, or None.
pub fn connect_ms(host: &str, port: u16, timeout: Duration) -> Option<f64> {
    let addresses = (host, port).to_socket_addrs().ok()?;
    for address in addresses {
        let started = Instant::now();
        if TcpStream::connect_timeout(&address, timeout).is_ok() {
            return Some((started.elapsed().as_secs_f64() * 10000.0).round() / 10.0);
        }
    }
    None
}

fn local_ip() -> Option<String> {
    // Connecting a UDP socket sends nothing; it only picks the network card.
    let socket = UdpSocket::bind("0.0.0.0:0").ok()?;
    socket.connect("192.0.2.1:80").ok()?;
    Some(socket.local_addr().ok()?.ip().to_string())
}

/// The "SSID : name" line of `netsh wlan show interfaces` (the same in every language).
pub fn parse_wifi_name(text: &str) -> Option<String> {
    text.lines().find_map(|line| {
        let (key, value) = line.split_once(':')?;
        let value = value.trim();
        (key.trim() == "SSID" && !value.is_empty()).then(|| value.to_string())
    })
}

fn wifi(timeout: Duration) -> Option<String> {
    if cfg!(target_os = "macos") {
        let mut command = Command::new("networksetup");
        command.args(["-getairportnetwork", "en0"]);
        let out = run(command, timeout, &[]).ok()?;
        return out.split_once(": ").map(|(_, name)| name.trim().to_string()).filter(|n| !n.is_empty());
    }
    let mut command = Command::new("netsh");
    command.args(["wlan", "show", "interfaces"]);
    parse_wifi_name(&run(command, timeout, &[]).ok()?)
}

fn download(url: &str, path: &Path, deadline: Instant) -> Result<u64, String> {
    let lower = url.to_lowercase();
    if !lower.starts_with("http://") && !lower.starts_with("https://") {
        return Err("the address must start with http:// or https://".into());
    }
    let limit = budget(deadline, Duration::from_secs(600))?;
    let agent = ureq::AgentBuilder::new()
        .timeout(limit)
        .user_agent(concat!("LocalFlow/", env!("CARGO_PKG_VERSION")))
        .build();
    let response = agent.get(url).call().map_err(|e| e.to_string())?;
    // Download next to the target first, so a broken download never leaves half a file.
    let partial = path.with_extension(format!(
        "{}localflow-part",
        path.extension().map(|e| format!("{}.", e.to_string_lossy())).unwrap_or_default()
    ));
    let mut file = std::fs::File::create(&partial).map_err(|e| e.to_string())?;
    let copied = std::io::copy(&mut response.into_reader().take(4 * 1024 * 1024 * 1024), &mut file);
    drop(file);
    match copied {
        Ok(bytes) => {
            std::fs::rename(&partial, path).map_err(|e| e.to_string())?;
            Ok(bytes)
        }
        Err(e) => {
            let _ = std::fs::remove_file(&partial);
            Err(format!("download stopped: {e}"))
        }
    }
}

// ---- winget -----------------------------------------------------------------------------

/// Rows of a winget table (Name, Id, Version, Available, ...). Column headers are in the
/// PC's language, so columns are found by position from the header above the dashes.
pub fn parse_winget_table(text: &str) -> Vec<Vec<String>> {
    // winget draws a spinner with carriage returns; keep what's after the last one.
    let lines: Vec<Vec<char>> = text
        .lines()
        .map(|l| l.rsplit('\r').next().unwrap_or("").trim_end().chars().collect())
        .collect();
    let Some(dashes) = lines.iter().position(|l| l.len() > 10 && l.iter().all(|c| *c == '-')) else {
        return Vec::new();
    };
    let Some(header) = dashes.checked_sub(1).map(|i| &lines[i]) else { return Vec::new() };
    let starts: Vec<usize> = (0..header.len())
        .filter(|&i| header[i] != ' ' && (i == 0 || header[i - 1] == ' '))
        .collect();
    let mut rows = Vec::new();
    for line in &lines[dashes + 1..] {
        if line.is_empty() || starts.len() < 2 || line.len() <= starts[starts.len() - 2] {
            if line.is_empty() {
                break;
            }
            continue;
        }
        let cell = |n: usize| -> String {
            let from = starts[n].min(line.len());
            let to = starts.get(n + 1).copied().unwrap_or(line.len()).min(line.len());
            line[from..to].iter().collect::<String>().trim().to_string()
        };
        rows.push((0..starts.len()).map(cell).collect());
    }
    rows
}

fn winget(args: &[&str], timeout: Duration) -> Result<String, String> {
    if !cfg!(windows) {
        return Err("app updates use winget, which is only on Windows".into());
    }
    let mut command = Command::new("winget");
    command.args(args).args(["--accept-source-agreements", "--disable-interactivity"]);
    let (code, out, error) = run_program(command, timeout)?;
    if code != 0 && !out.contains("---") {
        let text = if error.trim().is_empty() { out } else { error };
        let last = text.lines().map(|l| l.rsplit('\r').next().unwrap_or("").trim()).filter(|l| !l.is_empty()).last();
        return Err(last.unwrap_or("winget failed").to_string());
    }
    Ok(out)
}

// ---- services ---------------------------------------------------------------------------

fn services_json(filter: Option<&str>, timeout: Duration) -> Result<serde_json::Value, String> {
    if !cfg!(windows) {
        return Err("services are only on Windows".into());
    }
    let pick = if filter.is_some() { "Get-Service -Name $env:LF_NAME -ErrorAction Stop" } else { "Get-Service" };
    let script = format!(
        "@({pick} | Select-Object Name, DisplayName, @{{n='Status';e={{$_.Status.ToString().ToLower()}}}}, @{{n='StartType';e={{$_.StartType.ToString().ToLower()}}}}) | ConvertTo-Json -Compress"
    );
    let out = run(powershell_command(&script), timeout, &[("LF_NAME", filter.unwrap_or(""))])?;
    let value: serde_json::Value = serde_json::from_str(out.trim()).map_err(|e| e.to_string())?;
    // PowerShell writes a lone service as an object, not a list of one.
    Ok(if value.is_array() { value } else { serde_json::Value::Array(vec![value]) })
}

fn service_table(lua: &Lua, value: &serde_json::Value) -> mlua::Result<Table> {
    let t = lua.create_table()?;
    let text = |k: &str| value.get(k).and_then(|v| v.as_str()).unwrap_or("").to_string();
    t.set("name", text("Name"))?;
    t.set("title", text("DisplayName"))?;
    t.set("status", text("Status"))?;
    t.set("start", text("StartType"))?;
    t.set("running", text("Status") == "running")?;
    Ok(t)
}

// ---- register -----------------------------------------------------------------------------

pub fn register(lua: &Lua, allowed: bool, policy: Arc<PathPolicy>, deadline: Instant) -> mlua::Result<()> {
    let globals = lua.globals();

    let screen: Table = globals.get("screen")?;
    let p = policy.clone();
    screen.set(
        "capture",
        lua.create_function(move |_, path: String| {
            let path = prepare_output(&p, &path).map_err(|e| err("screen.capture", e))?;
            let limit = budget(deadline, Duration::from_secs(30)).map_err(|e| err("screen.capture", e))?;
            capture(&path, limit).map_err(|e| err("screen.capture", e))?;
            Ok(path.to_string_lossy().into_owned())
        })?,
    )?;

    globals.set(
        "speak",
        lua.create_function(move |_, text: String| {
            let limit = budget(deadline, Duration::from_secs(120)).map_err(|e| err("speak", e))?;
            speak(&text, limit).map_err(|e| err("speak", e))
        })?,
    )?;

    let network: Table = globals.get("network")?;
    network.set(
        "online",
        lua.create_function(|_, ()| {
            let wait = Duration::from_secs(3);
            Ok(connect_ms("1.1.1.1", 443, wait).is_some() || connect_ms("8.8.8.8", 53, wait).is_some())
        })?,
    )?;
    network.set(
        "ping",
        lua.create_function(|_, (host, port): (String, Option<u16>)| Ok(connect_ms(&host, port.unwrap_or(443), Duration::from_secs(5))))?,
    )?;
    network.set(
        "port_open",
        lua.create_function(|_, (host, port): (String, u16)| Ok(connect_ms(&host, port, Duration::from_secs(3)).is_some()))?,
    )?;
    network.set("local_ip", lua.create_function(|_, ()| Ok(local_ip()))?)?;
    network.set(
        "wifi",
        lua.create_function(move |_, ()| Ok(budget(deadline, Duration::from_secs(10)).ok().and_then(wifi)))?,
    )?;

    let http: Table = globals.get("http")?;
    let p = policy.clone();
    http.set(
        "download",
        lua.create_function(move |_, (url, path): (String, String)| {
            let path = prepare_output(&p, &path).map_err(|e| err("http.download", e))?;
            download(&url, &path, deadline).map_err(|e| err("http.download", e))?;
            Ok(path.to_string_lossy().into_owned())
        })?,
    )?;

    let process: Table = globals.get("process")?;
    process.set(
        "top",
        lua.create_function(|lua, (count, by): (Option<usize>, Option<String>)| {
            let mut system = sysinfo::System::new();
            system.refresh_processes(sysinfo::ProcessesToUpdate::All, true);
            std::thread::sleep(sysinfo::MINIMUM_CPU_UPDATE_INTERVAL.max(Duration::from_millis(300)));
            system.refresh_processes(sysinfo::ProcessesToUpdate::All, true);
            let cores = std::thread::available_parallelism().map(|n| n.get()).unwrap_or(1) as f32;
            // Group by program name, so a browser's many processes count as one.
            let mut groups: std::collections::BTreeMap<String, (u32, f32, u64)> = Default::default();
            for (pid, p) in system.processes() {
                let name = p.name().to_string_lossy().trim_end_matches(".exe").to_string();
                let entry = groups.entry(name).or_insert((pid.as_u32(), 0.0, 0));
                entry.1 += p.cpu_usage() / cores;
                entry.2 += p.memory();
            }
            let mut list: Vec<_> = groups.into_iter().collect();
            if by.as_deref().unwrap_or("cpu") == "memory" {
                list.sort_by(|a, b| b.1 .2.cmp(&a.1 .2));
            } else {
                list.sort_by(|a, b| b.1 .1.total_cmp(&a.1 .1));
            }
            let table = lua.create_table()?;
            for (name, (pid, cpu, memory)) in list.into_iter().take(count.unwrap_or(5)) {
                let entry = lua.create_table()?;
                entry.set("name", name)?;
                entry.set("pid", pid)?;
                entry.set("cpu", (cpu * 10.0).round() / 10.0)?;
                entry.set("memory_mb", (memory as f64 / 1024.0 / 1024.0).round())?;
                table.push(entry)?;
            }
            Ok(table)
        })?,
    )?;

    let env = lua.create_table()?;
    env.set("get", lua.create_function(|_, name: String| Ok(std::env::var(name).ok()))?)?;
    globals.set("env", env)?;

    let service = lua.create_table()?;
    service.set(
        "list",
        lua.create_function(move |lua, ()| {
            let limit = budget(deadline, Duration::from_secs(60)).map_err(|e| err("service.list", e))?;
            let all = services_json(None, limit).map_err(|e| err("service.list", e))?;
            let table = lua.create_table()?;
            for item in all.as_array().into_iter().flatten() {
                table.push(service_table(lua, item)?)?;
            }
            Ok(table)
        })?,
    )?;
    service.set(
        "status",
        lua.create_function(move |lua, name: String| {
            let limit = budget(deadline, Duration::from_secs(60)).map_err(|e| err("service.status", e))?;
            match services_json(Some(&name), limit) {
                Ok(v) => match v.as_array().and_then(|a| a.first()) {
                    Some(item) => Ok(Some(service_table(lua, item)?)),
                    None => Ok(None),
                },
                Err(e) if e.contains("Cannot find") || e.contains("NoServiceFound") => Ok(None),
                Err(e) => Err(err("service.status", e)),
            }
        })?,
    )?;
    for (name, verb) in [("start", "Start-Service"), ("stop", "Stop-Service"), ("restart", "Restart-Service")] {
        let function = format!("service.{name}");
        service.set(
            name,
            lua.create_function(move |_, target: String| {
                require(allowed, &function)?;
                if !cfg!(windows) {
                    return Err(err(&function, "services are only on Windows"));
                }
                let limit = budget(deadline, Duration::from_secs(90)).map_err(|e| err(&function, e))?;
                let script = format!("{verb} -Name $env:LF_NAME -ErrorAction Stop");
                run(powershell_command(&script), limit, &[("LF_NAME", &target)])
                    .map_err(|e| err(&function, format!("{e} (most services need LocalFlow to run as administrator)")))?;
                Ok(true)
            })?,
        )?;
    }
    globals.set("service", service)?;

    let packages = lua.create_table()?;
    packages.set(
        "updates",
        lua.create_function(move |lua, ()| {
            let limit = budget(deadline, Duration::from_secs(120)).map_err(|e| err("packages.updates", e))?;
            let out = winget(&["upgrade"], limit).map_err(|e| err("packages.updates", e))?;
            let table = lua.create_table()?;
            for row in parse_winget_table(&out) {
                if row.len() < 4 {
                    continue;
                }
                let entry = lua.create_table()?;
                entry.set("name", row[0].as_str())?;
                entry.set("id", row[1].as_str())?;
                entry.set("version", row[2].as_str())?;
                entry.set("available", row[3].as_str())?;
                table.push(entry)?;
            }
            Ok(table)
        })?,
    )?;
    packages.set(
        "install",
        lua.create_function(move |_, id: String| {
            require(allowed, "packages.install")?;
            let limit = budget(deadline, Duration::from_secs(900)).map_err(|e| err("packages.install", e))?;
            winget(&["install", "--id", &id, "--exact", "--silent", "--accept-package-agreements"], limit)
                .map_err(|e| err("packages.install", e))?;
            Ok(true)
        })?,
    )?;
    packages.set(
        "upgrade",
        lua.create_function(move |_, id: String| {
            require(allowed, "packages.upgrade")?;
            let limit = budget(deadline, Duration::from_secs(1800)).map_err(|e| err("packages.upgrade", e))?;
            let args: Vec<&str> = if id == "all" {
                vec!["upgrade", "--all", "--silent", "--accept-package-agreements"]
            } else {
                vec!["upgrade", "--id", &id, "--exact", "--silent", "--accept-package-agreements"]
            };
            winget(&args, limit).map_err(|e| err("packages.upgrade", e))?;
            Ok(true)
        })?,
    )?;
    globals.set("packages", packages)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_winget_tables_in_any_language() {
        let text = "\r   - \r   \\ \r\
Имя                 ИД                   Версия   Доступно Источник\n\
---------------------------------------------------------------------\n\
Mozilla Firefox     Mozilla.Firefox      130.0    131.0    winget\n\
7-Zip 23.01         7zip.7zip            23.01    24.08    winget\n\
2 upgrades available.\n";
        let rows = parse_winget_table(text);
        assert_eq!(rows.len(), 2, "{rows:?}");
        assert_eq!(rows[0], ["Mozilla Firefox", "Mozilla.Firefox", "130.0", "131.0", "winget"]);
        assert_eq!(rows[1][1], "7zip.7zip");
        assert!(parse_winget_table("No installed package found.").is_empty());
    }

    #[test]
    fn finds_the_wifi_name() {
        let text = "    Name                   : Wi-Fi\n    BSSID                  : 00:11:22:33:44:55\n    SSID                   : Home network\n";
        assert_eq!(parse_wifi_name(text).as_deref(), Some("Home network"));
        assert_eq!(parse_wifi_name("    Name : Ethernet\n"), None);
    }

    #[test]
    fn never_overwrites() {
        let dir = tempfile::TempDir::new().unwrap();
        let first = dir.path().join("shot.png");
        assert_eq!(unused_path(&first), first);
        std::fs::write(&first, b"x").unwrap();
        assert_eq!(unused_path(&first), dir.path().join("shot (2).png"));
    }

    #[test]
    fn measures_connections() {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        assert!(connect_ms("127.0.0.1", port, Duration::from_secs(2)).is_some());
        drop(listener);
        assert!(connect_ms("127.0.0.1", port, Duration::from_secs(2)).is_none());
    }
}
