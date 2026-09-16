//! The listening state machine: modes, mute, device loss, echo guard.
//!
//! One worker thread owns every part (microphone, segmenter, recogniser, controller, speaker). The
//! handle ([`Session`]) only sends commands over a channel and reads a few shared values, so all of
//! its methods are cheap and never block on speech recognition (except `stop`/`Drop`, which wait for
//! the worker with a timeout, and `commands`, which waits up to [`COMMANDS_WAIT`]).
//!
//! | state      | entered when                                                        | left when                       |
//! |------------|---------------------------------------------------------------------|---------------------------------|
//! | Off        | the worker has ended (`stop`, drop, or a panic is reported as Error) | -                               |
//! | Idle       | start, a command finished, armed time ran out, unmuted, recovered   | press / wake phrase / mute      |
//! | Listening  | push-to-talk held (and its tail), or armed after the wake phrase    | release tail over, armed expiry |
//! | Processing | recognising / carrying out a command                                | the command is done             |
//! | Speaking   | a reply is spoken aloud (input ignored, then +[`SPEAK_GUARD`])      | speech finished                 |
//! | Muted      | `set_muted(true)` or a voice request to mute / stop listening       | `set_muted(false)`              |
//! | Error(msg) | microphone lost / cannot open                                       | retry works / settings update / next press |
//!
//! Privacy: in always-on mode, speech that does not start with the wake phrase (and arrives while not
//! armed) produces no event, no state change and no log; its text is dropped at once.

use std::{
    collections::VecDeque,
    panic::{catch_unwind, AssertUnwindSafe},
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc::{self, Receiver, RecvTimeoutError, Sender, TryRecvError},
        Arc, Mutex,
    },
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};

use super::{
    controller::{ControlRequest, VoiceController},
    wake::{match_wake, WakeMatch},
    AudioEvent, AudioSource, CommandExample, ListenMode, Recognized, Recognizer, Reply, ReplyKind, Segmented, Segmenter,
    Speaker, Transcript, VoiceEvent, VoiceEvents, VoiceSettings, VoiceState,
};

// ---- constants -----------------------------------------------------------------------------------

/// Push-to-talk: the microphone keeps running this long after the key is released (last syllable).
pub const PTT_TAIL_MS: u64 = 300;
/// Push-to-talk presses shorter than this are ignored (an accidental tap).
pub const PTT_MIN_PRESS_MS: u64 = 250;
/// Safety: a push-to-talk press is ended automatically after this long (a lost key-release; on macOS
/// global shortcuts can miss the release event while another app grabs the keyboard).
pub const PTT_MAX_SECONDS: u64 = 30;
/// After the wake phrase alone, the next utterance is the command for this long.
pub const ARMED_SECONDS: u64 = 8;
/// Input is ignored this long after a spoken reply (room echo).
pub const SPEAK_GUARD_MS: u64 = 400;
/// A spoken reply that has not finished after this long is abandoned.
pub const SPEAK_CAP_SECONDS: u64 = 20;
/// Retry delays after the microphone is lost in always-on mode; afterwards the session stays in Error.
pub const RETRY_BACKOFF_MS: [u64; 3] = [1_000, 2_000, 4_000];
/// `stop()` / drop wait at most this long for the worker.
pub const JOIN_TIMEOUT_MS: u64 = 2_000;
/// Longest audio sent to the recogniser (seconds); more is cut off.
pub const MAX_UTTERANCE_SECONDS: usize = 30;
/// Audio queued while the worker is busy is capped to this many seconds (oldest dropped).
pub const MAX_BACKLOG_SECONDS: usize = 60;
/// How long `commands()` waits for the worker.
pub const COMMANDS_WAIT: Duration = Duration::from_millis(1_500);

const SAMPLE_RATE: usize = 16_000;
const MAX_UTTERANCE_SAMPLES: usize = MAX_UTTERANCE_SECONDS * SAMPLE_RATE;
const MAX_BACKLOG_SAMPLES: usize = MAX_BACKLOG_SECONDS * SAMPLE_RATE;
const MAX_DRAIN_EVENTS: usize = 20_000;

/// Every duration the session uses, so tests can run with tiny values.
#[derive(Debug, Clone)]
pub struct SessionTimings {
    pub ptt_tail: Duration,
    pub ptt_min_press: Duration,
    pub ptt_max: Duration,
    pub armed: Duration,
    pub speak_guard: Duration,
    pub speak_cap: Duration,
    pub backoff: Vec<Duration>,
    /// How often the worker looks at timers and commands while no audio arrives.
    pub tick: Duration,
    pub join_timeout: Duration,
}

impl Default for SessionTimings {
    fn default() -> Self {
        SessionTimings {
            ptt_tail: Duration::from_millis(PTT_TAIL_MS),
            ptt_min_press: Duration::from_millis(PTT_MIN_PRESS_MS),
            ptt_max: Duration::from_secs(PTT_MAX_SECONDS),
            armed: Duration::from_secs(ARMED_SECONDS),
            speak_guard: Duration::from_millis(SPEAK_GUARD_MS),
            speak_cap: Duration::from_secs(SPEAK_CAP_SECONDS),
            backoff: RETRY_BACKOFF_MS.iter().map(|m| Duration::from_millis(*m)).collect(),
            tick: Duration::from_millis(10),
            join_timeout: Duration::from_millis(JOIN_TIMEOUT_MS),
        }
    }
}

