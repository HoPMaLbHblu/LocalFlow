//! Hero suggestions and item plans. OWNER: research agent.
//!
//! Suggestions support the player's decision; they never promise a win. Every reason says
//! where it comes from: measured statistics (`Evidence::Sourced`, with sample size and age)
//! or a rule of thumb from the curated trait table (`Evidence::Heuristic`).
//! Missing statistics never fail a call: the result falls back to heuristics and says so.

use std::collections::{HashMap, HashSet};

use super::{
    data::{age_text, DotaSource, ItemPopularity, Matchup},
    traits::{self, Trait},
    DraftState, Evidence, Hero, HeroSuggestion, ItemAdvice, ItemInfo, ItemPlan, Reason, Role,
};

/// Hero pairs with fewer games than this are ignored as too small to trust.
pub const MIN_GAMES: u32 = 20;
/// Shrinks small samples: weight = games / (games + SHRINK).
const SHRINK: f32 = 50.0;
/// Below this position fit a hero is not suggested for the chosen role (unless too few are left).
const MIN_ROLE_FIT: f32 = 0.4;
const MATCHUP_SOURCE: &str = "OpenDota, professional matches of the last 12 months";
const ITEM_SOURCE: &str = "OpenDota, the hero's last 100 parsed professional matches";

/// Suggestions plus a note on the data behind them (freshness, what was missing).
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Suggestions {
    pub heroes: Vec<HeroSuggestion>,
    pub note: String,
    /// True when no statistics were used at all (heuristics only).
    pub heuristic_only: bool,
}

pub fn suggest_heroes(src: &dyn DotaSource, draft: &DraftState, role: Option<Role>, count: usize) -> Result<Vec<HeroSuggestion>, String> {
    Ok(suggest(src, draft, role, count).heroes)
}

// ---- shared helpers ------------------------------------------------------------------------

fn pretty(short: &str) -> String {
    short
        .split('_')
        .map(|w| {
            let mut c = w.chars();
            c.next().map(|f| f.to_uppercase().chain(c).collect::<String>()).unwrap_or_default()
        })
        .collect::<Vec<_>>()
        .join(" ")
}

/// The source's hero list, or the built-in table when the source has none.
fn hero_list(src: &dyn DotaSource, notes: &mut Vec<String>) -> Vec<Hero> {
    match src.heroes() {
        Ok(h) => h,
        Err(e) => {
            notes.push(format!("Hero list unavailable ({e}); using the built-in list."));
            traits::all_curated()
                .into_iter()
                .map(|t| Hero {
                    id: t.id,
                    name: format!("npc_dota_hero_{}", t.short_name),
                    localized_name: pretty(t.short_name),
                    primary_attr: String::new(),
                    attack_type: String::new(),
                    roles: Vec::new(),
                })
                .collect()
        }
    }
}

fn picked(draft: &DraftState) -> HashSet<u32> {
    draft.allies.iter().chain(draft.enemies.iter()).filter_map(|s| s.hero_id).chain(draft.player_hero).collect()
}

fn pct(x: f32) -> String {
    format!("{:.1}%", x * 100.0)
}

fn role_name(role: Role) -> &'static str {
    match role {
        Role::Carry => "carry (position 1)",
        Role::Mid => "mid (position 2)",
        Role::Offlane => "offlane (position 3)",
        Role::SoftSupport => "soft support (position 4)",
        Role::HardSupport => "hard support (position 5)",
    }
}

fn names(heroes: &[&Hero]) -> String {
    heroes.iter().map(|h| h.localized_name.as_str()).collect::<Vec<_>>().join(", ")
}

fn heuristic(text: String, rule: &str) -> Reason {
    Reason { text, evidence: Evidence::Heuristic { rule: rule.into() } }
}

// ---- hero suggestions ------------------------------------------------------------------------

