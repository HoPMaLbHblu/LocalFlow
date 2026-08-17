//! Telegram remote control: while it's on, LocalFlow listens for messages from the
//! owner's chat (and nobody else's) and answers commands like /status or /screenshot.
//! The commands live in `scripts/remote_commands.lua`; /list and /run are handled here.
//! Every command also shows a notification on the PC, so remote use is never silent.

use std::{
    collections::HashMap,
    sync::atomic::{AtomicBool, Ordering},
    time::Duration,
};

use crate::{messaging, LocalFlow};

static ENABLED: AtomicBool = AtomicBool::new(false);
static ALLOW_POWER: AtomicBool = AtomicBool::new(false);

pub const COMMANDS: &str = include_str!("../scripts/remote_commands.lua");

pub fn configure(enabled: bool, allow_power: bool) {
    ENABLED.store(enabled, Ordering::SeqCst);
    ALLOW_POWER.store(allow_power, Ordering::SeqCst);
}

pub fn is_enabled() -> bool {
    ENABLED.load(Ordering::SeqCst)
}

/// A Lua string literal that is safe for any text: every byte is written as \ddd.
pub fn lua_string(text: &str) -> String {
    let mut out = String::with_capacity(text.len() * 4 + 2);
    out.push('"');
    for byte in text.bytes() {
        out.push_str(&format!("\\{byte:03}"));
    }
    out.push('"');
    out
}

/// Runs forever; does nothing while remote control is off or Telegram isn't set up.
pub async fn serve(flow: LocalFlow) {
    loop {
        if !is_enabled() || !messaging::telegram_ready() {
            tokio::time::sleep(Duration::from_secs(3)).await;
            continue;
        }
        // Long polling: Telegram holds the request until a message arrives (up to 25 s).
        let messages = tokio::task::spawn_blocking(|| messaging::telegram_wait(25, Duration::from_secs(35))).await;
        match messages {
            Ok(Ok(list)) => {
                for (_, command, args) in list {
                    if is_enabled() {
                        handle(&flow, &command, &args).await;
                    }
                }
            }
            Ok(Err(e)) => {
                tracing::warn!("Telegram remote control: {e}");
                tokio::time::sleep(Duration::from_secs(15)).await;
            }
            Err(_) => tokio::time::sleep(Duration::from_secs(15)).await,
        }
    }
}

async fn send(text: String) {
    let _ = tokio::task::spawn_blocking(move || messaging::telegram_send(&text, Duration::from_secs(30))).await;
}

async fn handle(flow: &LocalFlow, command: &str, args: &str) {
    let shown = if command.is_empty() { "a message" } else { command };
    flow.notify_desktop(&format!("Telegram remote control: {shown} {args}").trim().to_string());

    match command {
        "/list" => {
            let names: Vec<String> = match flow.list().await {
                Ok(list) => list.iter().map(|a| format!("{} {}", if a.automation.enabled { "•" } else { "◦" }, a.automation.name)).collect(),
                Err(e) => vec![format!("Could not read the list: {e}")],
            };
            let text = if names.is_empty() { "You have no automations yet".to_string() } else { names.join("\n") };
            send(format!("{text}\n\nRun one with /run <name>")).await;
        }
        "/run" => {
            if args.is_empty() {
                send("Which one? For example: /run Tidy screenshots. Send /list to see them.".into()).await;
                return;
            }
            let list = flow.list().await.unwrap_or_default();
            let wanted = args.to_lowercase();
            let found = list
                .iter()
                .find(|a| a.automation.name.to_lowercase() == wanted)
                .or_else(|| list.iter().find(|a| a.automation.name.to_lowercase().contains(&wanted)));
            let Some(found) = found else {
                send(format!("No automation called \"{args}\". Send /list to see them.")).await;
                return;
            };
            let (id, name) = (found.automation.id, found.automation.name.clone());
            send(format!("Running {name}...")).await;
            let text = match flow.run_with_details(id, "telegram", None, HashMap::new()).await {
                Ok(run) => {
                    let output: String = run.output.unwrap_or_default().chars().rev().take(3000).collect::<Vec<_>>().into_iter().rev().collect();
                    match run.error {
                        Some(error) => format!("{name} failed: {error}\n\n{output}"),
                        None => format!("{name} finished.\n\n{output}"),
                    }
                }
                Err(e) => format!("Could not run {name}: {e}"),
            };
            send(text.trim().to_string()).await;
        }
        _ => {
            let code = format!(
                "COMMAND = {}\nARGS = {}\nALLOW_POWER = {}\n{}",
                lua_string(command),
                lua_string(args),
                ALLOW_POWER.load(Ordering::SeqCst),
                COMMANDS
            );
            let result = flow.test_run_with(code, "Telegram remote control".into(), true).await;
            if !result.success {
                let error = result.error.unwrap_or_default();
                send(format!("That didn't work: {}", error.lines().next().unwrap_or(""))).await;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn any_text_becomes_a_safe_lua_string() {
        let tricky = "a\"b\\c]]\n--x ${}";
        let lua = mlua::Lua::new();
        let back: String = lua.load(format!("return {}", lua_string(tricky))).eval().unwrap();
        assert_eq!(back, tricky);
        let back: String = lua.load(format!("return {}", lua_string("Привет"))).eval().unwrap();
        assert_eq!(back, "Привет");
    }

    #[test]
    fn the_commands_script_compiles() {
        crate::lua::engine::validate(COMMANDS).unwrap();
    }
}
