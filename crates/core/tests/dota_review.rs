//! Dota companion: post-game review from OpenDota.
//! Fully offline: recorded (anonymised) OpenDota responses in `tests/fixtures/dota/review`,
//! seeded into a temporary cache, and an unreachable local address as the API.
//! Account ids (4000000001 and up) and match ids 70000000xx are made up: no real player.

use std::path::{Path, PathBuf};

use localflow_core::dota::{
    data::OpenDotaSource,
    review::*,
    Evidence,
};
use serde_json::Value;

const ACCOUNT: u64 = 4_000_000_001;
const UNREACHABLE: &str = "http://127.0.0.1:9";

fn fixture_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/dota/review")
}

fn data_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/dota/data")
}

fn load(dir: &Path, file: &str) -> Value {
    serde_json::from_str(&std::fs::read_to_string(dir.join(file)).unwrap()).unwrap()
}

fn fixture(file: &str) -> Value {
    load(&fixture_dir(), file)
}

fn seed(cache: &Path, key: &str, body: &Value, fetched_at: i64) {
    std::fs::create_dir_all(cache).unwrap();
    let entry = serde_json::json!({ "source": "OpenDota", "fetched_at": fetched_at, "body": body });
    std::fs::write(cache.join(format!("{key}.json")), entry.to_string()).unwrap();
}

/// Everything a review needs, cached `age` seconds ago (recent matches: `recent_age`).
fn seed_all(cache: &Path, recent_age: i64) {
    let now = localflow_core::dota::now();
    seed(cache, &format!("recentMatches_{ACCOUNT}"), &fixture("recentMatches.json"), now - recent_age);
    seed(cache, "benchmarks_53_b7", &fixture("benchmarks_53_b7.json"), now - 3600);
    seed(cache, "match_7000000001", &trim_match(fixture("match_parsed.json")), now - 3600);
    seed(cache, "heroes", &load(&data_dir(), "heroes.json"), now - 3600);
    seed(cache, "items", &load(&data_dir(), "items.json"), now - 3600);
    seed(cache, "patch", &load(&data_dir(), "patch.json"), now - 3600);
}

#[test]
fn account_ids() {
    let ok = [
        ("4000000001", 4000000001),
        ("  4000000123 ", 4000000123),
        ("https://www.dotabuff.com/players/4000000123", 4000000123),
        ("dotabuff.com/players/4000000123/matches?enhance=overview", 4000000123),
        ("https://www.opendota.com/players/4000000123", 4000000123),
        ("http://opendota.com/players/4000000123/heroes", 4000000123),
        ("https://stratz.com/players/4000000123", 4000000123),
        ("https://STRATZ.com/players/4000000123#top", 4000000123),
        ("76561201960265851", 4000000123),
        ("https://steamcommunity.com/profiles/76561201960265851/", 4000000123),
        ("[U:1:4000000123]", 4000000123),
        ("<https://www.dotabuff.com/players/4000000123>", 4000000123),
        ("4294967294", 4294967294),
    ];
    for (text, want) in ok {
        assert_eq!(account_id_from(text), Some(want), "{text}");
    }
    let junk = [
        "",
        "   ",
        "hello",
        "0",
        "-5",
        "12.5",
        "4294967295",               // the anonymous id
        "4294967296",               // too big for Steam32, too small for Steam64
        "76561197960265728",        // Steam64 base = account 0
        "999999999999999999999999", // overflow
        "https://www.dotabuff.com/matches/4000000001",
        "https://www.dotabuff.com/players/",
        "https://www.dotabuff.com/players/abc",
        "https://evil.example/players/4000000001",
        "https://dotabuff.com.evil.example/players/4000000001",
        "https://steamcommunity.com/id/somename",
        "4000 000001",
        "[U:1:abc]",
    ];
    for text in junk {
        assert_eq!(account_id_from(text), None, "{text:?}");
    }
}

#[test]
fn parses_recorded_recent_matches() {
    let rows = parse_recent_matches(&fixture("recentMatches.json")).unwrap();
    assert_eq!(rows.len(), 20);
    let first = &rows[0];
    assert_eq!(first.match_id, 7000000001);
    assert_eq!(first.hero_id, 53);
    assert_eq!(first.player_slot, 129);
    assert!(!first.is_radiant());
    assert_eq!(first.won(), Some(true)); // Dire slot, radiant_win false
    assert_eq!((first.kills, first.deaths, first.assists), (17, 1, 14));
    assert_eq!((first.gold_per_min, first.xp_per_min, first.last_hits), (842, 744, 225));
    assert_eq!(first.duration, 1310);
    assert_eq!(first.hero_damage, Some(21912));
    assert_eq!(first.bracket(), Some(7));
    assert!(first.parsed);
    assert!(rows.iter().all(|r| r.duration > 0 && r.start_time > 1_700_000_000));

    let summary = summary_from(first, &[]);
    assert_eq!(summary.hero, "Hero 53");
    assert!(summary.won);
    assert!(summary.items.is_empty());

    assert!(parse_recent_matches(&fixture("recentMatches_empty.json")).unwrap().is_empty());
    assert!(parse_recent_matches(&serde_json::json!({"error": "x"})).is_err());
    // Nulls and missing fields don't break parsing.
    let odd = serde_json::json!([{"match_id": 1, "hero_id": 2, "radiant_win": null, "version": null, "kills": null}]);
    let rows = parse_recent_matches(&odd).unwrap();
    assert_eq!(rows[0].won(), None);
    assert!(!rows[0].parsed);
}

