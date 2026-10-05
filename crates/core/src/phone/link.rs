//! The connection to the LocalFlow Remote relay: pairing, per-phone encrypted sessions,
//! requests and events. The desktop app owns the secrets (keyring) and shows the UI; this
//! module does the networking and keeps the list of paired phones (no secrets in it).

use std::collections::{BTreeSet, HashMap};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use futures_util::{SinkExt, StreamExt};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tokio::sync::mpsc;
use tokio_tungstenite::tungstenite::{client::IntoClientRequest, Message};

use super::crypto::{b64, random, sha256_hex, unb64, unb64_32, KeyPair, Session, Side};
use super::handler::{handle, Permission, PcInfo, Request};
use crate::LocalFlow;

pub const DEFAULT_RELAY: &str = "wss://localflow-relay.vfvf5389127.workers.dev";
const PAIRING_SECONDS: u64 = 300;

/// A phone that was paired with this PC. Stored in `phone_remote.json` (no secrets).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct PairedPhone {
    pub device: String,
    pub name: String,
    /// The phone's static X25519 public key (base64url).
    pub key: String,
    pub permissions: BTreeSet<Permission>,
    pub added_at: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct PhoneConfig {
    pub enabled: bool,
    pub relay: String,
    pub pc_id: String,
    pub phones: Vec<PairedPhone>,
}

impl PhoneConfig {
    fn path(dir: &std::path::Path) -> PathBuf {
        dir.join("phone_remote.json")
    }

    pub fn load(dir: &std::path::Path) -> Self {
        let mut c: PhoneConfig = std::fs::read(Self::path(dir)).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default();
        if c.relay.is_empty() {
            c.relay = DEFAULT_RELAY.into();
        }
        if c.pc_id.is_empty() {
            c.pc_id = b64(&random::<16>());
        }
        c
    }

    pub fn save(&self, dir: &std::path::Path) -> Result<(), String> {
        crate::appdata::write_safely(&Self::path(dir), &serde_json::to_vec_pretty(self).map_err(|e| e.to_string())?)
    }
}

/// Secrets the desktop keeps in the OS keyring.
#[derive(Clone)]
pub struct PcSecrets {
    pub static_secret: [u8; 32],
    pub relay_token: String,
}

impl PcSecrets {
    pub fn generate() -> Self {
        PcSecrets { static_secret: random(), relay_token: b64(&random::<32>()) }
    }
}

/// What the desktop should show.
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum LinkEvent {
    /// Connected to (or lost) the relay.
    Online { online: bool },
    /// A phone scanned the QR code and wants to be paired: ask the user.
    PairRequest { device: String, name: String },
    /// The list of paired phones or their state changed.
    PhonesChanged,
}

enum Command {
    StartPairing(tokio::sync::oneshot::Sender<String>),
    Allow { device: String },
    Deny { device: String },
    Revoke { device: String },
    SetPermissions { device: String, permissions: BTreeSet<Permission> },
    Broadcast(Value),
    Stop,
}

/// A handle the desktop keeps to control the running link.
#[derive(Clone)]
pub struct Link {
    tx: mpsc::UnboundedSender<Command>,
    pub state: Arc<Mutex<LinkState>>,
}

#[derive(Debug, Default, Clone, Serialize)]
pub struct LinkState {
    pub online: bool,
    pub connected: BTreeSet<String>,
    /// Phones waiting for "Allow?" on the PC: device → name.
    pub pending: HashMap<String, String>,
}

impl Link {
    /// Start connecting (and keep reconnecting) in the background.
    pub fn start(flow: LocalFlow, dir: PathBuf, secrets: PcSecrets, info: PcInfo, events: Arc<dyn Fn(LinkEvent) + Send + Sync>) -> Link {
        let (tx, rx) = mpsc::unbounded_channel();
        let state = Arc::new(Mutex::new(LinkState::default()));
        let link = Link { tx, state: state.clone() };
        tokio::spawn(run(flow, dir, secrets, info, events, rx, state));
        link
    }

    /// Open pairing for 5 minutes; returns the text for the QR code.
    pub async fn start_pairing(&self) -> Option<String> {
        let (tx, rx) = tokio::sync::oneshot::channel();
        self.tx.send(Command::StartPairing(tx)).ok()?;
        rx.await.ok()
    }
    pub fn allow(&self, device: &str) {
        let _ = self.tx.send(Command::Allow { device: device.into() });
    }
    pub fn deny(&self, device: &str) {
        let _ = self.tx.send(Command::Deny { device: device.into() });
    }
    pub fn revoke(&self, device: &str) {
        let _ = self.tx.send(Command::Revoke { device: device.into() });
    }
    pub fn set_permissions(&self, device: &str, permissions: BTreeSet<Permission>) {
        let _ = self.tx.send(Command::SetPermissions { device: device.into(), permissions });
    }
    /// Send an event (`{"ev":..,"d":..}`) to every connected, allowed phone.
    pub fn broadcast(&self, event: &str, data: Value) {
        let _ = self.tx.send(Command::Broadcast(json!({ "ev": event, "d": data })));
    }
    pub fn stop(&self) {
        let _ = self.tx.send(Command::Stop);
    }
}

