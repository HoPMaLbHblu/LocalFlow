//! macOS versions of the functions that talk to the operating system: dialogs,
//! sound, battery, idle time, power, volume, wallpaper, wake timers and windows.
//!
//! They use the tools every Mac has (`osascript`, `pmset`, `ioreg`, `afplay`).
//! Controlling other apps' windows needs LocalFlow to be allowed under
//! System Settings › Privacy & Security › Accessibility.

// Parsers are plain functions, so they're compiled (and tested) everywhere.
#![cfg_attr(not(target_os = "macos"), allow(dead_code))]

use std::{
    process::{Child, Command, Stdio},
    sync::Mutex,
};

const ACCESSIBILITY_HINT: &str = "allow LocalFlow under System Settings › Privacy & Security › Accessibility, then try again";

fn run(program: &str, args: &[&str]) -> Result<String, String> {
    let output = Command::new(program)
        .args(args)
        .stdin(Stdio::null())
        .output()
        .map_err(|e| format!("could not start {program}: {e}"))?;
    if output.status.success() {
        Ok(String::from_utf8_lossy(&output.stdout).trim_end().to_string())
    } else {
        Err(String::from_utf8_lossy(&output.stderr).trim().to_string())
    }
}

/// Run AppleScript and return what it printed.
pub fn applescript(script: &str) -> Result<String, String> {
    run("/usr/bin/osascript", &["-e", script])
}

/// Run JavaScript for Automation (JXA) and return what it printed.
fn jxa(script: &str) -> Result<String, String> {
    run("/usr/bin/osascript", &["-l", "JavaScript", "-e", script]).map_err(|e| {
        if e.contains("-1719") || e.contains("-25211") || e.to_lowercase().contains("assistive") {
            format!("macOS blocked this: {ACCESSIBILITY_HINT}")
        } else {
            e
        }
    })
}

/// Text as an AppleScript string literal.
fn quoted(text: &str) -> String {
    format!("\"{}\"", text.replace('\\', "\\\\").replace('"', "\\\""))
}

// ---- dialogs and sound ----------------------------------------------------------------

pub fn ask(question: &str, title: &str) -> Result<bool, String> {
    let script = format!(
        "display dialog {} with title {} buttons {{\"No\", \"Yes\"}} default button \"Yes\"",
        quoted(question),
        quoted(title)
    );
    match applescript(&script) {
        Ok(answer) => Ok(answer.contains("Yes")),
        // -128 is "user cancelled", e.g. Esc.
        Err(e) if e.contains("-128") => Ok(false),
        Err(e) => Err(e),
    }
}

pub fn beep() -> Result<(), String> {
    applescript("beep").map(|_| ())
}

/// Starts playing and returns immediately.
pub fn play(path: &std::path::Path) -> Result<(), String> {
    Command::new("/usr/bin/afplay")
        .arg(path)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map(|_| ())
        .map_err(|e| format!("could not play this file: {e}"))
}

// ---- battery and idle time --------------------------------------------------------------

/// `(percent, charging, plugged_in)` from `pmset -g batt`, or `None` without a battery.
pub fn battery() -> Option<(u8, bool, bool)> {
    parse_battery(&run("/usr/bin/pmset", &["-g", "batt"]).ok()?)
}

pub fn parse_battery(text: &str) -> Option<(u8, bool, bool)> {
    let line = text.lines().find(|l| l.contains("InternalBattery"))?;
    let percent = line.split('\t').nth(1)?.split('%').next()?.trim().parse().ok()?;
    let plugged_in = text.contains("'AC Power'");
    let charging = line.contains("; charging") || line.contains("; charged") && plugged_in;
    Some((percent, charging, plugged_in))
}

/// Seconds since the last keyboard or mouse input.
pub fn idle_seconds() -> u64 {
    run("/usr/sbin/ioreg", &["-c", "IOHIDSystem", "-d", "4"])
        .ok()
        .and_then(|text| parse_idle(&text))
        .unwrap_or(0)
}

