//! Game State Integration and the once-per-launch browser tab.
//!
//! **Game State Integration (GSI)** is Valve's official way for Dota 2 to report its state
//! to other programs: a small `.cfg` file in `<dota>/game/dota/cfg/gamestate_integration/`
//! tells the game to POST JSON to an address. Ours points at `http://127.0.0.1:<port>/`,
//! so nothing leaves this PC. Since March 2022 the game only does this when it is started
//! with the `-gamestateintegration` launch option (Steam › Dota 2 › Properties).
//!
//! Every POST carries the token from the `.cfg` file; posts with another token are refused.
//! The game reports the whole state each time (not only changes), so the last post is the
//! current state. In the main menu there is no `map` section and `player.activity` is
//! `"menu"`; in a match `map.game_state` is one of the `DOTA_GAMERULES_STATE_*` names.
//!
//! **Once per launch**: a launch of the game is identified by the `dota2` process id and
//! its start time. The first time the menu is reached in a launch, the page from the
//! settings opens in the default browser, once. The last launch that opened a page is
//! saved, so restarting LocalFlow during the same game doesn't open it again.

use std::{
    io::{Read, Write},
    net::{Ipv4Addr, SocketAddr, TcpListener, TcpStream},
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex, RwLock,
    },
    thread::JoinHandle,
    time::{Duration, Instant},
};

use serde::{Deserialize, Serialize};

use super::{data_dir, now, write_atomic, DotaSettings, DraftState, PickSource, Team};
use crate::{CoreEvent, EventHandler};

/// The file name the game looks for (`gamestate_integration_*.cfg`).
pub const CFG_NAME: &str = "gamestate_integration_localflow.cfg";

/// Without any Game State Integration data, assume the menu is up this long after the game
/// started. Longer when Game State Integration is set up, because then its data should come.
pub const FALLBACK_MENU_SECS: i64 = 60;
pub const FALLBACK_MENU_SECS_WITH_GSI: i64 = 180;
/// Without Game State Integration data: the menu is assumed this long after the game's
/// window first appears (loading to the menu usually takes about this long).
pub const WINDOW_MENU_SECS: i64 = 25;

/// Largest request body accepted (the game's posts with our sections are a few kilobytes).
const MAX_BODY: usize = 2 * 1024 * 1024;

// ---- events for the desktop app ----------------------------------------------------------------

static EVENTS: RwLock<Option<EventHandler>> = RwLock::new(None);

/// Where companion events (`DotaChanged`, `ShowDota`) go. Set by `LocalFlow::start`.
pub fn set_events(handler: Option<EventHandler>) {
    *EVENTS.write().unwrap_or_else(|e| e.into_inner()) = handler;
}

/// Send an event to the desktop app. False when no app is listening (e.g. the web server).
pub fn emit(event: CoreEvent) -> bool {
    let handler = EVENTS.read().unwrap_or_else(|e| e.into_inner()).clone();
    match handler {
        Some(handler) => {
            handler(event);
            true
        }
        None => false,
    }
}

// ---- finding the game ------------------------------------------------------------------------

/// Where Steam is installed (it may have more game libraries elsewhere).
pub fn steam_dirs() -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    #[cfg(windows)]
    {
        if let Some(path) = registry_string(r"Software\Valve\Steam", "SteamPath") {
            dirs.push(PathBuf::from(path));
        }
        for base in ["ProgramFiles(x86)", "ProgramFiles"] {
            if let Some(dir) = std::env::var_os(base) {
                dirs.push(PathBuf::from(dir).join("Steam"));
            }
        }
    }
    if let Some(home) = dirs::home_dir() {
        if cfg!(target_os = "macos") {
            dirs.push(home.join("Library/Application Support/Steam"));
        } else if !cfg!(windows) {
            dirs.push(home.join(".steam/steam"));
            dirs.push(home.join(".local/share/Steam"));
        }
    }
    dirs.dedup();
    dirs
}

#[cfg(windows)]
fn registry_string(key: &str, name: &str) -> Option<String> {
    use windows_sys::Win32::{
        Foundation::ERROR_SUCCESS,
        System::Registry::{RegGetValueW, HKEY, HKEY_CURRENT_USER, RRF_RT_REG_SZ},
    };
    let wide = |t: &str| t.encode_utf16().chain(std::iter::once(0)).collect::<Vec<u16>>();
    let (key, name) = (wide(key), wide(name));
    let mut buffer = vec![0u16; 2048];
    let mut size = (buffer.len() * 2) as u32;
    // SAFETY: NUL-terminated strings; `buffer` has `size` bytes of room.
    let status = unsafe {
        RegGetValueW(HKEY_CURRENT_USER as HKEY, key.as_ptr(), name.as_ptr(), RRF_RT_REG_SZ, std::ptr::null_mut(), buffer.as_mut_ptr().cast(), &mut size)
    };
    (status == ERROR_SUCCESS).then(|| String::from_utf16_lossy(&buffer[..(size as usize / 2).saturating_sub(1)]))
}

