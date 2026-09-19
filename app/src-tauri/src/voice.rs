//! Voice control on the desktop: the manager that owns the listening session, the backend that
//! lets the voice layer use `LocalFlow`, and the Tauri commands of the voice contract.
//!
//! Safety rules kept here:
//! - Voice is off by default. The microphone is opened only by a running session (settings
//!   `enabled` + consent + an installed model) or by the user's own "test microphone" click.
//! - No audio is stored and no transcript is logged: only state changes and error kinds.
//! - Anything that fails while building a session ends in the `error` state with a friendly
//!   message; nothing crashes and the rest of the app keeps working.
//! - The session is stopped (microphone released) when voice is switched off, when the device or
//!   language changes, and on exit (never blocking the exit for more than about two seconds).

use std::{
    collections::HashMap,
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc, Arc, Mutex, MutexGuard,
    },
    time::{Duration, Instant},
};

use localflow_core::{
    voice::{
        audio,
        controller::VoiceController,
        engine::{self, EngineInfo, ModelStatus},
        session::{Session, SessionParts},
        settings as voice_settings, AutomationInfo, CommandExample, InputDevice, ListenMode, MicTest, RunningInfo,
        SettingChange, VoiceBackend, VoiceEvent, VoiceEvents, VoiceSettings, VoiceState,
    },
    LocalFlow,
};
use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager};

use crate::{commands::CommandError, notify, AppState};

pub const VOICE_EVENT: &str = "localflow://voice";
pub const DOWNLOAD_EVENT: &str = "localflow://voice-download";
/// Extra event (not in the contract): voice changed an app setting, so open windows can refresh.
pub const SETTING_EVENT: &str = "localflow://voice-setting";

/// Languages speech recognition supports.
const LANGUAGES: [&str; 3] = ["en", "ru", "de"];
/// How long exit waits for the session to release the microphone.
const EXIT_WAIT: Duration = Duration::from_secs(2);

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

// ---- shapes of the contract --------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct RunningView {
    pub run_id: i64,
    pub automation_id: i64,
    pub name: String,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct VoiceStatus {
    /// "off", "idle", "listening", "processing", "speaking", "muted" or "error".
    pub state: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
    pub enabled: bool,
    pub mode: ListenMode,
    pub muted: bool,
    /// The resolved recognition language ("auto" became the app language).
    pub language: String,
    /// The model for `language` is installed.
    pub model_ready: bool,
    pub pending_confirmation: Option<String>,
    pub running: Vec<RunningView>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct EngineView {
    pub info: EngineInfo,
    pub status: ModelStatus,
    pub recommended: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct DownloadProgress {
    pub engine: String,
    pub done: u64,
    pub total: u64,
    pub finished: bool,
    pub error: Option<String>,
}

fn state_parts(state: &VoiceState) -> (&'static str, Option<String>) {
    match state {
        VoiceState::Off => ("off", None),
        VoiceState::Idle => ("idle", None),
        VoiceState::Listening => ("listening", None),
        VoiceState::Processing => ("processing", None),
        VoiceState::Speaking => ("speaking", None),
        VoiceState::Muted => ("muted", None),
        VoiceState::Error(m) => ("error", Some(m.clone())),
    }
}

// ---- pure logic --------------------------------------------------------------------------------

/// "auto" follows the app's language; the result is always one of en/ru/de (fallback en).
pub fn resolve_language(setting: &str, app_language: &str) -> &'static str {
    let wanted = if setting == "auto" || setting.is_empty() { app_language } else { setting };
    let prefix = wanted.split(['-', '_']).next().unwrap_or("").to_lowercase();
    LANGUAGES.into_iter().find(|l| *l == prefix).unwrap_or("en")
}

/// What a running session depends on; a change needs a restart.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionKey {
    pub language: String,
    pub microphone: Option<String>,
    pub engine: String,
    pub spoken_feedback: bool,
}

impl SessionKey {
    pub fn of(settings: &VoiceSettings, language: &str) -> Self {
        SessionKey {
            language: language.to_string(),
            microphone: settings.microphone.clone(),
            engine: settings.engine.clone(),
            spoken_feedback: settings.spoken_feedback,
        }
    }
}

pub fn needs_restart(old: &SessionKey, new: &SessionKey) -> bool {
    old != new
}

/// Model installation and downloads; the real one wraps `localflow_core::voice::engine`.
pub trait ModelStore: Send + Sync {
    fn engines(&self) -> Vec<EngineInfo>;
    fn status(&self, model: &str) -> ModelStatus;
    fn download(&self, model: &str, progress: &dyn Fn(u64, u64), cancel: &AtomicBool) -> Result<(), String>;
    fn remove(&self, model: &str) -> Result<(), String>;
}

pub struct CoreStore;

impl ModelStore for CoreStore {
    fn engines(&self) -> Vec<EngineInfo> {
        engine::available_engines()
    }
    fn status(&self, model: &str) -> ModelStatus {
        engine::model_status(model)
    }
    fn download(&self, model: &str, progress: &dyn Fn(u64, u64), cancel: &AtomicBool) -> Result<(), String> {
        engine::download_model(model, progress, cancel)
    }
    fn remove(&self, model: &str) -> Result<(), String> {
        engine::remove_model(model)
    }
}

/// Validate new settings and refuse `enabled: true` unless consent was given and the model for
/// the language is installed. Returns the cleaned copy that is saved.
pub fn prepare_settings(new: &VoiceSettings, app_language: &str, store: &dyn ModelStore) -> Result<VoiceSettings, String> {
    let cleaned = voice_settings::validate(new)?;
    if cleaned.enabled {
        if !cleaned.consented {
            return Err("Voice control can only be switched on after you accept the notice about how it works.".into());
        }
        let language = resolve_language(&cleaned.language, app_language);
        let model_ready = engine::model_for_language(language).map(|m| store.status(&m).installed).unwrap_or(false);
        if !model_ready {
            return Err(format!(
                "The speech model for {} is not installed yet. Download it first.",
                language_name(language)
            ));
        }
    }
    Ok(cleaned)
}

