//! The voice session state machine, driven entirely by mocks: no microphone, no speech engine, no
//! speaker is ever opened. Time-dependent behaviour uses `SessionTimings` with tiny values.

use std::{
    collections::VecDeque,
    sync::{
        mpsc::Sender,
        Arc, Mutex,
    },
    thread,
    time::{Duration, Instant},
};

use localflow_core::voice::{
    controller::VoiceController,
    session::{EnergySegmenter, Session, SessionParts, SessionTimings},
    AudioEvent, AudioSource, AutomationInfo, ListenMode, Recognized, Recognizer, ReplyKind, RunningInfo, SettingChange,
    Speaker, VoiceBackend, VoiceEvent, VoiceSettings, VoiceState,
};

// ---- mocks -----------------------------------------------------------------------------------------------

#[derive(Default)]
struct SrcState {
    sender: Option<Sender<AudioEvent>>,
    starts: usize,
    stops: usize,
    fail_starts: usize,
    device: Option<String>,
}

#[derive(Clone, Default)]
struct SrcHandle(Arc<Mutex<SrcState>>);

impl SrcHandle {
    fn is_open(&self) -> bool {
        self.0.lock().unwrap().sender.is_some()
    }
    fn starts(&self) -> usize {
        self.0.lock().unwrap().starts
    }
    fn stops(&self) -> usize {
        self.0.lock().unwrap().stops
    }
    fn set_fail_starts(&self, n: usize) {
        self.0.lock().unwrap().fail_starts = n;
    }
    /// Deliver an event; false if the microphone is closed (nothing is captured then).
    fn send(&self, ev: AudioEvent) -> bool {
        match &self.0.lock().unwrap().sender {
            Some(s) => s.send(ev).is_ok(),
            None => false,
        }
    }
    fn chunk(&self, c: Vec<f32>) -> bool {
        self.send(AudioEvent::Chunk(c))
    }
    /// Speech, then enough silence for the segmenter to finish the utterance.
    fn utterance(&self) -> bool {
        let mut ok = true;
        for _ in 0..3 {
            ok &= self.chunk(tone(100));
        }
        for _ in 0..4 {
            ok &= self.chunk(quiet(100));
        }
        ok
    }
}

struct MockSource(SrcHandle);

impl AudioSource for MockSource {
    fn start(&mut self, device: Option<&str>, sink: Sender<AudioEvent>) -> Result<(), String> {
        let mut s = self.0 .0.lock().unwrap();
        s.starts += 1;
        if s.fail_starts > 0 {
            s.fail_starts -= 1;
            return Err("no such device".into());
        }
        s.device = device.map(str::to_string);
        s.sender = Some(sink);
        Ok(())
    }
    fn stop(&mut self) {
        let mut s = self.0 .0.lock().unwrap();
        s.stops += 1;
        s.sender = None;
    }
}

fn tone(ms: usize) -> Vec<f32> {
    (0..ms * 16).map(|i| if i % 2 == 0 { 0.3 } else { -0.3 }).collect()
}
fn quiet(ms: usize) -> Vec<f32> {
    vec![0.0; ms * 16]
}

#[derive(Default)]
struct RecState {
    queue: VecDeque<Result<Recognized, String>>,
    calls: usize,
    delay_ms: u64,
    panic: bool,
}

#[derive(Clone, Default)]
struct RecHandle(Arc<Mutex<RecState>>);

impl RecHandle {
    fn say(&self, text: &str) {
        self.0.lock().unwrap().queue.push_back(Ok(Recognized { text: text.into(), confidence: Some(0.9), language: Some("en".into()) }));
    }
    fn fail(&self, e: &str) {
        self.0.lock().unwrap().queue.push_back(Err(e.into()));
    }
    fn calls(&self) -> usize {
        self.0.lock().unwrap().calls
    }
}

struct FakeRecognizer(RecHandle);

impl Recognizer for FakeRecognizer {
    fn transcribe(&mut self, _audio: &[f32], _language: &str) -> Result<Recognized, String> {
        let (next, delay, panic) = {
            let mut s = (self.0).0.lock().unwrap();
            s.calls += 1;
            (s.queue.pop_front(), s.delay_ms, s.panic)
        };
        if panic {
            panic!("mock recogniser panics");
        }
        if delay > 0 {
            thread::sleep(Duration::from_millis(delay));
        }
        next.unwrap_or(Ok(Recognized { text: String::new(), confidence: None, language: None }))
    }
    fn name(&self) -> String {
        "fake".into()
    }
}

#[derive(Default)]
struct FakeSpeaker {
    spoken: Mutex<Vec<String>>,
    /// While "playing", the room echoes the speech back into the microphone.
    echo_into: Mutex<Option<SrcHandle>>,
}