/// The library folders listed in Steam's `steamapps/libraryfolders.vdf`.
pub fn parse_library_folders(vdf: &str) -> Vec<PathBuf> {
    let mut out = Vec::new();
    for line in vdf.lines() {
        let parts = quoted_parts(line);
        if parts.len() == 2 && parts[0].eq_ignore_ascii_case("path") {
            out.push(PathBuf::from(parts[1].replace("\\\\", "\\")));
        }
    }
    out
}

/// `"key"   "value"` → ["key", "value"] (backslash escapes kept as written).
fn quoted_parts(line: &str) -> Vec<String> {
    let mut parts = Vec::new();
    let mut chars = line.chars();
    while let Some(c) = chars.next() {
        if c != '"' {
            continue;
        }
        let mut part = String::new();
        let mut escaped = false;
        for c in chars.by_ref() {
            if escaped {
                part.push(c);
                escaped = false;
            } else if c == '\\' {
                part.push(c);
                escaped = true;
            } else if c == '"' {
                break;
            } else {
                part.push(c);
            }
        }
        parts.push(part);
    }
    parts
}

/// A Dota 2 install folder has `game/dota` inside.
fn is_dota_dir(dir: &Path) -> bool {
    dir.join("game").join("dota").is_dir()
}

/// The Dota 2 folder (`.../steamapps/common/dota 2 beta`), if it can be found.
/// `LOCALFLOW_DOTA_GAME_DIR` overrides the search (used by tests).
pub fn find_dota_dir() -> Option<PathBuf> {
    if let Some(dir) = std::env::var_os("LOCALFLOW_DOTA_GAME_DIR") {
        let dir = PathBuf::from(dir);
        return is_dota_dir(&dir).then_some(dir);
    }
    for steam in steam_dirs() {
        let mut libraries = vec![steam.clone()];
        if let Ok(text) = std::fs::read_to_string(steam.join("steamapps").join("libraryfolders.vdf")) {
            libraries.extend(parse_library_folders(&text));
        }
        for library in libraries {
            let dir = library.join("steamapps").join("common").join("dota 2 beta");
            if is_dota_dir(&dir) {
                return Some(dir);
            }
        }
    }
    None
}

/// `<dota>/game/dota/cfg/gamestate_integration`
pub fn cfg_dir(dota_dir: &Path) -> PathBuf {
    dota_dir.join("game").join("dota").join("cfg").join("gamestate_integration")
}

pub fn cfg_path(dota_dir: &Path) -> PathBuf {
    cfg_dir(dota_dir).join(CFG_NAME)
}

// ---- the .cfg file ------------------------------------------------------------------------------

/// The secret the game sends with every post, created once and kept in the data folder.
pub fn token() -> String {
    static LOCK: Mutex<()> = Mutex::new(());
    let _guard = LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let file = data_dir().join("gsi_token.txt");
    if let Ok(text) = std::fs::read_to_string(&file) {
        let text = text.trim();
        if text.len() >= 16 && text.chars().all(|c| c.is_ascii_alphanumeric()) {
            return text.to_string();
        }
    }
    let mut bytes = [0u8; 16];
    if getrandom::getrandom(&mut bytes).is_err() {
        // Extremely unlikely; still unpredictable enough for a local-only check.
        let seed = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_nanos();
        bytes.copy_from_slice(&seed.to_le_bytes());
    }
    let token: String = bytes.iter().map(|b| format!("{b:02x}")).collect();
    let _ = std::fs::create_dir_all(data_dir());
    if let Err(e) = write_atomic(&file, token.as_bytes()) {
        tracing::warn!("could not save the Game State Integration token: {e}");
    }
    token
}

/// The text of the `.cfg` file with this port and token.
///
/// Sections (all only about the player's own hero, never other players):
/// - `provider`, `auth`: who sends and the token;
/// - `map`: `game_state`, `matchid`, `clock_time` (game clock, negative before the horn);
/// - `player`: `activity`, `team_name`, `gold` (reliable + unreliable);
/// - `hero`: `id`, `name`, `alive`;
/// - `items`: `slot0`-`slot8` (6-8 are the backpack), `stash0`-`stash5`, `teleport0`,
///   `neutral0`, each `{ "name": "item_black_king_bar" | "empty", ... }`. Used by the live helper.
///
/// Field names as documented by the Dota2GSI (C#) and dota-gsi (Rust) libraries.
pub fn gsi_config_text_with(port: u16, token: &str) -> String {
    format!(
        r#""LocalFlow Dota 2 companion"
{{
    "uri"           "http://127.0.0.1:{port}/"
    "timeout"       "5.0"
    "buffer"        "0.1"
    "throttle"      "0.5"
    "heartbeat"     "30.0"
    "data"
    {{
        "auth"      "1"
        "provider"  "1"
        "map"       "1"
        "player"    "1"
        "hero"      "1"
        "items"     "1"
    }}
    "auth"
    {{
        "token"     "{token}"
    }}
}}
"#
    )
}

