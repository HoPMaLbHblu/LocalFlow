//! Lua functions that reach beyond files: opening apps, dates and times, waiting.
//!
//! ```lua
//! app.open("Spotify")            -- an app by its Start-menu name, a file, a folder, a URL or an .exe
//! app.open("C:/Tools/tool.exe", {"--flag"})
//! app.running("Discord")         -- true if a process with that name is running
//! app.list()                     -- names of running processes
//! app.shortcuts()                -- names of apps in the Start menu (what app.open understands)
//! wait(2)                        -- pause for 2 seconds
//! time.now()                     -- seconds since 1970 (a timestamp)
//! time.format("%d.%m.%Y", t)     -- a timestamp as text; both arguments optional
//! time.date(t)                   -- a timestamp as { year, month, day, hour, min, sec, weekday, yday }
//! time.today()                   -- "2026-09-29"
//! time.days(n), time.hours(n), time.minutes(n)  -- durations in seconds
//! ```
//!
//! Launching apps is deliberately not limited to the allowed folders: opening a
//! program is the point. Scripts still can't read or change files outside them.

use std::{
    collections::BTreeSet,
    fmt::Write as _,
    path::{Path, PathBuf},
    process::Command,
    time::{Duration, Instant},
};

use chrono::{DateTime, Datelike, Local, Timelike};
use mlua::{Lua, Table};
use sysinfo::{ProcessesToUpdate, System};

fn err(function: &str, error: impl std::fmt::Display) -> mlua::Error {
    mlua::Error::runtime(format!("{function}: {error}"))
}

pub fn register(lua: &Lua, deadline: Instant) -> mlua::Result<()> {
    let globals = lua.globals();
    globals.set("app", app_table(lua)?)?;
    globals.set("time", time_table(lua)?)?;

    globals.set(
        "wait",
        lua.create_function(move |_, seconds: f64| {
            if !(0.0..=3600.0).contains(&seconds) {
                return Err(err("wait", "seconds must be between 0 and 3600"));
            }
            let duration = Duration::from_secs_f64(seconds);
            if Instant::now() + duration > deadline {
                return Err(err("wait", "waiting that long would pass the script's time limit"));
            }
            std::thread::sleep(duration);
            Ok(())
        })?,
    )?;
    Ok(())
}

// ---- apps ----------------------------------------------------------------------

fn app_table(lua: &Lua) -> mlua::Result<Table> {
    let app = lua.create_table()?;

    app.set(
        "open",
        lua.create_function(|_, (target, args): (String, Option<Vec<String>>)| {
            open_target(&target, args.unwrap_or_default()).map_err(|e| err("app.open", e))
        })?,
    )?;

    app.set(
        "running",
        lua.create_function(|_, name: String| {
            let wanted = process_key(&name);
            Ok(running_processes().contains(&wanted))
        })?,
    )?;

    app.set(
        "list",
        lua.create_function(|_, ()| Ok(running_processes().into_iter().collect::<Vec<_>>()))?,
    )?;

    app.set(
        "shortcuts",
        lua.create_function(|_, ()| {
            let names: BTreeSet<String> = start_menu_shortcuts().into_iter().map(|(name, _)| name).collect();
            Ok(names.into_iter().collect::<Vec<_>>())
        })?,
    )?;

    Ok(app)
}

/// What `app.open` launched, for logging.
fn open_target(target: &str, args: Vec<String>) -> Result<String, String> {
    let target = target.trim();
    if target.is_empty() {
        return Err("nothing to open".into());
    }

    let lower = target.to_lowercase();
    if ["http://", "https://", "mailto:"].iter().any(|p| lower.starts_with(p)) {
        open::that_detached(target).map_err(|e| e.to_string())?;
        return Ok(target.to_string());
    }

    let path = expand_home(target);
    if path.exists() {
        let is_program = path.extension().is_some_and(|e| e.eq_ignore_ascii_case("exe"));
        if is_program {
            Command::new(&path).args(&args).spawn().map_err(|e| e.to_string())?;
        } else {
            open::that_detached(&path).map_err(|e| e.to_string())?;
        }
        return Ok(path.display().to_string());
    }

    if let Some(shortcut) = find_shortcut(target) {
        open::that_detached(&shortcut).map_err(|e| e.to_string())?;
        return Ok(shortcut.display().to_string());
    }

    // Finally, a program on the PATH such as "notepad" or "code".
    Command::new(target).args(&args).spawn().map_err(|_| {
        format!(
            "could not find an app called '{target}'. Use app.shortcuts() to list installed app names, \
             or give the full path to the program"
        )
    })?;
    Ok(target.to_string())
}

fn expand_home(raw: &str) -> PathBuf {
    let home = dirs::home_dir().unwrap_or_default();
    if raw == "~" {
        home
    } else if let Some(rest) = raw.strip_prefix("~/").or_else(|| raw.strip_prefix("~\\")) {
        home.join(rest)
    } else {
        PathBuf::from(raw)
    }
}

