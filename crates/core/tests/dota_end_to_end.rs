//! The whole companion on real data: OpenDota statistics, Valve's hero portraits, two
//! screenshots built from those portraits, the draft growing between them, a correction,
//! then suggestions and item plans for contrasting enemy teams. Needs the internet.
//!
//! cargo test -p localflow-core --test dota_end_to_end -- --ignored --nocapture

use image::{imageops, Rgb, RgbImage};
use localflow_core::dota::{
    data::default_source, recommend, vision, DraftState, Evidence, Hero, PickSource, Role, Side, Slot, Team, UNCERTAIN,
};

fn hero<'a>(heroes: &'a [Hero], short: &str) -> &'a Hero {
    heroes.iter().find(|h| h.short_name() == short).unwrap_or_else(|| panic!("no hero {short}"))
}

/// A 1920x1080 "draft screen": dark background, the given heroes pasted into the layout's slots.
fn screenshot(layout: &vision::Layout, portraits_dir: &std::path::Path, picks: &[(Team, u8, &Hero)]) -> RgbImage {
    let (w, h) = (1920u32, 1080u32);
    let mut img = RgbImage::from_pixel(w, h, Rgb([18, 22, 28]));
    for slot in &layout.slots {
        let Some((_, _, hero)) = picks.iter().find(|p| p.0 == slot.team && p.1 == slot.slot) else { continue };
        let art = image::open(portraits_dir.join(format!("{}.png", hero.short_name()))).unwrap().to_rgb8();
        let (sw, sh) = ((slot.w * h as f32).round() as u32, (slot.h * h as f32).round() as u32);
        let tile = imageops::resize(&art, sw, sh, imageops::FilterType::Triangle);
        let x0 = (w as f32 / 2.0 + slot.x * h as f32).round() as i64;
        let y0 = (slot.y * h as f32).round() as i64;
        imageops::replace(&mut img, &tile, x0, y0);
    }
    img
}

fn names(heroes: &[Hero], ids: impl IntoIterator<Item = u32>) -> Vec<String> {
    ids.into_iter().map(|id| heroes.iter().find(|h| h.id == id).map(|h| h.localized_name.clone()).unwrap_or(id.to_string())).collect()
}

