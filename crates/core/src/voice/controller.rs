//! `VoiceController`: recognised utterance -> replies.
//!
//! Safety design (mirrors the Telegram remote control, `remote.rs`):
//! - every recognised command is announced with `backend.notice("Voice: <command>")` before anything
//!   is done; speech that is not understood is announced as exactly "Voice: not understood", never
//!   with the heard text;
//! - speech is parsed by the closed grammar in `grammar.rs`; there is no free-form action;
//! - automations are found by `matcher.rs`, which never guesses between close matches;
//! - the real recogniser reports no confidence (`None`), which counts as LOW everywhere: only an
//!   exact name of an enabled, non-system automation runs at once; any fuzzy match asks
//!   "did you mean ...?" first;
//! - disabled automations are refused; automations with "Allow system control" need an exact name,
//!   the `run_system_automations` setting *and* a yes/no question that names them every time;
//! - high-risk questions (system automations, autostart, notifications off) are never confirmed by
//!   a low-confidence spoken "yes", and in always-on mode never by a spoken "yes" at all: the
//!   on-screen Yes button (or typing, or push-to-talk) is needed;
//! - only the whitelisted settings can change, and only when `change_settings` is on (off by
//!   default); autostart, language and switching notifications / update check off ask first;
//! - a pending question expires after [`CONFIRM_TTL_MS`] and is cancelled by any other command;
//! - duplicates (same transcript id, same text within [`DUPLICATE_TEXT_MS`], same automation
//!   within [`LAUNCH_GUARD_MS`]) and automations that are already running are never started twice.
//!
//! Voice's own switches (stop listening, mute, spoken feedback) are not something the controller
//! can do; it queues a [`ControlRequest`] that the session drains with
//! [`VoiceController::take_control_requests`] after `handle`.

use std::{
    collections::{HashMap, HashSet, VecDeque},
    sync::Arc,
};

use super::{
    grammar::{self, detect_language, normalize, Intent, Lang},
    matcher::{self, Candidate, Match},
    AutomationInfo, CommandExample, ListenMode, Reply, ReplyKind, RunningInfo, SettingChange, Transcript, VoiceBackend, VoiceSettings,
};

/// How long a yes/no question waits for an answer.
pub const CONFIRM_TTL_MS: u64 = 15_000;
/// The same normalised text again within this time is a re-emitted recognition result: ignored.
pub const DUPLICATE_TEXT_MS: u64 = 3_000;
/// The same automation is not started twice within this time, whatever the transcript ids.
pub const LAUNCH_GUARD_MS: u64 = 5_000;
/// A recognition below this confidence asks before running anything.
pub const LOW_CONFIDENCE: f32 = 0.5;
/// A "yes" recognised below this confidence is not trusted: it has to be repeated.
pub const YES_MIN_CONFIDENCE: f32 = 0.4;
/// A spoken "yes" to a high-risk question (push-to-talk only) needs at least this confidence. The
/// real engine reports none, so in practice such questions are answered with the on-screen button.
pub const HIGH_RISK_YES_CONFIDENCE: f32 = 0.8;
/// Longest utterance considered (characters); the rest is cut off.
pub const MAX_TEXT_CHARS: usize = 400;
/// How many candidate names an "which one?" question lists.
pub const MAX_LISTED: usize = 3;
const SEEN_IDS: usize = 256;
const TEXT_ID_BASE: u64 = 1 << 63;

/// Things voice itself must do, for the listening session (the controller cannot).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ControlRequest {
    StopListening,
    Mute,
    Unmute,
    SpokenFeedback(bool),
}

#[derive(Debug, Clone)]
enum Action {
    Run { id: i64, name: String, system: bool },
    Setting(SettingChange),
}

impl Action {
    fn high_risk(&self) -> bool {
        match self {
            Action::Run { system, .. } => *system,
            Action::Setting(change) => change.high_risk(),
        }
    }
}

/// How a yes/no was given.
#[derive(Debug, Clone, Copy)]
enum Answer {
    /// The on-screen button.
    Button,
    /// Typed in the app ("try a phrase"): the person is at the keyboard.
    Typed,
    /// Spoken, with the engine's confidence (`None` = the engine gives none = low).
    Spoken(Option<f32>),
}

#[derive(Debug, Clone)]
struct Pending {
    action: Action,
    prompt: String,
    expires_at: u64,
    lang: Lang,
    high_risk: bool,
}

pub struct VoiceController {
    backend: Arc<dyn VoiceBackend>,
    settings: VoiceSettings,
    pending: Option<Pending>,
    expired_recently: bool,
    seen_ids: HashSet<u64>,
    seen_order: VecDeque<u64>,
    last_text: Option<(String, u64)>,
    last_launch: HashMap<i64, u64>,
    text_seq: u64,
    controls: Vec<ControlRequest>,
}

macro_rules! t {
    ($l:expr, $en:literal, $ru:literal, $de:literal) => {
        match $l {
            Lang::En => format!($en),
            Lang::Ru => format!($ru),
            Lang::De => format!($de),
        }
    };
}

fn reply(kind: ReplyKind, text: String, speak: bool) -> Reply {
    Reply { kind, text, speak }
}
fn done(text: String) -> Reply {
    reply(ReplyKind::Done, text, true)
}
fn problem(text: String) -> Reply {
    reply(ReplyKind::Problem, text, true)
}
fn info(text: String) -> Reply {
    reply(ReplyKind::Info, text, true)
}

fn clip(text: &str, n: usize) -> String {
    if text.chars().count() <= n {
        text.to_string()
    } else {
        text.chars().take(n).collect::<String>() + "..."
    }
}

