//! Voice control logic: transcripts in, replies and backend calls out. No audio is involved.

use std::sync::{Arc, Mutex};

use localflow_core::voice::{
    controller::{ControlRequest, VoiceController, CONFIRM_TTL_MS, DUPLICATE_TEXT_MS, LAUNCH_GUARD_MS},
    grammar::{self, Intent},
    settings, AutomationInfo, ListenMode, Reply, ReplyKind, RunningInfo, SettingChange, Transcript, VoiceAlias, VoiceBackend, VoiceSettings,
};

#[derive(Default)]
struct State {
    automations: Vec<AutomationInfo>,
    running: Vec<RunningInfo>,
    started: Vec<i64>,
    confirmed: Vec<bool>,
    stopped: Vec<i64>,
    applied: Vec<SettingChange>,
    notices: Vec<String>,
    fail_start: Option<String>,
    fail_apply: Option<String>,
    fail_stop: Option<String>,
    /// Starting really makes it "running" (like the real backend).
    start_makes_running: bool,
}

#[derive(Default, Clone)]
struct MockBackend(Arc<Mutex<State>>);

impl MockBackend {
    fn with(autos: Vec<AutomationInfo>) -> Self {
        let b = MockBackend::default();
        b.0.lock().unwrap().automations = autos;
        b
    }
    fn st(&self) -> std::sync::MutexGuard<'_, State> {
        self.0.lock().unwrap()
    }
    fn started_and_confirmed(&self) -> (Vec<i64>, Vec<bool>) {
        let s = self.st();
        (s.started.clone(), s.confirmed.clone())
    }
    fn actions(&self) -> usize {
        let s = self.st();
        s.started.len() + s.stopped.len() + s.applied.len()
    }
}

impl VoiceBackend for MockBackend {
    fn automations(&self) -> Vec<AutomationInfo> {
        self.st().automations.clone()
    }
    fn running(&self) -> Vec<RunningInfo> {
        self.st().running.clone()
    }
    fn is_running(&self, id: i64) -> bool {
        self.st().running.iter().any(|r| r.automation_id == id)
    }
    fn start(&self, id: i64, confirmed: bool) -> Result<(), String> {
        let mut s = self.st();
        if let Some(e) = &s.fail_start {
            return Err(e.clone());
        }
        s.started.push(id);
        s.confirmed.push(confirmed);
        if s.start_makes_running {
            let name = s.automations.iter().find(|a| a.id == id).map(|a| a.name.clone()).unwrap_or_default();
            let run_id = 100 + s.started.len() as i64;
            s.running.push(RunningInfo { run_id, automation_id: id, name });
        }
        Ok(())
    }
    fn stop(&self, run_id: i64) -> Result<bool, String> {
        let mut s = self.st();
        if let Some(e) = &s.fail_stop {
            return Err(e.clone());
        }
        s.stopped.push(run_id);
        let before = s.running.len();
        s.running.retain(|r| r.run_id != run_id);
        Ok(s.running.len() != before)
    }
    fn apply(&self, change: &SettingChange) -> Result<String, String> {
        let mut s = self.st();
        if let Some(e) = &s.fail_apply {
            return Err(e.clone());
        }
        s.applied.push(change.clone());
        Ok("ok".into())
    }
    fn notice(&self, text: &str) {
        self.st().notices.push(text.to_string());
    }
}

fn auto(id: i64, name: &str) -> AutomationInfo {
    AutomationInfo { id, name: name.into(), description: format!("does {name}"), enabled: true, allow_system: false }
}

fn run_info(run_id: i64, id: i64, name: &str) -> RunningInfo {
    RunningInfo { run_id, automation_id: id, name: name.into() }
}

fn lib() -> Vec<AutomationInfo> {
    vec![auto(1, "Backup notes"), auto(2, "Backup photos"), auto(3, "Tidy screenshots"), auto(4, "Бэкап заметок"), auto(5, "Aufräumen Downloads")]
}

fn setup(autos: Vec<AutomationInfo>) -> (MockBackend, VoiceController) {
    setup_with(autos, VoiceSettings::default())
}

fn setup_cs(autos: Vec<AutomationInfo>) -> (MockBackend, VoiceController) {
    setup_with(autos, VoiceSettings { change_settings: true, ..VoiceSettings::default() })
}

fn setup_with(autos: Vec<AutomationInfo>, settings: VoiceSettings) -> (MockBackend, VoiceController) {
    let b = MockBackend::with(autos);
    let c = VoiceController::new(Arc::new(b.clone()), settings);
    (b, c)
}

fn say(c: &mut VoiceController, text: &str, now: u64) -> Vec<Reply> {
    c.handle_text(text, now)
}

fn kinds(r: &[Reply]) -> Vec<ReplyKind> {
    r.iter().map(|r| r.kind).collect()
}

fn text(r: &[Reply]) -> String {
    r.iter().map(|r| r.text.as_str()).collect::<Vec<_>>().join(" | ")
}

// ---- running ------------------------------------------------------------------------------------

#[test]
fn run_in_three_languages() {
    let (b, mut c) = setup(lib());
    let r = say(&mut c, "run tidy screenshots", 0);
    assert_eq!(kinds(&r), vec![ReplyKind::Done]);
    assert!(r[0].text.contains("Tidy screenshots"));
    let r = say(&mut c, "Запусти бэкап заметок", 10_000);
    assert_eq!(kinds(&r), vec![ReplyKind::Done]);
    assert!(r[0].text.starts_with("Запускаю"));
    let r = say(&mut c, "Starte Aufräumen Downloads bitte", 20_000);
    assert_eq!(kinds(&r), vec![ReplyKind::Done]);
    assert!(r[0].text.starts_with("Starte"));
    assert_eq!(b.st().started, vec![3, 4, 5]);
}

#[test]
fn ambiguity_asks_and_runs_nothing() {
    let (b, mut c) = setup(lib());
    let r = say(&mut c, "run backup", 0);
    assert_eq!(kinds(&r), vec![ReplyKind::Problem]);
    assert!(r[0].text.contains("Backup notes") && r[0].text.contains("Backup photos"), "{}", r[0].text);
    assert!(b.st().started.is_empty());
    assert!(c.pending_confirmation(0).is_none());
    // many candidates: at most three are listed
    let many = vec![auto(1, "Sync a"), auto(2, "Sync b"), auto(3, "Sync c"), auto(4, "Sync d"), auto(5, "Sync e")];
    let (b2, mut c2) = setup(many);
    let r = say(&mut c2, "run sync", 0);
    assert_eq!(r[0].text.matches("Sync ").count(), 3, "{}", r[0].text);
    assert!(b2.st().started.is_empty());
}