/// One enemy's matchup table, oriented as "how that enemy did against each hero".
struct EnemyData<'a> {
    hero: &'a Hero,
    /// The enemy's win rate over all its rows (its baseline in this data).
    baseline: f32,
    rows: HashMap<u32, Matchup>,
}

/// Suggest up to `count` heroes for the player, best first.
pub fn suggest(src: &dyn DotaSource, draft: &DraftState, role: Option<Role>, count: usize) -> Suggestions {
    let mut notes = Vec::new();
    let heroes = hero_list(src, &mut notes);
    let by_id: HashMap<u32, &Hero> = heroes.iter().map(|h| (h.id, h)).collect();
    let taken = picked(draft);

    let allies: Vec<&Hero> =
        draft.allies.iter().filter_map(|s| s.hero_id).chain(draft.player_hero).collect::<HashSet<_>>().into_iter().filter_map(|id| by_id.get(&id).copied()).collect();
    let enemies: Vec<&Hero> = draft.enemies.iter().filter_map(|s| s.hero_id).filter_map(|id| by_id.get(&id).copied()).collect();

    // Matchups: one request per known enemy, never one per candidate.
    let mut data: Vec<EnemyData> = Vec::new();
    let mut missing: Vec<&Hero> = Vec::new();
    for e in &enemies {
        match src.matchups(e.id) {
            Ok(rows) if !rows.is_empty() => {
                let games: u32 = rows.iter().map(|r| r.games).sum();
                let wins: u32 = rows.iter().map(|r| r.wins).sum();
                let baseline = if games > 0 { wins as f32 / games as f32 } else { 0.5 };
                data.push(EnemyData { hero: e, baseline, rows: rows.into_iter().map(|r| (r.against, r)).collect() });
            }
            _ => missing.push(e),
        }
    }
    let info = src.info();
    let fetched_at = info.fetched_at.unwrap_or(0);

    let team_has = |t: Trait| allies.iter().any(|h| traits::has_trait(h, t));
    let enemy_with = |t: Trait| enemies.iter().filter(|h| traits::has_trait(h, t)).copied().collect::<Vec<_>>();
    let enemy_units: Vec<&Hero> =
        enemies.iter().filter(|h| traits::has_trait(h, Trait::Illusions) || traits::has_trait(h, Trait::Summons)).copied().collect();
    let enemy_evasion = enemy_with(Trait::Evasion);

    let mut small_samples = 0usize;
    let mut scored: Vec<(f32, f32, HeroSuggestion)> = Vec::new();
    for h in heroes.iter().filter(|h| !taken.contains(&h.id)) {
        let fit = role.map(|r| traits::role_fit(h, r));
        let mut score = fit.map(|f| 1.5 * f).unwrap_or(0.0);
        let mut reasons: Vec<(f32, Reason)> = Vec::new();

        // Counter value from statistics.
        for d in &data {
            let Some(m) = d.rows.get(&h.id) else { continue };
            if m.games < MIN_GAMES {
                small_samples += 1;
                continue;
            }
            let enemy_wr = m.wins as f32 / m.games as f32;
            let delta = d.baseline - enemy_wr; // > 0: this hero tends to do well against the enemy
            let weight = m.games as f32 / (m.games as f32 + SHRINK);
            let value = delta * weight * 10.0;
            score += value;
            let detail = format!(
                "{} won {} of {} games against {} ({} over all its games)",
                d.hero.localized_name,
                pct(enemy_wr),
                m.games,
                h.localized_name,
                pct(d.baseline)
            );
            let evidence = Evidence::Sourced { source: MATCHUP_SOURCE.into(), detail: detail.clone(), fetched_at };
            if value >= 0.1 {
                reasons.push((value, Reason { text: format!("Data suggests {} tends to do well against {}: {}.", h.localized_name, d.hero.localized_name, detail), evidence }));
            } else if value <= -0.2 {
                reasons.push((-value * 0.8, Reason { text: format!("Caution: data suggests {} tends to struggle against {}: {}.", h.localized_name, d.hero.localized_name, detail), evidence }));
            }
        }

        // Team composition heuristics.
        if !allies.is_empty() {
            let rules: [(Trait, f32, &str); 5] = [
                (Trait::Disable, 0.35, "Team without a stun/disable -> prefer a hero that has one"),
                (Trait::Initiation, 0.3, "Team without initiation -> prefer an initiator"),
                (Trait::MagicDamage, 0.25, "Team without magic damage -> mix damage types"),
                (Trait::PhysicalDamage, 0.25, "Team without physical damage -> mix damage types"),
                (Trait::Save, 0.2, "Team without a save -> prefer a hero that can save allies"),
            ];
            for (t, bonus, rule) in rules {
                if !team_has(t) && traits::has_trait(h, t) {
                    score += bonus;
                    reasons.push((bonus, heuristic(format!("Your team has no {} yet; {} brings it.", t.describe(), h.localized_name), rule)));
                }
            }
        }
        if !enemy_units.is_empty() && traits::has_trait(h, Trait::AreaDamage) {
            score += 0.3;
            reasons.push((
                0.3,
                heuristic(
                    format!("Enemy {} rely on illusions or summons; area damage from {} tends to help.", names(&enemy_units), h.localized_name),
                    "Enemy illusions/summons -> area damage",
                ),
            ));
        }
        if !enemy_evasion.is_empty() && traits::has_trait(h, Trait::MagicDamage) {
            score += 0.15;
            reasons.push((
                0.15,
                heuristic(
                    format!("{} has evasion or blind; {}'s magic damage is not affected by it.", names(&enemy_evasion), h.localized_name),
                    "Enemy evasion -> magic damage is unaffected",
                ),
            ));
        }

        if let (Some(r), Some(f)) = (role, fit) {
            reasons.push((0.05, heuristic(format!("{} is commonly played as {} (fit {:.0}/9 in the trait table).", h.localized_name, role_name(r), f * 9.0), "Curated position table")));
        }

        scored.push((score, fit.unwrap_or(1.0), HeroSuggestion { hero_id: h.id, hero: h.localized_name.clone(), score, reasons: finish_reasons(reasons, h, data.is_empty(), enemies.is_empty()) }));
    }

    // Role filter: drop poor fits, unless that leaves too few.
    if role.is_some() {
        let good = scored.iter().filter(|s| s.1 >= MIN_ROLE_FIT).count();
        if good >= count {
            scored.retain(|s| s.1 >= MIN_ROLE_FIT);
        }
    }
    scored.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal).then(a.2.hero_id.cmp(&b.2.hero_id)));
    let heroes_out: Vec<HeroSuggestion> = scored.into_iter().take(count).map(|s| s.2).collect();

    // The note.
    let heuristic_only = data.is_empty();
    if enemies.is_empty() {
        notes.push("No enemy picks known yet, so counter statistics can't be used.".into());
    } else if heuristic_only {
        notes.push(format!("No matchup statistics available for {}: suggestions use rules of thumb only.", names(&missing)));
    } else {
        notes.push(format!("Counter statistics: {MATCHUP_SOURCE}."));
        if !info.note.is_empty() {
            notes.push(info.note.clone());
        }
        if !missing.is_empty() {
            notes.push(format!("No matchup statistics for {}.", names(&missing)));
        }
        if small_samples > 0 {
            notes.push(format!("{small_samples} hero pairings with fewer than {MIN_GAMES} games were ignored as too small to trust."));
        }
    }
    if let Some(p) = &info.patch {
        notes.push(format!("Latest patch known to the source: {p}."));
    }
    notes.push("Suggestions support your decision; they do not predict a win.".into());

    Suggestions { heroes: heroes_out, note: notes.join(" "), heuristic_only }
}