fn or_list(lang: Lang, names: &[String]) -> String {
    let word = match lang {
        Lang::En => "or",
        Lang::Ru => "или",
        Lang::De => "oder",
    };
    match names {
        [] => String::new(),
        [one] => one.clone(),
        [init @ .., last] => format!("{} {word} {last}", init.join(", ")),
    }
}

fn and_list(lang: Lang, names: &[String]) -> String {
    let word = match lang {
        Lang::En => "and",
        Lang::Ru => "и",
        Lang::De => "und",
    };
    match names {
        [] => String::new(),
        [one] => one.clone(),
        [init @ .., last] => format!("{} {word} {last}", init.join(", ")),
    }
}

fn lang_name(lang: Lang, code: &str) -> String {
    match (lang, code) {
        (Lang::En, "en") => "English",
        (Lang::En, "ru") => "Russian",
        (Lang::En, "de") => "German",
        (Lang::En, _) => "automatic",
        (Lang::Ru, "en") => "английский",
        (Lang::Ru, "ru") => "русский",
        (Lang::Ru, "de") => "немецкий",
        (Lang::Ru, _) => "автоматически",
        (Lang::De, "en") => "Englisch",
        (Lang::De, "ru") => "Russisch",
        (Lang::De, "de") => "Deutsch",
        (Lang::De, _) => "automatisch",
    }
    .to_string()
}

/// A short statement of the new state, e.g. "Dark theme is on".
pub fn setting_text(l: Lang, change: &SettingChange) -> String {
    let on = |b: &bool| *b;
    match change {
        SettingChange::Notifications(b) => {
            if on(b) {
                t!(l, "Notifications are on", "Уведомления включены", "Benachrichtigungen sind an")
            } else {
                t!(l, "Notifications are off", "Уведомления выключены", "Benachrichtigungen sind aus")
            }
        }
        SettingChange::Theme(th) => match th.as_str() {
            "dark" => t!(l, "Dark theme is on", "Тёмная тема включена", "Dunkles Design ist an"),
            "light" => t!(l, "Light theme is on", "Светлая тема включена", "Helles Design ist an"),
            _ => t!(l, "The theme follows the system", "Тема следует за системой", "Das Design folgt dem System"),
        },
        SettingChange::Language(code) => {
            let n = lang_name(l, code);
            t!(l, "Language set to {n}", "Язык: {n}", "Sprache: {n}")
        }
        SettingChange::UpdateCheck(b) => {
            if on(b) {
                t!(l, "Update check is on", "Проверка обновлений включена", "Update-Prüfung ist an")
            } else {
                t!(l, "Update check is off", "Проверка обновлений выключена", "Update-Prüfung ist aus")
            }
        }
        SettingChange::DotaLiveHelper(b) => {
            if on(b) {
                t!(l, "Dota live helper is on", "Помощник Dota включён", "Dota-Live-Helfer ist an")
            } else {
                t!(l, "Dota live helper is off", "Помощник Dota выключен", "Dota-Live-Helfer ist aus")
            }
        }
        SettingChange::Autostart(b) => {
            if on(b) {
                t!(l, "Autostart is on", "Автозапуск включён", "Autostart ist an")
            } else {
                t!(l, "Autostart is off", "Автозапуск выключен", "Autostart ist aus")
            }
        }
    }
}

fn setting_question(l: Lang, change: &SettingChange) -> String {
    match change {
        SettingChange::Autostart(true) => t!(l, "Start LocalFlow with the computer? Say yes or no.", "Запускать LocalFlow вместе с компьютером? Скажите да или нет.", "LocalFlow mit dem Computer starten? Sagen Sie ja oder nein."),
        SettingChange::Autostart(false) => t!(l, "Stop starting LocalFlow with the computer? Say yes or no.", "Больше не запускать LocalFlow вместе с компьютером? Скажите да или нет.", "LocalFlow nicht mehr mit dem Computer starten? Sagen Sie ja oder nein."),
        SettingChange::Language(code) => {
            let n = lang_name(l, code);
            t!(l, "Change the language to {n}? Say yes or no.", "Сменить язык на {n}? Скажите да или нет.", "Sprache auf {n} umstellen? Sagen Sie ja oder nein.")
        }
        SettingChange::Notifications(false) => t!(l, "Turn notifications off? Then you won't see what voice does. Say yes or no.", "Выключить уведомления? Тогда вы не увидите, что делает голос. Скажите да или нет.", "Benachrichtigungen ausschalten? Dann sehen Sie nicht, was die Sprachsteuerung tut. Sagen Sie ja oder nein."),
        SettingChange::UpdateCheck(false) => t!(l, "Turn the update check off? Say yes or no.", "Выключить проверку обновлений? Скажите да или нет.", "Die Update-Prüfung ausschalten? Sagen Sie ja oder nein."),
        other => setting_text(l, other),
    }
}

fn stale_alias_text(l: Lang, phrase: &str) -> String {
    let phrase = clip(phrase, 60);
    t!(
        l,
        "The alias '{phrase}' points to an automation that changed; set it up again.",
        "Псевдоним «{phrase}» указывает на изменённую автоматизацию; настройте его заново.",
        "Der Alias „{phrase}“ verweist auf eine geänderte Automatisierung; richten Sie ihn neu ein."
    )
}

impl VoiceController {
    pub fn new(backend: Arc<dyn VoiceBackend>, settings: VoiceSettings) -> Self {
        VoiceController {
            backend,
            settings,
            pending: None,
            expired_recently: false,
            seen_ids: HashSet::new(),
            seen_order: VecDeque::new(),
            last_text: None,
            last_launch: HashMap::new(),
            text_seq: TEXT_ID_BASE,
            controls: Vec::new(),
        }
    }

    pub fn update_settings(&mut self, settings: VoiceSettings) {
        self.settings = settings;
        // Anything asked under the old settings is no longer valid.
        self.pending = None;
        self.expired_recently = false;
    }

