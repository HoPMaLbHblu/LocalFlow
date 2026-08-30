//! Dota 2 live match helper: Game State Integration fields, next item, timing reminders,
//! throttling, and upgrading an old `.cfg` file. Offline: no game, no network, no browser.
//! Only temporary folders are touched (never the real Dota folder).

use std::{
    path::{Path, PathBuf},
    sync::OnceLock,
};

use localflow_core::dota::{
    launch::{self, GameState, Phase},
    live::{self, Helper, LiveState, LEAD_SECS, MIN_GAP_SECS},
    Evidence, ItemAdvice, ItemInfo, ItemPlan,
};

fn setup() -> &'static Path {
    static DIR: OnceLock<PathBuf> = OnceLock::new();
    DIR.get_or_init(|| {
        let dir = tempfile::tempdir().unwrap().keep();
        std::env::set_var("LOCALFLOW_DOTA_DIR", dir.join("data"));
        // A fake game folder, so nothing ever looks for the real one.
        let game = dir.join("steamapps/common/dota 2 beta");
        std::fs::create_dir_all(game.join("game/dota")).unwrap();
        std::env::set_var("LOCALFLOW_DOTA_GAME_DIR", &game);
        dir
    })
}

// ---- Game State Integration ---------------------------------------------------------------------

/// Shaped like a real post during a match (field names as in the Dota2GSI and dota-gsi
/// libraries' documented samples).
const PLAYING: &str = r#"{
  "provider": { "name": "Dota 2", "appid": 570, "version": 47, "timestamp": 1759300000 },
  "map": {
    "name": "start", "matchid": "8012345678", "game_time": 1012, "clock_time": 925,
    "daytime": false, "nightstalker_night": false, "radiant_score": 12, "dire_score": 9,
    "game_state": "DOTA_GAMERULES_STATE_GAME_IN_PROGRESS", "paused": false,
    "win_team": "none", "customgamename": "", "ward_purchase_cooldown": 0
  },
  "player": {
    "steamid": "76561197960265728", "accountid": "1", "name": "someone", "activity": "playing",
    "kills": 3, "deaths": 1, "assists": 5, "last_hits": 88, "denies": 7, "kill_streak": 0,
    "commands_issued": 4000, "kill_list": {}, "team_name": "dire",
    "gold": 2345, "gold_reliable": 345, "gold_unreliable": 2000,
    "gold_from_hero_kills": 500, "gold_from_creep_kills": 3000, "gold_from_income": 900,
    "gold_from_shared": 100, "gpm": 520, "xpm": 610
  },
  "hero": {
    "xpos": -1200, "ypos": 300, "id": 1, "name": "npc_dota_hero_antimage", "level": 12,
    "xp": 9000, "alive": true, "respawn_seconds": 0, "buyback_cost": 800,
    "buyback_cooldown": 0, "health": 1100, "max_health": 1400, "health_percent": 78,
    "mana": 300, "max_mana": 500, "mana_percent": 60, "silenced": false, "stunned": false,
    "disarmed": false, "magicimmune": false, "hexed": false, "muted": false, "break": false,
    "aghanims_scepter": false, "aghanims_shard": false, "smoked": false, "has_debuff": false
  },
  "items": {
    "slot0": { "name": "item_power_treads", "purchaser": 0, "item_level": 1, "can_cast": true, "cooldown": 0, "passive": false },
    "slot1": { "name": "item_bfury", "purchaser": 0, "item_level": 1, "passive": true },
    "slot2": { "name": "empty" },
    "slot3": { "name": "item_magic_wand", "purchaser": 0, "item_level": 1, "can_cast": true, "cooldown": 0, "passive": false, "charges": 7 },
    "slot4": { "name": "empty" },
    "slot5": { "name": "empty" },
    "slot6": { "name": "item_quelling_blade", "purchaser": 0, "item_level": 1, "passive": true },
    "slot7": { "name": "empty" },
    "slot8": { "name": "empty" },
    "stash0": { "name": "item_ultimate_orb", "purchaser": 0, "item_level": 1, "passive": true },
    "stash1": { "name": "empty" },
    "stash2": { "name": "empty" },
    "stash3": { "name": "empty" },
    "stash4": { "name": "empty" },
    "stash5": { "name": "empty" },
    "teleport0": { "name": "item_tpscroll", "purchaser": 0, "item_level": 1, "can_cast": false, "cooldown": 40, "passive": false, "item_charges": 1, "charges": 1 },
    "neutral0": { "name": "item_occult_bracelet", "purchaser": 0, "item_level": 1, "passive": true }
  },
  "auth": { "token": "secret-token-123" }
}"#;

