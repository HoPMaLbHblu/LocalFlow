//! Microphone devices and capture. OWNER: audio agent. Placeholder with the agreed interface.

use super::{AudioSource, InputDevice};

/// Input devices (names only; never opens a stream).
pub fn list_input_devices() -> Result<Vec<InputDevice>, String> {
    Ok(Vec::new())
}

/// The real microphone source for this platform.
pub fn create_audio_source() -> Result<Box<dyn AudioSource>, String> {
    Err("microphone capture is not implemented yet".into())
}

/// A short, user-initiated microphone check for the setup screen: listens for `seconds` and reports
/// the level. Never called by tests (they use mocks) and never stores audio.
pub fn test_input(_device: Option<&str>, _seconds: f32) -> Result<super::MicTest, String> {
    Err("microphone capture is not implemented yet".into())
}

/// Spoken replies through the system voice (the same one `speak()` in scripts uses).
pub fn create_speaker() -> std::sync::Arc<dyn super::Speaker> {
    struct Silent;
    impl super::Speaker for Silent {
        fn speak(&self, _text: &str) -> Result<(), String> {
            Ok(())
        }
    }
    std::sync::Arc::new(Silent)
}