    /// Drop the pending question without answering it (mute, mode change, the app speaking ...).
    /// A question is never confirmed by being dropped or by expiring.
    pub fn cancel_pending(&mut self) {
        self.pending = None;
        self.expired_recently = false;
    }

    /// Aliases that are ignored because their automation is gone or was renamed since the alias
    /// was made (for the settings screen: "set it up again").
    pub fn stale_aliases(&self) -> Vec<super::VoiceAlias> {
        super::settings::stale_aliases(&self.settings, &self.backend.automations())
    }

    pub fn settings(&self) -> &VoiceSettings {
        &self.settings
    }

    /// What voice itself has been asked to do (stop listening, mute, ...) since the last call.
    pub fn take_control_requests(&mut self) -> Vec<ControlRequest> {
        std::mem::take(&mut self.controls)
    }

    /// Handle one recognised utterance. `now_ms`: a monotonic clock in milliseconds (injected for tests).
    pub fn handle(&mut self, transcript: &Transcript, now_ms: u64) -> Vec<Reply> {
        if self.seen_ids.contains(&transcript.id) {
            return Vec::new();
        }
        self.seen_ids.insert(transcript.id);
        self.seen_order.push_back(transcript.id);
        if self.seen_order.len() > SEEN_IDS {
            if let Some(old) = self.seen_order.pop_front() {
                self.seen_ids.remove(&old);
            }
        }
        self.process(&transcript.text, transcript.confidence, transcript.language.as_deref(), false, now_ms)
    }

    /// Typed text ("try a phrase" in the UI): parsed exactly like speech, but the person is at the
    /// keyboard, so it counts as full confidence (and can confirm high-risk questions).
    pub fn handle_text(&mut self, text: &str, now_ms: u64) -> Vec<Reply> {
        self.text_seq += 1;
        let id = self.text_seq;
        let _ = id;
        self.process(text, Some(1.0), None, true, now_ms)
    }

    /// The on-screen Yes/No buttons.
    pub fn answer_confirmation(&mut self, yes: bool, now_ms: u64) -> Vec<Reply> {
        let lang = self.pending.as_ref().map(|p| p.lang).unwrap_or_else(|| self.default_lang());
        self.backend.notice(&format!("Voice: {}", if yes { "yes (button)" } else { "no (button)" }));
        self.expire(now_ms);
        self.answer(yes, Answer::Button, lang, now_ms)
    }

    /// The question waiting for an answer, if any (expires after a short time).
    pub fn pending_confirmation(&self, now_ms: u64) -> Option<String> {
        self.pending.as_ref().filter(|p| now_ms < p.expires_at).map(|p| p.prompt.clone())
    }

    // ---- pipeline ------------------------------------------------------------------------------

    fn default_lang(&self) -> Lang {
        Lang::from_code(&self.settings.language).unwrap_or(Lang::En)
    }

    fn reply_lang(&self, text: &str, hint: Option<&str>) -> Lang {
        match detect_language(text) {
            Some("ru") => Lang::Ru,
            Some("de") => Lang::De,
            _ => hint.and_then(Lang::from_code).unwrap_or_else(|| self.default_lang()),
        }
    }

    fn expire(&mut self, now: u64) {
        if self.pending.as_ref().is_some_and(|p| now >= p.expires_at) {
            self.pending = None;
            self.expired_recently = true;
        }
    }

    fn process(&mut self, text: &str, confidence: Option<f32>, hint: Option<&str>, typed: bool, now: u64) -> Vec<Reply> {
        let text: String = text.chars().take(MAX_TEXT_CHARS).collect();
        let norm = normalize(&text);
        if norm.is_empty() {
            return Vec::new();
        }
        if let Some((last, at)) = &self.last_text {
            if *last == norm && now.saturating_sub(*at) < DUPLICATE_TEXT_MS {
                return Vec::new();
            }
        }
        // The wake phrase on its own is not a command (in speech it only arms the session).
        let wake = grammar::words(&self.settings.wake_phrase);
        if !wake.is_empty() && grammar::words(&text) == wake {
            return Vec::new();
        }
        self.last_text = Some((norm, now));
        let intent = grammar::parse(&text, &self.settings);
        if intent == Intent::Unknown {
            // What was not understood is never repeated in notifications.
            self.backend.notice("Voice: not understood");
        } else {
            self.backend.notice(&format!("Voice: {}", clip(text.trim(), 200)));
        }
        self.expire(now);
        let lang = self.reply_lang(&text, hint);
        let mut out = Vec::new();

        if self.pending.is_some() {
            match intent {
                Intent::Yes => {
                    let how = if typed { Answer::Typed } else { Answer::Spoken(confidence) };
                    return self.answer(true, how, lang, now);
                }
                Intent::No => return self.answer(false, Answer::Spoken(confidence), lang, now),
                Intent::Stop { target: None } => return vec![self.decline_for_stop(lang)],
                _ => {
                    self.pending = None;
                    out.push(info(t!(lang, "Cancelled.", "Отменено.", "Abgebrochen.")));
                }
            }
        } else if matches!(intent, Intent::Yes | Intent::No) {
            return vec![self.nothing_to_confirm(lang)];
        }
        self.expired_recently = false;
        let norm = norm_for_suggestions(&text);
        out.extend(self.run_intent(intent, &norm, confidence, lang, now));
        out
    }