#[test]
fn unknown_target_and_suggestions() {
    let (b, mut c) = setup(lib());
    let r = say(&mut c, "run something else entirely", 0);
    assert_eq!(kinds(&r), vec![ReplyKind::Problem]);
    assert!(b.st().started.is_empty());
    let r = say(&mut c, "run", 1_000);
    assert_eq!(kinds(&r), vec![ReplyKind::Problem]);
    // text that is no command but resembles an automation: suggestion, nothing run
    let r = say(&mut c, "tidy screenshots", 2_000);
    assert_eq!(kinds(&r), vec![ReplyKind::Problem]);
    assert!(r[0].text.contains("Run Tidy screenshots"), "{}", r[0].text);
    assert!(r[0].text.contains("what can I say"));
    assert!(b.st().started.is_empty());
}

#[test]
fn a_weak_match_asks_first() {
    let (b, mut c) = setup(vec![auto(5, "Weekly report summary"), auto(1, "Backup notes")]);
    // only one of three words
    let r = say(&mut c, "run weekly", 0);
    assert_eq!(kinds(&r), vec![ReplyKind::Confirm], "{}", text(&r));
    assert!(b.st().started.is_empty());
    let r = say(&mut c, "yes", 1_000);
    assert_eq!(kinds(&r), vec![ReplyKind::Done]);
    assert_eq!(b.st().started, vec![5]);
    // one of two words is a likely match, and a likely match asks too
    let (b, mut c) = setup(lib());
    assert_eq!(say(&mut c, "run Aufräumen", 0)[0].kind, ReplyKind::Confirm);
    assert!(b.st().started.is_empty());
}

#[test]
fn low_recognition_confidence_asks_first() {
    let (b, mut c) = setup(lib());
    let t = Transcript { id: 1, text: "run tidy screenshots".into(), confidence: Some(0.3), language: Some("en".into()) };
    let r = c.handle(&t, 0);
    assert_eq!(r[0].kind, ReplyKind::Confirm);
    assert!(b.st().started.is_empty());
    let t = Transcript { id: 2, text: "yes".into(), confidence: Some(0.2), language: None };
    let r = c.handle(&t, 1_000);
    assert_eq!(r[0].kind, ReplyKind::Confirm, "a doubtful yes is not trusted");
    assert!(b.st().started.is_empty());
    assert!(c.pending_confirmation(1_001).is_some());
    let t = Transcript { id: 3, text: "yes".into(), confidence: Some(0.9), language: None };
    assert_eq!(c.handle(&t, 2_000)[0].kind, ReplyKind::Done);
    assert_eq!(b.st().started, vec![3]);
}

#[test]
fn disabled_automation_is_refused() {
    let mut off = auto(7, "Old job");
    off.enabled = false;
    let (b, mut c) = setup(vec![off]);
    let r = say(&mut c, "run old job", 0);
    assert_eq!(kinds(&r), vec![ReplyKind::Problem]);
    assert!(r[0].text.contains("turned off"));
    assert!(b.st().started.is_empty());
    assert!(c.pending_confirmation(0).is_none());
    // and it is not offered
    assert!(c.commands().iter().all(|e| !e.say.contains("Old job")));
}

#[test]
fn system_automation_needs_the_setting_and_a_confirmation() {
    let mut sys = auto(9, "Power off");
    sys.allow_system = true;
    // 1. setting off: refused, no question
    let (b, mut c) = setup(vec![sys.clone()]);
    let r = say(&mut c, "run power off", 0);
    assert_eq!(kinds(&r), vec![ReplyKind::Problem]);
    assert!(r[0].text.contains("voice settings"));
    assert!(b.st().started.is_empty() && c.pending_confirmation(0).is_none());
    assert!(c.commands().iter().all(|e| !e.say.contains("Power off")), "not listed while it cannot work");

    // 2. setting on: asks, yes runs it
    let s = VoiceSettings { run_system_automations: true, ..VoiceSettings::default() };
    let (b, mut c) = setup_with(vec![sys.clone()], s.clone());
    let r = say(&mut c, "run power off", 0);
    assert_eq!(kinds(&r), vec![ReplyKind::Confirm]);
    assert!(c.pending_confirmation(1).is_some());
    assert!(b.st().started.is_empty());
    let cmds = c.commands();
    assert!(cmds.iter().any(|e| e.say == "Run Power off" && e.does.contains("confirm")));
    let r = say(&mut c, "yes", 2_000);
    assert_eq!(kinds(&r), vec![ReplyKind::Done]);
    assert_eq!(b.st().started, vec![9]);

    // 3. no cancels
    let (b, mut c) = setup_with(vec![sys.clone()], s.clone());
    say(&mut c, "run power off", 0);
    let r = say(&mut c, "no", 1_000);
    assert_eq!(r[0].text, "Cancelled.");
    assert!(b.st().started.is_empty() && c.pending_confirmation(1_001).is_none());

    // 4. the on-screen buttons
    let (b, mut c) = setup_with(vec![sys.clone()], s.clone());
    say(&mut c, "run power off", 0);
    let r = c.answer_confirmation(true, 500);
    assert_eq!(r[0].kind, ReplyKind::Done);
    assert_eq!(b.st().started, vec![9]);
    let (b, mut c) = setup_with(vec![sys.clone()], s.clone());
    say(&mut c, "run power off", 0);
    assert_eq!(c.answer_confirmation(false, 500)[0].text, "Cancelled.");
    assert!(b.st().started.is_empty());

    // 5. a high-confidence match still asks, and the setting is re-checked at "yes"
    let (b, mut c) = setup_with(vec![sys.clone()], s.clone());
    say(&mut c, "run power off", 0);
    c.update_settings(VoiceSettings { run_system_automations: false, ..s });
    assert!(c.pending_confirmation(10).is_none(), "new settings drop the old question");
    let r = say(&mut c, "yes", 1_000);
    assert_eq!(r[0].kind, ReplyKind::Info);
    assert!(b.st().started.is_empty());
}

