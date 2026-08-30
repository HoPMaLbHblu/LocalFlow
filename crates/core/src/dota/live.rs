//! Live match helper: the next item in the plan, when it becomes affordable, and reminders
//! shortly before timed map events (runes, Shrines of Wisdom, Lotus Pools, Tormentor,
//! neutral item tiers, day and night).
//!
//! Uses only Valve's Game State Integration data (game clock, gold, own items, see
//! `launch.rs`); never game memory, never input to the game. It only shows notifications.
//!
//! **Owning an item** is kept simple on purpose: an item counts as owned when its key is in
//! the inventory, backpack or stash, or a numbered upgrade of it is (`dagon_3` owns `dagon`,
//! `travel_boots_2` owns `travel_boots`). Recipes are not modelled: components that are on
//! the way to an item don't count as owning it, and an item that was built into something
//! bigger (Blink Dagger into Overwhelming Blink) is no longer seen as owned.

use std::{
    collections::HashSet,
    sync::{Arc, Mutex},
};

use serde::{Deserialize, Serialize};

use super::{
    launch::{GameState, Phase},
    ItemAdvice, ItemInfo, ItemPlan,
};

/// What GSI says about the player right now.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct LiveState {
    /// Game clock in seconds (negative before the horn).
    pub clock: i64,
    pub gold: u32,
    /// Item keys in inventory, backpack and stash, e.g. "black_king_bar".
    pub items: Vec<String>,
    pub hero_id: Option<u32>,
    pub alive: bool,
    /// Unix seconds of the last GSI update.
    pub updated_at: i64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Reminder {
    /// Game clock second it is for.
    pub clock: i64,
    pub text: String,
    /// "rune", "wisdom", "lotus", "tormentor", "neutral", "daynight"
    pub kind: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct NextItem {
    pub advice: ItemAdvice,
    /// Gold still needed (0 when affordable).
    pub missing_gold: u32,
    pub affordable: bool,
}

// ---- live state ---------------------------------------------------------------------------------

/// A Game State Integration post older than this is not "live" any more. During a match the
/// game posts at least every second (the clock changes); a paused game may post less often.
pub const FRESH_SECS: i64 = 10;

/// The latest live state from GSI, if the game is sending one.
pub fn live_state() -> Option<LiveState> {
    live_state_from(&super::launch::game_state(), super::now())
}

/// The live state described by a GSI state, if it is fresh (`FRESH_SECS`) and in a match.
pub fn live_state_from(state: &GameState, now: i64) -> Option<LiveState> {
    let updated_at = state.last_update?;
    if now - updated_at > FRESH_SECS || updated_at - now > FRESH_SECS {
        return None;
    }
    if matches!(state.phase, Phase::Unknown | Phase::Menu | Phase::PostGame) {
        return None;
    }
    let clock = state.clock?;
    Some(LiveState {
        clock,
        gold: state.gold.unwrap_or(0),
        items: state.items.clone(),
        hero_id: state.hero_id,
        alive: state.alive.unwrap_or(state.hero_id.is_some()),
        updated_at,
    })
}

// ---- next item ----------------------------------------------------------------------------------

/// Whether `owned` (item keys) contains `key` or a numbered upgrade of it (`dagon_2`).
pub fn owns(owned: &[String], key: &str) -> bool {
    owned.iter().any(|k| {
        k == key
            || k.strip_prefix(key)
                .and_then(|rest| rest.strip_prefix('_'))
                .is_some_and(|n| !n.is_empty() && n.chars().all(|c| c.is_ascii_digit()))
    })
}

/// The next item from the plan that the player doesn't own yet: the core items in order of
/// priority, then the situational ones. Items whose cost is unknown are skipped (the plan
/// and the cost list come from the same item list, so this is rare). Starting items are
/// not considered: they are bought before the horn.
pub fn next_item(plan: &ItemPlan, live: &LiveState, items: &[ItemInfo]) -> Option<NextItem> {
    let by_priority = |list: &[ItemAdvice]| {
        let mut list: Vec<ItemAdvice> = list.to_vec();
        list.sort_by_key(|a| a.priority); // stable: keeps the plan's order for equal priority
        list
    };
    let mut seen = HashSet::new();
    by_priority(&plan.core)
        .into_iter()
        .chain(by_priority(&plan.situational))
        .filter(|advice| seen.insert(advice.key.clone()))
        .filter(|advice| !owns(&live.items, &advice.key))
        .find_map(|advice| {
            let cost = items.iter().find(|i| i.key == advice.key).map(|i| i.cost).filter(|c| *c > 0)?;
            let missing_gold = cost.saturating_sub(live.gold);
            Some(NextItem { advice, missing_gold, affordable: missing_gold == 0 })
        })
}

// ---- timings ------------------------------------------------------------------------------------

/// A timed map event.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Timing {
    pub kind: &'static str,
    /// What happens, e.g. "Power rune".
    pub what: &'static str,
    /// First time, game clock seconds.
    pub first: i64,
    /// Repeats every this many seconds; 0 = once.
    pub every: i64,
    /// Last time (inclusive), game clock seconds.
    pub until: i64,
    /// Where the timing comes from.
    pub source: &'static str,
}

