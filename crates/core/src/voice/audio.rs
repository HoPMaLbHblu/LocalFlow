//! Microphone devices, capture and spoken replies.
//!
//! Capture (feature `voice-engine`) runs on its own thread: cpal initialises COM per thread on
//! Windows, so the stream is created, owned and dropped there and no COM state leaks into the
//! caller. Audio is converted to f32, mixed down to mono, resampled to 16 kHz and sent as chunks of
//! about 80 ms. Nothing is written to disk and neither audio nor text is logged.
//!
//! Without the feature the same functions exist and report "this build has no voice engine".

use super::{AudioSource, InputDevice, MicTest};

/// Shown whenever the engine is compiled out.
pub const NO_ENGINE: &str = "this build has no voice engine";

/// A microphone check "heard sound" when the peak is at least this (about -34 dBFS) ...
pub const HEARD_SOUND_PEAK: f32 = 0.02;
/// ... or the RMS level is at least this (about -50 dBFS).
pub const HEARD_SOUND_RMS: f32 = 0.003;

/// Longest text spoken in one reply (characters). Longer replies are cut at a word boundary.
const MAX_SPOKEN_CHARS: usize = 400;
/// Longest time one spoken reply may take.
const SPEAK_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(30);

/// Level measurement of a block of 16 kHz mono samples (used by `test_input`).
pub fn analyse_level(samples: &[f32]) -> MicTest {
    let mut peak = 0.0f32;
    let mut sum = 0.0f64;
    for &s in samples {
        let a = if s.is_finite() { s.abs() } else { 0.0 };
        peak = peak.max(a);
        sum += (a as f64) * (a as f64);
    }
    let rms = if samples.is_empty() { 0.0 } else { (sum / samples.len() as f64).sqrt() as f32 };
    let peak = peak.min(1.0);
    MicTest { peak, rms, heard_sound: peak >= HEARD_SOUND_PEAK || rms >= HEARD_SOUND_RMS }
}

/// Cut a reply to the spoken-length cap at a word boundary.
fn cap_text(text: &str) -> String {
    let text = text.trim();
    if text.chars().count() <= MAX_SPOKEN_CHARS {
        return text.to_string();
    }
    let cut: String = text.chars().take(MAX_SPOKEN_CHARS).collect();
    match cut.rfind(char::is_whitespace) {
        Some(i) if i > MAX_SPOKEN_CHARS / 2 => cut[..i].trim_end().to_string(),
        _ => cut,
    }
}

/// Spoken replies through the system voice (the same one `speak()` in scripts uses). Blocks until
/// the speech has finished; an error means the system has no usable voice.
pub fn create_speaker() -> std::sync::Arc<dyn super::Speaker> {
    struct SystemSpeaker;
    impl super::Speaker for SystemSpeaker {
        fn speak(&self, text: &str) -> Result<(), String> {
            let text = cap_text(text);
            if text.is_empty() {
                return Ok(());
            }
            crate::lua::tools::speak(&text, SPEAK_TIMEOUT)
                .map_err(|e| format!("spoken replies are not available on this PC: {e}"))
        }
    }
    std::sync::Arc::new(SystemSpeaker)
}

#[cfg(feature = "voice-engine")]
pub use real::*;
#[cfg(not(feature = "voice-engine"))]
pub use fallback::*;

#[cfg(not(feature = "voice-engine"))]
mod fallback {
    use super::*;

    /// Input devices (names only; never opens a stream).
    pub fn list_input_devices() -> Result<Vec<InputDevice>, String> {
        Ok(Vec::new())
    }

    pub fn create_audio_source() -> Result<Box<dyn AudioSource>, String> {
        Err(NO_ENGINE.into())
    }

    pub fn test_input(_device: Option<&str>, _seconds: f32) -> Result<MicTest, String> {
        Err(NO_ENGINE.into())
    }
}

#[cfg(feature = "voice-engine")]
mod real {
    use std::sync::mpsc::{self, Receiver, Sender};
    use std::thread::JoinHandle;
    use std::time::{Duration, Instant};

    use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
    use cpal::{FromSample, Sample, SampleFormat, SizedSample};

    use super::*;
    use crate::voice::AudioEvent;

    /// Target format of everything the voice layer sees.
    const TARGET_RATE: u32 = 16_000;
    /// Chunk size sent to the sink: 80 ms.
    const CHUNK_SAMPLES: usize = 1_280;