#[test]
fn confirmations_expire_and_are_cancelled_by_other_commands() {
    let mut sys = auto(9, "Power off");
    sys.allow_system = true;
    let s = VoiceSettings { run_system_automations: true, ..VoiceSettings::default() };
    let (b, mut c) = setup_with(vec![sys, auto(3, "Tidy screenshots")], s);

    say(&mut c, "run power off", 0);
    assert!(c.pending_confirmation(CONFIRM_TTL_MS - 1).is_some());
    assert!(c.pending_confirmation(CONFIRM_TTL_MS).is_none());
    let r = say(&mut c, "yes", CONFIRM_TTL_MS + 1);
    assert_eq!(kinds(&r), vec![ReplyKind::Info]);
    assert!(r[0].text.contains("expired"), "{}", r[0].text);
    assert!(b.st().started.is_empty());
    // a second stray yes just says there is nothing to confirm
    let r = say(&mut c, "да", CONFIRM_TTL_MS + 10_000);
    assert!(r[0].text.contains("nothing") || r[0].text.contains("нечего"), "{}", r[0].text);
    assert!(c.answer_confirmation(true, 60_000)[0].text.contains("nothing"));

    // any other command cancels the question and is carried out
    say(&mut c, "run power off", 100_000);
    let r = say(&mut c, "run tidy screenshots", 102_000);
    assert_eq!(kinds(&r), vec![ReplyKind::Info, ReplyKind::Done]);
    assert_eq!(r[0].text, "Cancelled.");
    assert_eq!(b.st().started, vec![3]);
    assert!(c.pending_confirmation(102_001).is_none());
    // "yes" afterwards does not resurrect the old question
    let r = say(&mut c, "yes", 110_000);
    assert_eq!(b.st().started, vec![3]);
    assert_eq!(r[0].kind, ReplyKind::Info);
    // bare "cancel"/"stop" answers a question with no
    say(&mut c, "run power off", 200_000);
    assert_eq!(say(&mut c, "cancel", 201_000)[0].text, "Cancelled.");
}

// ---- duplicates, in-flight guard ------------------------------------------------------------------

#[test]
fn duplicates_are_suppressed() {
    let (b, mut c) = setup(lib());
    let t = |id, text: &str| Transcript { id, text: text.into(), confidence: None, language: None };
    assert_eq!(c.handle(&t(1, "run tidy screenshots"), 0).len(), 1);
    // same id: never twice (even much later, even with other text)
    assert!(c.handle(&t(1, "run tidy screenshots"), 60_000).is_empty());
    assert!(c.handle(&t(1, "what's running"), 61_000).is_empty());
    // same text, new id, inside the window: a re-emitted result
    assert!(c.handle(&t(2, "Run tidy screenshots!"), DUPLICATE_TEXT_MS - 1).is_empty());
    assert_eq!(b.st().started, vec![3]);
    // the notice is only for what was really handled
    assert_eq!(b.st().notices.len(), 1);
    // later the same words are a new command (and then the launch guard / running state decide)
    let r = c.handle(&t(3, "run tidy screenshots"), DUPLICATE_TEXT_MS + 1);
    assert_eq!(r.len(), 1);
    assert_eq!(r[0].kind, ReplyKind::Info, "{}", r[0].text);
    assert_eq!(b.st().started, vec![3]);
}

#[test]
fn the_same_automation_is_not_launched_twice_quickly() {
    let (b, mut c) = setup(lib());
    // different words, different ids, same automation
    assert_eq!(say(&mut c, "run tidy screenshots", 0)[0].kind, ReplyKind::Done);
    let r = say(&mut c, "launch tidy", 1_000);
    assert_eq!(r[0].kind, ReplyKind::Info);
    assert!(r[0].text.contains("just started"));
    assert_eq!(b.st().started, vec![3]);
    // after the guard it can run again
    let r = say(&mut c, "start tidy screenshots", LAUNCH_GUARD_MS + 1_000);
    assert_eq!(r[0].kind, ReplyKind::Done);
    assert_eq!(b.st().started, vec![3, 3]);
}

#[test]
fn already_running_is_skipped() {
    let (b, mut c) = setup(lib());
    b.st().running.push(run_info(50, 3, "Tidy screenshots"));
    let r = say(&mut c, "run tidy screenshots", 0);
    assert_eq!(kinds(&r), vec![ReplyKind::Info]);
    assert!(r[0].text.contains("already running"));
    assert!(b.st().started.is_empty());
    // same in Russian
    b.st().running.push(run_info(51, 4, "Бэкап заметок"));
    let r = say(&mut c, "запусти бэкап заметок", 10_000);
    assert!(r[0].text.contains("уже запущена"));
}

#[test]
fn start_returns_and_failures_are_reported() {
    let (b, mut c) = setup(lib());
    b.st().fail_start = Some("database is locked".into());
    let r = say(&mut c, "run tidy screenshots", 0);
    assert_eq!(kinds(&r), vec![ReplyKind::Problem]);
    assert!(r[0].text.contains("database is locked"));
    // a failed start doesn't engage the launch guard
    b.st().fail_start = None;
    assert_eq!(say(&mut c, "run tidy screenshots now", 4_000)[0].kind, ReplyKind::Done);
}

// ---- stopping --------------------------------------------------------------------------------------

#[test]
fn stop_by_name_and_everything() {
    let (b, mut c) = setup(lib());
    {
        let mut s = b.st();
        s.running = vec![run_info(10, 1, "Backup notes"), run_info(11, 3, "Tidy screenshots")];
    }
    // bare stop with two running: asks, stops nothing
    let r = say(&mut c, "stop", 0);
    assert_eq!(kinds(&r), vec![ReplyKind::Problem]);
    assert!(r[0].text.contains("Backup notes") && r[0].text.contains("Tidy screenshots"));
    assert!(b.st().stopped.is_empty());
    // by name
    let r = say(&mut c, "stop tidy", 10_000);
    assert_eq!(kinds(&r), vec![ReplyKind::Done]);
    assert_eq!(b.st().stopped, vec![11]);
    // now one is left: bare stop is clear
    let r = say(&mut c, "стоп", 20_000);
    assert_eq!(kinds(&r), vec![ReplyKind::Done]);
    assert_eq!(b.st().stopped, vec![11, 10]);
    // nothing running
    assert_eq!(say(&mut c, "stop", 30_000)[0].kind, ReplyKind::Info);
    // not running by name
    b.st().running = vec![run_info(12, 1, "Backup notes")];
    let r = say(&mut c, "stop tidy screenshots", 40_000);
    assert_eq!(kinds(&r), vec![ReplyKind::Problem]);
    assert_eq!(b.st().stopped, vec![11, 10]);
    // everything, in all three languages
    b.st().running = vec![run_info(20, 1, "Backup notes"), run_info(21, 2, "Backup photos"), run_info(22, 2, "Backup photos")];
    let r = say(&mut c, "stop everything", 50_000);
    assert_eq!(kinds(&r), vec![ReplyKind::Done]);
    assert_eq!(&b.st().stopped[2..], &[20, 21, 22]);
    b.st().running = vec![run_info(30, 1, "Backup notes")];
    say(&mut c, "останови всё", 60_000);
    b.st().running = vec![run_info(31, 1, "Backup notes")];
    say(&mut c, "alles stoppen", 70_000);
    assert_eq!(&b.st().stopped[5..], &[30, 31]);
}

#[test]
fn stop_ambiguous_by_name_and_failures() {
    let (b, mut c) = setup(lib());
    b.st().running = vec![run_info(10, 1, "Backup notes"), run_info(11, 2, "Backup photos")];
    let r = say(&mut c, "stop backup", 0);
    assert_eq!(kinds(&r), vec![ReplyKind::Problem]);
    assert!(b.st().stopped.is_empty());
    b.st().fail_stop = Some("access denied".into());
    let r = say(&mut c, "stop backup notes", 10_000);
    assert_eq!(kinds(&r), vec![ReplyKind::Problem]);
    assert!(r[0].text.contains("access denied"));
    let r = say(&mut c, "stop all", 20_000);
    assert_eq!(kinds(&r), vec![ReplyKind::Problem]);
}