#[test]
fn a_match_post_gives_clock_gold_alive_and_items() {
    setup();
    let state = launch::parse_payload(PLAYING.as_bytes(), "secret-token-123").unwrap();
    assert_eq!(state.phase, Phase::Playing);
    assert_eq!(state.clock, Some(925));
    assert_eq!(state.gold, Some(2345));
    assert_eq!(state.alive, Some(true));
    assert_eq!(state.hero_id, Some(1));
    assert_eq!(state.match_id.as_deref(), Some("8012345678"));
    // Inventory and backpack first, then the stash; no "empty", teleport or neutral slot.
    assert_eq!(state.items, vec!["power_treads", "bfury", "magic_wand", "quelling_blade", "ultimate_orb"]);

    let live = live::live_state_from(&state, state.last_update.unwrap()).unwrap();
    assert_eq!(live.clock, 925);
    assert_eq!(live.gold, 2345);
    assert!(live.alive);
    assert_eq!(live.hero_id, Some(1));
    assert_eq!(live.items.len(), 5);
}

#[test]
fn missing_total_gold_uses_the_two_parts_and_dead_heroes_are_dead() {
    let json = serde_json::json!({
        "map": { "game_state": "DOTA_GAMERULES_STATE_GAME_IN_PROGRESS", "clock_time": -45.0, "matchid": "1" },
        "player": { "activity": "playing", "gold_reliable": 100, "gold_unreliable": 250 },
        "hero": { "id": 2, "name": "npc_dota_hero_axe", "alive": false, "respawn_seconds": 12 },
        "items": { "slot0": { "name": "empty" }, "slot10": "not an object", "stash0": { "name": "item_" } }
    });
    let state = launch::state_from_json(&json);
    assert_eq!(state.clock, Some(-45), "negative before the horn, floats accepted");
    assert_eq!(state.gold, Some(350));
    assert_eq!(state.alive, Some(false));
    assert!(state.items.is_empty());
}

#[test]
fn menu_posts_and_old_posts_are_not_live() {
    let menu = serde_json::json!({
        "provider": { "name": "Dota 2" },
        "player": { "activity": "menu", "gold": 0 },
        "items": { "slot0": { "name": "item_blink" } }
    });
    let state = launch::state_from_json(&menu);
    assert_eq!(state.phase, Phase::Menu);
    assert!(state.items.is_empty() && state.gold.is_none() && state.clock.is_none());
    assert!(live::live_state_from(&state, state.last_update.unwrap()).is_none());

    let playing = GameState { phase: Phase::Playing, clock: Some(600), gold: Some(10), last_update: Some(1_000), ..Default::default() };
    assert!(live::live_state_from(&playing, 1_000 + live::FRESH_SECS).is_some());
    assert!(live::live_state_from(&playing, 1_000 + live::FRESH_SECS + 1).is_none(), "stale");
    let no_clock = GameState { clock: None, ..playing.clone() };
    assert!(live::live_state_from(&no_clock, 1_000).is_none());
}

// ---- next item ----------------------------------------------------------------------------------

fn advice(key: &str, priority: u8) -> ItemAdvice {
    ItemAdvice {
        item: key.replace('_', " "),
        key: key.into(),
        priority,
        why: "test".into(),
        evidence: Evidence::Heuristic { rule: "test".into() },
        alternatives: Vec::new(),
    }
}

fn plan() -> ItemPlan {
    ItemPlan {
        hero_id: 1,
        hero: "Anti-Mage".into(),
        starting: vec![advice("tango", 1)],
        // Out of order on purpose: priority decides.
        core: vec![advice("manta", 3), advice("power_treads", 1), advice("bfury", 2)],
        situational: vec![advice("black_king_bar", 2), advice("abyssal_blade", 1), advice("travel_boots", 3)],
        adaptations: Vec::new(),
        data_note: String::new(),
    }
}

fn items() -> Vec<ItemInfo> {
    [("tango", 90), ("power_treads", 1400), ("bfury", 4100), ("manta", 4650), ("black_king_bar", 4050), ("travel_boots", 2500)]
        .iter()
        .enumerate()
        .map(|(i, (key, cost))| ItemInfo { id: i as u32 + 1, key: key.to_string(), name: key.replace('_', " "), cost: *cost })
        .collect()
}

