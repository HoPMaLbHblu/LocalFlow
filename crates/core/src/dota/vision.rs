//! Screen layouts, hero portraits and recognition. OWNER: vision agent.
//!
//! ## Where the heroes are on screen
//!
//! Dota's Panorama UI is laid out in a 1080-pixel-high reference space, scaled by
//! `screen height / 1080` and centred horizontally. So every slot is stored as a rectangle
//! whose `x` is measured from the screen centre, and whose `x`, `y`, `w`, `h` are fractions of
//! the screen **height**. The same numbers then work for 1080p, 1440p and 4K, and wider
//! screens only add empty space at the sides.
//!
//! The built-in numbers are derived from Valve's own Panorama files (decompiled from the
//! game's VPK, as mirrored on GitHub by `spirit-bear-productions/dota_vpk_updates`, files
//! `panorama/styles/hud/dota_hud_top_bar.css`, `dota_hud_pregame.css`,
//! `dota_hud_hero_picking_player.css`, version of 2026-03):
//!
//! - **Top bar (in game)**: each `.TopBarHeroImage` is 66x36 at y=4. The score (58 wide,
//!   4 margin) sits 40 px from the centre, so Radiant slot i starts at `-416 + 62*i` and Dire
//!   slot i at `+102 + 62*i` (slots overlap by 4 px: `margin -4px`). Same for every aspect ratio.
//! - **Hero selection header (draft screen)**: `#Header` flows
//!   `fill | RadiantTeamPlayers | coach | HeaderCenter | coach | DireTeamPlayers | fill`, so it is
//!   symmetric about the centre. `HeaderCenter` is 250 wide (300 at 16:10), team padding 8,
//!   each player 128 wide with -2 margins (124 pitch), the hero image 118x66 at x+3 (Radiant)
//!   or x+5 (Dire), y=6. At 4:3 the image is 100x56 with a 102 pitch. With coaches present
//!   (`.CoachPresent`) each side moves 60 px outwards.
//!
//! These are derived, not measured on real screenshots. They can be tuned without code changes
//! by putting a `layout.json` (a list of [`Layout`]) in the data folder: an entry with the same
//! `name` as a built-in replaces it, others are added. See [`default_layouts`] for the format.
//!
//! ## Several monitors
//!
//! The capture is the whole virtual screen. Recognition tries the whole image and, when it is
//! much wider (or taller) than any single screen, also common single-screen regions at its edges,
//! and keeps whichever region shows heroes most clearly. Monitors of different heights, or
//! arrangements that are neither side by side nor stacked, are not handled; pass the Dota
//! screen's rectangle to [`recognize_region`] for those.

use std::{
    path::{Path, PathBuf},
    time::Duration,
};

use image::RgbImage;
use serde::{Deserialize, Serialize};

use super::{data_dir, write_atomic, Hero, Recognition, RecognizedPick, Team};

// ---- layouts --------------------------------------------------------------------------------

/// One hero picture on screen. Fractions of the screen height; `x` from the screen centre.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SlotRect {
    pub team: Team,
    /// 0-4, left to right within the team's half.
    pub slot: u8,
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

/// Where the ten hero pictures are on one kind of screen.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Layout {
    /// "draft 16:9", "top bar", ...
    pub name: String,
    /// Screen width / height this layout applies to (inclusive range).
    pub min_aspect: f32,
    pub max_aspect: f32,
    /// The part of each slot compared with the portraits, as fractions of the slot
    /// `[left, top, right, bottom]`. Keeps slanted edges, borders and gradients out.
    pub inner: [f32; 4],
    pub slots: Vec<SlotRect>,
    /// Where the numbers come from ("Valve Panorama CSS, not checked on a screenshot").
    #[serde(default)]
    pub note: String,
}

impl Layout {
    fn applies_to(&self, aspect: f32) -> bool {
        aspect >= self.min_aspect - 1e-3 && aspect <= self.max_aspect + 1e-3
    }
}