#[test]
fn whats_running_and_lists() {
    let (b, mut c) = setup(lib());
    assert!(say(&mut c, "what's running", 0)[0].text.contains("Nothing"));
    b.st().running = vec![run_info(10, 1, "Backup notes")];
    let r = say(&mut c, "что запущено", 10_000);
    assert!(r[0].text.contains("Backup notes") && r[0].text.starts_with("Сейчас"));
    let r = c.handle_text("list automations", 20_000);
    assert!(r[0].text.contains("Tidy screenshots"));
    assert!(!r[0].speak, "long lists are not spoken");
    let r = c.handle_text("what can I say", 30_000);
    assert!(r[0].text.contains("Run Backup notes"));
    assert!(!r[0].speak);
    assert_eq!(c.handle_text("hilfe", 40_000)[0].kind, ReplyKind::Info);
}

// ---- settings ----------------------------------------------------------------------------------------

#[test]
fn whitelisted_settings_are_applied_and_reported() {
    let (b, mut c) = setup_cs(lib());
    assert_eq!(say(&mut c, "dark mode", 0)[0].text, "Dark theme is on");
    assert_eq!(say(&mut c, "turn off notifications", 10_000)[0].kind, ReplyKind::Confirm);
    assert_eq!(say(&mut c, "yes", 15_000)[0].text, "Notifications are off");
    assert_eq!(say(&mut c, "включи проверку обновлений", 20_000)[0].text, "Проверка обновлений включена");
    assert_eq!(say(&mut c, "Dota live helper on", 30_000)[0].text, "Dota live helper is on");
    assert_eq!(say(&mut c, "dunkles Design", 40_000)[0].text, "Dunkles Design ist an");
    assert_eq!(
        b.st().applied,
        vec![
            SettingChange::Theme("dark".into()),
            SettingChange::Notifications(false),
            SettingChange::UpdateCheck(true),
            SettingChange::DotaLiveHelper(true),
            SettingChange::Theme("dark".into())
        ]
    );
}

#[test]
fn autostart_and_language_ask_first() {
    let (b, mut c) = setup_cs(lib());
    let r = say(&mut c, "autostart on", 0);
    assert_eq!(kinds(&r), vec![ReplyKind::Confirm]);
    assert!(b.st().applied.is_empty());
    assert_eq!(say(&mut c, "yes", 1_000)[0].text, "Autostart is on");
    assert_eq!(b.st().applied, vec![SettingChange::Autostart(true)]);
    let r = say(&mut c, "смени язык на немецкий", 10_000);
    assert_eq!(kinds(&r), vec![ReplyKind::Confirm]);
    say(&mut c, "нет", 11_000);
    assert_eq!(b.st().applied.len(), 1);
    say(&mut c, "Sprache Deutsch", 20_000);
    assert_eq!(c.answer_confirmation(true, 21_000)[0].kind, ReplyKind::Done);
    assert_eq!(b.st().applied[1], SettingChange::Language("de".into()));
}

#[test]
fn settings_can_be_switched_off() {
    let s = VoiceSettings { change_settings: false, ..VoiceSettings::default() };
    let (b, mut c) = setup_with(lib(), s);
    let r = say(&mut c, "dark mode", 0);
    assert_eq!(kinds(&r), vec![ReplyKind::Problem]);
    let r = say(&mut c, "autostart on", 10_000);
    assert_eq!(kinds(&r), vec![ReplyKind::Problem]);
    assert_eq!(b.actions(), 0);
    assert!(c.commands().iter().all(|e| e.group != "setting"));
}

#[test]
fn setting_failures_are_reported() {
    let (b, mut c) = setup_cs(lib());
    b.st().fail_apply = Some("no permission".into());
    let r = say(&mut c, "dark mode", 0);
    assert_eq!(kinds(&r), vec![ReplyKind::Problem]);
    assert!(r[0].text.contains("no permission"));
}

#[test]
fn nothing_outside_the_whitelist_reaches_the_backend() {
    let mut sys = auto(9, "Shell");
    sys.allow_system = true;
    let (b, mut c) = setup(vec![auto(1, "Backup notes"), sys]);
    let mut now = 0;
    for phrase in [
        "open the shell",
        "delete everything",
        "change allowed folders",
        "turn on system control",
        "disable confirmations",
        "allow system control",
        "turn off confirmations",
        "format the disk",
        "включи системное управление",
        "отключи подтверждения",
        "lösche alle Dateien",
        "schalte die Bestätigung aus",
        "enable voice system automations",
        "powershell remove-item C:\\ -recurse",
    ] {
        now += 10_000;
        let r = say(&mut c, phrase, now);
        assert_eq!(kinds(&r), vec![ReplyKind::Problem], "{phrase}: {}", text(&r));
        assert!(c.pending_confirmation(now).is_none(), "{phrase}");
    }
    assert_eq!(b.actions(), 0);
    assert!(c.take_control_requests().is_empty());
}

#[test]
fn voice_controls_are_queued_for_the_session() {
    let (b, mut c) = setup(lib());
    assert_eq!(say(&mut c, "mute", 0)[0].kind, ReplyKind::Done);
    say(&mut c, "stop listening", 10_000);
    say(&mut c, "spoken feedback on", 20_000);
    assert_eq!(c.take_control_requests(), vec![ControlRequest::Mute, ControlRequest::StopListening, ControlRequest::SpokenFeedback(true)]);
    assert!(c.take_control_requests().is_empty());
    assert!(c.settings().spoken_feedback);
    assert_eq!(b.actions(), 0);
}

// ---- notices and odd input -------------------------------------------------------------------------------

#[test]
fn every_heard_command_is_noticed_before_it_is_acted_on() {
    let (b, mut c) = setup(lib());
    say(&mut c, "run tidy screenshots", 0);
    say(&mut c, "stop listening", 10_000);
    say(&mut c, "dark mode", 20_000);
    say(&mut c, "yes", 30_000);
    c.answer_confirmation(false, 40_000);
    let s = b.st();
    assert_eq!(s.notices[0], "Voice: run tidy screenshots");
    assert_eq!(s.notices[1], "Voice: stop listening");
    assert_eq!(s.notices[2], "Voice: dark mode");
    assert_eq!(s.notices.len(), 5);
}

