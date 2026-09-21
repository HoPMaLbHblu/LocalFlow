//! AI through GigaChat (Sber): `ai.ask` in scripts, and writing automations from a description.
//!
//! ```lua
//! ai.available()                                -- true when a GigaChat key is set in Settings
//! ai.ask(question, { system, model, temperature, max_tokens })   -- the answer as text
//! ai.chat({ { role = "user", content = "..." }, ... }, options)  -- a whole conversation
//! ai.ask(question, { cache_hours = 24 })        -- reuse the answer to the exact same question
//! local chat = ai.conversation("journal")       -- remembers earlier questions between runs
//! chat:ask("..."), chat:history(), chat:forget()
//! ```
//!
//! The authorization key lives in the operating system's password store (set from
//! Settings) and is handed to this module by the app. Scripts can use the AI but can
//! never read the key. GigaChat's servers use the Russian Trusted Root CA certificate,
//! which is trusted here only for GigaChat's own connection, not for `http.get`.

use std::{
    collections::HashMap,
    path::PathBuf,
    sync::{Arc, Mutex, OnceLock, RwLock},
    time::{Duration, Instant},
};

use mlua::{Lua, Table, Value};
use serde_json::{json, Value as Json};

const OAUTH_URL: &str = "https://ngw.devices.sberbank.ru:9443/api/v2/oauth";
const CHAT_URL: &str = "https://gigachat.devices.sberbank.ru/api/v1/chat/completions";
const ROOT_CA: &[u8] = include_bytes!("../certs/russian_trusted_root_ca.pem");

pub const DEFAULT_MODEL: &str = "GigaChat-2";
pub const MODELS: &[&str] = &["GigaChat-2", "GigaChat-2-Pro", "GigaChat-2-Max"];
/// `GIGACHAT_API_PERS` for individuals, `_B2B` and `_CORP` for companies.
pub const SCOPES: &[&str] = &["GIGACHAT_API_PERS", "GIGACHAT_API_B2B", "GIGACHAT_API_CORP"];

/// What LocalFlow needs to talk to GigaChat.
#[derive(Clone, PartialEq)]
pub struct Credentials {
    /// The "authorization key" from the GigaChat developer page (Base64 of client id and secret).
    pub key: String,
    pub scope: String,
    pub model: String,
}

// Never print the key, even in debug output.
impl std::fmt::Debug for Credentials {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Credentials").field("scope", &self.scope).field("model", &self.model).finish_non_exhaustive()
    }
}

static CREDENTIALS: RwLock<Option<Credentials>> = RwLock::new(None);
/// The current access token (valid 30 minutes) and when it expires.
static TOKEN: Mutex<Option<(String, Instant)>> = Mutex::new(None);

/// Set or clear the credentials (called by the app when the user saves the key).
pub fn configure(credentials: Option<Credentials>) {
    let mut current = CREDENTIALS.write().unwrap_or_else(|e| e.into_inner());
    if *current != credentials {
        *TOKEN.lock().unwrap_or_else(|e| e.into_inner()) = None;
    }
    *current = credentials;
}

pub fn is_configured() -> bool {
    CREDENTIALS.read().unwrap_or_else(|e| e.into_inner()).is_some()
}

fn credentials() -> Result<Credentials, String> {
    CREDENTIALS
        .read()
        .unwrap_or_else(|e| e.into_inner())
        .clone()
        .ok_or_else(|| "AI isn't set up yet: add your GigaChat authorization key in Settings › AI".into())
}

/// An HTTP client that trusts only the Russian Trusted Root CA.
fn agent(timeout: Duration) -> Result<ureq::Agent, String> {
    static TLS: OnceLock<Result<Arc<rustls::ClientConfig>, String>> = OnceLock::new();
    let tls = TLS
        .get_or_init(|| {
            use rustls_pki_types::{pem::PemObject, CertificateDer};
            let mut roots = rustls::RootCertStore::empty();
            for cert in CertificateDer::pem_slice_iter(ROOT_CA) {
                roots.add(cert.map_err(|e| e.to_string())?).map_err(|e| e.to_string())?;
            }
            let provider = Arc::new(rustls::crypto::ring::default_provider());
            let config = rustls::ClientConfig::builder_with_provider(provider)
                .with_safe_default_protocol_versions()
                .map_err(|e| e.to_string())?
                .with_root_certificates(roots)
                .with_no_client_auth();
            Ok(Arc::new(config))
        })
        .clone()?;
    Ok(ureq::AgentBuilder::new().tls_config(tls).timeout(timeout).build())
}