/// Builds a layout from 1080p reference pixels: slot i of Radiant starts at `radiant_x + i*pitch`
/// from the centre, Dire at `dire_x + i*pitch`.
#[allow(clippy::too_many_arguments)]
fn layout_px(name: &str, aspect: (f32, f32), radiant_x: f32, dire_x: f32, pitch: f32, y: f32, w: f32, h: f32, inner: [f32; 4], note: &str) -> Layout {
    let mut slots = Vec::with_capacity(10);
    for (team, x0) in [(Team::Radiant, radiant_x), (Team::Dire, dire_x)] {
        for i in 0..5u8 {
            slots.push(SlotRect { team, slot: i, x: (x0 + pitch * i as f32) / 1080.0, y: y / 1080.0, w: w / 1080.0, h: h / 1080.0 });
        }
    }
    Layout { name: name.into(), min_aspect: aspect.0, max_aspect: aspect.1, inner, slots, note: note.into() }
}

/// The built-in layouts (see the module docs for where the numbers come from).
pub fn default_layouts() -> Vec<Layout> {
    const NOTE: &str = "derived from Valve's Panorama CSS (2026-03); not yet checked on a real screenshot";
    // Draft: image slanted on the outer side and darkened in the top/bottom 10 %.
    let draft_inner = [0.16, 0.14, 0.84, 0.86];
    let bar_inner = [0.18, 0.12, 0.82, 0.88];
    let wide = (1.70, 4.0);
    let w1610 = (1.55, 1.70);
    let w43 = (1.20, 1.55);
    vec![
        // HeaderCenter 250: Radiant players' content ends 133 left of centre.
        layout_px("draft 16:9", wide, -752.0, 136.0, 124.0, 6.0, 118.0, 66.0, draft_inner, NOTE),
        layout_px("draft 16:9 with coaches", wide, -812.0, 196.0, 124.0, 6.0, 118.0, 66.0, draft_inner, NOTE),
        // HeaderCenter 300.
        layout_px("draft 16:10", w1610, -777.0, 161.0, 124.0, 6.0, 118.0, 66.0, draft_inner, NOTE),
        layout_px("draft 16:10 with coaches", w1610, -837.0, 221.0, 124.0, 6.0, 118.0, 66.0, draft_inner, NOTE),
        // 4:3 and 5:4: 100x56 images, 102 pitch.
        layout_px("draft 4:3", w43, -644.0, 134.0, 102.0, 6.0, 100.0, 56.0, draft_inner, NOTE),
        layout_px("top bar", (1.20, 4.0), -416.0, 102.0, 62.0, 4.0, 66.0, 36.0, bar_inner, NOTE),
    ]
}

/// Built-in layouts merged with `data_dir()/layout.json`, plus a warning if that file is broken.
pub fn load_layouts() -> (Vec<Layout>, Option<String>) {
    let mut layouts = default_layouts();
    let path = data_dir().join("layout.json");
    let Ok(text) = std::fs::read_to_string(&path) else { return (layouts, None) };
    match serde_json::from_str::<Vec<Layout>>(&text) {
        Ok(extra) => {
            for layout in extra {
                match layouts.iter_mut().find(|l| l.name == layout.name) {
                    Some(existing) => *existing = layout,
                    None => layouts.push(layout),
                }
            }
            (layouts, None)
        }
        Err(e) => (layouts, Some(format!("layout.json ignored ({e}); using the built-in layouts"))),
    }
}

/// A rectangle of the captured image, in pixels.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Region {
    pub x: u32,
    pub y: u32,
    pub w: u32,
    pub h: u32,
}

