//! Controlling the PC: commands, processes, windows, keyboard and mouse, power.
//!
//! Anything that can change the system only works when the automation has
//! "Allow system control" switched on. Reading information is always allowed.
//!
//! ```lua
//! -- always allowed
//! process.list()                      -- { pid, name, memory_mb } for every process
//! process.running(name)               -- true if a program with that name runs
//! process.wait_for(name, seconds)     -- wait until it runs; true if it did
//! window.list()                       -- visible windows: { id, title, app, x, y, width, height, minimized, maximized }
//! window.find(text)                   -- first window whose title or app contains text, or nil
//! window.active()                     -- the window in front
//! screen.size()                       -- width, height of the main screen
//! mouse.position()                    -- x, y
//! system.idle_seconds()               -- time since the last key press or mouse move
//! network.wake_on_lan(mac)            -- wake another PC on the network
//!
//! -- need "Allow system control"
//! shell.run(command, { cwd, timeout })        -- cmd.exe: { code, ok, output, error }
//! shell.powershell(script, { cwd, timeout })  -- same, with PowerShell
//! process.kill(name_or_pid)                   -- stop a program; returns how many were stopped
//! window.focus/minimize/maximize/restore/close(win)
//! window.move(win, x, y, width, height)
//! keyboard.press("ctrl+shift+esc")  keyboard.type("text")
//! mouse.move(x, y)  mouse.click(x, y, "left")
//! system.lock()  system.sleep()  system.shutdown(delay)  system.restart(delay)  system.cancel_shutdown()
//! system.volume_up(steps)  system.volume_down(steps)  system.mute()
//! system.brightness(percent)  system.set_wallpaper(path)
//! system.wake_at("07:30")  system.cancel_wake()
//! ```

use std::{
    io::Read,
    net::UdpSocket,
    path::PathBuf,
    process::{Command, Stdio},
    sync::Arc,
    time::{Duration, Instant},
};

use chrono::{Local, NaiveDateTime, NaiveTime, TimeZone};
use mlua::{Lua, Table, Value};

use super::sandbox::PathPolicy;

fn err(function: &str, error: impl std::fmt::Display) -> mlua::Error {
    mlua::Error::runtime(format!("{function}: {error}"))
}

/// The error for a powerful function in an automation without permission.
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

pub fn register(lua: &Lua, allowed: bool, policy: Arc<PathPolicy>, deadline: Instant) -> mlua::Result<()> {
    let globals = lua.globals();
    globals.set("shell", shell_table(lua, allowed, deadline)?)?;
    globals.set("process", process_table(lua, allowed, deadline)?)?;
    globals.set("window", window_table(lua, allowed)?)?;
    globals.set("keyboard", keyboard_table(lua, allowed)?)?;
    globals.set("mouse", mouse_table(lua, allowed)?)?;
    globals.set("screen", screen_table(lua)?)?;
    globals.set("network", network_table(lua)?)?;
    // Adds to the `system` table made in system.rs.
    let system: Table = globals.get("system")?;
    add_power_functions(lua, &system, allowed, policy, deadline)?;
    Ok(())
}

// ---- shell -------------------------------------------------------------------------

#[cfg(windows)]
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

/// Run a program, capturing its output, and kill it if it runs past `timeout`.
fn run_program(mut command: Command, timeout: Duration) -> Result<(i32, String, String), String> {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(CREATE_NO_WINDOW);
    }
    command.stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped());
    let mut child = command.spawn().map_err(|e| format!("could not start: {e}"))?;

    // Read output on threads, so a chatty program can't fill the pipe and hang.
    let mut stdout = child.stdout.take().expect("piped stdout");
    let mut stderr = child.stderr.take().expect("piped stderr");
    let out = std::thread::spawn(move || {
        let mut buf = Vec::new();
        let _ = stdout.read_to_end(&mut buf);
        buf
    });
    let err_out = std::thread::spawn(move || {
        let mut buf = Vec::new();
        let _ = stderr.read_to_end(&mut buf);
        buf
    });

    let started = Instant::now();
    let status = loop {
        if let Some(status) = child.try_wait().map_err(|e| e.to_string())? {
            break status;
        }
        if started.elapsed() > timeout {
            let _ = child.kill();
            let _ = child.wait();
            return Err(format!("stopped after {} seconds (time limit)", timeout.as_secs()));
        }
        std::thread::sleep(Duration::from_millis(20));
    };
    let text = |bytes: Vec<u8>| String::from_utf8_lossy(&bytes).trim_end().to_string();
    Ok((status.code().unwrap_or(-1), text(out.join().unwrap_or_default()), text(err_out.join().unwrap_or_default())))
}

