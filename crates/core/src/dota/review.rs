//! Post-game review from OpenDota. OWNER: review agent.
//!
//! What the OpenDota endpoints return (checked with live requests, 2026-10-01):
//! - `/players/{id}/recentMatches`: the player's last 20 matches (fixed, no paging), newest
//!   first. Each row has `match_id`, `player_slot` (0-4 Radiant, 128-132 Dire), `radiant_win`,
//!   `hero_id`, `start_time`, `duration` (seconds), `kills`, `deaths`, `assists`,
//!   `gold_per_min`, `xp_per_min`, `last_hits`, `hero_damage`, `tower_damage`,
//!   `hero_healing`, `average_rank` (the match's average rank tier, e.g. 75 = Divine 5),
//!   `lane_role`, `version` (non-null only when OpenDota parsed the replay). No items.
//! - `/benchmarks?hero_id=H[&bracket=1-8]`: `{"hero_id", "result": {metric: [{percentile,
//!   value}]}}` with percentiles 0.1 ... 0.9, 0.95, 0.99 for `gold_per_min`, `xp_per_min`,
//!   `kills_per_min`, `deaths_per_min`, `assists_per_min`, `last_hits_per_min`,
//!   `denies_per_min`, `hero_damage_per_min`, `hero_healing_per_min` and `tower_damage`
//!   (a total, not per minute). The curves come from recent public matches of that hero
//!   (all ranks, or one rank bracket). An unknown hero gives `null` values.
//! - `/matches/{id}`: `players[]` with `item_0` ... `item_5` (item ids) for every match,
//!   parsed or not (unparsed matches still carry the final inventory from Steam's match
//!   data); parsed matches add logs, which we drop before caching.
//! - Privacy: when a player turns off "Expose Public Match Data" in Dota 2, Valve hides their
//!   account id in match data (it appears as missing / 4294967295), so OpenDota never links
//!   new matches to them: `recentMatches` answers `[]` (or only matches from before the
//!   setting was changed). OpenDota's profile then usually has `fh_unavailable: true`
//!   (Steam refused the match history). An account OpenDota has never seen answers
//!   `recentMatches` with `[]` and `/players/{id}` with HTTP 404.
//!
//! All requests go through [`OpenDotaSource`] (rate limited, cached): recent matches for
//! 2 minutes, benchmarks and the profile for a day, match details for 7 days.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::{
    data::{age_text, DotaSource, OpenDotaSource},
    Evidence, Hero, Reason,
};

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct MatchSummary {
    pub match_id: u64,
    pub hero_id: u32,
    pub hero: String,
    pub won: bool,
    pub kills: u32,
    pub deaths: u32,
    pub assists: u32,
    pub gpm: u32,
    pub xpm: u32,
    pub last_hits: u32,
    pub duration_secs: u32,
    /// Unix seconds.
    pub start_time: i64,
    /// Final item names, when the source has them.
    pub items: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Benchmark {
    /// "gold per minute", "last hits per minute", ...
    pub metric: String,
    pub value: f64,
    /// 0.0 - 1.0 compared with other players of this hero, if the source has it.
    /// Higher is always better: for deaths the scale is turned around (0.9 = fewer deaths
    /// than about 90% of players).
    pub percentile: Option<f64>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MatchReview {
    pub summary: MatchSummary,
    pub benchmarks: Vec<Benchmark>,
    /// Takeaways, each labelled Sourced or Heuristic.
    pub notes: Vec<Reason>,
    pub data_note: String,
}

/// Steam64 ids are Steam32 ids plus this.
pub const STEAM64_BASE: u64 = 76_561_197_960_265_728;
/// The account id Valve uses for hidden / anonymous players.
pub const ANONYMOUS_ACCOUNT: u64 = 4_294_967_295;
/// OpenDota's `recentMatches` never returns more than this.
pub const MAX_RECENT: usize = 20;
const SOURCE: &str = "OpenDota";

// ---- account ids ------------------------------------------------------------------------------

/// An account id from what the player pasted: a Dotabuff/OpenDota/STRATZ profile link,
/// a Steam32 id or a Steam64 id (also a steamcommunity.com/profiles/<Steam64> link and the
/// `[U:1:<Steam32>]` form). Returns the Steam32 id, or `None` for anything else.
pub fn account_id_from(text: &str) -> Option<u64> {
    let t = text.trim().trim_matches(|c: char| c == '<' || c == '>' || c == '"' || c == '\'').trim();
    if t.is_empty() || t.len() > 200 {
        return None;
    }
    if let Some(inner) = t.strip_prefix("[U:1:").and_then(|r| r.strip_suffix(']')) {
        return normalize(inner);
    }
    if t.bytes().all(|b| b.is_ascii_digit()) {
        return normalize(t);
    }
    // A link: optional scheme and "www.", then a known host and path.
    let lower = t.to_ascii_lowercase();
    let rest = lower
        .strip_prefix("https://")
        .or_else(|| lower.strip_prefix("http://"))
        .unwrap_or(&lower);
    let rest = rest.strip_prefix("www.").unwrap_or(rest);
    let (host, path) = rest.split_once('/')?;
    let prefix = match host {
        "dotabuff.com" | "opendota.com" | "stratz.com" => "players/",
        "steamcommunity.com" => "profiles/",
        _ => return None,
    };
    let id = path.strip_prefix(prefix)?;
    let id = id.split(['/', '?', '#']).next()?;
    if id.is_empty() || !id.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    normalize(id)
}

/// Digits -> Steam32, accepting Steam64. Rejects 0, the anonymous id and out-of-range values.
fn normalize(digits: &str) -> Option<u64> {
    if digits.is_empty() || digits.len() > 20 || !digits.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    let n: u64 = digits.parse().ok()?;
    let id = if n >= STEAM64_BASE { n - STEAM64_BASE } else { n };
    (id > 0 && id < ANONYMOUS_ACCOUNT).then_some(id)
}

// ---- parsing ----------------------------------------------------------------------------------

/// One row of OpenDota's `/players/{id}/recentMatches`.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct RecentMatch {
    pub match_id: u64,
    pub player_slot: u32,
    pub radiant_win: Option<bool>,
    pub hero_id: u32,
    pub start_time: i64,
    pub duration: u32,
    pub kills: u32,
    pub deaths: u32,
    pub assists: u32,
    pub gold_per_min: u32,
    pub xp_per_min: u32,
    pub last_hits: u32,
    pub hero_damage: Option<u32>,
    /// Average rank tier of the match (tens digit = medal: 1 Herald ... 8 Immortal).
    pub average_rank: Option<u32>,
    /// True when OpenDota parsed the replay.
    pub parsed: bool,
}

impl RecentMatch {
    pub fn is_radiant(&self) -> bool {
        self.player_slot < 128
    }

    /// `None` when the result is unknown.
    pub fn won(&self) -> Option<bool> {
        self.radiant_win.map(|rw| rw == self.is_radiant())
    }

    /// OpenDota benchmark bracket (1-8) from the match's average rank.
    pub fn bracket(&self) -> Option<u8> {
        self.average_rank.map(|r| (r / 10) as u8).filter(|b| (1..=8).contains(b))
    }
}

fn num(v: &Value, key: &str) -> Option<f64> {
    v.get(key).and_then(|x| x.as_f64())
}

fn uint(v: &Value, key: &str) -> u32 {
    num(v, key).map(|n| n.max(0.0) as u32).unwrap_or(0)
}

/// Parse `/players/{id}/recentMatches`. Rows without a match id or hero are skipped.
pub fn parse_recent_matches(body: &Value) -> Result<Vec<RecentMatch>, String> {
    let rows = body.as_array().ok_or("unexpected recent matches format")?;
    Ok(rows
        .iter()
        .filter_map(|r| {
            let match_id = r.get("match_id")?.as_u64()?;
            let hero_id = r.get("hero_id")?.as_u64()? as u32;
            Some(RecentMatch {
                match_id,
                player_slot: uint(r, "player_slot"),
                radiant_win: r.get("radiant_win").and_then(|v| v.as_bool()),
                hero_id,
                start_time: r.get("start_time").and_then(|v| v.as_i64()).unwrap_or(0),
                duration: uint(r, "duration"),
                kills: uint(r, "kills"),
                deaths: uint(r, "deaths"),
                assists: uint(r, "assists"),
                gold_per_min: uint(r, "gold_per_min"),
                xp_per_min: uint(r, "xp_per_min"),
                last_hits: uint(r, "last_hits"),
                hero_damage: num(r, "hero_damage").map(|n| n.max(0.0) as u32),
                average_rank: num(r, "average_rank").map(|n| n as u32),
                parsed: r.get("version").map(|v| !v.is_null()).unwrap_or(false),
            })
        })
        .collect())
}

/// A percentile curve: `(percentile 0-1, value)` points, sorted by percentile.
pub type Curve = Vec<(f64, f64)>;

/// Parse `/benchmarks`: metric name -> curve. `null` points are dropped, and a metric with
/// fewer than two points is left out (OpenDota has no data for it).
pub fn parse_benchmarks(body: &Value) -> Result<HashMap<String, Curve>, String> {
    let result = body.get("result").and_then(|r| r.as_object()).ok_or("unexpected benchmarks format")?;
    let mut out = HashMap::new();
    for (metric, points) in result {
        let Some(points) = points.as_array() else { continue };
        let mut curve: Curve = points
            .iter()
            .filter_map(|p| {
                let pct = num(p, "percentile")?;
                let value = num(p, "value")?;
                (pct.is_finite() && value.is_finite()).then_some((pct, value))
            })
            .collect();
        curve.sort_by(|a, b| a.0.total_cmp(&b.0));
        if curve.len() >= 2 {
            out.insert(metric.clone(), curve);
        }
    }
    Ok(out)
}

/// Where `value` falls on a percentile curve (0.0 - 1.0), interpolating linearly between
/// points. Ties resolve to the highest percentile with that value ("at least as good as").
/// Below the first point it interpolates from (0, 0) (all metrics are >= 0); above the last
/// point it returns the last percentile. `None` for an empty curve or a non-finite value.
pub fn percentile_at(curve: &[(f64, f64)], value: f64) -> Option<f64> {
    if curve.is_empty() || !value.is_finite() {
        return None;
    }
    // Values should rise with the percentile; smooth out any dips.
    let mut points: Vec<(f64, f64)> = Vec::with_capacity(curve.len());
    for &(p, v) in curve {
        let v = points.last().map_or(v, |&(_, last): &(f64, f64)| v.max(last));
        points.push((p, v));
    }
    let last_at_or_below = points.iter().rposition(|&(_, v)| v <= value);
    let pct = match last_at_or_below {
        None => {
            let (p0, v0) = points[0];
            if v0 > 0.0 && value > 0.0 {
                p0 * (value / v0)
            } else {
                0.0
            }
        }
        Some(i) if i + 1 == points.len() => points[i].0,
        Some(i) => {
            let (p1, v1) = points[i];
            let (p2, v2) = points[i + 1];
            p1 + (p2 - p1) * (value - v1) / (v2 - v1)
        }
    };
    Some(pct.clamp(0.0, 1.0))
}

/// The six final inventory item ids (non-empty slots) of the player in `player_slot`, from a
/// `/matches/{id}` body. `None` when the player is not in the body.
pub fn parse_match_items(body: &Value, player_slot: u32) -> Option<Vec<u32>> {
    let player = body
        .get("players")?
        .as_array()?
        .iter()
        .find(|p| p.get("player_slot").and_then(|s| s.as_u64()) == Some(player_slot as u64))?;
    Some(
        (0..6)
            .filter_map(|i| player.get(format!("item_{i}")).and_then(|v| v.as_u64()))
            .filter(|&id| id > 0)
            .map(|id| id as u32)
            .collect(),
    )
}

/// Shrink a `/matches/{id}` body to what the review uses (a parsed match is ~200 KB with
/// chat and logs). Account ids and names are dropped, so the cache holds nobody's identity.
pub fn trim_match(body: Value) -> Value {
    const MATCH_KEYS: &[&str] =
        &["match_id", "version", "duration", "start_time", "radiant_win", "radiant_score", "dire_score", "game_mode", "lobby_type"];
    const PLAYER_KEYS: &[&str] = &[
        "player_slot", "hero_id", "item_0", "item_1", "item_2", "item_3", "item_4", "item_5", "item_neutral", "kills",
        "deaths", "assists", "last_hits", "denies", "gold_per_min", "xp_per_min", "hero_damage", "net_worth",
    ];
    let Value::Object(map) = body else { return body };
    let mut out = serde_json::Map::new();
    for key in MATCH_KEYS {
        if let Some(v) = map.get(*key) {
            out.insert((*key).into(), v.clone());
        }
    }
    if let Some(players) = map.get("players").and_then(|p| p.as_array()) {
        let slim: Vec<Value> = players
            .iter()
            .map(|p| {
                let mut o = serde_json::Map::new();
                for key in PLAYER_KEYS {
                    if let Some(v) = p.get(*key) {
                        o.insert((*key).into(), v.clone());
                    }
                }
                Value::Object(o)
            })
            .collect();
        out.insert("players".into(), Value::Array(slim));
    }
    Value::Object(out)
}

// ---- summaries and benchmarks ------------------------------------------------------------------

fn hero_name(heroes: &[Hero], id: u32) -> String {
    heroes.iter().find(|h| h.id == id).map(|h| h.localized_name.clone()).unwrap_or_else(|| format!("Hero {id}"))
}

/// A summary without items (recent matches have none).
pub fn summary_from(row: &RecentMatch, heroes: &[Hero]) -> MatchSummary {
    MatchSummary {
        match_id: row.match_id,
        hero_id: row.hero_id,
        hero: hero_name(heroes, row.hero_id),
        won: row.won().unwrap_or(false),
        kills: row.kills,
        deaths: row.deaths,
        assists: row.assists,
        gpm: row.gold_per_min,
        xpm: row.xp_per_min,
        last_hits: row.last_hits,
        duration_secs: row.duration,
        start_time: row.start_time,
        items: Vec::new(),
    }
}

/// (benchmark key, label, lower is better)
const METRICS: &[(&str, &str, bool)] = &[
    ("gold_per_min", "gold per minute", false),
    ("xp_per_min", "experience per minute", false),
    ("last_hits_per_min", "last hits per minute", false),
    ("hero_damage_per_min", "hero damage per minute", false),
    ("kills_per_min", "kills per minute", false),
    ("deaths_per_min", "deaths per minute", true),
    ("assists_per_min", "assists per minute", false),
];

fn metric_value(row: &RecentMatch, key: &str) -> Option<f64> {
    let minutes = row.duration as f64 / 60.0;
    let per_min = |n: f64| (minutes > 0.0).then(|| n / minutes);
    match key {
        "gold_per_min" => Some(row.gold_per_min as f64),
        "xp_per_min" => Some(row.xp_per_min as f64),
        "last_hits_per_min" => per_min(row.last_hits as f64),
        "hero_damage_per_min" => per_min(row.hero_damage? as f64),
        "kills_per_min" => per_min(row.kills as f64),
        "deaths_per_min" => per_min(row.deaths as f64),
        "assists_per_min" => per_min(row.assists as f64),
        _ => None,
    }
}

/// The match's numbers against the hero's percentile curves. Metrics the match can't
/// provide are left out; metrics without a curve get `percentile: None`.
pub fn benchmarks_for(row: &RecentMatch, curves: &HashMap<String, Curve>) -> Vec<Benchmark> {
    METRICS
        .iter()
        .filter_map(|&(key, label, lower_better)| {
            let value = metric_value(row, key)?;
            let percentile = curves
                .get(key)
                .and_then(|c| percentile_at(c, value))
                .map(|p| if lower_better { 1.0 - p } else { p });
            Some(Benchmark { metric: label.into(), value: round2(value), percentile: percentile.map(round2) })
        })
        .collect()
}

fn round2(x: f64) -> f64 {
    (x * 100.0).round() / 100.0
}

/// "63rd"
pub fn ordinal(n: u32) -> String {
    let suffix = match (n % 10, n % 100) {
        (_, 11..=13) => "th",
        (1, _) => "st",
        (2, _) => "nd",
        (3, _) => "rd",
        _ => "th",
    };
    format!("{n}{suffix}")
}

fn pct_text(p: f64) -> String {
    ordinal(((p * 100.0).round() as u32).clamp(1, 99))
}

/// "Herald" ... "Immortal" for OpenDota's benchmark brackets 1-8.
pub fn bracket_name(bracket: u8) -> Option<&'static str> {
    ["Herald", "Guardian", "Crusader", "Archon", "Legend", "Ancient", "Divine", "Immortal"]
        .get((bracket as usize).wrapping_sub(1))
        .copied()
}