    /// "stop" / "cancel" while a question is pending declines the question; it does not stop runs.
    fn decline_for_stop(&mut self, lang: Lang) -> Reply {
        self.pending = None;
        self.expired_recently = false;
        let running = self.backend.running();
        if running.is_empty() {
            return info(t!(lang, "Cancelled.", "Отменено.", "Abgebrochen."));
        }
        let names: Vec<String> = distinct_ids(&running).iter().filter_map(|i| running.iter().find(|r| r.automation_id == *i)).map(|r| r.name.clone()).collect();
        let l = and_list(lang, &names);
        info(t!(
            lang,
            "Cancelled the question. {l} is still running - say stop again to stop it.",
            "Вопрос отменён. {l} всё ещё выполняется - скажите стоп ещё раз, чтобы остановить.",
            "Frage abgebrochen. {l} läuft noch - sagen Sie noch einmal stopp, um es zu stoppen."
        ))
    }

    fn nothing_to_confirm(&mut self, lang: Lang) -> Reply {
        let expired = std::mem::take(&mut self.expired_recently);
        if expired {
            info(t!(lang, "That question has expired. Nothing was done.", "Вопрос устарел. Ничего не сделано.", "Die Frage ist abgelaufen. Es wurde nichts getan."))
        } else {
            info(t!(lang, "There is nothing to confirm.", "Подтверждать нечего.", "Es gibt nichts zu bestätigen."))
        }
    }

    fn answer(&mut self, yes: bool, how: Answer, lang: Lang, now: u64) -> Vec<Reply> {
        let Some(p) = self.pending.as_ref() else {
            return vec![self.nothing_to_confirm(lang)];
        };
        self.expired_recently = false;
        let always_on = self.settings.mode == ListenMode::AlwaysOn;
        let spoken = matches!(how, Answer::Spoken(_));
        if yes {
            if let Answer::Spoken(conf) = how {
                if p.high_risk {
                    let plang = p.lang;
                    if always_on {
                        return vec![info(t!(
                            plang,
                            "This must be confirmed with the Yes button on the screen, or switch to push-to-talk. A spoken yes is not enough in always-on mode.",
                            "Это нужно подтвердить кнопкой «Да» на экране или переключитесь на режим «нажми и говори». В режиме постоянного прослушивания «да» голосом недостаточно.",
                            "Das muss mit der Ja-Taste auf dem Bildschirm bestätigt werden, oder wechseln Sie zu Push-to-Talk. Ein gesprochenes Ja reicht im Dauerbetrieb nicht."
                        ))];
                    }
                    if conf.map_or(true, |c| c < HIGH_RISK_YES_CONFIDENCE) {
                        return vec![info(t!(
                            plang,
                            "I can't be sure that was you. Please press Yes on the screen.",
                            "Не уверен, что это были вы. Нажмите «Да» на экране.",
                            "Ich bin nicht sicher, dass Sie das waren. Bitte drücken Sie Ja auf dem Bildschirm."
                        ))];
                    }
                } else if conf.is_some_and(|c| c < YES_MIN_CONFIDENCE) {
                    // The retry must not be mistaken for a re-emitted result.
                    self.last_text = None;
                    return vec![reply(
                        ReplyKind::Confirm,
                        t!(lang, "I'm not sure I heard \"yes\". Please say it again or press Yes.", "Не уверен, что услышал «да». Скажите ещё раз или нажмите «Да».", "Ich bin nicht sicher, ob ich „ja“ gehört habe. Bitte wiederholen oder Ja drücken."),
                        true,
                    )];
                }
            }
        }
        let Some(p) = self.pending.take() else {
            return vec![self.nothing_to_confirm(lang)];
        };
        if !yes {
            return vec![info(t!(p.lang, "Cancelled.", "Отменено.", "Abgebrochen."))];
        }
        // An always-on spoken yes can come from the room (or from our own voice): it starts the
        // run, but does not count as a real confirmation for chained system steps.
        let confirmed = !(always_on && spoken);
        match p.action {
            Action::Run { id, name, .. } => self.start(id, &name, p.lang, now, confirmed),
            Action::Setting(change) => self.apply(&change, p.lang),
        }
    }

    fn ask(&mut self, action: Action, prompt: String, lang: Lang, now: u64) -> Reply {
        let high_risk = action.high_risk();
        self.pending = Some(Pending { action, prompt: prompt.clone(), expires_at: now + CONFIRM_TTL_MS, lang, high_risk });
        reply(ReplyKind::Confirm, prompt, true)
    }

    fn run_intent(&mut self, intent: Intent, norm: &str, confidence: Option<f32>, lang: Lang, now: u64) -> Vec<Reply> {
        match intent {
            Intent::Run { target } => self.run(&target, confidence, lang, now),
            Intent::Stop { target } => self.stop(target.as_deref(), lang),
            Intent::StopAll => self.stop_all(lang),
            Intent::WhatsRunning => vec![self.whats_running(lang)],
            Intent::ListCommands => vec![self.list_commands(lang)],
            Intent::ListAutomations => vec![self.list_automations(lang)],
            Intent::Setting(change) => self.setting(change, lang, now),
            Intent::StopListening => {
                self.controls.push(ControlRequest::StopListening);
                vec![done(t!(lang, "Okay, I stopped listening.", "Хорошо, я перестал слушать.", "Okay, ich höre nicht mehr zu."))]
            }
            Intent::Mute => {
                self.controls.push(ControlRequest::Mute);
                vec![done(t!(lang, "Muted.", "Микрофон отключён.", "Stummgeschaltet."))]
            }
            Intent::Unmute => {
                self.controls.push(ControlRequest::Unmute);
                vec![done(t!(lang, "Microphone is on again.", "Микрофон снова включён.", "Das Mikrofon ist wieder an."))]
            }
            Intent::SpokenFeedback(on) => {
                self.controls.push(ControlRequest::SpokenFeedback(on));
                self.settings.spoken_feedback = on;
                vec![done(if on {
                    t!(lang, "I will speak my replies.", "Буду озвучивать ответы.", "Ich spreche meine Antworten.")
                } else {
                    t!(lang, "I will only show my replies.", "Ответы только текстом.", "Ich zeige die Antworten nur an.")
                })]
            }
            Intent::Yes | Intent::No => vec![self.nothing_to_confirm(lang)],
            Intent::Unknown => vec![self.unknown(norm, lang)],
        }
    }