    // ---- friendly errors -----------------------------------------------------------------------

    /// Turn a cpal stream error into what the session should hear about, or `None` for harmless
    /// notices (buffer under/overruns, real-time scheduling refused).
    pub fn map_stream_error(error: &cpal::Error) -> Option<AudioEvent> {
        use cpal::ErrorKind as K;
        Some(match error.kind() {
            // The device went away or the stream must be rebuilt (a rerouted default device is
            // reported the same way so the session simply reopens the microphone).
            K::DeviceNotAvailable | K::StreamInvalidated | K::DeviceChanged => AudioEvent::Disconnected,
            K::Xrun | K::RealtimeDenied => return None,
            kind => AudioEvent::Error(friendly_open_error(kind, error)),
        })
    }

    /// A user-facing sentence for a failure to use the microphone.
    pub fn friendly_open_error(kind: cpal::ErrorKind, error: &cpal::Error) -> String {
        use cpal::ErrorKind as K;
        match kind {
            K::DeviceNotAvailable => "The microphone is not available. It may have been unplugged or disabled.".into(),
            K::StreamInvalidated | K::DeviceChanged => "The microphone changed. Voice control will reconnect.".into(),
            K::HostUnavailable => "The audio system is not available on this PC.".into(),
            K::PermissionDenied => {
                if cfg!(target_os = "macos") {
                    "Microphone access was denied. Allow LocalFlow in System Settings > Privacy & Security > Microphone."
                        .into()
                } else {
                    "Microphone access was denied. In Windows Settings > Privacy & security > Microphone, turn on \
                     \"Let desktop apps access your microphone\"."
                        .into()
                }
            }
            K::DeviceBusy => "The microphone is in use by another program. Close it there and try again.".into(),
            K::UnsupportedConfig | K::InvalidInput => {
                "This microphone cannot be used with LocalFlow (unsupported audio format).".into()
            }
            _ => format!("The microphone failed: {error}"),
        }
    }

    // ---- devices -----------------------------------------------------------------------------

    fn device_name(device: &cpal::Device) -> Option<String> {
        device.description().ok().map(|d| d.name().to_string())
    }

    /// Input devices (names only; never opens a stream).
    pub fn list_input_devices() -> Result<Vec<InputDevice>, String> {
        let host = cpal::default_host();
        let default = host.default_input_device().and_then(|d| device_name(&d));
        let devices = host.input_devices().map_err(|e| friendly_open_error(e.kind(), &e))?;
        let mut out: Vec<InputDevice> = Vec::new();
        for device in devices {
            if let Some(name) = device_name(&device) {
                if out.iter().any(|d| d.name == name) {
                    continue;
                }
                let is_default = default.as_deref() == Some(name.as_str());
                out.push(InputDevice { name, is_default });
            }
        }
        Ok(out)
    }

    fn find_device(host: &cpal::Host, name: Option<&str>) -> Result<cpal::Device, String> {
        match name {
            None => host
                .default_input_device()
                .ok_or_else(|| "No microphone was found. Plug one in or enable it in the sound settings.".to_string()),
            Some(wanted) => {
                let devices = host.input_devices().map_err(|e| friendly_open_error(e.kind(), &e))?;
                for device in devices {
                    if device_name(&device).as_deref() == Some(wanted) {
                        return Ok(device);
                    }
                }
                Err(format!("The microphone \"{wanted}\" was not found. It may be unplugged."))
            }
        }
    }

    // ---- sample conversion ----------------------------------------------------------------------

    /// Interleaved device samples in, 16 kHz mono f32 chunks out.
    pub struct Mono16k {
        channels: usize,
        resampler: Option<sherpa_onnx::LinearResampler>,
        pending: Vec<f32>,
    }

    impl Mono16k {
        pub fn new(rate: u32, channels: u16) -> Result<Self, String> {
            let resampler = if rate == TARGET_RATE {
                None
            } else {
                Some(
                    sherpa_onnx::LinearResampler::create(rate as i32, TARGET_RATE as i32)
                        .ok_or_else(|| format!("cannot resample {rate} Hz audio"))?,
                )
            };
            Ok(Mono16k { channels: usize::from(channels.max(1)), resampler, pending: Vec::new() })
        }

