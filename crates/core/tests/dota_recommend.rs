//! Dota companion: statistics source, hero suggestions and item plans.
//! Fully offline: recorded OpenDota data in `tests/fixtures/dota/data`, and an unreachable
//! local address for the cache tests.

use std::{collections::HashSet, path::PathBuf};

use localflow_core::dota::{
    data::{DotaSource, FixtureSource, OpenDotaSource},
    recommend::{item_plan, suggest, suggest_heroes},
    traits, DraftState, Evidence, Role,
};

fn fixtures() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/dota/data")
}

fn src() -> FixtureSource {
    FixtureSource::new(fixtures())
}

fn draft(allies: &[u32], enemies: &[u32]) -> DraftState {
    let mut d = DraftState::default();
    for (i, id) in allies.iter().enumerate() {
        d.allies[i].hero_id = Some(*id);
    }
    for (i, id) in enemies.iter().enumerate() {
        d.enemies[i].hero_id = Some(*id);
    }
    d
}

// Phantom Lancer, Naga Siren, Chaos Knight, Terrorblade
const ILLUSIONS: &[u32] = &[12, 89, 81, 109];
// Lina, Lion, Zeus, Skywrath Mage, Leshrac
const MAGIC: &[u32] = &[25, 26, 22, 101, 52];
// Necrophos, Omniknight, Dazzle, Huskar, Oracle
const HEALING: &[u32] = &[36, 57, 50, 59, 111];
const JUGGERNAUT: u32 = 8;

fn top(ids: &[u32], role: Option<Role>, n: usize) -> Vec<u32> {
    suggest_heroes(&src(), &draft(&[], ids), role, n).unwrap().iter().map(|s| s.hero_id).collect()
}

#[test]
fn fixture_source_reports_patch_and_data() {
    let s = src();
    let info = s.info();
    assert_eq!(info.patch.as_deref(), Some("7.41"));
    assert!(info.fetched_at.is_some());
    assert!(s.heroes().unwrap().len() > 120);
    assert!(s.items().unwrap().iter().any(|i| i.key == "black_king_bar"));
    assert!(!s.matchups(25).unwrap().is_empty());
}

#[test]
fn every_current_hero_has_curated_traits() {
    let missing: Vec<String> =
        src().heroes().unwrap().iter().filter(|h| traits::curated(h).is_none()).map(|h| h.short_name().to_string()).collect();
    // Heroes left out on purpose fall back to the source's roles.
    assert_eq!(missing, vec!["largo".to_string()]);
}

