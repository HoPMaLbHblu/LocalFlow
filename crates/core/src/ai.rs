//! AI through GigaChat (Sber): `ai.ask` in scripts, and writing automations from a description.
//!
//! ```lua
//! ai.available()                                -- true when a GigaChat key is set in Settings
//! ai.ask(question, { system, model, temperature, max_tokens })   -- the answer as text
//! ai.chat({ { role = "user", content = "..." }, ... }, options)  -- a whole conversation
//! ```
//!
//! The authorization key lives in the operating system's password store (set from
//! Settings) and is handed to this module by the app. Scripts can use the AI but can
//! never read the key. GigaChat's servers use the Russian Trusted Root CA certificate,
//! which is trusted here only for GigaChat's own connection, not for `http.get`.

use std::{
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
Need "Allow system control": shell.run(cmd), process.kill(name), window.find/focus/close/move, keyboard.press("ctrl+s"), keyboard.type(text), mouse.click(x, y), system.lock(), system.sleep(), system.shutdown(delay), system.mute(), system.volume_up(n)
Helpers: local strings = require("lf.strings"), require("lf.tables"), require("lf.paths"), require("lf.dates").
ctx.trigger tells how the run started; ctx.file is the new file for folder-watch runs.
Prefer safe actions (move or copy instead of delete). Use log() so the user sees what happened."#;

/// Turn a description into a Lua automation, checking that the code compiles.
pub fn write_automation(description: &str, language: &str, timeout: Duration) -> Result<String, String> {
    let description = description.trim();
    if description.is_empty() {
        return Err("describe what the automation should do".into());
    }
    let language = match language {
        "ru" => "Russian",
        "de" => "German",
        _ => "English",
    };
    let mut messages = vec![
        ("system".to_string(), WRITER_PROMPT.to_string()),
        ("user".to_string(), format!("Write comments and log messages in {language}.\n\nThe automation should: {description}")),
    ];
    let options = AskOptions { temperature: Some(0.2), ..Default::default() };
    let started = Instant::now();
    let mut code = String::new();
    for _ in 0..2 {
        code = strip_fences(&chat(&messages, &options, timeout.saturating_sub(started.elapsed()))?);
        match crate::lua::engine::validate(&code) {
            Ok(()) => return Ok(code),
            // One more try, telling the AI what was wrong.
            Err(problem) => {
                messages.push(("assistant".to_string(), code.clone()));
                messages.push(("user".to_string(), format!("That code has an error: {problem}. Send the corrected code only.")));
            }
        }
    }
    // Still broken: hand it over anyway; the editor shows the error.
    Ok(code)
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

fn options_from(table: &Option<Table>) -> mlua::Result<(AskOptions, Option<String>)> {
    let Some(t) = table else { return Ok((AskOptions::default(), None)) };
    Ok((
        AskOptions {
            model: t.get("model")?,
            temperature: t.get("temperature")?,
            max_tokens: t.get("max_tokens")?,
        },
        t.get("system")?,
    ))
}

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
            let (options, system) = options_from(&options)?;
            ask(&question, system.as_deref(), &options, budget(deadline, "ai.ask")?).map_err(|e| err("ai.ask", e))
        })?,
    )?;
    ai.set(
        "chat",
        lua.create_function(move |_, (list, options): (Table, Option<Table>)| {
            let (options, system) = options_from(&options)?;
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
            chat(&messages, &options, budget(deadline, "ai.chat")?).map_err(|e| err("ai.chat", e))
        })?,
    )?;
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