fn language_name(code: &str) -> &'static str {
    match code {
        "ru" => "Russian",
        "de" => "German",
        _ => "English",
    }
}

#[allow(clippy::too_many_arguments)]
pub fn build_status(
    state: &VoiceState,
    hotkey_notice: Option<&str>,
    settings: &VoiceSettings,
    language: &str,
    model_ready: bool,
    session_muted: bool,
    pending_confirmation: Option<String>,
    running: Vec<RunningView>,
) -> VoiceStatus {
    let (name, message) = state_parts(state);
    // A problem with the push-to-talk key is shown while nothing worse is going on.
    let message = message.or_else(|| {
        (settings.enabled && settings.mode == ListenMode::PushToTalk).then(|| hotkey_notice.map(str::to_string)).flatten()
    });
    VoiceStatus {
        state: name.to_string(),
        message,
        enabled: settings.enabled,
        mode: settings.mode,
        muted: session_muted || *state == VoiceState::Muted,
        language: language.to_string(),
        model_ready,
        pending_confirmation,
        running,
    }
}

// ---- download jobs -----------------------------------------------------------------------------

#[derive(Default)]
pub struct Downloads {
    jobs: Mutex<HashMap<String, Arc<AtomicBool>>>,
}

pub type ProgressSink = Arc<dyn Fn(DownloadProgress) + Send + Sync>;

impl Downloads {
    pub fn is_active(&self, model: &str) -> bool {
        lock(&self.jobs).contains_key(model)
    }

    pub fn cancel(&self, model: &str) -> bool {
        match lock(&self.jobs).get(model) {
            Some(flag) => {
                flag.store(true, Ordering::SeqCst);
                true
            }
            None => false,
        }
    }

    pub fn cancel_all(&self) {
        for flag in lock(&self.jobs).values() {
            flag.store(true, Ordering::SeqCst);
        }
    }

    /// Start a background download and return at once. `Ok(None)` = already downloading.
    /// The returned handle is for tests; the app lets the thread run.
    pub fn start(
        self: &Arc<Self>,
        store: Arc<dyn ModelStore>,
        model: &str,
        sink: ProgressSink,
    ) -> Result<Option<std::thread::JoinHandle<()>>, String> {
        if !store.engines().iter().any(|e| e.id == model) {
            return Err(format!("Unknown speech model: {model}"));
        }
        let cancel = {
            let mut jobs = lock(&self.jobs);
            if jobs.contains_key(model) {
                return Ok(None);
            }
            let flag = Arc::new(AtomicBool::new(false));
            jobs.insert(model.to_string(), flag.clone());
            flag
        };
        let this = self.clone();
        let model = model.to_string();
        let handle = std::thread::Builder::new()
            .name("voice-download".into())
            .spawn(move || {
                let last = Arc::new(Mutex::new((0u64, 0u64)));
                let last_emit = Mutex::new(Instant::now() - Duration::from_secs(1));
                let result = {
                    let (sink, last, name) = (sink.clone(), last.clone(), model.clone());
                    let progress = move |done: u64, total: u64| {
                        *lock(&last) = (done, total);
                        let mut at = lock(&last_emit);
                        // About ten updates a second; the last one is always sent.
                        if done >= total || at.elapsed() >= Duration::from_millis(100) {
                            *at = Instant::now();
                            sink(DownloadProgress { engine: name.clone(), done, total, finished: false, error: None });
                        }
                    };
                    store.download(&model, &progress, &cancel)
                };
                let (done, total) = *lock(&last);
                let error = match result {
                    Ok(()) => None,
                    Err(_) if cancel.load(Ordering::SeqCst) => Some("Download cancelled.".to_string()),
                    Err(e) => {
                        tracing::warn!(model = %model, "speech model download failed");
                        Some(e)
                    }
                };
                // Remove the job before the final event so a retry right away is accepted.
                lock(&this.jobs).remove(&model);
                sink(DownloadProgress { engine: model, done, total, finished: true, error });
            })
            .map_err(|e| format!("Could not start the download: {e}"))?;
        Ok(Some(handle))
    }
}

// ---- the backend over LocalFlow ----------------------------------------------------------------

/// The parts of the backend that need the running app (settings commands, notifications).
pub trait Host: Send + Sync {
    fn apply(&self, change: &SettingChange) -> Result<String, String>;
    fn notice(&self, text: &str);
}

/// `VoiceBackend` over `LocalFlow`.
pub struct AppVoiceBackend {
    flow: LocalFlow,
    host: Arc<dyn Host>,
}

impl AppVoiceBackend {
    pub fn new(flow: LocalFlow, host: Arc<dyn Host>) -> Self {
        AppVoiceBackend { flow, host }
    }
}

impl VoiceBackend for AppVoiceBackend {
    fn automations(&self) -> Vec<AutomationInfo> {
        // Called from the session thread, never from an async runtime thread.
        match tauri::async_runtime::block_on(self.flow.list()) {
            Ok(list) => list
                .into_iter()
                .map(|s| s.automation)
                .filter(|a| a.deleted_at.is_none())
                .map(|a| AutomationInfo {
                    id: a.id,
                    name: a.name,
                    description: a.description,
                    enabled: a.enabled,
                    allow_system: a.allow_system,
                })
                .collect(),
            Err(e) => {
                tracing::warn!("voice: could not list automations: {e}");
                Vec::new()
            }
        }
    }

    fn running(&self) -> Vec<RunningInfo> {
        self.flow
            .running()
            .into_iter()
            .map(|r| RunningInfo { run_id: r.run_id, automation_id: r.automation_id, name: r.name })
            .collect()
    }

    fn start(&self, automation_id: i64) -> Result<(), String> {
        let flow = self.flow.clone();
        tauri::async_runtime::spawn(async move {
            match flow.run_guarded(automation_id, "voice").await {
                Ok(Some(_)) => {}
                Ok(None) => tracing::info!(automation_id, "voice: already running, skipped"),
                // Failures of runs not started by hand already notify through RunFinished.
                Err(e) => tracing::warn!(automation_id, "voice: could not run: {e}"),
            }
        });
        Ok(())
    }

