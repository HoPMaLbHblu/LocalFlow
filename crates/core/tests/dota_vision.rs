//! Hero recognition on synthetic screenshots (no Valve artwork): made-up "portraits" are
//! composited into full screenshots at the layout positions, with the distortions a real
//! capture has (scaling, slanted masks, dark gradients, brightness, blur, JPEG), and some
//! slots left empty.
//!
//! The live test downloads the real portraits into a temp folder:
//! cargo test -p localflow-core --test dota_vision -- --ignored --nocapture

use std::{collections::HashMap, io::Cursor, time::Instant};

use image::{imageops, ImageBuffer, Rgb, RgbImage};
use localflow_core::dota::{
    vision::{self, candidate_regions, default_layouts, recognize_with, Layout, Portraits, Region},
    Hero, Recognition, Team, UNCERTAIN,
};

// ---- synthetic material ------------------------------------------------------------------------

struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }
    fn f(&mut self) -> f32 {
        (self.next() % 1_000_000) as f32 / 1_000_000.0
    }
    fn range(&mut self, a: f32, b: f32) -> f32 {
        a + (b - a) * self.f()
    }
    fn color(&mut self) -> [f32; 3] {
        [self.range(0.0, 255.0), self.range(0.0, 255.0), self.range(0.0, 255.0)]
    }
}

/// A distinctive 256x144 picture: a gradient with random circles, bars and stripes.
fn fake_portrait(seed: u64) -> RgbImage {
    let mut rng = Rng(seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1);
    for _ in 0..8 {
        rng.next();
    }
    let (c0, c1) = (rng.color(), rng.color());
    let mut shapes = Vec::new();
    for _ in 0..9 {
        let kind = rng.next() % 3;
        shapes.push((kind, rng.range(0.0, 256.0), rng.range(0.0, 144.0), rng.range(12.0, 60.0), rng.range(8.0, 40.0), rng.color()));
    }
    ImageBuffer::from_fn(256, 144, |x, y| {
        let t = (x as f32 / 256.0 + y as f32 / 144.0) / 2.0;
        let mut c = [c0[0] + (c1[0] - c0[0]) * t, c0[1] + (c1[1] - c0[1]) * t, c0[2] + (c1[2] - c0[2]) * t];
        for &(kind, sx, sy, a, b, col) in &shapes {
            let (dx, dy) = (x as f32 - sx, y as f32 - sy);
            let inside = match kind {
                0 => dx * dx + dy * dy < a * a,
                1 => dx.abs() < a && dy.abs() < b / 2.0,
                _ => (((x as f32 + y as f32 * 0.5) / (b / 2.0 + 4.0)) as i32 % 2 == 0) && dx.abs() < a * 1.5 && dy.abs() < a,
            };
            if inside {
                c = col;
            }
        }
        Rgb([c[0] as u8, c[1] as u8, c[2] as u8])
    })
}

fn fake_heroes(n: u32) -> Vec<(u32, RgbImage)> {
    (1..=n).map(|id| (id, fake_portrait(id as u64 * 7919))).collect()
}

/// A dark, slightly noisy "game" background with a few soft blobs.
fn background(w: u32, h: u32, seed: u64) -> RgbImage {
    let mut rng = Rng(seed | 1);
    let blobs: Vec<_> = (0..6).map(|_| (rng.range(0.0, w as f32), rng.range(0.0, h as f32), rng.range(100.0, 500.0), rng.color())).collect();
    let mut img = ImageBuffer::from_fn(w, h, |x, y| {
        let mut c = [22.0f32, 27.0, 35.0];
        for &(bx, by, r, col) in &blobs {
            let d = ((x as f32 - bx).powi(2) + (y as f32 - by).powi(2)).sqrt();
            let a = (1.0 - d / r).max(0.0) * 0.35;
            for k in 0..3 {
                c[k] = c[k] * (1.0 - a) + col[k] * a;
            }
        }
        Rgb([c[0] as u8, c[1] as u8, c[2] as u8])
    });
    for p in img.pixels_mut() {
        let n = (rng.next() % 9) as i32 - 4;
        for k in 0..3 {
            p.0[k] = (p.0[k] as i32 + n).clamp(0, 255) as u8;
        }
    }
    img
}

