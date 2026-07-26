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
//! time.parse("2026-09-29 14:05")  -- text as a timestamp (nil if it isn't a date); optional format
//! time.make{ year = 2026, month = 9, day = 29, hour = 7 }  -- parts as a timestamp
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

use chrono::{DateTime, Datelike, Local, TimeZone, Timelike};
use mlua::{Lua, Table, Value};
use sysinfo::{ProcessesToUpdate, System};

fn err(function: &str, error: impl std::fmt::Display) -> mlua::Error {
    mlua::Error::runtime(format!("{function}: {error}"))
}

pub fn register(lua: &Lua, policy: std::sync::Arc<super::sandbox::PathPolicy>, deadline: Instant) -> mlua::Result<()> {
    let globals = lua.globals();
    globals.set("app", app_table(lua)?)?;
    globals.set("time", time_table(lua)?)?;
    globals.set("system", system_table(lua)?)?;
    globals.set("clipboard", clipboard_table(lua)?)?;
    globals.set("sound", sound_table(lua, policy)?)?;

    globals.set(
        "ask",
        lua.create_function(|_, (question, title): (String, Option<String>)| {
            ask(&question, title.as_deref().unwrap_or("LocalFlow")).map_err(|e| err("ask", e))
        })?,
    )?;

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
    // Links like https://, mailto: or ms-settings:display (but not C:\...).
    let scheme = lower.split(':').next().unwrap_or("");
    let is_link = lower.contains(':')
        && scheme.len() > 1
        && scheme.chars().all(|c| c.is_ascii_alphanumeric() || "+.-".contains(c));
    if is_link {
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

pub fn expand_home(raw: &str) -> PathBuf {
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

// ---- system information ----------------------------------------------------------

fn system_table(lua: &Lua) -> mlua::Result<Table> {
    use sysinfo::{Disks, MemoryRefreshKind, RefreshKind};

    let system = lua.create_table()?;

    system.set("computer_name", lua.create_function(|_, ()| Ok(System::host_name().unwrap_or_default()))?)?;
    system.set(
        "user_name",
        lua.create_function(|_, ()| {
            Ok(std::env::var("USERNAME").or_else(|_| std::env::var("USER")).unwrap_or_default())
        })?,
    )?;
    system.set(
        "os",
        lua.create_function(|_, ()| {
            Ok(System::long_os_version().or_else(System::name).unwrap_or_else(|| std::env::consts::OS.to_string()))
        })?,
    )?;
    system.set("uptime", lua.create_function(|_, ()| Ok(System::uptime()))?)?;

    system.set(
        "memory",
        lua.create_function(|lua, ()| {
            let sys = System::new_with_specifics(RefreshKind::nothing().with_memory(MemoryRefreshKind::everything()));
            let table = lua.create_table()?;
            table.set("total", sys.total_memory())?;
            table.set("used", sys.used_memory())?;
            table.set("free", sys.available_memory())?;
            Ok(table)
        })?,
    )?;

    system.set(
        "cpu",
        lua.create_function(|_, ()| {
            // CPU usage is measured over a short interval.
            let mut sys = System::new();
            sys.refresh_cpu_usage();
            std::thread::sleep(sysinfo::MINIMUM_CPU_UPDATE_INTERVAL.max(Duration::from_millis(200)));
            sys.refresh_cpu_usage();
            Ok((sys.global_cpu_usage() * 10.0).round() / 10.0)
        })?,
    )?;

    system.set(
        "disks",
        lua.create_function(|lua, ()| {
            let disks = Disks::new_with_refreshed_list();
            let list = lua.create_table()?;
            for disk in disks.list() {
                let entry = lua.create_table()?;
                entry.set("name", disk.name().to_string_lossy().into_owned())?;
                entry.set("mount", disk.mount_point().to_string_lossy().into_owned())?;
                entry.set("total", disk.total_space())?;
                entry.set("free", disk.available_space())?;
                entry.set("removable", disk.is_removable())?;
                list.push(entry)?;
            }
            Ok(list)
        })?,
    )?;

    system.set(
        "disk_free",
        lua.create_function(|_, path: Option<String>| {
            let target = expand_home(path.as_deref().unwrap_or("~")).to_string_lossy().to_lowercase().replace('/', "\\");
            let disks = Disks::new_with_refreshed_list();
            // The disk whose mount point is the longest prefix of the path.
            let best = disks
                .list()
                .iter()
                .filter(|d| {
                    let mount = d.mount_point().to_string_lossy().to_lowercase().replace('/', "\\");
                    target.starts_with(&mount)
                })
                .max_by_key(|d| d.mount_point().as_os_str().len());
            Ok(best.map(|d| d.available_space()))
        })?,
    )?;

    system.set(
        "battery",
        lua.create_function(|lua, ()| match battery_status() {
            None => Ok(Value::Nil),
            Some((percent, charging, plugged_in)) => {
                let table = lua.create_table()?;
                table.set("percent", percent)?;
                table.set("charging", charging)?;
                table.set("plugged_in", plugged_in)?;
                Ok(Value::Table(table))
            }
        })?,
    )?;

    Ok(system)
}

/// `(percent, charging, plugged_in)`, or `None` on a PC without a battery.
#[cfg(windows)]
pub(crate) fn battery_status() -> Option<(u8, bool, bool)> {
    use windows_sys::Win32::System::Power::{GetSystemPowerStatus, SYSTEM_POWER_STATUS};
    let mut status: SYSTEM_POWER_STATUS = unsafe { std::mem::zeroed() };
    // SAFETY: `status` is a valid, writable SYSTEM_POWER_STATUS.
    if unsafe { GetSystemPowerStatus(&mut status) } == 0 {
        return None;
    }
    // 128 = no system battery, 255 = unknown.
    if status.BatteryFlag & 128 != 0 || status.BatteryLifePercent == 255 {
        return None;
    }
    Some((status.BatteryLifePercent, status.BatteryFlag & 8 != 0, status.ACLineStatus == 1))
}

#[cfg(not(windows))]
pub(crate) fn battery_status() -> Option<(u8, bool, bool)> {
    None
}

// ---- clipboard, dialogs, sound ---------------------------------------------------

fn clipboard_table(lua: &Lua) -> mlua::Result<Table> {
    let clipboard = lua.create_table()?;
    clipboard.set(
        "get",
        lua.create_function(|_, ()| {
            let mut board = arboard::Clipboard::new().map_err(|e| err("clipboard.get", e))?;
            Ok(board.get_text().ok())
        })?,
    )?;
    clipboard.set(
        "set",
        lua.create_function(|_, text: String| {
            let mut board = arboard::Clipboard::new().map_err(|e| err("clipboard.set", e))?;
            board.set_text(text).map_err(|e| err("clipboard.set", e))
        })?,
    )?;
    Ok(clipboard)
}

/// Show a Yes/No question and wait for the answer.
#[cfg(windows)]
fn ask(question: &str, title: &str) -> Result<bool, String> {
    let answer = rfd::MessageDialog::new()
        .set_title(title)
        .set_description(question)
        .set_level(rfd::MessageLevel::Info)
        .set_buttons(rfd::MessageButtons::YesNo)
        .show();
    Ok(answer == rfd::MessageDialogResult::Yes)
}

#[cfg(not(windows))]
fn ask(_question: &str, _title: &str) -> Result<bool, String> {
    Err("dialogs are only available on Windows".into())
}

fn sound_table(lua: &Lua, policy: std::sync::Arc<super::sandbox::PathPolicy>) -> mlua::Result<Table> {
    let sound = lua.create_table()?;
    sound.set("beep", lua.create_function(|_, ()| beep().map_err(|e| err("sound.beep", e)))?)?;
    sound.set(
        "play",
        lua.create_function(move |_, path: String| {
            let resolved = policy.resolve(&path).map_err(|e| err("sound.play", e))?;
            if !resolved.is_file() {
                return Err(err("sound.play", format!("file not found: {path}")));
            }
            let is_wav = resolved.extension().is_some_and(|e| e.eq_ignore_ascii_case("wav"));
            if !is_wav {
                return Err(err("sound.play", "only .wav files can be played"));
            }
            play_wav(&resolved).map_err(|e| err("sound.play", e))
        })?,
    )?;
    Ok(sound)
}

#[cfg(windows)]
fn beep() -> Result<(), String> {
    use windows_sys::Win32::{System::Diagnostics::Debug::MessageBeep, UI::WindowsAndMessaging::MB_OK};
    // SAFETY: MessageBeep takes a plain flag and has no memory requirements.
    unsafe { MessageBeep(MB_OK) };
    Ok(())
}

#[cfg(not(windows))]
fn beep() -> Result<(), String> {
    Err("sound is only available on Windows".into())
}

/// Starts playing and returns immediately; the sound keeps playing in the background.
#[cfg(windows)]
fn play_wav(path: &Path) -> Result<(), String> {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::Media::Audio::{PlaySoundW, SND_ASYNC, SND_FILENAME, SND_NODEFAULT};
    let wide: Vec<u16> = path.as_os_str().encode_wide().chain(std::iter::once(0)).collect();
    // SAFETY: `wide` is a NUL-terminated UTF-16 path that outlives the call.
    let ok = unsafe { PlaySoundW(wide.as_ptr(), std::ptr::null_mut(), SND_FILENAME | SND_ASYNC | SND_NODEFAULT) };
    if ok == 0 {
        Err("could not play this file".into())
    } else {
        Ok(())
    }
}

#[cfg(not(windows))]
fn play_wav(_path: &Path) -> Result<(), String> {
    Err("sound is only available on Windows".into())
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
    time.set(
        "parse",
        lua.create_function(|_, (text, format): (String, Option<String>)| {
            let text = text.trim();
            let with_format = |f: &str| {
                chrono::NaiveDateTime::parse_from_str(text, f)
                    .ok()
                    .or_else(|| chrono::NaiveDate::parse_from_str(text, f).ok().map(|d| d.and_time(chrono::NaiveTime::MIN)))
            };
            let parsed = match format.as_deref() {
                Some(format) => with_format(format),
                None => ["%Y-%m-%d %H:%M:%S", "%Y-%m-%d %H:%M", "%Y-%m-%dT%H:%M:%S", "%Y-%m-%d"].iter().find_map(|f| with_format(f)),
            };
            // Text that isn't a date gives nil, so scripts can check.
            Ok(parsed.and_then(|n| Local.from_local_datetime(&n).earliest()).map(|t| t.timestamp()))
        })?,
    )?;
    time.set(
        "make",
        lua.create_function(|_, parts: Table| {
            // Values may overflow (day 35, hour -1): they roll over like a calendar.
            let get = |key: &str, default: i64| -> mlua::Result<i64> { Ok(parts.get::<Option<i64>>(key)?.unwrap_or(default)) };
            let year: i64 = parts.get("year")?;
            let months = year * 12 + get("month", 1)? - 1;
            let first = i32::try_from(months.div_euclid(12))
                .ok()
                .and_then(|y| chrono::NaiveDate::from_ymd_opt(y, months.rem_euclid(12) as u32 + 1, 1))
                .ok_or_else(|| err("time.make", "that year is out of range"))?;
            let offset = chrono::TimeDelta::try_days(get("day", 1)? - 1)
                .zip(chrono::TimeDelta::try_hours(get("hour", 0)?))
                .zip(chrono::TimeDelta::try_minutes(get("min", 0)?))
                .zip(chrono::TimeDelta::try_seconds(get("sec", 0)?))
                .map(|(((d, h), m), s)| d + h + m + s)
                .ok_or_else(|| err("time.make", "the numbers are too large"))?;
            let naive = first
                .and_time(chrono::NaiveTime::MIN)
                .checked_add_signed(offset)
                .ok_or_else(|| err("time.make", "that date is out of range"))?;
            // A time skipped by a clock change moves forward an hour.
            let local = Local
                .from_local_datetime(&naive)
                .earliest()
                .or_else(|| Local.from_local_datetime(&(naive + chrono::TimeDelta::hours(1))).earliest());
            Ok(local.map(|t| t.timestamp()))
        })?,
    )?;
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