    fn stop(&self, run_id: i64) -> Result<bool, String> {
        Ok(self.flow.stop_run(run_id))
    }

    fn apply(&self, change: &SettingChange) -> Result<String, String> {
        self.host.apply(change)
    }

    fn notice(&self, text: &str) {
        self.host.notice(text);
    }
}

fn command_message(e: CommandError) -> String {
    match e {
        CommandError::Error { message } => message,
        CommandError::Validation { messages } => messages.join(" "),
        CommandError::NotFound => "not found".into(),
    }
}

/// The real host: the same functions the Settings commands use.
struct AppHost {
    app: AppHandle,
}

impl Host for AppHost {
    fn apply(&self, change: &SettingChange) -> Result<String, String> {
        let app = &self.app;
        let state = app.state::<AppState>();
        // Called from the session thread, so blocking on the async command logic is fine.
        let on_off = |b: bool| if b { "on" } else { "off" };
        let described = match change {
            SettingChange::Notifications(b) => {
                tauri::async_runtime::block_on(crate::settings::set_notifications(state, *b)).map_err(command_message)?;
                format!("notifications {}", on_off(*b))
            }
            SettingChange::Theme(t) => {
                tauri::async_runtime::block_on(crate::settings::set_theme(app.clone(), state, t.clone()))
                    .map_err(command_message)?;
                format!("theme {t}")
            }
            SettingChange::Language(l) => {
                tauri::async_runtime::block_on(crate::settings::set_language(app.clone(), state, l.clone()))
                    .map_err(command_message)?;
                // "auto" voice language follows the app language: restart off this thread.
                let app = app.clone();
                tauri::async_runtime::spawn(async move {
                    let _ = tauri::async_runtime::spawn_blocking(move || reconcile_after_external_change(&app)).await;
                });
                format!("language {l}")
            }
            SettingChange::UpdateCheck(b) => {
                tauri::async_runtime::block_on(crate::updates::set_update_check(state, *b)).map_err(command_message)?;
                format!("update check {}", on_off(*b))
            }
            SettingChange::DotaLiveHelper(b) => {
                tauri::async_runtime::block_on(crate::dota::dota_set_live_helper(*b)).map_err(command_message)?;
                format!("Dota live helper {}", on_off(*b))
            }
            SettingChange::Autostart(b) => {
                crate::settings::set_autostart(app.clone(), *b).map_err(command_message)?;
                format!("autostart {}", on_off(*b))
            }
        };
        let _ = app.emit(SETTING_EVENT, change);
        Ok(described)
    }

    fn notice(&self, text: &str) {
        // Always shown, like the Telegram remote: voice must never act silently.
        notify(&self.app, "LocalFlow", text);
    }
}

// ---- the manager -------------------------------------------------------------------------------

pub struct VoiceManager {
    settings: Mutex<VoiceSettings>,
    session: Mutex<Option<Session>>,
    /// What the running session was built for.
    key: Mutex<Option<SessionKey>>,
    /// Last state seen; used when there is no session (off, or a failed start).
    state: Mutex<VoiceState>,
    /// Why the push-to-talk key isn't working, if it isn't.
    hotkey_notice: Mutex<Option<String>>,
    pending: Mutex<Option<String>>,
    /// Serialises start/stop/restart.
    ops: Mutex<()>,
    downloads: Arc<Downloads>,
}

impl VoiceManager {
    pub fn new(settings: VoiceSettings) -> Self {
        VoiceManager {
            settings: Mutex::new(settings),
            session: Mutex::new(None),
            key: Mutex::new(None),
            state: Mutex::new(VoiceState::Off),
            hotkey_notice: Mutex::new(None),
            pending: Mutex::new(None),
            ops: Mutex::new(()),
            downloads: Arc::new(Downloads::default()),
        }
    }

    fn current_state(&self) -> VoiceState {
        match lock(&self.session).as_ref() {
            Some(session) => session.state(),
            None => lock(&self.state).clone(),
        }
    }
}

fn manager(app: &AppHandle) -> Option<tauri::State<'_, VoiceManager>> {
    app.try_state::<VoiceManager>()
}

fn app_language(app: &AppHandle) -> &'static str {
    app.try_state::<AppState>().map(|s| *s.prefs.language.read().expect("language lock")).unwrap_or("en")
}

/// Show an event to the UI and keep tray, status and pending confirmation in step.
fn publish(app: &AppHandle, event: VoiceEvent) {
    if let Some(mgr) = manager(app) {
        match &event {
            VoiceEvent::State { state } => {
                *lock(&mgr.state) = state.clone();
            }
            VoiceEvent::Confirm { prompt } => *lock(&mgr.pending) = Some(prompt.clone()),
            VoiceEvent::Reply { reply } if reply.kind != localflow_core::voice::ReplyKind::Confirm => {
                *lock(&mgr.pending) = None;
            }
            VoiceEvent::Settings { settings } => persist_from_voice(&mgr, settings),
            _ => {}
        }
    }
    if let VoiceEvent::State { state } = &event {
        // Only state names are logged, never what was said.
        tracing::info!(state = state_parts(state).0, "voice state");
        crate::tray::set_voice_state(app, state);
    }
    let _ = app.emit(VOICE_EVENT, &event);
}

/// Voice changed its own settings (for example spoken feedback): validate and save them.
fn persist_from_voice(mgr: &VoiceManager, settings: &VoiceSettings) {
    match voice_settings::validate(settings) {
        Ok(clean) => {
            if let Err(e) = voice_settings::save(&clean) {
                tracing::warn!("voice: could not save settings: {e}");
            } else {
                *lock(&mgr.settings) = clean;
            }
        }
        Err(e) => tracing::warn!("voice: settings from voice were rejected: {e}"),
    }
}

fn events_for(app: &AppHandle) -> VoiceEvents {
    let app = app.clone();
    Arc::new(move |event| publish(&app, event))
}