fn value_text(b: &Benchmark) -> String {
    if b.value >= 50.0 {
        format!("{:.0}", b.value)
    } else if b.value < 1.0 {
        format!("{:.2}", b.value)
    } else {
        format!("{:.1}", b.value)
    }
}

/// What the benchmarks are compared against, for the notes: "Divine games" / "all ranks".
fn scope_text(bracket: Option<u8>) -> String {
    match bracket.and_then(bracket_name) {
        Some(name) => format!("in {name} games"),
        None => "across all ranks".into(),
    }
}

/// Context the notes need besides the numbers.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct NoteContext {
    pub hero: String,
    /// The hero's roles as OpenDota lists them ("Support", "Carry", ...).
    pub roles: Vec<String>,
    /// Benchmark bracket used (None = all ranks).
    pub bracket: Option<u8>,
    /// When the benchmarks were fetched (Unix seconds), for Sourced evidence.
    pub benchmarks_fetched_at: Option<i64>,
    /// Team kills, when the match details were available (for kill participation).
    pub team_kills: Option<u32>,
}

const FARM_METRICS: &[&str] = &["gold per minute", "last hits per minute", "experience per minute"];
const HEADLINE_METRICS: &[&str] = &["gold per minute", "experience per minute", "last hits per minute", "hero damage per minute"];

