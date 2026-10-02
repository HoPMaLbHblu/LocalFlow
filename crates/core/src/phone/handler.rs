//! What a paired phone may ask LocalFlow to do. Every method checks the phone's permissions
//! first. System actions run as tiny Lua snippets through the normal engine, so they get the
//! same sandbox and behaviour as in scripts.

use std::collections::{BTreeSet, HashMap};

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::remote::{lua_string, pick_automation, Pick};
use crate::{AutomationInput, LocalFlow};

/// One permission a phone can be given. `Power`, `Clipboard` and `Screen` start off.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, Hash)]
#[serde(rename_all = "snake_case")]
pub enum Permission {
    Status,
    View,
    Run,
    Edit,
    Power,
    Clipboard,
    Share,
    Screen,
}

impl Permission {
    pub const ALL: [Permission; 8] = [
        Permission::Status,
        Permission::View,
        Permission::Run,
        Permission::Edit,
        Permission::Power,
        Permission::Clipboard,
        Permission::Share,
        Permission::Screen,
    ];

    /// What a newly paired phone gets.
    pub fn defaults() -> BTreeSet<Permission> {
        [Permission::Status, Permission::View, Permission::Run, Permission::Share].into()
    }
}

#[derive(Debug, Deserialize)]
pub struct Request {
    pub id: u64,
    pub m: String,
    #[serde(default)]
    pub p: Value,
}

/// A failure the phone can show.
#[derive(Debug)]
pub struct Failure {
    pub code: &'static str,
    pub msg: String,
}

fn fail(code: &'static str, msg: impl Into<String>) -> Failure {
    Failure { code, msg: msg.into() }
}

type Out = Result<Value, Failure>;

/// Information about this PC used by `pc.info`.
pub struct PcInfo {
    pub name: String,
    pub version: String,
}

pub fn required(method: &str) -> Option<Permission> {
    use Permission::*;
    Some(match method {
        "pc.info" => return None,
        "pc.status" => Status,
        "automations.list" | "automations.get" | "runs.list" | "runs.get" | "runs.log" => View,
        "automations.run" | "runs.stop" | "voice.command" | "confirm.answer" => Run,
        "automations.set_enabled" | "automations.set_schedule" => Edit,
        "system.lock" | "system.sleep" | "system.volume" | "system.mute" | "wol.wake" => Power,
        "clipboard.get" | "clipboard.set" => Clipboard,
        "share.open_url" | "share.text" => Share,
        "screen.capture" => Screen,
        _ => return None,
    })
}

/// Answer one request: `{"id":..,"ok":true,"r":..}` or `{"id":..,"ok":false,"e":{..}}`.
pub async fn handle(flow: &LocalFlow, perms: &BTreeSet<Permission>, info: &PcInfo, req: Request) -> Value {
    let result = match required(&req.m) {
        Some(p) if !perms.contains(&p) => Err(fail("forbidden", format!("This phone isn't allowed to do that ({p:?}). Change it in LocalFlow → Settings → Phone remote."))),
        _ => dispatch(flow, perms, info, &req.m, &req.p).await,
    };
    match result {
        Ok(r) => json!({ "id": req.id, "ok": true, "r": r }),
        Err(e) => json!({ "id": req.id, "ok": false, "e": { "code": e.code, "msg": e.msg } }),
    }
}

fn int(p: &Value, key: &str) -> Result<i64, Failure> {
    p.get(key).and_then(Value::as_i64).ok_or_else(|| fail("bad_request", format!("missing number `{key}`")))
}

fn text<'a>(p: &'a Value, key: &str, max: usize) -> Result<&'a str, Failure> {
    let s = p.get(key).and_then(Value::as_str).ok_or_else(|| fail("bad_request", format!("missing text `{key}`")))?;
    if s.len() > max {
        return Err(fail("bad_request", format!("`{key}` is too long")));
    }
    Ok(s)
}

fn core(e: crate::CoreError) -> Failure {
    fail("error", e.to_string())
}

/// Run a short Lua snippet with system control allowed; returns what it logged.
async fn lua(flow: &LocalFlow, code: String) -> Result<Vec<String>, Failure> {
    let result = flow.test_run_with(code, "Phone remote".into(), true).await;
    if !result.success {
        return Err(fail("error", result.error.unwrap_or_else(|| "failed".into())));
    }
    Ok(result.logs.into_iter().map(|l| l.message).collect())
}