// ---- parts and handle ------------------------------------------------------------------------------

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

enum Cmd {
    Press,
    Release,
    Mute(bool),
    Text(String),
    Answer(bool),
    Update(Box<VoiceSettings>),
    Commands(Sender<Vec<CommandExample>>),
    Stop,
}

/// A running voice session on its own worker thread. Dropping it stops listening and releases
/// the microphone. Never call `stop`/drop from an async runtime thread (it may wait up to
/// [`JOIN_TIMEOUT_MS`]); use `spawn_blocking`.
pub struct Session {
    tx: Mutex<Sender<Cmd>>,
    state: Arc<Mutex<VoiceState>>,
    muted: Arc<AtomicBool>,
    settings: Mutex<VoiceSettings>,
    cache: Mutex<Vec<CommandExample>>,
    handle: Option<JoinHandle<()>>,
    done: Mutex<Receiver<()>>,
    join_timeout: Duration,
}

impl Session {
    pub fn start(parts: SessionParts) -> Session {
        Session::start_with(parts, SessionTimings::default())
    }

    pub fn start_with(parts: SessionParts, timings: SessionTimings) -> Session {
        let (tx, rx) = mpsc::channel();
        let (done_tx, done_rx) = mpsc::channel();
        let state = Arc::new(Mutex::new(VoiceState::Idle));
        let muted = Arc::new(AtomicBool::new(false));
        let settings = parts.settings.clone();
        let join_timeout = timings.join_timeout;
        let mut worker = Worker {
            settings: parts.settings,
            language: parts.language,
            source: parts.source,
            recognizer: parts.recognizer,
            segmenter: parts.segmenter,
            speaker: parts.speaker,
            controller: parts.controller,
            events: parts.events,
            cmds: rx,
            shared_state: state.clone(),
            shared_muted: muted.clone(),
            timings,
            t0: Instant::now(),
            audio_rx: None,
            ptt: None,
            armed_until: None,
            speech: false,
            error: None,
            retry_at: None,
            attempt: 0,
            ignore_until: None,
            reset_pending: false,
            next_id: 1,
            recog_error_reported: false,
            muted: false,
            stopping: false,
            deferred: VecDeque::new(),
        };
        let handle = thread::Builder::new().name("voice-session".into()).spawn(move || {
            let result = catch_unwind(AssertUnwindSafe(|| worker.run()));
            worker.finish(result.is_err());
            drop(worker);
            let _ = done_tx.send(());
        });
        let handle = match handle {
            Ok(h) => Some(h),
            Err(_) => {
                *state.lock().unwrap() = VoiceState::Error("Voice control could not start.".into());
                None
            }
        };
        Session {
            tx: Mutex::new(tx),
            state,
            muted,
            settings: Mutex::new(settings),
            cache: Mutex::new(Vec::new()),
            handle,
            done: Mutex::new(done_rx),
            join_timeout,
        }
    }

    fn send(&self, cmd: Cmd) {
        if let Ok(tx) = self.tx.lock() {
            let _ = tx.send(cmd);
        }
    }

    pub fn state(&self) -> VoiceState {
        self.state.lock().map(|s| s.clone()).unwrap_or(VoiceState::Off)
    }
    /// Push-to-talk key (or on-screen button) pressed / released. Repeats are ignored.
    pub fn press(&self) {
        self.send(Cmd::Press);
    }
    pub fn release(&self) {
        self.send(Cmd::Release);
    }
    /// Muting stops consuming audio and releases the microphone.
    pub fn set_muted(&self, muted: bool) {
        self.muted.store(muted, Ordering::SeqCst);
        self.send(Cmd::Mute(muted));
    }
    pub fn is_muted(&self) -> bool {
        self.muted.load(Ordering::SeqCst)
    }
    /// A typed phrase ("try a phrase"): handled exactly like speech (also while muted); the phrase
    /// is shown as `Heard`, replies arrive as events.
    pub fn submit_text(&self, text: &str) {
        self.send(Cmd::Text(text.to_string()));
    }
    /// The on-screen Yes/No buttons.
    pub fn answer_confirmation(&self, yes: bool) {
        self.send(Cmd::Answer(yes));
    }
    /// Apply new settings live where possible (wake phrase, spoken feedback, aliases, confirmations,
    /// push-to-talk vs always-on). Returns true if the microphone, language or engine changed and the
    /// session must be restarted by the caller.
    pub fn update_settings(&self, settings: VoiceSettings) -> bool {
        let restart = {
            let mut cur = match self.settings.lock() {
                Ok(c) => c,
                Err(p) => p.into_inner(),
            };
            let r = cur.microphone != settings.microphone || cur.language != settings.language || cur.engine != settings.engine;
            *cur = settings.clone();
            r
        };
        self.send(Cmd::Update(Box::new(settings)));
        restart
    }
    pub fn commands(&self) -> Vec<CommandExample> {
        let (tx, rx) = mpsc::channel();
        self.send(Cmd::Commands(tx));
        match rx.recv_timeout(COMMANDS_WAIT) {
            Ok(list) => {
                if let Ok(mut c) = self.cache.lock() {
                    *c = list.clone();
                }
                list
            }
            Err(_) => self.cache.lock().map(|c| c.clone()).unwrap_or_default(),
        }
    }
    /// Stop listening now, release the microphone and join the worker (waits at most
    /// [`JOIN_TIMEOUT_MS`]; a worker stuck inside a recogniser or speaker is abandoned).
    pub fn stop(mut self) {
        self.shutdown();
    }