/// Keep the 2-4 most important reasons; make sure there are at least two.
fn finish_reasons(mut reasons: Vec<(f32, Reason)>, h: &Hero, no_stats: bool, no_enemies: bool) -> Vec<Reason> {
    reasons.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal));
    let mut out: Vec<Reason> = reasons.into_iter().take(3).map(|r| r.1).collect();
    if no_stats {
        let text = if no_enemies {
            "No enemy picks yet: this suggestion uses rules of thumb only."
        } else {
            "No matchup statistics available: this suggestion uses rules of thumb only."
        };
        out.push(heuristic(text.into(), "Statistics missing -> heuristics only"));
    }
    if out.len() < 2 {
        let list: Vec<&str> = Trait::ALL.iter().filter(|t| traits::has_trait(h, **t)).map(|t| t.describe()).collect();
        let text = if list.is_empty() {
            format!("{}: no strong signal either way in this draft.", h.localized_name)
        } else {
            format!("{} brings {}.", h.localized_name, list.join(", "))
        };
        out.push(heuristic(text, "Curated hero traits"));
    }
    out.truncate(4);
    out
}

// ---- item plans ------------------------------------------------------------------------------

/// Bought mid-game as parts of bigger items; not advice on their own.
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

struct Threat {
    trait_: fn(&Hero) -> bool,
    min: usize,
    title: &'static str,
    rule: &'static str,
    advice: &'static str,
    carry_mid: &'static [&'static str],
    offlane: &'static [&'static str],
    support: &'static [&'static str],
}