async fn dispatch(flow: &LocalFlow, perms: &BTreeSet<Permission>, info: &PcInfo, m: &str, p: &Value) -> Out {
    match m {
        "pc.info" => Ok(json!({
            "name": info.name,
            "version": info.version,
            "os": std::env::consts::OS,
            "permissions": perms,
        })),

        "pc.status" => {
            let s = crate::metrics::latest().unwrap_or_else(crate::metrics::measure);
            let running = flow.running();
            Ok(json!({
                "cpu": s.cpu, "memory": s.memory, "disk": s.disk, "battery": s.battery, "at": s.at,
                "running": running.iter().map(|r| json!({ "run_id": r.run_id, "automation_id": r.automation_id, "name": r.name, "started_at": r.started_at })).collect::<Vec<_>>(),
            }))
        }

        "automations.list" => {
            let list = flow.list().await.map_err(core)?;
            Ok(Value::Array(
                list.into_iter()
                    .map(|a| {
                        let x = &a.automation;
                        json!({
                            "id": x.id, "name": x.name, "description": x.description, "enabled": x.enabled,
                            "schedule": x.schedule, "next_run": a.next_run, "running": flow.is_running(x.id),
                            "last_run": a.last_run.map(|r| json!({ "status": r.status, "finished_at": r.finished_at, "error": r.error })),
                        })
                    })
                    .collect(),
            ))
        }

        "automations.get" => {
            let a = flow.get(int(p, "id")?).await.map_err(core)?;
            Ok(json!({
                "id": a.id, "name": a.name, "description": a.description, "enabled": a.enabled,
                "schedule": a.schedule, "allow_system": a.allow_system, "code": a.lua_code,
                "next_run": flow.next_run(a.id).await, "running": flow.is_running(a.id),
            }))
        }

        "automations.run" => {
            let id = int(p, "id")?;
            // Optional form values from the phone become ctx.<name> in the script.
            let mut details = HashMap::new();
            if let Some(input) = p.get("input").and_then(Value::as_object) {
                for (k, v) in input.iter().take(20) {
                    let key = k.chars().filter(|c| c.is_ascii_alphanumeric() || *c == '_').take(32).collect::<String>();
                    let value = v.as_str().map(str::to_string).unwrap_or_else(|| v.to_string());
                    if !key.is_empty() && value.len() <= 4000 {
                        details.insert(key, value);
                    }
                }
            }
            let flow2 = flow.clone();
            // Don't make the phone wait for long scripts: it gets run.started/run.finished events.
            tokio::spawn(async move {
                let _ = flow2.run_with_details(id, "phone", None, details).await;
            });
            Ok(json!({ "started": true }))
        }

        "runs.stop" => {
            if let Some(run) = p.get("run_id").and_then(Value::as_i64) {
                Ok(json!({ "stopped": flow.stop_run(run) as usize }))
            } else {
                Ok(json!({ "stopped": flow.stop_automation(int(p, "id")?) }))
            }
        }

        "automations.set_enabled" => {
            let enabled = p.get("enabled").and_then(Value::as_bool).ok_or_else(|| fail("bad_request", "missing `enabled`"))?;
            let a = flow.set_enabled(int(p, "id")?, enabled).await.map_err(core)?;
            Ok(json!({ "enabled": a.enabled }))
        }

        "automations.set_schedule" => {
            let id = int(p, "id")?;
            let schedule = match p.get("schedule") {
                Some(Value::String(s)) if s.len() <= 100 => Some(s.clone()).filter(|s| !s.trim().is_empty()),
                Some(Value::Null) | None => None,
                _ => return Err(fail("bad_request", "`schedule` must be text or null")),
            };
            let a = flow.get(id).await.map_err(core)?;
            let input = AutomationInput {
                name: a.name,
                description: a.description,
                lua_code: a.lua_code,
                schedule,
                enabled: a.enabled,
                run_on_startup: a.run_on_startup,
                watch_path: a.watch_path,
                watch_pattern: a.watch_pattern,
                allow_system: a.allow_system,
                triggers: crate::triggers::ExtraTriggers::from_json(a.triggers.as_deref()),
            };
            let saved = flow.update(id, &input).await.map_err(|e| fail("invalid", e.to_string()))?;
            Ok(json!({ "schedule": saved.schedule, "next_run": flow.next_run(id).await }))
        }

        "runs.list" => {
            let limit = p.get("limit").and_then(Value::as_i64).unwrap_or(20).clamp(1, 100);
            let runs = flow.runs(int(p, "id")?, limit).await.map_err(core)?;
            Ok(json!(runs.into_iter().map(|r| json!({
                "id": r.id, "status": r.status, "started_at": r.started_at, "finished_at": r.finished_at,
                "duration_ms": r.duration_ms(), "error": r.error,
            })).collect::<Vec<_>>()))
        }

        "runs.get" => {
            let id = int(p, "id")?;
            let runs = flow.runs(int(p, "automation_id")?, 200).await.map_err(core)?;
            let r = runs.into_iter().find(|r| r.id == id).ok_or_else(|| fail("not_found", "run not found"))?;
            let output = r.output.as_deref().map(|o| o.chars().take(20_000).collect::<String>());
            Ok(json!({ "id": r.id, "status": r.status, "started_at": r.started_at, "finished_at": r.finished_at, "error": r.error, "output": output }))
        }

        "runs.log" => {
            let limit = p.get("limit").and_then(Value::as_i64).unwrap_or(100).clamp(1, 500);
            let logs = flow.logs(int(p, "id")?, limit).await.map_err(core)?;
            Ok(json!(logs.into_iter().map(|l| json!({ "level": l.level, "message": l.message.chars().take(2000).collect::<String>(), "at": l.created_at })).collect::<Vec<_>>()))
        }

        "system.lock" => lua(flow, "system.lock()".into()).await.map(|_| json!({})),
        "system.sleep" => lua(flow, "system.sleep()".into()).await.map(|_| json!({})),
        "system.volume" => {
            let level = int(p, "level")?.clamp(0, 100);
            lua(flow, format!("system.set_volume({level})")).await.map(|_| json!({ "level": level }))
        }
        "system.mute" => {
            let on = p.get("on").and_then(Value::as_bool).unwrap_or(true);
            lua(flow, format!("system.set_mute({on})")).await.map(|_| json!({ "on": on }))
        }
        "wol.wake" => {
            let mac = text(p, "mac", 32)?;
            crate::lua::control::magic_packet(mac).map_err(|e| fail("bad_request", e))?;
            lua(flow, format!("network.wake_on_lan({})", lua_string(mac))).await.map(|_| json!({}))
        }

        "clipboard.get" => {
            let lines = lua(flow, "log(clipboard.get() or '')".into()).await?;
            Ok(json!({ "text": lines.join("\n").chars().take(100_000).collect::<String>() }))
        }
        "clipboard.set" => {
            let t = text(p, "text", 100_000)?;
            lua(flow, format!("clipboard.set({})", lua_string(t))).await.map(|_| json!({}))
        }

        "share.open_url" => {
            let url = text(p, "url", 4000)?.trim();
            if !(url.starts_with("https://") || url.starts_with("http://")) || url.chars().any(char::is_whitespace) {
                return Err(fail("bad_request", "only http(s) links can be opened"));
            }
            lua(flow, format!("app.open({})", lua_string(url))).await.map(|_| json!({}))
        }
        "share.text" => {
            let t = text(p, "text", 100_000)?;
            // Shared text lands in the clipboard, and the user sees a notice.
            lua(flow, format!("clipboard.set({})", lua_string(t))).await?;
            flow.notify_desktop("Text from your phone is in the clipboard");
            Ok(json!({}))
        }

        "screen.capture" => capture(flow).await,

        "voice.command" => {
            let said = text(p, "text", 300)?.trim().to_lowercase();
            let wanted = ["run ", "start ", "запусти ", "starte "].iter().find_map(|w| said.strip_prefix(w)).unwrap_or(&said);
            let list = flow.list().await.map_err(core)?;
            let names: Vec<&str> = list.iter().map(|a| a.automation.name.as_str()).collect();
            match pick_automation(&names, wanted) {
                Pick::One(index) => {
                    let (id, name) = (list[index].automation.id, list[index].automation.name.clone());
                    let flow2 = flow.clone();
                    tokio::spawn(async move {
                        let _ = flow2.run_with_details(id, "phone voice", None, HashMap::new()).await;
                    });
                    Ok(json!({ "started": name }))
                }
                Pick::Several(found) => Ok(json!({ "ambiguous": found })),
                Pick::None => Err(fail("not_found", format!("No automation called “{wanted}”"))),
            }
        }

        "confirm.answer" => Err(fail("not_supported", "Answering questions from the phone isn't available yet")),

        _ => Err(fail("unknown_method", format!("unknown method `{m}`"))),
    }
}

