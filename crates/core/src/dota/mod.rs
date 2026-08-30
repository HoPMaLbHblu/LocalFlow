//! Dota 2 companion: launch assistant, draft assistant, build assistant.
//!
//! Nothing here reads game memory, injects code or automates gameplay. The game is
//! observed only through (a) Valve's official Game State Integration, which Dota posts
//! to a local address, and (b) screenshots the player asks for with a hotkey.
//! Screenshots stay on this PC.
//!
//! Module ownership (one owner per file):
//! - `mod.rs`        shared types, config, data folder (lead)
//! - `data.rs`       statistics source, caching, freshness (research)
//! - `traits.rs`     curated hero traits used by heuristics (research)
//! - `recommend.rs`  hero suggestions and item plans (research)
//! - `vision.rs`     screen layouts, hero portraits, recognition (vision)
//! - `draft.rs`      draft state: merging captures, corrections, saving (vision)
//! - `launch.rs`     Game State Integration and the once-per-launch browser tab (live agent)
//! - `live.rs`       live match helper: next item, gold, timing reminders (live agent)
//! - `review.rs`     post-game review from OpenDota (review agent)
//! - `lookup.rs`     look up any hero: matchups and common items (lookup agent)

pub mod data;
pub mod draft;
pub mod launch;
pub mod live;
pub mod lookup;
pub mod recommend;
pub mod review;
pub mod traits;
pub mod vision;

use std::{
    path::PathBuf,
    sync::RwLock,
};

use serde::{Deserialize, Serialize};

// ---- data folder --------------------------------------------------------------------------

static DATA_DIR: RwLock<Option<PathBuf>> = RwLock::new(None);

/// Where the companion keeps its cache, portraits, draft state and settings.
/// The desktop app sets this to `<app data>/dota`. Tests set `LOCALFLOW_DOTA_DIR`.
pub fn set_data_dir(dir: PathBuf) {
    *DATA_DIR.write().unwrap() = Some(dir);
}

pub fn data_dir() -> PathBuf {
    if let Some(dir) = std::env::var_os("LOCALFLOW_DOTA_DIR") {
        return PathBuf::from(dir);
    }
    DATA_DIR
        .read()
        .unwrap()
        .clone()
        .unwrap_or_else(|| std::env::temp_dir().join("localflow-dota"))
}

// ---- heroes and items -----------------------------------------------------------------------

/// A hero, as the statistics source describes it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Hero {
    /// The game's numeric hero id (the same in every source).
    pub id: u32,
    /// "npc_dota_hero_antimage"
    pub name: String,
    /// "Anti-Mage"
    pub localized_name: String,
    /// "str", "agi", "int" or "all"
    pub primary_attr: String,
    /// "Melee" or "Ranged"
    pub attack_type: String,
    /// "Carry", "Nuker", "Disabler", ... as the source lists them.
    pub roles: Vec<String>,
}

impl Hero {
    /// "antimage": the name without the "npc_dota_hero_" prefix (used for portrait files).
    pub fn short_name(&self) -> &str {
        self.name.strip_prefix("npc_dota_hero_").unwrap_or(&self.name)
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ItemInfo {
    pub id: u32,
    /// "black_king_bar"
    pub key: String,
    /// "Black King Bar"
    pub name: String,
    pub cost: u32,
}

/// Where a piece of advice comes from. The UI shows these differently.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Evidence {
    /// Backed by statistics: which source, what was measured, how fresh.
    Sourced {
        source: String,
        /// e.g. "52.3% win rate over 1,204 games against Axe"
        detail: String,
        /// Unix seconds when the data was fetched.
        fetched_at: i64,
    },
    /// A rule of thumb from game knowledge (hero traits), not measured.
    Heuristic { rule: String },
}

// ---- draft state --------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Side {
    Allies,
    Enemies,
}

/// Radiant is shown on the left of the top bar, Dire on the right.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Team {
    Radiant,
    Dire,
}

/// Position 1-5 in the usual Dota sense.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Role {
    Carry,
    Mid,
    Offlane,
    SoftSupport,
    HardSupport,
}

impl Role {
    pub fn parse(text: &str) -> Option<Role> {
        match text.trim().to_lowercase().replace(['-', ' '], "_").as_str() {
            "1" | "carry" | "pos1" | "safe_lane" | "safelane" => Some(Role::Carry),
            "2" | "mid" | "pos2" | "middle" => Some(Role::Mid),
            "3" | "offlane" | "pos3" | "off_lane" => Some(Role::Offlane),
            "4" | "soft_support" | "pos4" | "roamer" => Some(Role::SoftSupport),
            "5" | "hard_support" | "pos5" | "support" => Some(Role::HardSupport),
            _ => None,
        }
    }
}

