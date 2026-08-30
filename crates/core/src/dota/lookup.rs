//! Look up any hero: who it tends to beat, who tends to beat it, what it usually buys.
//! OWNER: lookup agent.
//!
//! - Matchups come from ONE request: `matchups(hero_id)`, the hero's results against every other
//!   hero. Each pairing is compared with the hero's own win rate over all rows of that table
//!   (its baseline in this data, the same idea `recommend.rs` uses for an enemy's table) and
//!   weighted by sample size; pairings under [`MIN_GAMES`] are ignored.
//! - Common items come from `item_popularity(hero_id)`: purchase counts per stage.
//! - Traits: the source's role labels plus the curated table in `traits.rs` (rules of thumb).
//!
//! Nothing here fails because statistics are missing: the result is partial and `data_note`
//! says what is missing. Only an unknown hero id is an error.

use std::collections::{HashMap, HashSet};

use serde::{Deserialize, Serialize};

use super::{
    data::{age_text, DotaSource, ItemPopularity},
    recommend::MIN_GAMES,
    traits::{self, Trait},
    Evidence, Hero, ItemAdvice, ItemInfo, Reason,
};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Matchup {
    pub hero_id: u32,
    pub hero: String,
    pub reason: Reason,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct HeroLookup {
    pub hero_id: u32,
    pub hero: String,
    /// Heroes this hero tends to do well against, best first.
    pub strong_against: Vec<Matchup>,
    /// Heroes that tend to do well against this hero ("counters"), strongest first.
    pub weak_against: Vec<Matchup>,
    pub common_items: Vec<ItemAdvice>,
    /// "Carry", "Disabler", ... plus curated traits.
    pub traits: Vec<String>,
    pub data_note: String,
}

/// Shrinks small samples: weight = games / (games + SHRINK). Same value as `recommend.rs`.
const SHRINK: f32 = 50.0;
/// Same wording as `recommend.rs` (its constants are private).
const MATCHUP_SOURCE: &str = "OpenDota, professional matches of the last 12 months";
const ITEM_SOURCE: &str = "OpenDota, the hero's last 100 parsed professional matches";

/// Items shown per stage: (stage, how many, minimum cost, label, phrase).
const STAGES: [(Stage, usize, u32, &str, &str); 4] = [
    (Stage::Start, 4, 0, "Starting items", "in the starting inventory"),
    (Stage::Early, 3, 400, "Early game", "in the first 10 minutes"),
    (Stage::Mid, 3, 1000, "Mid game", "in the mid game"),
    (Stage::Late, 3, 2000, "Late game", "in the late game"),
];

#[derive(Clone, Copy)]
enum Stage {
    Start,
    Early,
    Mid,
    Late,
}

// Minimal re-implementation of `recommend.rs`'s private COMPONENTS / CONSUMABLES filters:
// bought as parts of bigger items or used up; not "common items" worth listing after the start.
const COMPONENTS: &[&str] = &[
    "ogre_axe", "blade_of_alacrity", "staff_of_wizardry", "mithril_hammer", "ultimate_orb", "point_booster", "demon_edge",
    "eagle", "reaver", "hyperstone", "mystic_staff", "relic", "platemail", "talisman_of_evasion", "quarterstaff", "broadsword",
    "claymore", "javelin", "void_stone", "energy_booster", "vitality_booster", "chainmail", "cornucopia", "lifesteal",
    "ring_of_health", "gloves", "boots_of_elves", "belt_of_strength", "robe", "blades_of_attack", "ring_of_regen",
    "sobi_mask", "helm_of_iron_will", "blitz_knuckles", "voodoo_mask", "fluffy_hat", "wind_lace", "crown", "diadem",
    "ring_of_tarrasque", "tiara_of_selemene", "gauntlets", "slippers", "mantle", "circlet", "ring_of_protection",
];
const CONSUMABLES: &[&str] = &[
    "tango", "clarity", "flask", "ward_observer", "ward_sentry", "dust", "smoke_of_deceit", "tpscroll", "enchanted_mango",
    "faerie_fire", "blood_grenade", "tome_of_knowledge", "ward_dispenser", "cheese", "aegis",
];

/// Look up `hero_id`: up to `count` heroes it tends to do well against, up to `count` that tend
/// to do well against it, common items per stage and its traits.
pub fn hero_lookup(src: &dyn DotaSource, hero_id: u32, count: usize) -> Result<HeroLookup, String> {
    let mut notes: Vec<String> = Vec::new();
    let heroes = match src.heroes() {
        Ok(h) => h,
        Err(e) => {
            notes.push(format!("Hero list unavailable ({e}); using the built-in list."));
            builtin_heroes()
        }
    };
    let hero = heroes.iter().find(|h| h.id == hero_id).cloned().ok_or_else(|| format!("unknown hero id {hero_id}"))?;
    let names: HashMap<u32, &str> = heroes.iter().map(|h| (h.id, h.localized_name.as_str())).collect();
    let name_of = |id: u32| names.get(&id).map(|n| n.to_string()).unwrap_or_else(|| format!("Hero #{id}"));

    // ---- matchups (one request) ----
    let rows = src.matchups(hero_id);
    let info = src.info();
    let fetched_at = info.fetched_at.unwrap_or(0);
    let mut strong_against = Vec::new();
    let mut weak_against = Vec::new();
    let mut used_stats = false;
    match rows {
        Ok(rows) if !rows.is_empty() => {
            used_stats = true;
            let rows: Vec<_> = rows.into_iter().filter(|r| r.against != hero_id && r.games > 0).collect();
            let games: u32 = rows.iter().map(|r| r.games).sum();
            let wins: u32 = rows.iter().map(|r| r.wins).sum();
            let baseline = if games > 0 { wins as f32 / games as f32 } else { 0.5 };
            let small = rows.iter().filter(|r| r.games < MIN_GAMES).count();
            let mut scored: Vec<(f32, u32, Reason)> = Vec::new();
            for m in rows.iter().filter(|r| r.games >= MIN_GAMES) {
                let wr = m.wins as f32 / m.games as f32;
                let delta = wr - baseline; // > 0: the hero tends to do well against this opponent
                let score = delta * m.games as f32 / (m.games as f32 + SHRINK);
                if score == 0.0 {
                    continue;
                }
                let other = name_of(m.against);
                let detail = format!(
                    "{} won {} of {} games against {} ({} over all its games)",
                    hero.localized_name,
                    pct(wr),
                    m.games,
                    other,
                    pct(baseline)
                );
                let text = if score > 0.0 {
                    format!("Data suggests {} tends to do well against {}: {}.", hero.localized_name, other, detail)
                } else {
                    format!("Data suggests {} tends to struggle against {}: {}.", hero.localized_name, other, detail)
                };
                let evidence = Evidence::Sourced { source: MATCHUP_SOURCE.into(), detail, fetched_at };
                scored.push((score, m.against, Reason { text, evidence }));
            }
            scored.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal).then(a.1.cmp(&b.1)));
            let to_matchup = |(_, id, reason): &(f32, u32, Reason)| Matchup { hero_id: *id, hero: name_of(*id), reason: reason.clone() };
            strong_against = scored.iter().filter(|s| s.0 > 0.0).take(count).map(to_matchup).collect();
            weak_against = scored.iter().rev().filter(|s| s.0 < 0.0).take(count).map(to_matchup).collect();
            notes.push(format!(
                "Matchups: {MATCHUP_SOURCE}; each pairing is compared with {}'s {} win rate over all its games there and weighted by the number of games.",
                hero.localized_name,
                pct(baseline)
            ));
            if small > 0 {
                notes.push(format!("{small} pairings with fewer than {MIN_GAMES} games were ignored as too small to trust."));
            }
        }
        Ok(_) => notes.push(format!("No matchup statistics recorded for {}.", hero.localized_name)),
        Err(e) => notes.push(format!("No matchup statistics for {} ({e}).", hero.localized_name)),
    }

    // ---- common items ----
    let mut common_items = Vec::new();
    match src.item_popularity(hero_id) {
        Ok(p) if !is_empty(&p) => match src.items() {
            Ok(items) => {
                used_stats = true;
                common_items = common_items_from(&p, &items, fetched_at);
                notes.push(format!("Item counts are purchases in {ITEM_SOURCE}, not win rates."));
            }
            Err(e) => notes.push(format!("Item list unavailable ({e}), so the item statistics can't be named.")),
        },
        Ok(_) => notes.push(format!("No item statistics recorded for {}.", hero.localized_name)),
        Err(e) => notes.push(format!("No item statistics for {} ({e}).", hero.localized_name)),
    }

    // ---- traits ----
    let traits_out = hero_traits(&hero);
    notes.push("Roles come from the source; the other traits are curated rules of thumb, not measured.".into());

    // ---- note ----
    let mut data_note: Vec<String> = Vec::new();
    if used_stats {
        let age = info.fetched_at.map(|t| format!(", fetched {}", age_text(t, super::now()))).unwrap_or_default();
        data_note.push(format!("Data: {}{}.", info.name, age));
        if info.offline && !info.note.is_empty() {
            data_note.push(format!("Offline: {}", info.note));
        }
    } else {
        data_note.push("No statistics available: this is a partial result (traits only).".into());
    }
    if let Some(p) = &info.patch {
        data_note.push(format!("Latest patch known to the source: {p}."));
    }
    data_note.extend(notes);
    data_note.push("Statistics describe past games; they do not predict a single game.".into());

    Ok(HeroLookup {
        hero_id,
        hero: hero.localized_name.clone(),
        strong_against,
        weak_against,
        common_items,
        traits: traits_out,
        data_note: data_note.join(" "),
    })
}

