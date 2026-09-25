//! `voice.json`: load and save voice preferences. No audio and no transcript is ever persisted.
//!
//! `save` validates first (push key, language, wake phrase, aliases) and writes through
//! `appdata::write_safely` (temporary file + rename, previous version kept as `voice.bak`).
//! `load` never loses a damaged file: it is renamed `voice.corrupt-<unix time>.json` and the
//! defaults are used.

use std::{
    collections::HashSet,
    path::{Path, PathBuf},
};

use super::{grammar, AutomationInfo, ListenMode, VoiceAlias, VoiceSettings};

/// A wake phrase needs at least this many words, or at least [`WAKE_MIN_CHARS`] characters.
pub const WAKE_MIN_WORDS: usize = 2;
pub const WAKE_MIN_CHARS: usize = 6;
/// Words a wake phrase must not be or start with: it would collide with the control commands.
// First words a wake phrase may not start with. "ok"/"okay" are allowed ("okay flow", like "OK Google"):
// the grammar treats them as filler, never as a confirmation.
pub const CONTROL_WORDS: &[&str] = &[
    "stop", "stopp", "stoppe", "cancel", "abort", "mute", "unmute", "yes", "no", "yeah", "nope", "run", "start", "starte", "list", "show", "help", "what", "whats",
    "ja", "nein", "da", "нет", "да", "стоп", "стой", "запусти", "запуск", "останови", "отмена", "отмени", "хватит", "покажи", "помощь", "hilfe", "zeige", "abbrechen", "fuhre",
];
pub const WAKE_MAX_CHARS: usize = 40;
pub const ALIAS_MAX_CHARS: usize = 60;
pub const LANGUAGES: [&str; 4] = ["auto", "en", "ru", "de"];

pub fn path() -> PathBuf {
    crate::appdata::dir().join("voice.json")
}

/// The saved settings; defaults when the file is missing. A damaged file is kept aside
/// (renamed `voice.corrupt-<time>.json`) and defaults are used, never silently lost.
pub fn load() -> VoiceSettings {
    load_from(&path())
}

/// Validate (push key, language, wake phrase, aliases) and save atomically.
pub fn save(settings: &VoiceSettings) -> Result<(), String> {
    save_to(&path(), settings)
}

pub fn load_from(file: &Path) -> VoiceSettings {
    let bytes = match std::fs::read(file) {
        Ok(b) => b,
        Err(_) => return VoiceSettings::default(),
    };
    match serde_json::from_slice::<VoiceSettings>(&bytes) {
        Ok(s) => s,
        Err(e) => {
            tracing::warn!("voice settings are damaged ({e}); keeping the file aside");
            keep_aside(file);
            VoiceSettings::default()
        }
    }
}

fn keep_aside(file: &Path) {
    let secs = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
    let mut target = file.with_file_name(format!("voice.corrupt-{secs}.json"));
    let mut n = 1;
    while target.exists() {
        target = file.with_file_name(format!("voice.corrupt-{secs}-{n}.json"));
        n += 1;
    }
    if std::fs::rename(file, &target).is_err() {
        let _ = std::fs::copy(file, &target);
    }
}

pub fn save_to(file: &Path, settings: &VoiceSettings) -> Result<(), String> {
    let clean = validate(settings)?;
    let json = serde_json::to_vec_pretty(&clean).map_err(|e| e.to_string())?;
    crate::appdata::write_safely(file, &json)
}

/// Check everything and return the cleaned copy that is saved (push key in its canonical form,
/// trimmed wake phrase, aliases with trimmed phrases).
pub fn validate(settings: &VoiceSettings) -> Result<VoiceSettings, String> {
    let mut s = settings.clone();
    s.push_key = crate::triggers::normalize_hotkey(settings.push_key.trim()).map_err(|e| format!("Push-to-talk key: {e}"))?;
    if !LANGUAGES.contains(&s.language.as_str()) {
        return Err(format!("Language \"{}\" is not supported (use auto, en, ru or de).", s.language));
    }
    s.wake_phrase = settings.wake_phrase.trim().to_string();
    let wake_len = s.wake_phrase.chars().count();
    if wake_len == 0 {
        if s.mode == ListenMode::AlwaysOn {
            return Err("Always-on listening needs a wake phrase.".into());
        }
    } else {
        check_wake_phrase(&s.wake_phrase)?;
    }
    let mut seen = HashSet::new();
    for alias in &mut s.aliases {
        alias.phrase = alias.phrase.trim().to_string();
        let norm = grammar::normalize(&alias.phrase);
        if norm.is_empty() {
            return Err("A spoken name can't be empty.".into());
        }
        if alias.phrase.chars().count() > ALIAS_MAX_CHARS {
            return Err(format!("The spoken name \"{}\" is too long (at most {ALIAS_MAX_CHARS} characters).", alias.phrase));
        }
        if grammar::is_reserved_phrase(&norm) {
            return Err(format!("\"{}\" is a built-in voice command and can't be used as a spoken name.", alias.phrase));
        }
        if !seen.insert(norm) {
            return Err(format!("The spoken name \"{}\" is used twice.", alias.phrase));
        }
    }
    Ok(s)
}