#[test]
fn contrasting_lineups_give_different_suggestions() {
    let a = top(ILLUSIONS, None, 3);
    let b = top(MAGIC, None, 3);
    let c = top(HEALING, None, 3);
    println!("illusions {a:?} magic {b:?} healing {c:?}");
    assert_ne!(a, b);
    assert_ne!(b, c);
    assert_ne!(a, c);
    assert!(a[0] != b[0] || b[0] != c[0], "top picks should not all be the same");

    // With statistics every suggestion is backed by at least one sourced reason or says why not.
    let s = suggest(&src(), &draft(&[], MAGIC), None, 5);
    println!("{}
{:#?}", s.note, s.heroes[0]);
    assert!(!s.heuristic_only);
    assert!(s.note.contains("do not predict a win"), "{}", s.note);
    for h in &s.heroes {
        assert!((2..=4).contains(&h.reasons.len()), "{:?}", h.reasons);
    }
    assert!(s.heroes[0].reasons.iter().any(|r| matches!(&r.evidence, Evidence::Sourced { detail, .. } if detail.contains("games"))));
}

#[test]
fn contrasting_lineups_give_different_adaptations() {
    let plan = |enemies: &[u32]| item_plan(&src(), JUGGERNAUT, &draft(&[], enemies)).unwrap();
    let ill = plan(ILLUSIONS);
    let mag = plan(MAGIC);
    let heal = plan(HEALING);
    let keys = |p: &localflow_core::dota::ItemPlan| p.situational.iter().map(|a| a.key.clone()).collect::<HashSet<_>>();
    let texts = |p: &localflow_core::dota::ItemPlan| p.adaptations.iter().map(|a| a.text.clone()).collect::<Vec<_>>();
    println!("{:#?}\n{:#?}\n{:#?}", texts(&ill), texts(&mag), texts(&heal));

    assert!(texts(&ill).iter().any(|t| t.starts_with("Illusions and summons")));
    assert!(keys(&ill).contains("bfury") || keys(&ill).contains("mjollnir"));
    assert!(texts(&mag).iter().any(|t| t.starts_with("Lots of magic damage")));
    assert!(keys(&mag).contains("black_king_bar"));
    assert!(texts(&heal).iter().any(|t| t.starts_with("Healing and regeneration")));
    assert!(keys(&heal).contains("skadi") || keys(&heal).contains("spirit_vessel"));
    assert_ne!(texts(&ill), texts(&mag));
    assert_ne!(texts(&mag), texts(&heal));

    // Adaptations are rules of thumb; the base plan comes from statistics.
    for p in [&ill, &mag, &heal] {
        assert!(p.adaptations.iter().all(|a| matches!(a.evidence, Evidence::Heuristic { .. })));
        assert!(!p.starting.is_empty() && !p.core.is_empty());
        assert!(p.starting.iter().chain(&p.core).all(|a| matches!(a.evidence, Evidence::Sourced { .. })));
        assert!(p.data_note.contains("OpenDota"), "{}", p.data_note);
        let prios: Vec<u8> = p.core.iter().map(|a| a.priority).collect();
        assert_eq!(prios, (1..=prios.len() as u8).collect::<Vec<_>>());
    }
    // Only items that exist in the item data.
    let known: HashSet<String> = src().items().unwrap().into_iter().map(|i| i.key).collect();
    for p in [&ill, &mag, &heal] {
        for a in p.starting.iter().chain(&p.core).chain(&p.situational) {
            assert!(known.contains(&a.key), "{}", a.key);
        }
    }
}

#[test]
fn ranged_heroes_are_not_told_to_buy_battle_fury() {
    // Lion (ranged) as a carry, against illusions.
    let mut d = draft(&[], ILLUSIONS);
    d.role = Some(Role::Carry);
    let plan = item_plan(&src(), 26, &d).unwrap();
    assert!(plan.situational.iter().all(|a| a.key != "bfury"));
}

#[test]
fn picked_heroes_are_never_suggested() {
    let mut d = draft(&[1, 2], &[25, 26, 12]);
    d.player_hero = Some(8);
    let taken: HashSet<u32> = [1, 2, 25, 26, 12, 8].into_iter().collect();
    for role in [None, Some(Role::Carry), Some(Role::HardSupport)] {
        let all = suggest_heroes(&src(), &d, role, 500).unwrap();
        assert!(!all.is_empty());
        assert!(all.iter().all(|s| !taken.contains(&s.hero_id)), "{role:?}");
    }
}

#[test]
fn role_changes_results() {
    let carry = top(MAGIC, Some(Role::Carry), 5);
    let support = top(MAGIC, Some(Role::HardSupport), 5);
    println!("carry {carry:?} support {support:?}");
    assert!(carry.iter().all(|id| !support.contains(id)));
    let heroes = src().heroes().unwrap();
    for id in &carry {
        let h = heroes.iter().find(|h| h.id == *id).unwrap();
        assert!(traits::role_fit(h, Role::Carry) >= 0.4, "{}", h.localized_name);
    }
}

#[test]
fn works_with_partial_and_empty_drafts() {
    let s = suggest(&src(), &DraftState::default(), None, 5);
    assert_eq!(s.heroes.len(), 5);
    assert!(s.note.contains("No enemy picks"));
    let s = suggest(&src(), &draft(&[18], &[25]), Some(Role::Mid), 5);
    assert_eq!(s.heroes.len(), 5);
    assert!(!s.heuristic_only);
}

#[test]
fn missing_data_degrades_to_heuristics() {
    // No data at all: built-in hero list, heuristics only, never a crash.
    let empty = tempfile::tempdir().unwrap();
    let none = FixtureSource::new(empty.path());
    let s = suggest(&none, &draft(&[], MAGIC), Some(Role::Offlane), 5);
    assert_eq!(s.heroes.len(), 5);
    assert!(s.heuristic_only);
    assert!(s.note.contains("rules of thumb"), "{}", s.note);
    for h in &s.heroes {
        assert!(h.reasons.iter().all(|r| matches!(r.evidence, Evidence::Heuristic { .. })));
        assert!(h.reasons.len() >= 2);
    }
    let plan = item_plan(&none, JUGGERNAUT, &draft(&[], ILLUSIONS)).unwrap();
    assert!(plan.starting.is_empty() && plan.core.is_empty() && plan.situational.is_empty());
    assert!(plan.data_note.contains("unavailable"), "{}", plan.data_note);
    assert!(plan.adaptations.iter().any(|a| a.text.starts_with("Illusions and summons")));

    // Enemies without recorded matchups (Bane, Crystal Maiden): heuristic only, with a note.
    let s = suggest(&src(), &draft(&[], &[3, 5]), None, 5);
    assert!(s.heuristic_only);
    assert!(s.note.contains("Bane"), "{}", s.note);

    // A hero without item statistics: generic heuristic base plan.
    let plan = item_plan(&src(), 5, &draft(&[], HEALING)).unwrap();
    assert!(!plan.starting.is_empty());
    assert!(plan.starting.iter().chain(&plan.core).all(|a| matches!(a.evidence, Evidence::Heuristic { .. })));
    assert!(plan.data_note.contains("No item statistics"), "{}", plan.data_note);
}

#[test]
fn offline_serves_stale_cache() {
    // The only test in this binary that touches LOCALFLOW_DOTA_DIR.
    let dir = tempfile::tempdir().unwrap();
    std::env::set_var("LOCALFLOW_DOTA_DIR", dir.path());
    let cache = dir.path().join("cache");
    std::fs::create_dir_all(&cache).unwrap();
    let heroes: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(fixtures().join("heroes.json")).unwrap()).unwrap();
    let month_ago = localflow_core::dota::now() - 30 * 86400;
    let entry = serde_json::json!({ "source": "OpenDota", "fetched_at": month_ago, "body": heroes });
    std::fs::write(cache.join("heroes.json"), entry.to_string()).unwrap();
    let items: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(fixtures().join("items.json")).unwrap()).unwrap();
    let fresh = serde_json::json!({ "source": "OpenDota", "fetched_at": localflow_core::dota::now() - 60, "body": items });
    std::fs::write(cache.join("items.json"), fresh.to_string()).unwrap();

    // Fresh cache: served without touching the network.
    let online = OpenDotaSource::with_base_url("http://127.0.0.1:9");
    assert!(online.items().unwrap().len() > 100);
    assert!(!online.info().offline);

    // Stale cache + unreachable server: stale copy, offline flag, note.
    let source = OpenDotaSource::with_base_url("http://127.0.0.1:9");
    let list = source.heroes().unwrap();
    assert!(list.len() > 120);
    let info = source.info();
    assert!(info.offline);
    assert_eq!(info.fetched_at, Some(month_ago));
    assert!(info.note.contains("Could not refresh the hero list"), "{}", info.note);

    // Nothing cached: a clear error.
    let err = source.matchups(25).unwrap_err();
    assert!(err.contains("no cached copy"), "{err}");

    // Recommendations still work and say they are offline.
    let plan = item_plan(&source, JUGGERNAUT, &draft(&[], MAGIC)).unwrap();
    assert!(plan.data_note.contains("Offline"), "{}", plan.data_note);
    let s = suggest(&source, &draft(&[], MAGIC), None, 3);
    assert_eq!(s.heroes.len(), 3);
    assert!(s.heuristic_only);
    std::env::remove_var("LOCALFLOW_DOTA_DIR");
}
