//! Statistics source, caching and freshness. OWNER: research agent.
//!
//! The real source is the public OpenDota API (<https://api.opendota.com/api>), which works
//! without an account. What each endpoint measures (checked against OpenDota's own source):
//! - `/heroes`, `/constants/items`, `/constants/patch`: game constants (dotaconstants).
//! - `/heroes/{id}/matchups`: results of `id` against every other hero in the professional
//!   and league matches OpenDota has parsed, over the last 12 months (all patches in that window).
//! - `/heroes/{id}/itemPopularity`: purchase counts from the hero's last 100 parsed
//!   professional matches, split into start / early (<10 min) / mid / late game.
//!
//! Anonymous use is rate limited (OpenDota reports 60 requests a minute and a daily cap in its
//! `X-Rate-Limit-*` headers), so requests are spaced out, cached on disk and fetched lazily:
//! matchups only for heroes that are actually in the draft. An optional API key (see
//! [`set_api_key`]) is sent in the `Authorization` header, never in the URL, and never logged.
//!
//! Every response is cached as JSON in `data_dir()/cache` with the time it was fetched.
//! When the network fails, stale cache is served and [`SourceInfo::offline`] is set.
//!
//! [`FixtureSource`] reads the same raw JSON bodies from a folder, fully offline (for tests).

use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    sync::{Mutex, RwLock},
    time::{Duration, Instant},
};

use serde_json::Value;

use super::{Hero, ItemInfo};

/// How `hero` did against `against`: wins of `hero` out of `games`.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Matchup {
    pub against: u32,
    pub games: u32,
    pub wins: u32,
}

/// How often each item (by item id) is bought at each stage of the game.
/// Each list is sorted by count, most bought first.
#[derive(Debug, Clone, PartialEq, Default, serde::Serialize, serde::Deserialize)]
pub struct ItemPopularity {
    pub start: Vec<(u32, u32)>,
    pub early: Vec<(u32, u32)>,
    pub mid: Vec<(u32, u32)>,
    pub late: Vec<(u32, u32)>,
}

/// What the data is and how fresh it is, for the "sourced vs heuristic" labels.
#[derive(Debug, Clone, PartialEq, Default, serde::Serialize, serde::Deserialize)]
pub struct SourceInfo {
    pub name: String,
    /// Unix seconds of the oldest data in use, if any.
    pub fetched_at: Option<i64>,
    pub patch: Option<String>,
    /// True when the network failed and only cached (or no) data was used.
    pub offline: bool,
    pub note: String,
}

pub trait DotaSource: Send + Sync {
    fn heroes(&self) -> Result<Vec<Hero>, String>;
    fn items(&self) -> Result<Vec<ItemInfo>, String>;
    /// Matchups of `hero_id` against every other hero.
    fn matchups(&self, hero_id: u32) -> Result<Vec<Matchup>, String>;
    fn item_popularity(&self, hero_id: u32) -> Result<ItemPopularity, String>;
    fn info(&self) -> SourceInfo;
}

/// The real source (OpenDota over the network + cache in `data_dir()/cache`).
/// Calls block (network, and waits to respect rate limits): call from a blocking thread.
pub fn default_source() -> Box<dyn DotaSource> {
    Box::new(OpenDotaSource::new())
}

// ---- API key ----------------------------------------------------------------------------------

static API_KEY: RwLock<Option<String>> = RwLock::new(None);

/// Environment variable read when no key was set with [`set_api_key`].
pub const API_KEY_ENV: &str = "LOCALFLOW_OPENDOTA_KEY";

/// Set (or clear) the optional OpenDota API key. The desktop app keeps it in the OS keyring.
/// Without a key the free anonymous limits apply. The key is never logged or cached.
pub fn set_api_key(key: Option<String>) {
    *API_KEY.write().unwrap() = key.map(|k| k.trim().to_string()).filter(|k| !k.is_empty());
}

fn api_key() -> Option<String> {
    if let Some(key) = API_KEY.read().unwrap().clone() {
        return Some(key);
    }
    std::env::var(API_KEY_ENV).ok().map(|k| k.trim().to_string()).filter(|k| !k.is_empty())
}