/// A random RqUID (a UUID v4), which GigaChat wants with every sign-in.
fn request_id() -> String {
    let mut b = [0u8; 16];
    let _ = getrandom::getrandom(&mut b);
    b[6] = (b[6] & 0x0f) | 0x40;
    b[8] = (b[8] & 0x3f) | 0x80;
    let hex: String = b.iter().map(|x| format!("{x:02x}")).collect();
    format!("{}-{}-{}-{}-{}", &hex[0..8], &hex[8..12], &hex[12..16], &hex[16..20], &hex[20..32])
}

/// A readable reason for a GigaChat error status.
fn explain(status: u16, body: &str) -> String {
    let detail = serde_json::from_str::<Json>(body)
        .ok()
        .and_then(|j| j["message"].as_str().map(String::from))
        .unwrap_or_default();
    let reason = match status {
        400 => "GigaChat didn't understand the request",
        401 => "GigaChat didn't accept the authorization key; check it in Settings › AI",
        402 => "your GigaChat balance or free tokens are used up",
        403 => "this key isn't allowed to do that (check the scope in Settings › AI)",
        404 => "GigaChat doesn't know this model",
        413 | 422 => "the text is too long for GigaChat",
        429 => "too many requests to GigaChat; wait a little and try again",
        500..=599 => "GigaChat is having problems; try again later",
        _ => "GigaChat answered with an error",
    };
    if detail.is_empty() {
        format!("{reason} ({status})")
    } else {
        format!("{reason} ({status}: {detail})")
    }
}

fn call_error(error: ureq::Error) -> String {
    match error {
        ureq::Error::Status(status, response) => explain(status, &response.into_string().unwrap_or_default()),
        ureq::Error::Transport(t) => format!("couldn't reach GigaChat ({t})"),
    }
}

/// A valid access token, signing in again when the old one is about to expire.
fn token(credentials: &Credentials, timeout: Duration) -> Result<String, String> {
    let mut cached = TOKEN.lock().unwrap_or_else(|e| e.into_inner());
    if let Some((token, expires)) = cached.as_ref() {
        if Instant::now() + Duration::from_secs(60) < *expires {
            return Ok(token.clone());
        }
    }
    let response = agent(timeout)?
        .post(OAUTH_URL)
        .set("Authorization", &format!("Basic {}", credentials.key.trim()))
        .set("RqUID", &request_id())
        .set("Accept", "application/json")
        .send_form(&[("scope", credentials.scope.as_str())])
        .map_err(|e| match e {
            // At sign-in, 400 and 401 both mean the key itself is wrong.
            ureq::Error::Status(400 | 401, _) => {
                "GigaChat didn't accept the authorization key; copy it again from the GigaChat developer page into Settings › AI".to_string()
            }
            other => call_error(other),
        })?;
    let body: Json = response.into_json().map_err(|e| format!("GigaChat sent an unreadable answer ({e})"))?;
    let token = body["access_token"].as_str().ok_or("GigaChat didn't send an access token")?.to_string();
    // Tokens last 30 minutes.
    *cached = Some((token.clone(), Instant::now() + Duration::from_secs(29 * 60)));
    Ok(token)
}

/// Options for one request.
#[derive(Debug, Default, Clone)]
pub struct AskOptions {
    pub model: Option<String>,
    pub temperature: Option<f64>,
    pub max_tokens: Option<u32>,
}

/// Send a conversation (`(role, text)` pairs) and return the answer.
pub fn chat(messages: &[(String, String)], options: &AskOptions, timeout: Duration) -> Result<String, String> {
    let credentials = credentials()?;
    if messages.is_empty() {
        return Err("nothing to ask".into());
    }
    let mut body = json!({
        "model": options.model.clone().unwrap_or_else(|| credentials.model.clone()),
        "messages": messages.iter().map(|(role, content)| json!({ "role": role, "content": content })).collect::<Vec<_>>(),
    });
    if let Some(t) = options.temperature {
        body["temperature"] = json!(t.clamp(0.0, 2.0));
    }
    if let Some(n) = options.max_tokens {
        body["max_tokens"] = json!(n.clamp(1, 32_000));
    }
    let started = Instant::now();
    for attempt in 0..2 {
        let left = timeout.saturating_sub(started.elapsed());
        if left.is_zero() {
            return Err("the time limit was reached while waiting for GigaChat".into());
        }
        let token = token(&credentials, left)?;
        let result = agent(left)?
            .post(CHAT_URL)
            .set("Authorization", &format!("Bearer {token}"))
            .set("Accept", "application/json")
            .send_json(body.clone());
        match result {
            Ok(response) => {
                let answer: Json = response.into_json().map_err(|e| format!("GigaChat sent an unreadable answer ({e})"))?;
                return answer["choices"][0]["message"]["content"]
                    .as_str()
                    .map(|s| s.trim().to_string())
                    .ok_or_else(|| "GigaChat's answer was empty".into());
            }
            // An expired token: sign in again once.
            Err(ureq::Error::Status(401, _)) if attempt == 0 => {
                *TOKEN.lock().unwrap_or_else(|e| e.into_inner()) = None;
            }
            Err(e) => return Err(call_error(e)),
        }
    }
    Err("GigaChat didn't accept the sign-in".into())
}