/// 2-4 takeaways. Sourced notes quote a benchmark percentile; Heuristic notes are rules of
/// thumb. Supportive wording, no promises.
pub fn notes_for(row: &RecentMatch, benchmarks: &[Benchmark], ctx: &NoteContext) -> Vec<Reason> {
    let mut notes = Vec::new();
    let sourced = |detail: String| Evidence::Sourced {
        source: SOURCE.into(),
        detail,
        fetched_at: ctx.benchmarks_fetched_at.unwrap_or(0),
    };
    let heuristic = |rule: &str| Evidence::Heuristic { rule: rule.into() };
    let scope = scope_text(ctx.bracket);
    let is_support = ctx.roles.iter().any(|r| r == "Support") && !ctx.roles.iter().any(|r| r == "Carry");
    let rated: Vec<&Benchmark> = benchmarks.iter().filter(|b| b.percentile.is_some()).collect();
    let pct = |b: &Benchmark| b.percentile.unwrap_or(0.0);
    let detail_for =
        |b: &Benchmark| format!("{} {} = {} percentile for {} {scope}", b.metric, value_text(b), pct_text(pct(b)), ctx.hero);

    // 1. Best metric: the headline numbers (farm, experience, damage) first; kills and
    // assists only when none of those has a benchmark. Deaths get their own note.
    let best_of = |headline: bool| {
        rated
            .iter()
            .copied()
            .filter(|b| !b.metric.starts_with("deaths"))
            .filter(|b| !headline || HEADLINE_METRICS.contains(&b.metric.as_str()))
            .max_by(|a, b| pct(a).total_cmp(&pct(b)))
    };
    let best = best_of(true).or_else(|| best_of(false));
    if let Some(b) = best {
        let p = pct(b);
        let tail = if p >= 0.75 {
            " That's a real strength this game."
        } else if p >= 0.5 {
            " Above the middle of the pack."
        } else {
            " This was your strongest number this game."
        };
        notes.push(Reason {
            text: format!(
                "Your {} of {} is around the {} percentile for {} {scope} on OpenDota.{tail}",
                b.metric,
                value_text(b),
                pct_text(p),
                ctx.hero
            ),
            evidence: sourced(detail_for(b)),
        });
    }

    // 2. Weakest metric, when it is clearly low.
    // Only the headline numbers: kills and assists per minute depend too much on the game.
    let worst = rated
        .iter()
        .copied()
        .filter(|b| HEADLINE_METRICS.contains(&b.metric.as_str()))
        .filter(|b| Some(b.metric.as_str()) != best.map(|x| x.metric.as_str()))
        .min_by(|a, b| pct(a).total_cmp(&pct(b)));
    if let Some(b) = worst.filter(|b| pct(b) < 0.35) {
        let farm = FARM_METRICS.contains(&b.metric.as_str());
        let tip = if farm && is_support {
            " On a support hero that is often expected, since the cores take most of the farm."
        } else if farm {
            " Farm tends to be the easiest number to raise: keep a lane or camp to hit whenever nothing else is happening."
        } else if b.metric.starts_with("hero damage") {
            " More time in fights (or safer positions to keep hitting from) usually raises it."
        } else {
            " Something to keep an eye on next game."
        };
        notes.push(Reason {
            text: format!(
                "Your {} of {} is around the {} percentile for {} {scope}.{tip}",
                b.metric,
                value_text(b),
                pct_text(pct(b)),
                ctx.hero
            ),
            evidence: sourced(detail_for(b)),
        });
    }

    // 3. Deaths: benchmark if we have one, rule of thumb otherwise.
    let minutes = (row.duration as f64 / 60.0).max(1.0);
    let deaths = rated.iter().copied().find(|b| b.metric.starts_with("deaths"));
    match deaths {
        Some(b) if pct(b) < 0.3 => notes.push(Reason {
            text: format!(
                "{} deaths in {:.0} minutes is more than most {} players {scope} (fewer deaths than only about {}% of them). Looking at the minimap before walking forward is a cheap habit that often helps.",
                row.deaths,
                minutes,
                ctx.hero,
                (pct(b) * 100.0).round().max(1.0)
            ),
            evidence: sourced(format!(
                "deaths per minute {:.2} = {} percentile for {} {scope} (fewer is better)",
                b.value,
                pct_text(pct(b)),
                ctx.hero
            )),
        }),
        Some(b) if pct(b) >= 0.75 && row.duration >= 20 * 60 => notes.push(Reason {
            text: format!(
                "{} death{} in {:.0} minutes: you stayed alive better than about {:.0}% of {} players {scope}. Staying alive keeps your gold and map presence.",
                row.deaths,
                if row.deaths == 1 { "" } else { "s" },
                minutes,
                pct(b) * 100.0,
                ctx.hero
            ),
            evidence: sourced(format!(
                "deaths per minute {:.2} = {} percentile for {} {scope} (fewer is better)",
                b.value,
                pct_text(pct(b)),
                ctx.hero
            )),
        }),
        None if row.deaths >= 10 || (row.deaths as f64 / minutes) > 0.3 => notes.push(Reason {
            text: format!(
                "{} deaths is a lot for one game. Each death costs gold and time on the map; checking the minimap and keeping a TP scroll can help.",
                row.deaths
            ),
            evidence: heuristic("10 or more deaths (or more than one every ~3 minutes) usually hurts a game"),
        }),
        _ => {}
    }

    // 4. Kill participation, when the match details give team kills.
    if notes.len() < 4 {
        if let Some(team) = ctx.team_kills.filter(|&k| k >= 5) {
            let kp = (row.kills + row.assists) as f64 / team as f64;
            let text = if kp >= 0.6 {
                Some(format!("You took part in {:.0}% of your team's kills, so you were where the action was.", (kp * 100.0).min(100.0)))
            } else if kp < 0.35 {
                Some(format!(
                    "You took part in {:.0}% of your team's kills. If you were farming on purpose that's fine; otherwise joining more fights may pay off.",
                    kp * 100.0
                ))
            } else {
                None
            };
            if let Some(text) = text {
                notes.push(Reason { text, evidence: heuristic("kill participation = (kills + assists) / team kills; around 50-70% is common") });
            }
        }
    }

    // Short games distort per-minute numbers.
    if notes.len() < 4 && row.duration > 0 && row.duration < 20 * 60 {
        notes.push(Reason {
            text: format!("This game was short ({} min), so per-minute numbers swing more than usual.", row.duration / 60),
            evidence: heuristic("per-minute statistics are noisy in games under 20 minutes"),
        });
    }

    // Always give at least two takeaways.
    if notes.len() < 2 {
        let kda = (row.kills + row.assists) as f64 / row.deaths.max(1) as f64;
        let text = if kda >= 3.0 {
            format!("A KDA ratio of {kda:.1} ({}/{}/{}) means you contributed a lot while staying alive.", row.kills, row.deaths, row.assists)
        } else {
            format!(
                "Your KDA was {}/{}/{}. Watching the replay of one or two deaths is a quick way to spot something to try next game.",
                row.kills, row.deaths, row.assists
            )
        };
        notes.push(Reason { text, evidence: heuristic("(kills + assists) / deaths of 3 or more is a solid game") });
    }
    if notes.len() < 2 && rated.is_empty() {
        notes.push(Reason {
            text: "No hero benchmarks were available, so these notes are general rules of thumb.".into(),
            evidence: heuristic("without benchmarks there is nothing to compare against"),
        });
    }
    notes.truncate(4);
    notes
}

