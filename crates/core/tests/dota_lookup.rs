//! Dota companion: hero lookup and hero search. Fully offline (recorded OpenDota data in
//! `tests/fixtures/dota/data`).

use std::{collections::HashSet, path::PathBuf};

use localflow_core::dota::{
    data::{DotaSource, FixtureSource},
    lookup::{find_hero, hero_lookup},
    recommend::MIN_GAMES,
    Evidence,
};

fn fixtures() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/dota/data")
}

fn src() -> FixtureSource {
    FixtureSource::new(fixtures())
}

const PHANTOM_ASSASSIN: u32 = 44;
const LION: u32 = 26;
const PHANTOM_LANCER: u32 = 12; // matchups recorded, item popularity not

fn assert_sourced_matchup(text: &str, ev: &Evidence) {
    assert!(text.contains("tends to") && text.contains("Data suggests"), "{text}");
    assert!(!text.to_lowercase().contains("will win") && !text.to_lowercase().contains("guarantee"), "{text}");
    match ev {
        Evidence::Sourced { source, detail, fetched_at } => {
            assert!(source.contains("OpenDota"), "{source}");
            assert!(detail.contains("games") && detail.contains('%'), "{detail}");
            assert!(*fetched_at > 0);
        }
        other => panic!("expected sourced evidence, got {other:?}"),
    }
}

#[test]
fn known_hero_has_both_lists_without_overlap() {
    let l = hero_lookup(&src(), PHANTOM_ASSASSIN, 5).unwrap();
    assert_eq!(l.hero, "Phantom Assassin");
    assert_eq!(l.strong_against.len(), 5);
    assert_eq!(l.weak_against.len(), 5);
    let strong: HashSet<u32> = l.strong_against.iter().map(|m| m.hero_id).collect();
    let weak: HashSet<u32> = l.weak_against.iter().map(|m| m.hero_id).collect();
    assert!(strong.is_disjoint(&weak), "a hero is in both lists");
    assert!(!strong.contains(&PHANTOM_ASSASSIN) && !weak.contains(&PHANTOM_ASSASSIN));
    for m in l.strong_against.iter().chain(&l.weak_against) {
        assert!(!m.hero.is_empty() && !m.hero.starts_with("Hero #"), "{}", m.hero);
        assert_sourced_matchup(&m.reason.text, &m.reason.evidence);
    }
    assert!(l.strong_against.iter().all(|m| m.reason.text.contains("do well against")));
    assert!(l.weak_against.iter().all(|m| m.reason.text.contains("struggle against")));
    assert!(l.data_note.contains("OpenDota"), "{}", l.data_note);
    assert!(l.data_note.contains("7.41"), "{}", l.data_note);
    assert!(l.data_note.contains("rules of thumb"), "{}", l.data_note);
}

#[test]
fn ranking_follows_the_data() {
    // Recompute the expected order from the raw rows.
    let rows = src().matchups(PHANTOM_ASSASSIN).unwrap();
    let games: u32 = rows.iter().map(|r| r.games).sum();
    let wins: u32 = rows.iter().map(|r| r.wins).sum();
    let base = wins as f32 / games as f32;
    let mut scored: Vec<(f32, u32)> = rows
        .iter()
        .filter(|r| r.games >= MIN_GAMES)
        .map(|r| ((r.wins as f32 / r.games as f32 - base) * r.games as f32 / (r.games as f32 + 50.0), r.against))
        .collect();
    scored.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap().then(a.1.cmp(&b.1)));
    let l = hero_lookup(&src(), PHANTOM_ASSASSIN, 3).unwrap();
    assert_eq!(l.strong_against.iter().map(|m| m.hero_id).collect::<Vec<_>>(), scored.iter().take(3).map(|s| s.1).collect::<Vec<_>>());
    assert_eq!(l.weak_against[0].hero_id, scored.last().unwrap().1);
    // Every listed opponent really did better / worse than the baseline.
    for m in &l.strong_against {
        let r = rows.iter().find(|r| r.against == m.hero_id).unwrap();
        assert!(r.wins as f32 / r.games as f32 > base);
    }
    for m in &l.weak_against {
        let r = rows.iter().find(|r| r.against == m.hero_id).unwrap();
        assert!((r.wins as f32 / r.games as f32) < base);
    }
}