        /// Add one callback's worth of interleaved f32 samples; returns every complete chunk.
        pub fn push(&mut self, interleaved: &[f32]) -> Vec<Vec<f32>> {
            let mono: Vec<f32> = if self.channels == 1 {
                interleaved.to_vec()
            } else {
                interleaved.chunks_exact(self.channels).map(|f| f.iter().sum::<f32>() / self.channels as f32).collect()
            };
            match &self.resampler {
                Some(r) => {
                    let out = r.resample(&mono, false);
                    self.pending.extend(out);
                }
                None => self.pending.extend(mono),
            }
            let mut chunks = Vec::new();
            while self.pending.len() >= CHUNK_SAMPLES {
                let rest = self.pending.split_off(CHUNK_SAMPLES);
                chunks.push(std::mem::replace(&mut self.pending, rest));
            }
            chunks
        }
    }

    fn build_stream<T>(
        device: &cpal::Device,
        config: cpal::StreamConfig,
        mut conv: Mono16k,
        sink: Sender<AudioEvent>,
    ) -> Result<cpal::Stream, cpal::Error>
    where
        T: SizedSample + Send + 'static,
        f32: FromSample<T>,
    {
        let data_sink = sink.clone();
        let mut scratch: Vec<f32> = Vec::new();
        device.build_input_stream::<T, _, _>(
            config,
            move |data: &[T], _| {
                scratch.clear();
                scratch.extend(data.iter().map(|s| f32::from_sample(*s)));
                for chunk in conv.push(&scratch) {
                    if data_sink.send(AudioEvent::Chunk(chunk)).is_err() {
                        break;
                    }
                }
            },
            move |error| {
                if let Some(event) = map_stream_error(&error) {
                    let _ = sink.send(event);
                }
            },
            None,
        )
    }

    // ---- the capture thread -----------------------------------------------------------------------

    /// Capture from a microphone on a dedicated thread.
    #[derive(Default)]
    pub struct CpalSource {
        thread: Option<JoinHandle<()>>,
        stop: Option<Sender<()>>,
    }

    impl CpalSource {
        pub fn new() -> Self {
            Self::default()
        }
    }

    fn capture_thread(
        device_name: Option<String>,
        sink: Sender<AudioEvent>,
        ready: Sender<Result<(), String>>,
        stop: Receiver<()>,
    ) {
        let host = cpal::default_host();
        let opened = (|| -> Result<cpal::Stream, String> {
            let device = find_device(&host, device_name.as_deref())?;
            let supported = device.default_input_config().map_err(|e| friendly_open_error(e.kind(), &e))?;
            let format = supported.sample_format();
            let config: cpal::StreamConfig = supported.config();
            let conv = Mono16k::new(config.sample_rate, config.channels)?;
            let sink = sink.clone();
            let stream = match format {
                SampleFormat::I8 => build_stream::<i8>(&device, config, conv, sink),
                SampleFormat::I16 => build_stream::<i16>(&device, config, conv, sink),
                SampleFormat::I24 => build_stream::<cpal::I24>(&device, config, conv, sink),
                SampleFormat::I32 => build_stream::<i32>(&device, config, conv, sink),
                SampleFormat::I64 => build_stream::<i64>(&device, config, conv, sink),
                SampleFormat::U8 => build_stream::<u8>(&device, config, conv, sink),
                SampleFormat::U16 => build_stream::<u16>(&device, config, conv, sink),
                SampleFormat::U24 => build_stream::<cpal::U24>(&device, config, conv, sink),
                SampleFormat::U32 => build_stream::<u32>(&device, config, conv, sink),
                SampleFormat::U64 => build_stream::<u64>(&device, config, conv, sink),
                SampleFormat::F32 => build_stream::<f32>(&device, config, conv, sink),
                SampleFormat::F64 => build_stream::<f64>(&device, config, conv, sink),
                other => return Err(format!("This microphone uses an unsupported audio format ({other:?}).")),
            }
            .map_err(|e| friendly_open_error(e.kind(), &e))?;
            stream.play().map_err(|e| friendly_open_error(e.kind(), &e))?;
            Ok(stream)
        })();
        match opened {
            Ok(stream) => {
                let _ = ready.send(Ok(()));
                // Block until stop() (or the source is dropped); the stream stays alive meanwhile.
                let _ = stop.recv();
                drop(stream);
            }
            Err(message) => {
                let _ = ready.send(Err(message));
            }
        }
        // `host` and the COM state cpal set up on this thread go away with the thread.
    }