fn pct(x: f32) -> String {
    format!("{:.1}%", x * 100.0)
}

fn is_empty(p: &ItemPopularity) -> bool {
    p.start.is_empty() && p.early.is_empty() && p.mid.is_empty() && p.late.is_empty()
}

/// Minimal re-implementation of `recommend.rs`'s private hero-list fallback: the curated table.
fn builtin_heroes() -> Vec<Hero> {
    traits::all_curated()
        .into_iter()
        .map(|t| Hero {
            id: t.id,
            name: format!("npc_dota_hero_{}", t.short_name),
            localized_name: t
                .short_name
                .split('_')
                .map(|w| {
                    let mut c = w.chars();
                    c.next().map(|f| f.to_uppercase().chain(c).collect::<String>()).unwrap_or_default()
                })
                .collect::<Vec<_>>()
                .join(" "),
            primary_attr: String::new(),
            attack_type: String::new(),
            roles: Vec::new(),
        })
        .collect()
}

/// Top few items per stage, each with its priority within the stage (1 = most bought).
/// An item is listed only in the first stage where it qualifies.
fn common_items_from(p: &ItemPopularity, items: &[ItemInfo], fetched_at: i64) -> Vec<ItemAdvice> {
    let by_id: HashMap<u32, &ItemInfo> = items.iter().map(|i| (i.id, i)).collect();
    let mut used: HashSet<&str> = HashSet::new();
    let mut out = Vec::new();
    for (stage, take, min_cost, label, phrase) in STAGES {
        let list = match stage {
            Stage::Start => &p.start,
            Stage::Early => &p.early,
            Stage::Mid => &p.mid,
            Stage::Late => &p.late,
        };
        let mut rank = 0u8;
        for (id, n) in list {
            if rank as usize == take {
                break;
            }
            let Some(item) = by_id.get(id) else { continue };
            // Cost 0 = removed or neutral item; not something to buy.
            if item.cost == 0 || item.cost < min_cost || used.contains(item.key.as_str()) {
                continue;
            }
            let key = item.key.as_str();
            if !matches!(stage, Stage::Start) && (COMPONENTS.contains(&key) || CONSUMABLES.contains(&key)) {
                continue;
            }
            used.insert(key);
            rank += 1;
            out.push(ItemAdvice {
                item: item.name.clone(),
                key: item.key.clone(),
                priority: rank,
                why: format!("{label}: bought {n} times {phrase} across the hero's last 100 parsed professional matches."),
                evidence: Evidence::Sourced {
                    source: ITEM_SOURCE.into(),
                    detail: format!("{} bought {n} times {phrase}", item.name),
                    fetched_at,
                },
                alternatives: Vec::new(),
            });
        }
    }
    out
}