    fn shutdown(&mut self) {
        let Some(handle) = self.handle.take() else { return };
        self.send(Cmd::Stop);
        let finished = match self.done.lock() {
            Ok(d) => !matches!(d.recv_timeout(self.join_timeout), Err(RecvTimeoutError::Timeout)),
            Err(_) => false,
        };
        if finished {
            let _ = handle.join();
        }
        // else: detached; the worker ends (and releases the microphone) as soon as it is free.
    }
}

impl Drop for Session {
    fn drop(&mut self) {
        self.shutdown();
    }
}

// ---- messages (en / ru / de) --------------------------------------------------------------------------

fn tr(lang: &str, en: String, ru: String, de: String) -> String {
    match lang {
        "ru" => ru,
        "de" => de,
        _ => en,
    }
}

// ---- worker --------------------------------------------------------------------------------------------

struct Ptt {
    pressed_at: Instant,
    released_at: Option<Instant>,
    audio: Vec<f32>,
}

struct Worker {
    settings: VoiceSettings,
    language: String,
    source: Box<dyn AudioSource>,
    recognizer: Box<dyn Recognizer>,
    segmenter: Box<dyn Segmenter>,
    speaker: Option<Arc<dyn Speaker>>,
    controller: VoiceController,
    events: VoiceEvents,
    cmds: Receiver<Cmd>,
    shared_state: Arc<Mutex<VoiceState>>,
    shared_muted: Arc<AtomicBool>,
    timings: SessionTimings,
    t0: Instant,
    audio_rx: Option<Receiver<AudioEvent>>,
    ptt: Option<Ptt>,
    armed_until: Option<Instant>,
    speech: bool,
    error: Option<String>,
    retry_at: Option<Instant>,
    attempt: usize,
    ignore_until: Option<Instant>,
    reset_pending: bool,
    next_id: u64,
    recog_error_reported: bool,
    muted: bool,
    stopping: bool,
    deferred: VecDeque<Cmd>,
}

impl Drop for Worker {
    fn drop(&mut self) {
        let _ = catch_unwind(AssertUnwindSafe(|| self.source.stop()));
    }
}

impl Worker {
    fn now_ms(&self) -> u64 {
        self.t0.elapsed().as_millis() as u64
    }

    fn emit(&self, event: VoiceEvent) {
        let _ = catch_unwind(AssertUnwindSafe(|| (self.events)(event)));
    }

    fn set_state(&self, state: VoiceState) {
        let changed = {
            let mut cur = match self.shared_state.lock() {
                Ok(c) => c,
                Err(p) => p.into_inner(),
            };
            if *cur == state {
                false
            } else {
                *cur = state.clone();
                true
            }
        };
        if changed {
            self.emit(VoiceEvent::State { state });
        }
    }

    fn is_armed(&self) -> bool {
        self.armed_until.is_some_and(|t| Instant::now() < t || self.speech)
    }

    /// Show the state the session rests in.
    fn settle(&self) {
        let state = if self.muted {
            VoiceState::Muted
        } else if let Some(e) = &self.error {
            VoiceState::Error(e.clone())
        } else if self.ptt.is_some() || self.is_armed() {
            VoiceState::Listening
        } else {
            VoiceState::Idle
        };
        self.set_state(state);
    }

    fn run(&mut self) {
        // Announce the starting state first; a failing microphone then shows up as an Error event.
        let first = self.shared_state.lock().map(|s| s.clone()).unwrap_or(VoiceState::Idle);
        self.emit(VoiceEvent::State { state: first });
        self.open_always_on();
        self.settle();
        while !self.stopping {
            self.tick();
            while let Some(cmd) = self.next_cmd() {
                self.handle_cmd(cmd);
                if self.stopping {
                    return;
                }
            }
            self.pump();
        }
    }

    fn finish(&mut self, panicked: bool) {
        let _ = catch_unwind(AssertUnwindSafe(|| self.source.stop()));
        self.audio_rx = None;
        let state = if panicked { VoiceState::Error("Voice control stopped unexpectedly. Turn it off and on again.".into()) } else { VoiceState::Off };
        self.set_state(state);
    }