/// The text of the `.cfg` file, for the user to copy if LocalFlow can't write it.
pub fn gsi_config_text(port: u16) -> String {
    gsi_config_text_with(port, &token())
}

pub fn gsi_installed_in(dota_dir: &Path) -> bool {
    cfg_path(dota_dir).is_file()
}

/// True when our `.cfg` file is in the game's folder.
pub fn gsi_installed() -> bool {
    find_dota_dir().is_some_and(|d| gsi_installed_in(&d))
}

/// Write our `.cfg` file (and nothing else) into this Dota folder.
pub fn install_gsi_into(dota_dir: &Path, port: u16) -> Result<PathBuf, String> {
    if port == 0 {
        return Err("choose a port between 1024 and 65535".into());
    }
    let dir = cfg_dir(dota_dir);
    let path = dir.join(CFG_NAME);
    let text = gsi_config_text(port);
    if std::fs::read_to_string(&path).is_ok_and(|old| old == text) {
        // Already up to date: installing again changes nothing.
        return Ok(path);
    }
    let write = || -> std::io::Result<()> {
        std::fs::create_dir_all(&dir)?;
        std::fs::write(&path, text.as_bytes())
    };
    write().map_err(|e| {
        format!(
            "Could not write {} ({e}). Create that file yourself and paste the text shown below into it.",
            path.display()
        )
    })?;
    Ok(path)
}

/// The first line of every `.cfg` file LocalFlow writes. Files without it are not ours.
const CFG_HEADER: &str = "\"LocalFlow Dota 2 companion\"";

/// Whether an existing `.cfg` text was written by LocalFlow.
pub fn is_our_cfg(text: &str) -> bool {
    text.trim_start().starts_with(CFG_HEADER)
}

/// Bring an installed `.cfg` file up to date (e.g. files from older versions without the
/// `items` section). Only rewrites our own file, only when it is already there and differs.
/// True when it was rewritten: the game reads the file when it starts, so Dota needs a restart.
pub fn upgrade_gsi_in(dota_dir: &Path, port: u16) -> Result<bool, String> {
    let path = cfg_path(dota_dir);
    let Ok(old) = std::fs::read_to_string(&path) else { return Ok(false) };
    if !is_our_cfg(&old) || port == 0 {
        return Ok(false);
    }
    if old == gsi_config_text(port) {
        return Ok(false);
    }
    install_gsi_into(dota_dir, port)?;
    Ok(true)
}

/// [`upgrade_gsi_in`] for the installed game, with the port from the settings.
pub fn upgrade_gsi() -> Result<bool, String> {
    match find_dota_dir() {
        Some(dota) => upgrade_gsi_in(&dota, DotaSettings::load().gsi_port),
        None => Ok(false),
    }
}

/// Find the game and write our `.cfg` file into it. The game reads it when it starts.
pub fn install_gsi(port: u16) -> Result<PathBuf, String> {
    let dota = find_dota_dir().ok_or(
        "Dota 2 wasn't found. Is it installed through Steam? You can also create the file yourself: \
         <Steam library>/steamapps/common/dota 2 beta/game/dota/cfg/gamestate_integration/gamestate_integration_localflow.cfg",
    )?;
    install_gsi_into(&dota, port)
}

/// Remove our `.cfg` file. False if it wasn't there.
pub fn uninstall_gsi_from(dota_dir: &Path) -> Result<bool, String> {
    let path = cfg_path(dota_dir);
    if !path.is_file() {
        return Ok(false);
    }
    std::fs::remove_file(&path).map_err(|e| format!("Could not remove {} ({e}). Delete it yourself.", path.display()))?;
    Ok(true)
}

pub fn uninstall_gsi() -> Result<bool, String> {
    match find_dota_dir() {
        Some(dota) => uninstall_gsi_from(&dota),
        None => Ok(false),
    }
}

// ---- game state -----------------------------------------------------------------------------

/// Where the player is, as far as Game State Integration tells.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Phase {
    /// No data yet.
    #[default]
    Unknown,
    /// Main menu (no match).
    Menu,
    /// Loading the map or waiting for players.
    Loading,
    HeroSelection,
    StrategyTime,
    /// Heroes are in the map, before the horn.
    PreGame,
    Playing,
    PostGame,
}

impl Phase {
    /// From `map.game_state` (`DOTA_GAMERULES_STATE_*`).
    pub fn from_game_state(state: &str) -> Phase {
        match state.trim_start_matches("DOTA_GAMERULES_STATE_") {
            "HERO_SELECTION" => Phase::HeroSelection,
            "STRATEGY_TIME" | "TEAM_SHOWCASE" => Phase::StrategyTime,
            "PRE_GAME" => Phase::PreGame,
            "GAME_IN_PROGRESS" => Phase::Playing,
            "POST_GAME" | "DISCONNECT" => Phase::PostGame,
            "INIT" | "WAIT_FOR_PLAYERS_TO_LOAD" | "WAIT_FOR_MAP_TO_LOAD" | "CUSTOM_GAME_SETUP" => Phase::Loading,
            _ => Phase::Unknown,
        }
    }
}