    // ---- running automations -----------------------------------------------------------------

    fn run(&mut self, target: &str, confidence: Option<f32>, lang: Lang, now: u64) -> Vec<Reply> {
        if target.trim().is_empty() {
            return vec![problem(t!(lang, "Which automation? Say 'run' and its name.", "Какую автоматизацию? Скажите «запусти» и название.", "Welche Automatisierung? Sagen Sie „starte“ und den Namen."))];
        }
        let list = self.backend.automations();
        let live = self.live_aliases(&list);
        let m = matcher::find(target, &list, &live);
        if !matches!(m, Match::Exact(_)) {
            let wanted = normalize(target);
            if let Some(stale) = super::settings::stale_aliases(&self.settings, &list).into_iter().find(|a| normalize(&a.phrase) == wanted) {
                return vec![problem(stale_alias_text(lang, &stale.phrase))];
            }
        }
        let (id, exact) = match m {
            Match::Exact(id) => (id, true),
            Match::Likely(id) | Match::Weak(id) => (id, false),
            Match::Ambiguous(c) => return vec![problem(self.which_one(lang, &c))],
            Match::None => return vec![self.not_found(target, &list, &live, lang)],
        };
        let Some(auto) = list.iter().find(|a| a.id == id) else {
            return vec![self.not_found(target, &list, &live, lang)];
        };
        if let Some(refusal) = self.gate(auto, lang, now) {
            return vec![refusal];
        }
        let name = auto.name.clone();
        if auto.allow_system {
            if !exact {
                return vec![problem(t!(
                    lang,
                    "{name} can control this PC, so I only run it when you say its exact name.",
                    "{name} может управлять компьютером, поэтому я запускаю её только по точному названию.",
                    "{name} kann den PC steuern, deshalb starte ich sie nur beim genauen Namen."
                ))];
            }
            let prompt = t!(
                lang,
                "{name} can control this PC (commands, keys, programs). Run {name}? Say yes or no.",
                "{name} может управлять этим компьютером (команды, клавиши, программы). Запустить {name}? Скажите да или нет.",
                "{name} kann diesen PC steuern (Befehle, Tasten, Programme). {name} ausführen? Sagen Sie ja oder nein."
            );
            return vec![self.ask(Action::Run { id, name, system: true }, prompt, lang, now)];
        }
        // No confidence (the real engine) counts as low, but only fuzzy matches and clearly
        // low-confidence hearings ask; an exact name of an enabled automation runs at once.
        if !exact || confidence.is_some_and(|c| c < LOW_CONFIDENCE) {
            let prompt = t!(lang, "Did you mean {name}? Say yes to run it, or no.", "Вы имели в виду {name}? Скажите да, чтобы запустить, или нет.", "Meinten Sie {name}? Sagen Sie ja zum Starten oder nein.");
            return vec![self.ask(Action::Run { id, name, system: false }, prompt, lang, now)];
        }
        self.start(id, &name, lang, now, false)
    }

    /// Aliases whose automation still exists under the name they were made for.
    fn live_aliases(&self, list: &[AutomationInfo]) -> Vec<super::VoiceAlias> {
        let stale = super::settings::stale_aliases(&self.settings, list);
        self.settings.aliases.iter().filter(|a| !stale.contains(a)).cloned().collect()
    }

    /// Everything that can forbid starting this automation right now (None = fine).
    fn gate(&self, auto: &AutomationInfo, lang: Lang, now: u64) -> Option<Reply> {
        let name = &auto.name;
        if !auto.enabled {
            return Some(problem(t!(
                lang,
                "{name} is turned off, so I won't run it. Turn it on in LocalFlow first.",
                "{name} выключена, поэтому я её не запускаю. Сначала включите её в LocalFlow.",
                "{name} ist ausgeschaltet, deshalb starte ich sie nicht. Schalten Sie sie zuerst in LocalFlow ein."
            )));
        }
        if auto.allow_system && !self.settings.run_system_automations {
            return Some(problem(t!(
                lang,
                "{name} can control this PC, and voice isn't allowed to start those. You can allow it in the voice settings, or run it yourself.",
                "{name} может управлять компьютером, а голосу это запрещено. Разрешите в настройках голоса или запустите вручную.",
                "{name} kann den PC steuern, und Sprache darf solche nicht starten. Erlauben Sie es in den Spracheinstellungen oder starten Sie sie selbst."
            )));
        }
        if self.backend.is_running(auto.id) {
            return Some(info(t!(lang, "{name} is already running.", "{name} уже запущена.", "{name} läuft bereits.")));
        }
        if self.last_launch.get(&auto.id).is_some_and(|at| now.saturating_sub(*at) < LAUNCH_GUARD_MS) {
            return Some(info(t!(lang, "I just started {name}.", "Я только что запустил {name}.", "Ich habe {name} gerade erst gestartet.")));
        }
        None
    }

    /// Re-check and start (also after a confirmation, when the world may have changed).
    fn start(&mut self, id: i64, name: &str, lang: Lang, now: u64, confirmed: bool) -> Vec<Reply> {
        let list = self.backend.automations();
        let Some(auto) = list.iter().find(|a| a.id == id) else {
            return vec![problem(t!(lang, "{name} doesn't exist any more.", "{name} больше не существует.", "{name} gibt es nicht mehr."))];
        };
        if let Some(refusal) = self.gate(auto, lang, now) {
            return vec![refusal];
        }
        let name = auto.name.clone();
        match self.backend.start(id, confirmed) {
            Ok(()) => {
                self.last_launch.insert(id, now);
                vec![done(t!(lang, "Running {name}.", "Запускаю {name}.", "Starte {name}."))]
            }
            Err(e) => vec![problem(t!(lang, "I couldn't start {name}: {e}", "Не удалось запустить {name}: {e}", "{name} konnte nicht gestartet werden: {e}"))],
        }
    }

