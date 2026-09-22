//! Telegram and Discord: send yourself messages, screenshots and files, and control the
//! PC from your phone with Telegram commands. The bot token and the Discord webhook are
//! secrets: the desktop app keeps them in the system's password store and hands them over
//! here; scripts never see them.
//!
//! ```lua
//! telegram.available()                      discord.available()
//! telegram.send(text)                       discord.send(text)
//! telegram.send_photo(path, caption)        discord.send_file(path, text)
//! telegram.send_file(path, caption)
//! for _, c in ipairs(telegram.commands()) do  -- new messages from YOUR chat only
//!     -- c.command = "/volume", c.args = "20", c.text = "/volume 20"
//! end
//! ```

use std::{
    path::Path,
    sync::{atomic::{AtomicI64, Ordering}, RwLock},
    time::{Duration, Instant},
};

use mlua::{Lua, Table};

#[derive(Clone, Default)]
pub struct Bots {
    pub telegram_token: Option<String>,
    /// Only messages from this chat are read, and messages go only here.
    pub telegram_chat: Option<i64>,
    pub discord_webhook: Option<String>,
}

static BOTS: RwLock<Option<Bots>> = RwLock::new(None);
/// The next Telegram update to read, so each command is handled once.
static NEXT_UPDATE: AtomicI64 = AtomicI64::new(0);

pub fn configure(bots: Bots) {
    *BOTS.write().unwrap() = Some(bots);
}

fn bots() -> Bots {
    BOTS.read().unwrap().clone().unwrap_or_default()
}

fn telegram() -> Result<(String, i64), String> {
    let b = bots();
    match (b.telegram_token, b.telegram_chat) {
        (Some(token), Some(chat)) => Ok((token, chat)),
        _ => Err("set up the Telegram bot in Settings first (bot token and your chat)".into()),
    }
}

fn agent(timeout: Duration) -> ureq::Agent {
    ureq::AgentBuilder::new().timeout(timeout).user_agent(concat!("LocalFlow/", env!("CARGO_PKG_VERSION"))).build()
}

/// Telegram's answer, or its error text. The token is never put in error messages.
fn telegram_call(token: &str, method: &str, body: serde_json::Value, timeout: Duration) -> Result<serde_json::Value, String> {
    let url = format!("https://api.telegram.org/bot{token}/{method}");
    let response = match agent(timeout).post(&url).send_json(body) {
        Ok(r) | Err(ureq::Error::Status(_, r)) => r,
        Err(ureq::Error::Transport(e)) => return Err(format!("could not reach Telegram ({})", e.kind())),
    };
    let json: serde_json::Value = response.into_json().map_err(|e| e.to_string())?;
    if json["ok"].as_bool() == Some(true) {
        Ok(json["result"].clone())
    } else {
        Err(json["description"].as_str().unwrap_or("Telegram refused the request").to_string())
    }
}

/// A multipart/form-data body for uploading one file.
pub fn multipart(fields: &[(&str, String)], file_field: &str, file_name: &str, bytes: &[u8]) -> (String, Vec<u8>) {
    let boundary = format!("----LocalFlow{:x}", std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_nanos());
    let mut body = Vec::new();
    for (name, value) in fields {
        body.extend(format!("--{boundary}\r\nContent-Disposition: form-data; name=\"{name}\"\r\n\r\n{value}\r\n").as_bytes());
    }
    let safe_name = file_name.replace(['"', '\r', '\n'], "_");
    body.extend(
        format!("--{boundary}\r\nContent-Disposition: form-data; name=\"{file_field}\"; filename=\"{safe_name}\"\r\nContent-Type: application/octet-stream\r\n\r\n").as_bytes(),
    );
    body.extend(bytes);
    body.extend(format!("\r\n--{boundary}--\r\n").as_bytes());
    (format!("multipart/form-data; boundary={boundary}"), body)
}

const MAX_UPLOAD: u64 = 45 * 1024 * 1024;