/// The latest state Game State Integration reported.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct GameState {
    pub phase: Phase,
    /// `map.game_state` as sent, e.g. "DOTA_GAMERULES_STATE_HERO_SELECTION".
    pub game_state: Option<String>,
    pub team: Option<Team>,
    /// The player's own hero, once picked.
    pub hero_id: Option<u32>,
    /// "npc_dota_hero_axe"
    pub hero_name: Option<String>,
    pub match_id: Option<String>,
    /// Unix seconds of the last accepted post.
    pub last_update: Option<i64>,
    /// `map.clock_time`: the game clock in seconds (negative before the horn).
    pub clock: Option<i64>,
    /// `player.gold` (reliable + unreliable).
    pub gold: Option<u32>,
    /// `hero.alive`.
    pub alive: Option<bool>,
    /// Item keys without the `item_` prefix from `items.slot0`-`slot8` (inventory and
    /// backpack) and `items.stash0`-`stash5`, in that order. Empty slots are left out.
    #[serde(default)]
    pub items: Vec<String>,
}

/// Why a post was refused.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GsiError {
    NotJson,
    WrongToken,
}

/// Read one post from the game. The token must match.
pub fn parse_payload(body: &[u8], token: &str) -> Result<GameState, GsiError> {
    let json: serde_json::Value = serde_json::from_slice(body).map_err(|_| GsiError::NotJson)?;
    if !json.is_object() {
        return Err(GsiError::NotJson);
    }
    let sent = json.pointer("/auth/token").and_then(|t| t.as_str()).unwrap_or("");
    if token.is_empty() || !constant_time_eq(sent.as_bytes(), token.as_bytes()) {
        return Err(GsiError::WrongToken);
    }
    Ok(state_from_json(&json))
}

fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    a.len() == b.len() && a.iter().zip(b).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
}

/// The state described by one (already checked) post.
pub fn state_from_json(json: &serde_json::Value) -> GameState {
    let map = json.get("map").filter(|m| m.is_object());
    let player = json.get("player").filter(|p| p.is_object());
    let hero = json.get("hero").filter(|h| h.is_object());
    let text = |v: Option<&serde_json::Value>, key: &str| {
        v.and_then(|v| v.get(key)).and_then(|s| s.as_str()).map(str::to_string).filter(|s| !s.is_empty())
    };

    let game_state = text(map, "game_state");
    let activity = text(player, "activity");
    let phase = if activity.as_deref() == Some("menu") {
        Phase::Menu
    } else if let Some(state) = &game_state {
        Phase::from_game_state(state)
    } else if map.is_none() && (player.is_some() || json.get("provider").is_some()) {
        // The menu: the game is running but there is no match.
        Phase::Menu
    } else {
        Phase::Unknown
    };
    let team = match text(player, "team_name").map(|t| t.to_lowercase()).as_deref() {
        Some("radiant") => Some(Team::Radiant),
        Some("dire") => Some(Team::Dire),
        _ => None,
    };
    let hero_id = hero.and_then(|h| h.get("id")).and_then(|v| v.as_u64()).filter(|id| *id > 0).map(|id| id as u32);
    let hero_name = text(hero, "name").filter(|n| n.starts_with("npc_dota_hero_"));
    let match_id = map
        .and_then(|m| m.get("matchid"))
        .and_then(|v| v.as_str().map(str::to_string).or_else(|| v.as_u64().map(|n| n.to_string())))
        .filter(|m| !m.is_empty() && m != "0");
    let in_match = phase != Phase::Menu;
    let clock = map.and_then(|m| m.get("clock_time")).and_then(json_int);
    let gold = player
        .and_then(|p| p.get("gold").and_then(json_int).or_else(|| {
            // Older posts: only the two parts.
            let reliable = p.get("gold_reliable").and_then(json_int)?;
            let unreliable = p.get("gold_unreliable").and_then(json_int)?;
            Some(reliable + unreliable)
        }))
        .map(|g| g.clamp(0, u32::MAX as i64) as u32);
    let alive = hero.and_then(|h| h.get("alive")).and_then(|v| v.as_bool());
    let items = json.get("items").map(item_keys).unwrap_or_default();
    GameState {
        phase,
        game_state,
        team,
        hero_id: if in_match { hero_id } else { None },
        hero_name: if in_match { hero_name } else { None },
        match_id,
        last_update: Some(now()),
        clock: if in_match { clock } else { None },
        gold: if in_match { gold } else { None },
        alive: if in_match { alive } else { None },
        items: if in_match { items } else { Vec::new() },
    }
}

/// A whole number sent as an integer or a float.
fn json_int(value: &serde_json::Value) -> Option<i64> {
    value.as_i64().or_else(|| value.as_f64().filter(|f| f.is_finite()).map(|f| f.floor() as i64))
}

