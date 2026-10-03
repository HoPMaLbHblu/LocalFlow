//! Settings › Phone remote: control LocalFlow from the LocalFlow Remote app on a phone.
//! Off until the user turns it on. The PC's key and relay token live in Windows Credential
//! Manager or the macOS Keychain; the list of paired phones (no secrets) is phone_remote.json.

use std::collections::BTreeSet;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use localflow_core::phone::crypto::{b64, unb64_32};
use localflow_core::phone::handler::{PcInfo, Permission};
use localflow_core::phone::link::{Link, LinkEvent, PcSecrets, PhoneConfig};
use localflow_core::CoreEvent;
use serde::Serialize;
use serde_json::json;
use tauri::{AppHandle, Emitter, Manager, State};

use crate::{commands::CommandError, AppState};

const KEYRING_SERVICE: &str = "LocalFlow";
const KEY_USER: &str = "Phone remote key";
const TOKEN_USER: &str = "Phone remote relay token";
pub const EVENT: &str = "localflow://phone";

pub struct PhoneRemote {
    dir: PathBuf,
    link: Mutex<Option<Link>>,
}

fn error(message: impl std::fmt::Display) -> CommandError {
    CommandError::Error { message: message.to_string() }
}

fn entry(user: &str) -> Result<keyring::Entry, String> {
    keyring::Entry::new(KEYRING_SERVICE, user).map_err(|e| e.to_string())
}

/// The PC's secrets, created on first use.
fn secrets() -> Result<PcSecrets, String> {
    let key = entry(KEY_USER)?;
    let token = entry(TOKEN_USER)?;
    if let (Ok(k), Ok(t)) = (key.get_password(), token.get_password()) {
        if let Some(secret) = unb64_32(&k) {
            return Ok(PcSecrets { static_secret: secret, relay_token: t });
        }
    }
    let fresh = PcSecrets::generate();
    key.set_password(&b64(&fresh.static_secret)).map_err(|e| e.to_string())?;
    token.set_password(&fresh.relay_token).map_err(|e| e.to_string())?;
    Ok(fresh)
}

fn pc_name() -> String {
    std::env::var("COMPUTERNAME").or_else(|_| std::env::var("HOSTNAME")).unwrap_or_else(|_| "My PC".into())
}

impl PhoneRemote {
    pub fn new(dir: PathBuf) -> Self {
        PhoneRemote { dir, link: Mutex::new(None) }
    }

    fn start(&self, app: &AppHandle) -> Result<(), String> {
        let mut link = self.link.lock().unwrap();
        if link.is_some() {
            return Ok(());
        }
        let flow = app.state::<AppState>().flow.clone();
        let handle = app.clone();
        *link = Some(Link::start(
            flow,
            self.dir.clone(),
            secrets()?,
            PcInfo { name: pc_name(), version: env!("CARGO_PKG_VERSION").into() },
            Arc::new(move |event| on_link_event(&handle, event)),
        ));
        Ok(())
    }

    fn stop(&self) {
        if let Some(link) = self.link.lock().unwrap().take() {
            link.stop();
        }
    }

    fn link(&self) -> Option<Link> {
        self.link.lock().unwrap().clone()
    }
}

fn on_link_event(app: &AppHandle, event: LinkEvent) {
    if let LinkEvent::PairRequest { name, .. } = &event {
        // The user must answer on the PC: bring the window up and say why.
        crate::show_main_window(app);
        crate::notify(app, "LocalFlow", &format!("“{name}” wants to control this PC. Allow it in Settings → Phone remote."));
    }
    let _ = app.emit(EVENT, &event);
}

/// Start the link at launch if the user turned it on.
pub fn load(app: &AppHandle) {
    let remote = app.state::<PhoneRemote>();
    if PhoneConfig::load(&remote.dir).enabled {
        if let Err(e) = remote.start(app) {
            tracing::warn!("phone remote: {e}");
        }
    }
}