fn set_state(app: &AppHandle, state: VoiceState) {
    publish(app, VoiceEvent::State { state });
}

/// Take the session out (releasing the lock first so nothing waits on it while it is joined) and stop it.
fn stop_session(mgr: &VoiceManager) -> bool {
    let session = lock(&mgr.session).take();
    *lock(&mgr.key) = None;
    *lock(&mgr.pending) = None;
    match session {
        Some(s) => {
            s.stop();
            true
        }
        None => false,
    }
}

/// Build the real parts and start a session. Every failure becomes a friendly message.
fn build_session(app: &AppHandle, settings: &VoiceSettings, language: &str) -> Result<Session, String> {
    let model = engine::model_for_language(language).ok_or_else(|| "This language is not supported for voice control.".to_string())?;
    if !CoreStore.status(&model).installed {
        return Err(format!("The speech model for {} is not installed. Download it in Settings > Voice.", language_name(language)));
    }
    let source = audio::create_audio_source()?;
    let recognizer = engine::create_recognizer(&model, language)?;
    let segmenter = engine::create_segmenter()?;
    let speaker = settings.spoken_feedback.then(audio::create_speaker);
    let flow = app.state::<AppState>().flow.clone();
    let backend = Arc::new(AppVoiceBackend::new(flow, Arc::new(AppHost { app: app.clone() })));
    let controller = VoiceController::new(backend, settings.clone());
    Ok(Session::start(SessionParts {
        settings: settings.clone(),
        language: language.to_string(),
        source,
        recognizer,
        segmenter,
        speaker,
        controller,
        events: events_for(app),
    }))
}

/// Start a session for `settings` (caller holds `ops`, and there is no session).
fn start_session(app: &AppHandle, mgr: &VoiceManager, settings: &VoiceSettings) {
    let language = resolve_language(&settings.language, app_language(app));
    match build_session(app, settings, language) {
        Ok(session) => {
            *lock(&mgr.key) = Some(SessionKey::of(settings, language));
            let state = session.state();
            *lock(&mgr.session) = Some(session);
            tracing::info!(language, "voice session started");
            set_state(app, if state == VoiceState::Off { VoiceState::Idle } else { state });
        }
        Err(message) => {
            tracing::warn!("voice session did not start");
            set_state(app, VoiceState::Error(message));
        }
    }
}

/// Make the running session match `settings`: start, stop, update live or restart.
fn reconcile(app: &AppHandle, mgr: &VoiceManager, settings: &VoiceSettings) {
    if !settings.enabled {
        if stop_session(mgr) {
            tracing::info!("voice session stopped");
        }
        set_state(app, VoiceState::Off);
        return;
    }
    let language = resolve_language(&settings.language, app_language(app));
    let wanted = SessionKey::of(settings, language);
    let has_session = lock(&mgr.session).is_some();
    if !has_session {
        start_session(app, mgr, settings);
        return;
    }
    let live_cannot = lock(&mgr.session).as_ref().map(|s| s.update_settings(settings.clone())).unwrap_or(false);
    let changed = lock(&mgr.key).as_ref().map(|old| needs_restart(old, &wanted)).unwrap_or(true);
    if live_cannot || changed {
        stop_session(mgr);
        start_session(app, mgr, settings);
    }
}

/// Something outside voice changed (the app language): restart only if the session depends on it.
fn reconcile_after_external_change(app: &AppHandle) {
    let Some(mgr) = manager(app) else { return };
    let _op = lock(&mgr.ops);
    let settings = lock(&mgr.settings).clone();
    if settings.enabled && lock(&mgr.session).is_some() {
        let wanted = SessionKey::of(&settings, resolve_language(&settings.language, app_language(app)));
        if lock(&mgr.key).as_ref().map(|k| needs_restart(k, &wanted)).unwrap_or(false) {
            stop_session(&mgr);
            start_session(app, &mgr, &settings);
        }
    }
}

fn running_views(app: &AppHandle) -> Vec<RunningView> {
    app.try_state::<AppState>()
        .map(|s| {
            s.flow
                .running()
                .into_iter()
                .map(|r| RunningView { run_id: r.run_id, automation_id: r.automation_id, name: r.name })
                .collect()
        })
        .unwrap_or_default()
}

fn status(app: &AppHandle) -> VoiceStatus {
    let Some(mgr) = manager(app) else {
        return build_status(&VoiceState::Off, None, &VoiceSettings::default(), "en", false, false, None, Vec::new());
    };
    let settings = lock(&mgr.settings).clone();
    let language = resolve_language(&settings.language, app_language(app));
    let model_ready = engine::model_for_language(language).map(|m| CoreStore.status(&m).installed).unwrap_or(false);
    let state = mgr.current_state();
    let muted = lock(&mgr.session).as_ref().map(|s| s.is_muted()).unwrap_or(false);
    let notice = lock(&mgr.hotkey_notice).clone();
    let pending = lock(&mgr.pending).clone();
    build_status(&state, notice.as_deref(), &settings, language, model_ready, muted, pending, running_views(app))
}

/// Validate, save and apply new settings. Blocking (loads models): call off the async runtime.
pub fn apply_settings(app: &AppHandle, new: VoiceSettings) -> Result<VoiceStatus, String> {
    let mgr = manager(app).ok_or("Voice control is not available.")?;
    let _op = lock(&mgr.ops);
    let cleaned = prepare_settings(&new, app_language(app), &CoreStore)?;
    voice_settings::save(&cleaned)?;
    *lock(&mgr.settings) = cleaned.clone();
    reconcile(app, &mgr, &cleaned);
    // The push-to-talk key follows enabled/mode/key.
    crate::hotkeys::refresh(app);
    Ok(status(app))
}

// ---- called from lib.rs, hotkeys.rs and tray.rs --------------------------------------------------

/// At startup, after the settings are loaded: start the session if voice was left on.
pub fn init(app: &AppHandle) {
    let app = app.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let Some(mgr) = manager(&app) else { return };
        let _op = lock(&mgr.ops);
        let settings = lock(&mgr.settings).clone();
        if settings.enabled && settings.consented {
            start_session(&app, &mgr, &settings);
        }
        crate::hotkeys::refresh(&app);
    });
}