fn has(t: Trait) -> impl Fn(&Hero) -> bool {
    move |h| traits::has_trait(h, t)
}

fn threats() -> Vec<Threat> {
    vec![
        Threat {
            trait_: |h| has(Trait::MagicDamage)(h),
            min: 3,
            title: "Lots of magic damage",
            rule: "3+ enemy heroes with mainly magic damage -> magic resistance or spell immunity",
            advice: "magic resistance, spell immunity or a magic barrier tends to help",
            carry_mid: &["black_king_bar", "eternal_shroud", "mage_slayer"],
            offlane: &["pipe", "black_king_bar", "eternal_shroud"],
            support: &["glimmer_cape", "pipe", "force_staff"],
        },
        Threat {
            trait_: |h| has(Trait::PhysicalDamage)(h),
            min: 3,
            title: "Lots of physical damage",
            rule: "3+ enemy heroes with mainly physical damage -> armor, evasion or ethereal form",
            advice: "armor, evasion, attack slows or ethereal form tend to help",
            carry_mid: &["butterfly", "assault", "blade_mail", "shivas_guard"],
            offlane: &["crimson_guard", "assault", "shivas_guard", "blade_mail"],
            support: &["ghost", "solar_crest", "force_staff"],
        },
        Threat {
            trait_: |h| has(Trait::Disable)(h),
            min: 3,
            title: "Many stuns and disables",
            rule: "3+ enemy heroes with reliable disables -> spell immunity, spell block or dispels",
            advice: "spell immunity, spell block or dispels tend to help",
            carry_mid: &["black_king_bar", "manta", "sphere"],
            offlane: &["black_king_bar", "lotus_orb", "sphere"],
            support: &["glimmer_cape", "force_staff", "lotus_orb", "aeon_disk"],
        },
        Threat {
            trait_: |h| has(Trait::Heal)(h),
            min: 1,
            title: "Healing and regeneration",
            rule: "Enemy heroes with healing/regeneration -> healing reduction",
            advice: "healing reduction tends to help",
            carry_mid: &["skadi", "shivas_guard", "spirit_vessel"],
            offlane: &["spirit_vessel", "shivas_guard"],
            support: &["spirit_vessel"],
        },
        Threat {
            trait_: |h| has(Trait::Illusions)(h) || has(Trait::Summons)(h),
            min: 1,
            title: "Illusions and summons",
            rule: "Enemy illusions/summons -> area damage",
            advice: "area damage tends to help against many units",
            carry_mid: &["bfury", "mjollnir", "maelstrom", "radiance"],
            offlane: &["shivas_guard", "radiance", "crimson_guard"],
            support: &[],
        },
        Threat {
            trait_: |h| has(Trait::Invisibility)(h),
            min: 1,
            title: "Invisibility",
            rule: "Enemy invisibility -> detection",
            advice: "detection (Dust, Sentry Wards, Gem) tends to help",
            carry_mid: &["dust"],
            offlane: &["dust", "ward_sentry"],
            support: &["ward_sentry", "dust", "gem"],
        },
        Threat {
            trait_: |h| has(Trait::Evasion)(h),
            min: 1,
            title: "Evasion",
            rule: "Enemy evasion -> True Strike or magic damage",
            advice: "True Strike, magic damage and disables are not affected by evasion",
            carry_mid: &["monkey_king_bar"],
            offlane: &["monkey_king_bar"],
            support: &[],
        },
    ]
}