fn base64(bytes: &[u8]) -> String {
    const TABLE: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let n = u32::from(chunk[0]) << 16 | u32::from(*chunk.get(1).unwrap_or(&0)) << 8 | u32::from(*chunk.get(2).unwrap_or(&0));
        for i in 0..4 {
            if i <= chunk.len() {
                out.push(TABLE[(n >> (18 - 6 * i) & 63) as usize] as char);
            } else {
                out.push('=');
            }
        }
    }
    out
}

/// A PowerShell command line that runs `script` with UTF-8 output.
fn powershell_command(script: &str) -> Command {
    let full = format!("[Console]::OutputEncoding = [Text.Encoding]::UTF8\n$ProgressPreference = 'SilentlyContinue'\n{script}");
    let utf16: Vec<u8> = full.encode_utf16().flat_map(|u| u.to_le_bytes()).collect();
    let mut command = Command::new("powershell.exe");
    command.args(["-NoProfile", "-NonInteractive", "-ExecutionPolicy", "Bypass", "-EncodedCommand", &base64(&utf16)]);
    command
}

fn cmd_command(line: &str) -> Command {
    let mut command = Command::new("cmd.exe");
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        // chcp 65001: UTF-8 output. raw_arg: cmd.exe does its own quoting.
        command.args(["/d", "/s", "/c"]).raw_arg(format!("\"chcp 65001 >nul & {line}\""));
    }
    #[cfg(not(windows))]
    command.args(["-c", line]);
    command
}

/// Seconds left before the script's time limit, capped by the caller's own timeout.
fn time_budget(deadline: Instant, wanted: Option<f64>) -> mlua::Result<Duration> {
    let remaining = deadline.saturating_duration_since(Instant::now());
    let wanted = wanted.map(Duration::from_secs_f64).unwrap_or(Duration::from_secs(60));
    let budget = remaining.min(wanted);
    if budget.is_zero() {
        return Err(mlua::Error::runtime("no time left before the script's time limit"));
    }
    Ok(budget)
}

fn shell_table(lua: &Lua, allowed: bool, deadline: Instant) -> mlua::Result<Table> {
    let shell = lua.create_table()?;
    for (name, use_powershell) in [("run", false), ("powershell", true)] {
        let function = if use_powershell { "shell.powershell" } else { "shell.run" };
        shell.set(
            name,
            lua.create_function(move |lua, (text, options): (String, Option<Table>)| {
                require(allowed, function)?;
                let (cwd, timeout) = match &options {
                    Some(o) => (o.get::<Option<String>>("cwd")?, o.get::<Option<f64>>("timeout")?),
                    None => (None, None),
                };
                let mut command = if use_powershell { powershell_command(&text) } else { cmd_command(&text) };
                if let Some(cwd) = cwd {
                    command.current_dir(super::system::expand_home(&cwd));
                }
                let (code, output, error) =
                    run_program(command, time_budget(deadline, timeout)?).map_err(|e| err(function, e))?;
                let result = lua.create_table()?;
                result.set("code", code)?;
                result.set("ok", code == 0)?;
                result.set("output", output)?;
                result.set("error", error)?;
                Ok(result)
            })?,
        )?;
    }
    Ok(shell)
}

// ---- processes -----------------------------------------------------------------------

/// Programs LocalFlow will never stop: Windows needs them, or it's LocalFlow itself.
const PROTECTED: &[&str] = &[
    "system", "idle", "registry", "smss", "csrss", "wininit", "winlogon", "services", "lsass", "lsaiso", "svchost",
    "dwm", "fontdrvhost", "memory compression", "secure system", "localflow-desktop", "localflow",
];

fn process_key(name: &str) -> String {
    let lower = name.trim().to_lowercase();
    lower.strip_suffix(".exe").unwrap_or(&lower).to_string()
}

fn processes() -> sysinfo::System {
    let mut system = sysinfo::System::new();
    system.refresh_processes(sysinfo::ProcessesToUpdate::All, true);
    system
}