/// A phone's state during one connection.
struct Peer {
    session: Option<Session>,
}

/// A phone that scanned the QR code and is waiting for the user's decision.
struct Candidate {
    key: [u8; 32],
    name: String,
}

#[allow(clippy::too_many_arguments)]
async fn run(
    flow: LocalFlow,
    dir: PathBuf,
    secrets: PcSecrets,
    info: PcInfo,
    events: Arc<dyn Fn(LinkEvent) + Send + Sync>,
    mut rx: mpsc::UnboundedReceiver<Command>,
    state: Arc<Mutex<LinkState>>,
) {
    let me = KeyPair::from_secret(secrets.static_secret);
    let mut delay = Duration::from_secs(1);
    loop {
        let config = PhoneConfig::load(&dir);
        let url = format!("{}/v1/pc/{}/host", config.relay.trim_end_matches('/'), config.pc_id);
        let mut request = match url.as_str().into_client_request() {
            Ok(r) => r,
            Err(e) => {
                tracing::warn!("phone remote: bad relay address: {e}");
                return;
            }
        };
        request.headers_mut().insert("authorization", format!("Bearer {}", secrets.relay_token).parse().unwrap());

        let stream = tokio::select! {
            r = tokio_tungstenite::connect_async(request) => r,
            cmd = rx.recv() => match cmd { Some(Command::Stop) | None => return, _ => continue },
        };
        let (mut ws, _) = match stream {
            Ok(s) => s,
            Err(e) => {
                tracing::info!("phone remote: relay not reachable ({e}); retrying in {delay:?}");
                tokio::select! {
                    _ = tokio::time::sleep(delay) => {}
                    cmd = rx.recv() => if matches!(cmd, Some(Command::Stop) | None) { return },
                }
                delay = (delay * 2).min(Duration::from_secs(60));
                continue;
            }
        };
        delay = Duration::from_secs(1);
        set_online(&state, &events, true);

        let mut peers: HashMap<String, Peer> = HashMap::new();
        let mut candidates: HashMap<String, Candidate> = HashMap::new();
        let mut ping = tokio::time::interval(Duration::from_secs(30));

        let stop = loop {
            tokio::select! {
                _ = ping.tick() => {
                    if ws.send(Message::Ping(Vec::new().into())).await.is_err() { break false; }
                }
                cmd = rx.recv() => {
                    let Some(cmd) = cmd else { break true };
                    let mut config = PhoneConfig::load(&dir);
                    match cmd {
                        Command::Stop => { let _ = ws.close(None).await; break true; }
                        Command::StartPairing(reply) => {
                            let secret = b64(&random::<32>());
                            let expires = chrono::Utc::now().timestamp() as u64 + PAIRING_SECONDS;
                            let _ = send(&mut ws, json!({ "t": "pair-open", "hash": sha256_hex(&secret), "expires": expires })).await;
                            let qr = format!(
                                "lfremote://pair?v=1&relay={}&pc={}&key={}&s={}&name={}",
                                urlencode(&config.relay), config.pc_id, b64(&me.public), secret, urlencode(&info.name)
                            );
                            let _ = reply.send(qr);
                        }
                        Command::Allow { device } => {
                            if let Some(c) = candidates.remove(&device) {
                                config.phones.retain(|p| p.device != device);
                                config.phones.push(PairedPhone { device: device.clone(), name: c.name, key: b64(&c.key), permissions: Permission::defaults(), added_at: chrono::Utc::now().timestamp() });
                                let _ = config.save(&dir);
                                let _ = send(&mut ws, json!({ "t": "device-allow", "device": device })).await;
                                let _ = send(&mut ws, json!({ "t": "pair-close" })).await;
                                // The phone is mid-handshake: tell it it's in.
                                if let Some(Peer { session: Some(s) }) = peers.get_mut(&device) {
                                    let frame = s.seal(json!({ "ev": "paired", "d": {} }).to_string().as_bytes());
                                    let _ = send(&mut ws, json!({ "t": "data", "device": device, "d": frame })).await;
                                }
                            }
                            state.lock().unwrap().pending.remove(&device);
                            events(LinkEvent::PhonesChanged);
                        }
                        Command::Deny { device } | Command::Revoke { device } => {
                            candidates.remove(&device);
                            peers.remove(&device);
                            config.phones.retain(|p| p.device != device);
                            let _ = config.save(&dir);
                            let _ = send(&mut ws, json!({ "t": "device-revoke", "device": device })).await;
                            state.lock().unwrap().pending.remove(&device);
                            events(LinkEvent::PhonesChanged);
                        }
                        Command::SetPermissions { device, permissions } => {
                            if let Some(p) = config.phones.iter_mut().find(|p| p.device == device) {
                                p.permissions = permissions;
                                let _ = config.save(&dir);
                                events(LinkEvent::PhonesChanged);
                            }
                        }
                        Command::Broadcast(event) => {
                            let text = event.to_string();
                            for phone in &config.phones {
                                if let Some(Peer { session: Some(s) }) = peers.get_mut(&phone.device) {
                                    let frame = s.seal(text.as_bytes());
                                    let _ = send(&mut ws, json!({ "t": "data", "device": phone.device, "d": frame })).await;
                                }
                            }
                        }
                    }
                }
                msg = ws.next() => {
                    let text = match msg {
                        Some(Ok(Message::Text(t))) => t.to_string(),
                        Some(Ok(Message::Close(_))) | None | Some(Err(_)) => break false,
                        Some(Ok(_)) => continue,
                    };
                    let Ok(frame) = serde_json::from_str::<Value>(&text) else { continue };
                    let device = frame.get("device").and_then(Value::as_str).unwrap_or("").to_string();
                    match frame.get("t").and_then(Value::as_str) {
                        Some("device-online") => { state.lock().unwrap().connected.insert(device.clone()); peers.insert(device, Peer { session: None }); events(LinkEvent::PhonesChanged); }
                        Some("device-offline") => { state.lock().unwrap().connected.remove(&device); peers.remove(&device); events(LinkEvent::PhonesChanged); }
                        Some("device-pending") => { peers.insert(device, Peer { session: None }); }
                        Some("data") => {
                            let d = frame.get("d").and_then(Value::as_str).unwrap_or("");
                            let config = PhoneConfig::load(&dir);
                            if let Some(reply) = on_data(&flow, &info, &config, &me, &device, d, &mut peers, &mut candidates, &state, &events).await {
                                let _ = send(&mut ws, json!({ "t": "data", "device": device, "d": reply })).await;
                            }
                        }
                        _ => {}
                    }
                }
            }
        };
        set_online(&state, &events, false);
        if stop {
            return;
        }
        tokio::time::sleep(delay).await;
    }
}