/// Release the microphone. Waits at most two seconds; safe to call twice.
pub fn shutdown(app: &AppHandle) {
    let Some(mgr) = manager(app) else { return };
    mgr.downloads.cancel_all();
    let session = lock(&mgr.session).take();
    if let Some(session) = session {
        let (tx, rx) = mpsc::channel();
        let spawned = std::thread::Builder::new().name("voice-shutdown".into()).spawn(move || {
            session.stop();
            let _ = tx.send(());
        });
        if spawned.is_ok() && rx.recv_timeout(EXIT_WAIT).is_err() {
            tracing::warn!("voice session did not stop within two seconds; exiting anyway");
        }
    }
}

/// The push-to-talk key the hotkey slot should hold, if voice needs one.
pub fn wanted_push_key(app: &AppHandle) -> Option<String> {
    let mgr = manager(app)?;
    let s = lock(&mgr.settings);
    (s.enabled && s.mode == ListenMode::PushToTalk).then(|| s.push_key.clone())
}

pub fn set_hotkey_notice(app: &AppHandle, notice: Option<String>) {
    if let Some(mgr) = manager(app) {
        *lock(&mgr.hotkey_notice) = notice;
    }
}

pub fn press(app: &AppHandle) {
    if let Some(mgr) = manager(app) {
        if let Some(s) = lock(&mgr.session).as_ref() {
            s.press();
        }
    }
}

pub fn release(app: &AppHandle) {
    if let Some(mgr) = manager(app) {
        if let Some(s) = lock(&mgr.session).as_ref() {
            s.release();
        }
    }
}

/// `Some(muted)` while a session runs (the tray shows the mute item), else `None`.
pub fn tray_mute_state(app: &AppHandle) -> Option<bool> {
    let mgr = manager(app)?;
    let session = lock(&mgr.session);
    session.as_ref().map(|s| s.is_muted())
}

pub fn toggle_mute(app: &AppHandle) {
    if let Some(mgr) = manager(app) {
        if let Some(s) = lock(&mgr.session).as_ref() {
            s.set_muted(!s.is_muted());
        }
    }
    crate::tray::refresh(app);
}

// ---- commands ----------------------------------------------------------------------------------

fn err(message: impl ToString) -> CommandError {
    CommandError::Error { message: message.to_string() }
}

async fn blocking<T: Send + 'static>(work: impl FnOnce() -> Result<T, String> + Send + 'static) -> Result<T, CommandError> {
    tauri::async_runtime::spawn_blocking(work).await.map_err(err)?.map_err(err)
}

#[tauri::command]
pub async fn voice_get_settings(app: AppHandle) -> Result<VoiceSettings, CommandError> {
    Ok(manager(&app).map(|m| lock(&m.settings).clone()).unwrap_or_default())
}

#[tauri::command]
pub async fn voice_set_settings(app: AppHandle, settings: VoiceSettings) -> Result<VoiceStatus, CommandError> {
    blocking(move || apply_settings(&app, settings)).await
}

#[tauri::command]
pub async fn voice_status(app: AppHandle) -> Result<VoiceStatus, CommandError> {
    Ok(status(&app))
}

#[tauri::command]
pub async fn voice_list_devices() -> Result<Vec<InputDevice>, CommandError> {
    blocking(audio::list_input_devices).await
}

#[tauri::command]
pub async fn voice_engines(app: AppHandle) -> Result<Vec<EngineView>, CommandError> {
    let recommended = engine::model_for_language(app_language(&app));
    Ok(engine::available_engines()
        .into_iter()
        .map(|info| EngineView {
            status: engine::model_status(&info.id),
            recommended: recommended.as_deref() == Some(info.id.as_str()),
            info,
        })
        .collect())
}

#[tauri::command]
pub async fn voice_download_model(app: AppHandle, engine: String) -> Result<(), CommandError> {
    let mgr = manager(&app).ok_or_else(|| err("Voice control is not available."))?;
    let sink_app = app.clone();
    let sink: ProgressSink = Arc::new(move |p: DownloadProgress| {
        let _ = sink_app.emit(DOWNLOAD_EVENT, &p);
    });
    mgr.downloads.start(Arc::new(CoreStore), &engine, sink).map(|_| ()).map_err(err)
}

#[tauri::command]
pub async fn voice_cancel_download(app: AppHandle, engine: String) -> Result<(), CommandError> {
    if let Some(mgr) = manager(&app) {
        mgr.downloads.cancel(&engine);
    }
    Ok(())
}

#[tauri::command]
pub async fn voice_remove_model(app: AppHandle, engine: String) -> Result<(), CommandError> {
    blocking(move || {
        let mgr = manager(&app).ok_or("Voice control is not available.")?;
        if mgr.downloads.is_active(&engine) {
            return Err("This model is downloading. Cancel the download first.".to_string());
        }
        let _op = lock(&mgr.ops);
        // Removing the model in use switches voice control off first (and releases the microphone).
        let active = {
            let s = lock(&mgr.settings);
            engine::model_for_language(resolve_language(&s.language, app_language(&app))).as_deref() == Some(engine.as_str())
        };
        if active && lock(&mgr.settings).enabled {
            stop_session(&mgr);
            let mut s = lock(&mgr.settings).clone();
            s.enabled = false;
            voice_settings::save(&s)?;
            *lock(&mgr.settings) = s;
            set_state(&app, VoiceState::Off);
            crate::hotkeys::refresh(&app);
        }
        CoreStore.remove(&engine)
    })
    .await
}

#[tauri::command]
pub async fn voice_test_microphone(device: Option<String>) -> Result<MicTest, CommandError> {
    // The only place the app opens the microphone on its own: the user pressed "Test".
    blocking(move || audio::test_input(device.as_deref(), 3.0)).await
}

