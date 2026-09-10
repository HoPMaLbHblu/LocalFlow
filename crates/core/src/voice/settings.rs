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

use super::{grammar, ListenMode, VoiceSettings};

pub const WAKE_MIN_CHARS: usize = 2;
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
    } else if !(WAKE_MIN_CHARS..=WAKE_MAX_CHARS).contains(&wake_len) {
        return Err(format!("The wake phrase must be {WAKE_MIN_CHARS}-{WAKE_MAX_CHARS} characters long."));
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
        assert!(s.change_settings);
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
}