#[test]
fn parses_recorded_benchmarks() {
    let curves = parse_benchmarks(&fixture("benchmarks_53.json")).unwrap();
    for metric in ["gold_per_min", "xp_per_min", "last_hits_per_min", "hero_damage_per_min", "kills_per_min", "deaths_per_min"] {
        let c = &curves[metric];
        assert_eq!(c.len(), 11, "{metric}");
        assert_eq!(c[0].0, 0.1);
        assert_eq!(c[10].0, 0.99);
    }
    // Unknown hero: OpenDota answers null values -> no curves.
    assert!(parse_benchmarks(&fixture("benchmarks_9999.json")).unwrap().is_empty());
    assert!(parse_benchmarks(&serde_json::json!([])).is_err());
}

#[test]
fn percentile_interpolation() {
    let curve = vec![(0.1, 100.0), (0.5, 300.0), (0.9, 500.0), (0.99, 800.0)];
    let at = |v| percentile_at(&curve, v).unwrap();
    assert!((at(100.0) - 0.1).abs() < 1e-9);
    assert!((at(300.0) - 0.5).abs() < 1e-9);
    assert!((at(200.0) - 0.3).abs() < 1e-9);
    assert!((at(400.0) - 0.7).abs() < 1e-9);
    assert!((at(650.0) - 0.945).abs() < 1e-9);
    assert!((at(50.0) - 0.05).abs() < 1e-9); // below the first point: towards (0, 0)
    assert_eq!(at(0.0), 0.0);
    assert_eq!(at(10_000.0), 0.99); // above the last point
    assert_eq!(percentile_at(&[], 5.0), None);
    assert_eq!(percentile_at(&curve, f64::NAN), None);

    // Flat runs (lots of zeros): a tie counts as the top of the run.
    let flat = vec![(0.1, 0.0), (0.2, 0.0), (0.3, 0.0), (0.4, 10.0)];
    assert!((percentile_at(&flat, 0.0).unwrap() - 0.3).abs() < 1e-9);
    assert!((percentile_at(&flat, 5.0).unwrap() - 0.35).abs() < 1e-9);
    // A dip in the data is smoothed, never going backwards.
    let dip = vec![(0.1, 10.0), (0.2, 8.0), (0.3, 20.0)];
    assert!((percentile_at(&dip, 10.0).unwrap() - 0.2).abs() < 1e-9);
    assert!((percentile_at(&dip, 15.0).unwrap() - 0.25).abs() < 1e-9);

    // Against the recorded curves, our interpolation lands close to the percentile OpenDota
    // itself computed for the same player in the same (recorded) match.
    let row = parse_recent_matches(&fixture("recentMatches.json")).unwrap()[0].clone();
    let curves = parse_benchmarks(&fixture("benchmarks_53.json")).unwrap();
    let marks = benchmarks_for(&row, &curves);
    let matched = fixture("match_parsed.json");
    let player = matched["players"].as_array().unwrap().iter().find(|p| p["player_slot"] == 129).unwrap();
    for (label, key) in [
        ("gold per minute", "gold_per_min"),
        ("experience per minute", "xp_per_min"),
        ("last hits per minute", "last_hits_per_min"),
        ("hero damage per minute", "hero_damage_per_min"),
        ("deaths per minute", "deaths_per_min"),
    ] {
        let ours = marks.iter().find(|b| b.metric == label).unwrap();
        let theirs = player["benchmarks"][key]["pct"].as_f64().unwrap();
        let p = ours.percentile.unwrap();
        assert!((p - theirs).abs() < 0.08, "{label}: ours {p}, OpenDota {theirs}");
    }
    let gpm = marks.iter().find(|b| b.metric == "gold per minute").unwrap();
    assert_eq!(gpm.value, 842.0);
    // Deaths are turned around: 1 death in 22 minutes is better than almost everyone.
    let deaths = marks.iter().find(|b| b.metric == "deaths per minute").unwrap();
    assert!(deaths.percentile.unwrap() > 0.9);
    // No curves: values still reported, percentile None.
    let bare = benchmarks_for(&row, &Default::default());
    assert!(bare.len() >= 6 && bare.iter().all(|b| b.percentile.is_none()));
}