/// Screen regions worth trying for a capture of `w` x `h` (the whole image first).
pub fn candidate_regions(w: u32, h: u32) -> Vec<Region> {
    let mut out = vec![Region { x: 0, y: 0, w, h }];
    let mut push = |r: Region| {
        if r.w >= 320 && r.h >= 240 && !out.contains(&r) {
            out.push(r);
        }
    };
    let aspects = [16.0 / 9.0, 16.0 / 10.0, 21.0 / 9.0, 4.0 / 3.0];
    if h > 0 && w as f32 / h as f32 > 2.45 {
        // Side by side: the Dota screen is at one edge or a multiple of its width in.
        for a in aspects {
            let rw = (h as f32 * a).round() as u32;
            if rw >= w {
                continue;
            }
            push(Region { x: 0, y: 0, w: rw, h });
            push(Region { x: w - rw, y: 0, w: rw, h });
            let mut x = rw;
            while x + rw < w {
                push(Region { x, y: 0, w: rw, h });
                x += rw;
            }
        }
    }
    if w > 0 && (h as f32 / w as f32) > 0.9 {
        // Stacked.
        for a in aspects {
            let rh = (w as f32 / a).round() as u32;
            if rh >= h {
                continue;
            }
            push(Region { x: 0, y: 0, w, h: rh });
            push(Region { x: 0, y: h - rh, w, h: rh });
        }
    }
    out
}

// ---- features ---------------------------------------------------------------------------------

const FW: usize = 20;
const FH: usize = 12;
const N: usize = FW * FH;

/// A small normalised description of one picture: luminance and colour, each zero-mean and
/// unit-length, so a dot product is a normalised cross-correlation.
#[derive(Debug, Clone)]
struct Feat {
    lum: Vec<f32>,
    chroma: Vec<f32>,
    lum_ok: bool,
    chroma_ok: bool,
    /// Mean absolute difference between neighbouring cells (0..1): texture.
    detail: f32,
}

/// Average colour of each grid cell over the float rectangle (pixels, clamped to the image).
fn grid(img: &RgbImage, x0: f32, y0: f32, x1: f32, y1: f32) -> Vec<[f32; 3]> {
    let (iw, ih) = (img.width() as i64, img.height() as i64);
    let mut out = Vec::with_capacity(N);
    let raw = img.as_raw();
    for j in 0..FH {
        let cy0 = y0 + (y1 - y0) * j as f32 / FH as f32;
        let cy1 = y0 + (y1 - y0) * (j + 1) as f32 / FH as f32;
        let (mut py0, mut py1) = ((cy0 - 0.5).ceil() as i64, (cy1 - 0.5).ceil() as i64 - 1);
        if py1 < py0 {
            py0 = ((cy0 + cy1) / 2.0).floor() as i64;
            py1 = py0;
        }
        for i in 0..FW {
            let cx0 = x0 + (x1 - x0) * i as f32 / FW as f32;
            let cx1 = x0 + (x1 - x0) * (i + 1) as f32 / FW as f32;
            let (mut px0, mut px1) = ((cx0 - 0.5).ceil() as i64, (cx1 - 0.5).ceil() as i64 - 1);
            if px1 < px0 {
                px0 = ((cx0 + cx1) / 2.0).floor() as i64;
                px1 = px0;
            }
            let mut sum = [0u32; 3];
            let mut count = 0u32;
            for py in py0..=py1 {
                let yy = py.clamp(0, ih - 1) as usize;
                for px in px0..=px1 {
                    let xx = px.clamp(0, iw - 1) as usize;
                    let k = (yy * iw as usize + xx) * 3;
                    sum[0] += raw[k] as u32;
                    sum[1] += raw[k + 1] as u32;
                    sum[2] += raw[k + 2] as u32;
                    count += 1;
                }
            }
            let c = count.max(1) as f32 * 255.0;
            out.push([sum[0] as f32 / c, sum[1] as f32 / c, sum[2] as f32 / c]);
        }
    }
    out
}

/// Zero mean, unit length. Returns the standard deviation before normalising.
fn normalise(v: &mut [f32]) -> f32 {
    let n = v.len() as f32;
    let mean = v.iter().sum::<f32>() / n;
    let mut ss = 0.0;
    for x in v.iter_mut() {
        *x -= mean;
        ss += *x * *x;
    }
    let norm = ss.sqrt();
    if norm > 1e-6 {
        for x in v.iter_mut() {
            *x /= norm;
        }
    }
    (ss / n).sqrt()
}

