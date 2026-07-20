//! Sharing automations as `.localflow` files.
//!
//! A `.localflow` file is plain JSON, so it can be read and reviewed anywhere:
//!
//! ```json
//! {
//!   "format": "localflow",
//!   "version": 1,
//!   "name": "Organize PDF files",
//!   "description": "...",
//!   "lua_code": "automation { ... }",
//!   "schedule": "0 0 * * * *",
//!   "run_on_startup": false,
//!   "watch_path": null,
//!   "watch_pattern": null
//! }
//! ```
//!
//! Imports always arrive disabled, together with a list of the risky things the
//! script can do, so people can look before they switch it on.

use serde::{Deserialize, Serialize};

use crate::{db::models::Automation, service::AutomationInput, triggers::ExtraTriggers};

pub const FORMAT: &str = "localflow";
pub const VERSION: u32 = 1;
pub const EXTENSION: &str = "localflow";

/// Largest file we accept, to reject obviously wrong files quickly.
const MAX_FILE_BYTES: usize = 2 * 1024 * 1024;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SharedAutomation {
    pub format: String,
    pub version: u32,
    pub name: String,
    #[serde(default)]
    pub description: String,
    pub lua_code: String,
    #[serde(default)]
    pub schedule: Option<String>,
    #[serde(default)]
    pub run_on_startup: bool,
    #[serde(default)]
    pub watch_path: Option<String>,
    #[serde(default)]
    pub watch_pattern: Option<String>,
    /// Hotkey, app, idle and USB triggers.
    #[serde(default, skip_serializing_if = "ExtraTriggers::is_empty")]
    pub triggers: ExtraTriggers,
    /// Whether the author had "Allow system control" on. Imports never get it
    /// automatically; it is only shown as a warning.
    #[serde(default)]
    pub allow_system: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub exported_at: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub app_version: Option<String>,
}

impl SharedAutomation {
    pub fn from_automation(a: &Automation) -> Self {
        SharedAutomation {
            format: FORMAT.into(),
            version: VERSION,
            name: a.name.clone(),
            description: a.description.clone(),
            lua_code: a.lua_code.clone(),
            schedule: a.schedule.clone(),
            run_on_startup: a.run_on_startup,
            watch_path: a.watch_path.clone(),
            watch_pattern: a.watch_pattern.clone(),
            // "Run after" points at an automation on this PC only, so it isn't shared.
            triggers: ExtraTriggers { after: None, ..ExtraTriggers::from_json(a.triggers.as_deref()) },
            allow_system: a.allow_system,
            exported_at: Some(crate::db::repository::now()),
            app_version: Some(env!("CARGO_PKG_VERSION").into()),
        }
    }

    pub fn to_json(&self) -> String {
        serde_json::to_string_pretty(self).expect("a SharedAutomation always serializes")
    }

    /// Parse and check a `.localflow` file's contents.
    pub fn parse(text: &str) -> Result<Self, String> {
        if text.len() > MAX_FILE_BYTES {
            return Err("This file is too large to be a LocalFlow automation.".into());
        }
        let shared: SharedAutomation =
            serde_json::from_str(text.trim_start_matches('\u{feff}')).map_err(|_| "This is not a LocalFlow automation file.".to_string())?;
        if shared.format != FORMAT {
            return Err("This is not a LocalFlow automation file.".into());
        }
        if shared.version > VERSION {
            return Err("This file was made by a newer version of LocalFlow. Please update LocalFlow to import it.".into());
        }
        Ok(shared)
    }

    /// The automation to create when importing: always disabled at first.
    pub fn to_input(&self) -> AutomationInput {
        AutomationInput {
            name: self.name.clone(),
            description: self.description.clone(),
            lua_code: self.lua_code.clone(),
            schedule: self.schedule.clone(),
            enabled: false,
            run_on_startup: self.run_on_startup,
            watch_path: self.watch_path.clone(),
            watch_pattern: self.watch_pattern.clone(),
            // Never granted by a file: the user switches it on after reading the code.
            allow_system: false,
            triggers: ExtraTriggers { after: None, ..self.triggers.clone() },
        }
    }
}

/// Things an imported script can do that deserve a look before enabling it.
/// The frontend turns these ids into translated explanations.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Risk {
    DeletesFiles,
    MovesFiles,
    WritesFiles,
    OpensApps,
    UsesInternet,
    UsesClipboard,
    RunsOnStartup,
    WatchesFolder,
    RunsOnSchedule,
    /// Runs commands or PowerShell.
    RunsCommands,
    /// Presses keys, types or clicks.
    ControlsInput,
    /// Stops programs, closes windows, locks, sleeps, shuts down or wakes the PC.
    ControlsPower,
    /// The author had "Allow system control" on.
    NeedsSystemControl,
    /// Runs by itself on a hotkey, when an app starts or closes, when idle, or on USB.
    RunsOnEvents,
}