// ---- errors ------------------------------------------------------------------------------------

/// Friendly text for an empty match list. `profile` is the `/players/{id}` body (`None` if it
/// could not be loaded), `not_found` when OpenDota answered 404 for the profile.
pub fn explain_empty(account_id: u64, profile: Option<&Value>, not_found: bool) -> String {
    let setting = "In Dota 2, open Settings, then Options, and turn on \"Expose Public Match Data\" \
        (under Social). OpenDota can only see matches played after it is on, so play one match and try again.";
    let known = profile.and_then(|p| p.get("profile")).map(|p| !p.is_null()).unwrap_or(false);
    let private = profile
        .and_then(|p| p.get("profile"))
        .and_then(|p| p.get("fh_unavailable"))
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    if not_found || (profile.is_some() && !known) {
        format!(
            "OpenDota doesn't know account {account_id}. Please check the id (pasting your Dotabuff, OpenDota or STRATZ profile link works too). \
             If the id is right, your match data may be private. {setting}"
        )
    } else if private {
        format!("Your match data looks private, so OpenDota can't see your games. {setting}")
    } else {
        format!(
            "OpenDota has no recent matches for account {account_id}. If you just finished a game, it can take a few minutes to appear. \
             If it never shows up, your match data may be private. {setting}"
        )
    }
}