fn feat_from_grid(cells: &[[f32; 3]]) -> Feat {
    let mut lum: Vec<f32> = cells.iter().map(|c| 0.299 * c[0] + 0.587 * c[1] + 0.114 * c[2]).collect();
    let mut detail = 0.0;
    let mut pairs = 0;
    for j in 0..FH {
        for i in 0..FW {
            let v = lum[j * FW + i];
            if i + 1 < FW {
                detail += (v - lum[j * FW + i + 1]).abs();
                pairs += 1;
            }
            if j + 1 < FH {
                detail += (v - lum[(j + 1) * FW + i]).abs();
                pairs += 1;
            }
        }
    }
    let detail = detail / pairs as f32;
    let mut chroma: Vec<f32> = cells.iter().map(|c| c[0] - c[1]).chain(cells.iter().map(|c| (c[0] + c[1]) / 2.0 - c[2])).collect();
    let lum_std = normalise(&mut lum);
    let chroma_std = normalise(&mut chroma);
    Feat { lum, chroma, lum_ok: lum_std > 0.01, chroma_ok: chroma_std > 0.02, detail }
}

fn dot(a: &[f32], b: &[f32]) -> f32 {
    a.iter().zip(b).map(|(x, y)| x * y).sum()
}

/// Similarity -1..1 (normalised cross-correlation of luminance and colour).
fn similarity(s: &Feat, p: &Feat) -> f32 {
    let l = if s.lum_ok && p.lum_ok { dot(&s.lum, &p.lum) } else { 0.0 };
    if s.chroma_ok && p.chroma_ok {
        0.55 * l + 0.45 * dot(&s.chroma, &p.chroma)
    } else {
        // Greyed-out picture (e.g. a tentative pick): colour can't help, so trust it less.
        0.85 * l
    }
}

// ---- portraits --------------------------------------------------------------------------------

struct PortraitRef {
    hero_id: u32,
    /// One feature per built-in layout `inner` crop.
    feats: Vec<([f32; 4], Feat)>,
    image: RgbImage,
}

/// Reference images of every hero, prepared for matching.
#[derive(Default)]
pub struct Portraits {
    /// How many heroes can be recognised.
    pub count: usize,
    /// Heroes ("antimage", ...) whose portrait couldn't be found or downloaded.
    pub missing: Vec<String>,
    refs: Vec<PortraitRef>,
}

impl Portraits {
    /// Portraits from images already in memory (16:9 hero pictures, any size).
    pub fn from_images(images: impl IntoIterator<Item = (u32, RgbImage)>) -> Portraits {
        let mut p = Portraits::default();
        for (hero_id, image) in images {
            let mut feats: Vec<([f32; 4], Feat)> = Vec::new();
            for layout in default_layouts() {
                if !feats.iter().any(|(k, _)| *k == layout.inner) {
                    feats.push((layout.inner, portrait_feat(&image, layout.inner)));
                }
            }
            p.refs.push(PortraitRef { hero_id, feats, image });
        }
        p.count = p.refs.len();
        p
    }

    pub fn hero_ids(&self) -> Vec<u32> {
        self.refs.iter().map(|r| r.hero_id).collect()
    }

    pub fn is_empty(&self) -> bool {
        self.refs.is_empty()
    }

    fn feats_for(&self, inner: [f32; 4]) -> Option<Vec<&Feat>> {
        self.refs.iter().map(|r| r.feats.iter().find(|(k, _)| *k == inner).map(|(_, f)| f)).collect()
    }
}

fn portrait_feat(image: &RgbImage, inner: [f32; 4]) -> Feat {
    let (w, h) = (image.width() as f32, image.height() as f32);
    feat_from_grid(&grid(image, inner[0] * w, inner[1] * h, inner[2] * w, inner[3] * h))
}

/// The official portrait for a hero (256x144 PNG).
pub fn portrait_url(hero: &Hero) -> String {
    format!("https://cdn.cloudflare.steamstatic.com/apps/dota2/images/dota_react/heroes/{}.png", hero.short_name())
}