#[test]
fn odd_input_never_panics() {
    let (b, mut c) = setup(lib());
    let long = "run ".to_string() + &"backup ".repeat(10_000);
    let mut now = 0;
    for t in ["", "   ", "🙂🙂", "שלום עולם", "مرحبا بالعالم", "\0\u{1}", "run 🙂", &long, "stop \u{202e}", "ǅ İ ẞ", "a\u{300}\u{300}"] {
        now += 10_000;
        let _ = c.handle_text(t, now);
        let _ = c.handle(&Transcript { id: now, text: t.into(), confidence: Some(f32::NAN), language: Some("xx".into()) }, now + 1);
    }
    assert!(b.st().started.is_empty());
    // an empty utterance is not even announced
    assert!(b.st().notices.iter().all(|n| n != "Voice: "));
}

// ---- aliases ---------------------------------------------------------------------------------------------

#[test]
fn aliases_run_their_automation() {
    let s = VoiceSettings {
        aliases: vec![
            VoiceAlias { phrase: "nightly".into(), automation_id: 2, automation_name: "Backup photos".into() },
            VoiceAlias { phrase: "Бэкап".into(), automation_id: 4, automation_name: "Бэкап заметок".into() },
            VoiceAlias { phrase: "gone".into(), automation_id: 99, automation_name: "Deleted".into() },
        ],
        ..VoiceSettings::default()
    };
    let (b, mut c) = setup_with(lib(), s);
    assert_eq!(say(&mut c, "run nightly", 0)[0].kind, ReplyKind::Done);
    assert_eq!(say(&mut c, "запусти бэкап", 10_000)[0].kind, ReplyKind::Done);
    assert_eq!(say(&mut c, "run gone", 20_000)[0].kind, ReplyKind::Problem);
    assert_eq!(b.st().started, vec![2, 4]);
    let cmds = c.commands();
    assert!(cmds.iter().any(|e| e.group == "alias" && e.say == "Run nightly"));
    assert!(cmds.iter().all(|e| !e.say.contains("gone")));
}

#[test]
fn an_alias_equal_to_another_name_is_not_guessed() {
    let s = VoiceSettings { aliases: vec![VoiceAlias { phrase: "tidy screenshots".into(), automation_id: 1, automation_name: "Backup notes".into() }], ..VoiceSettings::default() };
    let (b, mut c) = setup_with(lib(), s);
    let r = say(&mut c, "run tidy screenshots", 0);
    assert_eq!(kinds(&r), vec![ReplyKind::Problem]);
    assert!(b.st().started.is_empty());
}

// ---- commands() -------------------------------------------------------------------------------------------

#[test]
fn commands_reflect_the_backend_and_all_work() {
    let mut off = auto(7, "Disabled one");
    off.enabled = false;
    let mut sys = auto(9, "Shell thing");
    sys.allow_system = true;
    for (lang, verb) in [("en", "Run "), ("ru", "Запусти "), ("de", "Starte ")] {
        let s = VoiceSettings { language: lang.into(), run_system_automations: true, change_settings: true, ..VoiceSettings::default() };
        let (b, c) = setup_with(vec![auto(1, "Backup notes"), off.clone(), sys.clone()], s.clone());
        b.st().running = vec![run_info(5, 1, "Backup notes")];
        let cmds = c.commands();
        assert!(cmds.iter().any(|e| e.say == format!("{verb}Backup notes") && e.group == "automation"), "{lang}");
        assert!(cmds.iter().all(|e| !e.say.contains("Disabled one")), "{lang}");
        assert!(cmds.iter().any(|e| e.say.contains("Shell thing") && e.does.contains(match lang { "en" => "confirm", "ru" => "подтверждение", _ => "fragt" })), "{lang}");
        assert!(cmds.iter().any(|e| e.group == "control" && e.say.contains("Backup notes") && !e.say.starts_with(verb)), "stop entry while running ({lang})");
        assert!(cmds.iter().any(|e| e.group == "setting"));
        // every listed command really parses to something that works
        for e in &cmds {
            let intent = grammar::parse(&e.say, &s);
            assert_ne!(intent, Intent::Unknown, "{lang}: {:?} is listed but not understood", e.say);
            match e.group.as_str() {
                "setting" => assert!(matches!(intent, Intent::Setting(_)), "{lang}: {}", e.say),
                "automation" | "alias" => assert!(matches!(intent, Intent::Run { .. }), "{lang}: {}", e.say),
                _ => {}
            }
        }
    }
    // stopped: the stop entry disappears
    let (_, c) = setup(vec![auto(1, "Backup notes")]);
    assert!(c.commands().iter().all(|e| e.say != "Stop Backup notes"));
    // the system automation is listed only when it can work
    let (_, c) = setup(vec![sys]);
    assert!(c.commands().iter().all(|e| !e.say.contains("Shell thing")));
}

// ---- settings persistence -------------------------------------------------------------------------------

#[test]
fn settings_round_trip_through_a_temp_folder() {
    let dir = tempfile::tempdir().unwrap();
    let f = dir.path().join("voice.json");
    assert_eq!(settings::load_from(&f), VoiceSettings::default());
    let mut s = VoiceSettings { enabled: true, consented: true, spoken_feedback: true, ..VoiceSettings::default() };
    s.aliases.push(VoiceAlias { phrase: "nightly".into(), automation_id: 2, automation_name: "Backup photos".into() });
    settings::save_to(&f, &s).unwrap();
    assert_eq!(settings::load_from(&f), s);
    // nothing about audio or what was said is in the file
    let json = std::fs::read_to_string(&f).unwrap();
    assert!(!json.contains("transcript") && !json.contains("audio"));
    // invalid settings are rejected and leave the file alone
    let mut bad = s.clone();
    bad.push_key = "nonsense".into();
    assert!(settings::save_to(&f, &bad).is_err());
    assert_eq!(settings::load_from(&f), s);
    // a damaged file is kept, not deleted
    std::fs::write(&f, "{{{").unwrap();
    assert_eq!(settings::load_from(&f), VoiceSettings::default());
    let kept = std::fs::read_dir(dir.path()).unwrap().flatten().filter(|e| e.file_name().to_string_lossy().starts_with("voice.corrupt-")).count();
    assert_eq!(kept, 1);
}

// ---- hardening: confidence, risk, privacy ----------------------------------------------------------

fn heard(c: &mut VoiceController, id: u64, text: &str, confidence: Option<f32>, now: u64) -> Vec<Reply> {
    c.handle(&Transcript { id, text: text.into(), confidence, language: None }, now)
}

fn sys(id: i64, name: &str) -> AutomationInfo {
    AutomationInfo { allow_system: true, ..auto(id, name) }
}

fn sys_settings(mode: ListenMode) -> VoiceSettings {
    VoiceSettings { mode, run_system_automations: true, ..VoiceSettings::default() }
}

