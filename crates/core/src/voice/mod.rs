//! Voice control: speak to LocalFlow to run automations and change a few safe settings.
//!
//! Nothing here turns recognised speech into shell commands. A spoken sentence is parsed into
//! one of a small fixed set of intents (run a named automation, stop, list, change a whitelisted
//! setting, confirm/cancel); everything else is "I didn't understand". Automations run through
//! the same `LocalFlow` API as hotkeys and schedules, so their permissions still apply.
//!
//! Layers (one owner per file; everything is testable without a microphone):
//! - `mod.rs`         shared types and traits (lead)
//! - `settings.rs`    `voice.json`: load/save with atomic writes (logic)
//! - `grammar.rs`     normalising text and parsing it into an `Intent` in en/ru/de (logic)
//! - `matcher.rs`     fuzzy matching of automation names and aliases, never guessing (logic)
//! - `controller.rs`  `VoiceController`: transcript -> replies; confirmations, duplicate
//!                    suppression, in-flight guard, whitelisted settings (logic)
//! - `wake.rs`        wake-phrase detection (audio)
//! - `session.rs`     the listening state machine: modes, mute, device loss, echo guard (audio)
//! - `engine.rs`      speech engines and their models: list, download, verify, create (audio)
//! - `audio.rs`       microphone devices and capture (audio)

pub mod audio;
pub mod controller;
pub mod engine;
pub mod grammar;
pub mod matcher;
pub mod session;
pub mod settings;
pub mod wake;

use std::sync::mpsc::Sender;

use serde::{Deserialize, Serialize};

// ---- settings ---------------------------------------------------------------------------------

/// How the microphone is used.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum ListenMode {
    /// Radio mode: hold a key (or the on-screen button) while speaking.
    #[default]
    PushToTalk,
    /// Listen continuously; a wake phrase is required before a command.
    AlwaysOn,
}

/// A spoken name for an automation, added by the user ("backup" -> "Zip backup").
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct VoiceAlias {
    pub phrase: String,
    pub automation_id: i64,
    /// The automation's name when the alias was made (shown if the automation is gone).
    pub automation_name: String,
}

/// Voice preferences, saved as `voice.json` in the app's data folder. No audio is ever stored.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct VoiceSettings {
    /// The first-run setup was finished.
    pub setup_done: bool,
    /// The user accepted the disclosure (what is downloaded, that audio stays on this PC).
    pub consented: bool,
    /// Voice control is switched on. Off by default; continuous listening is always opt-in.
    pub enabled: bool,
    pub mode: ListenMode,
    /// Push-to-talk shortcut, e.g. "Ctrl+Alt+Space".
    pub push_key: String,
    /// Said before a command in always-on mode, e.g. "hey localflow".
    pub wake_phrase: String,
    /// "auto" (the app language), "en", "ru" or "de".
    pub language: String,
    /// Input device name; `None` = the system default.
    pub microphone: Option<String>,
    /// Speech engine id (see `engine::available_engines`).
    pub engine: String,
    /// Speak replies aloud.
    pub spoken_feedback: bool,
    /// May voice start automations that have "Allow system control"? Always asks first.
    pub run_system_automations: bool,
    /// May voice change the whitelisted app settings?
    pub change_settings: bool,
    pub aliases: Vec<VoiceAlias>,
}

impl Default for VoiceSettings {
    fn default() -> Self {
        VoiceSettings {
            setup_done: false,
            consented: false,
            enabled: false,
            mode: ListenMode::PushToTalk,
            push_key: "Ctrl+Alt+Space".into(),
            wake_phrase: "hey localflow".into(),
            language: "auto".into(),
            microphone: None,
            engine: String::new(),
            spoken_feedback: false,
            run_system_automations: false,
            change_settings: true,
            aliases: Vec::new(),
        }
    }
}

// ---- what the voice layer sees of LocalFlow ------------------------------------------------------

/// An automation as the voice layer sees it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AutomationInfo {
    pub id: i64,
    pub name: String,
    pub description: String,
    pub enabled: bool,
    /// Has "Allow system control": it can run commands, press keys, close programs, power off.
    pub allow_system: bool,
}

/// A run in progress.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RunningInfo {
    pub run_id: i64,
    pub automation_id: i64,
    pub name: String,
}

/// App settings voice may change. Deliberately small: no folders, no system control, no secrets,
/// no backups, no creating or editing automations.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "setting", content = "value", rename_all = "snake_case")]
pub enum SettingChange {
    Notifications(bool),
    /// "light", "dark" or "system".
    Theme(String),
    /// "auto", "en", "ru" or "de".
    Language(String),
    UpdateCheck(bool),
    DotaLiveHelper(bool),
    Autostart(bool),
}

/// Everything the controller needs from the app. The real implementation (desktop crate) wraps
/// `LocalFlow`; tests use a mock. Methods may block briefly and are called from the session
/// thread, never from an async runtime thread.
pub trait VoiceBackend: Send + Sync {
    fn automations(&self) -> Vec<AutomationInfo>;
    fn running(&self) -> Vec<RunningInfo>;
    /// Start an automation (trigger "voice") and return at once; the run continues in the background.
    fn start(&self, automation_id: i64) -> Result<(), String>;
    /// Ask a run to stop. `Ok(false)` = it had already finished.
    fn stop(&self, run_id: i64) -> Result<bool, String>;
    /// Apply a whitelisted setting; returns a short description of the new state.
    fn apply(&self, change: &SettingChange) -> Result<String, String>;
    /// Show a desktop notification. Every command that was heard is shown, like Telegram's.
    fn notice(&self, text: &str);
}