fn live_with(gold: u32, owned: &[&str]) -> LiveState {
    LiveState { clock: 900, gold, items: owned.iter().map(|s| s.to_string()).collect(), hero_id: Some(1), alive: true, updated_at: 0 }
}

#[test]
fn next_item_skips_owned_items_and_follows_priority() {
    let plan = plan();
    let items = items();
    let next = live::next_item(&plan, &live_with(0, &[]), &items).unwrap();
    assert_eq!(next.advice.key, "power_treads");
    assert_eq!(next.missing_gold, 1400);
    assert!(!next.affordable);

    let next = live::next_item(&plan, &live_with(5000, &["power_treads", "bfury"]), &items).unwrap();
    assert_eq!(next.advice.key, "manta");
    assert!(next.affordable);
    assert_eq!(next.missing_gold, 0);

    // Core done: situational by priority. Abyssal Blade has no known cost and is skipped.
    let next = live::next_item(&plan, &live_with(4050, &["power_treads", "bfury", "manta"]), &items).unwrap();
    assert_eq!(next.advice.key, "black_king_bar");
    assert!(next.affordable, "gold == cost is affordable");

    // A numbered upgrade owns the base item; everything owned = nothing next.
    let all = ["power_treads", "bfury", "manta", "black_king_bar", "travel_boots_2"];
    assert!(live::next_item(&plan, &live_with(99_999, &all), &items).is_none());
}

#[test]
fn owning_is_exact_or_a_numbered_upgrade() {
    let owned = vec!["dagon_3".to_string(), "travel_boots_2".to_string(), "manta".to_string()];
    assert!(live::owns(&owned, "dagon"));
    assert!(live::owns(&owned, "travel_boots"));
    assert!(live::owns(&owned, "manta"));
    assert!(!live::owns(&owned, "travel"), "a prefix is not an upgrade");
    assert!(!live::owns(&["boots_of_elves".to_string()], "boots"));
}

// ---- reminders ----------------------------------------------------------------------------------

#[test]
fn reminders_come_fifteen_seconds_before_and_respect_the_boundaries() {
    assert_eq!(LEAD_SECS, 15);
    // The first power rune (6:00) is reminded at 5:45: (from, to] includes `to`, excludes `from`.
    let at = |from, to| live::reminders_between(from, to).into_iter().map(|r| r.text).collect::<Vec<_>>();
    assert_eq!(at(344, 345), vec!["Power rune at 6:00", "Healing Lotus in the Lotus Pools at 6:00"]);
    assert!(at(345, 346).is_empty(), "already reminded");
    assert!(at(343, 344).is_empty(), "not yet");
    assert!(at(400, 400).is_empty() && at(400, 390).is_empty(), "empty or backwards range");

    // Water runes only at 2:00 and 4:00; bounty runes at 4:00 (every 4 minutes since 7.38).
    assert_eq!(at(100, 105), vec!["Water runes at 2:00"]);
    assert_eq!(at(220, 225), vec!["Bounty runes at 4:00", "Water runes at 4:00"]);
    assert!(at(340, 345).iter().all(|t| !t.starts_with("Water")));
    // Several things at 12:00, in table order.
    assert_eq!(at(700, 705), vec!["Bounty runes at 12:00", "Power rune at 12:00", "Healing Lotus in the Lotus Pools at 12:00"]);
    // Once-only events.
    assert_eq!(at(1184, 1185), vec!["Bounty runes at 20:00", "Power rune at 20:00", "Tormentor at 20:00", "Day at 20:00"]);
    let twenty = live::reminders_between(1184, 1185);
    assert!(twenty.iter().any(|r| r.kind == "tormentor" && r.clock == 1200));
    assert!(live::reminders_between(0, 5000).iter().filter(|r| r.kind == "tormentor").count() == 1);
    let tiers: Vec<i64> = live::reminders_between(-100, 5000).into_iter().filter(|r| r.kind == "neutral").map(|r| r.clock).collect();
    assert_eq!(tiers, vec![900, 1500, 2100, 3600]);
    // Shrines every 7 minutes; lotus reminders stop when the pools could be full (18:00).
    let wisdom: Vec<i64> = live::reminders_between(0, 1700).into_iter().filter(|r| r.kind == "wisdom").map(|r| r.clock).collect();
    assert_eq!(wisdom, vec![420, 840, 1260, 1680]);
    let lotus = live::reminders_between(0, 3000).into_iter().filter(|r| r.kind == "lotus").count();
    assert_eq!(lotus, 6);
    // Night at 5:00, day at 10:00.
    let daynight: Vec<String> = live::reminders_between(0, 600).into_iter().filter(|r| r.kind == "daynight").map(|r| r.text).collect();
    assert_eq!(daynight, vec!["Night at 5:00", "Day at 10:00"]);
    // Bounty runes at 0:00 are reminded before the horn.
    assert_eq!(at(-16, -15), vec!["Bounty runes at 0:00"]);
    // Nothing is left out or doubled when a range is split.
    let whole = live::reminders_between(-100, 4000);
    let mut split = Vec::new();
    let mut t = -100;
    while t < 4000 {
        split.extend(live::reminders_between(t, (t + 7).min(4000)));
        t += 7;
    }
    assert_eq!(whole, split);
}