fn process_table(lua: &Lua, allowed: bool, deadline: Instant) -> mlua::Result<Table> {
    let process = lua.create_table()?;

    process.set(
        "list",
        lua.create_function(|lua, ()| {
            let system = processes();
            let mut list: Vec<_> = system.processes().iter().collect();
            list.sort_by_key(|(_, p)| process_key(&p.name().to_string_lossy()));
            let table = lua.create_table()?;
            for (pid, p) in list {
                let entry = lua.create_table()?;
                entry.set("pid", pid.as_u32())?;
                entry.set("name", p.name().to_string_lossy().into_owned())?;
                entry.set("memory_mb", (p.memory() as f64 / 1024.0 / 1024.0).round())?;
                table.push(entry)?;
            }
            Ok(table)
        })?,
    )?;

    process.set(
        "running",
        lua.create_function(|_, name: String| {
            let wanted = process_key(&name);
            Ok(processes().processes().values().any(|p| process_key(&p.name().to_string_lossy()) == wanted))
        })?,
    )?;

    process.set(
        "wait_for",
        lua.create_function(move |_, (name, seconds): (String, Option<f64>)| {
            let wanted = process_key(&name);
            let budget = time_budget(deadline, Some(seconds.unwrap_or(30.0)))?;
            let until = Instant::now() + budget;
            loop {
                if processes().processes().values().any(|p| process_key(&p.name().to_string_lossy()) == wanted) {
                    return Ok(true);
                }
                if Instant::now() >= until {
                    return Ok(false);
                }
                std::thread::sleep(Duration::from_millis(500));
            }
        })?,
    )?;

    process.set(
        "kill",
        lua.create_function(move |_, target: Value| {
            require(allowed, "process.kill")?;
            let system = processes();
            let matches: Vec<&sysinfo::Process> = match &target {
                Value::Integer(pid) => system.process(sysinfo::Pid::from_u32(*pid as u32)).into_iter().collect(),
                Value::Number(pid) => system.process(sysinfo::Pid::from_u32(*pid as u32)).into_iter().collect(),
                Value::String(name) => {
                    let wanted = process_key(&name.to_str()?);
                    system.processes().values().filter(|p| process_key(&p.name().to_string_lossy()) == wanted).collect()
                }
                _ => return Err(err("process.kill", "give a program name or a process id")),
            };
            let mut stopped = 0;
            for p in matches {
                let name = process_key(&p.name().to_string_lossy());
                if PROTECTED.contains(&name.as_str()) || p.pid().as_u32() == std::process::id() {
                    return Err(err("process.kill", format!("{name} is protected; LocalFlow won't stop it")));
                }
                if p.kill() {
                    stopped += 1;
                }
            }
            Ok(stopped)
        })?,
    )?;

    Ok(process)
}

// ---- windows ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub struct WindowInfo {
    pub id: isize,
    pub title: String,
    pub app: String,
    pub rect: (i32, i32, i32, i32),
    pub minimized: bool,
    pub maximized: bool,
}

#[cfg(windows)]
mod win {
    use super::WindowInfo;
    use windows_sys::Win32::{
        Foundation::{BOOL, HWND, LPARAM, RECT},
        UI::WindowsAndMessaging::*,
    };

    unsafe extern "system" fn collect(hwnd: HWND, lparam: LPARAM) -> BOOL {
        let list = &mut *(lparam as *mut Vec<HWND>);
        list.push(hwnd);
        1
    }

    fn title(hwnd: HWND) -> String {
        // SAFETY: the buffer is sized from GetWindowTextLengthW plus the NUL.
        unsafe {
            let len = GetWindowTextLengthW(hwnd);
            if len <= 0 {
                return String::new();
            }
            let mut buf = vec![0u16; len as usize + 1];
            let copied = GetWindowTextW(hwnd, buf.as_mut_ptr(), buf.len() as i32);
            String::from_utf16_lossy(&buf[..copied.max(0) as usize])
        }
    }

    /// Visible top-level windows with a title, front to back.
    pub fn list(process_name: impl Fn(u32) -> String) -> Vec<WindowInfo> {
        let mut handles: Vec<HWND> = Vec::new();
        // SAFETY: the callback only pushes into the Vec passed through lparam.
        unsafe { EnumWindows(Some(collect), &mut handles as *mut _ as LPARAM) };
        handles
            .into_iter()
            .filter_map(|hwnd| {
                // SAFETY: plain queries on a window handle from EnumWindows.
                unsafe {
                    if IsWindowVisible(hwnd) == 0 {
                        return None;
                    }
                    let title = title(hwnd);
                    if title.trim().is_empty() {
                        return None;
                    }
                    let mut pid = 0u32;
                    GetWindowThreadProcessId(hwnd, &mut pid);
                    let mut rect: RECT = std::mem::zeroed();
                    GetWindowRect(hwnd, &mut rect);
                    Some(WindowInfo {
                        id: hwnd as isize,
                        title,
                        app: process_name(pid),
                        rect: (rect.left, rect.top, rect.right - rect.left, rect.bottom - rect.top),
                        minimized: IsIconic(hwnd) != 0,
                        maximized: IsZoomed(hwnd) != 0,
                    })
                }
            })
            .collect()
    }

    pub fn active() -> isize {
        // SAFETY: no arguments.
        unsafe { GetForegroundWindow() as isize }
    }

    pub fn exists(id: isize) -> bool {
        // SAFETY: IsWindow accepts any value.
        unsafe { IsWindow(id as HWND) != 0 }
    }