#[derive(Clone, Copy)]
struct Distort {
    brightness: f32,
    jitter_px: i32,
    blur: f32,
    jpeg: Option<u8>,
}

const MILD: Distort = Distort { brightness: 0.15, jitter_px: 1, blur: 0.6, jpeg: Some(75) };

/// Puts `picks` (team, slot, hero index) into a screenshot with `layout`, the rest empty.
#[allow(clippy::too_many_arguments)]
fn compose(w: u32, h: u32, region: Region, layout: &Layout, heroes: &[(u32, RgbImage)], picks: &[(Team, u8, Option<u32>)], d: Distort, seed: u64) -> RgbImage {
    let mut img = background(w, h, seed);
    let mut rng = Rng(seed.wrapping_add(99) | 1);
    let top_bar = layout.name.contains("top bar");
    for slot in &layout.slots {
        let rh = region.h as f32;
        let cx = region.x as f32 + region.w as f32 / 2.0;
        let jx = (rng.next() % (2 * d.jitter_px as u64 + 1)) as i32 - d.jitter_px;
        let jy = (rng.next() % (2 * d.jitter_px as u64 + 1)) as i32 - d.jitter_px;
        let x0 = (cx + slot.x * rh).round() as i32 + jx;
        let y0 = (region.y as f32 + slot.y * rh).round() as i32 + jy;
        let sw = (slot.w * rh).round().max(2.0) as u32;
        let sh = (slot.h * rh).round().max(2.0) as u32;
        let hero = picks.iter().find(|p| p.0 == slot.team && p.1 == slot.slot).and_then(|p| p.2);
        let tile: RgbImage = match hero {
            Some(id) => {
                let src = &heroes.iter().find(|h| h.0 == id).unwrap().1;
                let mut t = imageops::resize(src, sw, sh, imageops::FilterType::Triangle);
                let b = 1.0 + rng.range(-d.brightness, d.brightness);
                for p in t.pixels_mut() {
                    for k in 0..3 {
                        p.0[k] = (p.0[k] as f32 * b).clamp(0.0, 255.0) as u8;
                    }
                }
                t
            }
            // Empty slots: the game's smooth placeholder gradients.
            None if top_bar => ImageBuffer::from_fn(sw, sh, |_, y| {
                let t = y as f32 / sh as f32;
                let v = if t < 0.2 { 51.0 + 17.0 * t / 0.2 } else if t < 0.5 { 68.0 - 17.0 * (t - 0.2) / 0.3 } else { 51.0 - 34.0 * (t - 0.5) / 0.5 };
                Rgb([v as u8, v as u8, v as u8])
            }),
            None => ImageBuffer::from_fn(sw, sh, |x, y| {
                let (dx, dy) = (x as f32 / sw as f32 - 0.5, y as f32 / sh as f32 - 0.9);
                let t = ((dx * dx + dy * dy).sqrt() / 0.8).min(1.0);
                let a = [0x44 as f32, 0x4d as f32, 0x5a as f32];
                let b = [0x16 as f32, 0x1b as f32, 0x23 as f32];
                Rgb([(a[0] + (b[0] - a[0]) * t) as u8, (a[1] + (b[1] - a[1]) * t) as u8, (a[2] + (b[2] - a[2]) * t) as u8])
            }),
        };
        for ty in 0..sh {
            for tx in 0..sw {
                // Slanted outer edge (the game's opacity mask).
                let fx = tx as f32 / sw as f32;
                let fy = ty as f32 / sh as f32;
                let cut = 0.1 * (1.0 - fy);
                let slanted = match slot.team {
                    Team::Radiant => fx < cut,
                    Team::Dire => fx > 1.0 - cut,
                };
                if slanted {
                    continue;
                }
                let (px, py) = (x0 + tx as i32, y0 + ty as i32);
                if px < 0 || py < 0 || px >= w as i32 || py >= h as i32 {
                    continue;
                }
                let mut c = tile.get_pixel(tx, ty).0;
                // Dark top/bottom gradient on the draft screen.
                if !top_bar {
                    let dark = if fy < 0.1 { 0.67 * (1.0 - fy / 0.1) } else if fy > 0.9 { 0.53 * (fy - 0.9) / 0.1 } else { 0.0 };
                    for v in c.iter_mut() {
                        *v = (*v as f32 * (1.0 - dark)) as u8;
                    }
                }
                img.put_pixel(px as u32, py as u32, Rgb(c));
            }
        }
    }
    finish(img, d)
}