/// Groups of items that do a similar job (for `alternatives`).
const ALTERNATIVES: &[&[&str]] = &[
    &["black_king_bar", "manta", "sphere", "lotus_orb", "aeon_disk"],
    &["pipe", "eternal_shroud", "glimmer_cape", "mage_slayer"],
    &["spirit_vessel", "skadi", "shivas_guard"],
    &["bfury", "mjollnir", "maelstrom", "radiance", "gungir"],
    &["dust", "ward_sentry", "gem"],
    &["ghost", "solar_crest", "crimson_guard", "blade_mail", "assault", "butterfly"],
    &["blink", "force_staff", "hurricane_pike", "cyclone", "wind_waker"],
    &["power_treads", "phase_boots", "arcane_boots", "tranquil_boots", "boots"],
    &["magic_wand", "bracer", "wraith_band", "null_talisman"],
];

struct ItemBook {
    by_id: HashMap<u32, ItemInfo>,
    by_key: HashMap<String, ItemInfo>,
}

impl ItemBook {
    /// Only items that exist and can be bought (removed or neutral items have no cost).
    fn get(&self, key: &str) -> Option<&ItemInfo> {
        self.by_key.get(key).filter(|i| i.cost > 0)
    }
    fn alternatives(&self, key: &str) -> Vec<String> {
        ALTERNATIVES
            .iter()
            .filter(|g| g.contains(&key))
            .flat_map(|g| g.iter())
            .filter(|k| **k != key)
            .filter_map(|k| self.get(k).map(|i| i.name.clone()))
            .take(3)
            .collect()
    }
}