/// Folders holding Start-menu shortcuts (Windows only; empty elsewhere).
fn start_menu_dirs() -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    if let Some(program_data) = std::env::var_os("ProgramData") {
        dirs.push(PathBuf::from(program_data).join(r"Microsoft\Windows\Start Menu\Programs"));
    }
    if let Some(app_data) = std::env::var_os("APPDATA") {
        dirs.push(PathBuf::from(app_data).join(r"Microsoft\Windows\Start Menu\Programs"));
    }
    dirs
}

/// `(name, path)` for every app shortcut in the Start menu, skipping uninstallers.
pub fn start_menu_shortcuts() -> Vec<(String, PathBuf)> {
    fn walk(dir: &Path, depth: usize, out: &mut Vec<(String, PathBuf)>) {
        let Ok(entries) = std::fs::read_dir(dir) else { return };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                if depth < 4 {
                    walk(&path, depth + 1, out);
                }
                continue;
            }
            let is_shortcut = path
                .extension()
                .is_some_and(|e| e.eq_ignore_ascii_case("lnk") || e.eq_ignore_ascii_case("url"));
            let Some(name) = path.file_stem().map(|s| s.to_string_lossy().into_owned()) else { continue };
            if is_shortcut && !name.to_lowercase().contains("uninstall") {
                out.push((name, path));
            }
        }
    }

    let mut out = Vec::new();
    for dir in start_menu_dirs() {
        walk(&dir, 0, &mut out);
    }
    out
}

/// Best match for `name`: exact name first, then names starting with it, then names containing it.
fn find_shortcut(name: &str) -> Option<PathBuf> {
    let wanted = name.to_lowercase();
    let shortcuts = start_menu_shortcuts();
    let pick = |matches: &dyn Fn(&str) -> bool| {
        shortcuts
            .iter()
            .filter(|(n, _)| matches(&n.to_lowercase()))
            .min_by_key(|(n, _)| n.len())
            .map(|(_, p)| p.clone())
    };
    pick(&|n| n == wanted)
        .or_else(|| pick(&|n| n.starts_with(&wanted)))
        .or_else(|| pick(&|n| n.contains(&wanted)))
}

/// "Discord.exe" and "discord" both become "discord".
fn process_key(name: &str) -> String {
    let lower = name.trim().to_lowercase();
    lower.strip_suffix(".exe").unwrap_or(&lower).to_string()
}

fn running_processes() -> BTreeSet<String> {
    let mut system = System::new();
    system.refresh_processes(ProcessesToUpdate::All, true);
    system
        .processes()
        .values()
        .map(|p| process_key(&p.name().to_string_lossy()))
        .filter(|n| !n.is_empty())
        .collect()
}

// ---- time ----------------------------------------------------------------------

fn local_time(timestamp: Option<i64>) -> mlua::Result<DateTime<Local>> {
    match timestamp {
        None => Ok(Local::now()),
        Some(t) => DateTime::from_timestamp(t, 0)
            .map(|d| d.with_timezone(&Local))
            .ok_or_else(|| err("time", format!("{t} is not a valid timestamp"))),
    }
}

fn time_table(lua: &Lua) -> mlua::Result<Table> {
    let time = lua.create_table()?;

    time.set("now", lua.create_function(|_, ()| Ok(Local::now().timestamp()))?)?;

    time.set(
        "format",
        lua.create_function(|_, (format, timestamp): (Option<String>, Option<i64>)| {
            let format = format.unwrap_or_else(|| "%Y-%m-%d %H:%M:%S".into());
            let mut out = String::new();
            // Writing (rather than to_string) reports bad format codes instead of panicking.
            write!(out, "{}", local_time(timestamp)?.format(&format))
                .map_err(|_| err("time.format", format!("invalid format '{format}'")))?;
            Ok(out)
        })?,
    )?;

    time.set(
        "date",
        lua.create_function(|lua, timestamp: Option<i64>| {
            let t = local_time(timestamp)?;
            let table = lua.create_table()?;
            table.set("year", t.year())?;
            table.set("month", t.month())?;
            table.set("day", t.day())?;
            table.set("hour", t.hour())?;
            table.set("min", t.minute())?;
            table.set("sec", t.second())?;
            table.set("weekday", t.weekday().number_from_monday())?; // 1 = Monday ... 7 = Sunday
            table.set("yday", t.ordinal())?;
            Ok(table)
        })?,
    )?;

    time.set("today", lua.create_function(|_, ()| Ok(Local::now().format("%Y-%m-%d").to_string()))?)?;
    time.set("days", lua.create_function(|_, n: f64| Ok((n * 86_400.0) as i64))?)?;
    time.set("hours", lua.create_function(|_, n: f64| Ok((n * 3_600.0) as i64))?)?;
    time.set("minutes", lua.create_function(|_, n: f64| Ok((n * 60.0) as i64))?)?;

    Ok(time)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn process_names_are_normalised() {
        assert_eq!(process_key("Discord.EXE"), "discord");
        assert_eq!(process_key(" notepad "), "notepad");
    }

    #[test]
    fn missing_apps_give_a_helpful_error() {
        let e = open_target("definitely-not-an-app-3f9c1a", vec![]).unwrap_err();
        assert!(e.contains("app.shortcuts()"), "{e}");
    }
}