#[test]
fn every_timing_says_where_it_comes_from() {
    for timing in live::TIMINGS {
        assert!(!timing.source.is_empty(), "{}", timing.what);
        assert!(timing.first <= timing.until);
    }
    assert_eq!(live::clock_text(-15), "-0:15");
    assert_eq!(live::clock_text(3725), "62:05");
}

// ---- notifications ------------------------------------------------------------------------------

#[test]
fn notifications_are_throttled_and_never_repeat() {
    let plan = plan();
    let items = items();
    let mut helper = Helper::new();
    let mut now = 10_000;
    let mut live = live_with(0, &[]);

    // First look at the match at 5:00: nothing due, nothing affordable.
    live.clock = 300;
    assert_eq!(helper.step(now, "m1", &live, Some(&plan), &items), None);

    // Enough gold for the treads: told once.
    now += 3;
    live.clock = 303;
    live.gold = 1500;
    assert_eq!(helper.step(now, "m1", &live, Some(&plan), &items).as_deref(), Some("You can now afford power treads (next in your plan)"));
    now += 3;
    live.clock = 306;
    assert_eq!(helper.step(now, "m1", &live, Some(&plan), &items), None, "not again");

    // 5:45: the power rune reminder is due but the last notice was 19 s ago: held back...
    let told_at = now - 3;
    now = told_at + MIN_GAP_SECS - 1;
    live.clock = 345;
    assert_eq!(helper.step(now, "m1", &live, Some(&plan), &items), None);
    // ...and shown once the gap has passed, while the event is still ahead.
    now += 2;
    live.clock = 347;
    assert_eq!(helper.step(now, "m1", &live, Some(&plan), &items).as_deref(), Some("Power rune at 6:00; Healing Lotus in the Lotus Pools at 6:00"));

    // A held-back reminder whose event has passed is dropped.
    now += 5;
    live.clock = 405; // 6:45: shrine (7:00) due, but throttled
    assert_eq!(helper.step(now, "m1", &live, Some(&plan), &items), None);
    now += 30;
    live.clock = 425; // 7:05: the shrines already activated
    assert_eq!(helper.step(now, "m1", &live, Some(&plan), &items), None);

    // Same match, the item is still affordable: never repeated. (The clock jumped past the
    // 8:00 runes, so their reminders are dropped too.)
    now += 100;
    live.clock = 500;
    assert_eq!(helper.step(now, "m1", &live, Some(&plan), &items), None);

    // Treads bought, gold for Battle Fury: the next item is announced.
    live.items = vec!["power_treads".into()];
    live.gold = 4200;
    now += 3;
    live.clock = 503;
    assert_eq!(helper.step(now, "m1", &live, Some(&plan), &items).as_deref(), Some("You can now afford bfury (next in your plan)"));

    // Reminders at the same time are joined into one notification.
    now += 60;
    live.clock = 704;
    let _ = helper.step(now, "m1", &live, Some(&plan), &items);
    now += 60;
    live.clock = 706;
    assert_eq!(
        helper.step(now, "m1", &live, Some(&plan), &items).as_deref(),
        Some("Bounty runes at 12:00; Power rune at 12:00; Healing Lotus in the Lotus Pools at 12:00")
    );

    // A new match starts fresh: the same item can be announced again there.
    let mut fresh = live_with(1500, &[]);
    fresh.clock = 200;
    now += 60;
    assert_eq!(helper.step(now, "m2", &fresh, Some(&plan), &items).as_deref(), Some("You can now afford power treads (next in your plan)"));
}