    fn which_one(&self, lang: Lang, candidates: &[Candidate]) -> String {
        let shown: Vec<String> = candidates.iter().take(MAX_LISTED).map(|c| c.name.clone()).collect();
        let list = or_list(lang, &shown);
        let more = candidates.len() > MAX_LISTED;
        let dots = if more { " ..." } else { "" };
        t!(
            lang,
            "Which one: {list}{dots}? Say the full name.",
            "Какую именно: {list}{dots}? Назовите полное название.",
            "Welche meinen Sie: {list}{dots}? Sagen Sie den vollständigen Namen."
        )
    }

    fn not_found(&self, target: &str, list: &[AutomationInfo], aliases: &[super::VoiceAlias], lang: Lang) -> Reply {
        let target = clip(target, 60);
        let near = matcher::suggest(&target, list, aliases, 2);
        let mut text = t!(lang, "I couldn't find an automation called '{target}'.", "Не нашёл автоматизацию «{target}».", "Ich habe keine Automatisierung „{target}“ gefunden.");
        if !near.is_empty() {
            let names: Vec<String> = near.iter().map(|c| c.name.clone()).collect();
            let l = or_list(lang, &names);
            text.push_str(&t!(lang, " Did you mean {l}?", " Может быть, {l}?", " Meinten Sie {l}?"));
        } else {
            text.push_str(&t!(lang, " Say 'list automations' to hear them.", " Скажите «покажи автоматизации».", " Sagen Sie „zeige Automatisierungen“."));
        }
        problem(text)
    }

    // ---- stopping --------------------------------------------------------------------------------

    fn stop(&mut self, target: Option<&str>, lang: Lang) -> Vec<Reply> {
        let running = self.backend.running();
        if running.is_empty() {
            return vec![info(t!(lang, "Nothing is running.", "Сейчас ничего не запущено.", "Es läuft gerade nichts."))];
        }
        let ids = distinct_ids(&running);
        let chosen: i64 = match target {
            None if ids.len() == 1 => ids[0],
            None => {
                let names: Vec<String> = ids.iter().filter_map(|i| running.iter().find(|r| r.automation_id == *i)).map(|r| r.name.clone()).collect();
                let l = and_list(lang, &names);
                return vec![problem(t!(
                    lang,
                    "Several are running: {l}. Say 'stop' and a name, or 'stop everything'.",
                    "Запущено несколько: {l}. Скажите «стоп» и название или «останови всё».",
                    "Es laufen mehrere: {l}. Sagen Sie „stopp“ und einen Namen oder „stoppe alles“."
                ))];
            }
            Some(target) => {
                let pseudo: Vec<AutomationInfo> = ids
                    .iter()
                    .filter_map(|i| running.iter().find(|r| r.automation_id == *i))
                    .map(|r| AutomationInfo { id: r.automation_id, name: r.name.clone(), description: String::new(), enabled: true, allow_system: false })
                    .collect();
                match matcher::find(target, &pseudo, &self.live_aliases(&pseudo)) {
                    Match::Exact(id) | Match::Likely(id) => id,
                    Match::Weak(id) => {
                        let name = pseudo.iter().find(|a| a.id == id).map(|a| a.name.clone()).unwrap_or_default();
                        return vec![problem(t!(lang, "Did you mean {name}? Say 'stop {name}'.", "Вы имели в виду {name}? Скажите «стоп {name}».", "Meinten Sie {name}? Sagen Sie „stopp {name}“."))];
                    }
                    Match::Ambiguous(c) => return vec![problem(self.which_one(lang, &c))],
                    Match::None => {
                        let target = clip(target, 60);
                        return vec![problem(t!(lang, "'{target}' isn't running.", "«{target}» сейчас не запущена.", "„{target}“ läuft nicht."))];
                    }
                }
            }
        };
        let runs: Vec<&RunningInfo> = running.iter().filter(|r| r.automation_id == chosen).collect();
        let name = runs[0].name.clone();
        vec![self.stop_runs(&runs, &name, lang)]
    }

    fn stop_runs(&self, runs: &[&RunningInfo], name: &str, lang: Lang) -> Reply {
        let (mut stopped, mut finished) = (0, 0);
        let mut errors = Vec::new();
        for r in runs {
            match self.backend.stop(r.run_id) {
                Ok(true) => stopped += 1,
                Ok(false) => finished += 1,
                Err(e) => errors.push(e),
            }
        }
        if let Some(e) = errors.first() {
            problem(t!(lang, "I couldn't stop {name}: {e}", "Не удалось остановить {name}: {e}", "{name} konnte nicht gestoppt werden: {e}"))
        } else if stopped > 0 {
            done(t!(lang, "Asked {name} to stop.", "Попросил {name} остановиться.", "Habe {name} gebeten, zu stoppen."))
        } else {
            let _ = finished;
            info(t!(lang, "{name} had already finished.", "{name} уже завершилась.", "{name} war schon fertig."))
        }
    }