fn position_name(i: usize) -> &'static str {
    ["carry (position 1)", "mid (position 2)", "offlane (position 3)", "soft support (position 4)", "hard support (position 5)"][i]
}

/// Source roles, then curated traits and usual positions.
fn hero_traits(hero: &Hero) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for r in &hero.roles {
        if !out.contains(r) {
            out.push(r.clone());
        }
    }
    if let Some(c) = traits::curated(hero) {
        for t in Trait::ALL {
            if c.has(t) {
                let label = t.label().to_string();
                if !out.contains(&label) {
                    out.push(label);
                }
            }
        }
        for (i, fit) in c.positions.iter().enumerate() {
            if *fit >= 7.0 / 9.0 - 1e-4 {
                out.push(format!("Usually played as {}", position_name(i)));
            }
        }
    }
    out
}

// ---- finding a hero by name ------------------------------------------------------------------

/// Common nicknames and abbreviations -> short name. Only ones that point at a single hero.
const NICKNAMES: &[(&str, &str)] = &[
    ("am", "antimage"),
    ("pa", "phantom_assassin"),
    ("cm", "crystal_maiden"),
    ("wr", "windrunner"),
    ("sf", "nevermore"),
    ("od", "obsidian_destroyer"),
    ("ck", "chaos_knight"),
    ("tb", "terrorblade"),
    ("pl", "phantom_lancer"),
    ("es", "earthshaker"),
    ("shaker", "earthshaker"),
    ("wk", "skeleton_king"),
    ("ta", "templar_assassin"),
    ("qop", "queenofpain"),
    ("nyx", "nyx_assassin"),
    ("lc", "legion_commander"),
    ("bb", "bristleback"),
    ("bs", "bloodseeker"),
    ("dk", "dragon_knight"),
    ("dp", "death_prophet"),
    ("ds", "dark_seer"),
    ("et", "elder_titan"),
    ("ls", "life_stealer"),
    ("np", "furion"),
    ("kotl", "keeper_of_the_light"),
    ("aa", "ancient_apparition"),
    ("sk", "sand_king"),
    ("wd", "witch_doctor"),
    ("ww", "winter_wyvern"),
    ("fv", "faceless_void"),
    ("sb", "spirit_breaker"),
    ("sd", "shadow_demon"),
    ("ns", "night_stalker"),
    ("bh", "bounty_hunter"),
    ("mk", "monkey_king"),
    ("dw", "dark_willow"),
    ("ld", "lone_druid"),
    ("zet", "arc_warden"),
    ("dusa", "medusa"),
];