#[test]
fn without_a_plan_only_reminders_are_shown() {
    let mut helper = Helper::new();
    let mut live = live_with(99_999, &[]);
    live.clock = 100;
    assert_eq!(helper.step(1, "m", &live, None, &[]), None);
    live.clock = 105;
    assert_eq!(helper.step(4, "m", &live, None, &[]).as_deref(), Some("Water runes at 2:00"));
}

// ---- the .cfg file ------------------------------------------------------------------------------

const OLD_CFG: &str = r#""LocalFlow Dota 2 companion"
{
    "uri"           "http://127.0.0.1:3417/"
    "timeout"       "5.0"
    "buffer"        "0.1"
    "throttle"      "0.5"
    "heartbeat"     "30.0"
    "data"
    {
        "auth"      "1"
        "provider"  "1"
        "map"       "1"
        "player"    "1"
        "hero"      "1"
    }
    "auth"
    {
        "token"     "oldtoken"
    }
}
"#;

#[test]
fn the_cfg_asks_for_items_and_old_localflow_files_are_upgraded() {
    setup();
    let text = launch::gsi_config_text_with(3417, "abc");
    assert!(text.contains(r#""items"     "1""#), "{text}");
    for extra in ["allplayers", "abilities", "draft", "wearables"] {
        assert!(!text.contains(&format!("\"{extra}\"")), "{extra} should not be requested");
    }

    // A fake game folder of its own (not the real one, not the shared one).
    let game = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(game.path().join("game/dota")).unwrap();
    assert!(!launch::upgrade_gsi_in(game.path(), 3417).unwrap(), "nothing installed: nothing written");
    assert!(!launch::cfg_path(game.path()).exists());

    let path = launch::cfg_path(game.path());
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(&path, OLD_CFG).unwrap();
    let other = launch::cfg_dir(game.path()).join("gamestate_integration_other.cfg");
    std::fs::write(&other, "\"Other app\" { }").unwrap();

    assert!(launch::upgrade_gsi_in(game.path(), 3417).unwrap(), "old file rewritten");
    let new = std::fs::read_to_string(&path).unwrap();
    assert_eq!(new, launch::gsi_config_text(3417));
    assert!(new.contains("\"items\""));
    assert!(!launch::upgrade_gsi_in(game.path(), 3417).unwrap(), "second time: already up to date");
    assert_eq!(std::fs::read_to_string(&other).unwrap(), "\"Other app\" { }", "other programs' files untouched");

    // Installing again is idempotent (the file is not rewritten when unchanged).
    let modified = std::fs::metadata(&path).unwrap().modified().unwrap();
    std::thread::sleep(std::time::Duration::from_millis(20));
    assert_eq!(launch::install_gsi_into(game.path(), 3417).unwrap(), path);
    assert_eq!(std::fs::metadata(&path).unwrap().modified().unwrap(), modified);

    // A file with our name but not written by LocalFlow is left alone.
    std::fs::write(&path, "\"Hand made\"\n{\n}\n").unwrap();
    assert!(!launch::upgrade_gsi_in(game.path(), 3417).unwrap());
    assert_eq!(std::fs::read_to_string(&path).unwrap(), "\"Hand made\"\n{\n}\n");
}

// ---- the template -------------------------------------------------------------------------------

#[test]
fn the_live_helper_template_explains_itself() {
    setup();
    let template = localflow_core::lua::find_example("dota-live-helper").unwrap();
    assert_eq!(template.category, "games");
    assert!(!template.allow_system);
    assert!(template.triggers.contains("hotkey"));
    let policy = std::sync::Arc::new(localflow_core::lua::sandbox::PathPolicy::new(&[setup().to_path_buf()]));
    let ctx = localflow_core::lua::engine::RunContext::new(1, "Dota live test", "manual", false);
    let result = localflow_core::lua::engine::execute(template.code, &ctx, policy, std::time::Duration::from_secs(20));
    assert!(result.success, "{:?}\n{}", result.error, result.output());
    let notice = result.logs.iter().find(|l| l.level == "notify").expect("a notification");
    assert!(notice.message.contains("Dota 2 live helper"), "{}", notice.message);
    assert!(notice.message.contains("Settings"), "{}", notice.message);
}