fn read_upload(path: &Path) -> Result<(String, Vec<u8>), String> {
    let size = std::fs::metadata(path).map_err(|e| format!("{}: {e}", path.display()))?.len();
    if size > MAX_UPLOAD {
        return Err("the file is larger than 45 MB, the most bots can send".into());
    }
    let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| "file".into());
    Ok((name, std::fs::read(path).map_err(|e| e.to_string())?))
}

fn telegram_upload(method: &str, field: &str, path: &Path, caption: Option<&str>, timeout: Duration) -> Result<(), String> {
    let (token, chat) = telegram()?;
    let (name, bytes) = read_upload(path)?;
    let mut fields = vec![("chat_id", chat.to_string())];
    if let Some(caption) = caption {
        fields.push(("caption", caption.chars().take(1000).collect()));
    }
    let (content_type, body) = multipart(&fields, field, &name, &bytes);
    let url = format!("https://api.telegram.org/bot{token}/{method}");
    let response = match agent(timeout).post(&url).set("Content-Type", &content_type).send_bytes(&body) {
        Ok(r) | Err(ureq::Error::Status(_, r)) => r,
        Err(ureq::Error::Transport(e)) => return Err(format!("could not reach Telegram ({})", e.kind())),
    };
    let json: serde_json::Value = response.into_json().map_err(|e| e.to_string())?;
    if json["ok"].as_bool() == Some(true) {
        Ok(())
    } else {
        Err(json["description"].as_str().unwrap_or("Telegram refused the file").to_string())
    }
}

pub fn discord_ready() -> bool {
    discord_webhook().is_ok()
}

pub fn telegram_ready() -> bool {
    telegram().is_ok()
}

/// Waits up to `poll_seconds` for new messages from the owner's chat (long polling).
pub fn telegram_wait(poll_seconds: u32, timeout: Duration) -> Result<Vec<(String, String, String)>, String> {
    read_commands(poll_seconds, timeout)
}

pub fn telegram_send(text: &str, timeout: Duration) -> Result<(), String> {
    let (token, chat) = telegram()?;
    // Telegram's limit is 4096 characters per message.
    let chars: Vec<char> = text.chars().collect();
    for part in chars.chunks(4000) {
        let part: String = part.iter().collect();
        telegram_call(&token, "sendMessage", serde_json::json!({ "chat_id": chat, "text": part }), timeout)?;
    }
    Ok(())
}

/// A message from the bot's updates: (update id, chat id, first name, text).
pub fn parse_updates(result: &serde_json::Value) -> Vec<(i64, i64, String, String)> {
    result
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|u| {
            let id = u["update_id"].as_i64()?;
            let message = if u["message"].is_object() { &u["message"] } else { &u["edited_message"] };
            let chat = message["chat"]["id"].as_i64().unwrap_or(0);
            let name = message["from"]["first_name"].as_str().unwrap_or("").to_string();
            let text = message["text"].as_str().or(message["caption"].as_str()).unwrap_or("").to_string();
            Some((id, chat, name, text))
        })
        .collect()
}

/// "/volume@MyBot 20" -> ("/volume", "20"); plain text -> ("", text)
pub fn split_command(text: &str) -> (String, String) {
    let text = text.trim();
    if !text.starts_with('/') {
        return (String::new(), text.to_string());
    }
    let (first, rest) = text.split_once(char::is_whitespace).unwrap_or((text, ""));
    let command = first.split('@').next().unwrap_or(first).to_lowercase();
    (command, rest.trim().to_string())
}

/// New messages from the owner's chat. Messages from anyone else are skipped (and
/// marked as read, so they don't pile up).
pub fn telegram_commands(timeout: Duration) -> Result<Vec<(String, String, String)>, String> {
    if crate::remote::is_enabled() {
        return Err("remote control is on, so LocalFlow reads the bot's messages itself. Turn it off in Settings, or use /run <automation> in Telegram".into());
    }
    read_commands(0, timeout)
}

