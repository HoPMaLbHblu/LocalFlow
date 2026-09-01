//! Efficiency mode and priority, like Task Manager's "Efficiency mode" (Windows 11).
//!
//! ```lua
//! process.set_efficiency("Discord", true)    -- 🔒 like Task Manager: low priority + EcoQoS
//! process.efficiency("Discord")              -- true / false, nil if it isn't running
//! process.set_priority("7zFM", "below_normal") -- 🔒 low, below_normal, normal, above_normal, high
//! process.priority("7zFM")                   -- "normal", ...
//! ```
//!
//! Efficiency mode is what Task Manager does: the process gets the lowest priority class and
//! Windows' power throttling ("EcoQoS"), so it runs on efficient cores at lower clock speeds.
//! It applies to every process with that name (browsers and chat apps run many). Some apps
//! (Chrome, Edge) switch their own tabs in and out of it, which may undo the setting for a tab.
//! Only affects running processes; it is gone when the app restarts. Switching it off gives
//! each process back the priority it had before and lets Windows and the app manage
//! throttling again.

use mlua::{Lua, Table, Value};

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

/// Windows' own processes, which LocalFlow never slows down.
const PROTECTED: &[&str] = &[
    "system", "idle", "registry", "smss", "csrss", "wininit", "winlogon", "services", "lsass", "lsaiso", "svchost",
    "dwm", "fontdrvhost", "memory compression", "secure system", "explorer", "audiodg", "ctfmon", "sihost",
    "localflow-desktop", "localflow", "kernel_task", "launchd", "windowserver",
];

pub const PRIORITIES: &[&str] = &["low", "below_normal", "normal", "above_normal", "high"];

fn process_key(name: &str) -> String {
    let lower = name.trim().to_lowercase();
    lower.strip_suffix(".exe").unwrap_or(&lower).to_string()
}

/// The process ids a target means: a pid, or every process with that program name.
fn targets(function: &str, target: &Value) -> mlua::Result<(String, Vec<u32>)> {
    let mut system = sysinfo::System::new();
    system.refresh_processes(sysinfo::ProcessesToUpdate::All, true);
    match target {
        Value::Integer(_) | Value::Number(_) => {
            let pid = match target {
                Value::Integer(n) => *n as u32,
                Value::Number(n) => *n as u32,
                _ => unreachable!(),
            };
            let Some(p) = system.process(sysinfo::Pid::from_u32(pid)) else {
                return Ok((pid.to_string(), Vec::new()));
            };
            let name = process_key(&p.name().to_string_lossy());
            if PROTECTED.contains(&name.as_str()) || pid == std::process::id() {
                return Err(err(function, format!("{name} is part of Windows; LocalFlow won't change it")));
            }
            Ok((name, vec![pid]))
        }
        Value::String(name) => {
            let wanted = process_key(&name.to_str()?);
            if PROTECTED.contains(&wanted.as_str()) {
                return Err(err(function, format!("{wanted} is part of Windows; LocalFlow won't change it")));
            }
            let pids = system
                .processes()
                .values()
                .filter(|p| process_key(&p.name().to_string_lossy()) == wanted && p.pid().as_u32() != std::process::id())
                .map(|p| p.pid().as_u32())
                .collect();
            Ok((wanted, pids))
        }
        _ => Err(err(function, "give a program name like \"Discord\" or a process id")),
    }
}

#[cfg(windows)]
mod win {
    use windows_sys::Win32::{
        Foundation::{CloseHandle, HANDLE},
        System::Threading::{
            GetPriorityClass, GetProcessInformation, OpenProcess, ProcessPowerThrottling, SetPriorityClass,
            SetProcessInformation, ABOVE_NORMAL_PRIORITY_CLASS, BELOW_NORMAL_PRIORITY_CLASS, HIGH_PRIORITY_CLASS,
            IDLE_PRIORITY_CLASS, NORMAL_PRIORITY_CLASS, PROCESS_POWER_THROTTLING_CURRENT_VERSION,
            PROCESS_POWER_THROTTLING_EXECUTION_SPEED, PROCESS_POWER_THROTTLING_STATE, PROCESS_QUERY_LIMITED_INFORMATION,
            PROCESS_SET_INFORMATION,
        },
    };