fn finish(mut img: RgbImage, d: Distort) -> RgbImage {
    if d.blur > 0.0 {
        img = imageops::blur(&img, d.blur);
    }
    if let Some(q) = d.jpeg {
        let mut bytes = Vec::new();
        image::codecs::jpeg::JpegEncoder::new_with_quality(Cursor::new(&mut bytes), q).encode_image(&img).unwrap();
        img = image::load_from_memory(&bytes).unwrap().to_rgb8();
    }
    img
}

fn layout(name: &str) -> Layout {
    default_layouts().into_iter().find(|l| l.name == name).unwrap()
}

fn full(w: u32, h: u32) -> Region {
    Region { x: 0, y: 0, w, h }
}

/// Picks: Radiant slots 0-4 and Dire slots 0-4 with the given heroes (None = empty).
fn picks(radiant: [Option<u32>; 5], dire: [Option<u32>; 5]) -> Vec<(Team, u8, Option<u32>)> {
    let mut v = Vec::new();
    for (i, h) in radiant.iter().enumerate() {
        v.push((Team::Radiant, i as u8, *h));
    }
    for (i, h) in dire.iter().enumerate() {
        v.push((Team::Dire, i as u8, *h));
    }
    v
}

struct Score {
    correct: usize,
    wrong: usize,
    missed: usize,
    phantom: usize,
    min_conf_correct: f32,
    max_conf_wrong: f32,
}

fn score(rec: &Recognition, expected: &[(Team, u8, Option<u32>)]) -> Score {
    let got: HashMap<(Team, u8), (u32, f32)> = rec.picks.iter().map(|p| ((p.team, p.slot), (p.hero_id, p.confidence))).collect();
    let mut s = Score { correct: 0, wrong: 0, missed: 0, phantom: 0, min_conf_correct: 1.0, max_conf_wrong: 0.0 };
    for &(team, slot, hero) in expected {
        match (hero, got.get(&(team, slot))) {
            (Some(h), Some(&(g, c))) if g == h => {
                s.correct += 1;
                s.min_conf_correct = s.min_conf_correct.min(c);
            }
            (Some(_), Some(&(_, c))) => {
                s.wrong += 1;
                s.max_conf_wrong = s.max_conf_wrong.max(c);
            }
            (Some(_), None) => s.missed += 1,
            (None, Some(&(_, c))) => {
                s.phantom += 1;
                s.max_conf_wrong = s.max_conf_wrong.max(c);
            }
            (None, None) => {}
        }
    }
    s
}

fn report(label: &str, rec: &Recognition, s: &Score) {
    println!(
        "{label}: layout={:?} correct={} wrong={} missed={} phantom={} min_conf_correct={:.2} max_conf_wrong={:.2} warnings={:?}",
        rec.layout, s.correct, s.wrong, s.missed, s.phantom, s.min_conf_correct, s.max_conf_wrong, rec.warnings
    );
}

fn run_case(label: &str, w: u32, h: u32, layout_name: &str, expected: &[(Team, u8, Option<u32>)], d: Distort, seed: u64) -> (Recognition, Score) {
    let heroes = fake_heroes(20);
    let portraits = Portraits::from_images(heroes.clone());
    let shot = compose(w, h, full(w, h), &layout(layout_name), &heroes, expected, d, seed);
    let start = Instant::now();
    let rec = recognize_with(&shot, &portraits, &default_layouts(), None);
    let s = score(&rec, expected);
    report(&format!("{label} ({} ms)", start.elapsed().as_millis()), &rec, &s);
    assert!(rec.layout.starts_with(layout_name), "{label}: chose {}", rec.layout);
    (rec, s)
}

