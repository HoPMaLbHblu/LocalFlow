//! The Dota 2 companion for scripts, and the operations the desktop app shares with them.
//!
//! ```lua
//! dota.status()               -- Game State Integration, game state, team, hero, data source
//! dota.in_menu()              -- true when Dota 2 runs and shows its main menu
//! dota.wait_for_menu(seconds) -- wait (within the script's time limit) for the menu; true if reached
//! dota.open_launch_url()      -- open the page from Settings once per launch; true if it opened
//! dota.capture_draft()        -- screenshot, recognise the heroes, update the draft
//! dota.draft()                -- the ten slots, the player's hero, team and role
//! dota.correct("enemies", 2, "Axe")  -- fix a slot (nil clears it)
//! dota.reset()                -- start a fresh draft
//! dota.suggest(5)             -- heroes that fit this draft, with reasons
//! dota.build()                -- item plan for your hero (or dota.build("Axe"))
//! dota.heroes()               -- every hero: id, name, short_name, roles
//! dota.set_role("mid")        -- your position: "carry", "mid", ... or 1-5
//! dota.show()                 -- open the companion window in the desktop app
//! ```
//!
//! Nothing here clicks, types or reads the game's memory; screenshots stay on this PC.

use std::{
    path::PathBuf,
    time::{Duration, Instant},
};

use mlua::{Lua, LuaSerdeExt, SerializeOptions, Table, Value};
use serde::Serialize;

use crate::{
    dota::{
        data::{self, DotaSource, SourceInfo},
        data_dir,
        launch::{self, GameState, OpenOutcome},
        now, recommend, vision, DotaSettings, DraftState, Hero, HeroSuggestion, ItemPlan, PickSource, Role, Side, Slot, Team,
        UNCERTAIN,
    },
    CoreEvent,
};

/// How many screenshots are kept in `captures/`.
pub const KEEP_CAPTURES: usize = 10;

// ---- shared operations (Lua and the desktop app) ---------------------------------------------

/// The statistics source (network + cache).
pub fn source() -> Box<dyn DotaSource> {
    data::default_source()
}

/// Every hero, or a friendly reason why the list isn't available.
pub fn hero_list(src: &dyn DotaSource) -> Result<Vec<Hero>, String> {
    let heroes = src
        .heroes()
        .map_err(|e| format!("the hero list isn't available ({e}). Check the internet connection and try again"))?;
    if heroes.is_empty() {
        return Err("the hero list is empty. Check the internet connection and try again".into());
    }
    Ok(heroes)
}

fn simplify(text: &str) -> String {
    text.to_lowercase().chars().filter(|c| c.is_alphanumeric()).collect()
}

/// A hero by id, name ("Anti-Mage"), short name ("antimage") or full name
/// ("npc_dota_hero_antimage"), ignoring case, spaces and punctuation. A unique beginning
/// ("anti") is enough.
pub fn resolve_hero(heroes: &[Hero], text: &str) -> Result<u32, String> {
    let text = text.trim();
    if text.is_empty() {
        return Err("no hero name given".into());
    }
    if let Ok(id) = text.parse::<u32>() {
        return heroes.iter().find(|h| h.id == id).map(|h| h.id).ok_or(format!("no hero has the id {id}"));
    }
    let wanted = simplify(text);
    let names = |h: &Hero| [simplify(&h.localized_name), simplify(h.short_name()), simplify(&h.name)];
    if let Some(hero) = heroes.iter().find(|h| names(h).contains(&wanted)) {
        return Ok(hero.id);
    }
    let starts: Vec<&Hero> = heroes.iter().filter(|h| names(h).iter().any(|n| n.starts_with(&wanted))).collect();
    if starts.len() == 1 {
        return Ok(starts[0].id);
    }
    let contains: Vec<&Hero> = heroes.iter().filter(|h| names(h).iter().any(|n| n.contains(&wanted))).collect();
    if contains.len() == 1 {
        return Ok(contains[0].id);
    }
    let candidates = if starts.len() > 1 { starts } else { contains };
    if candidates.is_empty() {
        Err(format!("no hero called \"{text}\""))
    } else {
        let names: Vec<&str> = candidates.iter().take(6).map(|h| h.localized_name.as_str()).collect();
        Err(format!("\"{text}\" could be {}; write more of the name", names.join(", ")))
    }
}