// ---- the real thing ---------------------------------------------------------------------------

fn load_recent(src: &OpenDotaSource, account_id: u64) -> Result<Vec<RecentMatch>, String> {
    if account_id == 0 || account_id >= ANONYMOUS_ACCOUNT {
        return Err(format!("{account_id} is not a valid Dota account id."));
    }
    let rows = parse_recent_matches(&src.recent_matches_body(account_id)?)?;
    if rows.is_empty() {
        let profile = src.player_body(account_id);
        let not_found = matches!(&profile, Err(e) if e.contains("HTTP 404"));
        return Err(explain_empty(account_id, profile.as_ref().ok(), not_found));
    }
    Ok(rows)
}

/// The player's last matches (newest first, at most 20), without items.
pub fn recent_matches(account_id: u64, count: usize) -> Result<Vec<MatchSummary>, String> {
    recent_matches_with(&OpenDotaSource::new(), account_id, count)
}

/// [`recent_matches`] with a given source (tests point it at a cache and an unreachable URL).
pub fn recent_matches_with(src: &OpenDotaSource, account_id: u64, count: usize) -> Result<Vec<MatchSummary>, String> {
    let rows = load_recent(src, account_id)?;
    let heroes = src.heroes().unwrap_or_default();
    Ok(rows.iter().take(count.clamp(1, MAX_RECENT)).map(|r| summary_from(r, &heroes)).collect())
}