impl Speaker for FakeSpeaker {
    fn speak(&self, text: &str) -> Result<(), String> {
        self.spoken.lock().unwrap().push(text.to_string());
        let echo = self.echo_into.lock().unwrap().clone();
        for _ in 0..5 {
            if let Some(src) = &echo {
                src.chunk(tone(100));
            }
            thread::sleep(Duration::from_millis(20));
        }
        Ok(())
    }
}

#[derive(Default)]
struct BackendState {
    automations: Vec<AutomationInfo>,
    started: Vec<i64>,
    confirmed: Vec<bool>,
}

#[derive(Clone, Default)]
struct MockBackend(Arc<Mutex<BackendState>>);

impl VoiceBackend for MockBackend {
    fn automations(&self) -> Vec<AutomationInfo> {
        self.0.lock().unwrap().automations.clone()
    }
    fn running(&self) -> Vec<RunningInfo> {
        Vec::new()
    }
    fn is_running(&self, _id: i64) -> bool {
        false
    }
    fn start(&self, id: i64, confirmed: bool) -> Result<(), String> {
        let mut s = self.0.lock().unwrap();
        s.started.push(id);
        s.confirmed.push(confirmed);
        Ok(())
    }
    fn stop(&self, _run_id: i64) -> Result<bool, String> {
        Ok(true)
    }
    fn apply(&self, _change: &SettingChange) -> Result<String, String> {
        Ok("ok".into())
    }
    fn notice(&self, _text: &str) {}
}

fn auto(id: i64, name: &str, system: bool) -> AutomationInfo {
    AutomationInfo { id, name: name.into(), description: String::new(), enabled: true, allow_system: system }
}

// ---- rig ----------------------------------------------------------------------------------------------------

struct Rig {
    session: Session,
    src: SrcHandle,
    rec: RecHandle,
    backend: MockBackend,
    speaker: Arc<FakeSpeaker>,
    events: Arc<Mutex<Vec<VoiceEvent>>>,
}

fn fast() -> SessionTimings {
    SessionTimings {
        ptt_tail: Duration::from_millis(40),
        ptt_min_press: Duration::from_millis(30),
        ptt_max: Duration::from_millis(400),
        armed: Duration::from_millis(500),
        speak_guard: Duration::from_millis(150),
        speak_cap: Duration::from_secs(1),
        backoff: vec![Duration::from_millis(40), Duration::from_millis(80), Duration::from_millis(160)],
        tick: Duration::from_millis(2),
        join_timeout: Duration::from_secs(1),
    }
}

fn rig_with(mode: ListenMode, tweak: impl FnOnce(&mut VoiceSettings, &mut SessionTimings, &SrcHandle, &RecHandle)) -> Rig {
    let mut settings = VoiceSettings { mode, run_system_automations: true, ..VoiceSettings::default() };
    let mut timings = fast();
    let src = SrcHandle::default();
    let rec = RecHandle::default();
    tweak(&mut settings, &mut timings, &src, &rec);
    let backend = MockBackend::default();
    backend.0.lock().unwrap().automations = vec![auto(1, "Zip backup", false), auto(9, "Power off", true)];
    let events: Arc<Mutex<Vec<VoiceEvent>>> = Arc::default();
    let sink = events.clone();
    let speaker = Arc::new(FakeSpeaker::default());
    let parts = SessionParts {
        settings: settings.clone(),
        language: "en".into(),
        source: Box::new(MockSource(src.clone())),
        recognizer: Box::new(FakeRecognizer(rec.clone())),
        segmenter: Box::new(EnergySegmenter::with_config(0.02, 100, 100, 200, 5000)),
        speaker: Some(speaker.clone()),
        controller: VoiceController::new(Arc::new(backend.clone()), settings),
        events: Arc::new(move |e| sink.lock().unwrap().push(e)),
    };
    let session = Session::start_with(parts, timings);
    Rig { session, src, rec, backend, speaker, events }
}

fn rig(mode: ListenMode) -> Rig {
    rig_with(mode, |_, _, _, _| {})
}

impl Rig {
    fn started(&self) -> Vec<i64> {
        self.backend.0.lock().unwrap().started.clone()
    }
    fn confirmed(&self) -> Vec<bool> {
        self.backend.0.lock().unwrap().confirmed.clone()
    }
    fn events(&self) -> Vec<VoiceEvent> {
        self.events.lock().unwrap().clone()
    }
    fn states(&self) -> Vec<VoiceState> {
        self.events().into_iter().filter_map(|e| if let VoiceEvent::State { state } = e { Some(state) } else { None }).collect()
    }
    fn heard(&self) -> Vec<String> {
        self.events().into_iter().filter_map(|e| if let VoiceEvent::Heard { text, .. } = e { Some(text) } else { None }).collect()
    }
    fn replies(&self) -> Vec<(ReplyKind, String)> {
        self.events().into_iter().filter_map(|e| if let VoiceEvent::Reply { reply } = e { Some((reply.kind, reply.text)) } else { None }).collect()
    }
    fn wait_state(&self, want: VoiceState) {
        wait(&format!("state {want:?} (now {:?})", self.session.state()), || self.session.state() == want);
    }
}