    pub fn show(id: isize, command: &str) -> bool {
        let hwnd = id as HWND;
        // SAFETY: calls on a handle checked with IsWindow by the caller.
        unsafe {
            match command {
                "focus" => {
                    if IsIconic(hwnd) != 0 {
                        ShowWindow(hwnd, SW_RESTORE);
                    }
                    SetForegroundWindow(hwnd) != 0
                }
                "minimize" => ShowWindow(hwnd, SW_MINIMIZE) != 0 || true,
                "maximize" => ShowWindow(hwnd, SW_MAXIMIZE) != 0 || true,
                "restore" => ShowWindow(hwnd, SW_RESTORE) != 0 || true,
                // Asks the program to close, like clicking X: it can still ask to save.
                "close" => PostMessageW(hwnd, WM_CLOSE, 0, 0) != 0,
                _ => false,
            }
        }
    }

    pub fn move_to(id: isize, x: i32, y: i32, width: i32, height: i32) -> bool {
        let hwnd = id as HWND;
        // SAFETY: calls on a handle checked with IsWindow by the caller.
        unsafe {
            if IsZoomed(hwnd) != 0 || IsIconic(hwnd) != 0 {
                ShowWindow(hwnd, SW_RESTORE);
            }
            MoveWindow(hwnd, x, y, width, height, 1) != 0
        }
    }
}

fn window_list() -> Vec<WindowInfo> {
    #[cfg(windows)]
    {
        let system = processes();
        win::list(|pid| {
            system
                .process(sysinfo::Pid::from_u32(pid))
                .map(|p| process_key(&p.name().to_string_lossy()))
                .unwrap_or_default()
        })
    }
    #[cfg(not(windows))]
    Vec::new()
}

fn window_to_lua(lua: &Lua, w: &WindowInfo) -> mlua::Result<Table> {
    let t = lua.create_table()?;
    t.set("id", w.id as i64)?;
    t.set("title", w.title.as_str())?;
    t.set("app", w.app.as_str())?;
    t.set("x", w.rect.0)?;
    t.set("y", w.rect.1)?;
    t.set("width", w.rect.2)?;
    t.set("height", w.rect.3)?;
    t.set("minimized", w.minimized)?;
    t.set("maximized", w.maximized)?;
    Ok(t)
}

fn find_window(text: &str) -> Option<WindowInfo> {
    let wanted = text.trim().to_lowercase();
    let list = window_list();
    list.iter()
        .find(|w| w.app == process_key(&wanted))
        .or_else(|| list.iter().find(|w| w.title.to_lowercase().contains(&wanted)))
        .cloned()
}

/// A window given as an id, a table from window.list/find, or text to search for.
fn window_id(function: &str, value: &Value) -> mlua::Result<isize> {
    let id = match value {
        Value::Integer(id) => *id as isize,
        Value::Number(id) => *id as isize,
        Value::Table(t) => t.get::<i64>("id")? as isize,
        Value::String(s) => {
            let text = s.to_str()?.to_string();
            find_window(&text).map(|w| w.id).ok_or_else(|| err(function, format!("no window matches \"{text}\"")))?
        }
        _ => return Err(err(function, "give a window from window.find, or text to search for")),
    };
    #[cfg(windows)]
    if !win::exists(id) {
        return Err(err(function, "that window is gone"));
    }
    Ok(id)
}

fn window_table(lua: &Lua, allowed: bool) -> mlua::Result<Table> {
    let window = lua.create_table()?;

    window.set(
        "list",
        lua.create_function(|lua, ()| {
            let table = lua.create_table()?;
            for w in window_list() {
                table.push(window_to_lua(lua, &w)?)?;
            }
            Ok(table)
        })?,
    )?;

    window.set(
        "find",
        lua.create_function(|lua, text: String| match find_window(&text) {
            Some(w) => Ok(Value::Table(window_to_lua(lua, &w)?)),
            None => Ok(Value::Nil),
        })?,
    )?;

    window.set(
        "active",
        lua.create_function(|lua, ()| {
            #[cfg(windows)]
            {
                let id = win::active();
                if let Some(w) = window_list().into_iter().find(|w| w.id == id) {
                    return Ok(Value::Table(window_to_lua(lua, &w)?));
                }
            }
            let _ = lua;
            Ok(Value::Nil)
        })?,
    )?;

    for action in ["focus", "minimize", "maximize", "restore", "close"] {
        let function: &'static str = match action {
            "focus" => "window.focus",
            "minimize" => "window.minimize",
            "maximize" => "window.maximize",
            "restore" => "window.restore",
            _ => "window.close",
        };
        window.set(
            action,
            lua.create_function(move |_, target: Value| {
                require(allowed, function)?;
                let id = window_id(function, &target)?;
                #[cfg(windows)]
                return Ok(win::show(id, action));
                #[cfg(not(windows))]
                {
                    let _ = id;
                    Err(err(function, "only available on Windows"))
                }
            })?,
        )?;
    }

    window.set(
        "move",
        lua.create_function(move |_, (target, x, y, width, height): (Value, i32, i32, i32, i32)| {
            require(allowed, "window.move")?;
            let id = window_id("window.move", &target)?;
            #[cfg(windows)]
            return Ok(win::move_to(id, x, y, width.max(100), height.max(50)));
            #[cfg(not(windows))]
            {
                let _ = (id, x, y, width, height);
                Err(err("window.move", "only available on Windows"))
            }
        })?,
    )?;

    Ok(window)
}