/// Starting / core / situational items for `hero_id`, adapted to the enemy draft.
pub fn item_plan(src: &dyn DotaSource, hero_id: u32, draft: &DraftState) -> Result<ItemPlan, String> {
    let mut notes: Vec<String> = Vec::new();
    let heroes = hero_list(src, &mut notes);
    let hero = heroes.iter().find(|h| h.id == hero_id).cloned().ok_or_else(|| format!("unknown hero id {hero_id}"))?;
    let by_id: HashMap<u32, &Hero> = heroes.iter().map(|h| (h.id, h)).collect();
    let enemies: Vec<&Hero> = draft.enemies.iter().filter_map(|s| s.hero_id).filter_map(|id| by_id.get(&id).copied()).collect();
    let role = if draft.player_hero == Some(hero_id) || draft.player_hero.is_none() { draft.role } else { None }
        .unwrap_or_else(|| traits::main_role(&hero));
    let support = matches!(role, Role::SoftSupport | Role::HardSupport);

    let book = match src.items() {
        Ok(items) => ItemBook {
            by_id: items.iter().map(|i| (i.id, i.clone())).collect(),
            by_key: items.into_iter().map(|i| (i.key.clone(), i)).collect(),
        },
        Err(e) => {
            notes.push(format!("Item list unavailable ({e}): items can't be named, only the threats are described."));
            ItemBook { by_id: HashMap::new(), by_key: HashMap::new() }
        }
    };
    let popularity = match src.item_popularity(hero_id) {
        Ok(p) if !(p.start.is_empty() && p.early.is_empty() && p.mid.is_empty() && p.late.is_empty()) => Some(p),
        Ok(_) => {
            notes.push(format!("No item statistics recorded for {}: the base plan uses rules of thumb only.", hero.localized_name));
            None
        }
        Err(e) => {
            notes.push(format!("No item statistics for {} ({e}): the base plan uses rules of thumb only.", hero.localized_name));
            None
        }
    };
    let info = src.info();
    let fetched_at = info.fetched_at.unwrap_or(0);

    let mut starting = Vec::new();
    let mut core = Vec::new();
    let mut situational = Vec::new();
    let mut used: HashSet<String> = HashSet::new();

    if let Some(p) = &popularity {
        base_from_stats(p, &book, fetched_at, &mut starting, &mut core, &mut situational, &mut used);
    } else if !book.by_key.is_empty() {
        base_from_rules(&hero, support, &book, &mut starting, &mut core, &mut used);
    }

    // Threats in the enemy draft.
    let mut adaptations = Vec::new();
    if enemies.is_empty() {
        adaptations.push(heuristic("No enemy heroes known yet, so the plan is not adapted to threats.".into(), "No enemy picks -> no adaptations"));
    }
    for t in threats() {
        let who: Vec<&Hero> = enemies.iter().filter(|h| (t.trait_)(h)).copied().collect();
        if who.len() < t.min || who.is_empty() {
            continue;
        }
        let list = if support {
            t.support
        } else if role == Role::Offlane {
            t.offlane
        } else {
            t.carry_mid
        };
        let mut picked: Vec<&ItemInfo> = Vec::new();
        for key in list {
            if *key == "bfury" && hero.attack_type != "Melee" {
                continue; // Battle Fury cleave works for melee heroes only.
            }
            if let Some(item) = book.get(key) {
                picked.push(item);
            }
        }
        let item_names: Vec<String> = picked.iter().map(|i| i.name.clone()).collect();
        let text = if item_names.is_empty() {
            format!("{} ({}): {}.", t.title, names(&who), t.advice)
        } else {
            format!("{} ({}): {}; consider {}.", t.title, names(&who), t.advice, item_names.join(", "))
        };
        adaptations.push(heuristic(text, t.rule));
        for (n, item) in picked.iter().enumerate() {
            if used.contains(&item.key) {
                continue;
            }
            used.insert(item.key.clone());
            let alternatives = picked.iter().enumerate().filter(|(m, _)| *m != n).map(|(_, i)| i.name.clone()).collect();
            situational.push(ItemAdvice {
                item: item.name.clone(),
                key: item.key.clone(),
                priority: 0,
                why: format!("{} ({}): {}.", t.title, names(&who), t.advice),
                evidence: Evidence::Heuristic { rule: t.rule.into() },
                alternatives,
            });
        }
    }
    for list in [&mut starting, &mut core, &mut situational] {
        for (i, a) in list.iter_mut().enumerate() {
            a.priority = (i + 1).min(255) as u8;
        }
    }

    // The note.
    let mut data_note = Vec::new();
    if popularity.is_some() {
        let age = info.fetched_at.map(|t| format!(", fetched {}", age_text(t, super::now()))).unwrap_or_default();
        data_note.push(format!("Item counts: {}{}.", info.name, age));
        data_note.push(format!("Counts are purchases in {ITEM_SOURCE}, not win rates."));
    }
    if info.offline {
        data_note.push(format!("Offline: {}", info.note));
    }
    if let Some(p) = &info.patch {
        data_note.push(format!("Latest patch known to the source: {p}."));
    }
    data_note.extend(notes);
    data_note.push("Threat adaptations are rules of thumb, not measured.".into());

    Ok(ItemPlan { hero_id, hero: hero.localized_name.clone(), starting, core, situational, adaptations, data_note: data_note.join(" ") })
}