#[test]
fn without_confidence_only_exact_names_run_at_once() {
    let (b, mut c) = setup(lib());
    // the real engine reports no confidence: an exact name still runs, unconfirmed
    let r = heard(&mut c, 1, "run tidy screenshots", None, 0);
    assert_eq!(kinds(&r), vec![ReplyKind::Done]);
    assert_eq!(b.started_and_confirmed(), (vec![3], vec![false]));
    // a fuzzy name (one word of two) always asks first, even typed
    let r = heard(&mut c, 2, "run tidy", None, 10_000);
    assert_eq!(kinds(&r), vec![ReplyKind::Confirm], "{}", text(&r));
    assert!(r[0].text.contains("Did you mean Tidy screenshots"));
    assert_eq!(b.st().started, vec![3]);
    assert_eq!(say(&mut c, "no", 15_000)[0].text, "Cancelled.");
    let r = say(&mut c, "run Aufräumen", 20_000);
    assert_eq!(kinds(&r), vec![ReplyKind::Confirm], "{}", text(&r));
    // yes (typed) runs it and counts as confirmed
    assert_eq!(say(&mut c, "yes", 21_000)[0].kind, ReplyKind::Done);
    assert_eq!(b.started_and_confirmed(), (vec![3, 5], vec![false, true]));
}

#[test]
fn a_fuzzy_name_never_runs_a_system_automation() {
    let (b, mut c) = setup_with(vec![sys(9, "Power down PC")], sys_settings(ListenMode::PushToTalk));
    let r = say(&mut c, "run power", 0);
    assert_eq!(kinds(&r), vec![ReplyKind::Problem]);
    assert!(r[0].text.contains("exact name"), "{}", r[0].text);
    assert!(c.pending_confirmation(1).is_none() && b.st().started.is_empty());
    // exact name: a question that names it
    let r = say(&mut c, "run power down pc", 1_000);
    assert_eq!(kinds(&r), vec![ReplyKind::Confirm]);
    assert!(r[0].text.matches("Power down PC").count() >= 2, "{}", r[0].text);
}

#[test]
fn spoken_yes_for_a_system_run_needs_push_to_talk_and_confidence() {
    // push-to-talk, the real engine (no confidence): not enough, the question stays
    let (b, mut c) = setup_with(vec![sys(9, "Power off")], sys_settings(ListenMode::PushToTalk));
    heard(&mut c, 1, "run power off", None, 0);
    let r = heard(&mut c, 2, "yes", None, 1_000);
    assert_eq!(kinds(&r), vec![ReplyKind::Info]);
    assert!(r[0].text.contains("press Yes"), "{}", r[0].text);
    assert!(b.st().started.is_empty() && c.pending_confirmation(1_001).is_some());
    let r = heard(&mut c, 3, "yes", Some(0.6), 5_000);
    assert_eq!(kinds(&r), vec![ReplyKind::Info]);
    assert!(b.st().started.is_empty());
    // a confident yes works
    let r = heard(&mut c, 4, "yes", Some(0.95), 9_000);
    assert_eq!(kinds(&r), vec![ReplyKind::Done]);
    assert_eq!(b.started_and_confirmed(), (vec![9], vec![true]));

    // the button always works
    let (b, mut c) = setup_with(vec![sys(9, "Power off")], sys_settings(ListenMode::PushToTalk));
    heard(&mut c, 1, "run power off", None, 0);
    assert_eq!(c.answer_confirmation(true, 500)[0].kind, ReplyKind::Done);
    assert_eq!(b.st().confirmed, vec![true]);
}

#[test]
fn always_on_spoken_yes_never_confirms_high_risk() {
    let (b, mut c) = setup_with(vec![sys(9, "Power off")], sys_settings(ListenMode::AlwaysOn));
    heard(&mut c, 1, "run power off", Some(0.99), 0);
    for (i, conf) in [None, Some(0.99), Some(1.0)].into_iter().enumerate() {
        let r = heard(&mut c, 10 + i as u64, "yes", conf, 1_000 + 4_000 * i as u64);
        assert_eq!(kinds(&r), vec![ReplyKind::Info]);
        assert!(r[0].text.contains("Yes button") && r[0].text.contains("push-to-talk"), "{}", r[0].text);
    }
    assert!(b.st().started.is_empty());
    assert!(c.pending_confirmation(13_000).is_some(), "the question stays for the button");
    assert_eq!(c.answer_confirmation(true, 14_000)[0].kind, ReplyKind::Done);
    assert_eq!(b.started_and_confirmed(), (vec![9], vec![true]));
    // autostart is high risk too
    let (b, mut c) = setup_with(lib(), VoiceSettings { mode: ListenMode::AlwaysOn, change_settings: true, ..VoiceSettings::default() });
    assert_eq!(say(&mut c, "turn autostart on", 0)[0].kind, ReplyKind::Confirm);
    assert_eq!(heard(&mut c, 1, "yes", Some(0.9), 1_000)[0].kind, ReplyKind::Info);
    assert!(b.st().applied.is_empty());
    assert_eq!(c.answer_confirmation(true, 2_000)[0].kind, ReplyKind::Done);
    assert_eq!(b.st().applied, vec![SettingChange::Autostart(true)]);
    // "no" by voice is always fine
    let (b, mut c) = setup_with(vec![sys(9, "Power off")], sys_settings(ListenMode::AlwaysOn));
    heard(&mut c, 1, "run power off", None, 0);
    assert_eq!(heard(&mut c, 2, "no", None, 1_000)[0].text, "Cancelled.");
    assert!(b.st().started.is_empty());
}

#[test]
fn an_always_on_spoken_yes_to_a_fuzzy_question_runs_unconfirmed() {
    let s = VoiceSettings { mode: ListenMode::AlwaysOn, ..VoiceSettings::default() };
    let (b, mut c) = setup_with(lib(), s);
    assert_eq!(heard(&mut c, 1, "run tidy", None, 0)[0].kind, ReplyKind::Confirm);
    assert_eq!(heard(&mut c, 2, "yes", None, 1_000)[0].kind, ReplyKind::Done);
    // started, but a room voice does not count as a confirmation for system steps in the chain
    assert_eq!(b.started_and_confirmed(), (vec![3], vec![false]));
}

#[test]
fn pending_questions_are_cancelled_and_never_auto_confirm() {
    let s = sys_settings(ListenMode::PushToTalk);
    let (b, mut c) = setup_with(vec![sys(9, "Power off"), auto(3, "Tidy screenshots")], s.clone());
    say(&mut c, "run power off", 0);
    c.cancel_pending();
    assert!(c.pending_confirmation(1).is_none());
    assert!(c.answer_confirmation(true, 2)[0].text.contains("nothing"));
    assert!(b.st().started.is_empty());
    // settings update
    say(&mut c, "run power off", 100_000);
    c.update_settings(s.clone());
    assert!(c.pending_confirmation(100_001).is_none());
    assert!(say(&mut c, "yes", 101_000)[0].text.contains("nothing"));
    // expiry: even the button after the deadline does nothing
    say(&mut c, "run power off", 200_000);
    let r = c.answer_confirmation(true, 200_000 + CONFIRM_TTL_MS + 1);
    assert!(r[0].text.contains("expired"), "{}", r[0].text);
    assert!(b.st().started.is_empty());
}