pub fn parse_idle(text: &str) -> Option<u64> {
    let line = text.lines().find(|l| l.contains("\"HIDIdleTime\""))?;
    let nanos: u64 = line.rsplit('=').next()?.trim().parse().ok()?;
    Some(nanos / 1_000_000_000)
}

// ---- power, sound, display ---------------------------------------------------------------

pub fn lock() -> Result<(), String> {
    // Ctrl+Cmd+Q is the Lock Screen shortcut.
    applescript("tell application \"System Events\" to keystroke \"q\" using {control down, command down}")
        .map(|_| ())
        .map_err(|e| format!("{e} ({ACCESSIBILITY_HINT})"))
}

pub fn sleep() -> Result<(), String> {
    run("/usr/bin/pmset", &["sleepnow"]).map(|_| ())
}

/// A pending shutdown or restart, so it can be cancelled.
static PENDING: Mutex<Option<Child>> = Mutex::new(None);

/// Shut down or restart after `delay` seconds (cancel with [`cancel_shutdown`]).
pub fn shutdown(delay: u32, restart: bool) -> Result<(), String> {
    cancel_shutdown();
    let action = if restart { "restart" } else { "shut down" };
    let child = Command::new("/bin/sh")
        .args(["-c", &format!("sleep {delay} && osascript -e 'tell application \"System Events\" to {action}'")])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|e| e.to_string())?;
    *PENDING.lock().unwrap_or_else(|e| e.into_inner()) = Some(child);
    Ok(())
}

/// True if a pending shutdown or restart was stopped.
pub fn cancel_shutdown() -> bool {
    match PENDING.lock().unwrap_or_else(|e| e.into_inner()).take() {
        Some(mut child) => {
            let still_waiting = matches!(child.try_wait(), Ok(None));
            let _ = child.kill();
            let _ = child.wait();
            still_waiting
        }
        None => false,
    }
}

/// Change the volume by `delta` percent (-100..100).
pub fn change_volume(delta: i32) -> Result<(), String> {
    applescript(&format!(
        "set current to output volume of (get volume settings)\nset volume output volume (current + ({delta}))"
    ))
    .map(|_| ())
}

pub fn toggle_mute() -> Result<(), String> {
    applescript("set volume output muted (not (output muted of (get volume settings)))").map(|_| ())
}

pub fn set_wallpaper(path: &std::path::Path) -> Result<(), String> {
    applescript(&format!(
        "tell application \"System Events\" to tell every desktop to set picture to {}",
        quoted(&path.to_string_lossy())
    ))
    .map(|_| ())
}

/// Wake the Mac from sleep at a time. macOS asks for an administrator password.
pub fn wake_at(when: &chrono::DateTime<chrono::Local>) -> Result<(), String> {
    let command = format!("pmset schedule wake \\\"{}\\\"", when.format("%m/%d/%y %H:%M:%S"));
    applescript(&format!("do shell script \"{command}\" with administrator privileges"))
        .map(|_| ())
        .map_err(|e| if e.contains("-128") { "cancelled".into() } else { e })
}

pub fn cancel_wake() -> Result<(), String> {
    applescript("do shell script \"pmset schedule cancelall\" with administrator privileges")
        .map(|_| ())
        .map_err(|e| if e.contains("-128") { "cancelled".into() } else { e })
}

// ---- apps ------------------------------------------------------------------------

/// Folders holding installed apps (`*.app`).
pub fn app_dirs() -> Vec<std::path::PathBuf> {
    let mut dirs = vec![
        "/Applications".into(),
        "/Applications/Utilities".into(),
        "/System/Applications".into(),
        "/System/Applications/Utilities".into(),
    ];
    if let Some(home) = dirs::home_dir() {
        dirs.push(home.join("Applications"));
    }
    dirs
}

// ---- windows ---------------------------------------------------------------------

/// One window, as `(pid, index, app, title, x, y, width, height, minimized)`.
pub type RawWindow = (u32, u32, String, String, i32, i32, i32, i32, bool);