fn wait(what: &str, mut cond: impl FnMut() -> bool) {
    let end = Instant::now() + Duration::from_secs(5);
    while Instant::now() < end {
        if cond() {
            return;
        }
        thread::sleep(Duration::from_millis(5));
    }
    panic!("timed out waiting for {what}");
}

fn pause(ms: u64) {
    thread::sleep(Duration::from_millis(ms));
}

/// A full push-to-talk exchange: press, speak, release.
fn ptt_say(r: &Rig, text: &str) {
    r.rec.say(text);
    r.session.press();
    wait("source open", || r.src.is_open());
    pause(50);
    for _ in 0..3 {
        r.src.chunk(tone(100));
    }
    r.session.release();
}

// ---- modes and states --------------------------------------------------------------------------------------

#[test]
fn always_on_keeps_the_microphone_open_and_runs_a_command() {
    let r = rig(ListenMode::AlwaysOn);
    wait("source open", || r.src.is_open());
    assert_eq!(r.session.state(), VoiceState::Idle);
    r.rec.say("hey localflow run zip backup");
    assert!(r.src.utterance());
    wait("automation started", || r.started() == vec![1]);
    r.wait_state(VoiceState::Idle);
    assert!(r.src.is_open());
    assert_eq!(r.src.starts(), 1);
    assert_eq!(r.heard(), vec!["run zip backup".to_string()]);
    let states = r.states();
    assert_eq!(states.first(), Some(&VoiceState::Idle));
    assert!(states.contains(&VoiceState::Processing));
    let src = r.src.clone();
    let events = r.events.clone();
    r.session.stop();
    assert!(!src.is_open());
    assert_eq!(events.lock().unwrap().last(), Some(&VoiceEvent::State { state: VoiceState::Off }));
}

#[test]
fn push_to_talk_opens_on_press_and_closes_after_release() {
    let r = rig(ListenMode::PushToTalk);
    pause(30);
    assert!(!r.src.is_open());
    assert_eq!(r.src.starts(), 0);
    assert!(!r.src.chunk(tone(100)), "nothing is captured while the key is up");
    ptt_say(&r, "run zip backup");
    wait("automation started", || r.started() == vec![1]);
    wait("source closed", || !r.src.is_open());
    r.wait_state(VoiceState::Idle);
    assert_eq!(r.src.starts(), 1);
    let states = r.states();
    let pos = |s: &VoiceState| states.iter().position(|x| x == s).unwrap();
    assert!(pos(&VoiceState::Listening) < pos(&VoiceState::Processing));
    assert_eq!(r.rec.calls(), 1);
}

#[test]
fn push_to_talk_mic_stays_open_for_the_tail_only() {
    let r = rig_with(ListenMode::PushToTalk, |_, t, _, _| t.ptt_tail = Duration::from_millis(200));
    r.session.press();
    wait("open", || r.src.is_open());
    pause(60);
    r.src.chunk(tone(300));
    r.session.release();
    pause(80);
    assert!(r.src.is_open(), "still in the tail");
    wait("closed", || !r.src.is_open());
}

#[test]
fn key_repeat_and_duplicate_presses_are_ignored() {
    let r = rig(ListenMode::PushToTalk);
    r.rec.say("run zip backup");
    for _ in 0..5 {
        r.session.press();
    }
    wait("open", || r.src.is_open());
    pause(50);
    r.src.chunk(tone(300));
    for _ in 0..5 {
        r.session.press();
        pause(2);
    }
    r.session.release();
    r.session.release();
    wait("started", || r.started() == vec![1]);
    wait("closed", || !r.src.is_open());
    assert_eq!(r.src.starts(), 1);
    assert_eq!(r.rec.calls(), 1);
}

#[test]
fn a_too_short_press_is_ignored_without_error() {
    let r = rig_with(ListenMode::PushToTalk, |_, t, _, _| t.ptt_min_press = Duration::from_millis(300));
    r.rec.say("run zip backup");
    r.session.press();
    wait("open", || r.src.is_open());
    r.src.chunk(tone(300));
    r.session.release();
    wait("closed", || !r.src.is_open());
    r.wait_state(VoiceState::Idle);
    assert_eq!(r.rec.calls(), 0);
    assert!(r.started().is_empty());
    assert!(r.heard().is_empty() && r.replies().is_empty());
    assert!(r.states().iter().all(|s| !matches!(s, VoiceState::Error(_))));
}