/// Review of the player's newest match.
pub fn review_last_match(account_id: u64) -> Result<MatchReview, String> {
    review_last_match_with(&OpenDotaSource::new(), account_id)
}

/// [`review_last_match`] with a given source.
pub fn review_last_match_with(src: &OpenDotaSource, account_id: u64) -> Result<MatchReview, String> {
    let rows = load_recent(src, account_id)?;
    let row = rows.iter().max_by_key(|r| r.start_time).cloned().ok_or("no matches")?;
    let now = super::now();
    let heroes = src.heroes().unwrap_or_default();
    let mut summary = summary_from(&row, &heroes);
    let mut extra_notes: Vec<String> = Vec::new();

    // Benchmarks for the match's rank bracket (falls back to all ranks).
    let bracket = row.bracket();
    let curves = src
        .benchmarks_body(row.hero_id, bracket)
        .and_then(|b| parse_benchmarks(&b))
        .unwrap_or_default();
    let bench_key = match bracket {
        Some(b) => format!("benchmarks_{}_b{b}", row.hero_id),
        None => format!("benchmarks_{}", row.hero_id),
    };
    let benchmarks = benchmarks_for(&row, &curves);
    if curves.is_empty() {
        extra_notes.push(format!("No benchmarks for {} were available, so the notes are rules of thumb.", summary.hero));
    }

    // Final items and team kills from the match details (cached for a week).
    let mut team_kills = None;
    match src.match_body(row.match_id, &trim_match) {
        Ok(body) => {
            if let Some(ids) = parse_match_items(&body, row.player_slot) {
                let items = src.items().unwrap_or_default();
                summary.items = ids
                    .iter()
                    .map(|id| items.iter().find(|i| i.id == *id).map(|i| i.name.clone()).unwrap_or_else(|| format!("Item {id}")))
                    .collect();
            }
            let key = if row.is_radiant() { "radiant_score" } else { "dire_score" };
            team_kills = body.get(key).and_then(|v| v.as_u64()).map(|n| n as u32);
        }
        Err(_) => extra_notes.push("Final items are not available right now.".into()),
    }

    let hero = heroes.iter().find(|h| h.id == row.hero_id);
    let ctx = NoteContext {
        hero: summary.hero.clone(),
        roles: hero.map(|h| h.roles.clone()).unwrap_or_default(),
        bracket,
        benchmarks_fetched_at: src.fetched_at(&bench_key),
        team_kills,
    };
    let notes = notes_for(&row, &benchmarks, &ctx);

    // Data note: freshness, what the comparison is against, anything that went wrong.
    let mut data_note = String::from("OpenDota");
    let recent_key = format!("recentMatches_{account_id}");
    if let Some(at) = src.fetched_at(&recent_key) {
        data_note.push_str(&format!(": match list fetched {}", age_text(at, now)));
    }
    if !curves.is_empty() {
        data_note.push_str(&format!(
            "; benchmarks compare with recent public {} games",
            match bracket.and_then(bracket_name) {
                Some(n) => format!("{} {n}", summary.hero),
                None => format!("{} (all ranks)", summary.hero),
            }
        ));
        if let Some(at) = src.fetched_at(&bench_key) {
            data_note.push_str(&format!(", fetched {}", age_text(at, now)));
        }
    }
    data_note.push('.');
    if now - row.start_time > 3 * 24 * 3600 && row.start_time > 0 {
        extra_notes.push(format!(
            "Your newest match on OpenDota is from {}. If you played since, those games may be hidden: \"Expose Public Match Data\" in Dota 2's settings must be on.",
            age_text(row.start_time, now)
        ));
    }
    let (offline, notes_src) = src.offline_notes();
    if offline {
        data_note.push_str(" Offline:");
        for n in notes_src {
            data_note.push(' ');
            data_note.push_str(&n);
        }
    }
    for n in extra_notes {
        data_note.push(' ');
        data_note.push_str(&n);
    }
    Ok(MatchReview { summary, benchmarks, notes, data_note })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ordinals() {
        assert_eq!(ordinal(1), "1st");
        assert_eq!(ordinal(2), "2nd");
        assert_eq!(ordinal(3), "3rd");
        assert_eq!(ordinal(11), "11th");
        assert_eq!(ordinal(12), "12th");
        assert_eq!(ordinal(13), "13th");
        assert_eq!(ordinal(22), "22nd");
        assert_eq!(ordinal(63), "63rd");
    }

    #[test]
    fn brackets() {
        let row = |r| RecentMatch { average_rank: r, ..Default::default() };
        assert_eq!(row(Some(75)).bracket(), Some(7));
        assert_eq!(row(Some(80)).bracket(), Some(8));
        assert_eq!(row(Some(5)).bracket(), None);
        assert_eq!(row(None).bracket(), None);
        assert_eq!(bracket_name(7), Some("Divine"));
        assert_eq!(bracket_name(0), None);
    }
}