    impl AudioSource for CpalSource {
        fn start(&mut self, device: Option<&str>, sink: Sender<AudioEvent>) -> Result<(), String> {
            self.stop();
            let (ready_tx, ready_rx) = mpsc::channel();
            let (stop_tx, stop_rx) = mpsc::channel();
            let name = device.map(str::to_string);
            let handle = std::thread::Builder::new()
                .name("localflow-voice-capture".into())
                .spawn(move || capture_thread(name, sink, ready_tx, stop_rx))
                .map_err(|e| format!("cannot start microphone capture: {e}"))?;
            match ready_rx.recv_timeout(Duration::from_secs(15)) {
                Ok(Ok(())) => {
                    self.thread = Some(handle);
                    self.stop = Some(stop_tx);
                    Ok(())
                }
                Ok(Err(message)) => {
                    let _ = handle.join();
                    Err(message)
                }
                Err(_) => {
                    // The driver hung while opening: ask the thread to quit when it ever returns.
                    drop(stop_tx);
                    Err("The microphone did not respond. Another program may be blocking it.".into())
                }
            }
        }

        fn stop(&mut self) {
            if let Some(stop) = self.stop.take() {
                let _ = stop.send(());
            }
            if let Some(thread) = self.thread.take() {
                let _ = thread.join();
            }
        }
    }

    impl Drop for CpalSource {
        fn drop(&mut self) {
            self.stop();
        }
    }

    /// The real microphone source for this platform.
    pub fn create_audio_source() -> Result<Box<dyn AudioSource>, String> {
        Ok(Box::new(CpalSource::new()))
    }

    /// A short, user-initiated microphone check for the setup screen: listens for `seconds` and
    /// reports the level. Never called by tests (they use mocks) and never stores audio.
    pub fn test_input(device: Option<&str>, seconds: f32) -> Result<MicTest, String> {
        let seconds = if seconds.is_finite() { seconds.clamp(0.3, 10.0) } else { 2.0 };
        let (tx, rx) = mpsc::channel();
        let mut source = CpalSource::new();
        source.start(device, tx)?;
        let started = Instant::now();
        let wanted = (seconds * TARGET_RATE as f32) as usize;
        let mut samples: Vec<f32> = Vec::with_capacity(wanted);
        let mut failure = None;
        while samples.len() < wanted {
            let left = Duration::from_secs_f32(seconds + 3.0).saturating_sub(started.elapsed());
            match rx.recv_timeout(left) {
                Ok(AudioEvent::Chunk(c)) => samples.extend(c),
                Ok(AudioEvent::Disconnected) => {
                    failure = Some("The microphone was disconnected during the test.".to_string());
                    break;
                }
                Ok(AudioEvent::Error(e)) => {
                    failure = Some(e);
                    break;
                }
                Err(_) => {
                    failure = Some("No audio arrived from the microphone.".to_string());
                    break;
                }
            }
        }
        source.stop();
        match failure {
            Some(message) => Err(message),
            None => Ok(analyse_level(&samples)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn level_of_silence_and_tone() {
        assert!(!analyse_level(&[0.0; 1000]).heard_sound);
        assert!(!analyse_level(&[]).heard_sound);
        let tone: Vec<f32> = (0..1600).map(|i| 0.3 * (i as f32 * 0.2).sin()).collect();
        let m = analyse_level(&tone);
        assert!(m.heard_sound && m.peak > 0.29 && m.rms > 0.19 && m.rms < 0.23);
        // Far-off-scale or invalid samples never panic or exceed 1.0.
        let m = analyse_level(&[f32::NAN, 5.0]);
        assert_eq!(m.peak, 1.0);
    }

    #[test]
    fn spoken_text_is_capped_at_a_word() {
        let long = "word ".repeat(300);
        let capped = cap_text(&long);
        assert!(capped.chars().count() <= MAX_SPOKEN_CHARS);
        assert!(capped.ends_with("word"));
        assert_eq!(cap_text("  hi  "), "hi");
    }

    #[cfg(not(feature = "voice-engine"))]
    #[test]
    fn fallback_reports_no_engine() {
        assert!(list_input_devices().unwrap().is_empty());
        assert_eq!(create_audio_source().err().unwrap(), NO_ENGINE);
        assert_eq!(test_input(None, 1.0).err().unwrap(), NO_ENGINE);
    }
}