/// The player's items from the `items` section: inventory and backpack (`slot0`-`slot8`),
/// then the stash (`stash0`-`stash5`). `"empty"` slots, the teleport and neutral slots
/// are left out. `"item_black_king_bar"` becomes `"black_king_bar"`.
pub fn item_keys(items: &serde_json::Value) -> Vec<String> {
    let Some(items) = items.as_object() else { return Vec::new() };
    let mut slots: Vec<(u8, u32, String)> = items
        .iter()
        .filter_map(|(slot, item)| {
            let (group, number) = if let Some(n) = slot.strip_prefix("slot") {
                (0, n)
            } else if let Some(n) = slot.strip_prefix("stash") {
                (1, n)
            } else {
                return None;
            };
            let number: u32 = number.parse().ok()?;
            let name = item.get("name")?.as_str()?;
            let key = name.strip_prefix("item_")?;
            (!key.is_empty()).then(|| (group, number, key.to_string()))
        })
        .collect();
    slots.sort();
    slots.into_iter().map(|(_, _, key)| key).collect()
}

// ---- the local listener -----------------------------------------------------------------------

/// Called after every accepted post with the previous and the new state.
pub type OnUpdate = Arc<dyn Fn(&GameState, &GameState) + Send + Sync>;

/// A small HTTP listener on 127.0.0.1 that accepts the game's posts.
pub struct GsiListener {
    port: u16,
    state: Arc<RwLock<GameState>>,
    stop: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
}

impl GsiListener {
    /// Listen on 127.0.0.1:`port` (0 = any free port) for posts with this token.
    pub fn start(port: u16, token: String, on_update: Option<OnUpdate>) -> Result<GsiListener, String> {
        let listener = TcpListener::bind(SocketAddr::from((Ipv4Addr::LOCALHOST, port))).map_err(|e| {
            if e.kind() == std::io::ErrorKind::AddrInUse {
                format!("port {port} is already used by another program; choose another port in Settings › Dota 2 and install the file again")
            } else {
                format!("could not listen on port {port}: {e}")
            }
        })?;
        let port = listener.local_addr().map_err(|e| e.to_string())?.port();
        listener.set_nonblocking(true).map_err(|e| e.to_string())?;
        let state = Arc::new(RwLock::new(GameState::default()));
        let stop = Arc::new(AtomicBool::new(false));
        let (s, st) = (state.clone(), stop.clone());
        let thread = std::thread::Builder::new()
            .name("dota-gsi".into())
            .spawn(move || accept_loop(listener, token, s, st, on_update))
            .map_err(|e| e.to_string())?;
        Ok(GsiListener { port, state, stop, thread: Some(thread) })
    }

    pub fn port(&self) -> u16 {
        self.port
    }

    pub fn state(&self) -> GameState {
        self.state.read().unwrap_or_else(|e| e.into_inner()).clone()
    }

    pub fn stop(mut self) {
        self.shutdown();
    }