#[test]
fn a_lost_key_release_ends_the_press_automatically() {
    let r = rig_with(ListenMode::PushToTalk, |_, t, _, _| t.ptt_max = Duration::from_millis(250));
    r.rec.say("run zip backup");
    r.session.press();
    wait("open", || r.src.is_open());
    r.src.chunk(tone(300));
    // no release
    wait("started", || r.started() == vec![1]);
    assert!(!r.src.is_open());
    r.wait_state(VoiceState::Idle);
}

#[test]
fn push_to_talk_ignores_nothing_said_and_unknown_phrases_get_a_reply() {
    let r = rig(ListenMode::PushToTalk);
    ptt_say(&r, "");
    wait("closed", || !r.src.is_open());
    r.wait_state(VoiceState::Idle);
    assert!(r.heard().is_empty());
    ptt_say(&r, "make me a sandwich");
    wait("reply", || !r.replies().is_empty());
    assert_eq!(r.replies()[0].0, ReplyKind::Problem);
    assert!(r.started().is_empty());
}

#[test]
fn switching_mode_stops_and_starts_the_source() {
    let r = rig(ListenMode::PushToTalk);
    pause(20);
    assert!(!r.src.is_open());
    let mut s = VoiceSettings { mode: ListenMode::AlwaysOn, run_system_automations: true, ..VoiceSettings::default() };
    assert!(!r.session.update_settings(s.clone()), "mode changes apply live");
    wait("open", || r.src.is_open());
    s.mode = ListenMode::PushToTalk;
    assert!(!r.session.update_settings(s));
    wait("closed", || !r.src.is_open());
    r.wait_state(VoiceState::Idle);
    // a press now works again
    ptt_say(&r, "run zip backup");
    wait("started", || r.started() == vec![1]);
}

#[test]
fn push_to_talk_ignores_wake_phrase_requirement() {
    let r = rig(ListenMode::PushToTalk);
    ptt_say(&r, "run zip backup");
    wait("started", || r.started() == vec![1]);
}

// ---- wake phrase ---------------------------------------------------------------------------------------------

#[test]
fn wake_phrase_then_command_in_a_second_utterance() {
    let r = rig(ListenMode::AlwaysOn);
    wait("open", || r.src.is_open());
    r.rec.say("hey localflow");
    r.src.utterance();
    r.wait_state(VoiceState::Listening);
    assert!(r.heard().is_empty());
    r.rec.say("run zip backup");
    r.src.utterance();
    wait("started", || r.started() == vec![1]);
    r.wait_state(VoiceState::Idle);
    assert_eq!(r.heard(), vec!["run zip backup".to_string()]);
}

#[test]
fn the_armed_window_expires() {
    let r = rig_with(ListenMode::AlwaysOn, |_, t, _, _| t.armed = Duration::from_millis(250));
    wait("open", || r.src.is_open());
    r.rec.say("hey localflow");
    r.src.utterance();
    r.wait_state(VoiceState::Listening);
    r.wait_state(VoiceState::Idle);
    r.rec.say("run zip backup");
    r.src.utterance();
    wait("recognised", || r.rec.calls() == 2);
    pause(60);
    assert!(r.started().is_empty());
    assert!(r.heard().is_empty());
}

#[test]
fn speech_without_the_wake_phrase_leaves_no_trace() {
    let r = rig(ListenMode::AlwaysOn);
    wait("open", || r.src.is_open());
    r.rec.say("run zip backup please");
    r.rec.say("what a lovely day it is");
    r.src.utterance();
    r.src.utterance();
    wait("both recognised", || r.rec.calls() == 2);
    pause(60);
    assert!(r.started().is_empty());
    let events = r.events();
    assert!(events.iter().all(|e| matches!(e, VoiceEvent::State { state: VoiceState::Idle })), "got {events:?}");
    assert_eq!(r.session.state(), VoiceState::Idle);
}

#[test]
fn a_command_that_merely_contains_the_phrase_is_not_a_wake_up() {
    let r = rig(ListenMode::AlwaysOn);
    wait("open", || r.src.is_open());
    r.rec.say("please run zip backup hey localflow");
    r.src.utterance();
    wait("recognised", || r.rec.calls() == 1);
    pause(60);
    assert!(r.started().is_empty() && r.heard().is_empty());
}

#[test]
fn a_clipped_wake_phrase_still_works() {
    let r = rig(ListenMode::AlwaysOn);
    wait("open", || r.src.is_open());
    r.rec.say("local flow run zip backup");
    r.src.utterance();
    wait("started", || r.started() == vec![1]);
}