// ---- parsing (shared by the network source and fixtures) -------------------------------------

/// Parse OpenDota's `/heroes` body.
pub fn parse_heroes(body: &Value) -> Result<Vec<Hero>, String> {
    let heroes: Vec<Hero> =
        serde_json::from_value(body.clone()).map_err(|e| format!("unexpected hero list format: {e}"))?;
    if heroes.is_empty() {
        return Err("the hero list is empty".into());
    }
    Ok(heroes)
}

/// Parse OpenDota's `/constants/items` body (an object keyed by item key). Recipes are left out.
pub fn parse_items(body: &Value) -> Result<Vec<ItemInfo>, String> {
    let map = body.as_object().ok_or("unexpected item list format")?;
    let mut items: Vec<ItemInfo> = map
        .iter()
        .filter(|(key, _)| !key.starts_with("recipe_"))
        .filter_map(|(key, v)| {
            Some(ItemInfo {
                id: v.get("id")?.as_u64()? as u32,
                key: key.clone(),
                name: v.get("dname")?.as_str()?.to_string(),
                cost: v.get("cost").and_then(|c| c.as_u64()).unwrap_or(0) as u32,
            })
        })
        .collect();
    if items.is_empty() {
        return Err("the item list is empty".into());
    }
    items.sort_by_key(|i| i.id);
    Ok(items)
}

/// Parse OpenDota's `/heroes/{id}/matchups` body.
pub fn parse_matchups(body: &Value) -> Result<Vec<Matchup>, String> {
    let rows = body.as_array().ok_or("unexpected matchup format")?;
    Ok(rows
        .iter()
        .filter_map(|r| {
            Some(Matchup {
                against: r.get("hero_id")?.as_u64()? as u32,
                games: r.get("games_played")?.as_u64()? as u32,
                wins: r.get("wins")?.as_u64()? as u32,
            })
        })
        .collect())
}

/// Parse OpenDota's `/heroes/{id}/itemPopularity` body.
pub fn parse_item_popularity(body: &Value) -> Result<ItemPopularity, String> {
    if !body.is_object() {
        return Err("unexpected item popularity format".into());
    }
    let stage = |name: &str| -> Vec<(u32, u32)> {
        let mut list: Vec<(u32, u32)> = body
            .get(name)
            .and_then(|v| v.as_object())
            .map(|m| {
                m.iter()
                    .filter_map(|(id, n)| Some((id.parse().ok()?, n.as_u64()? as u32)))
                    .collect()
            })
            .unwrap_or_default();
        list.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
        list
    };
    Ok(ItemPopularity {
        start: stage("start_game_items"),
        early: stage("early_game_items"),
        mid: stage("mid_game_items"),
        late: stage("late_game_items"),
    })
}

/// The newest patch name in OpenDota's `/constants/patch` body ("7.41").
pub fn parse_latest_patch(body: &Value) -> Option<String> {
    body.as_array()?
        .iter()
        .filter_map(|p| Some((p.get("id").and_then(|i| i.as_u64()).unwrap_or(0), p.get("name")?.as_str()?)))
        .max_by_key(|(id, _)| *id)
        .map(|(_, name)| name.to_string())
}

/// "3 h ago", "2 days ago".
pub fn age_text(fetched_at: i64, now: i64) -> String {
    let secs = (now - fetched_at).max(0);
    match secs {
        s if s < 90 => "just now".into(),
        s if s < 90 * 60 => format!("{} min ago", s / 60),
        s if s < 36 * 3600 => format!("{} h ago", s / 3600),
        s => format!("{} days ago", s / 86400),
    }
}

// ---- OpenDota over the network ----------------------------------------------------------------