/// Pass run results and notifications on to connected phones.
pub fn forward(app: &AppHandle, event: &CoreEvent) {
    let Some(link) = app.state::<PhoneRemote>().link() else { return };
    match event {
        CoreEvent::RunStarted { automation_id, run_id, name, trigger } => {
            link.broadcast("run.started", json!({ "automation_id": automation_id, "run_id": run_id, "name": name, "trigger": trigger }));
        }
        CoreEvent::RunFinished { automation_id, name, trigger, run } => {
            link.broadcast(
                "run.finished",
                json!({ "automation_id": automation_id, "run_id": run.id, "name": name, "trigger": trigger, "status": run.status, "error": run.error }),
            );
        }
        CoreEvent::Log { level, message, automation_id: Some(id), .. } if level == "notify" => {
            link.broadcast("notify", json!({ "automation_id": id, "message": message }));
        }
        _ => {}
    }
}

#[derive(Serialize)]
pub struct PhoneView {
    device: String,
    name: String,
    permissions: BTreeSet<Permission>,
    added_at: i64,
    connected: bool,
}

#[derive(Serialize)]
pub struct PhoneStatus {
    enabled: bool,
    online: bool,
    relay: String,
    phones: Vec<PhoneView>,
    /// Phones waiting for "Allow?": (device, name).
    pending: Vec<(String, String)>,
}

#[tauri::command]
pub fn phone_status(remote: State<'_, PhoneRemote>) -> PhoneStatus {
    let config = PhoneConfig::load(&remote.dir);
    let state = remote.link().map(|l| l.state.lock().unwrap().clone()).unwrap_or_default();
    PhoneStatus {
        enabled: config.enabled,
        online: state.online,
        relay: config.relay,
        phones: config
            .phones
            .into_iter()
            .map(|p| PhoneView { connected: state.connected.contains(&p.device), device: p.device, name: p.name, permissions: p.permissions, added_at: p.added_at })
            .collect(),
        pending: state.pending.into_iter().collect(),
    }
}

#[tauri::command]
pub fn phone_set_enabled(app: AppHandle, remote: State<'_, PhoneRemote>, enabled: bool) -> Result<(), CommandError> {
    let mut config = PhoneConfig::load(&remote.dir);
    config.enabled = enabled;
    config.save(&remote.dir).map_err(error)?;
    if enabled {
        remote.start(&app).map_err(|e| error(format!("Could not use the system's password store: {e}")))?;
    } else {
        remote.stop();
    }
    let _ = app.emit(EVENT, &LinkEvent::PhonesChanged);
    Ok(())
}

/// Open pairing for 5 minutes; returns the text to show as a QR code.
#[tauri::command]
pub async fn phone_pair(remote: State<'_, PhoneRemote>) -> Result<String, CommandError> {
    let link = remote.link().ok_or_else(|| error("Turn on Phone remote first"))?;
    if !link.state.lock().unwrap().online {
        return Err(error("LocalFlow can't reach the relay right now. Check the internet connection."));
    }
    link.start_pairing().await.ok_or_else(|| error("Could not start pairing"))
}

#[tauri::command]
pub fn phone_answer(remote: State<'_, PhoneRemote>, device: String, allow: bool) -> Result<(), CommandError> {
    let link = remote.link().ok_or_else(|| error("Phone remote is off"))?;
    if allow {
        link.allow(&device);
    } else {
        link.deny(&device);
    }
    Ok(())
}

#[tauri::command]
pub fn phone_revoke(remote: State<'_, PhoneRemote>, device: String) -> Result<(), CommandError> {
    match remote.link() {
        Some(link) => link.revoke(&device),
        None => {
            // Off: just forget it locally; the relay drops it when the link is next on.
            let mut config = PhoneConfig::load(&remote.dir);
            config.phones.retain(|p| p.device != device);
            config.save(&remote.dir).map_err(error)?;
        }
    }
    Ok(())
}

#[tauri::command]
pub fn phone_set_permissions(remote: State<'_, PhoneRemote>, device: String, permissions: Vec<Permission>) -> Result<(), CommandError> {
    let permissions: BTreeSet<Permission> = permissions.into_iter().collect();
    match remote.link() {
        Some(link) => link.set_permissions(&device, permissions),
        None => {
            let mut config = PhoneConfig::load(&remote.dir);
            if let Some(p) = config.phones.iter_mut().find(|p| p.device == device) {
                p.permissions = permissions;
            }
            config.save(&remote.dir).map_err(error)?;
        }
    }
    Ok(())
}