#[test]
#[ignore = "uses the internet (OpenDota and Valve's image server)"]
fn draft_to_items_on_real_data() {
    let dir = tempfile::TempDir::new().unwrap();
    std::env::set_var("LOCALFLOW_DOTA_DIR", dir.path());
    let src = default_source();
    let heroes = src.heroes().expect("hero list");
    let info = src.info();
    println!("source: {} patch {:?} offline={}", info.name, info.patch, info.offline);

    let portraits = vision::load_portraits(&heroes).expect("portraits");
    println!("portraits: {} loaded, missing {:?}", portraits.count, portraits.missing);
    let layout = vision::default_layouts().into_iter().find(|l| l.name == "draft 16:9").expect("draft layout");
    let pdir = dir.path().join("portraits");

    // ---- incremental draft: two captures ------------------------------------------------------
    let (jug, lion, axe, cm, pudge) = (hero(&heroes, "juggernaut"), hero(&heroes, "lion"), hero(&heroes, "axe"), hero(&heroes, "crystal_maiden"), hero(&heroes, "pudge"));
    let (pl, naga, tb, ck, meepo) = (hero(&heroes, "phantom_lancer"), hero(&heroes, "naga_siren"), hero(&heroes, "terrorblade"), hero(&heroes, "chaos_knight"), hero(&heroes, "meepo"));
    let mut draft = DraftState::default();
    draft.set_player_team(Some(Team::Dire)); // the player is Dire: the right half is "allies"

    let first = screenshot(&layout, &pdir, &[(Team::Dire, 0, jug), (Team::Dire, 1, lion), (Team::Radiant, 0, pl), (Team::Radiant, 1, naga)]);
    let rec1 = vision::recognize(&first, &portraits);
    draft.merge(&rec1);
    println!("capture 1: {} picks, allies {:?}, enemies {:?}", rec1.picks.len(),
        names(&heroes, draft.allies.iter().filter_map(|s| s.hero_id)), names(&heroes, draft.enemies.iter().filter_map(|s| s.hero_id)));
    assert_eq!(draft.allies.iter().filter(|s| s.hero_id.is_some()).count(), 2);
    assert_eq!(draft.enemies.iter().filter(|s| s.hero_id.is_some()).count(), 2);

    // Second capture: everything picked. The player corrects one slot before it.
    draft.correct(Side::Allies, 4, Some(pudge.id));
    let second = screenshot(&layout, &pdir, &[
        (Team::Dire, 0, jug), (Team::Dire, 1, lion), (Team::Dire, 2, axe), (Team::Dire, 3, cm), (Team::Dire, 4, jug /* screen says jug, player said Pudge */),
        (Team::Radiant, 0, pl), (Team::Radiant, 1, naga), (Team::Radiant, 2, tb), (Team::Radiant, 3, ck), (Team::Radiant, 4, meepo),
    ]);
    let rec2 = vision::recognize(&second, &portraits);
    draft.merge(&rec2);
    let allies = names(&heroes, draft.allies.iter().filter_map(|s| s.hero_id));
    let enemies = names(&heroes, draft.enemies.iter().filter_map(|s| s.hero_id));
    println!("capture 2: {} picks, captures={}, allies {allies:?}, enemies {enemies:?}, uncertain {:?}", rec2.picks.len(), draft.captures, draft.uncertain());
    assert_eq!(draft.captures, 2);
    assert_eq!(draft.allies[4].hero_id, Some(pudge.id), "the manual correction survives the next capture");
    assert_eq!(draft.allies[4].source, Some(PickSource::Manual));
    assert_eq!(enemies.len(), 5);
    for s in draft.enemies.iter() {
        assert!(s.confidence >= UNCERTAIN, "{s:?}");
    }

    // ---- stage 2: suggestions for contrasting enemy teams ---------------------------------------
    let team = |shorts: [&str; 5]| -> [Slot; 5] {
        shorts.map(|s| Slot { hero_id: Some(hero(&heroes, s).id), confidence: 1.0, source: Some(PickSource::Manual), alternatives: vec![] })
    };
    let lineups = [
        ("illusions", ["phantom_lancer", "naga_siren", "terrorblade", "chaos_knight", "meepo"]),
        ("magic burst", ["lion", "lina", "zuus", "skywrath_mage", "leshrac"]),
        ("healing", ["necrolyte", "huskar", "omniknight", "witch_doctor", "alchemist"]),
    ];
    let mut tops = Vec::new();
    for (label, lineup) in lineups {
        let mut d = DraftState { enemies: team(lineup), role: Some(Role::Carry), ..Default::default() };
        d.allies[0] = Slot { hero_id: Some(hero(&heroes, "crystal_maiden").id), confidence: 1.0, source: Some(PickSource::Manual), alternatives: vec![] };
        let out = recommend::suggest(&*src, &d, Some(Role::Carry), 3);
        println!("\n== vs {label} (carry): {}", out.note);
        for s in &out.heroes {
            println!("  {} ({:.2})", s.hero, s.score);
            for r in &s.reasons {
                let tag = match &r.evidence { Evidence::Sourced { source, .. } => format!("DATA {source}"), Evidence::Heuristic { .. } => "RULE".into() };
                println!("     [{tag}] {}", r.text);
            }
        }
        tops.push(out.heroes.iter().map(|h| h.hero_id).collect::<Vec<_>>());

        // ---- stage 3: the item plan for one fixed hero against this team ------------------------
        d.player_hero = Some(hero(&heroes, "juggernaut").id);
        let plan = recommend::item_plan(&*src, d.player_hero.unwrap(), &d).expect("item plan");
        let list = |v: &[localflow_core::dota::ItemAdvice]| v.iter().map(|i| format!("{}#{}", i.item, i.priority)).collect::<Vec<_>>().join(", ");
        println!("  Juggernaut items — start: {} | core: {} | situational: {}", list(&plan.starting), list(&plan.core), list(&plan.situational));
        for a in &plan.adaptations {
            println!("     adapt: {}", a.text);
        }
        println!("     data: {}", plan.data_note);
        assert!(!plan.core.is_empty());
    }
    assert!(tops[0] != tops[1] || tops[1] != tops[2], "contrasting teams should not all give the same suggestions");
}