#[tauri::command]
pub async fn voice_set_enabled(app: AppHandle, enabled: bool) -> Result<VoiceStatus, CommandError> {
    blocking(move || {
        let mut settings = manager(&app).map(|m| lock(&m.settings).clone()).unwrap_or_default();
        settings.enabled = enabled;
        apply_settings(&app, settings)
    })
    .await
}

#[tauri::command]
pub async fn voice_set_muted(app: AppHandle, muted: bool) -> Result<VoiceStatus, CommandError> {
    {
        let mgr = manager(&app).ok_or_else(|| err("Voice control is not available."))?;
        let session = lock(&mgr.session);
        let session = session.as_ref().ok_or_else(|| err("Voice control is off."))?;
        session.set_muted(muted);
    }
    crate::tray::refresh(&app);
    Ok(status(&app))
}

#[tauri::command]
pub async fn voice_press(app: AppHandle) -> Result<(), CommandError> {
    press(&app);
    Ok(())
}

#[tauri::command]
pub async fn voice_release(app: AppHandle) -> Result<(), CommandError> {
    release(&app);
    Ok(())
}

#[tauri::command]
pub async fn voice_submit_text(app: AppHandle, text: String) -> Result<(), CommandError> {
    let mgr = manager(&app).ok_or_else(|| err("Voice control is not available."))?;
    let session = lock(&mgr.session);
    let session = session.as_ref().ok_or_else(|| err("Switch voice control on to try a phrase."))?;
    session.submit_text(&text);
    Ok(())
}

#[tauri::command]
pub async fn voice_answer(app: AppHandle, yes: bool) -> Result<(), CommandError> {
    if let Some(mgr) = manager(&app) {
        if let Some(s) = lock(&mgr.session).as_ref() {
            s.answer_confirmation(yes);
        }
    }
    Ok(())
}

#[tauri::command]
pub async fn voice_commands(app: AppHandle) -> Result<Vec<CommandExample>, CommandError> {
    blocking(move || {
        let mgr = manager(&app).ok_or("Voice control is not available.")?;
        if let Some(s) = lock(&mgr.session).as_ref() {
            return Ok(s.commands());
        }
        // Voice is off: build the list the same way the controller would (no microphone involved).
        let settings = lock(&mgr.settings).clone();
        let flow = app.state::<AppState>().flow.clone();
        let backend = Arc::new(AppVoiceBackend::new(flow, Arc::new(AppHost { app: app.clone() })));
        Ok(VoiceController::new(backend, settings).commands())
    })
    .await
}

#[tauri::command]
pub async fn running_automations(app: AppHandle) -> Result<Vec<RunningInfo>, CommandError> {
    Ok(running_views(&app)
        .into_iter()
        .map(|r| RunningInfo { run_id: r.run_id, automation_id: r.automation_id, name: r.name })
        .collect())
}

#[tauri::command]
pub async fn stop_run(state: tauri::State<'_, AppState>, run_id: i64) -> Result<bool, CommandError> {
    Ok(state.flow.stop_run(run_id))
}

#[tauri::command]
pub async fn stop_all_runs(app: AppHandle, state: tauri::State<'_, AppState>) -> Result<usize, CommandError> {
    let stopped = state.flow.stop_all();
    crate::tray::refresh(&app);
    Ok(stopped)
}