    fn shutdown(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

impl Drop for GsiListener {
    fn drop(&mut self) {
        self.shutdown();
    }
}

fn accept_loop(listener: TcpListener, token: String, state: Arc<RwLock<GameState>>, stop: Arc<AtomicBool>, on_update: Option<OnUpdate>) {
    while !stop.load(Ordering::SeqCst) {
        match listener.accept() {
            Ok((stream, _)) => {
                if let Some(new) = handle_connection(stream, &token) {
                    let previous = std::mem::replace(&mut *state.write().unwrap_or_else(|e| e.into_inner()), new.clone());
                    if let Some(on_update) = &on_update {
                        on_update(&previous, &new);
                    }
                }
            }
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => std::thread::sleep(Duration::from_millis(50)),
            Err(e) => {
                tracing::debug!("Game State Integration: {e}");
                std::thread::sleep(Duration::from_millis(200));
            }
        }
    }
}

/// Answer one request. Returns the new state for an accepted post.
fn handle_connection(mut stream: TcpStream, token: &str) -> Option<GameState> {
    let _ = stream.set_nonblocking(false);
    let _ = stream.set_read_timeout(Some(Duration::from_secs(5)));
    let _ = stream.set_write_timeout(Some(Duration::from_secs(5)));
    let (status, result) = match read_request(&mut stream) {
        Ok((method, body)) if method == "POST" => match parse_payload(&body, token) {
            Ok(state) => ("200 OK", Some(state)),
            Err(GsiError::WrongToken) => ("401 Unauthorized", None),
            Err(GsiError::NotJson) => ("400 Bad Request", None),
        },
        Ok(_) => ("405 Method Not Allowed", None),
        Err(status) => (status, None),
    };
    let _ = stream.write_all(format!("HTTP/1.1 {status}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n").as_bytes());
    let _ = stream.flush();
    result
}

/// The method and body of one HTTP request.
fn read_request(stream: &mut TcpStream) -> Result<(String, Vec<u8>), &'static str> {
    let mut buffer = Vec::with_capacity(8192);
    let mut chunk = [0u8; 8192];
    let header_end = loop {
        if let Some(end) = find(&buffer, b"\r\n\r\n") {
            break end;
        }
        if buffer.len() > 64 * 1024 {
            return Err("431 Request Header Fields Too Large");
        }
        let n = stream.read(&mut chunk).map_err(|_| "408 Request Timeout")?;
        if n == 0 {
            return Err("400 Bad Request");
        }
        buffer.extend_from_slice(&chunk[..n]);
    };
    let head = String::from_utf8_lossy(&buffer[..header_end]).into_owned();
    let method = head.split_whitespace().next().unwrap_or("").to_string();
    let length = head
        .lines()
        .skip(1)
        .filter_map(|l| l.split_once(':'))
        .find(|(k, _)| k.trim().eq_ignore_ascii_case("content-length"))
        .and_then(|(_, v)| v.trim().parse::<usize>().ok())
        .unwrap_or(0);
    if length > MAX_BODY {
        return Err("413 Payload Too Large");
    }
    let mut body = buffer[header_end + 4..].to_vec();
    while body.len() < length {
        let n = stream.read(&mut chunk).map_err(|_| "408 Request Timeout")?;
        if n == 0 {
            break;
        }
        body.extend_from_slice(&chunk[..n]);
    }
    body.truncate(length);
    Ok((method, body))
}

fn find(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack.windows(needle.len()).position(|w| w == needle)
}

// ---- the app's listener -------------------------------------------------------------------------

static LISTENER: Mutex<Option<GsiListener>> = Mutex::new(None);
static LISTEN_ERROR: RwLock<Option<String>> = RwLock::new(None);

/// Start (or restart) the app's listener on this port.
pub fn start_listening(port: u16) -> Result<u16, String> {
    let mut slot = LISTENER.lock().unwrap_or_else(|e| e.into_inner());
    if let Some(old) = slot.take() {
        old.stop();
    }
    let on_update: OnUpdate = Arc::new(|previous, new| on_game_update(previous, new));
    match GsiListener::start(port, token(), Some(on_update)) {
        Ok(listener) => {
            let port = listener.port();
            *slot = Some(listener);
            *LISTEN_ERROR.write().unwrap_or_else(|e| e.into_inner()) = None;
            tracing::info!("Dota 2 Game State Integration: listening on 127.0.0.1:{port}");
            Ok(port)
        }
        Err(e) => {
            tracing::warn!("Dota 2 Game State Integration: {e}");
            *LISTEN_ERROR.write().unwrap_or_else(|e| e.into_inner()) = Some(e.clone());
            Err(e)
        }
    }
}

pub fn stop_listening() {
    if let Some(listener) = LISTENER.lock().unwrap_or_else(|e| e.into_inner()).take() {
        listener.stop();
    }
}

/// The port the app's listener is on, if it runs.
pub fn listening_port() -> Option<u16> {
    LISTENER.lock().unwrap_or_else(|e| e.into_inner()).as_ref().map(|l| l.port())
}

/// Why the listener couldn't start, if it couldn't.
pub fn listen_error() -> Option<String> {
    LISTEN_ERROR.read().unwrap_or_else(|e| e.into_inner()).clone()
}

/// The latest state from the game (default when nothing arrived yet).
pub fn game_state() -> GameState {
    LISTENER.lock().unwrap_or_else(|e| e.into_inner()).as_ref().map(|l| l.state()).unwrap_or_default()
}

/// Start the listener if the file is installed and it isn't running; stop it if the file is gone.
pub fn ensure_listening() {
    if gsi_installed() {
        let wanted = DotaSettings::load().gsi_port;
        if listening_port() != Some(wanted) {
            let _ = start_listening(wanted);
        }
    } else if listening_port().is_some() {
        stop_listening();
    }
}

fn on_game_update(previous: &GameState, new: &GameState) {
    let mut memory = Memory::load();
    let new_match = new.match_id.is_some() && new.match_id != memory.last_match;
    if new_match {
        memory.last_match = new.match_id.clone();
        if let Err(e) = memory.save() {
            tracing::warn!("could not save the companion state: {e}");
        }
    }
    let mut draft = DraftState::load();
    let changed = apply_to_draft(&mut draft, new, new_match);
    if changed {
        draft.updated_at = now();
        if let Err(e) = draft.save() {
            tracing::warn!("could not save the draft: {e}");
        }
    }
    if changed || previous.phase != new.phase || previous.team != new.team || previous.hero_id != new.hero_id {
        emit(CoreEvent::DotaChanged);
    }
}

/// Set the player's team. Allies are always the player's team, so the two halves swap
/// when the team turns out to be the other one than assumed (Radiant).
pub fn set_player_team(draft: &mut DraftState, team: Team) -> bool {
    // One place decides how a team change moves the slots: DraftState::set_player_team.
    let changed = draft.player_team != Some(team);
    if changed {
        draft.set_player_team(Some(team));
    }
    changed
}

/// Feed what the game reported into the draft. A new match starts a fresh draft (the role
/// is kept). Returns whether anything changed.
pub fn apply_to_draft(draft: &mut DraftState, state: &GameState, new_match: bool) -> bool {
    let mut changed = false;
    if new_match && *draft != DraftState::default() {
        let role = draft.role;
        draft.reset();
        draft.role = role;
        changed = true;
    }
    if let Some(team) = state.team {
        changed |= set_player_team(draft, team);
    }
    if let Some(id) = state.hero_id {
        if draft.player_hero != Some(id) {
            draft.set_player_hero(Some(id), PickSource::Gsi);
            changed = true;
        }
    }
    changed
}

// ---- once per launch ---------------------------------------------------------------------------

/// What the companion remembers between LocalFlow restarts (`launch.json`).
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
struct Memory {
    /// The launch that already got its page opened.
    last_opened_launch: Option<String>,
    /// The last match seen, so a new one starts a fresh draft.
    last_match: Option<String>,
    /// "Open my page when Dota starts" is switched on (the background assistant).
    assistant: bool,
}

impl Memory {
    fn load() -> Memory {
        std::fs::read_to_string(data_dir().join("launch.json"))
            .ok()
            .and_then(|t| serde_json::from_str(&t).ok())
            .unwrap_or_default()
    }