    fn stop_all(&mut self, lang: Lang) -> Vec<Reply> {
        let running = self.backend.running();
        if running.is_empty() {
            return vec![info(t!(lang, "Nothing is running.", "Сейчас ничего не запущено.", "Es läuft gerade nichts."))];
        }
        let (mut stopped, mut errors) = (0usize, Vec::new());
        for r in &running {
            match self.backend.stop(r.run_id) {
                Ok(true) => stopped += 1,
                Ok(false) => {}
                Err(e) => errors.push(format!("{}: {e}", r.name)),
            }
        }
        if !errors.is_empty() {
            let l = errors.join("; ");
            return vec![problem(t!(lang, "I couldn't stop everything: {l}", "Не удалось остановить всё: {l}", "Nicht alles konnte gestoppt werden: {l}"))];
        }
        if stopped == 0 {
            return vec![info(t!(lang, "Everything had already finished.", "Всё уже завершилось.", "Alles war schon fertig."))];
        }
        vec![done(t!(lang, "Asked everything to stop ({stopped}).", "Попросил всё остановиться ({stopped}).", "Habe alles gebeten, zu stoppen ({stopped})."))]
    }

    fn whats_running(&self, lang: Lang) -> Reply {
        let running = self.backend.running();
        if running.is_empty() {
            return info(t!(lang, "Nothing is running.", "Сейчас ничего не запущено.", "Es läuft gerade nichts."));
        }
        let names: Vec<String> = distinct_ids(&running).iter().filter_map(|i| running.iter().find(|r| r.automation_id == *i)).map(|r| r.name.clone()).collect();
        let l = and_list(lang, &names);
        reply(ReplyKind::Info, t!(lang, "Running now: {l}.", "Сейчас запущено: {l}.", "Gerade laufen: {l}."), names.len() <= 3)
    }

    // ---- lists -----------------------------------------------------------------------------------

    fn list_commands(&self, lang: Lang) -> Reply {
        let all = self.commands();
        let mut sample: Vec<String> = all.iter().filter(|c| c.group == "automation").take(3).map(|c| c.say.clone()).collect();
        sample.extend(all.iter().filter(|c| c.group == "control").take(4).map(|c| c.say.clone()));
        let l = sample.join(", ");
        reply(ReplyKind::Info, t!(lang, "You can say: {l}. The full list is in the voice settings.", "Можно сказать: {l}. Полный список в настройках голоса.", "Sie können sagen: {l}. Die ganze Liste steht in den Spracheinstellungen."), false)
    }

    fn list_automations(&self, lang: Lang) -> Reply {
        let list: Vec<String> = self.backend.automations().into_iter().filter(|a| a.enabled).map(|a| a.name).collect();
        if list.is_empty() {
            return info(t!(lang, "You have no enabled automations.", "Нет включённых автоматизаций.", "Es gibt keine eingeschalteten Automatisierungen."));
        }
        let n = list.len();
        let shown = list.iter().take(8).cloned().collect::<Vec<_>>().join(", ");
        let more = if n > 8 { format!(" (+{})", n - 8) } else { String::new() };
        reply(ReplyKind::Info, t!(lang, "Automations ({n}): {shown}{more}.", "Автоматизации ({n}): {shown}{more}.", "Automatisierungen ({n}): {shown}{more}."), false)
    }

    // ---- settings --------------------------------------------------------------------------------

    fn setting(&mut self, change: SettingChange, lang: Lang, now: u64) -> Vec<Reply> {
        if !self.settings.change_settings {
            return vec![problem(t!(
                lang,
                "Changing settings by voice is switched off. You can allow it in the voice settings.",
                "Изменение настроек голосом выключено. Это можно разрешить в настройках голоса.",
                "Einstellungen per Sprache zu ändern ist ausgeschaltet. Sie können es in den Spracheinstellungen erlauben."
            ))];
        }
        if change.needs_confirmation() {
            let prompt = setting_question(lang, &change);
            return vec![self.ask(Action::Setting(change), prompt, lang, now)];
        }
        self.apply(&change, lang)
    }

    fn apply(&mut self, change: &SettingChange, lang: Lang) -> Vec<Reply> {
        if !self.settings.change_settings {
            return vec![problem(t!(lang, "Changing settings by voice is switched off.", "Изменение настроек голосом выключено.", "Einstellungen per Sprache zu ändern ist ausgeschaltet."))];
        }
        match self.backend.apply(change) {
            Ok(_) => vec![done(setting_text(lang, change))],
            Err(e) => vec![problem(t!(lang, "I couldn't change that: {e}", "Не удалось изменить: {e}", "Das ließ sich nicht ändern: {e}"))],
        }
    }

    // ---- not understood ------------------------------------------------------------------------

    fn unknown(&self, norm: &str, lang: Lang) -> Reply {
        // The heard text is not repeated: only commands that were understood are echoed.
        let mut msg = t!(lang, "I didn't understand that.", "Я не понял.", "Das habe ich nicht verstanden.");
        let list = self.backend.automations();
        let live = self.live_aliases(&list);
        let words: Vec<&str> = norm.split(' ').collect();
        let mut near = matcher::suggest(norm, &list, &live, 2);
        if near.is_empty() && words.len() > 1 {
            near = matcher::suggest(&words[1..].join(" "), &list, &live, 2);
        }
        let verb = t!(lang, "Run", "Запусти", "Starte");
        let near: Vec<String> = near.into_iter().filter(|c| c.score >= 0.6).map(|c| format!("{verb} {}", c.name)).collect();
        if !near.is_empty() {
            let l = or_list(lang, &near);
            msg.push_str(&t!(lang, " Did you mean '{l}'?", " Может быть, «{l}»?", " Meinten Sie „{l}“?"));
        }
        msg.push_str(&t!(lang, " Say 'what can I say' for the list.", " Скажите «что ты умеешь», чтобы узнать команды.", " Sagen Sie „was kann ich sagen“ für die Liste."));
        problem(msg)
    }

    // ---- what can be said ----------------------------------------------------------------------