fn sourced(item: &ItemInfo, count: u32, stage: &str, fetched_at: i64, book: &ItemBook) -> ItemAdvice {
    ItemAdvice {
        item: item.name.clone(),
        key: item.key.clone(),
        priority: 0,
        why: format!("Bought {count} times {stage} in the sample."),
        evidence: Evidence::Sourced {
            source: ITEM_SOURCE.into(),
            detail: format!("{} bought {count} times {stage}", item.name),
            fetched_at,
        },
        alternatives: book.alternatives(&item.key),
    }
}

fn base_from_stats(
    p: &ItemPopularity,
    book: &ItemBook,
    fetched_at: i64,
    starting: &mut Vec<ItemAdvice>,
    core: &mut Vec<ItemAdvice>,
    situational: &mut Vec<ItemAdvice>,
    used: &mut HashSet<String>,
) {
    let lookup = |id: u32| book.by_id.get(&id).filter(|i| i.cost > 0);
    for (id, n) in p.start.iter().take(6) {
        if let Some(item) = lookup(*id) {
            starting.push(sourced(item, *n, "in the starting inventory", fetched_at, book));
        }
    }
    let big = |item: &&ItemInfo, min_cost: u32| {
        item.cost >= min_cost && !COMPONENTS.contains(&item.key.as_str()) && !CONSUMABLES.contains(&item.key.as_str())
    };
    let add = |list: &[(u32, u32)], take: usize, min_cost: u32, stage: &str, out: &mut Vec<ItemAdvice>, used: &mut HashSet<String>| {
        let mut added = 0;
        for (id, n) in list {
            if added == take {
                break;
            }
            let Some(item) = lookup(*id) else { continue };
            if !big(&item, min_cost) || used.contains(&item.key) {
                continue;
            }
            used.insert(item.key.clone());
            out.push(sourced(item, *n, stage, fetched_at, book));
            added += 1;
        }
    };
    add(&p.early, 2, 400, "in the first 10 minutes", core, used);
    add(&p.mid, 3, 1000, "in the mid game", core, used);
    add(&p.late, 3, 2000, "in the late game", situational, used);
}

fn base_from_rules(hero: &Hero, support: bool, book: &ItemBook, starting: &mut Vec<ItemAdvice>, core: &mut Vec<ItemAdvice>, used: &mut HashSet<String>) {
    let melee = hero.attack_type == "Melee";
    let mut start: Vec<&str> = vec!["tango", "flask", "branches"];
    if support {
        start.push("ward_observer");
    } else {
        start.push("faerie_fire");
        if melee {
            start.push("quelling_blade");
        }
    }
    let core_keys: &[&str] = if support { &["boots", "magic_wand", "glimmer_cape", "force_staff"] } else { &["boots", "magic_wand"] };
    let rule = "No item statistics -> common generic items for the role";
    for (keys, out, why) in [(&start[..], &mut *starting, "A common generic start"), (core_keys, &mut *core, "A common generic early item")] {
        for key in keys {
            if let Some(item) = book.get(key) {
                used.insert(item.key.clone());
                out.push(ItemAdvice {
                    item: item.name.clone(),
                    key: item.key.clone(),
                    priority: 0,
                    why: format!("{why} (no statistics for this hero)."),
                    evidence: Evidence::Heuristic { rule: rule.into() },
                    alternatives: book.alternatives(key),
                });
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pretty_names() {
        assert_eq!(pretty("crystal_maiden"), "Crystal Maiden");
    }

    #[test]
    fn picked_includes_player_hero() {
        let mut d = DraftState::default();
        d.allies[0].hero_id = Some(1);
        d.enemies[4].hero_id = Some(2);
        d.player_hero = Some(3);
        assert_eq!(picked(&d), [1, 2, 3].into_iter().collect());
    }
}