    fn next_cmd(&mut self) -> Option<Cmd> {
        if let Some(c) = self.deferred.pop_front() {
            return Some(c);
        }
        match self.cmds.try_recv() {
            Ok(c) => Some(c),
            Err(TryRecvError::Empty) => None,
            Err(TryRecvError::Disconnected) => {
                self.stopping = true;
                None
            }
        }
    }

    /// Wait up to one tick for audio (or a command) and process what arrived.
    fn pump(&mut self) {
        let tick = self.timings.tick;
        let mut batch: Vec<AudioEvent> = Vec::new();
        match self.audio_rx.as_ref() {
            Some(rx) => {
                match rx.recv_timeout(tick) {
                    Ok(e) => batch.push(e),
                    Err(RecvTimeoutError::Timeout) => {}
                    Err(RecvTimeoutError::Disconnected) => batch.push(AudioEvent::Disconnected),
                }
                while batch.len() < MAX_DRAIN_EVENTS {
                    match rx.try_recv() {
                        Ok(e) => batch.push(e),
                        Err(_) => break,
                    }
                }
            }
            None => match self.cmds.recv_timeout(tick) {
                Ok(c) => self.deferred.push_back(c),
                Err(RecvTimeoutError::Timeout) => {}
                Err(RecvTimeoutError::Disconnected) => self.stopping = true,
            },
        }
        trim_backlog(&mut batch);
        for ev in batch {
            if self.audio_rx.is_none() || self.stopping {
                break; // the source was closed while handling an earlier event
            }
            self.on_audio(ev);
        }
    }

    // ---- timers ------------------------------------------------------------------------------------

    fn tick(&mut self) {
        let now = Instant::now();
        if let Some(t) = self.armed_until {
            if now >= t && !self.speech {
                self.armed_until = None;
                self.settle();
            }
        }
        let mut finalize = false;
        if let Some(p) = &mut self.ptt {
            if p.released_at.is_none() && now.duration_since(p.pressed_at) >= self.timings.ptt_max {
                p.released_at = Some(now); // lost key-release
            }
            finalize = p.released_at.is_some_and(|r| now.duration_since(r) >= self.timings.ptt_tail);
        }
        if finalize {
            self.finalize_ptt();
        }
        if let Some(t) = self.retry_at {
            if now >= t && self.error.is_some() && self.always_on() && !self.muted {
                self.retry_at = None;
                self.open_always_on();
                self.settle();
            }
        }
    }

    fn always_on(&self) -> bool {
        self.settings.mode == ListenMode::AlwaysOn
    }

    // ---- microphone --------------------------------------------------------------------------------

    fn open_source(&mut self) -> Result<(), String> {
        if self.audio_rx.is_some() {
            return Ok(());
        }
        let (tx, rx) = mpsc::channel();
        let device = self.settings.microphone.clone();
        let r = catch_unwind(AssertUnwindSafe(|| self.source.start(device.as_deref(), tx)));
        match r {
            Ok(Ok(())) => {
                self.audio_rx = Some(rx);
                Ok(())
            }
            Ok(Err(e)) => {
                let _ = catch_unwind(AssertUnwindSafe(|| self.source.stop()));
                Err(e)
            }
            Err(_) => Err("the audio driver failed".into()),
        }
    }

    /// Stop the source and return the audio that was still queued.
    fn close_source(&mut self) -> Vec<AudioEvent> {
        let _ = catch_unwind(AssertUnwindSafe(|| self.source.stop()));
        let mut rest = Vec::new();
        if let Some(rx) = self.audio_rx.take() {
            while rest.len() < MAX_DRAIN_EVENTS {
                match rx.try_recv() {
                    Ok(e) => rest.push(e),
                    Err(_) => break,
                }
            }
        }
        rest
    }

    fn open_always_on(&mut self) {
        if self.muted || !self.always_on() || self.audio_rx.is_some() {
            return;
        }
        match self.open_source() {
            Ok(()) => {
                self.error = None;
                self.retry_at = None;
            }
            Err(e) => {
                let msg = tr(
                    &self.language,
                    format!("Cannot use the microphone: {e}"),
                    format!("Не удаётся использовать микрофон: {e}"),
                    format!("Das Mikrofon kann nicht verwendet werden: {e}"),
                );
                self.device_failed(msg);
            }
        }
    }

    fn device_failed(&mut self, msg: String) {
        self.close_source();
        self.ptt = None;
        self.armed_until = None;
        self.speech = false;
        self.segmenter.reset();
        self.error = Some(msg);
        self.retry_at = None;
        if self.always_on() && !self.muted && self.attempt < self.timings.backoff.len() {
            self.retry_at = Some(Instant::now() + self.timings.backoff[self.attempt]);
            self.attempt += 1;
        }
        self.settle();
    }

    // ---- commands ----------------------------------------------------------------------------------