/// Every window of every visible app (needs Accessibility permission).
pub fn windows() -> Result<Vec<RawWindow>, String> {
    let script = r#"
const se = Application("System Events");
const out = [];
for (const p of se.applicationProcesses.whose({ visible: true })()) {
  let wins = [];
  try { wins = p.windows(); } catch (e) { continue; }
  wins.forEach((w, i) => {
    try {
      const pos = w.position(), size = w.size();
      let min = false;
      try { min = w.attributes.byName("AXMinimized").value(); } catch (e) {}
      out.push([p.unixId(), i + 1, p.name(), (w.name() || "").replace(/[\t\n]/g, " "), pos[0], pos[1], size[0], size[1], min].join("\t"));
    } catch (e) {}
  });
}
out.join("\n");
"#;
    Ok(jxa(script)?.lines().filter_map(parse_window).collect())
}

pub fn parse_window(line: &str) -> Option<RawWindow> {
    let f: Vec<&str> = line.split('\t').collect();
    if f.len() != 9 {
        return None;
    }
    Some((
        f[0].parse().ok()?,
        f[1].parse().ok()?,
        f[2].to_string(),
        f[3].to_string(),
        f[4].parse::<f64>().ok()? as i32,
        f[5].parse::<f64>().ok()? as i32,
        f[6].parse::<f64>().ok()? as i32,
        f[7].parse::<f64>().ok()? as i32,
        f[8] == "true",
    ))
}

/// The pid of the app in front.
pub fn frontmost_pid() -> Option<u32> {
    jxa(r#"Application("System Events").applicationProcesses.whose({ frontmost: true })()[0].unixId()"#)
        .ok()?
        .trim()
        .parse()
        .ok()
}

fn window_script(pid: u32, index: u32, body: &str) -> Result<(), String> {
    let script = format!(
        r#"
const se = Application("System Events");
const p = se.applicationProcesses.whose({{ unixId: {pid} }})()[0];
if (!p) throw new Error("that window is gone");
const w = p.windows[{i}];
{body}
"ok";
"#,
        i = index.saturating_sub(1)
    );
    jxa(&script).map(|_| ())
}

pub fn window_action(pid: u32, index: u32, action: &str) -> Result<(), String> {
    let body = match action {
        "focus" => "p.frontmost = true; w.actions.byName(\"AXRaise\").perform();",
        "minimize" => "w.attributes.byName(\"AXMinimized\").value = true;",
        "restore" => "w.attributes.byName(\"AXMinimized\").value = false; p.frontmost = true; w.actions.byName(\"AXRaise\").perform();",
        "maximize" => {
            "const b = Application(\"Finder\").desktop.window.bounds(); \
             w.position = [b.x, b.y + 25]; w.size = [b.width, b.height - 25];"
        }
        "close" => "w.buttons.whose({ subrole: \"AXCloseButton\" })()[0].click();",
        _ => return Err(format!("unknown window action {action}")),
    };
    window_script(pid, index, body)
}

pub fn move_window(pid: u32, index: u32, x: i32, y: i32, width: i32, height: i32) -> Result<(), String> {
    window_script(pid, index, &format!("w.position = [{x}, {y}]; w.size = [{width}, {height}];"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn battery_output_is_parsed() {
        let text = "Now drawing from 'AC Power'\n -InternalBattery-0 (id=1234)\t87%; charging; 0:42 remaining present: true";
        assert_eq!(parse_battery(text), Some((87, true, true)));
        let text = "Now drawing from 'Battery Power'\n -InternalBattery-0 (id=1234)\t15%; discharging; 1:02 remaining";
        assert_eq!(parse_battery(text), Some((15, false, false)));
        assert_eq!(parse_battery("Now drawing from 'AC Power'"), None, "desktop Macs have no battery");
    }

    #[test]
    fn idle_time_is_parsed() {
        let text = "    | |   \"HIDIdleTime\" = 12500000000\n";
        assert_eq!(parse_idle(text), Some(12));
    }

    #[test]
    fn window_lines_are_parsed() {
        assert_eq!(
            parse_window("501\t1\tSafari\tApple\t0\t25\t1440\t875\tfalse"),
            Some((501, 1, "Safari".into(), "Apple".into(), 0, 25, 1440, 875, false))
        );
        assert_eq!(parse_window("broken"), None);
    }
}
