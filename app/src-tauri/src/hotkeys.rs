//! Global hotkeys: pressing an automation's hotkey anywhere in Windows runs it.
//! A separate slot holds the voice push-to-talk key (press and release both matter there).

use std::{
    str::FromStr,
    sync::{
        atomic::{AtomicBool, Ordering},
        Mutex, OnceLock,
    },
};

use localflow_core::triggers::ExtraTriggers;
use tauri::{AppHandle, Manager};
use tauri_plugin_global_shortcut::{GlobalShortcutExt, Shortcut, ShortcutState};

use crate::AppState;

/// Registered hotkeys and the automation each one runs.
#[derive(Default)]
pub struct Hotkeys(pub Mutex<Vec<(Shortcut, i64)>>);

/// What the push-to-talk key did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeyAction {
    Press,
    Release,
}

/// The voice push-to-talk slot. It survives `refresh` (which re-registers it after
/// `unregister_all`) and filters key repeat: only the first `Pressed` counts until `Released`.
#[derive(Default)]
pub struct VoiceKey {
    shortcut: Mutex<Option<Shortcut>>,
    down: AtomicBool,
}

impl VoiceKey {
    pub fn matches(&self, shortcut: &Shortcut) -> bool {
        self.shortcut.lock().map(|s| s.as_ref() == Some(shortcut)).unwrap_or(false)
    }

    /// Turn a platform key event into an action (`None` for key repeat and stray releases).
    pub fn transition(&self, state: ShortcutState) -> Option<KeyAction> {
        match state {
            ShortcutState::Pressed => (!self.down.swap(true, Ordering::SeqCst)).then_some(KeyAction::Press),
            ShortcutState::Released => self.down.swap(false, Ordering::SeqCst).then_some(KeyAction::Release),
        }
    }

    /// Set the registered shortcut. Returns true if the key was held down (the caller releases).
    fn set(&self, shortcut: Option<Shortcut>) -> bool {
        if let Ok(mut s) = self.shortcut.lock() {
            *s = shortcut;
        }
        self.down.swap(false, Ordering::SeqCst)
    }
}

/// What `refresh` should do with the push-to-talk key.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum VoiceKeyPlan {
    /// Voice doesn't need a key (off, or always-on).
    Unused,
    Register(Shortcut),
    /// An automation already uses this key; the automation wins.
    Conflict(String),
    Invalid(String),
}

/// Decide about the push-to-talk key. `automation_keys` are the enabled automations' hotkeys.
pub fn plan_voice_key(wanted: Option<&str>, automation_keys: &[String]) -> VoiceKeyPlan {
    let Some(text) = wanted else { return VoiceKeyPlan::Unused };
    let shortcut = match Shortcut::from_str(text) {
        Ok(s) => s,
        Err(e) => return VoiceKeyPlan::Invalid(format!("{text}: {e}")),
    };
    let clash = automation_keys.iter().any(|k| {
        Shortcut::from_str(k).map(|s| s == shortcut).unwrap_or_else(|_| k.eq_ignore_ascii_case(text))
    });
    if clash {
        VoiceKeyPlan::Conflict(text.to_string())
    } else {
        VoiceKeyPlan::Register(shortcut)
    }
}