    struct Handle(HANDLE);
    impl Drop for Handle {
        fn drop(&mut self) {
            // SAFETY: the handle came from OpenProcess and is closed once.
            unsafe { CloseHandle(self.0) };
        }
    }

    fn open(pid: u32, write: bool) -> Option<Handle> {
        let access = if write { PROCESS_SET_INFORMATION | PROCESS_QUERY_LIMITED_INFORMATION } else { PROCESS_QUERY_LIMITED_INFORMATION };
        // SAFETY: plain OpenProcess; a null handle means no access.
        let h = unsafe { OpenProcess(access, 0, pid) };
        (!h.is_null()).then_some(Handle(h))
    }

    /// Priority classes processes had before LocalFlow put them into efficiency mode, by pid,
    /// so switching it off gives each process its own priority back (apps such as browsers
    /// set different priorities for their own processes).
    /// Keyed by pid and the process's start time, because Windows reuses pids.
    static BEFORE: std::sync::Mutex<Option<std::collections::HashMap<(u32, u64), u32>>> = std::sync::Mutex::new(None);

    /// When the process started (a FILETIME), to tell a reused pid from the original process.
    fn started(h: HANDLE) -> u64 {
        use windows_sys::Win32::{Foundation::FILETIME, System::Threading::GetProcessTimes};
        let zero = FILETIME { dwLowDateTime: 0, dwHighDateTime: 0 };
        let (mut created, mut exited, mut kernel, mut user) = (zero, zero, zero, zero);
        // SAFETY: valid handle with query rights; all four outputs are valid FILETIMEs.
        if unsafe { GetProcessTimes(h, &mut created, &mut exited, &mut kernel, &mut user) } == 0 {
            return 0;
        }
        ((created.dwHighDateTime as u64) << 32) | created.dwLowDateTime as u64
    }

    /// Returns false when Windows refused (usually a program running as administrator).
    pub fn set_efficiency(pid: u32, on: bool) -> bool {
        let Some(h) = open(pid, true) else { return false };
        let state = PROCESS_POWER_THROTTLING_STATE {
            Version: PROCESS_POWER_THROTTLING_CURRENT_VERSION,
            // On: throttle. Off: no control bits, which hands the decision back to Windows and
            // the app (instead of forcing "never throttle", which would override apps that
            // manage this themselves).
            ControlMask: if on { PROCESS_POWER_THROTTLING_EXECUTION_SPEED } else { 0 },
            StateMask: if on { PROCESS_POWER_THROTTLING_EXECUTION_SPEED } else { 0 },
        };
        // SAFETY: the struct and its size match what ProcessPowerThrottling expects.
        let throttled = unsafe {
            SetProcessInformation(h.0, ProcessPowerThrottling, &state as *const _ as *const _, std::mem::size_of::<PROCESS_POWER_THROTTLING_STATE>() as u32)
        } != 0;
        let mut before = BEFORE.lock().unwrap_or_else(|e| e.into_inner());
        let before = before.get_or_insert_with(Default::default);
        // SAFETY: valid handle with query rights.
        let current = unsafe { GetPriorityClass(h.0) };
        let key = (pid, started(h.0));
        // Forget processes that have ended, so the list doesn't grow forever.
        if before.len() > 256 {
            before.retain(|(p, s), _| open(*p, false).is_some_and(|h2| started(h2.0) == *s));
        }
        let class = if on {
            if current != 0 && current != IDLE_PRIORITY_CLASS {
                before.entry(key).or_insert(current);
            }
            IDLE_PRIORITY_CLASS
        } else {
            match before.remove(&key) {
                Some(original) => original,
                // Not ours: only undo the lowest priority, leave anything else as the app set it.
                None if current == IDLE_PRIORITY_CLASS => NORMAL_PRIORITY_CLASS,
                None => return throttled,
            }
        };
        // SAFETY: valid handle with PROCESS_SET_INFORMATION.
        let prioritised = unsafe { SetPriorityClass(h.0, class) } != 0;
        throttled && prioritised
    }

