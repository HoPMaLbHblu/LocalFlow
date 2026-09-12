//! The listening state machine: modes, mute, device loss, echo guard. OWNER: session agent.
//! Placeholder with the agreed interface.

use std::sync::Arc;

use super::{
    controller::VoiceController, AudioSource, CommandExample, Recognizer, Segmenter, Speaker, VoiceEvents,
    VoiceSettings, VoiceState,
};

/// Everything a session needs. The desktop app builds the real parts; tests pass mocks.
pub struct SessionParts {
    pub settings: VoiceSettings,
    /// The resolved recognition language: "en", "ru" or "de".
    pub language: String,
    pub source: Box<dyn AudioSource>,
    pub recognizer: Box<dyn Recognizer>,
    pub segmenter: Box<dyn Segmenter>,
    pub speaker: Option<Arc<dyn Speaker>>,
    pub controller: VoiceController,
    pub events: VoiceEvents,
}

/// A running voice session on its own worker thread. Dropping it stops listening and releases
/// the microphone.
pub struct Session {
    _private: (),
}

impl Session {
    pub fn start(_parts: SessionParts) -> Session {
        Session { _private: () }
    }
    pub fn state(&self) -> VoiceState {
        VoiceState::Off
    }
    /// Push-to-talk key (or on-screen button) pressed / released.
    pub fn press(&self) {}
    pub fn release(&self) {}
    pub fn set_muted(&self, _muted: bool) {}
    pub fn is_muted(&self) -> bool {
        false
    }
    /// A typed phrase ("try a phrase"): handled exactly like speech; replies arrive as events.
    pub fn submit_text(&self, _text: &str) {}
    /// The on-screen Yes/No buttons.
    pub fn answer_confirmation(&self, _yes: bool) {}
    /// Apply new settings live where possible (wake phrase, spoken feedback, aliases, confirmations,
    /// push-to-talk vs always-on). Returns true if the microphone, language or engine changed and the
    /// session must be restarted by the caller.
    pub fn update_settings(&self, _settings: VoiceSettings) -> bool {
        false
    }
    pub fn commands(&self) -> Vec<CommandExample> {
        Vec::new()
    }
    /// Stop listening now, release the microphone and join the worker.
    pub fn stop(self) {}
}