fn read_commands(poll_seconds: u32, timeout: Duration) -> Result<Vec<(String, String, String)>, String> {
    let (token, chat) = telegram()?;
    let offset = NEXT_UPDATE.load(Ordering::SeqCst);
    let result = telegram_call(
        &token,
        "getUpdates",
        serde_json::json!({ "offset": offset, "timeout": poll_seconds, "allowed_updates": ["message", "edited_message"] }),
        timeout,
    )?;
    let mut out = Vec::new();
    for (id, from_chat, _, text) in parse_updates(&result) {
        NEXT_UPDATE.fetch_max(id + 1, Ordering::SeqCst);
        if from_chat == chat && !text.is_empty() {
            let (command, args) = split_command(&text);
            out.push((text, command, args));
        }
    }
    Ok(out)
}

/// For Settings: the people who wrote to the bot lately, newest first: (chat id, name).
/// Doesn't mark anything as read.
pub fn telegram_recent_chats(token: &str, timeout: Duration) -> Result<Vec<(i64, String)>, String> {
    let result = telegram_call(token, "getUpdates", serde_json::json!({ "timeout": 0 }), timeout)?;
    let mut chats: Vec<(i64, String)> = Vec::new();
    for (_, chat, name, _) in parse_updates(&result).into_iter().rev() {
        if chat > 0 && !chats.iter().any(|(c, _)| *c == chat) {
            chats.push((chat, name));
        }
    }
    Ok(chats)
}

/// The bot's @username, to check a token.
pub fn telegram_bot_name(token: &str, timeout: Duration) -> Result<String, String> {
    let me = telegram_call(token, "getMe", serde_json::json!({}), timeout)?;
    Ok(me["username"].as_str().unwrap_or("").to_string())
}

pub fn is_discord_webhook(url: &str) -> bool {
    let url = url.trim();
    ["https://discord.com/api/webhooks/", "https://discordapp.com/api/webhooks/", "https://ptb.discord.com/api/webhooks/", "https://canary.discord.com/api/webhooks/"]
        .iter()
        .any(|p| url.starts_with(p))
}

fn discord_webhook() -> Result<String, String> {
    bots().discord_webhook.ok_or_else(|| "set up the Discord webhook in Settings first".into())
}

fn discord_result(result: Result<ureq::Response, ureq::Error>) -> Result<(), String> {
    match result {
        Ok(_) => Ok(()),
        Err(ureq::Error::Status(code, r)) => {
            let text = r.into_string().unwrap_or_default();
            Err(format!("Discord refused the message ({code}) {}", text.chars().take(200).collect::<String>()))
        }
        Err(ureq::Error::Transport(e)) => Err(format!("could not reach Discord ({})", e.kind())),
    }
}

pub fn discord_send(text: &str, timeout: Duration) -> Result<(), String> {
    let url = discord_webhook()?;
    let chars: Vec<char> = text.chars().collect();
    for part in chars.chunks(1900) {
        let part: String = part.iter().collect();
        discord_result(agent(timeout).post(&url).send_json(serde_json::json!({ "content": part })))?;
    }
    Ok(())
}

fn discord_file(path: &Path, text: Option<&str>, timeout: Duration) -> Result<(), String> {
    let url = discord_webhook()?;
    let (name, bytes) = read_upload(path)?;
    let mut fields = Vec::new();
    if let Some(text) = text {
        fields.push(("content", text.chars().take(1900).collect::<String>()));
    }
    let (content_type, body) = multipart(&fields, "file", &name, &bytes);
    discord_result(agent(timeout).post(&url).set("Content-Type", &content_type).send_bytes(&body))
}

fn err(function: &str, error: impl std::fmt::Display) -> mlua::Error {
    mlua::Error::runtime(format!("{function}: {error}"))
}

fn limit(deadline: Instant, function: &str) -> mlua::Result<Duration> {
    let left = deadline.saturating_duration_since(Instant::now()).min(Duration::from_secs(120));
    if left.is_zero() {
        return Err(err(function, "no time left before the script's time limit"));
    }
    Ok(left)
}