    pub fn efficiency(pid: u32) -> Option<bool> {
        let h = open(pid, false)?;
        let mut state = PROCESS_POWER_THROTTLING_STATE { Version: PROCESS_POWER_THROTTLING_CURRENT_VERSION, ControlMask: 0, StateMask: 0 };
        // SAFETY: as above, reading into a correctly sized struct.
        let ok = unsafe {
            GetProcessInformation(h.0, ProcessPowerThrottling, &mut state as *mut _ as *mut _, std::mem::size_of::<PROCESS_POWER_THROTTLING_STATE>() as u32)
        } != 0;
        // SAFETY: valid handle with query rights.
        let class = unsafe { GetPriorityClass(h.0) };
        Some(ok && state.StateMask & PROCESS_POWER_THROTTLING_EXECUTION_SPEED != 0 && class == IDLE_PRIORITY_CLASS)
    }

    fn class_of(level: &str) -> u32 {
        match level {
            "low" => IDLE_PRIORITY_CLASS,
            "below_normal" => BELOW_NORMAL_PRIORITY_CLASS,
            "above_normal" => ABOVE_NORMAL_PRIORITY_CLASS,
            "high" => HIGH_PRIORITY_CLASS,
            _ => NORMAL_PRIORITY_CLASS,
        }
    }

    pub fn set_priority(pid: u32, level: &str) -> bool {
        let Some(h) = open(pid, true) else { return false };
        // SAFETY: valid handle with PROCESS_SET_INFORMATION.
        unsafe { SetPriorityClass(h.0, class_of(level)) != 0 }
    }

    pub fn priority(pid: u32) -> Option<&'static str> {
        let h = open(pid, false)?;
        // SAFETY: valid handle with query rights.
        let class = unsafe { GetPriorityClass(h.0) };
        Some(match class {
            IDLE_PRIORITY_CLASS => "low",
            BELOW_NORMAL_PRIORITY_CLASS => "below_normal",
            ABOVE_NORMAL_PRIORITY_CLASS => "above_normal",
            HIGH_PRIORITY_CLASS => "high",
            0 => return None,
            // Realtime and anything else is reported as it is closest to.
            NORMAL_PRIORITY_CLASS => "normal",
            _ => "high",
        })
    }
}

#[cfg(not(windows))]
mod win {
    /// macOS: priority through `renice`. Lowering works for your own apps; raising needs an
    /// administrator. Efficiency mode (EcoQoS) is Windows-only.
    fn nice_of(level: &str) -> i32 {
        match level {
            "low" => 20,
            "below_normal" => 10,
            "above_normal" => -5,
            "high" => -10,
            _ => 0,
        }
    }
    pub fn set_efficiency(_pid: u32, _on: bool) -> bool {
        false
    }
    pub fn efficiency(_pid: u32) -> Option<bool> {
        None
    }
    pub fn set_priority(pid: u32, level: &str) -> bool {
        std::process::Command::new("renice")
            .args(["-n", &nice_of(level).to_string(), "-p", &pid.to_string()])
            .status()
            .is_ok_and(|s| s.success())
    }
    pub fn priority(pid: u32) -> Option<&'static str> {
        let out = std::process::Command::new("ps").args(["-o", "nice=", "-p", &pid.to_string()]).output().ok()?;
        let nice: i32 = String::from_utf8_lossy(&out.stdout).trim().parse().ok()?;
        Some(match nice {
            n if n >= 15 => "low",
            n if n > 0 => "below_normal",
            0 => "normal",
            n if n > -10 => "above_normal",
            _ => "high",
        })
    }
}

fn parse_priority(function: &str, level: &str) -> mlua::Result<&'static str> {
    let key = level.trim().to_lowercase().replace([' ', '-'], "_");
    let key = match key.as_str() {
        "idle" | "lowest" | "efficiency" => "low",
        "realtime" => return Err(err(function, "\"realtime\" can freeze the PC, so LocalFlow doesn't offer it; use \"high\"")),
        other => other,
    };
    PRIORITIES
        .iter()
        .find(|p| **p == key)
        .copied()
        .ok_or_else(|| err(function, format!("\"{level}\" isn't a priority; use one of {}", PRIORITIES.join(", "))))
}