// ---- tests ---------------------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use localflow_core::{AutomationInput, CoreConfig};
    use std::sync::atomic::AtomicUsize;

    // -- language resolution

    #[test]
    fn language_resolution() {
        assert_eq!(resolve_language("auto", "ru"), "ru");
        assert_eq!(resolve_language("", "de"), "de");
        assert_eq!(resolve_language("auto", "fr"), "en");
        assert_eq!(resolve_language("de", "ru"), "de");
        assert_eq!(resolve_language("ru-RU", "en"), "ru");
        assert_eq!(resolve_language("xx", "ru"), "en");
    }

    // -- settings refusals

    struct FakeStore {
        installed: Mutex<Vec<String>>,
        fail: bool,
        steps: u64,
        /// Wait for the cancel flag after the first step.
        block_until_cancel: bool,
        downloads: AtomicUsize,
    }

    impl FakeStore {
        fn new(installed: &[&str]) -> Arc<Self> {
            Arc::new(FakeStore {
                installed: Mutex::new(installed.iter().map(|s| s.to_string()).collect()),
                fail: false,
                steps: 3,
                block_until_cancel: false,
                downloads: AtomicUsize::new(0),
            })
        }
    }

    fn info(id: &str) -> EngineInfo {
        EngineInfo {
            id: id.into(),
            name: id.into(),
            description: String::new(),
            download_bytes: 100,
            languages: vec![],
            local: true,
            license: "test".into(),
        }
    }

    impl ModelStore for FakeStore {
        fn engines(&self) -> Vec<EngineInfo> {
            ["sherpa-en", "sherpa-ru", "sherpa-de"].iter().map(|i| info(i)).collect()
        }
        fn status(&self, model: &str) -> ModelStatus {
            ModelStatus { engine: model.into(), installed: lock(&self.installed).iter().any(|m| m == model), size_bytes: 0 }
        }
        fn download(&self, model: &str, progress: &dyn Fn(u64, u64), cancel: &AtomicBool) -> Result<(), String> {
            self.downloads.fetch_add(1, Ordering::SeqCst);
            if self.block_until_cancel {
                progress(1, 100);
                let start = Instant::now();
                while !cancel.load(Ordering::SeqCst) {
                    if start.elapsed() > Duration::from_secs(5) {
                        return Err("test timeout".into());
                    }
                    std::thread::sleep(Duration::from_millis(5));
                }
                return Err("stopped".into());
            }
            for i in 1..=self.steps {
                progress(i, self.steps);
            }
            if self.fail {
                return Err("network down".into());
            }
            lock(&self.installed).push(model.to_string());
            Ok(())
        }
        fn remove(&self, model: &str) -> Result<(), String> {
            lock(&self.installed).retain(|m| m != model);
            Ok(())
        }
    }

    fn enabled_settings() -> VoiceSettings {
        VoiceSettings { enabled: true, consented: true, ..VoiceSettings::default() }
    }

    #[test]
    fn enabling_needs_consent_and_the_model() {
        let store = FakeStore::new(&[]);
        // Off needs nothing.
        assert!(prepare_settings(&VoiceSettings::default(), "en", &*store).is_ok());
        // No consent.
        let mut s = enabled_settings();
        s.consented = false;
        assert!(prepare_settings(&s, "en", &*store).unwrap_err().contains("accept"));
        // No model for the (resolved) language.
        let e = prepare_settings(&enabled_settings(), "ru", &*store).unwrap_err();
        assert!(e.contains("Russian"), "{e}");
        // The English model doesn't help a Russian app with "auto", but does with an explicit "en".
        let store = FakeStore::new(&["sherpa-en"]);
        assert!(prepare_settings(&enabled_settings(), "ru", &*store).is_err());
        let mut s = enabled_settings();
        s.language = "en".into();
        assert!(prepare_settings(&s, "ru", &*store).is_ok());
        assert!(prepare_settings(&enabled_settings(), "fr", &*store).is_ok());
    }

    #[test]
    fn invalid_settings_are_refused_before_anything_else() {
        let store = FakeStore::new(&["sherpa-en"]);
        let mut s = enabled_settings();
        s.push_key = "Space".into();
        assert!(prepare_settings(&s, "en", &*store).is_err());
        let mut s = enabled_settings();
        s.language = "fr".into();
        assert!(prepare_settings(&s, "en", &*store).is_err());
        // Cleaned copy: canonical push key.
        let mut s = enabled_settings();
        s.push_key = "ctrl+alt+k".into();
        assert_eq!(prepare_settings(&s, "en", &*store).unwrap().push_key, "Ctrl+Alt+K");
    }

    #[test]
    fn restart_only_when_the_session_depends_on_it() {
        let base = enabled_settings();
        let key = SessionKey::of(&base, "en");
        assert!(!needs_restart(&key, &SessionKey::of(&base, "en")));
        assert!(needs_restart(&key, &SessionKey::of(&base, "ru")));
        let mut other = base.clone();
        other.microphone = Some("USB mic".into());
        assert!(needs_restart(&key, &SessionKey::of(&other, "en")));
        let mut other = base.clone();
        other.spoken_feedback = true;
        assert!(needs_restart(&key, &SessionKey::of(&other, "en")));
        // Wake phrase, aliases and the push key are applied live.
        let mut other = base;
        other.wake_phrase = "hello flow".into();
        other.push_key = "Ctrl+Alt+K".into();
        assert!(!needs_restart(&key, &SessionKey::of(&other, "en")));
    }

    // -- status

    #[test]
    fn status_shape_and_notice() {
        let s = enabled_settings();
        let st = build_status(&VoiceState::Idle, Some("key taken"), &s, "ru", true, false, None, vec![]);
        assert_eq!(st.state, "idle");
        assert_eq!(st.message.as_deref(), Some("key taken"));
        assert_eq!(st.language, "ru");
        assert!(st.model_ready && !st.muted);
        let json = serde_json::to_value(&st).unwrap();
        assert_eq!(json["mode"], "push_to_talk");
        assert!(json["pending_confirmation"].is_null());

        // An error message wins over the key notice; muted follows the state or the session.
        let st = build_status(&VoiceState::Error("no microphone".into()), Some("key taken"), &s, "en", false, false, None, vec![]);
        assert_eq!((st.state.as_str(), st.message.as_deref()), ("error", Some("no microphone")));
        let st = build_status(&VoiceState::Muted, None, &s, "en", true, false, Some("Stop?".into()), vec![]);
        assert!(st.muted);
        assert_eq!(st.pending_confirmation.as_deref(), Some("Stop?"));
        let st = build_status(&VoiceState::Idle, None, &s, "en", true, true, None, vec![]);
        assert!(st.muted);

        // No key notice when the key isn't used (off, or always-on).
        let off = VoiceSettings::default();
        assert_eq!(build_status(&VoiceState::Off, Some("x"), &off, "en", false, false, None, vec![]).message, None);
        let mut always = s;
        always.mode = ListenMode::AlwaysOn;
        assert_eq!(build_status(&VoiceState::Idle, Some("x"), &always, "en", true, false, None, vec![]).message, None);
        let json = serde_json::to_value(build_status(&VoiceState::Idle, None, &always, "en", true, false, None, vec![])).unwrap();
        assert!(json.get("message").is_none());
        assert_eq!(json["mode"], "always_on");
    }

    // -- downloads

    fn collect() -> (ProgressSink, Arc<Mutex<Vec<DownloadProgress>>>) {
        let events = Arc::new(Mutex::new(Vec::new()));
        let e = events.clone();
        (Arc::new(move |p| lock(&e).push(p)), events)
    }

    #[test]
    fn a_download_reports_progress_and_finishes() {
        let store = FakeStore::new(&[]);
        let jobs = Arc::new(Downloads::default());
        let (sink, events) = collect();
        jobs.start(store.clone(), "sherpa-en", sink).unwrap().unwrap().join().unwrap();
        let events = lock(&events);
        let last = events.last().unwrap();
        assert!(last.finished && last.error.is_none());
        assert_eq!((last.done, last.total), (3, 3));
        assert_eq!(events.iter().filter(|e| e.finished).count(), 1);
        assert!(store.status("sherpa-en").installed);
        assert!(!jobs.is_active("sherpa-en"));
    }

    #[test]
    fn a_failed_download_reports_the_error_and_can_be_retried() {
        let store = Arc::new(FakeStore {
            installed: Mutex::new(vec![]),
            fail: true,
            steps: 2,
            block_until_cancel: false,
            downloads: AtomicUsize::new(0),
        });
        let jobs = Arc::new(Downloads::default());
        let (sink, events) = collect();
        jobs.start(store.clone(), "sherpa-ru", sink.clone()).unwrap().unwrap().join().unwrap();
        assert_eq!(lock(&events).last().unwrap().error.as_deref(), Some("network down"));
        assert!(!store.status("sherpa-ru").installed);
        assert!(jobs.start(store.clone(), "sherpa-ru", sink).unwrap().is_some());
    }

    #[test]
    fn cancelling_stops_the_job_and_reports_it() {
        let store = Arc::new(FakeStore {
            installed: Mutex::new(vec![]),
            fail: false,
            steps: 3,
            block_until_cancel: true,
            downloads: AtomicUsize::new(0),
        });
        let jobs = Arc::new(Downloads::default());
        let (sink, events) = collect();
        let handle = jobs.start(store.clone(), "sherpa-de", sink.clone()).unwrap().unwrap();
        // A second start for the same model does nothing while it runs.
        assert!(jobs.start(store.clone(), "sherpa-de", sink).unwrap().is_none());
        assert!(jobs.is_active("sherpa-de"));
        assert!(jobs.cancel("sherpa-de"));
        handle.join().unwrap();
        let events = lock(&events);
        assert_eq!(events.last().unwrap().error.as_deref(), Some("Download cancelled."));
        assert_eq!(store.downloads.load(Ordering::SeqCst), 1);
        assert!(!jobs.is_active("sherpa-de"));
        assert!(!jobs.cancel("sherpa-de"));
    }

    #[test]
    fn an_unknown_model_is_refused() {
        let jobs = Arc::new(Downloads::default());
        let (sink, _) = collect();
        assert!(jobs.start(FakeStore::new(&[]), "whisper-xl", sink).is_err());
    }

    // -- the backend over a temporary LocalFlow

    #[derive(Default)]
    struct FakeHost {
        applied: Mutex<Vec<SettingChange>>,
        notices: Mutex<Vec<String>>,
    }

    impl Host for FakeHost {
        fn apply(&self, change: &SettingChange) -> Result<String, String> {
            lock(&self.applied).push(change.clone());
            Ok("done".into())
        }
        fn notice(&self, text: &str) {
            lock(&self.notices).push(text.to_string());
        }
    }

    fn temp_backend() -> (AppVoiceBackend, Arc<FakeHost>, LocalFlow) {
        let config = CoreConfig {
            database_url: "sqlite::memory:".into(),
            allowed_dirs: vec![std::env::temp_dir()],
            script_timeout: Duration::from_secs(10),
        };
        let flow = tauri::async_runtime::block_on(LocalFlow::open(config, None)).unwrap();
        let host = Arc::new(FakeHost::default());
        (AppVoiceBackend::new(flow.clone(), host.clone()), host, flow)
    }

    fn input(name: &str, code: &str) -> AutomationInput {
        AutomationInput { name: name.into(), lua_code: code.into(), enabled: true, ..AutomationInput::default() }
    }

    #[test]
    fn the_backend_lists_runs_and_stops_automations() {
        let (backend, host, flow) = temp_backend();
        let quick = tauri::async_runtime::block_on(flow.create(&input("Quick one", "notify('hi')"))).unwrap();
        let slow = tauri::async_runtime::block_on(flow.create(&input("Slow one", "while true do wait(0.05) end"))).unwrap();
        let trashed = tauri::async_runtime::block_on(flow.create(&input("Gone", "notify('x')"))).unwrap();
        tauri::async_runtime::block_on(flow.delete(trashed.id)).unwrap();

        let list = backend.automations();
        let names: Vec<_> = list.iter().map(|a| a.name.as_str()).collect();
        assert!(names.contains(&"Quick one") && names.contains(&"Slow one"));
        assert!(!names.contains(&"Gone"), "trashed automations are skipped");
        assert!(list.iter().all(|a| !a.allow_system));
        let _ = quick;

        // Start returns at once; the run shows up in running() and can be stopped.
        backend.start(slow.id).unwrap();
        let deadline = Instant::now() + Duration::from_secs(5);
        let run = loop {
            if let Some(r) = backend.running().into_iter().find(|r| r.automation_id == slow.id) {
                break r;
            }
            assert!(Instant::now() < deadline, "the run never started");
            std::thread::sleep(Duration::from_millis(20));
        };
        assert_eq!(run.name, "Slow one");
        // A second voice start is skipped (guarded), not doubled.
        backend.start(slow.id).unwrap();
        std::thread::sleep(Duration::from_millis(100));
        assert_eq!(backend.running().iter().filter(|r| r.automation_id == slow.id).count(), 1);
        assert_eq!(backend.stop(run.run_id), Ok(true));
        let deadline = Instant::now() + Duration::from_secs(5);
        while backend.running().iter().any(|r| r.automation_id == slow.id) {
            assert!(Instant::now() < deadline, "the run did not stop");
            std::thread::sleep(Duration::from_millis(20));
        }
        assert_eq!(backend.stop(run.run_id), Ok(false));

        // Settings and notices go to the host.
        assert_eq!(backend.apply(&SettingChange::Notifications(false)), Ok("done".into()));
        backend.notice("Voice: run Quick one");
        assert_eq!(*lock(&host.applied), vec![SettingChange::Notifications(false)]);
        assert_eq!(*lock(&host.notices), vec!["Voice: run Quick one".to_string()]);
    }

    #[test]
    fn tooltips_follow_the_state_in_every_language() {
        for lang in crate::i18n::LANGUAGES {
            let t = crate::i18n::texts(lang);
            let states = [
                VoiceState::Idle,
                VoiceState::Listening,
                VoiceState::Processing,
                VoiceState::Speaking,
                VoiceState::Muted,
                VoiceState::Error("x".into()),
            ];
            let tips: Vec<_> = states.iter().map(|s| crate::tray::tooltip(t, s)).collect();
            assert!(tips.iter().all(|t| t.starts_with("LocalFlow")));
            let mut unique = tips.clone();
            unique.sort();
            unique.dedup();
            assert_eq!(unique.len(), tips.len(), "{lang}");
            assert_eq!(crate::tray::tooltip(t, &VoiceState::Off), "LocalFlow");
        }
        assert_eq!(crate::tray::tooltip(crate::i18n::texts("en"), &VoiceState::Listening), "LocalFlow: listening");
    }
}
