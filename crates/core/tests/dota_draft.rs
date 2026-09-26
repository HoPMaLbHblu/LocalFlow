//! Draft state: merging screenshots, corrections, the player's hero and team, saving.

use localflow_core::dota::{DraftState, PickSource, Recognition, RecognizedPick, Side, Team, UNCERTAIN};

fn pick(team: Team, slot: u8, hero_id: u32, confidence: f32) -> RecognizedPick {
    RecognizedPick { team, slot, hero_id, confidence, alternatives: vec![(hero_id + 100, confidence / 2.0)] }
}

fn rec(picks: Vec<RecognizedPick>) -> Recognition {
    Recognition { picks, layout: "draft 16:9 (16:9)".into(), width: 1920, height: 1080, warnings: Vec::new() }
}

#[test]
fn incremental_updates_across_three_captures() {
    let mut d = DraftState::default();
    assert!(d.team_assumed());
    // Capture 1: two heroes, one of them unsure.
    d.merge(&rec(vec![pick(Team::Radiant, 0, 1, 0.95), pick(Team::Dire, 2, 2, 0.4)]));
    assert_eq!(d.captures, 1);
    assert_eq!(d.allies[0].hero_id, Some(1));
    assert_eq!(d.enemies[2].hero_id, Some(2));
    assert_eq!(d.allies[0].source, Some(PickSource::Screenshot));
    assert_eq!(d.uncertain(), vec![(Side::Enemies, 2)]);
    assert!(d.updated_at > 0);

    // Capture 2: nothing where hero 1 was (empty reading), a better reading of slot Dire 2,
    // a less confident different reading of Radiant 0, and new heroes.
    d.merge(&rec(vec![
        pick(Team::Dire, 2, 3, 0.9),
        pick(Team::Radiant, 0, 9, 0.5),
        pick(Team::Radiant, 1, 4, 0.8),
        pick(Team::Dire, 0, 5, 0.85),
    ]));
    assert_eq!(d.captures, 2);
    assert_eq!(d.allies[0].hero_id, Some(1), "a weaker reading must not replace a stronger one");
    assert_eq!(d.enemies[2].hero_id, Some(3), "a stronger reading replaces a weaker one");
    assert_eq!(d.allies[1].hero_id, Some(4));
    assert_eq!(d.enemies[0].hero_id, Some(5));
    assert!(d.uncertain().is_empty());

    // Capture 3: an empty reading changes nothing but the counter; then the rest arrives.
    let before = d.clone();
    d.merge(&rec(vec![]));
    assert_eq!(d.captures, 3);
    assert_eq!((d.allies.clone(), d.enemies.clone()), (before.allies, before.enemies));
    assert!(!d.is_complete());
    d.merge(&rec(vec![
        pick(Team::Radiant, 2, 6, 0.9),
        pick(Team::Radiant, 3, 7, 0.9),
        pick(Team::Radiant, 4, 8, 0.9),
        pick(Team::Dire, 1, 10, 0.9),
        pick(Team::Dire, 3, 11, 0.9),
        pick(Team::Dire, 4, 12, 0.55),
    ]));
    assert!(d.is_complete());
    assert_eq!(d.picked().len(), 10);
    assert_eq!(d.uncertain(), vec![(Side::Enemies, 4)]);
    // The same hero again with more confidence just raises the confidence.
    d.merge(&rec(vec![pick(Team::Dire, 4, 12, 0.9)]));
    assert!(d.enemies[4].confidence >= UNCERTAIN);
}

#[test]
fn manual_corrections_are_never_overwritten() {
    let mut d = DraftState::default();
    d.merge(&rec(vec![pick(Team::Radiant, 0, 1, 0.5), pick(Team::Dire, 1, 2, 0.9)]));
    // The player fixes Radiant 0 to hero 2: hero 2 leaves the enemy slot.
    d.correct(Side::Allies, 0, Some(2));
    assert_eq!(d.allies[0].hero_id, Some(2));
    assert_eq!(d.allies[0].source, Some(PickSource::Manual));
    assert_eq!(d.allies[0].confidence, 1.0);
    assert_eq!(d.enemies[1].hero_id, None);
    // A very confident screenshot disagrees: the correction stays.
    d.merge(&rec(vec![pick(Team::Radiant, 0, 1, 1.0)]));
    assert_eq!(d.allies[0].hero_id, Some(2));
    // A screenshot can't move the corrected hero elsewhere either.
    d.merge(&rec(vec![pick(Team::Dire, 3, 2, 1.0)]));
    assert_eq!(d.enemies[3].hero_id, None);
    assert_eq!(d.allies[0].hero_id, Some(2));
    // Clearing a slot empties it; screenshots may fill it again.
    d.correct(Side::Allies, 0, None);
    assert_eq!(d.allies[0].hero_id, None);
    d.merge(&rec(vec![pick(Team::Radiant, 0, 1, 0.7)]));
    assert_eq!(d.allies[0].hero_id, Some(1));
    // Out-of-range slots are ignored.
    d.correct(Side::Allies, 7, Some(3));
    assert!(!d.picked().contains(&3));
}