    fn handle_cmd(&mut self, cmd: Cmd) {
        match cmd {
            Cmd::Stop => self.stopping = true,
            Cmd::Press => self.press(),
            Cmd::Release => {
                if let Some(p) = &mut self.ptt {
                    if p.released_at.is_none() {
                        p.released_at = Some(Instant::now());
                    }
                }
            }
            Cmd::Mute(m) => {
                self.shared_muted.store(m, Ordering::SeqCst);
                self.apply_mute(m);
            }
            Cmd::Text(t) => {
                let t = t.trim().to_string();
                if !t.is_empty() {
                    self.run_command(t, None, None);
                }
            }
            Cmd::Answer(yes) => {
                self.set_state(VoiceState::Processing);
                let replies = self.controller.answer_confirmation(yes, self.now_ms());
                self.after_controller(replies);
            }
            Cmd::Update(new) => self.apply_update(*new),
            Cmd::Commands(tx) => {
                let _ = tx.send(self.controller.commands());
            }
        }
    }

    fn apply_mute(&mut self, mute: bool) {
        self.muted = mute;
        if mute {
            self.close_source();
            self.ptt = None;
            self.armed_until = None;
            self.speech = false;
            self.retry_at = None;
            self.segmenter.reset();
        } else {
            self.attempt = 0;
            self.error = None;
            self.open_always_on();
        }
        self.settle();
    }

    fn press(&mut self) {
        let in_guard = self.ignore_until.is_some_and(|t| Instant::now() < t);
        if self.settings.mode != ListenMode::PushToTalk || self.muted || in_guard {
            return;
        }
        if let Some(p) = &mut self.ptt {
            p.released_at = None; // key repeat, or pressed again during the tail: keep recording
            return;
        }
        self.segmenter.reset();
        self.error = None;
        match self.open_source() {
            Ok(()) => self.ptt = Some(Ptt { pressed_at: Instant::now(), released_at: None, audio: Vec::new() }),
            Err(e) => {
                let msg = tr(
                    &self.language,
                    format!("Cannot use the microphone: {e}"),
                    format!("Не удаётся использовать микрофон: {e}"),
                    format!("Das Mikrofon kann nicht verwendet werden: {e}"),
                );
                self.error = Some(msg);
            }
        }
        self.settle();
    }

    fn apply_update(&mut self, new: VoiceSettings) {
        let old_mode = self.settings.mode;
        self.settings = new.clone();
        self.controller.update_settings(new);
        if old_mode != self.settings.mode {
            self.close_source();
            self.ptt = None;
            self.armed_until = None;
            self.speech = false;
            self.segmenter.reset();
            self.error = None;
            self.retry_at = None;
            self.attempt = 0;
            self.open_always_on();
        } else if self.error.is_some() && self.always_on() && !self.muted {
            self.attempt = 0;
            self.error = None;
            self.open_always_on();
        }
        self.settle();
    }

    // ---- audio -------------------------------------------------------------------------------------

    fn on_audio(&mut self, ev: AudioEvent) {
        match ev {
            AudioEvent::Disconnected => {
                let msg = tr(
                    &self.language,
                    "The microphone was disconnected.".into(),
                    "Микрофон отключён.".into(),
                    "Das Mikrofon wurde getrennt.".into(),
                );
                self.device_failed(msg);
            }
            AudioEvent::Error(e) => {
                let msg = tr(
                    &self.language,
                    format!("Microphone problem: {e}"),
                    format!("Проблема с микрофоном: {e}"),
                    format!("Problem mit dem Mikrofon: {e}"),
                );
                self.device_failed(msg);
            }
            AudioEvent::Chunk(chunk) => {
                if self.muted {
                    return;
                }
                if self.ignore_until.is_some_and(|t| Instant::now() < t) {
                    self.reset_pending = true;
                    return;
                }
                if self.reset_pending {
                    self.segmenter.reset();
                    self.reset_pending = false;
                    self.ignore_until = None;
                }
                self.attempt = 0;
                self.feed(&chunk);
            }
        }
    }

    fn feed(&mut self, chunk: &[f32]) {
        let seg = self.segmenter.feed(chunk);
        self.take_segments(seg);
    }

    fn take_segments(&mut self, seg: Segmented) {
        match self.settings.mode {
            ListenMode::PushToTalk => {
                if let Some(p) = &mut self.ptt {
                    for u in seg.utterances {
                        append_capped(&mut p.audio, &u);
                    }
                }
            }
            ListenMode::AlwaysOn => {
                self.speech = seg.speech_in_progress;
                for u in seg.utterances {
                    if self.audio_rx.is_none() || self.stopping {
                        break;
                    }
                    self.on_utterance(u);
                }
            }
        }
    }

    fn finalize_ptt(&mut self) {
        let Some(p) = self.ptt.take() else { return };
        let released = p.released_at.unwrap_or_else(Instant::now);
        let mut audio = p.audio;
        for ev in self.close_source() {
            if let AudioEvent::Chunk(c) = ev {
                let seg = self.segmenter.feed(&c);
                for u in seg.utterances {
                    append_capped(&mut audio, &u);
                }
            }
        }
        for u in self.segmenter.flush() {
            append_capped(&mut audio, &u);
        }
        self.segmenter.reset();
        if released.duration_since(p.pressed_at) < self.timings.ptt_min_press || audio.is_empty() {
            self.settle();
            return;
        }
        self.set_state(VoiceState::Processing);
        match self.recognize(&audio) {
            Some(r) => self.run_command(r.text, r.confidence, r.language),
            None => self.settle(),
        }
    }