/// Remove `--` comments so a commented-out call isn't reported.
fn strip_comments(code: &str) -> String {
    let mut out = String::new();
    for line in code.lines() {
        // Good enough for a warning: ignores "--" inside strings only in rare cases.
        let code_part = match line.find("--") {
            Some(at) => &line[..at],
            None => line,
        };
        out.push_str(code_part);
        out.push('\n');
    }
    out
}

fn calls(code: &str, function: &str) -> bool {
    let mut rest = code;
    while let Some(at) = rest.find(function) {
        let before = rest[..at].chars().last();
        let after = rest[at + function.len()..].trim_start().chars().next();
        let starts_word = !before.is_some_and(|c| c.is_alphanumeric() || c == '_' || c == '.');
        if starts_word && matches!(after, Some('(') | Some('"') | Some('{')) {
            return true;
        }
        rest = &rest[at + function.len()..];
    }
    false
}

pub fn risks(shared: &SharedAutomation) -> Vec<Risk> {
    let code = strip_comments(&shared.lua_code);
    let mut found = Vec::new();
    let mut check = |risk: Risk, functions: &[&str]| {
        if functions.iter().any(|f| calls(&code, f)) {
            found.push(risk);
        }
    };
    check(Risk::DeletesFiles, &["fs.delete"]);
    check(Risk::MovesFiles, &["fs.move", "fs.rename"]);
    check(Risk::WritesFiles, &["fs.write", "fs.append", "fs.copy", "fs.mkdir", "zip.create", "zip.extract"]);
    check(Risk::OpensApps, &["app.open"]);
    check(Risk::UsesInternet, &["http.get", "http.post"]);
    check(Risk::UsesClipboard, &["clipboard.get", "clipboard.set"]);
    check(Risk::RunsCommands, &["shell.run", "shell.powershell"]);
    check(Risk::ControlsInput, &["keyboard.press", "keyboard.type", "mouse.move", "mouse.click"]);
    check(
        Risk::ControlsPower,
        &[
            "process.kill", "window.close", "system.lock", "system.sleep", "system.shutdown", "system.restart",
            "system.wake_at", "system.set_wallpaper", "system.brightness",
        ],
    );
    if shared.allow_system {
        found.push(Risk::NeedsSystemControl);
    }
    if !shared.triggers.is_empty() {
        found.push(Risk::RunsOnEvents);
    }
    if shared.run_on_startup {
        found.push(Risk::RunsOnStartup);
    }
    if shared.watch_path.as_deref().is_some_and(|p| !p.trim().is_empty()) {
        found.push(Risk::WatchesFolder);
    }
    if shared.schedule.as_deref().is_some_and(|s| !s.trim().is_empty()) {
        found.push(Risk::RunsOnSchedule);
    }
    found
}

/// What the import screen shows before anything is saved.
#[derive(Debug, Clone, Serialize)]
pub struct ImportPreview {
    pub automation: SharedAutomation,
    pub risks: Vec<Risk>,
    /// Problems that would stop the import (e.g. a Lua syntax error).
    pub problems: Vec<String>,
}

pub fn preview(text: &str) -> Result<ImportPreview, String> {
    let automation = SharedAutomation::parse(text)?;
    let problems = automation.to_input().validate().err().unwrap_or_default();
    Ok(ImportPreview { risks: risks(&automation), automation, problems })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn shared(code: &str) -> SharedAutomation {
        SharedAutomation {
            format: FORMAT.into(),
            version: 1,
            name: "t".into(),
            description: String::new(),
            lua_code: code.into(),
            schedule: None,
            run_on_startup: false,
            watch_path: None,
            watch_pattern: None,
            triggers: ExtraTriggers::default(),
            allow_system: false,
            exported_at: None,
            app_version: None,
        }
    }

    #[test]
    fn finds_risky_calls_but_not_comments() {
        let found = risks(&shared("fs.delete(x)\n-- app.open('x')\nlocal r = http.get(\"u\")"));
        assert_eq!(found, vec![Risk::DeletesFiles, Risk::UsesInternet]);
        assert!(risks(&shared("log('safe')")).is_empty());
        assert!(!calls("myfs.delete(x)", "fs.delete"));
    }

    #[test]
    fn rejects_other_files() {
        assert!(SharedAutomation::parse("hello").is_err());
        assert!(SharedAutomation::parse(r#"{"format":"other","version":1,"name":"x","lua_code":"x"}"#).is_err());
        assert!(SharedAutomation::parse(r#"{"format":"localflow","version":99,"name":"x","lua_code":"x"}"#)
            .unwrap_err()
            .contains("newer version"));
    }

    #[test]
    fn round_trips() {
        let original = shared("log(1)");
        assert_eq!(SharedAutomation::parse(&original.to_json()).unwrap(), original);
    }
}