// ---- answer cache ------------------------------------------------------------------

/// How many saved answers are kept (the oldest go first).
const CACHE_LIMIT: usize = 1000;

#[derive(Default)]
struct AnswerCache {
    /// Where the cache is saved; `None` keeps it in memory only (tests).
    file: Option<PathBuf>,
    /// Question fingerprint -> (answer, saved at in seconds since 1970).
    entries: HashMap<String, (String, i64)>,
}

static CACHE: Mutex<Option<AnswerCache>> = Mutex::new(None);

fn with_cache<T>(f: impl FnOnce(&mut AnswerCache) -> T) -> T {
    let mut guard = CACHE.lock().unwrap_or_else(|e| e.into_inner());
    f(guard.get_or_insert_with(AnswerCache::default))
}

/// Keep saved answers in this file (in LocalFlow's data folder), loading what's there.
pub fn set_cache_file(path: PathBuf) {
    let entries = std::fs::read_to_string(&path)
        .ok()
        .and_then(|text| serde_json::from_str::<HashMap<String, (String, i64)>>(&text).ok())
        .unwrap_or_default();
    with_cache(|cache| {
        cache.file = Some(path);
        cache.entries = entries;
    });
}

/// Forget every saved answer. Returns how many there were.
pub fn clear_cache() -> usize {
    with_cache(|cache| {
        let count = cache.entries.len();
        cache.entries.clear();
        save_cache(cache);
        count
    })
}

pub fn cache_size() -> usize {
    with_cache(|cache| cache.entries.len())
}

fn save_cache(cache: &AnswerCache) {
    let Some(file) = &cache.file else { return };
    // Write a new file and swap it in, so a crash can never leave half a file.
    let temp = file.with_extension("tmp");
    if let Ok(text) = serde_json::to_string(&cache.entries) {
        if std::fs::write(&temp, text).is_ok() {
            let _ = std::fs::rename(&temp, file);
        }
    }
}

/// A fingerprint of everything that shapes the answer.
fn cache_key(messages: &[(String, String)], model: &str, options: &AskOptions) -> String {
    use sha2::{Digest, Sha256};
    let text = serde_json::to_string(&(messages, model, options.temperature, options.max_tokens)).unwrap_or_default();
    Sha256::digest(text.as_bytes()).iter().map(|b| format!("{b:02x}")).collect()
}

/// `chat`, but reusing a saved answer to the same question if it is younger than `max_age`.
pub fn chat_cached(
    messages: &[(String, String)],
    options: &AskOptions,
    timeout: Duration,
    max_age: Option<Duration>,
) -> Result<String, String> {
    let Some(max_age) = max_age else { return chat(messages, options, timeout) };
    let model = options.model.clone().unwrap_or_else(|| credentials().map(|c| c.model).unwrap_or_default());
    let key = cache_key(messages, &model, options);
    let now = chrono::Utc::now().timestamp();
    let hit = with_cache(|cache| {
        cache.entries.get(&key).filter(|(_, at)| now - at <= max_age.as_secs() as i64).map(|(answer, _)| answer.clone())
    });
    if let Some(answer) = hit {
        return Ok(answer);
    }
    let answer = chat(messages, options, timeout)?;
    with_cache(|cache| {
        cache.entries.insert(key, (answer.clone(), now));
        if cache.entries.len() > CACHE_LIMIT {
            let mut by_age: Vec<(String, i64)> = cache.entries.iter().map(|(k, (_, at))| (k.clone(), *at)).collect();
            by_age.sort_by_key(|(_, at)| *at);
            let extra = cache.entries.len() - CACHE_LIMIT;
            for (k, _) in by_age.into_iter().take(extra) {
                cache.entries.remove(&k);
            }
        }
        save_cache(cache);
    });
    Ok(answer)
}