    fn on_utterance(&mut self, mut audio: Vec<f32>) {
        audio.truncate(MAX_UTTERANCE_SAMPLES);
        let armed = self.is_armed();
        if armed {
            self.set_state(VoiceState::Processing);
        }
        let Some(r) = self.recognize(&audio) else {
            self.settle();
            return;
        };
        let command = match match_wake(&r.text, &self.settings.wake_phrase) {
            WakeMatch::Command(rest) => Some(rest),
            WakeMatch::Armed => {
                self.armed_until = Some(Instant::now() + self.timings.armed);
                None
            }
            WakeMatch::No if armed => Some(r.text.trim().to_string()),
            WakeMatch::No => None, // privacy: not for us, dropped without a trace
        };
        match command {
            Some(text) if !text.is_empty() => {
                self.armed_until = None;
                self.run_command(text, r.confidence, r.language);
            }
            _ => self.settle(),
        }
    }

    /// Speech to text. `None`: nothing intelligible, or an error (reported once per streak).
    fn recognize(&mut self, audio: &[f32]) -> Option<Recognized> {
        match self.recognizer.transcribe(audio, &self.language) {
            Ok(mut r) => {
                self.recog_error_reported = false;
                r.text = r.text.trim().to_string();
                if r.text.is_empty() {
                    None
                } else {
                    Some(r)
                }
            }
            Err(e) => {
                if !self.recog_error_reported {
                    self.recog_error_reported = true;
                    let text = tr(
                        &self.language,
                        format!("Speech recognition failed: {e}"),
                        format!("Не удалось распознать речь: {e}"),
                        format!("Die Spracherkennung ist fehlgeschlagen: {e}"),
                    );
                    self.emit(VoiceEvent::Reply { reply: Reply { kind: ReplyKind::Problem, text, speak: false } });
                }
                None
            }
        }
    }

    // ---- commands and replies ---------------------------------------------------------------------

    fn run_command(&mut self, text: String, confidence: Option<f32>, language: Option<String>) {
        self.set_state(VoiceState::Processing);
        self.emit(VoiceEvent::Heard { text: text.clone(), confidence });
        let id = self.next_id;
        self.next_id += 1;
        let replies = self.controller.handle(&Transcript { id, text, confidence, language }, self.now_ms());
        self.after_controller(replies);
    }

    fn after_controller(&mut self, replies: Vec<Reply>) {
        let mut confirm = false;
        for r in &replies {
            self.emit(VoiceEvent::Reply { reply: r.clone() });
            if r.kind == ReplyKind::Confirm {
                confirm = true;
                self.emit(VoiceEvent::Confirm { prompt: r.text.clone() });
            }
        }
        let mut extra = Vec::new();
        for c in self.controller.take_control_requests() {
            match c {
                ControlRequest::Mute => {
                    self.shared_muted.store(true, Ordering::SeqCst);
                    self.apply_mute(true);
                }
                ControlRequest::StopListening => {
                    self.shared_muted.store(true, Ordering::SeqCst);
                    self.apply_mute(true);
                    let text = tr(
                        &self.language,
                        "Listening is paused. Turn it back on with the microphone button in LocalFlow or the push-to-talk hotkey.".into(),
                        "Прослушивание приостановлено. Включите его снова кнопкой микрофона в LocalFlow или горячей клавишей.".into(),
                        "Das Zuhören ist pausiert. Schalten Sie es mit der Mikrofon-Taste in LocalFlow oder dem Tastenkürzel wieder ein.".into(),
                    );
                    let reply = Reply { kind: ReplyKind::Info, text, speak: false };
                    self.emit(VoiceEvent::Reply { reply: reply.clone() });
                    extra.push(reply);
                }
                ControlRequest::Unmute => {
                    self.shared_muted.store(false, Ordering::SeqCst);
                    self.apply_mute(false);
                }
                ControlRequest::SpokenFeedback(on) => {
                    self.settings.spoken_feedback = on;
                    self.controller.update_settings(self.settings.clone());
                    self.emit(VoiceEvent::Settings { settings: self.settings.clone() });
                }
            }
        }
        if self.settings.spoken_feedback && self.speaker.is_some() {
            for r in replies.iter().chain(extra.iter()) {
                if r.speak && !self.stopping {
                    self.speak_blocking(&r.text);
                }
            }
        }
        // A question can be answered in the next breath without the wake phrase.
        if confirm && self.always_on() && !self.muted && !self.stopping {
            self.armed_until = Some(Instant::now() + self.timings.armed);
        }
        self.settle();
    }