fn portrait_urls(hero: &Hero) -> [String; 2] {
    [portrait_url(hero), format!("https://cdn.steamstatic.com/apps/dota2/images/dota_react/heroes/{}.png", hero.short_name())]
}

fn download(agent: &ureq::Agent, hero: &Hero) -> Result<Vec<u8>, String> {
    let mut last = String::new();
    for url in portrait_urls(hero) {
        match agent.get(&url).call() {
            Ok(response) => {
                let mut bytes = Vec::new();
                use std::io::Read;
                response.into_reader().take(8 << 20).read_to_end(&mut bytes).map_err(|e| e.to_string())?;
                image::load_from_memory(&bytes).map_err(|e| format!("{url}: not an image ({e})"))?;
                return Ok(bytes);
            }
            Err(e) => last = format!("{url}: {e}"),
        }
    }
    Err(last)
}

/// Downloads (once) and prepares the hero portraits in `data_dir()/portraits`.
pub fn load_portraits(heroes: &[Hero]) -> Result<Portraits, String> {
    load_portraits_in(&data_dir().join("portraits"), heroes, true)
}

/// Loads portraits from `dir` (`<short_name>.png`), downloading missing ones when `download`
/// is true. Heroes without a portrait are listed in `missing`; an error only if none loaded.
pub fn load_portraits_in(dir: &Path, heroes: &[Hero], download_missing: bool) -> Result<Portraits, String> {
    std::fs::create_dir_all(dir).map_err(|e| format!("can't create {}: {e}", dir.display()))?;
    let path_of = |h: &Hero| -> PathBuf { dir.join(format!("{}.png", h.short_name())) };
    let todo: Vec<&Hero> = heroes.iter().filter(|h| !path_of(h).is_file()).collect();
    let mut errors = Vec::new();
    if download_missing && !todo.is_empty() {
        let agent = ureq::AgentBuilder::new()
            .timeout(Duration::from_secs(20))
            .user_agent(concat!("LocalFlow/", env!("CARGO_PKG_VERSION")))
            .build();
        let results: Vec<(String, Result<(), String>)> = std::thread::scope(|scope| {
            let chunks: Vec<Vec<&Hero>> = todo.chunks(todo.len().div_ceil(8)).map(|c| c.to_vec()).collect();
            let handles: Vec<_> = chunks
                .into_iter()
                .map(|chunk| {
                    let agent = agent.clone();
                    let path_of = &path_of;
                    scope.spawn(move || {
                        chunk
                            .into_iter()
                            .map(|h| {
                                let r = download(&agent, h).and_then(|bytes| write_atomic(&path_of(h), &bytes));
                                (h.short_name().to_string(), r)
                            })
                            .collect::<Vec<_>>()
                    })
                })
                .collect();
            handles.into_iter().flat_map(|h| h.join().unwrap_or_default()).collect()
        });
        for (name, r) in results {
            if let Err(e) = r {
                errors.push(format!("{name}: {e}"));
            }
        }
    }
    let mut images = Vec::new();
    let mut missing = Vec::new();
    for h in heroes {
        match image::open(path_of(h)) {
            Ok(img) => images.push((h.id, img.to_rgb8())),
            Err(_) => missing.push(h.short_name().to_string()),
        }
    }
    if images.is_empty() && !heroes.is_empty() {
        let why = errors.first().cloned().unwrap_or_else(|| "no portrait files".into());
        return Err(format!("couldn't get any hero portraits ({why})"));
    }
    let mut p = Portraits::from_images(images);
    p.missing = missing;
    Ok(p)
}

// ---- recognition ------------------------------------------------------------------------------

/// Below this similarity a slot is not reported at all.
pub const MIN_SIMILARITY: f32 = 0.45;
/// Highest confidence for a greyed-out picture.
pub const GREY_CONFIDENCE: f32 = 0.5;
/// Below this texture a slot is treated as empty (no hero picked yet).
pub const MIN_DETAIL: f32 = 0.018;