/// Where a slot's hero came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PickSource {
    Screenshot,
    /// The player chose it (a correction). Never overwritten by a screenshot.
    Manual,
    /// Game State Integration reported the player's own hero.
    Gsi,
}

/// One of the ten hero slots.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct Slot {
    pub hero_id: Option<u32>,
    /// 0.0 - 1.0. 1.0 for manual and GSI picks.
    pub confidence: f32,
    pub source: Option<PickSource>,
    /// Next-best guesses with their confidence, best first (for corrections).
    pub alternatives: Vec<(u32, f32)>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct DraftState {
    pub allies: [Slot; 5],
    pub enemies: [Slot; 5],
    /// The player's own hero, once known (GSI or manual).
    pub player_hero: Option<u32>,
    pub role: Option<Role>,
    /// The player's team (from GSI or chosen by the player). Decides which screen side
    /// is "allies". Unknown = Radiant is assumed, and the UI says so.
    pub player_team: Option<Team>,
    /// How many screenshots have been merged in.
    pub captures: u32,
    /// Unix seconds.
    pub updated_at: i64,
}

/// What one screenshot showed.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct Recognition {
    /// One guess per slot that showed a hero. Empty slots are left out.
    pub picks: Vec<RecognizedPick>,
    /// Which layout was used, e.g. "16:9 top bar".
    pub layout: String,
    pub width: u32,
    pub height: u32,
    /// "unsupported aspect ratio 21:9, results may be wrong", ...
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RecognizedPick {
    /// Radiant = left half of the top bar, Dire = right half.
    pub team: Team,
    /// 0-4, left to right within that team's half.
    pub slot: u8,
    pub hero_id: u32,
    pub confidence: f32,
    pub alternatives: Vec<(u32, f32)>,
}

/// Below this, a recognised hero is shown as "please check".
pub const UNCERTAIN: f32 = 0.6;

// ---- recommendations ------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Reason {
    pub text: String,
    pub evidence: Evidence,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct HeroSuggestion {
    pub hero_id: u32,
    pub hero: String,
    /// Relative score for ordering only; not a win probability.
    pub score: f32,
    pub reasons: Vec<Reason>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ItemAdvice {
    pub item: String,
    pub key: String,
    /// 1 = buy first.
    pub priority: u8,
    pub why: String,
    pub evidence: Evidence,
    /// Other items that do the same job.
    pub alternatives: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ItemPlan {
    pub hero_id: u32,
    pub hero: String,
    pub starting: Vec<ItemAdvice>,
    pub core: Vec<ItemAdvice>,
    pub situational: Vec<ItemAdvice>,
    /// Threats in the enemy draft and how the plan adapts ("Lots of magic damage: ...").
    pub adaptations: Vec<Reason>,
    /// "OpenDota, fetched 2 h ago", or why statistics are missing.
    pub data_note: String,
}

// ---- settings --------------------------------------------------------------------------------

/// The player's companion settings, saved as `settings.json` in the data folder.
/// Credentials are never stored here (see `data.rs` for optional API keys in the keyring).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct DotaSettings {
    /// Opened when Dota reaches the main menu. Empty = don't open anything.
    pub launch_url: String,
    pub role: Option<Role>,
    /// Port for Game State Integration (127.0.0.1 only).
    pub gsi_port: u16,
    /// The player's Dota account id (Steam32), for the post-game review. `None` = not set.
    pub account_id: Option<u64>,
    /// Live match helper notifications (next item, timings). Needs Game State Integration.
    pub live_helper: bool,
}

impl Default for DotaSettings {
    fn default() -> Self {
        DotaSettings {
            launch_url: "https://www.dotabuff.com/heroes/meta".into(),
            role: None,
            gsi_port: 3417,
            account_id: None,
            live_helper: false,
        }
    }
}

impl DotaSettings {
    pub fn load() -> DotaSettings {
        std::fs::read_to_string(data_dir().join("settings.json"))
            .ok()
            .and_then(|t| serde_json::from_str(&t).ok())
            .unwrap_or_default()
    }

    pub fn save(&self) -> Result<(), String> {
        let dir = data_dir();
        std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
        let text = serde_json::to_string_pretty(self).map_err(|e| e.to_string())?;
        write_atomic(&dir.join("settings.json"), text.as_bytes())
    }
}

/// Write via a temporary file and rename, so a crash never leaves half a file.
pub fn write_atomic(path: &std::path::Path, bytes: &[u8]) -> Result<(), String> {
    let tmp = path.with_extension("tmp");
    std::fs::write(&tmp, bytes).map_err(|e| e.to_string())?;
    std::fs::rename(&tmp, path).map_err(|e| e.to_string())
}

pub fn now() -> i64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs() as i64).unwrap_or(0)
}
