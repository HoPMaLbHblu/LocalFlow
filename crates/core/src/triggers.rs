//! Event triggers besides schedules and folder watching: a global hotkey,
//! an app starting or closing, the PC being idle, and a USB drive being plugged in.
//!
//! Hotkeys are registered by the desktop app (they need a window system);
//! everything else is checked here by a small background monitor.

use std::{
    collections::{HashMap, HashSet},
    time::Duration,
};

use serde::{Deserialize, Serialize};

use crate::service::LocalFlow;

/// How often the monitor looks at processes, idle time and drives.
const POLL: Duration = Duration::from_secs(2);

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct ExtraTriggers {
    /// e.g. "Ctrl+Alt+K"
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hotkey: Option<String>,
    /// Run when a program with this name starts, e.g. "steam".
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub app_start: Option<String>,
    /// Run when a program with this name closes.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub app_exit: Option<String>,
    /// Run once when nobody has touched the keyboard or mouse for this many minutes.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub idle_minutes: Option<u32>,
    /// Run when a USB drive or memory card is plugged in.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub usb: bool,
    /// Run when another automation finishes.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub after: Option<AfterTrigger>,
}

/// "Run after another automation finishes".
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AfterTrigger {
    pub automation_id: i64,
    /// `"success"` (default), `"failure"` or `"always"`.
    #[serde(default = "AfterTrigger::default_when")]
    pub when: String,
}

impl AfterTrigger {
    fn default_when() -> String {
        "success".into()
    }

    /// Whether a finished run of the other automation should start this one.
    pub fn matches(&self, success: bool) -> bool {
        match self.when.as_str() {
            "always" => true,
            "failure" => !success,
            _ => success,
        }
    }
}

fn clean(value: &Option<String>) -> Option<String> {
    value.as_deref().map(str::trim).filter(|v| !v.is_empty()).map(String::from)
}

impl ExtraTriggers {
    pub fn from_json(json: Option<&str>) -> Self {
        json.and_then(|j| serde_json::from_str(j).ok()).unwrap_or_default()
    }

    /// Trimmed, with the hotkey written the standard way.
    pub fn normalized(&self) -> Self {
        let hotkey = clean(&self.hotkey).map(|h| normalize_hotkey(&h).unwrap_or(h));
        ExtraTriggers {
            hotkey,
            app_start: clean(&self.app_start),
            app_exit: clean(&self.app_exit),
            idle_minutes: self.idle_minutes.filter(|m| *m > 0),
            usb: self.usb,
            after: self.after.clone().filter(|a| a.automation_id > 0),
        }
    }

    pub fn is_empty(&self) -> bool {
        *self == ExtraTriggers::default()
    }

    /// `None` when there is nothing to store.
    pub fn to_json(&self) -> Option<String> {
        (!self.is_empty()).then(|| serde_json::to_string(self).expect("triggers serialize"))
    }

    /// Readable problems, for form validation.
    pub fn problems(&self) -> Vec<String> {
        let mut problems = Vec::new();
        if let Some(hotkey) = clean(&self.hotkey) {
            if let Err(e) = normalize_hotkey(&hotkey) {
                problems.push(format!("Hotkey \"{hotkey}\": {e}"));
            }
        }
        if let Some(minutes) = self.idle_minutes {
            if minutes > 24 * 60 {
                problems.push("Idle time must be at most 1440 minutes (24 hours).".into());
            }
        }
        if let Some(after) = &self.after {
            if !["success", "failure", "always"].contains(&after.when.as_str()) {
                problems.push(format!("\"Run after\": unknown choice \"{}\".", after.when));
            }
        }
        problems
    }
}

const MODIFIERS: &[(&str, &str)] = &[
    ("ctrl", "Ctrl"),
    ("control", "Ctrl"),
    ("alt", "Alt"),
    ("shift", "Shift"),
    ("win", "Super"),
    ("super", "Super"),
    ("meta", "Super"),
];

const NAMED_KEYS: &[&str] = &[
    "Space", "Enter", "Tab", "Escape", "Backspace", "Delete", "Insert", "Home", "End", "PageUp", "PageDown", "Up", "Down",
    "Left", "Right",
];

/// "ctrl + alt + k" -> "Ctrl+Alt+K". Needs at least one modifier and exactly one key.
pub fn normalize_hotkey(text: &str) -> Result<String, String> {
    let mut modifiers: Vec<&str> = Vec::new();
    let mut key: Option<String> = None;
    for part in text.split('+').map(str::trim).filter(|p| !p.is_empty()) {
        let lower = part.to_lowercase();
        if let Some((_, name)) = MODIFIERS.iter().find(|(alias, _)| *alias == lower) {
            if !modifiers.contains(name) {
                modifiers.push(name);
            }
            continue;
        }
        if key.is_some() {
            return Err("use one key with Ctrl, Alt, Shift or Win, like Ctrl+Alt+K".into());
        }
        let single = part.chars().count() == 1 && part.chars().all(|c| c.is_ascii_alphanumeric());
        let function = lower.strip_prefix('f').and_then(|n| n.parse::<u8>().ok()).is_some_and(|n| (1..=24).contains(&n));
        let named = NAMED_KEYS.iter().find(|n| n.to_lowercase() == lower);
        key = Some(if single {
            part.to_uppercase()
        } else if function {
            lower.to_uppercase()
        } else if let Some(named) = named {
            named.to_string()
        } else {
            return Err(format!("unknown key \"{part}\""));
        });
    }
    let key = key.ok_or("add a key, like Ctrl+Alt+K")?;
    if modifiers.is_empty() {
        return Err("add Ctrl, Alt, Shift or Win so it doesn't clash with normal typing".into());
    }
    // A fixed order makes hotkeys easy to compare.
    let order = ["Ctrl", "Alt", "Shift", "Super"];
    modifiers.sort_by_key(|m| order.iter().position(|o| o == m));
    Ok(format!("{}+{key}", modifiers.join("+")))
}