/// Handle one frame from a phone; returns the frame to send back, if any.
#[allow(clippy::too_many_arguments)]
async fn on_data(
    flow: &LocalFlow,
    info: &PcInfo,
    config: &PhoneConfig,
    me: &KeyPair,
    device: &str,
    d: &str,
    peers: &mut HashMap<String, Peer>,
    candidates: &mut HashMap<String, Candidate>,
    state: &Arc<Mutex<LinkState>>,
    events: &Arc<dyn Fn(LinkEvent) + Send + Sync>,
) -> Option<String> {
    let known = config.phones.iter().find(|p| p.device == device);
    let peer = peers.entry(device.to_string()).or_insert(Peer { session: None });

    // Handshake: {"h":1,"e":<ephemeral>,"s":<static>,"n":<phone name>} in clear text.
    if let Ok(h) = serde_json::from_slice::<Value>(&unb64(d).unwrap_or_default()) {
        if h.get("h").and_then(Value::as_i64) == Some(1) {
            let their_e = unb64_32(h.get("e")?.as_str()?)?;
            let claimed = h.get("s").and_then(Value::as_str).and_then(unb64_32);
            // A paired phone must use the key it paired with; a new one presents its key once.
            let their_s = match (known, claimed) {
                (Some(p), _) => unb64_32(&p.key)?,
                (None, Some(k)) => k,
                (None, None) => return None,
            };
            let mine_e = KeyPair::generate();
            let pc_id = unb64(&config.pc_id)?;
            let dev = unb64(device)?;
            peer.session = Session::derive(Side::Pc, &me.secret, &their_s, &mine_e.secret, &their_e, &pc_id, &dev).ok();
            peer.session.as_ref()?;
            if known.is_none() {
                let name: String = h.get("n").and_then(Value::as_str).unwrap_or("Phone").chars().take(40).collect();
                candidates.insert(device.to_string(), Candidate { key: their_s, name: name.clone() });
                state.lock().unwrap().pending.insert(device.to_string(), name.clone());
                events(LinkEvent::PairRequest { device: device.to_string(), name });
            }
            return Some(b64(json!({ "h": 1, "e": b64(&mine_e.public) }).to_string().as_bytes()));
        }
    }

    // Encrypted request — only from paired, allowed phones.
    let session = peer.session.as_mut()?;
    let Ok(plain) = session.open(d) else {
        tracing::warn!("phone remote: dropped a frame that failed to decrypt");
        return None;
    };
    let phone = known?;
    let request: Request = serde_json::from_slice(&plain).ok()?;
    let response = handle(flow, &phone.permissions, info, request).await;
    Some(session.seal(response.to_string().as_bytes()))
}

async fn send<S>(ws: &mut S, msg: Value) -> Result<(), ()>
where
    S: SinkExt<Message> + Unpin,
{
    ws.send(Message::Text(msg.to_string().into())).await.map_err(|_| ())
}

fn set_online(state: &Arc<Mutex<LinkState>>, events: &Arc<dyn Fn(LinkEvent) + Send + Sync>, online: bool) {
    let mut s = state.lock().unwrap();
    if s.online != online {
        s.online = online;
        if !online {
            s.connected.clear();
        }
        drop(s);
        events(LinkEvent::Online { online });
    }
}

fn urlencode(s: &str) -> String {
    s.bytes()
        .map(|b| match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => (b as char).to_string(),
            _ => format!("%{b:02X}"),
        })
        .collect()
}