// ---- keyboard, mouse, screen -------------------------------------------------------------

#[cfg(windows)]
mod input {
    use enigo::{Button, Coordinate, Direction, Enigo, Key, Keyboard, Mouse, Settings};

    fn enigo() -> Result<Enigo, String> {
        Enigo::new(&Settings::default()).map_err(|e| e.to_string())
    }

    fn key(name: &str) -> Result<Key, String> {
        let lower = name.to_lowercase();
        let key = match lower.as_str() {
            "ctrl" | "control" => Key::Control,
            "alt" => Key::Alt,
            "shift" => Key::Shift,
            "win" | "super" | "meta" => Key::Meta,
            "enter" | "return" => Key::Return,
            "tab" => Key::Tab,
            "esc" | "escape" => Key::Escape,
            "space" => Key::Space,
            "backspace" => Key::Backspace,
            "delete" | "del" => Key::Delete,
            "home" => Key::Home,
            "end" => Key::End,
            "pageup" => Key::PageUp,
            "pagedown" => Key::PageDown,
            "up" => Key::UpArrow,
            "down" => Key::DownArrow,
            "left" => Key::LeftArrow,
            "right" => Key::RightArrow,
            "volumeup" => Key::VolumeUp,
            "volumedown" => Key::VolumeDown,
            "mute" | "volumemute" => Key::VolumeMute,
            "playpause" => Key::MediaPlayPause,
            "next" => Key::MediaNextTrack,
            "prev" | "previous" => Key::MediaPrevTrack,
            "f1" => Key::F1,
            "f2" => Key::F2,
            "f3" => Key::F3,
            "f4" => Key::F4,
            "f5" => Key::F5,
            "f6" => Key::F6,
            "f7" => Key::F7,
            "f8" => Key::F8,
            "f9" => Key::F9,
            "f10" => Key::F10,
            "f11" => Key::F11,
            "f12" => Key::F12,
            _ => {
                let mut chars = lower.chars();
                match (chars.next(), chars.next()) {
                    (Some(c), None) => Key::Unicode(c),
                    _ => return Err(format!("unknown key \"{name}\"")),
                }
            }
        };
        Ok(key)
    }

    /// "ctrl+shift+esc": hold the first keys, tap the last, release in reverse.
    pub fn press(combo: &str) -> Result<(), String> {
        let keys: Vec<Key> = combo
            .split('+')
            .map(str::trim)
            .filter(|k| !k.is_empty())
            .map(key)
            .collect::<Result<_, _>>()?;
        let (last, held) = keys.split_last().ok_or("no key given")?;
        let mut e = enigo()?;
        for k in held {
            e.key(*k, Direction::Press).map_err(|e| e.to_string())?;
        }
        let result = e.key(*last, Direction::Click).map_err(|e| e.to_string());
        for k in held.iter().rev() {
            let _ = e.key(*k, Direction::Release);
        }
        result
    }

    pub fn type_text(text: &str) -> Result<(), String> {
        enigo()?.text(text).map_err(|e| e.to_string())
    }

    pub fn move_mouse(x: i32, y: i32) -> Result<(), String> {
        enigo()?.move_mouse(x, y, Coordinate::Abs).map_err(|e| e.to_string())
    }

    pub fn click(position: Option<(i32, i32)>, button: &str, double: bool) -> Result<(), String> {
        let mut e = enigo()?;
        if let Some((x, y)) = position {
            e.move_mouse(x, y, Coordinate::Abs).map_err(|e| e.to_string())?;
        }
        let button = match button {
            "right" => Button::Right,
            "middle" => Button::Middle,
            _ => Button::Left,
        };
        for _ in 0..if double { 2 } else { 1 } {
            e.button(button, Direction::Click).map_err(|e| e.to_string())?;
        }
        Ok(())
    }

    pub fn position() -> Result<(i32, i32), String> {
        enigo()?.location().map_err(|e| e.to_string())
    }

    pub fn screen() -> Result<(i32, i32), String> {
        enigo()?.main_display().map_err(|e| e.to_string())
    }
}

#[cfg(not(windows))]
mod input {
    const NO: &str = "only available on Windows";
    pub fn press(_: &str) -> Result<(), String> {
        Err(NO.into())
    }
    pub fn type_text(_: &str) -> Result<(), String> {
        Err(NO.into())
    }
    pub fn move_mouse(_: i32, _: i32) -> Result<(), String> {
        Err(NO.into())
    }
    pub fn click(_: Option<(i32, i32)>, _: &str, _: bool) -> Result<(), String> {
        Err(NO.into())
    }
    pub fn position() -> Result<(i32, i32), String> {
        Err(NO.into())
    }
    pub fn screen() -> Result<(i32, i32), String> {
        Err(NO.into())
    }
}