/// Reminders come this many seconds before the event.
pub const LEAD_SECS: i64 = 15;
/// Repeating events are reminded of up to this game clock (90:00).
pub const LAST_CLOCK: i64 = 90 * 60;
/// The patch the table was checked against.
pub const TIMINGS_PATCH: &str = "7.41f";

/// Map timings for patch 7.41f, checked in October 2026 against Valve's official patch notes
/// (dota2.com/patches, 7.33 - 7.41f) and the Liquipedia Dota 2 wiki (Runes, Shrine of
/// Wisdom, Lotus Pool, Tormentor, Time of Day, Neutral Items pages and their changelogs).
/// Left out on purpose: Roshan (its respawn depends on when it died, which GSI doesn't
/// report without the `roshan` section), the Tormentor's respawn (10 min after its death)
/// and Tier 1 neutral items (available from 0:00 since 7.41).
pub const TIMINGS: &[Timing] = &[
    Timing {
        kind: "rune",
        what: "Bounty runes",
        first: 0,
        every: 240,
        until: LAST_CLOCK,
        source: "First set at 0:00 (Liquipedia: Runes); 7.38 patch notes: spawn interval 3 -> 4 minutes",
    },
    Timing {
        kind: "rune",
        what: "Water runes",
        first: 120,
        every: 120,
        until: 240,
        source: "Liquipedia: Runes (water runes at 2:00 and 4:00 only)",
    },
    Timing {
        kind: "rune",
        what: "Power rune",
        first: 360,
        every: 120,
        until: LAST_CLOCK,
        source: "Liquipedia: Runes (first power rune at 6:00, then every 2:00)",
    },
    Timing {
        kind: "wisdom",
        what: "Shrines of Wisdom",
        first: 420,
        every: 420,
        until: LAST_CLOCK,
        source: "7.33 patch notes (wisdom at 7:00, every 7 minutes); 7.38: runes replaced by Shrines of Wisdom, \
                 activating every 7 minutes (Liquipedia: Shrine of Wisdom, first at 7:00)",
    },
    Timing {
        kind: "lotus",
        what: "Healing Lotus in the Lotus Pools",
        first: 180,
        every: 180,
        // The pools start empty and hold 6: reminders until they could be full.
        until: 18 * 60,
        source: "7.33 patch notes (1 Healing Lotus every 3 minutes, up to 6); Liquipedia: Lotus Pool (pools start empty)",
    },
    Timing {
        kind: "tormentor",
        what: "Tormentor",
        first: 1200,
        every: 0,
        until: 1200,
        source: "7.39 patch notes: first spawn 15:00 -> 20:00 (unchanged in 7.41)",
    },
    Timing {
        kind: "neutral",
        what: "Tier 2 neutral items",
        first: 900,
        every: 0,
        until: 900,
        source: "7.38 patch notes: Madstone cap raised at 15:00 to craft the next tier",
    },
    Timing {
        kind: "neutral",
        what: "Tier 3 neutral items",
        first: 1500,
        every: 0,
        until: 1500,
        source: "7.38 patch notes: Madstone cap raised at 25:00",
    },
    Timing {
        kind: "neutral",
        what: "Tier 4 neutral items",
        first: 2100,
        every: 0,
        until: 2100,
        source: "7.38 patch notes: Madstone cap raised at 35:00",
    },
    Timing {
        kind: "neutral",
        what: "Tier 5 neutral items",
        first: 3600,
        every: 0,
        until: 3600,
        source: "7.38 patch notes: Madstone cap raised at 60:00",
    },
    Timing {
        kind: "daynight",
        what: "Night",
        first: 300,
        every: 600,
        until: LAST_CLOCK,
        source: "Liquipedia: Time of Day (5 min day / 5 min night since 7.20; day starts at 0:00 since 7.28)",
    },
    Timing {
        kind: "daynight",
        what: "Day",
        first: 600,
        every: 600,
        until: LAST_CLOCK,
        source: "Liquipedia: Time of Day",
    },
];