    /// Speak one reply. The microphone input is ignored while it plays and for a short guard time
    /// afterwards. The speaker runs on a helper thread so `stop`, mute and the cap still work.
    fn speak_blocking(&mut self, text: &str) {
        let Some(speaker) = self.speaker.clone() else { return };
        self.set_state(VoiceState::Speaking);
        self.segmenter.reset();
        self.speech = false;
        self.discard_audio();
        let (dtx, drx) = mpsc::channel();
        let text = text.to_string();
        let spawned = thread::Builder::new().name("voice-speak".into()).spawn(move || {
            let _ = catch_unwind(AssertUnwindSafe(|| speaker.speak(&text)));
            let _ = dtx.send(());
        });
        if spawned.is_ok() {
            let deadline = Instant::now() + self.timings.speak_cap;
            loop {
                match drx.recv_timeout(self.timings.tick) {
                    Ok(()) | Err(RecvTimeoutError::Disconnected) => break,
                    Err(RecvTimeoutError::Timeout) => {}
                }
                if Instant::now() >= deadline {
                    break; // abandoned
                }
                loop {
                    match self.cmds.try_recv() {
                        Ok(Cmd::Stop) => self.stopping = true,
                        Ok(Cmd::Mute(m)) => {
                            self.shared_muted.store(m, Ordering::SeqCst);
                            self.apply_mute(m);
                            self.set_state(VoiceState::Speaking);
                        }
                        Ok(other) => self.deferred.push_back(other),
                        Err(TryRecvError::Empty) => break,
                        Err(TryRecvError::Disconnected) => {
                            self.stopping = true;
                            break;
                        }
                    }
                }
                if self.stopping {
                    break;
                }
                self.discard_audio();
            }
        }
        self.segmenter.reset();
        self.discard_audio();
        self.ignore_until = Some(Instant::now() + self.timings.speak_guard);
        self.reset_pending = true;
    }

    /// Throw away queued audio (it may contain our own voice). Device-loss events are kept.
    fn discard_audio(&mut self) {
        let mut lost: Option<AudioEvent> = None;
        if let Some(rx) = &self.audio_rx {
            for _ in 0..MAX_DRAIN_EVENTS {
                match rx.try_recv() {
                    Ok(AudioEvent::Chunk(_)) => {}
                    Ok(e) => lost = Some(e),
                    Err(_) => break,
                }
            }
        }
        if let Some(e) = lost {
            self.on_audio(e);
        }
    }
}

fn append_capped(dst: &mut Vec<f32>, src: &[f32]) {
    let room = MAX_UTTERANCE_SAMPLES.saturating_sub(dst.len());
    dst.extend_from_slice(&src[..src.len().min(room)]);
}

/// Keep at most [`MAX_BACKLOG_SAMPLES`] of queued audio, dropping the oldest chunks.
fn trim_backlog(batch: &mut Vec<AudioEvent>) {
    let total: usize = batch.iter().map(|e| if let AudioEvent::Chunk(c) = e { c.len() } else { 0 }).sum();
    if total <= MAX_BACKLOG_SAMPLES {
        return;
    }
    let mut excess = total - MAX_BACKLOG_SAMPLES;
    batch.retain(|e| match e {
        AudioEvent::Chunk(c) if excess > 0 => {
            excess = excess.saturating_sub(c.len());
            false
        }
        _ => true,
    });
}

// ---- energy segmenter -------------------------------------------------------------------------------------

const FRAME: usize = 160; // 10 ms

/// A simple energy-threshold voice-activity segmenter: used by tests and as a fallback when the real
/// VAD model is unavailable. Deterministic; works on any chunk size.
pub struct EnergySegmenter {
    threshold: f32,
    preroll: usize,
    min_speech: usize,
    hang: usize,
    max_len: usize,
    partial: Vec<f32>,
    pre: VecDeque<f32>,
    active: Vec<f32>,
    in_speech: bool,
    silent_run: usize,
}

impl Default for EnergySegmenter {
    fn default() -> Self {
        EnergySegmenter::with_config(0.02, 300, 200, 600, 15_000)
    }
}

impl EnergySegmenter {
    pub fn new() -> Self {
        Self::default()
    }

    /// `threshold_rms`: frame RMS counted as speech; times in milliseconds.
    pub fn with_config(threshold_rms: f32, preroll_ms: usize, min_speech_ms: usize, silence_hang_ms: usize, max_ms: usize) -> Self {
        let ms = |v: usize| v * SAMPLE_RATE / 1000;
        EnergySegmenter {
            threshold: threshold_rms,
            preroll: ms(preroll_ms),
            min_speech: ms(min_speech_ms),
            hang: ms(silence_hang_ms).max(FRAME),
            max_len: ms(max_ms).max(FRAME),
            partial: Vec::new(),
            pre: VecDeque::new(),
            active: Vec::new(),
            in_speech: false,
            silent_run: 0,
        }
    }

    fn finish(&mut self, out: &mut Vec<Vec<f32>>) {
        let speech = self.active.len().saturating_sub(self.silent_run);
        let audio = std::mem::take(&mut self.active);
        if speech >= self.min_speech && !audio.is_empty() {
            out.push(audio);
        }
        self.in_speech = false;
        self.silent_run = 0;
    }