fn keyboard_table(lua: &Lua, allowed: bool) -> mlua::Result<Table> {
    let keyboard = lua.create_table()?;
    keyboard.set(
        "press",
        lua.create_function(move |_, combo: String| {
            require(allowed, "keyboard.press")?;
            input::press(&combo).map_err(|e| err("keyboard.press", e))
        })?,
    )?;
    keyboard.set(
        "type",
        lua.create_function(move |_, text: String| {
            require(allowed, "keyboard.type")?;
            input::type_text(&text).map_err(|e| err("keyboard.type", e))
        })?,
    )?;
    Ok(keyboard)
}

fn mouse_table(lua: &Lua, allowed: bool) -> mlua::Result<Table> {
    let mouse = lua.create_table()?;
    mouse.set(
        "move",
        lua.create_function(move |_, (x, y): (i32, i32)| {
            require(allowed, "mouse.move")?;
            input::move_mouse(x, y).map_err(|e| err("mouse.move", e))
        })?,
    )?;
    mouse.set(
        "click",
        lua.create_function(move |_, (x, y, button, double): (Option<i32>, Option<i32>, Option<String>, Option<bool>)| {
            require(allowed, "mouse.click")?;
            let position = x.zip(y);
            input::click(position, button.as_deref().unwrap_or("left"), double.unwrap_or(false))
                .map_err(|e| err("mouse.click", e))
        })?,
    )?;
    mouse.set(
        "position",
        lua.create_function(|_, ()| input::position().map_err(|e| err("mouse.position", e)))?,
    )?;
    Ok(mouse)
}

fn screen_table(lua: &Lua) -> mlua::Result<Table> {
    let screen = lua.create_table()?;
    screen.set("size", lua.create_function(|_, ()| input::screen().map_err(|e| err("screen.size", e)))?)?;
    Ok(screen)
}

// ---- network -------------------------------------------------------------------------------

/// The "magic packet": six 0xFF bytes, then the MAC address 16 times.
pub fn magic_packet(mac: &str) -> Result<Vec<u8>, String> {
    let bytes: Vec<u8> = mac
        .split([':', '-'])
        .map(|part| u8::from_str_radix(part, 16))
        .collect::<Result<_, _>>()
        .map_err(|_| format!("\"{mac}\" is not a MAC address like AA:BB:CC:DD:EE:FF"))?;
    if bytes.len() != 6 {
        return Err(format!("\"{mac}\" is not a MAC address like AA:BB:CC:DD:EE:FF"));
    }
    let mut packet = vec![0xFF; 6];
    for _ in 0..16 {
        packet.extend_from_slice(&bytes);
    }
    Ok(packet)
}

fn network_table(lua: &Lua) -> mlua::Result<Table> {
    let network = lua.create_table()?;
    network.set(
        "wake_on_lan",
        lua.create_function(|_, (mac, address): (String, Option<String>)| {
            let packet = magic_packet(&mac).map_err(|e| err("network.wake_on_lan", e))?;
            let socket = UdpSocket::bind("0.0.0.0:0").map_err(|e| err("network.wake_on_lan", e))?;
            socket.set_broadcast(true).map_err(|e| err("network.wake_on_lan", e))?;
            let target = format!("{}:9", address.as_deref().unwrap_or("255.255.255.255"));
            socket.send_to(&packet, target).map_err(|e| err("network.wake_on_lan", e))?;
            Ok(true)
        })?,
    )?;
    Ok(network)
}

// ---- power, sound, display ----------------------------------------------------------------------

/// Seconds since the last keyboard or mouse input (0 where unknown).
pub fn idle_seconds() -> u64 {
    #[cfg(windows)]
    {
        use windows_sys::Win32::{
            System::SystemInformation::GetTickCount,
            UI::Input::KeyboardAndMouse::{GetLastInputInfo, LASTINPUTINFO},
        };
        let mut info = LASTINPUTINFO { cbSize: std::mem::size_of::<LASTINPUTINFO>() as u32, dwTime: 0 };
        // SAFETY: `info` is a valid LASTINPUTINFO with cbSize set.
        if unsafe { GetLastInputInfo(&mut info) } == 0 {
            return 0;
        }
        // SAFETY: no arguments.
        let now = unsafe { GetTickCount() };
        u64::from(now.wrapping_sub(info.dwTime)) / 1000
    }
    #[cfg(not(windows))]
    0
}

const WAKE_TASK: &str = "LocalFlow wake";