fn hero_name(heroes: &[Hero], id: u32) -> String {
    heroes.iter().find(|h| h.id == id).map(|h| h.localized_name.clone()).unwrap_or_else(|| format!("hero #{id}"))
}

#[derive(Debug, Clone, Serialize)]
pub struct Alternative {
    pub hero_id: u32,
    pub hero: String,
    pub confidence: f32,
}

#[derive(Debug, Clone, Serialize)]
pub struct SlotView {
    /// 1-5, left to right.
    pub slot: u8,
    pub hero_id: Option<u32>,
    pub hero: Option<String>,
    pub confidence: f32,
    pub source: Option<PickSource>,
    /// Recognised with low confidence: the player should check it.
    pub uncertain: bool,
    pub alternatives: Vec<Alternative>,
}

/// The draft as the window and scripts show it.
#[derive(Debug, Clone, Serialize)]
pub struct DraftView {
    pub allies: Vec<SlotView>,
    pub enemies: Vec<SlotView>,
    pub player_hero_id: Option<u32>,
    pub player_hero: Option<String>,
    /// The player's team; Radiant when unknown (see `team_assumed`).
    pub team: Team,
    pub team_assumed: bool,
    pub role: Option<Role>,
    pub captures: u32,
    pub updated_at: i64,
    pub complete: bool,
    /// "enemies 2", ... (1-based) for slots to check.
    pub uncertain: Vec<String>,
    /// Set when hero names couldn't be looked up (ids are shown instead).
    pub note: Option<String>,
}

fn is_uncertain(slot: &Slot) -> bool {
    slot.hero_id.is_some() && slot.confidence < UNCERTAIN && !matches!(slot.source, Some(PickSource::Manual | PickSource::Gsi))
}

fn slot_views(slots: &[Slot; 5], heroes: &[Hero]) -> Vec<SlotView> {
    slots
        .iter()
        .enumerate()
        .map(|(i, s)| SlotView {
            slot: i as u8 + 1,
            hero_id: s.hero_id,
            hero: s.hero_id.map(|id| hero_name(heroes, id)),
            confidence: s.confidence,
            source: s.source,
            uncertain: is_uncertain(s),
            alternatives: s
                .alternatives
                .iter()
                .map(|(id, c)| Alternative { hero_id: *id, hero: hero_name(heroes, *id), confidence: *c })
                .collect(),
        })
        .collect()
}

/// Build the view of a draft. `heroes` may be empty (names then read "hero #id").
pub fn draft_view(draft: &DraftState, heroes: &[Hero], note: Option<String>) -> DraftView {
    let allies = slot_views(&draft.allies, heroes);
    let enemies = slot_views(&draft.enemies, heroes);
    let mut uncertain = Vec::new();
    for (side, list) in [("allies", &allies), ("enemies", &enemies)] {
        uncertain.extend(list.iter().filter(|s| s.uncertain).map(|s| format!("{side} {}", s.slot)));
    }
    DraftView {
        allies,
        enemies,
        player_hero_id: draft.player_hero,
        player_hero: draft.player_hero.map(|id| hero_name(heroes, id)),
        team: draft.player_team.unwrap_or(Team::Radiant),
        team_assumed: draft.player_team.is_none(),
        role: draft.role.or(DotaSettings::load().role),
        captures: draft.captures,
        updated_at: draft.updated_at,
        complete: draft.is_complete(),
        uncertain,
        note,
    }
}

/// The saved draft with hero names (ids only when the hero list is unavailable).
pub fn current_draft() -> DraftView {
    let draft = DraftState::load();
    match hero_list(&*source()) {
        Ok(heroes) => draft_view(&draft, &heroes, None),
        Err(e) => draft_view(&draft, &[], Some(e)),
    }
}

fn save_and_announce(draft: &mut DraftState) -> Result<(), String> {
    draft.updated_at = now();
    draft.save().map_err(|e| format!("could not save the draft: {e}"))?;
    launch::emit(CoreEvent::DotaChanged);
    Ok(())
}