    fn frame(&mut self, frame: &[f32], out: &mut Vec<Vec<f32>>) {
        let clean = |s: f32| if s.is_finite() { s } else { 0.0 };
        let rms = (frame.iter().map(|s| clean(*s).powi(2)).sum::<f32>() / frame.len().max(1) as f32).sqrt();
        let loud = rms >= self.threshold;
        if !self.in_speech {
            if loud {
                self.in_speech = true;
                self.active = self.pre.drain(..).collect();
                self.active.extend(frame.iter().map(|s| clean(*s)));
                self.silent_run = 0;
            } else {
                self.pre.extend(frame.iter().map(|s| clean(*s)));
                while self.pre.len() > self.preroll {
                    self.pre.pop_front();
                }
            }
            return;
        }
        self.active.extend(frame.iter().map(|s| clean(*s)));
        if loud {
            self.silent_run = 0;
        } else {
            self.silent_run += frame.len();
        }
        if self.silent_run >= self.hang || self.active.len() >= self.max_len {
            self.finish(out);
        }
    }
}

impl Segmenter for EnergySegmenter {
    fn feed(&mut self, chunk: &[f32]) -> Segmented {
        let mut out = Vec::new();
        self.partial.extend_from_slice(chunk);
        let mut start = 0;
        while self.partial.len() - start >= FRAME {
            let frame: Vec<f32> = self.partial[start..start + FRAME].to_vec();
            self.frame(&frame, &mut out);
            start += FRAME;
        }
        self.partial.drain(..start);
        Segmented { utterances: out, speech_in_progress: self.in_speech }
    }

    fn flush(&mut self) -> Vec<Vec<f32>> {
        let mut out = Vec::new();
        if self.in_speech {
            self.finish(&mut out);
        }
        self.partial.clear();
        self.pre.clear();
        out
    }

    fn reset(&mut self) {
        self.partial.clear();
        self.pre.clear();
        self.active.clear();
        self.in_speech = false;
        self.silent_run = 0;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tone(ms: usize) -> Vec<f32> {
        (0..ms * 16).map(|i| if i % 2 == 0 { 0.3 } else { -0.3 }).collect()
    }
    fn quiet(ms: usize) -> Vec<f32> {
        vec![0.0; ms * 16]
    }

    #[test]
    fn segments_speech_with_pre_roll() {
        let mut s = EnergySegmenter::with_config(0.02, 100, 100, 200, 5000);
        let mut audio = quiet(500);
        audio.extend(tone(400));
        audio.extend(quiet(400));
        let r = s.feed(&audio);
        assert_eq!(r.utterances.len(), 1);
        assert!(!r.speech_in_progress);
        // 100 ms pre-roll + 400 ms speech + 200 ms hang
        assert_eq!(r.utterances[0].len(), (100 + 400 + 200) * 16);
    }

    #[test]
    fn any_chunk_size_gives_the_same_result() {
        let mut audio = quiet(300);
        audio.extend(tone(300));
        audio.extend(quiet(700));
        let mut whole = EnergySegmenter::with_config(0.02, 100, 100, 200, 5000);
        let a = whole.feed(&audio).utterances;
        let mut parts = EnergySegmenter::with_config(0.02, 100, 100, 200, 5000);
        let mut b = Vec::new();
        for c in audio.chunks(37) {
            b.extend(parts.feed(c).utterances);
        }
        assert_eq!(a, b);
    }

    #[test]
    fn short_blips_are_dropped_and_flush_returns_open_speech() {
        let mut s = EnergySegmenter::with_config(0.02, 0, 100, 200, 5000);
        let mut audio = tone(20);
        audio.extend(quiet(400));
        assert!(s.feed(&audio).utterances.is_empty());
        let r = s.feed(&tone(300));
        assert!(r.speech_in_progress && r.utterances.is_empty());
        assert_eq!(s.flush().len(), 1);
        assert!(s.flush().is_empty());
    }

    #[test]
    fn long_speech_is_cut_and_odd_samples_do_not_panic() {
        let mut s = EnergySegmenter::with_config(0.02, 0, 100, 200, 1000);
        let r = s.feed(&tone(2500));
        assert_eq!(r.utterances.len(), 2);
        let r = s.feed(&[f32::NAN, f32::INFINITY, -1e30, 0.0]);
        assert!(r.utterances.is_empty());
        s.reset();
        assert!(!s.feed(&[]).speech_in_progress);
    }

    #[test]
    fn backlog_is_trimmed_from_the_oldest_end() {
        let mut batch: Vec<AudioEvent> = (0..100).map(|_| AudioEvent::Chunk(vec![0.0; SAMPLE_RATE])).collect();
        batch.push(AudioEvent::Disconnected);
        trim_backlog(&mut batch);
        let samples: usize = batch.iter().map(|e| if let AudioEvent::Chunk(c) = e { c.len() } else { 0 }).sum();
        assert!(samples <= MAX_BACKLOG_SAMPLES);
        assert_eq!(batch.last(), Some(&AudioEvent::Disconnected));
    }
}