/// "07:30" (next time it's 07:30), "2026-10-01 07:30", or a timestamp.
fn wake_time(value: &Value) -> Result<chrono::DateTime<Local>, String> {
    let now = Local::now();
    match value {
        Value::Integer(t) => Local.timestamp_opt(*t, 0).single().ok_or("invalid timestamp".into()),
        Value::Number(t) => Local.timestamp_opt(*t as i64, 0).single().ok_or("invalid timestamp".into()),
        Value::String(s) => {
            let text = s.to_str().map_err(|e| e.to_string())?.trim().to_string();
            if let Ok(time) = NaiveTime::parse_from_str(&text, "%H:%M") {
                let today = now.date_naive().and_time(time);
                let mut when = Local.from_local_datetime(&today).single().ok_or("invalid time")?;
                if when <= now {
                    when += chrono::Duration::days(1);
                }
                return Ok(when);
            }
            let naive = NaiveDateTime::parse_from_str(&text, "%Y-%m-%d %H:%M")
                .map_err(|_| format!("\"{text}\" is not a time like \"07:30\" or \"2026-10-01 07:30\""))?;
            Local.from_local_datetime(&naive).single().ok_or("invalid time".into())
        }
        _ => Err("give a time like \"07:30\"".into()),
    }
}

fn add_power_functions(lua: &Lua, system: &Table, allowed: bool, policy: Arc<PathPolicy>, deadline: Instant) -> mlua::Result<()> {
    system.set("idle_seconds", lua.create_function(|_, ()| Ok(idle_seconds()))?)?;

    system.set(
        "lock",
        lua.create_function(move |_, ()| {
            require(allowed, "system.lock")?;
            #[cfg(windows)]
            {
                // SAFETY: no arguments.
                if unsafe { windows_sys::Win32::System::Shutdown::LockWorkStation() } == 0 {
                    return Err(err("system.lock", "Windows refused to lock"));
                }
                Ok(())
            }
            #[cfg(not(windows))]
            Err(err("system.lock", "only available on Windows"))
        })?,
    )?;

    system.set(
        "sleep",
        lua.create_function(move |_, ()| {
            require(allowed, "system.sleep")?;
            #[cfg(windows)]
            {
                // SAFETY: plain flags; puts the PC to sleep (not hibernate).
                unsafe { windows_sys::Win32::System::Power::SetSuspendState(0, 0, 0) };
                Ok(())
            }
            #[cfg(not(windows))]
            Err(err("system.sleep", "only available on Windows"))
        })?,
    )?;

    for (name, flag) in [("shutdown", "/s"), ("restart", "/r")] {
        let function: &'static str = if flag == "/s" { "system.shutdown" } else { "system.restart" };
        system.set(
            name,
            lua.create_function(move |_, delay: Option<u32>| {
                require(allowed, function)?;
                // A delay by default, so there's always time to cancel (shutdown /a).
                let delay = delay.unwrap_or(60).min(3600);
                let mut command = Command::new("shutdown.exe");
                command.args([flag, "/t", &delay.to_string(), "/c", "LocalFlow: an automation asked for this. To cancel: shutdown /a"]);
                run_program(command, Duration::from_secs(20)).map_err(|e| err(function, e))?;
                Ok(delay)
            })?,
        )?;
    }

    system.set(
        "cancel_shutdown",
        lua.create_function(move |_, ()| {
            require(allowed, "system.cancel_shutdown")?;
            let mut command = Command::new("shutdown.exe");
            command.arg("/a");
            let (code, _, _) = run_program(command, Duration::from_secs(20)).map_err(|e| err("system.cancel_shutdown", e))?;
            Ok(code == 0)
        })?,
    )?;

    for (name, key) in [("volume_up", "volumeup"), ("volume_down", "volumedown")] {
        let function: &'static str = if key == "volumeup" { "system.volume_up" } else { "system.volume_down" };
        system.set(
            name,
            lua.create_function(move |_, steps: Option<u32>| {
                require(allowed, function)?;
                // Each step is 2% on most PCs.
                for _ in 0..steps.unwrap_or(5).min(50) {
                    input::press(key).map_err(|e| err(function, e))?;
                }
                Ok(())
            })?,
        )?;
    }

    system.set(
        "mute",
        lua.create_function(move |_, ()| {
            require(allowed, "system.mute")?;
            input::press("mute").map_err(|e| err("system.mute", e))
        })?,
    )?;

    system.set(
        "brightness",
        lua.create_function(move |_, percent: u32| {
            require(allowed, "system.brightness")?;
            let percent = percent.min(100);
            let script = format!(
                "(Get-CimInstance -Namespace root/WMI -ClassName WmiMonitorBrightnessMethods -ErrorAction Stop) | \
                 Invoke-CimMethod -MethodName WmiSetBrightness -Arguments @{{Timeout=1; Brightness={percent}}} | Out-Null"
            );
            let (code, _, error) = run_program(powershell_command(&script), time_budget(deadline, Some(20.0))?)
                .map_err(|e| err("system.brightness", e))?;
            if code != 0 {
                return Err(err("system.brightness", format!("not supported on this screen ({error})")));
            }
            Ok(percent)
        })?,
    )?;

    system.set(
        "set_wallpaper",
        lua.create_function(move |_, path: String| {
            require(allowed, "system.set_wallpaper")?;
            let resolved: PathBuf = policy.resolve(&path).map_err(|e| err("system.set_wallpaper", e))?;
            if !resolved.is_file() {
                return Err(err("system.set_wallpaper", format!("file not found: {path}")));
            }
            #[cfg(windows)]
            {
                use std::os::windows::ffi::OsStrExt;
                use windows_sys::Win32::UI::WindowsAndMessaging::{
                    SystemParametersInfoW, SPIF_SENDCHANGE, SPIF_UPDATEINIFILE, SPI_SETDESKWALLPAPER,
                };
                let wide: Vec<u16> = resolved.as_os_str().encode_wide().chain(std::iter::once(0)).collect();
                // SAFETY: `wide` is a NUL-terminated path that outlives the call.
                let ok = unsafe {
                    SystemParametersInfoW(SPI_SETDESKWALLPAPER, 0, wide.as_ptr() as *mut _, SPIF_UPDATEINIFILE | SPIF_SENDCHANGE)
                };
                if ok == 0 {
                    return Err(err("system.set_wallpaper", "Windows refused this picture"));
                }
                Ok(())
            }
            #[cfg(not(windows))]
            Err(err("system.set_wallpaper", "only available on Windows"))
        })?,
    )?;

    system.set(
        "wake_at",
        lua.create_function(move |_, when: Value| {
            require(allowed, "system.wake_at")?;
            let time = wake_time(&when).map_err(|e| err("system.wake_at", e))?;
            // A Windows task that is allowed to wake the PC from sleep.
            let script = format!(
                "$trigger = New-ScheduledTaskTrigger -Once -At '{}'\n\
                 $settings = New-ScheduledTaskSettingsSet -WakeToRun -StartWhenAvailable -AllowStartIfOnBatteries -DontStopIfGoingOnBatteries\n\
                 $action = New-ScheduledTaskAction -Execute 'cmd.exe' -Argument '/c exit 0'\n\
                 Register-ScheduledTask -TaskName '{WAKE_TASK}' -Trigger $trigger -Settings $settings -Action $action -Force | Out-Null",
                time.format("%Y-%m-%dT%H:%M:%S")
            );
            let (code, _, error) = run_program(powershell_command(&script), time_budget(deadline, Some(30.0))?)
                .map_err(|e| err("system.wake_at", e))?;
            if code != 0 {
                return Err(err("system.wake_at", format!("could not set the wake timer: {error}")));
            }
            Ok(time.format("%Y-%m-%d %H:%M").to_string())
        })?,
    )?;

    system.set(
        "cancel_wake",
        lua.create_function(move |_, ()| {
            require(allowed, "system.cancel_wake")?;
            let script = format!("Unregister-ScheduledTask -TaskName '{WAKE_TASK}' -Confirm:$false -ErrorAction SilentlyContinue");
            run_program(powershell_command(&script), time_budget(deadline, Some(30.0))?)
                .map_err(|e| err("system.cancel_wake", e))?;
            Ok(())
        })?,
    )?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn magic_packet_layout() {
        let packet = magic_packet("AA:BB:CC:DD:EE:FF").unwrap();
        assert_eq!(packet.len(), 6 + 16 * 6);
        assert_eq!(&packet[..6], &[0xFF; 6]);
        assert_eq!(&packet[6..12], &[0xAA, 0xBB, 0xCC, 0xDD, 0xEE, 0xFF]);
        assert!(magic_packet("aa-bb-cc-dd-ee-ff").is_ok());
        assert!(magic_packet("not a mac").is_err());
        assert!(magic_packet("AA:BB:CC").is_err());
    }

    #[test]
    fn base64_matches_known_values() {
        assert_eq!(base64(b""), "");
        assert_eq!(base64(b"f"), "Zg==");
        assert_eq!(base64(b"fo"), "Zm8=");
        assert_eq!(base64(b"foo"), "Zm9v");
        assert_eq!(base64(b"foobar"), "Zm9vYmFy");
    }

    #[test]
    fn wake_times() {
        let t = wake_time(&Value::String(mlua::Lua::new().create_string("07:30").unwrap())).unwrap();
        assert!(t > Local::now() && t < Local::now() + chrono::Duration::days(1) + chrono::Duration::minutes(1));
        assert!(wake_time(&Value::Nil).is_err());
    }
}
