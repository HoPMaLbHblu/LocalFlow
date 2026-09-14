//! Global hotkeys: pressing an automation's hotkey anywhere in Windows runs it.

use std::{str::FromStr, sync::Mutex};

use localflow_core::triggers::ExtraTriggers;
use tauri::{AppHandle, Manager};
use tauri_plugin_global_shortcut::{GlobalShortcutExt, Shortcut, ShortcutState};

use crate::AppState;

/// Registered hotkeys and the automation each one runs.
#[derive(Default)]
pub struct Hotkeys(pub Mutex<Vec<(Shortcut, i64)>>);

/// Called by the plugin for every registered hotkey.
pub fn handle(app: &AppHandle, shortcut: &Shortcut, state: ShortcutState) {
    if state != ShortcutState::Pressed {
        return;
    }
    let id = app
        .state::<Hotkeys>()
        .0
        .lock()
        .ok()
        .and_then(|list| list.iter().find(|(s, _)| s == shortcut).map(|(_, id)| *id));
    let Some(id) = id else { return };
    let flow = app.state::<AppState>().flow.clone();
    tauri::async_runtime::spawn(async move {
        if let Err(e) = flow.run(id, "hotkey").await {
            tracing::error!(automation_id = id, "hotkey run failed: {e}");
        }
    });
}

/// Register the hotkeys of all enabled automations. Call when automations change.
pub fn refresh(app: &AppHandle) {
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        let Some(state) = app.try_state::<AppState>() else { return };
        let Ok(list) = state.flow.list().await else { return };
        let wanted: Vec<(String, i64)> = list
            .into_iter()
            .filter(|s| s.automation.enabled)
            .filter_map(|s| {
                let hotkey = ExtraTriggers::from_json(s.automation.triggers.as_deref()).hotkey?;
                Some((hotkey, s.automation.id))
            })
            .collect();

        let shortcuts = app.global_shortcut();
        let _ = shortcuts.unregister_all();
        let mut registered = Vec::new();
        for (text, id) in wanted {
            match Shortcut::from_str(&text) {
                Ok(shortcut) => match shortcuts.register(shortcut) {
                    Ok(()) => registered.push((shortcut, id)),
                    // Another program (or another automation) already uses it.
                    Err(e) => tracing::warn!(automation_id = id, hotkey = %text, "hotkey not available: {e}"),
                },
                Err(e) => tracing::warn!(automation_id = id, hotkey = %text, "invalid hotkey: {e}"),
            }
        }
        if let Ok(mut list) = app.state::<Hotkeys>().0.lock() {
            *list = registered;
        }
    });
}