pub const OPENDOTA_URL: &str = "https://api.opendota.com/api";
const SOURCE_NAME: &str = "OpenDota";
const DAY: i64 = 24 * 3600;
const CONSTANTS_TTL: i64 = 7 * DAY;
const STATS_TTL: i64 = DAY;
/// At most ~40 requests a minute, well under the anonymous 60.
const MIN_INTERVAL: Duration = Duration::from_millis(1500);
/// After a network failure, don't try again for this long (serve cache instead).
const RETRY_AFTER: Duration = Duration::from_secs(60);

static LAST_REQUEST: Mutex<Option<Instant>> = Mutex::new(None);

#[derive(Default)]
struct State {
    /// fetched_at of every cache entry served, by cache key.
    used: HashMap<String, i64>,
    offline: bool,
    notes: Vec<String>,
    net_down_until: Option<Instant>,
}

/// OpenDota with an on-disk cache. Use [`default_source`] in the app.
pub struct OpenDotaSource {
    base_url: String,
    cache_dir: Option<PathBuf>,
    agent: ureq::Agent,
    state: Mutex<State>,
}

impl Default for OpenDotaSource {
    fn default() -> Self {
        Self::new()
    }
}

impl OpenDotaSource {
    pub fn new() -> Self {
        Self::with_base_url(OPENDOTA_URL)
    }

    /// Use another base URL (tests point this at an unreachable address).
    pub fn with_base_url(base_url: &str) -> Self {
        let agent = ureq::AgentBuilder::new()
            .timeout_connect(Duration::from_secs(10))
            .timeout(Duration::from_secs(15))
            .user_agent(&format!("LocalFlow/{}", env!("CARGO_PKG_VERSION")))
            .build();
        OpenDotaSource {
            base_url: base_url.trim_end_matches('/').to_string(),
            cache_dir: None,
            agent,
            state: Mutex::new(State::default()),
        }
    }

    /// Keep the cache here instead of `data_dir()/cache`.
    pub fn with_cache_dir(mut self, dir: PathBuf) -> Self {
        self.cache_dir = Some(dir);
        self
    }

    fn cache_dir(&self) -> PathBuf {
        self.cache_dir.clone().unwrap_or_else(|| super::data_dir().join("cache"))
    }

    fn cache_path(&self, key: &str) -> PathBuf {
        self.cache_dir().join(format!("{key}.json"))
    }

    fn read_cache(&self, key: &str) -> Option<(Value, i64)> {
        let text = std::fs::read_to_string(self.cache_path(key)).ok()?;
        let v: Value = serde_json::from_str(&text).ok()?;
        Some((v.get("body")?.clone(), v.get("fetched_at")?.as_i64()?))
    }

    fn write_cache(&self, key: &str, body: &Value, fetched_at: i64) {
        let dir = self.cache_dir();
        if std::fs::create_dir_all(&dir).is_err() {
            return;
        }
        let entry = serde_json::json!({ "source": SOURCE_NAME, "fetched_at": fetched_at, "body": body });
        if let Err(e) = super::write_atomic(&self.cache_path(key), entry.to_string().as_bytes()) {
            tracing::warn!("dota: could not write cache {key}: {e}");
        }
    }

    fn download(&self, path: &str) -> Result<Value, String> {
        {
            let state = self.state.lock().unwrap();
            if let Some(until) = state.net_down_until {
                if Instant::now() < until {
                    return Err("the network failed a moment ago".into());
                }
            }
        }
        {
            let mut last = LAST_REQUEST.lock().unwrap();
            if let Some(t) = *last {
                let since = t.elapsed();
                if since < MIN_INTERVAL {
                    std::thread::sleep(MIN_INTERVAL - since);
                }
            }
            *last = Some(Instant::now());
        }
        let url = format!("{}{}", self.base_url, path);
        let mut request = self.agent.get(&url);
        if let Some(key) = api_key() {
            request = request.set("Authorization", &format!("Bearer {key}"));
        }
        let result = match request.call() {
            Ok(resp) => resp
                .into_string()
                .map_err(|e| format!("reading the response failed: {e}"))
                .and_then(|t| serde_json::from_str::<Value>(&t).map_err(|e| format!("the response was not JSON: {e}"))),
            Err(ureq::Error::Status(429, _)) => Err("OpenDota's rate limit was reached".into()),
            Err(ureq::Error::Status(code, _)) => Err(format!("OpenDota answered HTTP {code}")),
            Err(ureq::Error::Transport(t)) => Err(format!("network error: {}", t.kind())),
        };
        if result.is_err() {
            self.state.lock().unwrap().net_down_until = Some(Instant::now() + RETRY_AFTER);
        }
        result
    }