fn assert_perfect(label: &str, s: &Score) {
    assert_eq!((s.wrong, s.missed, s.phantom), (0, 0, 0), "{label}");
    assert!(s.min_conf_correct >= UNCERTAIN, "{label}: a correct pick had low confidence {}", s.min_conf_correct);
}

// ---- tests ---------------------------------------------------------------------------------------

#[test]
fn draft_1080p_with_empty_slots() {
    let expected = picks([Some(3), None, Some(7), Some(12), None], [Some(1), Some(19), None, Some(5), Some(16)]);
    let (_, s) = run_case("draft 1920x1080", 1920, 1080, "draft 16:9", &expected, MILD, 1);
    assert_eq!(s.correct, 7);
    assert_perfect("draft 1080p", &s);
}

#[test]
fn draft_1440p_full() {
    let expected = picks([Some(2), Some(4), Some(6), Some(8), Some(10)], [Some(11), Some(13), Some(15), Some(17), Some(20)]);
    let (_, s) = run_case("draft 2560x1440", 2560, 1440, "draft 16:9", &expected, MILD, 2);
    assert_eq!(s.correct, 10);
    assert_perfect("draft 1440p", &s);
}

#[test]
fn top_bar_1080p_and_1440p() {
    let expected = picks([Some(9), Some(14), Some(18), Some(1), Some(2)], [Some(3), Some(4), Some(5), Some(6), Some(7)]);
    let (_, s) = run_case("top bar 1920x1080", 1920, 1080, "top bar", &expected, MILD, 3);
    assert_perfect("top bar 1080p", &s);
    let (_, s) = run_case("top bar 2560x1440", 2560, 1440, "top bar", &expected, MILD, 4);
    assert_perfect("top bar 1440p", &s);
}

#[test]
fn draft_ultrawide_16_10_and_4_3() {
    let expected = picks([Some(20), Some(19), None, Some(17), Some(16)], [None, Some(14), Some(13), Some(12), Some(11)]);
    let (_, s) = run_case("draft 2560x1080 (21:9)", 2560, 1080, "draft 16:9", &expected, MILD, 5);
    assert_perfect("21:9", &s);
    let (_, s) = run_case("draft 1920x1200 (16:10)", 1920, 1200, "draft 16:10", &expected, MILD, 6);
    assert_perfect("16:10", &s);
    let (_, s) = run_case("draft 1600x1200 (4:3)", 1600, 1200, "draft 4:3", &expected, MILD, 7);
    assert_perfect("4:3", &s);
}

#[test]
fn harsher_distortions_stay_honest() {
    // Stronger blur, darker/brighter, heavy JPEG, 2 px misplacement, 900p.
    let harsh = Distort { brightness: 0.3, jitter_px: 2, blur: 1.0, jpeg: Some(45) };
    let expected = picks([Some(1), Some(2), Some(3), Some(4), Some(5)], [Some(6), Some(7), Some(8), Some(9), Some(10)]);
    let (_, s) = run_case("draft 1600x900 harsh", 1600, 900, "draft 16:9", &expected, harsh, 8);
    // Wrong answers must not be confident.
    assert!(s.correct >= 9, "only {} correct", s.correct);
    assert!(s.max_conf_wrong < UNCERTAIN, "a wrong answer had confidence {}", s.max_conf_wrong);
    let (_, s) = run_case("top bar 1600x900 harsh", 1600, 900, "top bar", &expected, harsh, 9);
    assert!(s.correct >= 8, "only {} correct", s.correct);
    assert!(s.max_conf_wrong < UNCERTAIN, "a wrong answer had confidence {}", s.max_conf_wrong);
}