#[test]
fn small_samples_are_excluded() {
    // Phantom Assassin's table has many pairings under MIN_GAMES (some with 1 game).
    let rows = src().matchups(PHANTOM_ASSASSIN).unwrap();
    let small: HashSet<u32> = rows.iter().filter(|r| r.games < MIN_GAMES).map(|r| r.against).collect();
    assert!(small.len() > 10);
    let l = hero_lookup(&src(), PHANTOM_ASSASSIN, 200).unwrap();
    for m in l.strong_against.iter().chain(&l.weak_against) {
        assert!(!small.contains(&m.hero_id), "{} has fewer than {MIN_GAMES} games", m.hero);
    }
    assert!(l.data_note.contains(&format!("{} pairings with fewer than {MIN_GAMES} games", small.len())), "{}", l.data_note);
    // With a huge count, every trusted pairing lands in exactly one list (or neither if exactly average).
    let trusted = rows.iter().filter(|r| r.games >= MIN_GAMES && r.against != PHANTOM_ASSASSIN).count();
    assert!(l.strong_against.len() + l.weak_against.len() <= trusted);
    assert!(l.strong_against.len() + l.weak_against.len() >= trusted - 2);
}

#[test]
fn count_is_respected() {
    for n in [0, 1, 3, 7] {
        let l = hero_lookup(&src(), LION, n).unwrap();
        assert_eq!(l.strong_against.len(), n, "strong, count {n}");
        assert_eq!(l.weak_against.len(), n, "weak, count {n}");
    }
}

#[test]
fn common_items_per_stage() {
    let l = hero_lookup(&src(), PHANTOM_ASSASSIN, 3).unwrap();
    assert!(!l.common_items.is_empty());
    for (label, max) in [("Starting items", 4), ("Early game", 3), ("Mid game", 3), ("Late game", 3)] {
        let stage: Vec<_> = l.common_items.iter().filter(|i| i.why.starts_with(label)).collect();
        assert!(!stage.is_empty() && stage.len() <= max, "{label}: {}", stage.len());
        let prios: Vec<u8> = stage.iter().map(|i| i.priority).collect();
        assert_eq!(prios, (1..=stage.len() as u8).collect::<Vec<_>>(), "{label}");
    }
    let keys: Vec<&str> = l.common_items.iter().map(|i| i.key.as_str()).collect();
    assert_eq!(keys.len(), keys.iter().collect::<HashSet<_>>().len(), "an item is listed twice: {keys:?}");
    assert!(keys.contains(&"tango"), "{keys:?}");
    for i in &l.common_items {
        assert!(i.why.contains("last 100 parsed professional matches"), "{}", i.why);
        match &i.evidence {
            Evidence::Sourced { source, detail, fetched_at } => {
                assert!(source.contains("OpenDota"));
                assert!(detail.contains("bought"));
                assert!(*fetched_at > 0);
            }
            other => panic!("expected sourced evidence, got {other:?}"),
        }
    }
    // After the start, no consumables or plain components.
    let later: Vec<&str> = l.common_items.iter().filter(|i| !i.why.starts_with("Starting")).map(|i| i.key.as_str()).collect();
    for k in ["tango", "flask", "ward_observer", "ogre_axe", "demon_edge"] {
        assert!(!later.contains(&k), "{k} in {later:?}");
    }
    assert!(l.data_note.contains("not win rates"), "{}", l.data_note);
}