/// "8:00", "-0:15"
pub fn clock_text(clock: i64) -> String {
    let sign = if clock < 0 { "-" } else { "" };
    let secs = clock.abs();
    format!("{sign}{}:{:02}", secs / 60, secs % 60)
}

/// Reminders whose time falls in (`from_clock`, `to_clock`]. A reminder's time is
/// `LEAD_SECS` before its event; `Reminder::clock` is the event's time.
pub fn reminders_between(from_clock: i64, to_clock: i64) -> Vec<Reminder> {
    if to_clock <= from_clock {
        return Vec::new();
    }
    // Events in (from + LEAD, to + LEAD].
    let (low, high) = (from_clock + LEAD_SECS, to_clock + LEAD_SECS);
    let mut out = Vec::new();
    for (order, timing) in TIMINGS.iter().enumerate() {
        let mut add = |clock: i64| {
            out.push((clock, order, Reminder { clock, text: format!("{} at {}", timing.what, clock_text(clock)), kind: timing.kind.into() }))
        };
        if timing.every <= 0 {
            if timing.first > low && timing.first <= high {
                add(timing.first);
            }
            continue;
        }
        // The first repetition after `low`.
        let start = if low < timing.first { 0 } else { (low - timing.first) / timing.every + 1 };
        let mut clock = timing.first + start * timing.every;
        while clock <= high && clock <= timing.until {
            if clock > low {
                add(clock);
            }
            clock += timing.every;
        }
    }
    out.sort_by_key(|(clock, order, _)| (*clock, *order));
    out.into_iter().map(|(_, _, r)| r).collect()
}

// ---- notifications ------------------------------------------------------------------------------

/// At most one notification per this many seconds.
pub const MIN_GAP_SECS: i64 = 20;

/// Decides what to tell the player, with throttling. Time is passed in, so it is testable.
#[derive(Debug, Clone, Default)]
pub struct Helper {
    match_key: Option<String>,
    /// Texts shown in this match (never repeated).
    said: HashSet<String>,
    /// Unix seconds of the last notification.
    last_notice: Option<i64>,
    last_clock: Option<i64>,
    /// Reminders that are due but were held back by the throttle.
    pending: Vec<Reminder>,
}

impl Helper {
    pub fn new() -> Helper {
        Helper::default()
    }

    /// One step: `now` is unix seconds, `match_key` identifies the match and hero (a new key
    /// starts fresh). Returns the notification text to show, if any.
    ///
    /// Reminders come first (they are time-critical) and all reminders due together are
    /// joined into one text; reminders whose event has passed while throttled are dropped.
    /// Then "You can now afford X (next in your plan)", once per item.
    pub fn step(&mut self, now: i64, match_key: &str, live: &LiveState, plan: Option<&ItemPlan>, items: &[ItemInfo]) -> Option<String> {
        if self.match_key.as_deref() != Some(match_key) {
            *self = Helper { match_key: Some(match_key.to_string()), ..Helper::default() };
        }
        let from = match self.last_clock {
            Some(previous) if previous <= live.clock => previous,
            // First look at this match (or the clock went back): only what is still ahead.
            _ => live.clock - LEAD_SECS,
        };
        self.last_clock = Some(live.clock);
        for reminder in reminders_between(from, live.clock) {
            if !self.pending.contains(&reminder) {
                self.pending.push(reminder);
            }
        }
        self.pending.retain(|r| r.clock > live.clock);

        if self.last_notice.is_some_and(|t| now - t < MIN_GAP_SECS && now >= t) {
            return None;
        }
        if !self.pending.is_empty() {
            let texts: Vec<String> = std::mem::take(&mut self.pending).into_iter().map(|r| r.text).filter(|t| !self.said.contains(t)).collect();
            for text in &texts {
                self.said.insert(text.clone());
            }
            if !texts.is_empty() {
                return self.notice(now, texts.join("; "));
            }
        }
        let next = plan.and_then(|plan| next_item(plan, live, items)).filter(|n| n.affordable)?;
        let text = format!("You can now afford {} (next in your plan)", next.advice.item);
        if self.said.contains(&text) {
            return None;
        }
        self.said.insert(text.clone());
        self.notice(now, text)
    }