/// Lowercase letters and digits only: "Nature's Prophet" -> "naturesprophet".
fn norm(s: &str) -> String {
    s.chars().filter(|c| c.is_alphanumeric()).flat_map(|c| c.to_lowercase()).collect()
}

/// Find a hero by what a player would type: the localized name ("Anti-Mage"), the short name
/// ("antimage", "nevermore"), a common nickname ("am", "qop", "wk") or a prefix that matches
/// only one hero ("jugg", "lancer"). Case, spaces and punctuation don't matter.
/// Returns `None` when nothing matches or the query is ambiguous ("shadow").
pub fn find_hero<'a>(heroes: &'a [Hero], query: &str) -> Option<&'a Hero> {
    let q = norm(query);
    if q.is_empty() {
        return None;
    }
    // 1. Exact name.
    if let Some(h) = heroes.iter().find(|h| norm(&h.localized_name) == q || norm(h.short_name()) == q || norm(&h.name) == q) {
        return Some(h);
    }
    // 2. Nickname.
    if let Some((_, short)) = NICKNAMES.iter().find(|(nick, _)| *nick == q) {
        if let Some(h) = heroes.iter().find(|h| h.short_name() == *short) {
            return Some(h);
        }
    }
    if q.len() < 2 {
        return None;
    }
    // 3. Prefix of the whole name (localized or short) or of any word of the localized name
    //    ("jugg", "lancer", "maiden"), when exactly one hero matches. "void" matches both
    //    Void Spirit and Faceless Void, so it is ambiguous.
    let mut matches = heroes.iter().filter(|h| {
        norm(&h.localized_name).starts_with(&q)
            || norm(h.short_name()).starts_with(&q)
            || h.localized_name.split(|c: char| c.is_whitespace() || c == '-').any(|w| norm(w).starts_with(&q))
    });
    let first = matches.next()?;
    matches.next().is_none().then_some(first)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalizes() {
        assert_eq!(norm("Nature's Prophet"), "naturesprophet");
        assert_eq!(norm("  Anti-Mage "), "antimage");
    }

    #[test]
    fn nicknames_are_unique_keys() {
        let mut seen = HashSet::new();
        for (nick, _) in NICKNAMES {
            assert_eq!(*nick, norm(nick), "nicknames must be normalized");
            assert!(seen.insert(*nick), "duplicate nickname {nick}");
        }
    }
}