fn smoothstep(e0: f32, e1: f32, x: f32) -> f32 {
    let t = ((x - e0) / (e1 - e0)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// Confidence 0..1 from the best similarity and its margin over the second best.
pub fn confidence(best: f32, second: f32) -> f32 {
    let quality = smoothstep(MIN_SIMILARITY, 0.80, best);
    let margin = smoothstep(0.02, 0.15, best - second);
    (quality * (0.3 + 0.7 * margin)).clamp(0.0, 1.0)
}

/// What one slot showed.
#[derive(Debug, Clone, PartialEq)]
pub enum SlotReading {
    Empty,
    /// Something is there but it doesn't look like any known hero.
    Unknown { best: f32 },
    Hero { hero_id: u32, similarity: f32, confidence: f32, alternatives: Vec<(u32, f32)> },
}

/// Pixel rectangle of a slot inside `region`.
fn slot_pixels(slot: &SlotRect, region: Region) -> (f32, f32, f32, f32) {
    let h = region.h as f32;
    let cx = region.x as f32 + region.w as f32 / 2.0;
    (cx + slot.x * h, region.y as f32 + slot.y * h, slot.w * h, slot.h * h)
}

fn read_slot(image: &RgbImage, refs: &[(u32, &Feat)], layout: &Layout, slot: &SlotRect, region: Region, search: bool) -> SlotReading {
    let (sx, sy, sw, sh) = slot_pixels(slot, region);
    let [ix0, iy0, ix1, iy1] = layout.inner;
    // The centre crop first, then (fine pass) small shifts and scalings, since the real screen
    // may differ a little from the layout table.
    let mut crops: Vec<(f32, f32, f32)> = vec![(0.0, 0.0, 1.0)];
    if search {
        for scale in [0.94f32, 1.0, 1.06] {
            for dy in [-0.08f32, 0.0, 0.08] {
                for dx in [-0.08f32, -0.04, 0.0, 0.04, 0.08] {
                    if (dx, dy, scale) != (0.0, 0.0, 1.0) {
                        crops.push((dx, dy, scale));
                    }
                }
            }
        }
    }
    let mut best = vec![f32::MIN; refs.len()];
    let mut textured = false;
    let mut grey = false;
    let (cw, ch) = ((ix1 - ix0) * sw, (iy1 - iy0) * sh);
    let (mx, my) = (sx + (ix0 + ix1) / 2.0 * sw, sy + (iy0 + iy1) / 2.0 * sh);
    for (n, &(dx, dy, scale)) in crops.iter().enumerate() {
        let (x_mid, y_mid) = (mx + dx * sw, my + dy * sh);
        let (hw, hh) = (cw * scale / 2.0, ch * scale / 2.0);
        let f = feat_from_grid(&grid(image, x_mid - hw, y_mid - hh, x_mid + hw, y_mid + hh));
        if n == 0 {
            textured = f.detail >= MIN_DETAIL && f.lum_ok;
            grey = !f.chroma_ok;
            if !f.lum_ok {
                return SlotReading::Empty;
            }
        }
        for (b, (_, p)) in best.iter_mut().zip(refs) {
            let s = similarity(&f, p);
            if s > *b {
                *b = s;
            }
        }
    }
    let mut order: Vec<usize> = (0..best.len()).collect();
    order.sort_by(|&a, &b| best[b].total_cmp(&best[a]));
    let Some(&top) = order.first() else { return SlotReading::Unknown { best: 0.0 } };
    let s1 = best[top];
    let s2 = order.get(1).map(|&k| best[k]).unwrap_or(-1.0);
    if !textured && (s1 < 0.85 || s1 - s2 < 0.1) {
        // Smooth, and not clearly a (plain-looking) hero: an empty slot.
        return SlotReading::Empty;
    }
    let mut conf = confidence(s1, s2);
    if s1 < MIN_SIMILARITY || conf < 0.05 {
        return SlotReading::Unknown { best: s1 };
    }
    if grey {
        // A greyed-out picture: a teammate's tentative pick on the draft screen, or a dead hero
        // in the top bar. Colour can't confirm it, so it is always "please check".
        conf = conf.min(GREY_CONFIDENCE);
    }
    let alternatives = order
        .iter()
        .skip(1)
        .take(3)
        .map(|&k| (refs[k].0, (smoothstep(MIN_SIMILARITY, 0.80, best[k]) * 0.5).min(conf * 0.95)))
        .collect();
    SlotReading::Hero { hero_id: refs[top].0, similarity: s1, confidence: conf, alternatives }
}

/// How clearly a layout shows heroes: sum of confidences plus a little for plain matches.
fn evidence(readings: &[SlotReading]) -> f32 {
    readings
        .iter()
        .map(|r| match r {
            SlotReading::Hero { confidence, similarity, .. } => confidence + 0.2 * similarity,
            _ => 0.0,
        })
        .sum()
}

fn aspect_name(a: f32) -> String {
    for (name, v) in [("16:9", 16.0 / 9.0), ("16:10", 1.6), ("21:9", 64.0 / 27.0), ("4:3", 4.0 / 3.0), ("5:4", 1.25), ("32:9", 32.0 / 9.0)] {
        if (a - v).abs() < 0.03 {
            return name.into();
        }
    }
    format!("{a:.2}:1")
}

/// Finds the heroes in a full-screen capture (the whole virtual screen is fine).
pub fn recognize(image: &RgbImage, portraits: &Portraits) -> Recognition {
    let (layouts, warning) = load_layouts();
    let mut rec = recognize_with(image, portraits, &layouts, None);
    if let Some(w) = warning {
        rec.warnings.insert(0, w);
    }
    rec
}

/// Like [`recognize`], but only looks at `region` (e.g. the monitor Dota runs on).
pub fn recognize_region(image: &RgbImage, portraits: &Portraits, region: Region) -> Recognition {
    let (layouts, warning) = load_layouts();
    let mut rec = recognize_with(image, portraits, &layouts, Some(region));
    if let Some(w) = warning {
        rec.warnings.insert(0, w);
    }
    rec
}

/// Recognition with explicit layouts and, optionally, a fixed screen region.
pub fn recognize_with(image: &RgbImage, portraits: &Portraits, layouts: &[Layout], region: Option<Region>) -> Recognition {
    let (w, h) = image.dimensions();
    let mut rec = Recognition { width: w, height: h, ..Default::default() };
    if portraits.is_empty() {
        rec.warnings.push("no hero portraits are loaded, so heroes can't be recognised".into());
        return rec;
    }
    if w < 320 || h < 240 {
        rec.warnings.push(format!("the image is too small ({w}x{h}) to recognise heroes"));
        return rec;
    }
    // Portrait features for each layout's crop (built-in crops are precomputed).
    let owned: Vec<Vec<Feat>> = layouts
        .iter()
        .map(|l| if portraits.feats_for(l.inner).is_some() { Vec::new() } else { portraits.refs.iter().map(|r| portrait_feat(&r.image, l.inner)).collect() })
        .collect();
    let tables: Vec<Vec<(u32, &Feat)>> = layouts
        .iter()
        .zip(&owned)
        .map(|(l, own)| match portraits.feats_for(l.inner) {
            Some(f) => portraits.refs.iter().map(|r| r.hero_id).zip(f).collect(),
            None => portraits.refs.iter().map(|r| r.hero_id).zip(own.iter()).collect(),
        })
        .collect();

    let regions = match region {
        Some(r) => vec![Region { x: r.x.min(w - 1), y: r.y.min(h - 1), w: r.w.min(w - r.x.min(w - 1)), h: r.h.min(h - r.y.min(h - 1)) }],
        None => candidate_regions(w, h),
    };
    // Coarse pass over every region and layout; keep the clearest.
    let mut best: Option<(f32, usize, Region)> = None;
    for (ri, &reg) in regions.iter().enumerate() {
        let aspect = reg.w as f32 / reg.h as f32;
        for (li, layout) in layouts.iter().enumerate() {
            if !layout.applies_to(aspect) {
                continue;
            }
            let readings: Vec<SlotReading> = layout.slots.iter().map(|s| read_slot(image, &tables[li], layout, s, reg, false)).collect();
            // Prefer the whole image when it is as good.
            let e = evidence(&readings) - if ri == 0 { 0.0 } else { 0.05 };
            if best.as_ref().is_none_or(|(b, _, _)| e > *b) {
                best = Some((e, li, reg));
            }
        }
    }
    let region0 = regions[0];
    let aspect0 = region0.w as f32 / region0.h as f32;
    let Some((_, li, reg)) = best else {
        rec.layout = "none".into();
        rec.warnings.push(format!("unsupported aspect ratio {} ({w}x{h}); heroes can't be located", aspect_name(aspect0)));
        return rec;
    };
    let layout = &layouts[li];
    let aspect = reg.w as f32 / reg.h as f32;
    rec.layout = if reg == (Region { x: 0, y: 0, w, h }) {
        format!("{} ({})", layout.name, aspect_name(aspect))
    } else {
        format!("{} ({}, region {}x{} at {},{})", layout.name, aspect_name(aspect), reg.w, reg.h, reg.x, reg.y)
    };
    if region.is_none() && reg != (Region { x: 0, y: 0, w, h }) {
        rec.warnings.push(format!("the capture spans several screens; guessed that Dota is on the {}x{} screen at {},{}", reg.w, reg.h, reg.x, reg.y));
    }
    if reg.h < 700 {
        rec.warnings.push(format!("low resolution ({}px high): recognition is less reliable", reg.h));
    }

    // Fine pass with a small position search.
    let mut unknown = 0;
    for s in &layout.slots {
        match read_slot(image, &tables[li], layout, s, reg, true) {
            SlotReading::Hero { hero_id, confidence, alternatives, .. } => {
                rec.picks.push(RecognizedPick { team: s.team, slot: s.slot, hero_id, confidence, alternatives });
            }
            SlotReading::Unknown { .. } => unknown += 1,
            SlotReading::Empty => {}
        }
    }
    if unknown > 0 {
        rec.warnings.push(format!("{unknown} slot(s) show something that doesn't match any known hero"));
    }
    if rec.picks.is_empty() {
        rec.warnings.push("no heroes recognised: is the hero selection screen or the in-game top bar visible?".into());
    } else {
        let mean = rec.picks.iter().map(|p| p.confidence).sum::<f32>() / rec.picks.len() as f32;
        if mean < super::UNCERTAIN {
            rec.warnings.push("low confidence overall: the screen layout may not match; please check the heroes".into());
        }
    }
    if !portraits.missing.is_empty() {
        rec.warnings.push(format!("{} hero portrait(s) are missing and can't be recognised: {}", portraits.missing.len(), portraits.missing.join(", ")));
    }
    rec
}

/// Every slot of one layout in one region, with what it showed (for diagnostics and tuning).
pub fn read_layout(image: &RgbImage, portraits: &Portraits, layout: &Layout, region: Region) -> Vec<(SlotRect, SlotReading)> {
    let owned: Vec<Feat>;
    let table: Vec<(u32, &Feat)> = match portraits.feats_for(layout.inner) {
        Some(f) => portraits.refs.iter().map(|r| r.hero_id).zip(f).collect(),
        None => {
            owned = portraits.refs.iter().map(|r| portrait_feat(&r.image, layout.inner)).collect();
            portraits.refs.iter().map(|r| r.hero_id).zip(owned.iter()).collect()
        }
    };
    layout.slots.iter().map(|s| (s.clone(), read_slot(image, &table, layout, s, region, true))).collect()
}

pub fn recognize_file(path: &Path, portraits: &Portraits) -> Result<Recognition, String> {
    let image = image::open(path).map_err(|e| e.to_string())?.to_rgb8();
    Ok(recognize(&image, portraits))
}