#[test]
fn match_items_parsed_and_unparsed() {
    let parsed = fixture("match_parsed.json");
    assert_eq!(parse_match_items(&parsed, 129), Some(vec![149, 63, 36, 263, 20, 158]));
    assert_eq!(parse_match_items(&parsed, 77), None);
    // An unparsed match (no replay) still has final items.
    let unparsed = fixture("match_unparsed.json");
    assert!(unparsed["version"].is_null());
    assert_eq!(parse_match_items(&unparsed, 129), Some(vec![29, 73, 34, 16, 88, 1849]));
    assert_eq!(parse_match_items(&unparsed, 131), Some(vec![]));

    let trimmed = trim_match(parsed.clone());
    assert_eq!(parse_match_items(&trimmed, 129), parse_match_items(&parsed, 129));
    assert_eq!(trimmed["radiant_score"], parsed["radiant_score"]);
    assert!(trimmed["players"][0].get("benchmarks").is_none());
    assert!(trimmed["players"][0].get("account_id").is_none());
    assert!(trimmed.to_string().len() < parsed.to_string().len() / 3);
}

#[test]
fn notes_are_labelled_and_supportive() {
    let row = parse_recent_matches(&fixture("recentMatches.json")).unwrap()[0].clone();
    let curves = parse_benchmarks(&fixture("benchmarks_53_b7.json")).unwrap();
    let marks = benchmarks_for(&row, &curves);
    let ctx = NoteContext {
        hero: "Nature's Prophet".into(),
        roles: vec!["Carry".into()],
        bracket: Some(7),
        benchmarks_fetched_at: Some(1_790_000_000),
        team_kills: Some(40),
    };
    let notes = notes_for(&row, &marks, &ctx);
    assert!((2..=4).contains(&notes.len()), "{notes:#?}");
    let first = &notes[0];
    assert!(first.text.starts_with("Your gold per minute of 842 is around the 80th percentile for Nature's Prophet in Divine games on OpenDota"), "{}", first.text);
    assert!(matches!(&first.evidence, Evidence::Sourced { source, fetched_at, .. } if source == "OpenDota" && *fetched_at == 1_790_000_000));
    // Kill participation from the match details -> a heuristic note.
    assert!(notes.iter().any(|n| matches!(n.evidence, Evidence::Heuristic { .. })), "{notes:#?}");
    for n in &notes {
        let t = n.text.to_lowercase();
        assert!(!t.contains("will win") && !t.contains("guarantee"), "{}", n.text);
    }

    // A rough game without benchmarks: rules of thumb only, deaths called out kindly.
    let rough = RecentMatch { kills: 1, deaths: 12, assists: 3, duration: 35 * 60, gold_per_min: 300, xp_per_min: 350, last_hits: 60, ..Default::default() };
    let notes = notes_for(&rough, &benchmarks_for(&rough, &Default::default()), &NoteContext { hero: "Lion".into(), ..Default::default() });
    assert!((2..=4).contains(&notes.len()), "{notes:#?}");
    assert!(notes.iter().all(|n| matches!(n.evidence, Evidence::Heuristic { .. })));
    assert!(notes.iter().any(|n| n.text.contains("12 deaths")));

    // Low farm on a support hero is explained, not criticised.
    let curve = |a: f64, b: f64| vec![(0.1, a), (0.5, (a + b) / 2.0), (0.99, b)];
    let mut sup_curves = std::collections::HashMap::new();
    sup_curves.insert("gold_per_min".to_string(), curve(250.0, 600.0));
    sup_curves.insert("xp_per_min".to_string(), curve(300.0, 700.0));
    sup_curves.insert("assists_per_min".to_string(), curve(0.2, 0.9));
    let support = RecentMatch { kills: 2, deaths: 5, assists: 25, duration: 40 * 60, gold_per_min: 240, xp_per_min: 520, last_hits: 30, ..Default::default() };
    let ctx = NoteContext { hero: "Lion".into(), roles: vec!["Support".into(), "Disabler".into()], ..Default::default() };
    let notes = notes_for(&support, &benchmarks_for(&support, &sup_curves), &ctx);
    let farm = notes.iter().find(|n| n.text.contains("gold per minute")).expect("a gold note");
    assert!(farm.text.contains("support"), "{}", farm.text);
    assert!(farm.text.contains("across all ranks"), "{}", farm.text);
}