/// Called by the plugin for every registered hotkey.
pub fn handle(app: &AppHandle, shortcut: &Shortcut, state: ShortcutState) {
    // The push-to-talk key comes first: it needs both press and release.
    if let Some(key) = app.try_state::<VoiceKey>() {
        if key.matches(shortcut) {
            match key.transition(state) {
                Some(KeyAction::Press) => crate::voice::press(app),
                Some(KeyAction::Release) => crate::voice::release(app),
                None => {}
            }
            return;
        }
    }
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

/// Register the hotkeys of all enabled automations and the voice key. Call when automations or
/// voice settings change.
pub fn refresh(app: &AppHandle) {
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        // One refresh at a time: two interleaved unregister/register rounds would fail each other.
        static LOCK: OnceLock<tokio::sync::Mutex<()>> = OnceLock::new();
        let _guard = LOCK.get_or_init(|| tokio::sync::Mutex::new(())).lock().await;

        let Some(state) = app.try_state::<AppState>() else { return };
        let list = state.flow.list().await.unwrap_or_default();
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
        for (text, id) in &wanted {
            match Shortcut::from_str(text) {
                Ok(shortcut) => match shortcuts.register(shortcut) {
                    Ok(()) => registered.push((shortcut, *id)),
                    // Another program (or another automation) already uses it.
                    Err(e) => tracing::warn!(automation_id = id, hotkey = %text, "hotkey not available: {e}"),
                },
                Err(e) => tracing::warn!(automation_id = id, hotkey = %text, "invalid hotkey: {e}"),
            }
        }
        if let Ok(mut list) = app.state::<Hotkeys>().0.lock() {
            *list = registered;
        }

        // The voice key.
        let keys: Vec<String> = wanted.into_iter().map(|(k, _)| k).collect();
        let plan = plan_voice_key(crate::voice::wanted_push_key(&app).as_deref(), &keys);
        let (shortcut, notice) = match plan {
            VoiceKeyPlan::Unused => (None, None),
            VoiceKeyPlan::Register(s) => match shortcuts.register(s) {
                Ok(()) => (Some(s), None),
                Err(e) => {
                    tracing::warn!("voice push-to-talk key not available: {e}");
                    (None, Some("The push-to-talk key is already used by another program. Choose another key.".to_string()))
                }
            },
            VoiceKeyPlan::Conflict(key) => {
                tracing::warn!("voice push-to-talk key is also used by an automation; not registered");
                (None, Some(format!("The push-to-talk key {key} is used by an automation. Choose another key.")))
            }
            VoiceKeyPlan::Invalid(why) => (None, Some(format!("The push-to-talk key is not valid ({why})."))),
        };
        if let Some(key) = app.try_state::<VoiceKey>() {
            if key.set(shortcut) {
                crate::voice::release(&app);
            }
        }
        crate::voice::set_hotkey_notice(&app, notice);
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn key_repeat_is_ignored_until_release() {
        let key = VoiceKey::default();
        assert_eq!(key.transition(ShortcutState::Pressed), Some(KeyAction::Press));
        assert_eq!(key.transition(ShortcutState::Pressed), None);
        assert_eq!(key.transition(ShortcutState::Pressed), None);
        assert_eq!(key.transition(ShortcutState::Released), Some(KeyAction::Release));
        assert_eq!(key.transition(ShortcutState::Released), None);
        assert_eq!(key.transition(ShortcutState::Pressed), Some(KeyAction::Press));
    }

    #[test]
    fn plan_register_unused_conflict_invalid() {
        assert_eq!(plan_voice_key(None, &[]), VoiceKeyPlan::Unused);
        let s = Shortcut::from_str("Ctrl+Alt+Space").unwrap();
        assert_eq!(plan_voice_key(Some("Ctrl+Alt+Space"), &["Ctrl+Alt+K".into()]), VoiceKeyPlan::Register(s));
        // Same key written differently still clashes.
        assert!(matches!(
            plan_voice_key(Some("Ctrl+Alt+Space"), &["alt+ctrl+space".into()]),
            VoiceKeyPlan::Conflict(_)
        ));
        assert!(matches!(plan_voice_key(Some("not a key"), &[]), VoiceKeyPlan::Invalid(_)));
    }

    #[test]
    fn replacing_the_shortcut_reports_a_held_key() {
        let key = VoiceKey::default();
        let s = Shortcut::from_str("Ctrl+Alt+Space").unwrap();
        assert!(!key.set(Some(s)));
        assert!(key.matches(&s));
        key.transition(ShortcutState::Pressed);
        assert!(key.set(None));
        assert!(!key.matches(&s));
    }
}