#[test]
fn the_wake_phrase_can_be_changed_live() {
    let r = rig(ListenMode::AlwaysOn);
    wait("open", || r.src.is_open());
    let s = VoiceSettings { mode: ListenMode::AlwaysOn, wake_phrase: "okay computer".into(), run_system_automations: true, ..VoiceSettings::default() };
    assert!(!r.session.update_settings(s));
    pause(30);
    r.rec.say("hey localflow run zip backup");
    r.src.utterance();
    wait("recognised", || r.rec.calls() == 1);
    pause(50);
    assert!(r.started().is_empty());
    r.rec.say("okay computer run zip backup");
    r.src.utterance();
    wait("started", || r.started() == vec![1]);
}

// ---- mute and typed input --------------------------------------------------------------------------------------

#[test]
fn mute_releases_the_microphone_and_unmute_reopens_it() {
    let r = rig(ListenMode::AlwaysOn);
    wait("open", || r.src.is_open());
    r.session.set_muted(true);
    assert!(r.session.is_muted());
    wait("closed", || !r.src.is_open());
    r.wait_state(VoiceState::Muted);
    assert!(!r.src.chunk(tone(100)));
    r.session.set_muted(false);
    assert!(!r.session.is_muted());
    wait("reopened", || r.src.is_open());
    r.wait_state(VoiceState::Idle);
    assert_eq!(r.src.starts(), 2);
}

#[test]
fn typed_commands_and_confirmations_work_while_muted() {
    let r = rig(ListenMode::AlwaysOn);
    wait("open", || r.src.is_open());
    r.session.set_muted(true);
    r.wait_state(VoiceState::Muted);
    r.session.submit_text("run zip backup");
    wait("started", || r.started() == vec![1]);
    assert_eq!(r.session.state(), VoiceState::Muted);
    r.session.submit_text("run power off");
    wait("confirm", || r.events().iter().any(|e| matches!(e, VoiceEvent::Confirm { .. })));
    assert!(r.started() == vec![1]);
    r.session.answer_confirmation(r.last_question(), true);
    wait("power off started", || r.started() == vec![1, 9]);
    assert_eq!(r.session.state(), VoiceState::Muted);
    assert!(!r.src.is_open());
}

#[test]
fn a_spoken_confirmation_needs_no_wake_phrase() {
    // a low-risk question ("did you mean ...?"): a spoken yes in the next breath is enough
    let r = rig(ListenMode::AlwaysOn);
    wait("open", || r.src.is_open());
    r.rec.say("hey localflow run zip");
    r.src.utterance();
    wait("confirm", || r.events().iter().any(|e| matches!(e, VoiceEvent::Confirm { .. })));
    r.rec.say("yes");
    r.src.utterance();
    wait("started", || r.started() == vec![1]);
    assert_eq!(r.confirmed(), vec![false], "an always-on spoken yes does not count as a real confirmation");
}

#[test]
fn an_always_on_spoken_yes_cannot_confirm_a_system_run_but_the_button_can() {
    let r = rig(ListenMode::AlwaysOn);
    wait("open", || r.src.is_open());
    r.rec.say("hey localflow run power off");
    r.src.utterance();
    wait("confirm", || r.events().iter().any(|e| matches!(e, VoiceEvent::Confirm { .. })));
    r.rec.say("yes");
    r.src.utterance();
    wait("refusal", || r.replies().iter().any(|(_, t)| t.contains("Yes button")));
    pause(100);
    assert!(r.started().is_empty());
    r.session.answer_confirmation(r.last_question(), true);
    wait("started", || r.started() == vec![9]);
    assert_eq!(r.confirmed(), vec![true]);
}

#[test]
fn muting_or_a_mode_change_cancels_the_pending_question() {
    let r = rig(ListenMode::AlwaysOn);
    wait("open", || r.src.is_open());
    r.rec.say("hey localflow run power off");
    r.src.utterance();
    wait("confirm", || r.events().iter().any(|e| matches!(e, VoiceEvent::Confirm { .. })));
    r.session.set_muted(true);
    r.wait_state(VoiceState::Muted);
    r.session.set_muted(false);
    r.wait_state(VoiceState::Idle);
    r.session.answer_confirmation(r.last_question(), true);
    wait("nothing to confirm", || r.replies().iter().any(|(_, t)| t.contains("nothing to confirm")));
    assert!(r.started().is_empty());

    // a mode change drops it as well (other words: the same text again would be a duplicate)
    r.rec.say("hey localflow start power off");
    r.src.utterance();
    wait("second confirm", || r.events().iter().filter(|e| matches!(e, VoiceEvent::Confirm { .. })).count() == 2);
    r.session.update_settings(VoiceSettings { mode: ListenMode::PushToTalk, run_system_automations: true, ..VoiceSettings::default() });
    pause(100);
    r.session.answer_confirmation(r.last_question(), true);
    wait("nothing to confirm again", || r.replies().iter().filter(|(_, t)| t.contains("nothing to confirm")).count() == 2);
    assert!(r.started().is_empty());
}