fn check_wake_phrase(phrase: &str) -> Result<(), String> {
    let len = phrase.chars().count();
    if len > WAKE_MAX_CHARS {
        return Err(format!("The wake phrase can be at most {WAKE_MAX_CHARS} characters long."));
    }
    let words = grammar::words(phrase);
    if words.len() < WAKE_MIN_WORDS && len < WAKE_MIN_CHARS {
        return Err(format!("The wake phrase needs at least {WAKE_MIN_WORDS} words or {WAKE_MIN_CHARS} characters, so ordinary speech does not trigger it."));
    }
    let Some(first) = words.first() else {
        return Err("The wake phrase needs real words.".into());
    };
    if CONTROL_WORDS.contains(&first.as_str()) || grammar::is_reserved_phrase(&words.join(" ")) {
        return Err(format!("\"{phrase}\" starts like a voice command (stop, mute, yes, run ...); choose another wake phrase."));
    }
    Ok(())
}

/// Aliases that are not used any more: their automation is gone, or it no longer has the name it
/// had when the alias was made (an id can be reused after a backup is restored). Matching is
/// case-insensitive. The settings screen lists these so the person can set them up again.
pub fn stale_aliases(settings: &VoiceSettings, automations: &[AutomationInfo]) -> Vec<VoiceAlias> {
    settings
        .aliases
        .iter()
        .filter(|al| {
            !automations
                .iter()
                .any(|a| a.id == al.automation_id && a.name.trim().to_lowercase() == al.automation_name.trim().to_lowercase())
        })
        .cloned()
        .collect()
}