/// Outcome text when nothing could be changed.
fn nothing_changed(function: &str, what: &str, found: usize) -> mlua::Error {
    if found == 0 {
        err(function, format!("{what} isn't running"))
    } else {
        err(function, format!("Windows didn't allow changing {what} (it may run as administrator; LocalFlow would need to run as administrator too)"))
    }
}

pub fn register(lua: &Lua, allowed: bool) -> mlua::Result<()> {
    let process: Table = lua.globals().get("process")?;

    process.set(
        "set_efficiency",
        lua.create_function(move |_, (target, on): (Value, Option<bool>)| {
            const F: &str = "process.set_efficiency";
            require(allowed, F)?;
            if !cfg!(windows) {
                return Err(err(F, "efficiency mode is a Windows 11 feature; on a Mac use process.set_priority(name, \"low\")"));
            }
            let (what, pids) = targets(F, &target)?;
            let changed = pids.iter().filter(|pid| win::set_efficiency(**pid, on.unwrap_or(true))).count();
            if changed == 0 {
                return Err(nothing_changed(F, &what, pids.len()));
            }
            Ok(changed)
        })?,
    )?;

    process.set(
        "efficiency",
        lua.create_function(|_, target: Value| {
            let (_, pids) = targets("process.efficiency", &target)?;
            let states: Vec<bool> = pids.iter().filter_map(|pid| win::efficiency(*pid)).collect();
            // On when every process we could look at is in efficiency mode.
            Ok(if states.is_empty() { None } else { Some(states.iter().all(|s| *s)) })
        })?,
    )?;

    process.set(
        "set_priority",
        lua.create_function(move |_, (target, level): (Value, String)| {
            const F: &str = "process.set_priority";
            require(allowed, F)?;
            let level = parse_priority(F, &level)?;
            let (what, pids) = targets(F, &target)?;
            let changed = pids.iter().filter(|pid| win::set_priority(**pid, level)).count();
            if changed == 0 {
                return Err(nothing_changed(F, &what, pids.len()));
            }
            Ok(changed)
        })?,
    )?;

    process.set(
        "priority",
        lua.create_function(|_, target: Value| {
            let (_, pids) = targets("process.priority", &target)?;
            Ok(pids.iter().find_map(|pid| win::priority(*pid)))
        })?,
    )?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn priorities_are_checked() {
        assert_eq!(parse_priority("f", "Below Normal").unwrap(), "below_normal");
        assert_eq!(parse_priority("f", "idle").unwrap(), "low");
        assert!(parse_priority("f", "realtime").unwrap_err().to_string().contains("freeze"));
        assert!(parse_priority("f", "fast").is_err());
    }

    /// Efficiency mode on a child process we start ourselves, then back off.
    #[cfg(windows)]
    #[test]
    fn efficiency_mode_on_our_own_child() {
        let mut child = std::process::Command::new("cmd").args(["/c", "ping -n 30 127.0.0.1 >nul"]).spawn().unwrap();
        let pid = child.id();
        // Build machines may run everything at a lower priority: compare with what it started at.
        let original = win::priority(pid);
        assert_eq!(win::efficiency(pid), Some(false));
        assert!(win::set_efficiency(pid, true));
        assert_eq!(win::efficiency(pid), Some(true));
        assert_eq!(win::priority(pid), Some("low"));
        assert!(win::set_efficiency(pid, false));
        assert_eq!(win::efficiency(pid), Some(false));
        assert_eq!(win::priority(pid), original);
        // A priority the process had before comes back, not "normal".
        assert!(win::set_priority(pid, "above_normal"));
        assert!(win::set_efficiency(pid, true));
        assert!(win::set_efficiency(pid, false));
        assert_eq!(win::priority(pid), Some("above_normal"));
        assert!(win::set_priority(pid, "below_normal"));
        assert_eq!(win::priority(pid), Some("below_normal"));
        let _ = child.kill();
    }
}