#[test]
fn typed_phrases_are_full_confidence_and_can_confirm() {
    let r = rig(ListenMode::AlwaysOn);
    wait("open", || r.src.is_open());
    r.session.submit_text("run power off");
    wait("confirm", || r.events().iter().any(|e| matches!(e, VoiceEvent::Confirm { .. })));
    r.session.submit_text("yes");
    wait("started", || r.started() == vec![9]);
    assert_eq!(r.confirmed(), vec![true]);
    assert!(r.events().iter().any(|e| matches!(e, VoiceEvent::Heard { text, confidence: Some(c) } if text == "run power off" && *c == 1.0)));
}

#[test]
fn a_spoken_question_is_guarded_against_its_own_echo() {
    let r = rig(ListenMode::AlwaysOn);
    *r.speaker.echo_into.lock().unwrap() = Some(r.src.clone());
    wait("open", || r.src.is_open());
    r.session.update_settings(VoiceSettings { mode: ListenMode::AlwaysOn, spoken_feedback: true, run_system_automations: true, ..VoiceSettings::default() });
    pause(30);
    // the room echo of the question would be recognised as "yes"
    r.rec.say("hey localflow run zip");
    r.rec.say("yes");
    r.src.utterance();
    wait("spoken", || !r.speaker.spoken.lock().unwrap().is_empty());
    r.src.chunk(tone(100));
    r.src.chunk(quiet(300));
    r.wait_state(VoiceState::Idle);
    pause(250);
    assert_eq!(r.rec.calls(), 1, "the echo of the prompt must not reach the recogniser");
    assert!(r.started().is_empty());
}

#[test]
fn voice_controls_become_state_and_events() {
    let r = rig(ListenMode::AlwaysOn);
    wait("open", || r.src.is_open());
    r.session.submit_text("spoken feedback on");
    wait("settings event", || r.events().iter().any(|e| matches!(e, VoiceEvent::Settings { settings } if settings.spoken_feedback)));
    r.session.submit_text("mute");
    r.wait_state(VoiceState::Muted);
    wait("closed", || !r.src.is_open());
    assert!(r.session.is_muted());
    r.session.set_muted(false);
    r.wait_state(VoiceState::Idle);
    r.session.submit_text("stop listening");
    r.wait_state(VoiceState::Muted);
    wait("closed", || !r.src.is_open());
    let reps = r.replies();
    assert!(reps.iter().any(|(k, t)| *k == ReplyKind::Info && t.contains("microphone button")), "{reps:?}");
}

// ---- self-hearing -----------------------------------------------------------------------------------------------

#[test]
fn the_apps_own_voice_is_never_a_command() {
    let r = rig(ListenMode::AlwaysOn);
    *r.speaker.echo_into.lock().unwrap() = Some(r.src.clone());
    wait("open", || r.src.is_open());
    r.session.update_settings(VoiceSettings { mode: ListenMode::AlwaysOn, spoken_feedback: true, run_system_automations: true, ..VoiceSettings::default() });
    pause(30);
    // If the echo were heard it would be recognised as this and the recogniser would be called again.
    r.rec.say("hey localflow what is running");
    r.rec.say("hey localflow run zip backup");
    r.src.utterance();
    wait("speaking", || r.session.state() == VoiceState::Speaking);
    wait("spoken", || !r.speaker.spoken.lock().unwrap().is_empty());
    // the room echo continues just after the speech ended
    r.src.chunk(tone(100));
    r.src.chunk(quiet(300));
    r.wait_state(VoiceState::Idle);
    pause(250);
    assert_eq!(r.rec.calls(), 1, "echo must not reach the recogniser");
    assert!(r.started().is_empty());
    assert!(r.states().contains(&VoiceState::Speaking));
    // the guard is over: a real utterance is heard again
    r.src.utterance();
    wait("heard again", || r.rec.calls() == 2);
    wait("started", || r.started() == vec![1]);
}

// ---- device loss ----------------------------------------------------------------------------------------------------