/// "Steam.exe" and " steam " both become "steam".
fn process_key(name: &str) -> String {
    let lower = name.trim().to_lowercase();
    lower.strip_suffix(".exe").unwrap_or(&lower).to_string()
}

fn running_process_names() -> HashSet<String> {
    let mut system = sysinfo::System::new();
    system.refresh_processes(sysinfo::ProcessesToUpdate::All, true);
    system.processes().values().map(|p| process_key(&p.name().to_string_lossy())).collect()
}

fn removable_drives() -> HashSet<String> {
    sysinfo::Disks::new_with_refreshed_list()
        .list()
        .iter()
        .filter(|d| d.is_removable())
        .map(|d| d.mount_point().to_string_lossy().into_owned())
        .collect()
}

/// Watches for app start/exit, idle and USB events and runs matching automations.
/// Runs until the program exits.
pub async fn monitor(flow: LocalFlow) {
    let mut processes: Option<HashSet<String>> = None;
    let mut drives: Option<HashSet<String>> = None;
    // Automations that already ran for the current idle period.
    let mut idle_fired: HashSet<i64> = HashSet::new();

    loop {
        tokio::time::sleep(POLL).await;
        let Ok(list) = flow.repo().list_automations().await else { continue };
        let watched: Vec<(i64, ExtraTriggers)> = list
            .into_iter()
            .filter(|a| a.enabled)
            .map(|a| (a.id, ExtraTriggers::from_json(a.triggers.as_deref())))
            .filter(|(_, t)| t.app_start.is_some() || t.app_exit.is_some() || t.idle_minutes.is_some() || t.usb)
            .collect();

        let run = |id: i64, trigger: &'static str, details: HashMap<String, String>| {
            let flow = flow.clone();
            tokio::spawn(async move {
                if let Err(e) = flow.run_with_details(id, trigger, None, details).await {
                    tracing::error!(automation_id = id, trigger, "run failed: {e}");
                }
            });
        };

        // Apps starting and closing.
        if watched.iter().any(|(_, t)| t.app_start.is_some() || t.app_exit.is_some()) {
            let now = tokio::task::spawn_blocking(running_process_names).await.unwrap_or_default();
            if let Some(before) = &processes {
                for (id, t) in &watched {
                    if let Some(app) = t.app_start.as_deref().map(process_key) {
                        if now.contains(&app) && !before.contains(&app) {
                            run(*id, "app_start", HashMap::from([("app".into(), app)]));
                        }
                    }
                    if let Some(app) = t.app_exit.as_deref().map(process_key) {
                        if before.contains(&app) && !now.contains(&app) {
                            run(*id, "app_exit", HashMap::from([("app".into(), app)]));
                        }
                    }
                }
            }
            processes = Some(now);
        } else {
            processes = None;
        }

        // Idle: once per idle period, reset when the user is back.
        let idle = crate::lua::control::idle_seconds();
        if idle < 5 {
            idle_fired.clear();
        }
        for (id, t) in &watched {
            if let Some(minutes) = t.idle_minutes {
                if idle >= u64::from(minutes) * 60 && idle_fired.insert(*id) {
                    run(*id, "idle", HashMap::from([("idle_minutes".into(), minutes.to_string())]));
                }
            }
        }

        // USB drives appearing.
        if watched.iter().any(|(_, t)| t.usb) {
            let now = tokio::task::spawn_blocking(removable_drives).await.unwrap_or_default();
            if let Some(before) = &drives {
                for drive in now.difference(before) {
                    for (id, t) in &watched {
                        if t.usb {
                            run(*id, "usb", HashMap::from([("drive".into(), drive.clone())]));
                        }
                    }
                }
            }
            drives = Some(now);
        } else {
            drives = None;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hotkeys_are_normalized_and_checked() {
        assert_eq!(normalize_hotkey("ctrl + alt + k").unwrap(), "Ctrl+Alt+K");
        assert_eq!(normalize_hotkey("Win+Shift+f5").unwrap(), "Shift+Super+F5");
        assert_eq!(normalize_hotkey("Alt+Ctrl+Space").unwrap(), "Ctrl+Alt+Space");
        assert!(normalize_hotkey("K").is_err(), "a key alone would clash with typing");
        assert!(normalize_hotkey("Ctrl+Alt").is_err());
        assert!(normalize_hotkey("Ctrl+K+L").is_err());
        assert!(normalize_hotkey("Ctrl+Banana").is_err());
    }

    #[test]
    fn triggers_round_trip_through_json() {
        let t = ExtraTriggers { hotkey: Some(" ctrl+alt+k ".into()), usb: true, ..Default::default() }.normalized();
        assert_eq!(t.hotkey.as_deref(), Some("Ctrl+Alt+K"));
        assert_eq!(ExtraTriggers::from_json(t.to_json().as_deref()), t);
        assert_eq!(ExtraTriggers::default().to_json(), None);
        assert_eq!(ExtraTriggers::from_json(Some("not json")), ExtraTriggers::default());
    }
}