/// What one capture found.
#[derive(Debug, Clone, Serialize)]
pub struct CaptureReport {
    pub draft: DraftView,
    /// Heroes recognised in this screenshot.
    pub recognized: usize,
    pub layout: String,
    pub width: u32,
    pub height: u32,
    pub warnings: Vec<String>,
    /// Where the screenshot was saved (only the last few are kept).
    pub image: String,
}

fn prune_captures(dir: &std::path::Path, keep: usize) {
    let mut files: Vec<PathBuf> = std::fs::read_dir(dir)
        .map(|entries| {
            entries
                .flatten()
                .map(|e| e.path())
                .filter(|p| p.extension().is_some_and(|e| e.eq_ignore_ascii_case("png")))
                .collect()
        })
        .unwrap_or_default();
    files.sort();
    if files.len() > keep {
        for old in &files[..files.len() - keep] {
            let _ = std::fs::remove_file(old);
        }
    }
}

/// Take a screenshot, recognise the heroes, merge them into the saved draft.
pub fn capture_draft(timeout: Duration) -> Result<CaptureReport, String> {
    let dir = data_dir().join("captures");
    std::fs::create_dir_all(&dir).map_err(|e| format!("could not create {}: {e}", dir.display()))?;
    let mut path = dir.join(format!("{}.png", chrono::Local::now().format("%Y-%m-%d_%H-%M-%S")));
    if path.exists() {
        path = super::tools::unused_path(&path);
    }
    super::tools::capture(&path, timeout).map_err(|e| format!("could not take a screenshot: {e}"))?;
    prune_captures(&dir, KEEP_CAPTURES);

    let src = source();
    let heroes = hero_list(&*src)?;
    let portraits = vision::load_portraits(&heroes).map_err(|e| format!("hero recognition isn't ready yet ({e})"))?;
    let recognition = vision::recognize_file(&path, &portraits).map_err(|e| format!("could not read the screenshot: {e}"))?;

    let mut draft = DraftState::load();
    draft.merge(&recognition);
    save_and_announce(&mut draft)?;

    let mut warnings = recognition.warnings.clone();
    if recognition.picks.is_empty() {
        warnings.push("no heroes were recognised. Is the draft (or the top bar in a match) visible on the screen?".into());
    }
    Ok(CaptureReport {
        draft: draft_view(&draft, &heroes, None),
        recognized: recognition.picks.len(),
        layout: recognition.layout,
        width: recognition.width,
        height: recognition.height,
        warnings,
        image: path.to_string_lossy().into_owned(),
    })
}

pub fn parse_side(text: &str) -> Result<Side, String> {
    match text.trim().to_lowercase().as_str() {
        "allies" | "ally" | "team" | "mine" | "friends" => Ok(Side::Allies),
        "enemies" | "enemy" | "them" | "opponents" => Ok(Side::Enemies),
        other => Err(format!("side must be \"allies\" or \"enemies\", not \"{other}\"")),
    }
}

pub fn parse_team(text: &str) -> Result<Team, String> {
    match text.trim().to_lowercase().as_str() {
        "radiant" => Ok(Team::Radiant),
        "dire" => Ok(Team::Dire),
        other => Err(format!("team must be \"radiant\" or \"dire\", not \"{other}\"")),
    }
}

/// Put a hero into a slot (1-5), or clear it with `None`. The player's choice is never
/// overwritten by later screenshots.
pub fn correct(side: Side, slot: u8, hero: Option<&str>) -> Result<DraftView, String> {
    if !(1..=5).contains(&slot) {
        return Err(format!("slot must be 1 to 5, not {slot}"));
    }
    let heroes = hero_list(&*source())?;
    let id = match hero.map(str::trim).filter(|h| !h.is_empty()) {
        Some(name) => Some(resolve_hero(&heroes, name)?),
        None => None,
    };
    let mut draft = DraftState::load();
    draft.correct(side, slot - 1, id);
    save_and_announce(&mut draft)?;
    Ok(draft_view(&draft, &heroes, None))
}