    /// Cached body for `key`, refreshed from `path` when older than `ttl`.
    fn fetch(&self, key: &str, path: &str, ttl: i64, what: &str) -> Result<Value, String> {
        let now = super::now();
        let cached = self.read_cache(key);
        if let Some((body, at)) = &cached {
            if now - at < ttl && *at <= now {
                self.state.lock().unwrap().used.insert(key.into(), *at);
                return Ok(body.clone());
            }
        }
        match self.download(path) {
            Ok(body) => {
                self.write_cache(key, &body, now);
                self.state.lock().unwrap().used.insert(key.into(), now);
                Ok(body)
            }
            Err(err) => {
                let mut state = self.state.lock().unwrap();
                state.offline = true;
                match cached {
                    Some((body, at)) => {
                        let note = format!("Could not refresh {what} ({err}); using the copy from {}.", age_text(at, now));
                        if !state.notes.contains(&note) {
                            state.notes.push(note);
                        }
                        state.used.insert(key.into(), at);
                        Ok(body)
                    }
                    None => {
                        let note = format!("No {what}: {err}, and nothing is cached yet.");
                        if !state.notes.contains(&note) {
                            state.notes.push(note);
                        }
                        Err(format!("could not fetch {what} from OpenDota ({err}) and there is no cached copy"))
                    }
                }
            }
        }
    }

    fn patch(&self) -> Option<String> {
        self.read_cache("patch").and_then(|(body, _)| parse_latest_patch(&body))
    }
}

impl DotaSource for OpenDotaSource {
    fn heroes(&self) -> Result<Vec<Hero>, String> {
        // The patch list is tiny and cached for a week; failure only means no patch label.
        let _ = self.fetch("patch", "/constants/patch", CONSTANTS_TTL, "the patch list");
        parse_heroes(&self.fetch("heroes", "/heroes", CONSTANTS_TTL, "the hero list")?)
    }

    fn items(&self) -> Result<Vec<ItemInfo>, String> {
        parse_items(&self.fetch("items", "/constants/items", CONSTANTS_TTL, "the item list")?)
    }

    fn matchups(&self, hero_id: u32) -> Result<Vec<Matchup>, String> {
        let key = format!("matchups_{hero_id}");
        let body = self.fetch(&key, &format!("/heroes/{hero_id}/matchups"), STATS_TTL, &format!("matchups for hero {hero_id}"))?;
        parse_matchups(&body)
    }

    fn item_popularity(&self, hero_id: u32) -> Result<ItemPopularity, String> {
        let key = format!("itemPopularity_{hero_id}");
        let body = self.fetch(
            &key,
            &format!("/heroes/{hero_id}/itemPopularity"),
            STATS_TTL,
            &format!("item popularity for hero {hero_id}"),
        )?;
        parse_item_popularity(&body)
    }

    fn info(&self) -> SourceInfo {
        let state = self.state.lock().unwrap();
        let fetched_at = state.used.values().copied().min();
        let mut note = match fetched_at {
            Some(at) => format!(
                "OpenDota (professional matches), fetched {}. Matchups cover the last 12 months; item counts the hero's last 100 parsed pro matches.",
                age_text(at, super::now())
            ),
            None => "OpenDota: nothing loaded yet.".to_string(),
        };
        for extra in &state.notes {
            note.push(' ');
            note.push_str(extra);
        }
        SourceInfo { name: SOURCE_NAME.into(), fetched_at, patch: self.patch(), offline: state.offline, note }
    }
}

// ---- fixtures ---------------------------------------------------------------------------------