    fn save(&self) -> Result<(), String> {
        let dir = data_dir();
        std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
        let text = serde_json::to_string_pretty(self).map_err(|e| e.to_string())?;
        write_atomic(&dir.join("launch.json"), text.as_bytes())
    }
}

/// Whether LocalFlow itself opens the page when Dota starts (without an automation).
pub fn launch_assistant_enabled() -> bool {
    Memory::load().assistant
}

pub fn set_launch_assistant(enabled: bool) -> Result<(), String> {
    let mut memory = Memory::load();
    memory.assistant = enabled;
    memory.save()
}

/// A running `dota2` process.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct DotaProcess {
    pub pid: u32,
    /// Unix seconds.
    pub started: u64,
}

impl DotaProcess {
    /// Identifies one launch of the game.
    pub fn launch_id(&self) -> String {
        format!("{}-{}", self.pid, self.started)
    }
}

/// The running game, if any (the oldest `dota2` process).
pub fn dota_process() -> Option<DotaProcess> {
    let mut system = sysinfo::System::new();
    system.refresh_processes(sysinfo::ProcessesToUpdate::All, true);
    system
        .processes()
        .iter()
        .filter(|(_, p)| {
            let name = p.name().to_string_lossy().to_lowercase();
            name == "dota2" || name == "dota2.exe"
        })
        .map(|(pid, p)| DotaProcess { pid: pid.as_u32(), started: p.start_time() })
        .min_by_key(|p| p.started)
}

/// Whether the game has reached its main menu in this launch.
/// With Game State Integration data from this launch, that decides. Without any, the menu is
/// assumed `fallback_secs` after the game started.
pub fn menu_reached(process: Option<&DotaProcess>, state: &GameState, now: i64, fallback_secs: i64) -> bool {
    let Some(process) = process else { return false };
    let started = process.started as i64;
    if state.last_update.is_some_and(|t| t + 5 >= started) {
        return state.phase == Phase::Menu;
    }
    now - started >= fallback_secs
}

/// When the current launch's game window was first seen: (launch id, unix seconds).
static WINDOW_SEEN: std::sync::Mutex<Option<(String, i64)>> = std::sync::Mutex::new(None);

/// Whether the Dota window of this launch has been visible for `WINDOW_MENU_SECS`.
/// Only a fallback: when the game sends Game State Integration data, that decides.
pub fn window_ready(process: &DotaProcess, state: &GameState, now: i64) -> bool {
    let gsi_this_launch = state.last_update.is_some_and(|t| t + 5 >= process.started as i64);
    if gsi_this_launch {
        return false;
    }
    let id = process.launch_id();
    let mut seen = WINDOW_SEEN.lock().unwrap_or_else(|e| e.into_inner());
    if seen.as_ref().map(|(l, _)| l != &id).unwrap_or(true) {
        *seen = None;
        let has_window = crate::lua::control::find_window("dota2").is_some_and(|w| !w.minimized || !w.title.is_empty());
        if has_window {
            *seen = Some((id, now));
        }
        return false;
    }
    seen.as_ref().is_some_and(|(_, first)| now - first >= WINDOW_MENU_SECS)
}

fn fallback_secs() -> i64 {
    if listening_port().is_some() {
        FALLBACK_MENU_SECS_WITH_GSI
    } else {
        FALLBACK_MENU_SECS
    }
}

/// True when Dota is running and in its main menu (see [`menu_reached`]).
pub fn in_menu() -> bool {
    let process = dota_process();
    let state = game_state();
    menu_reached(process.as_ref(), &state, now(), fallback_secs()) || process.as_ref().is_some_and(|p| window_ready(p, &state, now()))
}