/// Set the player's own hero (e.g. when Game State Integration isn't set up).
pub fn set_player_hero(hero: Option<&str>) -> Result<DraftView, String> {
    let heroes = hero_list(&*source())?;
    let id = match hero.map(str::trim).filter(|h| !h.is_empty()) {
        Some(name) => Some(resolve_hero(&heroes, name)?),
        None => None,
    };
    let mut draft = DraftState::load();
    draft.set_player_hero(id, PickSource::Manual);
    save_and_announce(&mut draft)?;
    Ok(draft_view(&draft, &heroes, None))
}

/// The player says which team they are on; the halves swap if needed.
pub fn set_team(team: Team) -> Result<DraftView, String> {
    let mut draft = DraftState::load();
    launch::set_player_team(&mut draft, team);
    save_and_announce(&mut draft)?;
    Ok(current_draft())
}

/// Start a fresh draft (the role is kept).
pub fn reset_draft() -> Result<DraftView, String> {
    let mut draft = DraftState::load();
    let role = draft.role;
    draft.reset();
    draft.role = role;
    save_and_announce(&mut draft)?;
    Ok(current_draft())
}

/// The player's position, saved in the settings and the draft. `None` = any.
pub fn set_role(role: Option<Role>) -> Result<(), String> {
    let mut settings = DotaSettings::load();
    settings.role = role;
    settings.save()?;
    let mut draft = DraftState::load();
    draft.role = role;
    save_and_announce(&mut draft)
}

/// Heroes that fit the current draft, best first.
pub fn suggest(count: usize) -> Result<Vec<HeroSuggestion>, String> {
    let draft = DraftState::load();
    let role = draft.role.or(DotaSettings::load().role);
    recommend::suggest_heroes(&*source(), &draft, role, count.clamp(1, 30)).map_err(|e| format!("no suggestions: {e}"))
}

/// An item plan for a hero (by name), or for the player's hero with `None`.
pub fn build(hero: Option<&str>) -> Result<ItemPlan, String> {
    let src = source();
    let draft = DraftState::load();
    let id = match hero.map(str::trim).filter(|h| !h.is_empty()) {
        Some(name) => resolve_hero(&hero_list(&*src)?, name)?,
        None => draft.player_hero.ok_or(
            "your hero isn't known yet. Pick it in the Dota 2 window, wait for Game State Integration, or name one: dota.build(\"Axe\")",
        )?,
    };
    recommend::item_plan(&*src, id, &draft).map_err(|e| format!("no item plan: {e}"))
}

/// Everything the status line and `dota.status()` show.
#[derive(Debug, Clone, Serialize)]
pub struct Status {
    pub gsi_installed: bool,
    pub dota_found: bool,
    pub dota_dir: Option<String>,
    pub cfg_path: Option<String>,
    pub listening: bool,
    pub port: u16,
    pub listen_error: Option<String>,
    pub dota_running: bool,
    pub in_menu: bool,
    /// "unknown", "menu", "hero_selection", "strategy_time", "playing", ...
    pub state: launch::Phase,
    pub team: Option<Team>,
    pub hero_id: Option<u32>,
    /// "npc_dota_hero_axe" as the game reports it.
    pub hero_name: Option<String>,
    pub last_update: Option<i64>,
    pub launch_assistant: bool,
    pub launch_url: String,
    pub source: SourceInfo,
}

pub fn status() -> Status {
    let settings = DotaSettings::load();
    let dota = launch::find_dota_dir();
    let game: GameState = launch::game_state();
    let process = launch::dota_process();
    let port = launch::listening_port();
    Status {
        gsi_installed: dota.as_deref().is_some_and(launch::gsi_installed_in),
        dota_found: dota.is_some(),
        cfg_path: dota.as_deref().map(|d| launch::cfg_path(d).to_string_lossy().into_owned()),
        dota_dir: dota.map(|d| d.to_string_lossy().into_owned()),
        listening: port.is_some(),
        port: port.unwrap_or(settings.gsi_port),
        listen_error: launch::listen_error(),
        dota_running: process.is_some(),
        in_menu: launch::menu_reached(
            process.as_ref(),
            &game,
            now(),
            if port.is_some() { launch::FALLBACK_MENU_SECS_WITH_GSI } else { launch::FALLBACK_MENU_SECS },
        ),
        state: game.phase,
        team: game.team,
        hero_id: game.hero_id,
        hero_name: game.hero_name,
        last_update: game.last_update,
        launch_assistant: launch::launch_assistant_enabled(),
        launch_url: settings.launch_url,
        source: source().info(),
    }
}