#[test]
fn a_lost_device_is_retried_and_recovers() {
    let r = rig(ListenMode::AlwaysOn);
    wait("open", || r.src.is_open());
    assert!(r.src.send(AudioEvent::Disconnected));
    wait("error", || matches!(r.session.state(), VoiceState::Error(_)));
    if let VoiceState::Error(m) = r.session.state() {
        assert!(m.contains("microphone"), "{m}");
    }
    wait("recovered", || r.session.state() == VoiceState::Idle);
    assert!(r.src.is_open());
    assert_eq!(r.src.starts(), 2);
    // and it works again
    r.rec.say("hey localflow run zip backup");
    r.src.utterance();
    wait("started", || r.started() == vec![1]);
}

#[test]
fn a_device_that_never_comes_back_ends_in_error_until_settings_change() {
    let r = rig(ListenMode::AlwaysOn);
    wait("open", || r.src.is_open());
    r.src.set_fail_starts(100);
    r.src.send(AudioEvent::Error("stream broke".into()));
    wait("error", || matches!(r.session.state(), VoiceState::Error(_)));
    wait("three retries", || r.src.starts() == 4);
    pause(500);
    assert_eq!(r.src.starts(), 4, "no more retries");
    assert!(matches!(r.session.state(), VoiceState::Error(_)));
    assert!(!r.src.is_open());
    // fixing the situation and applying settings tries again
    r.src.set_fail_starts(0);
    r.session.update_settings(VoiceSettings { mode: ListenMode::AlwaysOn, run_system_automations: true, ..VoiceSettings::default() });
    wait("recovered", || r.session.state() == VoiceState::Idle);
    assert!(r.src.is_open());
}

#[test]
fn a_microphone_that_cannot_be_opened_at_start_is_retried() {
    let r = rig_with(ListenMode::AlwaysOn, |_, _, src, _| src.set_fail_starts(1));
    wait("recovered", || r.src.is_open() && r.session.state() == VoiceState::Idle);
    assert!(r.states().iter().any(|s| matches!(s, VoiceState::Error(_))));
    assert_eq!(r.src.starts(), 2);
}

#[test]
fn push_to_talk_reports_a_missing_microphone_and_recovers_on_the_next_press() {
    let r = rig_with(ListenMode::PushToTalk, |_, _, src, _| src.set_fail_starts(1));
    r.session.press();
    wait("error", || matches!(r.session.state(), VoiceState::Error(_)));
    r.session.release();
    r.session.press();
    wait("open", || r.src.is_open());
    r.wait_state(VoiceState::Listening);
    r.session.release();
    wait("closed", || !r.src.is_open());
}

// ---- recogniser -----------------------------------------------------------------------------------------------------

#[test]
fn recogniser_errors_are_reported_once_and_the_session_keeps_running() {
    let r = rig(ListenMode::AlwaysOn);
    wait("open", || r.src.is_open());
    r.rec.fail("model crashed");
    r.rec.fail("model crashed");
    r.rec.say("");
    for _ in 0..3 {
        r.src.utterance();
    }
    wait("three calls", || r.rec.calls() == 3);
    pause(50);
    let problems: Vec<_> = r.replies().into_iter().filter(|(k, _)| *k == ReplyKind::Problem).collect();
    assert_eq!(problems.len(), 1, "{problems:?}");
    assert_eq!(r.session.state(), VoiceState::Idle);
    r.rec.say("hey localflow run zip backup");
    r.src.utterance();
    wait("started", || r.started() == vec![1]);
}

#[test]
fn a_panicking_recogniser_still_releases_everything() {
    let r = rig(ListenMode::AlwaysOn);
    wait("open", || r.src.is_open());
    r.rec.0.lock().unwrap().panic = true;
    r.src.utterance();
    wait("closed", || !r.src.is_open());
    wait("error", || matches!(r.session.state(), VoiceState::Error(_)));
    // handle calls stay harmless
    r.session.press();
    r.session.submit_text("hello");
    r.session.stop();
}

// ---- settings, shutdown, load ------------------------------------------------------------------------------------------

#[test]
fn update_settings_says_when_a_restart_is_needed() {
    let r = rig(ListenMode::PushToTalk);
    let base = VoiceSettings { mode: ListenMode::PushToTalk, run_system_automations: true, ..VoiceSettings::default() };
    let mut s = base.clone();
    s.wake_phrase = "ok computer".into();
    s.spoken_feedback = true;
    assert!(!r.session.update_settings(s.clone()));
    s.microphone = Some("USB mic".into());
    assert!(r.session.update_settings(s.clone()));
    assert!(!r.session.update_settings(s.clone()), "same device again");
    s.language = "ru".into();
    assert!(r.session.update_settings(s.clone()));
    s.engine = "other".into();
    assert!(r.session.update_settings(s));
}

#[test]
fn the_selected_device_is_passed_to_the_source() {
    let r = rig_with(ListenMode::AlwaysOn, |s, _, _, _| s.microphone = Some("USB mic".into()));
    wait("open", || r.src.is_open());
    assert_eq!(r.src.0.lock().unwrap().device.as_deref(), Some("USB mic"));
}