/// Ask one question, optionally with instructions for how to answer.
pub fn ask(question: &str, system: Option<&str>, options: &AskOptions, timeout: Duration) -> Result<String, String> {
    let mut messages = Vec::new();
    if let Some(system) = system.filter(|s| !s.trim().is_empty()) {
        messages.push(("system".to_string(), system.to_string()));
    }
    messages.push(("user".to_string(), question.to_string()));
    chat(&messages, options, timeout)
}

// ---- writing automations ------------------------------------------------------------

/// What the AI is told about LocalFlow when it writes an automation.
const WRITER_PROMPT: &str = r#"You write automations for LocalFlow, a desktop app that runs small Lua 5.4 scripts on the user's computer.
Answer with ONLY the Lua code: no explanations and no Markdown fences. Write short comments in the user's language.

The script has this shape:
automation {
    name = "Short name",
    run = function(ctx)
        -- the work
    end
}

Only these functions exist (plus Lua's string, table, math and utf8). There is no io, os, require of files, or load.
Paths: "~" is the home folder, "/" works as a separator. Files deleted or replaced go to the Recycle Bin / Trash.
fs.list(folder, pattern) -> list of paths ("*.pdf"); fs.find(folder, pattern) (subfolders too); fs.list_dirs(folder)
fs.exists, fs.is_dir, fs.size, fs.modified (timestamp), fs.basename, fs.join(a, b, ...)
fs.move(from, to) (to may be a folder; never overwrites), fs.copy, fs.rename(path, new_name), fs.delete (Recycle Bin), fs.mkdir
fs.read(path), fs.write(path, text), fs.append(path, text), fs.largest(folder, n), fs.duplicates(folder), fs.hash(path)
app.open(name_or_path_or_url), app.running(name), app.list(), app.shortcuts()
time.now(), time.today() -> "2026-09-29", time.format("%d.%m.%Y", t), time.date(t) -> {year, month, day, hour, min, sec, weekday}, time.days(n), time.hours(n), time.minutes(n), time.parse(text), wait(seconds)
log(text), notify(text), ask(question) -> true/false
system.disks(), system.disk_free(path), system.memory(), system.battery(), system.idle_seconds()
metrics.average("cpu"|"memory"|"disk"|"battery", minutes), metrics.peak(...)
image.resize(from, to, max_width), image.convert(from, to), image.taken(path)
csv.read(path) -> rows, csv.write(path, rows, { header = {...} })
zip.create(zip_path, folder), zip.extract(zip_path, folder)
json.encode(value), json.decode(text), http.get(url) -> { ok, status, body }, http.post(url, { json = {...} })
store.get(key, default), store.set(key, value)  -- values kept between runs
clipboard.get(), clipboard.set(text), sound.beep()
ai.ask(question, { system = "..." }) -> text   -- GigaChat
Need "Allow system control": app.close(name) (closes an app politely), shell.run(cmd), process.kill(name), window.find/focus/close/move, keyboard.press("ctrl+s"), keyboard.type(text), mouse.click(x, y), system.lock(), system.sleep(), system.shutdown(delay), system.mute(), system.volume_up(n)
Helpers: local strings = require("lf.strings"), require("lf.tables"), require("lf.paths"), require("lf.dates").
ctx.trigger tells how the run started; ctx.file is the new file for folder-watch runs.
Prefer safe actions (move or copy instead of delete). Use log() so the user sees what happened.

Common mistakes to avoid:
- Only call functions from the list below. There is no close(), exit(), sleep(), print_r(), os.*, io.*, file:read(). Use wait(seconds), not sleep().
- Join text with .. (not +), and wrap numbers with tostring() when joining if unsure.
- Loop over lists with: for _, item in ipairs(list) do ... end. Lists start at 1. #list is the length.
- fs.list returns full paths; use fs.basename(path) for the file name.
- Every if/for/function needs its own end. Strings use "double quotes".
- notify(text) shows a desktop notification; log(text) writes to the log.
- Folders on disk have English names in every language: ~/Downloads, ~/Documents, ~/Desktop, ~/Pictures, ~/Music, ~/Videos.
- Times are numbers (seconds). fs.modified(path) and time.now() are timestamps; time.today() is TEXT like "2026-09-29", never do maths with it.
  Age of a file in days: (time.now() - fs.modified(path)) / time.days(1). Older than a week: time.now() - fs.modified(path) > time.days(7).
- Count only what you actually did (e.g. a moved counter), not the whole list.
- Don't add fields like trigger, schedule or interval to automation { }: schedules and triggers are chosen in the editor, not in code.
- Functions marked "Need Allow system control" only work when the user switches that on; mention it in a comment at the top when you use them."#;

/// The full instructions for writing automations: the rules above, the exact list of
/// functions from the real sandbox, and two real templates as examples.
pub fn writer_prompt() -> String {
    let examples: Vec<&str> = ["organize-pdfs", "low-disk-space"]
        .iter()
        .filter_map(|slug| crate::lua::find_example(slug))
        .map(|e| e.code)
        .collect();
    format!(
        "{WRITER_PROMPT}

The complete list of functions that exist (nothing else does):
{}

Examples of good automations:

{}",
        crate::lua::catalog::catalog().summary(),
        examples.join("

")
    )
}

/// Code written by the AI, and anything the checks still found wrong with it.
#[derive(Debug, Clone, serde::Serialize)]
pub struct WrittenCode {
    pub code: String,
    /// Syntax errors or calls to functions that don't exist (empty when all looks right).
    pub warnings: Vec<String>,
    /// The code uses functions that need "Allow system control".
    pub needs_system_control: bool,
    /// A plain answer, when the user asked a question instead of a change (the code is unchanged).
    pub answer: Option<String>,
}

/// Functions that only work with "Allow system control" switched on.
const SYSTEM_CONTROL: &[&str] = &[
    "app.close", "shell.run", "shell.powershell", "process.kill", "window.focus", "window.minimize", "window.maximize", "window.restore",
    "window.close", "window.move", "keyboard.press", "keyboard.type", "mouse.move", "mouse.click", "system.lock",
    "system.sleep", "system.shutdown", "system.restart", "system.cancel_shutdown", "system.volume_up", "system.volume_down",
    "system.mute", "system.brightness", "system.set_wallpaper", "system.wake_at", "system.cancel_wake",
];

pub fn needs_system_control(code: &str) -> bool {
    let code: String = code.lines().map(|l| l.split("--").next().unwrap_or("")).collect::<Vec<_>>().join("
");
    SYSTEM_CONTROL.iter().any(|f| code.contains(&format!("{f}(")))
}

fn written(code: String, warnings: Vec<String>) -> WrittenCode {
    let needs_system_control = needs_system_control(&code);
    WrittenCode { code, warnings, needs_system_control, answer: None }
}

/// Settings the AI likes to invent inside `automation { }`; LocalFlow ignores them.
const MADE_UP_FIELDS: &[&str] = &["schedule", "trigger", "triggers", "interval", "cron", "every", "when", "enabled"];

/// Problems in generated code: syntax errors first, then invented functions and fields.
pub fn check_code(code: &str) -> Vec<String> {
    if let Err(problem) = crate::lua::engine::validate(code) {
        return vec![problem];
    }
    let mut problems = crate::lua::catalog::unknown_calls(code);
    for line in code.lines() {
        let line = line.trim();
        for field in MADE_UP_FIELDS {
            let assigned = line.strip_prefix(field).is_some_and(|rest| rest.trim_start().starts_with('=') && !rest.trim_start().starts_with("=="));
            if assigned && !line.starts_with("local") {
                problems.push(format!(
                    "automation {{ }} has a \"{field} = …\" field, which LocalFlow ignores: remove it (schedules and triggers are chosen in the editor)"
                ));
            }
        }
    }
    problems
}

/// Folder names the AI translates, and the names they really have on disk.
const FOLDER_NAMES: &[(&str, &str)] = &[
    ("Загрузки", "Downloads"), ("Документы", "Documents"), ("Рабочий стол", "Desktop"), ("Изображения", "Pictures"),
    ("Картинки", "Pictures"), ("Музыка", "Music"), ("Видео", "Videos"), ("Dokumente", "Documents"), ("Bilder", "Pictures"),
    ("Schreibtisch", "Desktop"), ("Musik", "Music"), ("Videos", "Videos"),
];

/// "~/Загрузки" → "~/Downloads": on disk these folders have English names in every language.
pub fn fix_folder_names(code: &str) -> String {
    let mut code = code.to_string();
    for (local, real) in FOLDER_NAMES {
        for prefix in ["~/", "~\\\\"] {
            code = code.replace(&format!("{prefix}{local}"), &format!("~/{real}"));
        }
    }
    code
}

/// One earlier turn of the editor's AI chat: what the user asked, and what came back
/// (code or an answer).
#[derive(Debug, Clone, serde::Deserialize)]
pub struct ChatTurn {
    pub request: String,
    pub reply: String,
}

/// Replies that answer a question instead of changing the code start with this.
const ANSWER_PREFIX: &str = "ANSWER:";

/// Folder paths in `code` ("~/Documents/Archives", "C:/Data"), to check the AI kept them.
fn paths_in(code: &str) -> Vec<String> {
    let mut paths = Vec::new();
    for quote in ['"', '\''] {
        let mut parts = code.split(quote);
        parts.next();
        while let (Some(inside), Some(_)) = (parts.next(), parts.next()) {
            let looks_like_path = inside.starts_with('~') || inside.contains(":/") || inside.contains(":\\");
            if looks_like_path && !inside.contains('\n') && !paths.contains(&inside.to_string()) {
                paths.push(inside.to_string());
            }
        }
    }
    paths
}

/// Turn a description into a Lua automation, or change the current code, as one turn of
/// a conversation. The code is checked for syntax errors, made-up functions and fields, and
/// (when changing code) for folder paths that disappeared; the AI gets up to two more tries
/// with the exact problems. A question gets a plain answer and leaves the code alone.
pub fn write_automation(
    description: &str,
    language: &str,
    current_code: Option<&str>,
    history: &[ChatTurn],
    timeout: Duration,
) -> Result<WrittenCode, String> {
    let description = description.trim();
    if description.is_empty() {
        return Err("describe what the automation should do".into());
    }
    let language = match language {
        "ru" => "Russian",
        "de" => "German",
        _ => "English",
    };
    let current_code = current_code.map(str::trim).filter(|c| !c.is_empty());
    let system = format!(
        "{}\n\nThe user talks to you in a chat next to the code editor. Write comments and log messages in {language}.\n\
         When they ask for a change or a new automation, reply with the whole code only.\n\
         When they ask a question (for example why something works, or what a function does), reply with \
         {ANSWER_PREFIX} followed by a short answer in {language}, without code.",
        writer_prompt()
    );
    let mut messages = vec![("system".to_string(), system)];
    // The conversation so far (the last 10 turns), so follow-ups like "also on weekdays" make sense.
    for turn in history.iter().rev().take(10).rev() {
        messages.push(("user".to_string(), turn.request.clone()));
        messages.push(("assistant".to_string(), turn.reply.clone()));
    }
    messages.push((
        "user".to_string(),
        match current_code {
            Some(code) => format!(
                "The code in the editor now:\n\n{code}\n\nRequest: {description}\n\n\
                 If this is a change, keep everything else exactly as it is: the automation's name, every folder \
                 path and file name (don't translate them), and everything that already works. Send the whole updated code."
            ),
            None => format!("Request: {description}"),
        },
    ));

    let options = AskOptions { temperature: Some(0.2), ..Default::default() };
    let started = Instant::now();
    let kept_paths = current_code.map(paths_in).unwrap_or_default();
    let mut best: Option<WrittenCode> = None;
    for _ in 0..3 {
        let reply = chat(&messages, &options, timeout.saturating_sub(started.elapsed()))?;
        if let Some(answer) = reply.trim().strip_prefix(ANSWER_PREFIX) {
            return Ok(WrittenCode {
                code: current_code.unwrap_or_default().to_string(),
                warnings: Vec::new(),
                needs_system_control: false,
                answer: Some(answer.trim().to_string()),
            });
        }
        let code = fix_folder_names(&strip_fences(&reply));
        let mut warnings = check_code(&code);
        for path in &kept_paths {
            if !code.contains(path.as_str()) && !description.contains(path.as_str()) {
                warnings.push(format!("the folder path \"{path}\" from the current code is missing: keep it exactly as it was (don't translate it)"));
            }
        }
        if warnings.is_empty() {
            return Ok(written(code, warnings));
        }
        // Keep the attempt with the fewest problems.
        if best.as_ref().is_none_or(|b| warnings.len() < b.warnings.len()) {
            best = Some(written(code.clone(), warnings.clone()));
        }
        if started.elapsed() >= timeout {
            break;
        }
        messages.push(("assistant".to_string(), code));
        messages.push((
            "user".to_string(),
            format!(
                "That code has problems:\n- {}\nUse only functions from the list. Send the corrected code only.",
                warnings.join("\n- ")
            ),
        ));
    }
    // Still not right: hand over the best try; the editor shows what's wrong.
    Ok(best.expect("at least one attempt"))
}

/// The code inside ```lua ... ``` fences, if the AI added them anyway.
pub fn strip_fences(text: &str) -> String {
    let trimmed = text.trim();
    if let Some(start) = trimmed.find("```") {
        let after = &trimmed[start + 3..];
        let after = after.strip_prefix("lua").unwrap_or(after);
        let body = after.split("```").next().unwrap_or(after);
        return body.trim().to_string() + "\n";
    }
    trimmed.to_string() + "\n"
}

// ---- Lua ------------------------------------------------------------------------------

fn err(function: &str, error: impl std::fmt::Display) -> mlua::Error {
    mlua::Error::runtime(format!("{function}: {error}"))
}

/// Options from Lua, the system text, and how long a cached answer may be reused.
fn options_from(table: &Option<Table>) -> mlua::Result<(AskOptions, Option<String>, Option<Duration>)> {
    let Some(t) = table else { return Ok((AskOptions::default(), None, None)) };
    // `cache = true` means a day; `cache_hours = n` sets it.
    let hours: Option<f64> = match t.get::<Value>("cache_hours")? {
        Value::Integer(n) => Some(n as f64),
        Value::Number(n) => Some(n),
        _ => t.get::<Option<bool>>("cache")?.filter(|c| *c).map(|_| 24.0),
    };
    Ok((
        AskOptions {
            model: t.get("model")?,
            temperature: t.get("temperature")?,
            max_tokens: t.get("max_tokens")?,
        },
        t.get("system")?,
        hours.filter(|h| *h > 0.0).map(|h| Duration::from_secs_f64(h.min(24.0 * 365.0) * 3600.0)),
    ))
}

/// `ai.conversation(name)`: a chat that remembers its messages between runs (in `store`).
const CONVERSATION_LUA: &str = r#"
local ai = ...
function ai.conversation(name, options)
    options = options or {}
    local key = "ai.conversation:" .. tostring(name or "default")
    local keep = options.keep or 20
    local chat = {}
    function chat:history()
        return store.get(key, {})
    end
    function chat:ask(text)
        local history = store.get(key, {})
        history[#history + 1] = { role = "user", content = tostring(text) }
        local answer = ai.chat(history, options)
        history[#history + 1] = { role = "assistant", content = answer }
        while #history > keep do
            table.remove(history, 1)
        end
        store.set(key, history)
        return answer
    end
    function chat:forget()
        store.delete(key)
    end
    return chat
end
"#;

/// Seconds left before the script's time limit, at most two minutes per request.
fn budget(deadline: Instant, function: &str) -> mlua::Result<Duration> {
    let left = deadline.saturating_duration_since(Instant::now()).min(Duration::from_secs(120));
    if left.is_zero() {
        return Err(err(function, "the time limit was reached"));
    }
    Ok(left)
}

pub fn register(lua: &Lua, deadline: Instant) -> mlua::Result<()> {
    let ai = lua.create_table()?;
    ai.set("available", lua.create_function(|_, ()| Ok(is_configured()))?)?;
    ai.set(
        "ask",
        lua.create_function(move |_, (question, options): (String, Option<Table>)| {
            let (options, system, cache) = options_from(&options)?;
            let mut messages = Vec::new();
            if let Some(system) = system.filter(|s| !s.trim().is_empty()) {
                messages.push(("system".to_string(), system));
            }
            messages.push(("user".to_string(), question));
            chat_cached(&messages, &options, budget(deadline, "ai.ask")?, cache).map_err(|e| err("ai.ask", e))
        })?,
    )?;
    ai.set(
        "chat",
        lua.create_function(move |_, (list, options): (Table, Option<Table>)| {
            let (options, system, cache) = options_from(&options)?;
            let mut messages = Vec::new();
            if let Some(system) = system {
                messages.push(("system".to_string(), system));
            }
            for item in list.sequence_values::<Value>() {
                let Value::Table(m) = item? else { return Err(err("ai.chat", "each message must be { role = ..., content = ... }")) };
                let role: String = m.get::<Option<String>>("role")?.unwrap_or_else(|| "user".into());
                if !["system", "user", "assistant"].contains(&role.as_str()) {
                    return Err(err("ai.chat", format!("unknown role \"{role}\" (use user, assistant or system)")));
                }
                messages.push((role, m.get::<String>("content")?));
            }
            chat_cached(&messages, &options, budget(deadline, "ai.chat")?, cache).map_err(|e| err("ai.chat", e))
        })?,
    )?;
    lua.load(CONVERSATION_LUA).set_name("=ai.conversation").call::<()>(ai.clone())?;
    lua.globals().set("ai", ai)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_certificate_loads() {
        assert!(agent(Duration::from_secs(5)).is_ok());
    }

    #[test]
    fn request_ids_are_uuids() {
        let id = request_id();
        assert_eq!(id.len(), 36);
        assert_eq!(&id[14..15], "4");
        assert_ne!(id, request_id());
    }

    #[test]
    fn fences_are_removed() {
        assert_eq!(strip_fences("```lua\nlog(1)\n```"), "log(1)\n");
        assert_eq!(strip_fences("Here you go:\n```\nlog(2)\n```\nEnjoy"), "log(2)\n");
        assert_eq!(strip_fences("log(3)"), "log(3)\n");
    }

    #[test]
    fn translated_folders_and_made_up_fields_are_handled() {
        assert_eq!(fix_folder_names(r#"fs.list("~/Загрузки", "*.pdf") fs.move(f, "~/Документы/Old")"#), r#"fs.list("~/Downloads", "*.pdf") fs.move(f, "~/Documents/Old")"#);
        let problems = check_code("automation {
  name = \"x\",
  run = function(ctx) local every = 5 log(every) end,
  schedule = \"0 * * * *\"
}");
        assert_eq!(problems.len(), 1, "{problems:?}");
        assert!(problems[0].contains("schedule"));
    }

    #[test]
    fn system_control_is_noticed() {
        assert!(needs_system_control("window.close(w)"));
        assert!(!needs_system_control("-- window.close(w)
log(1)"));
        assert!(!needs_system_control("window.find(\"x\")"));
    }

    #[test]
    fn the_writer_prompt_lists_real_functions_and_examples() {
        let prompt = writer_prompt();
        assert!(prompt.contains("fs: ") && prompt.contains("notify") && prompt.contains("Organize PDF files"));
        assert!(!prompt.contains("close,") || prompt.contains("window: "), "close only appears as window.close");
    }

    #[test]
    fn answers_are_cached_by_question() {
        let options = AskOptions::default();
        let q = |text: &str| vec![("user".to_string(), text.to_string())];
        assert_eq!(cache_key(&q("a"), "m", &options), cache_key(&q("a"), "m", &options));
        assert_ne!(cache_key(&q("a"), "m", &options), cache_key(&q("b"), "m", &options));
        assert_ne!(cache_key(&q("a"), "m", &options), cache_key(&q("a"), "other", &options));
        let warm = AskOptions { temperature: Some(1.0), ..Default::default() };
        assert_ne!(cache_key(&q("a"), "m", &options), cache_key(&q("a"), "m", &warm));
    }

    #[test]
    fn cache_file_survives_a_restart() {
        let dir = tempfile::TempDir::new().unwrap();
        let file = dir.path().join("ai_cache.json");
        set_cache_file(file.clone());
        with_cache(|c| {
            c.entries.insert("k".into(), ("answer".into(), 1));
            save_cache(c);
        });
        set_cache_file(file);
        assert_eq!(cache_size(), 1);
        assert_eq!(clear_cache(), 1);
        assert_eq!(cache_size(), 0);
    }

    #[test]
    fn folder_paths_are_found() {
        let code = "fs.move(f, \"~/Documents/Archives/\") log(\"hi\") fs.list('C:/Data', '*')";
        assert_eq!(paths_in(code), vec!["~/Documents/Archives/".to_string(), "C:/Data".to_string()]);
    }

    #[test]
    fn errors_are_explained() {
        assert!(explain(401, "").contains("authorization key"));
        assert!(explain(402, r#"{"message":"Payment Required"}"#).contains("Payment Required"));
    }

    #[test]
    fn nothing_works_without_a_key() {
        configure(None);
        let e = ask("hi", None, &AskOptions::default(), Duration::from_secs(1)).unwrap_err();
        assert!(e.contains("Settings › AI"));
        assert!(format!("{:?}", Credentials { key: "secret".into(), scope: "s".into(), model: "m".into() }).find("secret").is_none());
    }
}