// ---- Lua ---------------------------------------------------------------------------------------

fn err(function: &str, error: impl std::fmt::Display) -> mlua::Error {
    mlua::Error::runtime(format!("dota.{function}: {error}"))
}

fn to_lua<T: Serialize>(lua: &Lua, value: &T) -> mlua::Result<Value> {
    // Missing values become nil (not a "null" placeholder), so `if x then` works in scripts.
    lua.to_value_with(value, SerializeOptions::new().serialize_none_to_null(false).serialize_unit_to_null(false))
}

fn time_left(deadline: Instant, function: &str, wanted: Duration) -> mlua::Result<Duration> {
    let left = deadline.saturating_duration_since(Instant::now()).min(wanted);
    if left.is_zero() {
        return Err(err(function, "no time left before the script's time limit"));
    }
    Ok(left)
}

/// A role from "mid", "carry", "pos 4", 1-5, or nil / "any" for none.
fn role_arg(value: &Value) -> Result<Option<Role>, String> {
    let text = match value {
        Value::Nil => return Ok(None),
        Value::Integer(n) => n.to_string(),
        Value::Number(n) => (*n as i64).to_string(),
        Value::String(s) => s.to_str().map(|s| s.to_string()).unwrap_or_default(),
        _ => return Err("the role is a name like \"mid\" or a number 1-5".into()),
    };
    if matches!(text.trim().to_lowercase().as_str(), "" | "any" | "none") {
        return Ok(None);
    }
    Role::parse(&text)
        .map(Some)
        .ok_or(format!("unknown role \"{text}\". Use carry, mid, offlane, soft_support, hard_support or 1-5"))
}