/// A smaller JPEG of the screen (fits in one relay frame).
async fn capture(flow: &LocalFlow) -> Out {
    let path = std::env::temp_dir().join(format!("lf-phone-{}.png", crate::phone::crypto::b64(&crate::phone::crypto::random::<6>())));
    let shown = path.to_string_lossy().replace('\\', "/");
    lua(flow, format!("screen.capture({})", lua_string(&shown))).await?;
    let result = (|| -> Result<Vec<u8>, String> {
        let img = image::open(&path).map_err(|e| e.to_string())?;
        let img = img.resize(1280, 1280, image::imageops::FilterType::Triangle);
        let mut quality = 70u8;
        loop {
            let mut out = Vec::new();
            let enc = image::codecs::jpeg::JpegEncoder::new_with_quality(&mut out, quality);
            img.write_with_encoder(enc).map_err(|e| e.to_string())?;
            if out.len() <= 150_000 || quality <= 30 {
                return Ok(out);
            }
            quality -= 15;
        }
    })();
    let _ = std::fs::remove_file(&path); // our own temporary screenshot
    let jpeg = result.map_err(|e| fail("error", e))?;
    Ok(json!({ "jpeg": crate::phone::crypto::b64(&jpeg) }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_method_needs_the_right_permission() {
        assert_eq!(required("pc.info"), None);
        assert_eq!(required("automations.run"), Some(Permission::Run));
        assert_eq!(required("system.sleep"), Some(Permission::Power));
        assert_eq!(required("screen.capture"), Some(Permission::Screen));
        assert_eq!(required("clipboard.get"), Some(Permission::Clipboard));
        assert!(!Permission::defaults().contains(&Permission::Power));
        assert!(!Permission::defaults().contains(&Permission::Screen));
        assert!(!Permission::defaults().contains(&Permission::Clipboard));
    }
}