/// A fully offline source for tests and demos, reading raw OpenDota response bodies from a
/// folder:
/// - `heroes.json` (`/heroes`), `items.json` (`/constants/items`), `patch.json` (optional,
///   `/constants/patch`)
/// - `matchups_<hero id>.json` (`/heroes/<id>/matchups`)
/// - `itemPopularity_<hero id>.json` (`/heroes/<id>/itemPopularity`)
/// - `source.json` (optional): `{"name": "...", "fetched_at": <unix seconds>, "note": "..."}`
///
/// A missing file is an error for that call, like a failed download with no cache.
/// The recorded set used by the tests is in `crates/core/tests/fixtures/dota/data`.
pub struct FixtureSource {
    dir: PathBuf,
    meta: Value,
}

impl FixtureSource {
    pub fn new(dir: impl AsRef<Path>) -> Self {
        let dir = dir.as_ref().to_path_buf();
        let meta = std::fs::read_to_string(dir.join("source.json"))
            .ok()
            .and_then(|t| serde_json::from_str(&t).ok())
            .unwrap_or(Value::Null);
        FixtureSource { dir, meta }
    }

    fn load(&self, file: &str) -> Result<Value, String> {
        let text = std::fs::read_to_string(self.dir.join(file)).map_err(|_| format!("no recorded data ({file})"))?;
        serde_json::from_str(&text).map_err(|e| format!("{file}: {e}"))
    }
}

impl DotaSource for FixtureSource {
    fn heroes(&self) -> Result<Vec<Hero>, String> {
        parse_heroes(&self.load("heroes.json")?)
    }
    fn items(&self) -> Result<Vec<ItemInfo>, String> {
        parse_items(&self.load("items.json")?)
    }
    fn matchups(&self, hero_id: u32) -> Result<Vec<Matchup>, String> {
        parse_matchups(&self.load(&format!("matchups_{hero_id}.json"))?)
    }
    fn item_popularity(&self, hero_id: u32) -> Result<ItemPopularity, String> {
        parse_item_popularity(&self.load(&format!("itemPopularity_{hero_id}.json"))?)
    }
    fn info(&self) -> SourceInfo {
        let text = |k: &str| self.meta.get(k).and_then(|v| v.as_str()).map(str::to_string);
        SourceInfo {
            name: text("name").unwrap_or_else(|| "fixture".into()),
            fetched_at: self.meta.get("fetched_at").and_then(|v| v.as_i64()),
            patch: self.load("patch.json").ok().and_then(|b| parse_latest_patch(&b)),
            offline: true,
            note: text("note").unwrap_or_else(|| "Recorded data, not live.".into()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_item_popularity_sorted() {
        let body = serde_json::json!({"start_game_items": {"44": 3, "16": 9}, "late_game_items": {"116": 2}});
        let p = parse_item_popularity(&body).unwrap();
        assert_eq!(p.start, vec![(16, 9), (44, 3)]);
        assert_eq!(p.late, vec![(116, 2)]);
        assert!(p.mid.is_empty());
    }

    #[test]
    fn items_skip_recipes() {
        let body = serde_json::json!({
            "black_king_bar": {"id": 116, "dname": "Black King Bar", "cost": 4050},
            "recipe_black_king_bar": {"id": 117, "dname": "Recipe", "cost": 1375}
        });
        let items = parse_items(&body).unwrap();
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].key, "black_king_bar");
    }

    #[test]
    fn latest_patch_by_id() {
        let body = serde_json::json!([{"name": "7.40", "id": 59}, {"name": "7.41", "id": 60}]);
        assert_eq!(parse_latest_patch(&body).as_deref(), Some("7.41"));
    }

    #[test]
    fn api_key_trimmed_and_clearable() {
        set_api_key(Some("  ".into()));
        assert!(API_KEY.read().unwrap().is_none());
        set_api_key(Some(" abc ".into()));
        assert_eq!(API_KEY.read().unwrap().as_deref(), Some("abc"));
        set_api_key(None);
        assert!(API_KEY.read().unwrap().is_none());
    }
}
