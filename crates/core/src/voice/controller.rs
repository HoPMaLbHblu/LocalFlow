//! `VoiceController`: transcript -> replies. OWNER: logic agent. Placeholder with the agreed interface.

use std::sync::Arc;

use super::{CommandExample, Reply, Transcript, VoiceBackend, VoiceSettings};

pub struct VoiceController {
    _backend: Arc<dyn VoiceBackend>,
    _settings: VoiceSettings,
}

impl VoiceController {
    pub fn new(backend: Arc<dyn VoiceBackend>, settings: VoiceSettings) -> Self {
        VoiceController { _backend: backend, _settings: settings }
    }

    pub fn update_settings(&mut self, settings: VoiceSettings) {
        self._settings = settings;
    }

    /// Handle one recognised utterance. `now_ms`: a monotonic clock in milliseconds (injected for tests).
    pub fn handle(&mut self, _transcript: &Transcript, _now_ms: u64) -> Vec<Reply> {
        Vec::new()
    }

    /// Typed text ("try a phrase" in the UI): handled exactly like speech.
    pub fn handle_text(&mut self, _text: &str, _now_ms: u64) -> Vec<Reply> {
        Vec::new()
    }

    /// The on-screen Yes/No buttons.
    pub fn answer_confirmation(&mut self, _yes: bool, _now_ms: u64) -> Vec<Reply> {
        Vec::new()
    }

    /// The question waiting for an answer, if any (expires after a short time).
    pub fn pending_confirmation(&self, _now_ms: u64) -> Option<String> {
        None
    }

    /// What can be said right now: derived from the real automations, aliases and settings.
    pub fn commands(&self) -> Vec<CommandExample> {
        Vec::new()
    }
}