    fn notice(&mut self, now: i64, text: String) -> Option<String> {
        self.last_notice = Some(now);
        Some(text)
    }
}

// ---- background -----------------------------------------------------------------------------------

/// The item plan for the current match and hero, fetched off the tick thread.
#[derive(Default)]
struct PlanCache {
    /// match key + draft time the plan (or the running fetch) is for.
    key: Option<(String, i64)>,
    plan: Option<Arc<ItemPlan>>,
    items: Arc<Vec<ItemInfo>>,
    loading: bool,
    /// Unix seconds of the last fetch started.
    last_try: i64,
}

static PLANS: Mutex<Option<PlanCache>> = Mutex::new(None);
static HELPER: Mutex<Option<Helper>> = Mutex::new(None);

/// Refetch at most this often (a new draft capture, or a failed fetch).
const REFETCH_SECS: i64 = 60;

/// The cached plan for this match, starting a fetch in the background when needed.
fn cached_plan(match_key: &str, hero_id: u32, now: i64) -> (Option<Arc<ItemPlan>>, Arc<Vec<ItemInfo>>) {
    let draft_at = super::DraftState::load().updated_at;
    let key = (match_key.to_string(), draft_at);
    let mut guard = PLANS.lock().unwrap_or_else(|e| e.into_inner());
    let cache = guard.get_or_insert_with(PlanCache::default);
    if cache.key.as_ref().map(|k| &k.0) != Some(&key.0) {
        // Another match or hero: forget the old plan.
        *cache = PlanCache::default();
    }
    let stale = cache.key.as_ref() != Some(&key) || cache.plan.is_none();
    if stale && !cache.loading && (cache.last_try == 0 || now - cache.last_try >= REFETCH_SECS) {
        cache.key = Some(key.clone());
        cache.loading = true;
        cache.last_try = now;
        let spawned = std::thread::Builder::new().name("dota-live-plan".into()).spawn(move || {
            // The statistics source blocks (network, rate limits): never on the tick thread.
            let source = super::data::default_source();
            let plan = super::recommend::item_plan(&*source, hero_id, &super::DraftState::load());
            let items = source.items().unwrap_or_default();
            if let Err(e) = &plan {
                tracing::info!("Dota 2 live helper: no item plan ({e})");
            }
            let mut guard = PLANS.lock().unwrap_or_else(|e| e.into_inner());
            if let Some(cache) = guard.as_mut() {
                if cache.key.as_ref().map(|k| &k.0) == Some(&key.0) {
                    if let Ok(plan) = plan {
                        cache.plan = Some(Arc::new(plan));
                    }
                    if !items.is_empty() {
                        cache.items = Arc::new(items);
                    }
                }
                cache.loading = false;
            }
        });
        if spawned.is_err() {
            cache.loading = false;
        }
    }
    (cache.plan.clone(), cache.items.clone())
}

/// One round of the live helper, called by the background service every few seconds.
/// Does nothing unless the helper is switched on and a match is in progress.
pub fn background_tick(state: &GameState, now: i64) {
    if state.phase != Phase::Playing || !super::DotaSettings::load().live_helper {
        return;
    }
    let Some(live) = live_state_from(state, now) else { return };
    let Some(hero_id) = live.hero_id else { return };
    let match_key = format!("{}-{hero_id}", state.match_id.as_deref().unwrap_or("match"));
    let (plan, items) = cached_plan(&match_key, hero_id, now);
    let text = HELPER
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .get_or_insert_with(Helper::new)
        .step(now, &match_key, &live, plan.as_deref(), &items);
    if let Some(text) = text {
        super::launch::emit(crate::CoreEvent::Notice { message: format!("Dota 2: {text}") });
    }
}