#[test]
fn stop_with_a_pending_question_declines_it_and_says_what_still_runs() {
    let s = sys_settings(ListenMode::PushToTalk);
    let (b, mut c) = setup_with(vec![sys(9, "Power off"), auto(3, "Tidy screenshots")], s);
    b.st().running = vec![run_info(50, 3, "Tidy screenshots")];
    say(&mut c, "run power off", 0);
    let r = say(&mut c, "stop", 1_000);
    assert_eq!(kinds(&r), vec![ReplyKind::Info]);
    assert!(
        r[0].text.contains("Cancelled the question") && r[0].text.contains("Tidy screenshots is still running") && r[0].text.contains("stop again"),
        "{}",
        r[0].text
    );
    assert!(b.st().stopped.is_empty() && b.st().started.is_empty());
    assert!(c.pending_confirmation(1_001).is_none());
    // nothing running: a plain cancel
    b.st().running.clear();
    say(&mut c, "run power off", 10_000);
    assert_eq!(say(&mut c, "cancel", 11_000)[0].text, "Cancelled.");
    // the second stop really stops, and says it only asked
    b.st().running = vec![run_info(50, 3, "Tidy screenshots")];
    let r = say(&mut c, "stop", 20_000);
    assert!(r[0].text.starts_with("Asked Tidy screenshots to stop"), "{}", r[0].text);
    assert_eq!(b.st().stopped, vec![50]);
    assert!(!r[0].text.contains("Stopped"));
}

#[test]
fn the_in_flight_guard_uses_is_running() {
    let (b, mut c) = setup(lib());
    b.st().start_makes_running = true;
    assert_eq!(say(&mut c, "run tidy screenshots", 0)[0].kind, ReplyKind::Done);
    // a just-started run counts at once (even past the launch guard)
    let r = say(&mut c, "start tidy screenshots", LAUNCH_GUARD_MS + 1_000);
    assert!(r[0].text.contains("already running"), "{}", r[0].text);
    assert_eq!(b.st().started, vec![3]);
    // and it is the stop target
    assert_eq!(say(&mut c, "stop", LAUNCH_GUARD_MS + 10_000)[0].kind, ReplyKind::Done);
    assert_eq!(b.st().stopped.len(), 1);
}

#[test]
fn notices_only_echo_understood_commands() {
    let (b, mut c) = setup(lib());
    say(&mut c, "run tidy screenshots", 0);
    say(&mut c, "my bank password is hunter two", 10_000);
    heard(&mut c, 99, "what a lovely day for a picnic", None, 20_000);
    let notices = b.st().notices.clone();
    assert_eq!(notices, vec!["Voice: run tidy screenshots", "Voice: not understood", "Voice: not understood"]);
    // the reply does not repeat what was not understood either
    let r = say(&mut c, "purple monkey dishwasher", 30_000);
    assert!(!r[0].text.contains("purple") && !r[0].text.contains("dishwasher"), "{}", r[0].text);
    assert_eq!(kinds(&r), vec![ReplyKind::Problem]);
    for (i, n) in b.st().notices.iter().enumerate() {
        assert!(!n.contains("purple") && !n.contains("hunter") && !n.contains("picnic"), "notice {i}: {n}");
    }
}

// ---- settings --------------------------------------------------------------------------------------

#[test]
fn changing_settings_by_voice_is_off_by_default() {
    assert!(!VoiceSettings::default().change_settings);
    let (b, mut c) = setup(lib());
    let r = say(&mut c, "dark mode", 0);
    assert_eq!(kinds(&r), vec![ReplyKind::Problem]);
    assert!(b.st().applied.is_empty());
    assert!(c.commands().iter().all(|e| e.group != "setting"));
}

fn grammar_phrase(ch: &SettingChange) -> String {
    match ch {
        SettingChange::Notifications(b) => format!("notifications {}", if *b { "on" } else { "off" }),
        SettingChange::Theme(t) => match t.as_str() {
            "dark" => "dark mode".into(),
            _ => "theme system".into(),
        },
        SettingChange::Language(_) => "language german".into(),
        SettingChange::UpdateCheck(b) => format!("update check {}", if *b { "on" } else { "off" }),
        SettingChange::DotaLiveHelper(b) => format!("dota live helper {}", if *b { "on" } else { "off" }),
        SettingChange::Autostart(b) => format!("autostart {}", if *b { "on" } else { "off" }),
    }
}

#[test]
fn silencing_settings_ask_first_and_the_whitelist_is_closed() {
    let s = VoiceSettings { change_settings: true, ..VoiceSettings::default() };
    let (b, mut c) = setup_with(lib(), s.clone());
    assert_eq!(say(&mut c, "notifications off", 0)[0].kind, ReplyKind::Confirm);
    assert!(b.st().applied.is_empty());
    assert_eq!(say(&mut c, "no", 1_000)[0].text, "Cancelled.");
    assert_eq!(say(&mut c, "update check off", 10_000)[0].kind, ReplyKind::Confirm);
    assert_eq!(say(&mut c, "yes", 11_000)[0].kind, ReplyKind::Done);
    assert_eq!(b.st().applied, vec![SettingChange::UpdateCheck(false)]);
    // turning things on, or harmless settings, needs no question
    assert_eq!(say(&mut c, "notifications on", 20_000)[0].kind, ReplyKind::Done);
    assert_eq!(say(&mut c, "dark mode", 30_000)[0].kind, ReplyKind::Done);

    // every variant is low risk or asks; high risk implies asking
    let all = [
        SettingChange::Notifications(true),
        SettingChange::Notifications(false),
        SettingChange::Theme("dark".into()),
        SettingChange::Theme("system".into()),
        SettingChange::Language("de".into()),
        SettingChange::UpdateCheck(true),
        SettingChange::UpdateCheck(false),
        SettingChange::DotaLiveHelper(true),
        SettingChange::DotaLiveHelper(false),
        SettingChange::Autostart(true),
        SettingChange::Autostart(false),
    ];
    for ch in &all {
        assert!(!ch.high_risk() || ch.needs_confirmation(), "{ch:?}");
        let (b, mut c) = setup_with(lib(), s.clone());
        let before = b.actions();
        let r = c.handle_text(&grammar_phrase(ch), 0);
        if ch.needs_confirmation() {
            assert_eq!(r[0].kind, ReplyKind::Confirm, "{ch:?}: {}", text(&r));
            assert_eq!(b.actions(), before, "{ch:?} must not apply before the answer");
        } else {
            assert_eq!(r[0].kind, ReplyKind::Done, "{ch:?}: {}", text(&r));
        }
    }

    // nothing else can be reached: any phrase is Unknown or one of the whitelist
    for p in [
        "turn off the firewall",
        "change the download folder",
        "enable allow system control",
        "turn on run system automations",
        "turn on change settings",
        "set the wake phrase to computer",
        "delete all automations",
        "restore the backup",
        "set telegram token",
        "open the settings",
        "disable the antivirus",
        "set the password",
        "turn on always on",
        "export automations",
        "shut down the computer",
    ] {
        match grammar::parse(p, &s) {
            Intent::Unknown | Intent::Mute | Intent::Unmute | Intent::SpokenFeedback(_) | Intent::Run { .. } | Intent::Stop { .. } => {}
            Intent::Setting(ch) => assert!(all.contains(&ch) || matches!(ch, SettingChange::Theme(_)), "{p}: {ch:?}"),
            other => panic!("{p}: {other:?}"),
        }
    }
}