#[test]
fn commands_come_from_the_controller() {
    let r = rig(ListenMode::PushToTalk);
    let cmds = r.session.commands();
    assert!(cmds.iter().any(|c| c.say.contains("Zip backup")), "{cmds:?}");
}

#[test]
fn dropping_the_session_releases_the_microphone() {
    let r = rig(ListenMode::AlwaysOn);
    wait("open", || r.src.is_open());
    let src = r.src.clone();
    drop(r);
    assert!(!src.is_open());
    assert!(src.stops() >= 1);
}

#[test]
fn stop_does_not_hang_on_a_stuck_recogniser() {
    let r = rig_with(ListenMode::AlwaysOn, |_, t, _, rec| {
        rec.0.lock().unwrap().delay_ms = 1500;
        t.join_timeout = Duration::from_millis(300);
    });
    wait("open", || r.src.is_open());
    r.src.utterance();
    wait("recognising", || r.rec.calls() == 1);
    let src = r.src.clone();
    let started = Instant::now();
    r.session.stop();
    assert!(started.elapsed() < Duration::from_millis(1200), "took {:?}", started.elapsed());
    // the abandoned worker releases the microphone as soon as it is free
    wait("released", || !src.is_open());
}

#[test]
fn heavy_input_is_bounded_and_the_session_stays_usable() {
    let r = rig_with(ListenMode::AlwaysOn, |_, _, _, rec| rec.0.lock().unwrap().delay_ms = 3);
    wait("open", || r.src.is_open());
    // 150 s of audio in one burst while the recogniser is slow
    for _ in 0..150 {
        r.src.chunk(tone(300));
        r.src.chunk(quiet(700));
    }
    // and one absurdly long chunk
    r.src.chunk(tone(1000).repeat(40));
    r.src.chunk(quiet(1000));
    wait("drained", || {
        let n = r.rec.calls();
        pause(150);
        n == r.rec.calls() && n > 0
    });
    r.wait_state(VoiceState::Idle);
    r.rec.say("hey localflow run zip backup");
    r.src.utterance();
    wait("started", || r.started() == vec![1]);
}


impl Rig {
    /// The id of the newest question shown (0 if none was asked).
    fn last_question(&self) -> u64 {
        self.events().iter().rev().find_map(|e| match e {
            VoiceEvent::Confirm { id, .. } => Some(*id),
            _ => None,
        }).unwrap_or(0)
    }
}

#[test]
fn a_stale_on_screen_yes_does_not_approve_a_newer_question() {
    let r = rig(ListenMode::AlwaysOn);
    wait("open", || r.src.is_open());
    r.session.submit_text("run zip");
    wait("first question", || r.events().iter().filter(|e| matches!(e, VoiceEvent::Confirm { .. })).count() == 1);
    let stale = r.last_question();
    // a new question replaces it before the old card's Yes is clicked
    r.session.submit_text("run power off");
    wait("second question", || r.events().iter().filter(|e| matches!(e, VoiceEvent::Confirm { .. })).count() == 2);
    let fresh = r.last_question();
    assert_ne!(stale, fresh);
    r.session.answer_confirmation(stale, true);
    wait("refused", || r.replies().iter().any(|(_, t)| t.contains("earlier question")));
    pause(100);
    assert!(r.started().is_empty(), "the old Yes must not start the system automation");
    r.session.answer_confirmation(fresh, true);
    wait("started", || r.started() == vec![9]);
}

#[test]
fn a_question_asked_right_after_cancelled_survives_spoken_replies() {
    // always-on with spoken replies: "Cancelled." is spoken, then the new question; the new
    // question must still be waiting afterwards.
    let r = rig(ListenMode::AlwaysOn);
    wait("open", || r.src.is_open());
    r.session.update_settings(VoiceSettings { mode: ListenMode::AlwaysOn, spoken_feedback: true, run_system_automations: true, ..VoiceSettings::default() });
    pause(30);
    r.session.submit_text("run zip");
    wait("first question", || r.events().iter().filter(|e| matches!(e, VoiceEvent::Confirm { .. })).count() == 1);
    r.session.submit_text("run power off");
    wait("second question", || r.events().iter().filter(|e| matches!(e, VoiceEvent::Confirm { .. })).count() == 2);
    wait("spoken", || r.speaker.spoken.lock().unwrap().len() >= 3);
    r.session.submit_text("yes");
    wait("started", || r.started() == vec![9]);
    assert!(!r.replies().iter().any(|(_, t)| t.contains("nothing to confirm")), "{:?}", r.replies());
}