#[test]
fn traits_list_roles_and_curated() {
    let l = hero_lookup(&src(), PHANTOM_ASSASSIN, 1).unwrap();
    assert!(l.traits.iter().any(|t| t == "Carry"), "{:?}", l.traits);
    assert!(l.traits.iter().any(|t| t == "Evasion or blind"), "{:?}", l.traits);
    assert!(l.traits.iter().any(|t| t.contains("position 1")), "{:?}", l.traits);
}

#[test]
fn unknown_hero_is_an_error() {
    let err = hero_lookup(&src(), 9999, 5).unwrap_err();
    assert!(err.contains("unknown hero"), "{err}");
    let empty = tempdir("unknown");
    assert!(hero_lookup(&FixtureSource::new(&empty), 9999, 5).is_err());
    let _ = std::fs::remove_dir_all(&empty);
}

#[test]
fn missing_item_stats_give_a_partial_result() {
    let l = hero_lookup(&src(), PHANTOM_LANCER, 3).unwrap();
    assert_eq!(l.strong_against.len(), 3);
    assert!(l.common_items.is_empty());
    assert!(l.data_note.contains("No item statistics"), "{}", l.data_note);
}

fn tempdir(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("localflow-dota-lookup-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

#[test]
fn empty_data_dir_gives_partial_result_with_note() {
    let dir = tempdir("empty");
    let l = hero_lookup(&FixtureSource::new(&dir), PHANTOM_ASSASSIN, 5).unwrap();
    assert_eq!(l.hero_id, PHANTOM_ASSASSIN);
    assert!(!l.hero.is_empty());
    assert!(l.strong_against.is_empty() && l.weak_against.is_empty() && l.common_items.is_empty());
    assert!(!l.traits.is_empty(), "curated traits still work offline");
    assert!(l.data_note.contains("partial"), "{}", l.data_note);
    assert!(l.data_note.contains("Hero list unavailable"), "{}", l.data_note);
    assert!(l.data_note.contains("No matchup statistics"), "{}", l.data_note);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn find_hero_by_name_nickname_and_prefix() {
    let heroes = src().heroes().unwrap();
    let id = |q: &str| find_hero(&heroes, q).map(|h| h.id);
    // Exact names, any case / spacing / punctuation.
    assert_eq!(id("Anti-Mage"), Some(1));
    assert_eq!(id("anti mage"), Some(1));
    assert_eq!(id("ANTIMAGE"), Some(1));
    assert_eq!(id("natures prophet"), Some(53));
    assert_eq!(id("Nature's Prophet"), Some(53));
    assert_eq!(id("nevermore"), Some(11)); // short name
    assert_eq!(id("npc_dota_hero_zuus"), Some(22));
    assert_eq!(id("io"), Some(91));
    assert_eq!(id("abaddon"), Some(102));
    // Nicknames.
    for (nick, hero) in [
        ("am", 1),
        ("PA", 44),
        ("cm", 5),
        ("wr", 21),
        ("sf", 11),
        ("od", 76),
        ("ck", 81),
        ("tb", 109),
        ("pl", 12),
        ("es", 7),
        ("wk", 42),
        ("ta", 46),
        ("QoP", 39),
        ("nyx", 88),
        ("lc", 104),
        ("np", 53),
    ] {
        assert_eq!(id(nick), Some(hero), "{nick}");
    }
    // Unique prefixes (of the name or of a later word).
    assert_eq!(id("jugg"), Some(8));
    assert_eq!(id("Clock"), Some(51));
    assert_eq!(id("lancer"), Some(12));
    assert_eq!(id("maiden"), Some(5));
    // Ambiguous or unknown.
    assert_eq!(id("shadow"), None); // Shadow Fiend, Shadow Shaman, Shadow Demon
    assert_eq!(id("phantom"), None); // Phantom Assassin, Phantom Lancer
    assert_eq!(id("void"), None); // Faceless Void, Void Spirit
    assert_eq!(id("prophet"), None); // Death Prophet, Nature's Prophet
    assert_eq!(id("x"), None);
    assert_eq!(id(""), None);
    assert_eq!(id("  - "), None);
    assert_eq!(id("definitely not a hero"), None);
}