// ---- aliases ---------------------------------------------------------------------------------------

#[test]
fn an_alias_only_works_while_the_automation_keeps_its_name() {
    let alias = |phrase: &str, id: i64, name: &str| VoiceAlias { phrase: phrase.into(), automation_id: id, automation_name: name.into() };
    let s = VoiceSettings {
        aliases: vec![alias("my tidy", 3, "tidy SCREENSHOTS"), alias("notes", 1, "Old name"), alias("gone", 77, "Gone")],
        ..VoiceSettings::default()
    };
    let (b, mut c) = setup_with(lib(), s);
    // case-insensitive name equality: works
    assert_eq!(say(&mut c, "run my tidy", 0)[0].kind, ReplyKind::Done);
    assert_eq!(b.st().started, vec![3]);
    // renamed / id reused: ignored, with an explanation
    let r = say(&mut c, "run notes", 10_000);
    assert_eq!(kinds(&r), vec![ReplyKind::Problem]);
    assert!(r[0].text.contains("The alias 'notes' points to an automation that changed; set it up again"), "{}", r[0].text);
    let r = say(&mut c, "run gone", 20_000);
    assert!(r[0].text.contains("set it up again"), "{}", r[0].text);
    assert_eq!(b.st().started, vec![3]);
    // listed for the UI, and not offered as commands
    let stale: Vec<String> = c.stale_aliases().into_iter().map(|a| a.phrase).collect();
    assert_eq!(stale, vec!["notes", "gone"]);
    let cmds = c.commands();
    assert!(cmds.iter().any(|e| e.group == "alias" && e.say == "Run my tidy"));
    assert!(cmds.iter().all(|e| e.say != "Run notes" && e.say != "Run gone"));
}

#[test]
fn two_automations_with_the_same_spoken_name_are_ambiguous() {
    let (b, mut c) = setup(vec![auto(1, "Sync Files"), auto(2, "sync files!")]);
    let r = say(&mut c, "run sync files", 0);
    assert_eq!(kinds(&r), vec![ReplyKind::Problem]);
    assert!(r[0].text.contains("Which one"));
    assert!(b.st().started.is_empty());
    // an alias equal to another automation's name is ambiguous too
    let s = VoiceSettings {
        aliases: vec![VoiceAlias { phrase: "tidy screenshots".into(), automation_id: 1, automation_name: "Backup notes".into() }],
        ..VoiceSettings::default()
    };
    let (b, mut c) = setup_with(lib(), s);
    assert_eq!(say(&mut c, "run tidy screenshots", 0)[0].kind, ReplyKind::Problem);
    assert!(b.st().started.is_empty());
}

#[test]
fn typing_the_wake_phrase_behaves_like_speech() {
    let (b, mut c) = setup(lib());
    for (i, t) in ["hey localflow run tidy screenshots", "Hey LocalFlow, please run tidy screenshots", "hey localflow stop"].iter().enumerate() {
        let r = say(&mut c, t, 10_000 * i as u64);
        assert!(!r.is_empty(), "{t}");
        assert_ne!(r[0].kind, ReplyKind::Problem, "{t}: {}", text(&r));
    }
    assert_eq!(b.st().started, vec![3, 3]); // the first two (past the launch guard) both run it
    // the wake phrase alone is nothing, like in speech
    assert!(say(&mut c, "hey localflow", 100_000).is_empty());
    // and with a custom wake phrase
    let s = VoiceSettings { wake_phrase: "computer please".into(), ..VoiceSettings::default() };
    let (b, mut c) = setup_with(lib(), s);
    assert_eq!(say(&mut c, "computer please run tidy screenshots", 0)[0].kind, ReplyKind::Done);
    assert_eq!(b.st().started, vec![3]);
}

// ---- documentation phrases -------------------------------------------------------------------------

#[test]
fn every_phrase_in_the_docs_and_mocks_is_understood() {
    let s = VoiceSettings::default();
    let run = |t: &str| matches!(grammar::parse(t, &s), Intent::Run { target } if !target.is_empty());
    for p in ["run Zip backup", "запусти Zip backup", "starte Zip backup", "run Organize PDF files", "run organize pdf files"] {
        assert!(run(p), "{p}");
    }
    let stop = |t: &str| matches!(grammar::parse(t, &s), Intent::Stop { target: None });
    for p in ["stop", "стоп", "stopp"] {
        assert!(stop(p), "{p}");
    }
    for p in ["stop all", "stop everything", "останови всё", "stoppe alles"] {
        assert_eq!(grammar::parse(p, &s), Intent::StopAll, "{p}");
    }
    for p in ["what is running", "what's running", "что запущено", "was läuft"] {
        assert_eq!(grammar::parse(p, &s), Intent::WhatsRunning, "{p}");
    }
    for p in ["what can I say", "что можно сказать", "что ты умеешь", "was kann ich sagen"] {
        assert_eq!(grammar::parse(p, &s), Intent::ListCommands, "{p}");
    }
    assert_eq!(grammar::parse("yes", &s), Intent::Yes);
    assert_eq!(grammar::parse("no", &s), Intent::No);
    assert_eq!(grammar::parse("ja", &s), Intent::Yes);
    assert_eq!(grammar::parse("да", &s), Intent::Yes);
    assert_eq!(grammar::parse("mute", &s), Intent::Mute);
    assert_eq!(grammar::parse("list automations", &s), Intent::ListAutomations);
    assert_eq!(grammar::parse("dark mode", &s), Intent::Setting(SettingChange::Theme("dark".into())));
    assert_eq!(grammar::parse("dark theme", &s), Intent::Setting(SettingChange::Theme("dark".into())));
    assert_eq!(grammar::parse("turn notifications off", &s), Intent::Setting(SettingChange::Notifications(false)));
    assert_eq!(grammar::parse("turn spoken feedback on", &s), Intent::SpokenFeedback(true));
}