#[test]
fn empty_draft_has_no_picks() {
    let expected = picks([None; 5], [None; 5]);
    let heroes = fake_heroes(20);
    let portraits = Portraits::from_images(heroes.clone());
    let shot = compose(1920, 1080, full(1920, 1080), &layout("draft 16:9"), &heroes, &expected, MILD, 10);
    let rec = recognize_with(&shot, &portraits, &default_layouts(), None);
    println!("empty draft: {:?} {:?}", rec.picks, rec.warnings);
    assert!(rec.picks.is_empty());
    assert!(!rec.warnings.is_empty());
}

#[test]
fn garbage_images_give_warnings_not_confident_picks() {
    let heroes = fake_heroes(20);
    let portraits = Portraits::from_images(heroes);
    let mut rng = Rng(12345);
    let noise = ImageBuffer::from_fn(1920, 1080, |_, _| Rgb([(rng.next() % 256) as u8, (rng.next() % 256) as u8, (rng.next() % 256) as u8]));
    let smooth = background(1920, 1080, 77);
    // A "desktop": random rectangles of flat colour and some text-like stripes.
    let mut desk = background(1920, 1080, 5);
    let mut r = Rng(999);
    for _ in 0..60 {
        let (x, y, w, h, c) = (r.next() % 1900, r.next() % 1060, 20 + r.next() % 300, 10 + r.next() % 200, r.color());
        for yy in y..(y + h).min(1080) {
            for xx in x..(x + w).min(1920) {
                let stripe = (yy / 3) % 4 == 0 && (xx / 5) % 3 != 0;
                let k = if stripe { 0.4 } else { 1.0 };
                desk.put_pixel(xx as u32, yy as u32, Rgb([(c[0] * k) as u8, (c[1] * k) as u8, (c[2] * k) as u8]));
            }
        }
    }
    for (label, img) in [("noise", noise), ("smooth", smooth), ("desktop", desk)] {
        let rec = recognize_with(&img, &portraits, &default_layouts(), None);
        let max = rec.picks.iter().map(|p| p.confidence).fold(0.0f32, f32::max);
        println!("garbage {label}: {} picks, max confidence {max:.2}, warnings {:?}", rec.picks.len(), rec.warnings);
        assert!(!rec.warnings.is_empty(), "{label}: no warning");
        assert!(max < UNCERTAIN, "{label}: confident pick {max}");
    }
    // Unsupported shape and tiny images.
    let tall = RgbImage::new(600, 1400);
    let rec = recognize_with(&tall, &portraits, &default_layouts(), None);
    assert!(rec.picks.is_empty() && !rec.warnings.is_empty(), "{:?}", rec.warnings);
    let tiny = RgbImage::new(100, 60);
    let rec = recognize_with(&tiny, &portraits, &default_layouts(), None);
    assert!(rec.picks.is_empty() && rec.warnings[0].contains("too small"));
    // No portraits at all.
    let rec = recognize_with(&RgbImage::new(1920, 1080), &Portraits::default(), &default_layouts(), None);
    assert!(rec.warnings[0].contains("no hero portraits"));
}

#[test]
fn lookalike_heroes_get_low_confidence_or_alternatives() {
    let mut heroes = fake_heroes(20);
    // Hero 21: hero 4 with a slightly different tint and one changed corner.
    let mut twin = heroes[3].1.clone();
    for (x, y, p) in twin.enumerate_pixels_mut() {
        p.0[2] = p.0[2].saturating_add(12);
        if x > 200 && y > 100 {
            p.0 = [200, 30, 30];
        }
    }
    heroes.push((21, twin));
    let portraits = Portraits::from_images(heroes.clone());
    let expected = picks([Some(4), Some(21), None, None, None], [None; 5]);
    let shot = compose(1920, 1080, full(1920, 1080), &layout("draft 16:9"), &heroes, &expected, MILD, 11);
    let rec = recognize_with(&shot, &portraits, &default_layouts(), None);
    for p in &rec.picks {
        println!("lookalike: slot {} -> {} conf {:.2} alts {:?}", p.slot, p.hero_id, p.confidence, p.alternatives);
        // Either sure and right, or the twin is offered as the first alternative.
        let right = if p.slot == 0 { 4 } else { 21 };
        let twin = if p.slot == 0 { 21 } else { 4 };
        if p.hero_id != right {
            assert!(p.confidence < UNCERTAIN);
        }
        assert!(p.hero_id == twin || p.alternatives[0].0 == twin, "twin not offered");
        assert!(p.alternatives.len() == 3);
    }
    assert_eq!(rec.picks.len(), 2);
}