    /// What can be said right now: derived from the real automations, aliases and settings, in
    /// the settings language (`auto` -> English; the app passes the UI language). Only commands
    /// that would actually work now are listed.
    pub fn commands(&self) -> Vec<CommandExample> {
        let l = self.default_lang();
        let autos = self.backend.automations();
        let running = self.backend.running();
        let verb = t!(l, "Run", "Запусти", "Starte");
        let stop_verb = t!(l, "Stop", "Останови", "Stoppe");
        let confirm_note = t!(l, " (asks you to confirm)", " (спросит подтверждение)", " (fragt nach)");
        let mut out = Vec::new();
        let mut ex = |group: &str, say: String, does: String| out.push(CommandExample { say, does, group: group.into() });

        let runnable = |a: &AutomationInfo| a.enabled && (!a.allow_system || self.settings.run_system_automations);
        for a in autos.iter().filter(|a| runnable(a)) {
            let mut does = if a.description.trim().is_empty() { t!(l, "Runs this automation", "Запускает эту автоматизацию", "Startet diese Automatisierung") } else { a.description.trim().to_string() };
            if a.allow_system {
                does.push_str(&confirm_note);
            }
            ex("automation", format!("{verb} {}", a.name), does);
        }
        for al in &self.live_aliases(&autos) {
            if let Some(a) = autos.iter().find(|a| a.id == al.automation_id).filter(|a| runnable(a)) {
                let name = &a.name;
                let mut does = t!(l, "Runs {name}", "Запускает {name}", "Startet {name}");
                if a.allow_system {
                    does.push_str(&confirm_note);
                }
                ex("alias", format!("{verb} {}", al.phrase), does);
            }
        }
        for r in distinct_ids(&running).iter().filter_map(|i| running.iter().find(|r| r.automation_id == *i)) {
            let name = &r.name;
            ex("control", format!("{stop_verb} {name}"), t!(l, "Stops {name}", "Останавливает {name}", "Stoppt {name}"));
        }
        ex("control", t!(l, "Stop", "Стоп", "Stopp"), t!(l, "Stops the one automation that is running", "Останавливает единственную запущенную автоматизацию", "Stoppt die eine laufende Automatisierung"));
        ex("control", t!(l, "Stop everything", "Останови всё", "Stoppe alles"), t!(l, "Stops every running automation", "Останавливает все запущенные автоматизации", "Stoppt alle laufenden Automatisierungen"));
        ex("control", t!(l, "What's running", "Что запущено", "Was läuft gerade"), t!(l, "Lists the running automations", "Называет запущенные автоматизации", "Nennt die laufenden Automatisierungen"));
        ex("control", t!(l, "List automations", "Покажи автоматизации", "Zeige Automatisierungen"), t!(l, "Lists your automations", "Перечисляет ваши автоматизации", "Nennt Ihre Automatisierungen"));
        ex("control", t!(l, "What can I say", "Что ты умеешь", "Was kann ich sagen"), t!(l, "Lists what you can say", "Перечисляет команды", "Nennt die möglichen Befehle"));
        ex("control", t!(l, "Stop listening", "Перестань слушать", "Hör auf zuzuhören"), t!(l, "Switches voice control off", "Выключает голосовое управление", "Schaltet die Sprachsteuerung aus"));
        ex("control", t!(l, "Mute", "Выключи микрофон", "Mikrofon stumm"), t!(l, "Mutes the microphone", "Отключает микрофон", "Schaltet das Mikrofon stumm"));
        ex("control", t!(l, "Spoken feedback on", "Включи озвучивание", "Sprachausgabe an"), t!(l, "Speaks the replies aloud", "Озвучивает ответы", "Spricht die Antworten"));
        ex("control", t!(l, "Spoken feedback off", "Выключи озвучивание", "Sprachausgabe aus"), t!(l, "Shows the replies as text only", "Ответы только текстом", "Zeigt die Antworten nur als Text"));

        if self.settings.change_settings {
            use SettingChange::*;
            let rows: Vec<(String, SettingChange)> = vec![
                (t!(l, "Dark mode", "Тёмная тема", "Dunkles Design"), Theme("dark".into())),
                (t!(l, "Light mode", "Светлая тема", "Helles Design"), Theme("light".into())),
                (t!(l, "Notifications on", "Включи уведомления", "Benachrichtigungen an"), Notifications(true)),
                (t!(l, "Notifications off", "Выключи уведомления", "Benachrichtigungen aus"), Notifications(false)),
                (t!(l, "Update check on", "Включи проверку обновлений", "Updateprüfung an"), UpdateCheck(true)),
                (t!(l, "Update check off", "Выключи проверку обновлений", "Updateprüfung aus"), UpdateCheck(false)),
                (t!(l, "Dota live helper on", "Включи помощник доты", "Dota Helfer an"), DotaLiveHelper(true)),
                (t!(l, "Dota live helper off", "Выключи помощник доты", "Dota Helfer aus"), DotaLiveHelper(false)),
                (t!(l, "Autostart on", "Включи автозапуск", "Autostart an"), Autostart(true)),
                (t!(l, "Autostart off", "Выключи автозапуск", "Autostart aus"), Autostart(false)),
                (t!(l, "Language English", "Язык английский", "Sprache Englisch"), Language("en".into())),
                (t!(l, "Language Russian", "Язык русский", "Sprache Russisch"), Language("ru".into())),
                (t!(l, "Language German", "Язык немецкий", "Sprache Deutsch"), Language("de".into())),
            ];
            for (say, change) in rows {
                let mut does = setting_text(l, &change);
                if change.needs_confirmation() {
                    does.push_str(&confirm_note);
                }
                ex("setting", say, does);
            }
        }
        out
    }
}

fn distinct_ids(running: &[RunningInfo]) -> Vec<i64> {
    let mut ids: Vec<i64> = Vec::new();
    for r in running {
        if !ids.contains(&r.automation_id) {
            ids.push(r.automation_id);
        }
    }
    ids
}

fn norm_for_suggestions(text: &str) -> String {
    normalize(text)
}