pub fn register(lua: &Lua, deadline: Instant) -> mlua::Result<()> {
    let dota = lua.create_table()?;

    dota.set("status", lua.create_function(|lua, ()| to_lua(lua, &status()))?)?;
    dota.set("in_menu", lua.create_function(|_, ()| Ok(launch::in_menu()))?)?;
    dota.set(
        "wait_for_menu",
        lua.create_function(move |_, seconds: Option<f64>| {
            let seconds = seconds.unwrap_or(300.0);
            if !(0.0..=3600.0).contains(&seconds) {
                return Err(err("wait_for_menu", "seconds must be between 0 and 3600"));
            }
            // Never past the script's time limit (leave a moment to act afterwards).
            let until = (Instant::now() + Duration::from_secs_f64(seconds)).min(deadline.checked_sub(Duration::from_secs(2)).unwrap_or(deadline));
            loop {
                if launch::in_menu() {
                    return Ok(true);
                }
                if Instant::now() >= until {
                    return Ok(false);
                }
                std::thread::sleep(Duration::from_secs(1).min(until.saturating_duration_since(Instant::now())));
            }
        })?,
    )?;
    dota.set(
        "open_launch_url",
        lua.create_function(|_, ()| {
            let outcome = launch::open_launch_url().map_err(|e| err("open_launch_url", e))?;
            Ok((outcome == OpenOutcome::Opened, outcome.explain()))
        })?,
    )?;
    dota.set(
        "capture_draft",
        lua.create_function(move |lua, ()| {
            let timeout = time_left(deadline, "capture_draft", Duration::from_secs(30))?;
            let report = capture_draft(timeout).map_err(|e| err("capture_draft", e))?;
            to_lua(lua, &report)
        })?,
    )?;
    dota.set("draft", lua.create_function(|lua, ()| to_lua(lua, &current_draft()))?)?;
    dota.set(
        "correct",
        lua.create_function(|lua, (side, slot, hero): (String, u8, Option<String>)| {
            let side = parse_side(&side).map_err(|e| err("correct", e))?;
            let view = correct(side, slot, hero.as_deref()).map_err(|e| err("correct", e))?;
            to_lua(lua, &view)
        })?,
    )?;
    dota.set(
        "set_hero",
        lua.create_function(|lua, hero: Option<String>| {
            let view = set_player_hero(hero.as_deref()).map_err(|e| err("set_hero", e))?;
            to_lua(lua, &view)
        })?,
    )?;
    dota.set(
        "set_team",
        lua.create_function(|lua, team: String| {
            let team = parse_team(&team).map_err(|e| err("set_team", e))?;
            to_lua(lua, &set_team(team).map_err(|e| err("set_team", e))?)
        })?,
    )?;
    dota.set(
        "reset",
        lua.create_function(|_, ()| {
            reset_draft().map_err(|e| err("reset", e))?;
            Ok(true)
        })?,
    )?;
    dota.set(
        "suggest",
        lua.create_function(|lua, count: Option<usize>| to_lua(lua, &suggest(count.unwrap_or(5)).map_err(|e| err("suggest", e))?))?,
    )?;
    dota.set(
        "build",
        lua.create_function(|lua, hero: Option<String>| to_lua(lua, &build(hero.as_deref()).map_err(|e| err("build", e))?))?,
    )?;
    dota.set(
        "heroes",
        lua.create_function(|lua, ()| {
            let heroes = hero_list(&*source()).map_err(|e| err("heroes", e))?;
            let list = lua.create_table()?;
            for hero in &heroes {
                let entry: Table = lua.create_table()?;
                entry.set("id", hero.id)?;
                entry.set("name", hero.localized_name.clone())?;
                entry.set("short_name", hero.short_name())?;
                entry.set("full_name", hero.name.clone())?;
                entry.set("primary_attr", hero.primary_attr.clone())?;
                entry.set("attack_type", hero.attack_type.clone())?;
                entry.set("roles", hero.roles.clone())?;
                list.push(entry)?;
            }
            Ok(list)
        })?,
    )?;
    dota.set(
        "set_role",
        lua.create_function(|_, role: Value| {
            let role = role_arg(&role).map_err(|e| err("set_role", e))?;
            set_role(role).map_err(|e| err("set_role", e))?;
            Ok(true)
        })?,
    )?;
    dota.set("show", lua.create_function(|_, ()| Ok(launch::emit(CoreEvent::ShowDota)))?)?;

    lua.globals().set("dota", dota)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hero(id: u32, short: &str, name: &str) -> Hero {
        Hero {
            id,
            name: format!("npc_dota_hero_{short}"),
            localized_name: name.into(),
            primary_attr: "str".into(),
            attack_type: "Melee".into(),
            roles: vec![],
        }
    }

    #[test]
    fn heroes_are_found_by_any_name() {
        let heroes = vec![
            hero(1, "antimage", "Anti-Mage"),
            hero(2, "axe", "Axe"),
            hero(53, "furion", "Nature's Prophet"),
            hero(3, "bane", "Bane"),
            hero(4, "bloodseeker", "Bloodseeker"),
        ];
        assert_eq!(resolve_hero(&heroes, "anti-mage"), Ok(1));
        assert_eq!(resolve_hero(&heroes, "ANTIMAGE"), Ok(1));
        assert_eq!(resolve_hero(&heroes, "npc_dota_hero_axe"), Ok(2));
        assert_eq!(resolve_hero(&heroes, "natures prophet"), Ok(53));
        assert_eq!(resolve_hero(&heroes, "furion"), Ok(53));
        assert_eq!(resolve_hero(&heroes, "blood"), Ok(4));
        assert_eq!(resolve_hero(&heroes, "53"), Ok(53));
        assert!(resolve_hero(&heroes, "b").unwrap_err().contains("could be"));
        assert!(resolve_hero(&heroes, "pudge").unwrap_err().contains("no hero"));
        assert!(resolve_hero(&heroes, "999").is_err());
    }

    #[test]
    fn roles_are_read_from_names_and_numbers() {
        assert_eq!(role_arg(&Value::Integer(2)), Ok(Some(Role::Mid)));
        assert_eq!(role_arg(&Value::Nil), Ok(None));
        assert!(role_arg(&Value::Boolean(true)).is_err());
    }
}
