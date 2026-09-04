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
