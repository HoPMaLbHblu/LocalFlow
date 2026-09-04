//! `voice.json`: load and save voice preferences. OWNER: logic agent. Placeholder with the agreed interface.

use std::path::PathBuf;

use super::VoiceSettings;

pub fn path() -> PathBuf {
    crate::appdata::dir().join("voice.json")
}

/// The saved settings; defaults when the file is missing. A damaged file is kept aside
/// (renamed `voice.corrupt-<time>.json`) and defaults are used, never silently lost.
pub fn load() -> VoiceSettings {
    VoiceSettings::default()
}

/// Validate (push key, language, wake phrase, aliases) and save atomically.
pub fn save(_settings: &VoiceSettings) -> Result<(), String> {
    Err("not implemented yet".into())
}