// ---- what the controller says back ---------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReplyKind {
    /// Information (the command list, what is running).
    Info,
    /// Something was done.
    Done,
    /// Not understood, refused, ambiguous or failed.
    Problem,
    /// A yes/no question; the next utterance (or a button) answers it.
    Confirm,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Reply {
    pub kind: ReplyKind,
    pub text: String,
    /// Worth speaking aloud when spoken feedback is on.
    pub speak: bool,
}

/// One recognised utterance.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Transcript {
    /// Increases with every utterance; the same id is never handled twice.
    pub id: u64,
    pub text: String,
    /// 0.0 - 1.0 when the engine reports it.
    pub confidence: Option<f32>,
    /// "en", "ru", "de", ... when known.
    pub language: Option<String>,
}

/// A command available right now, for the "what can I say" list in the UI and by voice.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CommandExample {
    /// "Run Zip backup"
    pub say: String,
    /// What it does.
    pub does: String,
    /// "automation", "alias", "control" or "setting".
    pub group: String,
}

// ---- state shown to the user ---------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "state", content = "message", rename_all = "snake_case")]
pub enum VoiceState {
    /// Voice control is switched off; the microphone is not in use.
    Off,
    /// On, waiting for the push-to-talk key or the wake phrase.
    Idle,
    /// Hearing speech (or armed after the wake phrase).
    Listening,
    /// Recognising or carrying out a command.
    Processing,
    /// Speaking a reply (the microphone input is ignored meanwhile).
    Speaking,
    /// Muted by the user: nothing is heard until unmuted.
    Muted,
    /// Something is wrong (no microphone, no engine, device lost ...). The text is user-facing.
    Error(String),
}

/// Events for the UI and the tray (the desktop app forwards them as `localflow://voice`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum VoiceEvent {
    State { state: VoiceState },
    /// What was heard (shown, never stored as audio).
    Heard { text: String, confidence: Option<f32> },
    Reply { reply: Reply },
    /// A yes/no question is waiting for an answer.
    Confirm { prompt: String },
}

pub type VoiceEvents = std::sync::Arc<dyn Fn(VoiceEvent) + Send + Sync>;

// ---- audio and recognition (implemented by `audio.rs`, `engine.rs`, mocked in tests) -------------

/// An input device.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct InputDevice {
    pub name: String,
    pub is_default: bool,
}

/// What a capture source reports. Audio is always 16 kHz, mono, f32 in -1.0..1.0.
#[derive(Debug, Clone, PartialEq)]
pub enum AudioEvent {
    Chunk(Vec<f32>),
    /// The device was unplugged or disabled.
    Disconnected,
    Error(String),
}

/// A microphone. Real capture lives in `audio.rs` on its own thread; tests feed a script of events.
pub trait AudioSource: Send {
    /// Start capturing from `device` (`None` = default) and send events to `sink`.
    fn start(&mut self, device: Option<&str>, sink: Sender<AudioEvent>) -> Result<(), String>;
    /// Stop capturing and release the device. Safe to call twice.
    fn stop(&mut self);
}

#[derive(Debug, Clone, PartialEq)]
pub struct Recognized {
    pub text: String,
    pub confidence: Option<f32>,
    pub language: Option<String>,
}

/// What a segmenter found in the audio so far.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Segmented {
    /// Finished utterances (16 kHz mono), each with a little lead-in so the first word isn't clipped.
    pub utterances: Vec<Vec<f32>>,
    /// Someone is speaking right now.
    pub speech_in_progress: bool,
}

/// Cuts a stream of audio into utterances (voice activity detection). The real one is Silero VAD in
/// `engine.rs`; `session.rs` has a simple energy-based one for tests and as a fallback.
pub trait Segmenter: Send {
    fn feed(&mut self, chunk: &[f32]) -> Segmented;
    /// The speech collected so far, even if the speaker hasn't paused (push-to-talk release).
    fn flush(&mut self) -> Vec<Vec<f32>>;
    fn reset(&mut self);
}

/// Result of the user-initiated microphone check in the setup flow.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MicTest {
    /// Loudest sample, 0.0 - 1.0.
    pub peak: f32,
    pub rms: f32,
    /// Enough signal to say the microphone works.
    pub heard_sound: bool,
}

/// Speech to text for one utterance of 16 kHz mono audio. The real engine lives in `engine.rs`.
pub trait Recognizer: Send {
    /// `language`: "en", "ru", "de" or "auto". An empty text means nothing intelligible was heard.
    fn transcribe(&mut self, audio: &[f32], language: &str) -> Result<Recognized, String>;
    fn name(&self) -> String;
}

/// Spoken replies. `speak` returns when the speech has finished.
pub trait Speaker: Send + Sync {
    fn speak(&self, text: &str) -> Result<(), String>;
}