#[test]
fn private_and_empty_profiles() {
    let setting = "Expose Public Match Data";
    let private = explain_empty(ACCOUNT, Some(&fixture("player_private.json")), false);
    assert!(private.contains("private") && private.contains(setting), "{private}");
    let unknown = explain_empty(ACCOUNT, None, true);
    assert!(unknown.contains("doesn't know account 4000000001") && unknown.contains(setting), "{unknown}");
    let no_profile = explain_empty(ACCOUNT, Some(&serde_json::json!({"profile": null})), false);
    assert!(no_profile.contains("doesn't know"), "{no_profile}");
    let public_but_empty = explain_empty(ACCOUNT, Some(&serde_json::json!({"profile": {"fh_unavailable": false}})), false);
    assert!(public_but_empty.contains("few minutes") && public_but_empty.contains(setting), "{public_but_empty}");

    // Through the source: an empty (cached) match list becomes the friendly error.
    let dir = tempfile::tempdir().unwrap();
    let cache = dir.path().join("cache");
    let now = localflow_core::dota::now();
    seed(&cache, &format!("recentMatches_{ACCOUNT}"), &fixture("recentMatches_empty.json"), now - 10);
    seed(&cache, &format!("player_{ACCOUNT}"), &fixture("player_private.json"), now - 10);
    let src = OpenDotaSource::with_base_url(UNREACHABLE).with_cache_dir(cache);
    let err = review_last_match_with(&src, ACCOUNT).unwrap_err();
    assert!(err.contains("private") && err.contains(setting), "{err}");
    let err = recent_matches_with(&src, ACCOUNT, 5).unwrap_err();
    assert!(err.contains(setting), "{err}");
    // Invalid ids never reach the network.
    assert!(review_last_match_with(&src, 0).unwrap_err().contains("not a valid"));
    assert!(review_last_match_with(&src, 4_294_967_295).unwrap_err().contains("not a valid"));
}

#[test]
fn review_from_fresh_cache() {
    let dir = tempfile::tempdir().unwrap();
    let cache = dir.path().join("cache");
    seed_all(&cache, 30);
    // Everything is fresh: the unreachable server is never needed.
    let src = OpenDotaSource::with_base_url(UNREACHABLE).with_cache_dir(cache);

    let list = recent_matches_with(&src, ACCOUNT, 5).unwrap();
    assert_eq!(list.len(), 5);
    assert_eq!(list[0].hero, "Nature's Prophet");
    assert_eq!(recent_matches_with(&src, ACCOUNT, 100).unwrap().len(), 20);

    let review = review_last_match_with(&src, ACCOUNT).unwrap();
    let s = &review.summary;
    assert_eq!(s.match_id, 7000000001);
    assert_eq!(s.hero, "Nature's Prophet");
    assert!(s.won);
    assert_eq!((s.kills, s.deaths, s.assists, s.gpm, s.xpm, s.last_hits), (17, 1, 14, 842, 744, 225));
    assert_eq!(s.items.len(), 6);
    assert!(s.items.iter().all(|i| !i.starts_with("Item ")), "{:?}", s.items);
    assert!(review.benchmarks.iter().all(|b| b.percentile.is_some()), "{:#?}", review.benchmarks);
    assert!((2..=4).contains(&review.notes.len()));
    assert!(review.notes.iter().any(|n| matches!(n.evidence, Evidence::Sourced { .. })));
    assert!(review.data_note.contains("Divine"), "{}", review.data_note);
    assert!(!review.data_note.contains("Offline"), "{}", review.data_note);
    let (offline, _) = src.offline_notes();
    assert!(!offline);
}

#[test]
fn offline_serves_stale_cache() {
    // The only test in this binary that touches LOCALFLOW_DOTA_DIR.
    let dir = tempfile::tempdir().unwrap();
    std::env::set_var("LOCALFLOW_DOTA_DIR", dir.path());
    seed_all(&dir.path().join("cache"), 3 * 86400); // match list three days old: stale

    let src = OpenDotaSource::with_base_url(UNREACHABLE);
    let review = review_last_match_with(&src, ACCOUNT).unwrap();
    assert_eq!(review.summary.hero, "Nature's Prophet");
    assert_eq!(review.summary.items.len(), 6); // fresh match details still used
    assert!(review.data_note.contains("Offline"), "{}", review.data_note);
    assert!(review.data_note.contains("Could not refresh your recent matches"), "{}", review.data_note);
    assert!(review.data_note.contains("3 days ago"), "{}", review.data_note);
    let (offline, notes) = src.offline_notes();
    assert!(offline && !notes.is_empty());

    // Nothing cached for another account: a clear error.
    let err = review_last_match_with(&OpenDotaSource::with_base_url(UNREACHABLE), 4_000_000_002).unwrap_err();
    assert!(err.contains("no cached copy"), "{err}");
    std::env::remove_var("LOCALFLOW_DOTA_DIR");
}