/// What happened when asked to open the launch page.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum OpenOutcome {
    Opened,
    /// Already opened for this launch of the game.
    AlreadyOpened,
    /// No page is set (Settings › Dota 2).
    NoUrl,
    NotRunning,
}

impl OpenOutcome {
    pub fn explain(self) -> &'static str {
        match self {
            OpenOutcome::Opened => "opened",
            OpenOutcome::AlreadyOpened => "already opened for this launch of Dota 2",
            OpenOutcome::NoUrl => "no page is set in Settings › Dota 2",
            OpenOutcome::NotRunning => "Dota 2 isn't running",
        }
    }
}

static OPEN_LOCK: Mutex<()> = Mutex::new(());

/// Open `url` with `opener` unless it was already opened for `launch_id`.
/// Only http(s) addresses are opened.
pub fn open_once_with(launch_id: &str, url: &str, opener: &dyn Fn(&str) -> Result<(), String>) -> Result<OpenOutcome, String> {
    let url = url.trim();
    if url.is_empty() {
        return Ok(OpenOutcome::NoUrl);
    }
    let lower = url.to_lowercase();
    if !(lower.starts_with("https://") || lower.starts_with("http://")) {
        return Err(format!("the launch page must be a web address starting with https://, not \"{url}\""));
    }
    let _guard = OPEN_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let mut memory = Memory::load();
    if memory.last_opened_launch.as_deref() == Some(launch_id) {
        return Ok(OpenOutcome::AlreadyOpened);
    }
    opener(url)?;
    memory.last_opened_launch = Some(launch_id.to_string());
    memory.save()?;
    Ok(OpenOutcome::Opened)
}

fn open_in_browser(url: &str) -> Result<(), String> {
    crate::lua::system::shell_open(url).map_err(|e| format!("could not open the browser: {e}"))
}

/// Open the page from the settings once for the current launch of the game.
pub fn open_launch_url() -> Result<OpenOutcome, String> {
    let url = DotaSettings::load().launch_url;
    if url.trim().is_empty() {
        return Ok(OpenOutcome::NoUrl);
    }
    let Some(process) = dota_process() else { return Ok(OpenOutcome::NotRunning) };
    open_once_with(&process.launch_id(), &url, &open_in_browser)
}

// ---- background service ------------------------------------------------------------------------

/// Runs until LocalFlow closes: keeps the listener going while the file is installed (and up
/// to date), opens the launch page when the assistant is on, and runs the live match helper
/// when it is switched on (see `live.rs`). Does nothing for players without Dota,
/// and nothing at all unless the desktop app called `set_data_dir`.
pub async fn serve(events: Option<EventHandler>) {
    set_events(events);
    // Only in the desktop app, which gives the companion its data folder. The web server and
    // tests never listen on the port or open pages.
    let configured = super::DATA_DIR.read().map(|d| d.is_some()).unwrap_or(false);
    if !configured {
        return;
    }
    let mut last_check: Option<Instant> = None;
    let mut warned_port: Option<String> = None;
    loop {
        let check_listener = last_check.map_or(true, |t| t.elapsed() >= Duration::from_secs(30));
        if check_listener {
            last_check = Some(Instant::now());
        }
        let result = tokio::task::spawn_blocking(move || tick(check_listener)).await;
        if let Ok(Some(error)) = result {
            // Tell the user once per problem, not every 30 seconds.
            if warned_port.as_deref() != Some(error.as_str()) {
                emit(CoreEvent::Notice { message: format!("Dota 2 companion: {error}") });
                warned_port = Some(error);
            }
        }
        tokio::time::sleep(Duration::from_secs(3)).await;
    }
}

/// One round of the background service. Returns a listener error worth telling the user.
fn tick(check_listener: bool) -> Option<String> {
    let mut problem = None;
    if check_listener {
        // Files from older versions lack what the live helper needs: rewrite ours (only ours).
        match upgrade_gsi() {
            Ok(true) => {
                tracing::info!("Dota 2 companion: updated the Game State Integration file");
                emit(CoreEvent::Notice {
                    message: "Dota 2 companion: the Game State Integration file was updated for the live match helper.                               If Dota 2 is running, restart it so it reads the new file."
                        .into(),
                });
            }
            Ok(false) => {}
            Err(e) => tracing::warn!("Dota 2 companion: could not update the Game State Integration file: {e}"),
        }
        ensure_listening();
        if gsi_installed() && listening_port().is_none() {
            problem = listen_error();
        }
    }
    if launch_assistant_enabled() {
        if let Some(process) = dota_process() {
            let state = game_state();
            if menu_reached(Some(&process), &state, now(), fallback_secs()) || window_ready(&process, &state, now()) {
                match open_once_with(&process.launch_id(), &DotaSettings::load().launch_url, &open_in_browser) {
                    Ok(OpenOutcome::Opened) => tracing::info!("Dota 2 companion: opened the launch page"),
                    Ok(_) => {}
                    Err(e) => tracing::warn!("Dota 2 companion: {e}"),
                }
            }
        }
    }
    super::live::background_tick(&game_state(), now());
    problem
}