#[test]
fn team_mapping_for_a_dire_player() {
    let mut d = DraftState { player_team: Some(Team::Dire), ..Default::default() };
    assert!(!d.team_assumed());
    d.merge(&rec(vec![pick(Team::Radiant, 0, 1, 0.9), pick(Team::Dire, 4, 2, 0.9)]));
    assert_eq!(d.enemies[0].hero_id, Some(1));
    assert_eq!(d.allies[4].hero_id, Some(2));

    // Unknown team: Radiant assumed; learning the team later swaps the sides.
    let mut d = DraftState::default();
    d.merge(&rec(vec![pick(Team::Radiant, 0, 1, 0.9), pick(Team::Dire, 4, 2, 0.9)]));
    assert_eq!(d.allies[0].hero_id, Some(1));
    d.set_player_team(Some(Team::Dire));
    assert_eq!(d.enemies[0].hero_id, Some(1));
    assert_eq!(d.allies[4].hero_id, Some(2));
    // Setting the same team again changes nothing.
    d.set_player_team(Some(Team::Dire));
    assert_eq!(d.allies[4].hero_id, Some(2));
}

#[test]
fn duplicates_keep_the_more_confident_slot() {
    let mut d = DraftState::default();
    // Same hero in two slots of one capture.
    d.merge(&rec(vec![pick(Team::Radiant, 1, 5, 0.6), pick(Team::Dire, 3, 5, 0.9)]));
    assert_eq!(d.find(5), Some((Side::Enemies, 3)));
    assert_eq!(d.allies[1].hero_id, None);
    // A later, weaker reading elsewhere doesn't move it; a stronger one does.
    d.merge(&rec(vec![pick(Team::Radiant, 2, 5, 0.7)]));
    assert_eq!(d.find(5), Some((Side::Enemies, 3)));
    d.merge(&rec(vec![pick(Team::Radiant, 2, 5, 0.97)]));
    assert_eq!(d.find(5), Some((Side::Allies, 2)));
    assert_eq!(d.enemies[3].hero_id, None);
    assert_eq!(d.picked().len(), 1);
}

#[test]
fn player_hero_sits_in_an_allied_slot() {
    let mut d = DraftState::default();
    d.merge(&rec(vec![pick(Team::Dire, 0, 7, 0.8), pick(Team::Radiant, 0, 1, 0.9)]));
    // GSI says the player is hero 7, which a screenshot put among the enemies.
    d.set_player_hero(Some(7), PickSource::Gsi);
    assert_eq!(d.player_hero, Some(7));
    let (side, slot) = d.find(7).unwrap();
    assert_eq!(side, Side::Allies);
    assert_eq!(d.allies[slot as usize].source, Some(PickSource::Gsi));
    assert_eq!(d.enemies[0].hero_id, None);
    assert_eq!(d.allies[0].hero_id, Some(1), "an existing ally isn't displaced when a slot is free");
    // Screenshots never overwrite it.
    d.merge(&rec(vec![pick(Team::Radiant, slot, 3, 1.0)]));
    assert_eq!(d.allies[slot as usize].hero_id, Some(7));
    // Already allied: just marked.
    let mut d = DraftState::default();
    d.merge(&rec(vec![pick(Team::Radiant, 3, 4, 0.4)]));
    d.set_player_hero(Some(4), PickSource::Manual);
    assert_eq!(d.allies[3].hero_id, Some(4));
    assert_eq!(d.allies[3].source, Some(PickSource::Manual));
    assert!(d.uncertain().is_empty());
    // All allied slots taken by screenshots: the least confident guess makes way.
    let mut d = DraftState::default();
    d.merge(&rec((0..5).map(|i| pick(Team::Radiant, i, 10 + i as u32, if i == 2 { 0.3 } else { 0.9 })).collect()));
    d.set_player_hero(Some(99), PickSource::Gsi);
    assert_eq!(d.allies[2].hero_id, Some(99));
    d.set_player_hero(None, PickSource::Gsi);
    assert_eq!(d.player_hero, None);
}

#[test]
fn save_load_and_a_corrupt_file() {
    let dir = tempfile::TempDir::new().unwrap();
    std::env::set_var("LOCALFLOW_DOTA_DIR", dir.path());
    // Nothing saved yet.
    assert_eq!(DraftState::load(), DraftState::default());

    let mut d = DraftState::default();
    d.merge(&rec(vec![pick(Team::Radiant, 0, 1, 0.9), pick(Team::Dire, 1, 2, 0.45)]));
    d.correct(Side::Enemies, 4, Some(3));
    d.set_player_team(Some(Team::Radiant));
    d.save().unwrap();
    assert!(dir.path().join("draft.json").is_file());
    assert!(!dir.path().join("draft.tmp").exists());
    assert_eq!(DraftState::load(), d);

    // A corrupt file: fresh state, and the file is kept, not deleted.
    std::fs::write(dir.path().join("draft.json"), "{ \"allies\": [ broken").unwrap();
    let fresh = DraftState::load();
    assert_eq!(fresh, DraftState::default());
    let kept: Vec<_> = std::fs::read_dir(dir.path())
        .unwrap()
        .filter_map(|e| e.ok())
        .map(|e| e.file_name().to_string_lossy().to_string())
        .filter(|n| n.starts_with("draft.corrupt-"))
        .collect();
    assert_eq!(kept.len(), 1, "{kept:?}");
    assert_eq!(std::fs::read_to_string(dir.path().join(&kept[0])).unwrap(), "{ \"allies\": [ broken");
    assert!(!dir.path().join("draft.json").exists());
    // Saving again works.
    fresh.save().unwrap();
    assert_eq!(DraftState::load(), fresh);

    d.reset();
    assert!(d.picked().is_empty() && d.captures == 0 && d.player_team.is_none());
    std::env::remove_var("LOCALFLOW_DOTA_DIR");
}