#[test]
fn multi_monitor_capture_finds_the_dota_screen() {
    let heroes = fake_heroes(20);
    let portraits = Portraits::from_images(heroes.clone());
    let expected = picks([Some(1), Some(2), Some(3), None, None], [Some(4), Some(5), None, None, Some(6)]);
    // Two 1920x1080 screens side by side; Dota on the right one, a desktop on the left.
    let region = Region { x: 1920, y: 0, w: 1920, h: 1080 };
    let shot = compose(3840, 1080, region, &layout("draft 16:9"), &heroes, &expected, MILD, 12);
    assert!(candidate_regions(3840, 1080).contains(&region));
    let rec = recognize_with(&shot, &portraits, &default_layouts(), None);
    let s = score(&rec, &expected);
    report("dual monitor", &rec, &s);
    assert_perfect("dual monitor", &s);
    assert!(rec.warnings.iter().any(|w| w.contains("several screens")));
    // An explicit region works too.
    let rec = recognize_with(&shot, &portraits, &default_layouts(), Some(region));
    assert_perfect("explicit region", &score(&rec, &expected));
}

#[test]
fn layout_json_overrides_the_built_in_table() {
    let dir = tempfile::TempDir::new().unwrap();
    std::fs::copy(concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/dota/vision/layout_override.json"), dir.path().join("layout.json")).unwrap();
    std::env::set_var("LOCALFLOW_DOTA_DIR", dir.path());
    let (layouts, warning) = vision::load_layouts();
    assert!(warning.is_none());
    let moved = layouts.iter().find(|l| l.name == "draft 16:9").unwrap().clone();
    assert!((moved.slots[0].y - 300.0 / 1080.0).abs() < 1e-3);
    assert_eq!(layouts.len(), default_layouts().len());

    let heroes = fake_heroes(20);
    let portraits = Portraits::from_images(heroes.clone());
    let expected = picks([Some(1), None, Some(3), None, Some(5)], [Some(7), None, Some(9), None, Some(11)]);
    let shot = compose(1920, 1080, full(1920, 1080), &moved, &heroes, &expected, MILD, 13);
    let path = dir.path().join("shot.png");
    shot.save(&path).unwrap();
    let rec = vision::recognize_file(&path, &portraits).unwrap();
    let s = score(&rec, &expected);
    report("layout.json override", &rec, &s);
    assert_perfect("override", &s);

    // A broken file is reported, and the built-ins are used.
    std::fs::write(dir.path().join("layout.json"), "{ not json").unwrap();
    let rec = vision::recognize(&shot, &portraits);
    assert!(rec.warnings[0].contains("layout.json ignored"));
    std::env::remove_var("LOCALFLOW_DOTA_DIR");
}

#[test]
fn many_heroes_are_fast_enough() {
    // 125 heroes, full draft; timing is printed (run with --release for the real number).
    let heroes = fake_heroes(125);
    let portraits = Portraits::from_images(heroes.clone());
    let expected = picks([Some(100), Some(2), Some(50), Some(77), Some(125)], [Some(10), Some(33), Some(64), Some(90), Some(111)]);
    let shot = compose(2560, 1440, full(2560, 1440), &layout("draft 16:9"), &heroes, &expected, MILD, 14);
    let start = Instant::now();
    let rec = recognize_with(&shot, &portraits, &default_layouts(), None);
    let elapsed = start.elapsed();
    let s = score(&rec, &expected);
    report(&format!("125 heroes ({} ms)", elapsed.as_millis()), &rec, &s);
    assert_perfect("125 heroes", &s);
    if !cfg!(debug_assertions) {
        assert!(elapsed.as_millis() < 1000);
    }
}

// ---- live: real portraits from Valve's CDN ----------------------------------------------------------

#[test]
#[ignore = "downloads the real hero portraits"]
fn live_real_portraits() {
    let list: Vec<serde_json::Value> = ureq::get("https://api.opendota.com/api/heroes").call().unwrap().into_json().unwrap();
    let heroes: Vec<Hero> = list
        .iter()
        .map(|h| Hero {
            id: h["id"].as_u64().unwrap() as u32,
            name: h["name"].as_str().unwrap().into(),
            localized_name: h["localized_name"].as_str().unwrap_or_default().into(),
            primary_attr: h["primary_attr"].as_str().unwrap_or_default().into(),
            attack_type: h["attack_type"].as_str().unwrap_or_default().into(),
            roles: Vec::new(),
        })
        .collect();
    let dir = tempfile::TempDir::new().unwrap();
    let start = Instant::now();
    let portraits = vision::load_portraits_in(dir.path(), &heroes, true).unwrap();
    println!("downloaded {} of {} portraits in {:?}; missing {:?}", portraits.count, heroes.len(), start.elapsed(), portraits.missing);
    let again = Instant::now();
    let portraits = vision::load_portraits_in(dir.path(), &heroes, false).unwrap();
    println!("reloaded from disk in {:?}", again.elapsed());

    let images: Vec<(u32, RgbImage)> = heroes
        .iter()
        .filter_map(|h| image::open(dir.path().join(format!("{}.png", h.short_name()))).ok().map(|i| (h.id, i.to_rgb8())))
        .collect();
    let mut total = (0, 0, 0, 0);
    let mut rng = Rng(4242);
    let harsh = Distort { brightness: 0.3, jitter_px: 2, blur: 1.0, jpeg: Some(45) };
    for round in 0..9u64 {
        let mut chosen: Vec<u32> = Vec::new();
        while chosen.len() < 10 {
            let id = images[(rng.next() % images.len() as u64) as usize].0;
            if !chosen.contains(&id) {
                chosen.push(id);
            }
        }
        let r: [Option<u32>; 5] = std::array::from_fn(|i| if round % 2 == 1 && i == 2 { None } else { Some(chosen[i]) });
        let d: [Option<u32>; 5] = std::array::from_fn(|i| Some(chosen[5 + i]));
        let expected = picks(r, d);
        let (w, h, lname) = match round {
            0 => (1920, 1080, "draft 16:9"),
            1 => (2560, 1440, "draft 16:9"),
            2 => (1920, 1080, "top bar"),
            3 => (2560, 1440, "top bar"),
            4 => (3440, 1440, "draft 16:9"),
            5 => (1920, 1200, "draft 16:10"),
            6 => (1600, 900, "draft 16:9"),
            7 => (1600, 900, "top bar"),
            _ => (1920, 1080, "top bar"),
        };
        let mut lay = layout(lname);
        if round == 8 {
            // Misaligned: 5 px right, 3 px down, 6 % larger than the table.
            for s in &mut lay.slots {
                s.x += 5.0 / 1080.0 - s.w * 0.03;
                s.y += 3.0 / 1080.0 - s.h * 0.03;
                s.w *= 1.06;
                s.h *= 1.06;
            }
        }
        let d = if round >= 6 { harsh } else { MILD };
        let shot = compose(w, h, full(w, h), &lay, &images, &expected, d, 100 + round);
        let t = Instant::now();
        let rec = recognize_with(&shot, &portraits, &default_layouts(), None);
        let s = score(&rec, &expected);
        let tag = match round { 6 | 7 => " harsh", 8 => " misaligned", _ => "" };
        report(&format!("live {lname} {w}x{h}{tag} ({} ms)", t.elapsed().as_millis()), &rec, &s);
        for p in &rec.picks {
            if p.confidence < UNCERTAIN {
                println!("   uncertain: {:?} {} -> {} ({:.2}) alts {:?}", p.team, p.slot, p.hero_id, p.confidence, p.alternatives);
            }
        }
        total.0 += s.correct;
        total.1 += s.wrong;
        total.2 += s.missed;
        total.3 += s.phantom;
    }
    println!("live total: correct {} wrong {} missed {} phantom {}", total.0, total.1, total.2, total.3);
    // Screens without heroes, against the real portraits.
    let empty = compose(1920, 1080, full(1920, 1080), &layout("draft 16:9"), &images, &picks([None; 5], [None; 5]), MILD, 7);
    let mut r = Rng(31337);
    let noise = ImageBuffer::from_fn(1920, 1080, |_, _| Rgb([(r.next() % 256) as u8, (r.next() % 256) as u8, (r.next() % 256) as u8]));
    // Real hero art where no slot is: a big portrait stretched over the whole screen.
    let splash = imageops::resize(&images[0].1, 1920, 1080, imageops::FilterType::Triangle);
    for (label, img) in [("empty draft", empty), ("noise", noise), ("splash art", splash)] {
        let rec = recognize_with(&img, &portraits, &default_layouts(), None);
        let max = rec.picks.iter().map(|p| p.confidence).fold(0.0f32, f32::max);
        println!("live no-hero {label}: {} picks, max confidence {max:.2}, warnings {:?}", rec.picks.len(), rec.warnings);
        assert!(max < UNCERTAIN, "{label}");
    }
    assert!(total.1 + total.2 + total.3 <= 3);
}

#[test]
fn tolerates_small_layout_errors() {
    // The real screen differs from the table: 5 px right, 3 px down, 6 % larger (1080p pixels).
    let heroes = fake_heroes(40);
    let portraits = Portraits::from_images(heroes.clone());
    let expected = picks([Some(1), Some(12), Some(23), Some(34), Some(5)], [Some(16), Some(27), Some(38), Some(9), Some(20)]);
    for name in ["draft 16:9", "top bar"] {
        let mut off = layout(name);
        for s in &mut off.slots {
            s.x += 5.0 / 1080.0 - s.w * 0.03;
            s.y += 3.0 / 1080.0 - s.h * 0.03;
            s.w *= 1.06;
            s.h *= 1.06;
        }
        let shot = compose(1920, 1080, full(1920, 1080), &off, &heroes, &expected, MILD, 15);
        let rec = recognize_with(&shot, &portraits, &default_layouts(), None);
        let s = score(&rec, &expected);
        report(&format!("{name} misaligned"), &rec, &s);
        assert!(s.correct >= 9, "{name}: only {} correct", s.correct);
        assert!(s.max_conf_wrong < UNCERTAIN);
    }
}

#[test]
fn greyed_out_pictures_are_uncertain() {
    // A teammate's tentative pick: saturation 0, brightness 0.3 (Valve's .HeroPickTentative).
    // Not a final pick yet: it may be read as empty, but never as a confident hero.
    let heroes = fake_heroes(20);
    let portraits = Portraits::from_images(heroes.clone());
    let mut grey_heroes = heroes.clone();
    for (_, img) in &mut grey_heroes {
        for p in img.pixels_mut() {
            let l = 0.299 * p.0[0] as f32 + 0.587 * p.0[1] as f32 + 0.114 * p.0[2] as f32;
            let v = (l * 0.3 * 0.85 + 0.15 * 0xac as f32 * 0.3) as u8;
            p.0 = [v, v, v];
        }
    }
    let expected = picks([Some(3), Some(8), None, None, None], [None; 5]);
    let shot = compose(1920, 1080, full(1920, 1080), &layout("draft 16:9"), &grey_heroes, &expected, MILD, 16);
    let rec = recognize_with(&shot, &portraits, &default_layouts(), None);
    for p in &rec.picks {
        println!("grey: slot {} -> {} conf {:.2} alts {:?}", p.slot, p.hero_id, p.confidence, p.alternatives);
        assert!(p.confidence < UNCERTAIN);
    }
    let s = score(&rec, &expected);
    report("greyed out", &rec, &s);
}