pub fn register(lua: &Lua, policy: std::sync::Arc<crate::lua::sandbox::PathPolicy>, deadline: Instant) -> mlua::Result<()> {
    let globals = lua.globals();

    let tg = lua.create_table()?;
    tg.set("available", lua.create_function(|_, ()| Ok(telegram().is_ok()))?)?;
    tg.set(
        "send",
        lua.create_function(move |_, text: String| {
            telegram_send(&text, limit(deadline, "telegram.send")?).map_err(|e| err("telegram.send", e))?;
            Ok(true)
        })?,
    )?;
    for (name, method, field) in [("send_photo", "sendPhoto", "photo"), ("send_file", "sendDocument", "document")] {
        let function = format!("telegram.{name}");
        let policy = policy.clone();
        tg.set(
            name,
            lua.create_function(move |_, (path, caption): (String, Option<String>)| {
                let path = policy.resolve(&path).map_err(|e| err(&function, e))?;
                telegram_upload(method, field, &path, caption.as_deref(), limit(deadline, &function)?).map_err(|e| err(&function, e))?;
                Ok(true)
            })?,
        )?;
    }
    tg.set(
        "commands",
        lua.create_function(move |lua, ()| {
            let list = telegram_commands(limit(deadline, "telegram.commands")?).map_err(|e| err("telegram.commands", e))?;
            let table = lua.create_table()?;
            for (text, command, args) in list {
                let entry: Table = lua.create_table()?;
                entry.set("text", text)?;
                entry.set("command", command)?;
                entry.set("args", args)?;
                table.push(entry)?;
            }
            Ok(table)
        })?,
    )?;
    globals.set("telegram", tg)?;

    let dc = lua.create_table()?;
    dc.set("available", lua.create_function(|_, ()| Ok(discord_webhook().is_ok()))?)?;
    dc.set(
        "send",
        lua.create_function(move |_, text: String| {
            discord_send(&text, limit(deadline, "discord.send")?).map_err(|e| err("discord.send", e))?;
            Ok(true)
        })?,
    )?;
    dc.set(
        "send_file",
        lua.create_function(move |_, (path, text): (String, Option<String>)| {
            let path = policy.resolve(&path).map_err(|e| err("discord.send_file", e))?;
            discord_file(&path, text.as_deref(), limit(deadline, "discord.send_file")?).map_err(|e| err("discord.send_file", e))?;
            Ok(true)
        })?,
    )?;
    globals.set("discord", dc)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn commands_are_split() {
        assert_eq!(split_command("/volume 20"), ("/volume".into(), "20".into()));
        assert_eq!(split_command("/Open@MyPcBot  Google Chrome "), ("/open".into(), "Google Chrome".into()));
        assert_eq!(split_command("hello there"), ("".into(), "hello there".into()));
    }

    #[test]
    fn updates_are_read() {
        let json = serde_json::json!([
            { "update_id": 5, "message": { "chat": { "id": 42 }, "from": { "first_name": "Sam" }, "text": "/status" } },
            { "update_id": 6, "edited_message": { "chat": { "id": 7 }, "from": { "first_name": "X" }, "text": "hi" } },
            { "update_id": 7, "my_chat_member": {} }
        ]);
        let updates = parse_updates(&json);
        assert_eq!(updates.len(), 3);
        assert_eq!(updates[0], (5, 42, "Sam".into(), "/status".into()));
        assert_eq!(updates[1].1, 7);
        assert_eq!(updates[2].3, "");
    }

    #[test]
    fn only_real_discord_webhooks() {
        assert!(is_discord_webhook("https://discord.com/api/webhooks/1/abc"));
        assert!(!is_discord_webhook("https://example.com/api/webhooks/1/abc"));
        assert!(!is_discord_webhook("http://discord.com/api/webhooks/1/abc"));
    }

    #[test]
    fn multipart_has_the_file_and_fields() {
        let (kind, body) = multipart(&[("chat_id", "42".into())], "photo", "a\"b.png", b"PNGDATA");
        let text = String::from_utf8_lossy(&body);
        let boundary = kind.split("boundary=").nth(1).unwrap();
        assert!(text.contains("name=\"chat_id\"\r\n\r\n42\r\n"));
        assert!(text.contains("filename=\"a_b.png\""));
        assert!(text.contains("PNGDATA"));
        assert!(text.ends_with(&format!("--{boundary}--\r\n")));
    }
}