/// Checks that need the automations (so they are not part of [`validate`]): two automations whose
/// names say the same phrase, or an alias that says another automation's name, would be
/// ambiguous by voice. Call it when the person saves an alias.
pub fn check_alias_conflicts(settings: &VoiceSettings, automations: &[AutomationInfo]) -> Result<(), String> {
    for al in &settings.aliases {
        let phrase = grammar::normalize(&al.phrase);
        if let Some(other) = automations.iter().find(|a| a.id != al.automation_id && grammar::normalize(&a.name) == phrase) {
            return Err(format!("The spoken name \"{}\" is also the name of the automation \"{}\"; pick another.", al.phrase, other.name));
        }
    }
    let mut names = HashSet::new();
    for a in automations {
        let n = grammar::normalize(&a.name);
        if !n.is_empty() && !names.insert(n) {
            return Err(format!("Two automations sound the same (\"{}\"); voice could not tell them apart. Rename one.", a.name));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::voice::VoiceAlias;

    fn alias(p: &str, id: i64) -> VoiceAlias {
        VoiceAlias { phrase: p.into(), automation_id: id, automation_name: "X".into() }
    }

    #[test]
    fn round_trip_and_defaults() {
        let dir = tempfile::tempdir().unwrap();
        let f = dir.path().join("voice.json");
        assert_eq!(load_from(&f), VoiceSettings::default());
        let mut s = VoiceSettings::default();
        s.enabled = true;
        s.push_key = "ctrl + alt + k".into();
        s.aliases.push(alias("Бэкап", 3));
        save_to(&f, &s).unwrap();
        let back = load_from(&f);
        assert_eq!(back.push_key, "Ctrl+Alt+K");
        assert_eq!(back.aliases, s.aliases);
        assert!(back.enabled);
        save_to(&f, &s).unwrap();
        assert!(f.with_extension("bak").exists());
    }

    #[test]
    fn missing_and_unknown_fields_default() {
        let dir = tempfile::tempdir().unwrap();
        let f = dir.path().join("voice.json");
        std::fs::write(&f, r#"{"enabled": true, "future_field": 1}"#).unwrap();
        let s = load_from(&f);
        assert!(s.enabled);
        assert_eq!(s.push_key, "Ctrl+Alt+Space");
        assert!(!s.change_settings);
    }

    #[test]
    fn a_damaged_file_is_kept_aside() {
        let dir = tempfile::tempdir().unwrap();
        let f = dir.path().join("voice.json");
        std::fs::write(&f, "{ not json").unwrap();
        assert_eq!(load_from(&f), VoiceSettings::default());
        assert!(!f.exists());
        let kept: Vec<_> = std::fs::read_dir(dir.path())
            .unwrap()
            .flatten()
            .filter(|e| e.file_name().to_string_lossy().starts_with("voice.corrupt-"))
            .collect();
        assert_eq!(kept.len(), 1);
        assert_eq!(std::fs::read_to_string(kept[0].path()).unwrap(), "{ not json");
    }

    #[test]
    fn validation() {
        let ok = VoiceSettings::default();
        assert!(validate(&ok).is_ok());
        let mut s = ok.clone();
        s.push_key = "Space".into();
        assert!(validate(&s).is_err());
        let mut s = ok.clone();
        s.language = "fr".into();
        assert!(validate(&s).is_err());
        let mut s = ok.clone();
        s.wake_phrase = "x".into();
        assert!(validate(&s).is_err());
        s.wake_phrase = "a".repeat(41);
        assert!(validate(&s).is_err());
        let mut s = ok.clone();
        s.wake_phrase = "  ".into();
        assert!(validate(&s).is_ok());
        s.mode = ListenMode::AlwaysOn;
        assert!(validate(&s).is_err());
        let mut s = ok.clone();
        s.aliases = vec![alias("  ", 1)];
        assert!(validate(&s).is_err());
        s.aliases = vec![alias("Backup!", 1), alias("backup", 2)];
        assert!(validate(&s).is_err());
        for reserved in ["stop", "Помощь", "Hilfe", "yes", "dark mode"] {
            s.aliases = vec![alias(reserved, 1)];
            assert!(validate(&s).is_err(), "{reserved}");
        }
        s.aliases = vec![alias("my backup", 1), alias("фото", 2)];
        assert!(validate(&s).is_ok());
    }

    #[test]
    fn wake_phrase_rules() {
        let with = |p: &str| VoiceSettings { wake_phrase: p.into(), ..VoiceSettings::default() };
        for ok in ["hey localflow", "computer please", "локалфлоу", "okay flow", "Hallo Fluss", "jarvis"] {
            assert!(validate(&with(ok)).is_ok(), "{ok}");
        }
        for bad in ["x", "hey", "go", "stop", "stop it now", "mute", "mute mic", "yes sir", "no way", "run it", "start now", "стоп машина", "запусти", "ja bitte", &"a".repeat(41), "list things"] {
            assert!(validate(&with(bad)).is_err(), "{bad}");
        }
    }

    #[test]
    fn push_key_must_be_a_real_shortcut() {
        let with = |k: &str| VoiceSettings { push_key: k.into(), ..VoiceSettings::default() };
        for bad in ["Ctrl", "Ctrl+Alt", "Shift", "Space", "A", "Ctrl+Nonsense", ""] {
            assert!(validate(&with(bad)).is_err(), "{bad}");
        }
        assert!(validate(&with("ctrl+shift+f9")).is_ok());
    }

    fn info(id: i64, name: &str) -> AutomationInfo {
        AutomationInfo { id, name: name.into(), description: String::new(), enabled: true, allow_system: false }
    }

    #[test]
    fn stale_aliases_follow_the_automation_name() {
        let mut s = VoiceSettings::default();
        s.aliases = vec![
            VoiceAlias { phrase: "backup".into(), automation_id: 1, automation_name: "Zip Backup".into() },
            VoiceAlias { phrase: "photos".into(), automation_id: 2, automation_name: "Photos".into() },
            VoiceAlias { phrase: "gone".into(), automation_id: 3, automation_name: "Gone".into() },
        ];
        let list = vec![info(1, "zip backup "), info(2, "Something else")];
        let stale = stale_aliases(&s, &list);
        assert_eq!(stale.iter().map(|a| a.phrase.as_str()).collect::<Vec<_>>(), vec!["photos", "gone"]);
    }

    #[test]
    fn alias_conflicts_with_automation_names_are_reported() {
        let mut s = VoiceSettings::default();
        s.aliases = vec![VoiceAlias { phrase: "Photos!".into(), automation_id: 1, automation_name: "Zip".into() }];
        let list = vec![info(1, "Zip"), info(2, "photos")];
        assert!(check_alias_conflicts(&s, &list).is_err());
        s.aliases[0].phrase = "pictures".into();
        assert!(check_alias_conflicts(&s, &list).is_ok());
        assert!(check_alias_conflicts(&s, &[info(1, "Sync A"), info(2, "sync a!")]).is_err());
    }
}
