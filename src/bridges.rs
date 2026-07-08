//! **Bridges** — plank decks laid across the combined map's real river. The river is a carved
//! terrain channel (the sea plane shows through where `worldmap::is_river_world` is true), so we
//! SCAN that channel at a few depths, find the water run's centre + width, and span it bank to
//! bank. Each deck also registers a walkable span the nav-grid honours, so the night invaders'
//! A* can cross at a bridge. Ports Bridge.tsx/bridges.ts, placed on the actual water.

use std::sync::OnceLock;

use bevy::mesh::MeshBuilder;
use bevy::prelude::*;

use crate::biome::BiomeEntity;
use crate::palette::{lin, lin_scaled};
use crate::worldmap::is_river_world;
use crate::meshkit::tinted;

/// Half-width along the bank (the deck's SHORT axis) of a deck.
const DECK_HALF_Z: f32 = 1.2;
/// Bank overhang past the water edge on each side (world units). Small — the deck just seats onto
/// the sandy bank a little, it doesn't reach out over a long apron (shorter decks, set back from
/// the water line onto firm shore rather than teetering at the very edge).
const OVERHANG: f32 = 0.8;
/// Min world-XZ gap between two bridges (so they don't cluster on one crossing). Large — a
/// meandering river doubles back on itself, so a small gap let decks bunch up a few units apart at
/// odd angles; this keeps each crossing a single distinct landmark.
const MIN_SPACING: f32 = 34.0;
/// At most this many bridges (three rivers cross the island — roughly one crossing each, two on the
/// longest; more just litters the banks with decks pointing every which way).
const MAX_BRIDGES: usize = 5;
/// Acceptable half-width of the channel being bridged (skip slivers + wide lake-like spans —
/// a clean river crossing is a couple units across).
const MIN_HALF: f32 = 0.6;
const MAX_HALF: f32 = 3.5;

/// World-Y step a mover can walk on/off the deck in one move (mirrors `steer::MAX_STEP`, kept a
/// hair under it). A deck whose banks sit more than this from the plank top strands the hero ON
/// the deck — he can't step off and has to jump — so such a crossing is rejected.
const BANK_STEP: f32 = 0.55;
/// A real crossing has river running on BOTH sides of the deck along the flow axis. At a river
/// HEAD the channel dead-ends, so a "bridge" there spans nothing you couldn't walk around — skip.
const RIVER_CONTINUE: f32 = 3.0;

/// What kind of deck a [`Span`] is: an arched-look plank BRIDGE over a river crossing, or a
/// low stilted BOARDWALK over a swamp bog pool (map-character overhaul pass 3). Both register
/// identically for nav/footing — only mesh, sizing gates and candidate source differ.
#[derive(Clone, Copy, PartialEq)]
enum DeckKind {
    Bridge,
    Boardwalk,
}

/// Boardwalk sizing: pools are broader than river channels, so the span gate is wider; the
/// walkway itself is narrower than a cart bridge (single-file planks on stilts).
const BOARDWALK_MAX_HALF: f32 = 9.0;
const BOARDWALK_HALF_Z: f32 = 0.9;
const MAX_BOARDWALKS: usize = 10;
const BOARDWALK_SPACING: f32 = 14.0;

/// A deck: world-XZ centre, the long half-length across the water (incl. overhang), whether the
/// long axis runs along X (`across_x`), the short-axis half-width (`half_z` — bridges use
/// [`DECK_HALF_Z`], boardwalks the narrower [`BOARDWALK_HALF_Z`]), and `base_y` — the bank
/// terrain height the flat deck sits on (deck top = `base_y + 0.25`). Storing `base_y` once
/// (not re-sampling per call) keeps the spawn transform, footing, and walkability in agreement.
#[derive(Clone, Copy)]
struct Span {
    cx: f32,
    cz: f32,
    half: f32,
    half_z: f32,
    across_x: bool,
    base_y: f32,
    kind: DeckKind,
}

/// Find a few clean river crossings by scanning the whole island for NARROW water channels.
/// Cached — reads only the pure `is_river_world` channel (no built terrain). The rivers meander
/// freely (no longer axis-aligned), so a deck must span whichever axis the local channel is narrow
/// on — not always X.
fn spans() -> &'static [Span] {
    static SPANS: OnceLock<Vec<Span>> = OnceLock::new();
    SPANS.get_or_init(|| {
        // A bridge exists ONLY where a ROAD crosses a river. Take each road→river crossing the road
        // network reports, snap it to a valid narrow/steppable deck nearby, then de-cluster: drop
        // any deck within MIN_SPACING of one already kept (two roads fording the same spot → one
        // bridge). Narrowest-first keeps the cleanest decks. This replaces the old island-wide scan
        // (which placed well-spread decks at the nicest crossings regardless of any path, so decks
        // stranded in the open with no road leading to them — "bridges in random places").
        let mut cands: Vec<Span> = crate::roads::river_crossings()
            .into_iter()
            .filter_map(|p| nearest_crossing(p.x, p.y))
            .collect();
        cands.sort_by(|a, b| a.half.partial_cmp(&b.half).unwrap_or(std::cmp::Ordering::Equal));
        let mut out: Vec<Span> = Vec::new();
        for c in cands {
            if out.len() >= MAX_BRIDGES {
                break;
            }
            if out.iter().all(|s| (s.cx - c.cx).hypot(s.cz - c.cz) >= MIN_SPACING) {
                out.push(c);
            }
        }
        // BOARDWALKS (map-character overhaul pass 3): every spot a road crosses a swamp bog pool
        // gets a stilted walkway, same de-cluster discipline but a tighter spacing + higher cap —
        // a swamp road legitimately hops several pools in a row, and a missing deck there is a
        // nav break (A* would detour the whole pool while the painted road runs through it).
        let mut bw: Vec<Span> = crate::roads::pool_crossings()
            .into_iter()
            .filter_map(|p| nearest_boardwalk(p.x, p.y))
            .collect();
        bw.sort_by(|a, b| a.half.partial_cmp(&b.half).unwrap_or(std::cmp::Ordering::Equal));
        let n_bridges = out.len();
        for c in bw {
            if out.len() - n_bridges >= MAX_BOARDWALKS {
                break;
            }
            if out.iter().all(|s| (s.cx - c.cx).hypot(s.cz - c.cz) >= BOARDWALK_SPACING) {
                out.push(c);
            }
        }
        out
    })
}

/// [`nearest_crossing`]'s ring-probe, for boardwalks over bog pools. Same on-road gate: the
/// snap must not carry the deck centre off the path it exists to serve.
fn nearest_boardwalk(x: f32, z: f32) -> Option<Span> {
    let on_road_walk = |s: &Span| crate::roads::on_road(s.cx, s.cz);
    if let Some(s) = boardwalk_at(x, z).filter(on_road_walk) {
        return Some(s);
    }
    let mut r = 1.0;
    while r <= 4.0 {
        let mut a = 0.0;
        while a < std::f32::consts::TAU {
            if let Some(s) = boardwalk_at(x + r * a.cos(), z + r * a.sin()).filter(on_road_walk) {
                return Some(s);
            }
            a += std::f32::consts::FRAC_PI_4;
        }
        r += 1.0;
    }
    None
}

/// Boardwalk validator — [`crossing_at`]'s shape with pool water instead of river, a wider span
/// gate (pools are broad), and NO flow-continuation gate (a pool is a blob, not a channel; the
/// walkway is useful wherever the road crosses it).
fn boardwalk_at(x: f32, z: f32) -> Option<Span> {
    let pool = |px: f32, pz: f32| crate::worldmap::is_pool_world(px, pz);
    if !pool(x, z) {
        return None;
    }
    let (cx_x, half_x) = water_run_of(x, z, true, BOARDWALK_MAX_HALF, &pool)?;
    let (cz_z, half_z) = water_run_of(x, z, false, BOARDWALK_MAX_HALF, &pool)?;
    let (across_x, cx, cz, half) = if half_x <= half_z {
        (true, cx_x, z, half_x)
    } else {
        (false, x, cz_z, half_z)
    };
    if !(MIN_HALF..=BOARDWALK_MAX_HALF).contains(&half) || !pool(cx, cz) {
        return None;
    }
    // Steppable banks (same trap as bridges: a deck the hero can't step off).
    let bank = |sign: f32| -> Option<f32> {
        let mut d = half;
        while d <= half + OVERHANG + 1.0 {
            let (px, pz) = if across_x { (cx + sign * d, cz) } else { (cx, cz + sign * d) };
            if let Some(y) = crate::worldmap::ground_at_world(px, pz) {
                return Some(y);
            }
            d += 0.5;
        }
        None
    };
    let ya = bank(1.0)?;
    let yb = bank(-1.0)?;
    if (ya - yb).abs() > BANK_STEP {
        return None;
    }
    Some(Span {
        cx,
        cz,
        half: half + OVERHANG,
        half_z: BOARDWALK_HALF_Z,
        across_x,
        base_y: ya.min(yb),
        kind: DeckKind::Boardwalk,
    })
}

/// A road's river-crossing midpoint can sit a touch off the channel's narrow axis (the centreline
/// crosses on the diagonal), so probe at the point and then on a small expanding ring for the first
/// spot that validates as a real deck. The snap (probe ring + `water_run` recentring) can walk the
/// deck centre off the road that justified it — reject those, or the "bridge only where a path
/// crosses" invariant breaks (first bitten by the MAP_SCALE 2.6 bump). `None` if nothing nearby is
/// a clean ON-ROAD crossing (e.g. the road forded a too-wide span) — then that crossing simply
/// gets no bridge.
fn nearest_crossing(x: f32, z: f32) -> Option<Span> {
    let on_road_deck = |s: &Span| crate::roads::on_road(s.cx, s.cz);
    if let Some(s) = crossing_at(x, z).filter(on_road_deck) {
        return Some(s);
    }
    let mut r = 1.0;
    while r <= 4.0 {
        let mut a = 0.0;
        while a < std::f32::consts::TAU {
            if let Some(s) = crossing_at(x + r * a.cos(), z + r * a.sin()).filter(on_road_deck) {
                return Some(s);
            }
            a += std::f32::consts::FRAC_PI_4;
        }
        r += 1.0;
    }
    None
}

/// If `(x, z)` is river water on a clean, *useful*, *walkable* narrow crossing, return the centred
/// deck span. Measures the channel width along X and along Z; the deck spans the NARROWER axis
/// (bank to bank). Two placement bugs are gated here:
///  - **useless bridges at a river's end** — a genuine crossing has river continuing along the
///    flow axis on BOTH sides; at a head/mouth the channel dead-ends and you'd walk around it;
///  - **bridges you get stuck on** — the flat deck top must sit within one walkable step of the
///    terrain at BOTH immediate banks, or the hero can't step off and has to jump.
fn crossing_at(x: f32, z: f32) -> Option<Span> {
    if !is_river_world(x, z) {
        return None;
    }
    // The rival fort's forced-flat plateau buries the pure river channel under sand (`classify`
    // levels it before `is_river`), so a deck here would span an invisible river and read as a
    // random bridge in the dunes. Skip the whole fort zone — mirrors `near_fort` scatter rejection.
    if crate::rival::fort_flat_zone(x, z) {
        return None;
    }
    let (cx_x, half_x) = water_run(x, z, true)?; // channel along X
    let (cz_z, half_z) = water_run(x, z, false)?; // channel along Z
    // Narrower axis = the crossing direction (the deck spans it, bank to bank).
    let (across_x, cx, cz, half) = if half_x <= half_z {
        (true, cx_x, z, half_x)
    } else {
        (false, x, cz_z, half_z)
    };
    if !(MIN_HALF..=MAX_HALF).contains(&half) {
        return None;
    }
    // The deck CENTRE must actually be over water. `water_run` only measures the axis-aligned span,
    // so on a narrow, meandering, diagonal channel the computed midpoint can fall just off the real
    // (perpendicular-narrow) water — a deck there sits on dry land and a mover foots on the ground,
    // not the planks. Reject those so every deck genuinely bridges water.
    if !is_river_world(cx, cz) {
        return None;
    }

    // Useless-bridge gate: a real crossing has river running along the FLOW axis (perpendicular to
    // the deck) on BOTH sides. At a river head it just stops. Probe a short perpendicular window
    // so a gently meandering channel still registers a little way upstream/downstream.
    let river_along_flow = |s: f32| {
        [-1.5f32, -0.75, 0.0, 0.75, 1.5].iter().any(|&w| {
            let (px, pz) = if across_x { (cx + w, cz + s) } else { (cx + s, cz + w) };
            is_river_world(px, pz)
        })
    };
    if !(river_along_flow(RIVER_CONTINUE) && river_along_flow(-RIVER_CONTINUE)) {
        return None;
    }

    // Stuck-on-the-deck gate: find the first solid land stepping outward from the water on each
    // side of the span axis (a sea mouth has none → `None` → rejected, as a river mouth should
    // be). A flat deck can only meet both banks if they're near-level.
    let bank = |sign: f32| -> Option<f32> {
        let mut d = half;
        while d <= half + OVERHANG + 1.0 {
            let (px, pz) = if across_x { (cx + sign * d, cz) } else { (cx, cz + sign * d) };
            if let Some(y) = crate::worldmap::ground_at_world(px, pz) {
                return Some(y);
            }
            d += 0.5;
        }
        None
    };
    let ya = bank(1.0)?;
    let yb = bank(-1.0)?;
    if (ya - yb).abs() > BANK_STEP {
        return None; // skewed banks — a flat deck would leave one end an un-steppable cliff
    }
    // Deck top sits a hair above the LOWER bank (`base_y + 0.25`): the hero steps up ≤0.25 from
    // it and down onto the higher bank — both within `BANK_STEP`, so neither end traps him.
    Some(Span {
        cx,
        cz,
        half: half + OVERHANG,
        half_z: DECK_HALF_Z,
        across_x,
        base_y: ya.min(yb),
        kind: DeckKind::Bridge,
    })
}

/// Walk both ways from `(x, z)` along one axis (`x_axis` ? X : Z) to the channel banks; return
/// the channel's `(centre, half_width)` on that axis. `None` if the run overruns `MAX_HALF` (a
/// wide span — not a tidy crossing) so the caller bails.
fn water_run(x: f32, z: f32, x_axis: bool) -> Option<(f32, f32)> {
    water_run_of(x, z, x_axis, MAX_HALF, &|px, pz| is_river_world(px, pz))
}

/// [`water_run`] generalised over the water predicate + max half-width (boardwalks measure the
/// swamp pools, which are broader than a river channel).
fn water_run_of(
    x: f32,
    z: f32,
    x_axis: bool,
    max_half: f32,
    wet_at: &dyn Fn(f32, f32) -> bool,
) -> Option<(f32, f32)> {
    let limit = max_half * 2.0 + 2.0;
    let wet = |d: f32| if x_axis { wet_at(x + d, z) } else { wet_at(x, z + d) };
    let mut pos = 0.5;
    while wet(pos) {
        pos += 0.5;
        if pos > limit {
            return None;
        }
    }
    let mut neg = 0.5;
    while wet(-neg) {
        neg += 0.5;
        if neg > limit {
            return None;
        }
    }
    let centre = (if x_axis { x } else { z }) + (pos - neg) * 0.5;
    Some((centre, (pos + neg) * 0.5))
}

/// The span whose deck covers `(wx, wz)`, if any. The long axis is X when `across_x`, else Z.
fn span_at(wx: f32, wz: f32) -> Option<&'static Span> {
    spans().iter().find(|s| {
        let (along, across) =
            if s.across_x { (wx - s.cx, wz - s.cz) } else { (wz - s.cz, wx - s.cx) };
        along.abs() <= s.half && across.abs() <= s.half_z
    })
}

/// Is `(wx, wz)` on a bridge deck? Consulted by `navgrid::standable` so A* can cross the river.
pub fn is_on_bridge(wx: f32, wz: f32) -> bool {
    span_at(wx, wz).is_some()
}

/// World-XZ centres of every bridge deck. The road network threads its river crossings through
/// these so a path lands on a real deck instead of plunging into the water (`roads::wander`).
pub fn centers() -> Vec<Vec2> {
    spans().iter().map(|s| Vec2::new(s.cx, s.cz)).collect()
}

/// Is `(wx, wz)` on or hugging a bridge deck (footprint padded by `pad`)? Placement code
/// (worldmap scatter, verbs props/chests) rejects these spots — the deck overhangs `OVERHANG`
/// onto solid land at each end, so without this trees/props spawn up through the planks.
pub fn near_bridge(wx: f32, wz: f32, pad: f32) -> bool {
    spans().iter().any(|s| {
        let (along, across) =
            if s.across_x { (wx - s.cx, wz - s.cz) } else { (wz - s.cz, wx - s.cx) };
        along.abs() <= s.half + pad && across.abs() <= s.half_z + pad
    })
}

/// Walkable deck-top Y at `(wx, wz)` if it's on a bridge, else `None`. The hero ORs this onto
/// `worldmap::ground_at_world` (which is terrain-only and reads `None` over the river) so he can
/// stand + ground on the planks. Deck transform sits at `base_y + 0.2`; planks are 0.1 thick →
/// their top is `base_y + 0.25`, where the feet rest. `base_y` is the validated bank height
/// stored on the span, so this never recurses into a bridge lookup.
pub fn deck_y_at(wx: f32, wz: f32) -> Option<f32> {
    span_at(wx, wz).map(|s| s.base_y + 0.25)
}

// ── mesh ───────────────────────────────────────────────────────────────────────────
// All colour lives in `ATTRIBUTE_COLOR` (the shared white prop material batches every
// deck), so the planks' "texture" is faked the only way the contract allows: per-plank
// tone jitter, a damp moss tint over the deep-water span, iron nail heads, and a dark
// sub-deck so the inter-plank gaps read as shadow grooves instead of see-through holes.

/// Weathered plank tones: warm-light, dark, mid-warm, sun-bleached grey.
const PLANK_TONES: [u32; 4] = [0x8a5a32, 0x664020, 0x7c4d28, 0x6f5e46];
const MOSS_TINT: u32 = 0x46552c; // damp algae on the planks over open water
const GROOVE: u32 = 0x32241a; // gap / under-plank shadow
const RAIL: u32 = 0x5a3a22;
const RAIL_DK: u32 = 0x45301c;
const NAIL: u32 = 0x2a221a; // dark iron spike head

/// Small deterministic 0..1 hash so each deck weathers the same way every run.
fn hash01(mut s: u32) -> f32 {
    s ^= s >> 16;
    s = s.wrapping_mul(0x7feb_352d);
    s ^= s >> 15;
    s = s.wrapping_mul(0x846c_a68b);
    s ^= s >> 16;
    (s & 0x00ff_ffff) as f32 / 16_777_216.0
}
/// A tinted cuboid taking a linear colour directly (so callers can pass jittered tones).
fn bx(w: f32, h: f32, d: f32, off: Vec3, c: [f32; 4]) -> Mesh {
    tinted(Cuboid::new(w, h, d).mesh().build().translated_by(off), c)
}
fn mix4(a: [f32; 4], b: [f32; 4], t: f32) -> [f32; 4] {
    [a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t, a[2] + (b[2] - a[2]) * t, 1.0]
}

/// One deck mesh spanning `2·half_x` across X (local space; deck top at y≈0). `seed`
/// drives the per-plank weathering so the deck looks worn but stays deterministic.
fn deck_mesh(half_x: f32, seed: u32) -> Mesh {
    let len = half_x * 2.0;
    let dz = DECK_HALF_Z;
    let mut parts: Vec<Mesh> = Vec::new();

    // Dark continuous sub-deck just under the planks → plank gaps read as shadow lines.
    parts.push(bx(len, 0.06, dz * 2.0 - 0.06, Vec3::new(0.0, -0.04, 0.0), lin(GROOVE)));

    let planks = (len * 1.6).max(4.0) as i32;
    let cell = len / planks as f32;
    for i in 0..planks {
        let x = -half_x + (i as f32 + 0.5) * cell;
        let h = hash01(seed ^ (i as u32).wrapping_mul(0x9e37_79b1));
        let tone = if h < 0.18 {
            PLANK_TONES[3] // weathered grey
        } else if h < 0.52 {
            PLANK_TONES[0] // warm light
        } else if h < 0.82 {
            PLANK_TONES[2] // mid-warm
        } else {
            PLANK_TONES[1] // dark
        };
        let v = 0.82 + hash01(seed.wrapping_add((i as u32).wrapping_mul(0x2545_f491))) * 0.34; // brightness jitter
        // Damp moss creeps in over the centre span (the deepest, dankest water).
        let center = (1.0 - (x / half_x).abs()).clamp(0.0, 1.0);
        let moss = center * center * 0.4 * hash01(seed ^ 0x55 ^ i as u32);
        let col = mix4(lin_scaled(tone, v), lin(MOSS_TINT), moss);
        // A touch of per-plank warp/wear so the deck surface isn't dead-flat.
        let warp = (hash01(seed ^ 0xab ^ i as u32) - 0.5) * 0.02;
        parts.push(bx(cell * 0.84, 0.1, dz * 2.0, Vec3::new(x, warp, 0.0), col));
        // Two iron spike heads pinning each plank down at the cross-beams.
        for sz in [-dz * 0.78, dz * 0.78] {
            parts.push(bx(0.05, 0.03, 0.05, Vec3::new(x, 0.065 + warp, sz), lin(NAIL)));
        }
    }

    // Proper hand-railing on each side: a top rail, a mid rail, balusters and end posts.
    for (si, sz) in [-dz, dz].into_iter().enumerate() {
        parts.push(bx(len, 0.08, 0.1, Vec3::new(0.0, 0.5, sz), lin(RAIL))); // top rail
        parts.push(bx(len, 0.06, 0.08, Vec3::new(0.0, 0.26, sz), lin(RAIL_DK))); // mid rail
        let bals = (len / 1.3).max(2.0) as i32;
        for b in 0..=bals {
            let bxp = -half_x + b as f32 / bals as f32 * len;
            let j = 1.0 + (hash01(seed ^ (b as u32 * 131) ^ si as u32 * 7) - 0.5) * 0.12;
            parts.push(bx(0.07, 0.5, 0.07, Vec3::new(bxp, 0.22, sz), lin_scaled(RAIL, j)));
        }
        for sx in [-half_x + 0.2, half_x - 0.2] {
            parts.push(bx(0.15, 0.62, 0.15, Vec3::new(sx, 0.26, sz), lin(RAIL_DK))); // end post
        }
    }
    for sz in [-dz + 0.3, dz - 0.3] {
        parts.push(bx(len, 0.12, 0.14, Vec3::new(0.0, -0.12, sz), lin(PLANK_TONES[1]))); // underbeam
    }
    let mut it = parts.into_iter();
    let mut base = it.next().unwrap();
    for p in it {
        base.merge(&p).expect("bridge parts share attributes");
    }
    base.duplicate_vertices();
    base.compute_flat_normals();
    base
}

/// A swamp BOARDWALK mesh spanning `2·half_x` across X (local; deck top y≈0): a narrow run of
/// weathered planks on square stilt posts that drop through the bog water, low corner stubs
/// instead of a full railing — a marsh walkway, not a cart bridge.
fn boardwalk_mesh(half_x: f32, seed: u32) -> Mesh {
    let len = half_x * 2.0;
    let dz = BOARDWALK_HALF_Z;
    let mut parts: Vec<Mesh> = Vec::new();

    // Dark sub-deck for shadowed plank gaps (same trick as the bridge).
    parts.push(bx(len, 0.05, dz * 2.0 - 0.05, Vec3::new(0.0, -0.04, 0.0), lin(GROOVE)));
    let planks = (len * 1.5).max(4.0) as i32;
    let cell = len / planks as f32;
    for i in 0..planks {
        let x = -half_x + (i as f32 + 0.5) * cell;
        let h = hash01(seed ^ (i as u32).wrapping_mul(0x9e37_79b1));
        let tone = if h < 0.30 { PLANK_TONES[3] } else if h < 0.65 { PLANK_TONES[1] } else { PLANK_TONES[2] };
        let v = 0.72 + hash01(seed.wrapping_add((i as u32).wrapping_mul(0x2545_f491))) * 0.30;
        // Bog damp: moss creeps over most of the walkway, heavier mid-span.
        let center = (1.0 - (x / half_x).abs()).clamp(0.0, 1.0);
        let moss = 0.15 + center * 0.35 * hash01(seed ^ 0x55 ^ i as u32);
        let col = mix4(lin_scaled(tone, v), lin(MOSS_TINT), moss);
        let warp = (hash01(seed ^ 0xab ^ i as u32) - 0.5) * 0.03;
        parts.push(bx(cell * 0.80, 0.08, dz * 2.0, Vec3::new(x, warp, 0.0), col));
    }
    // Stilt posts: pairs every ~1.7u, dropping from the deck through the water surface (the
    // transform sits at base_y+0.2 ≈ 0.2 and the bog water at −0.4, so 1.0 of post reaches
    // through the surface into the murk).
    let pairs = (len / 1.7).max(2.0) as i32;
    for pi in 0..=pairs {
        let x = -half_x + pi as f32 / pairs as f32 * len;
        let j = 0.85 + hash01(seed ^ (pi as u32 * 977)) * 0.3;
        for sz in [-dz + 0.1, dz - 0.1] {
            parts.push(bx(0.11, 1.0, 0.11, Vec3::new(x, -0.5, sz), lin_scaled(RAIL_DK, j)));
        }
    }
    // Low corner stubs (mooring-post look) instead of railings.
    for sx in [-half_x + 0.15, half_x - 0.15] {
        for sz in [-dz + 0.1, dz - 0.1] {
            parts.push(bx(0.12, 0.34, 0.12, Vec3::new(sx, 0.14, sz), lin(RAIL)));
        }
    }
    let mut it = parts.into_iter();
    let mut base = it.next().unwrap();
    for p in it {
        base.merge(&p).expect("boardwalk parts share attributes");
    }
    base.duplicate_vertices();
    base.compute_flat_normals();
    base
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The core invariant of the rework: a bridge exists ONLY where a path crosses the river. Every
    /// placed deck must therefore sit on a road — its centre reads as on-road in the baked field.
    /// (A river no road crosses simply gets no deck now; that's intended — a bridge to nowhere was
    /// the bug. At least one crossing is bridged, since the trunks fan out across the island.)
    #[test]
    fn every_bridge_is_on_a_road() {
        let s = spans();
        assert!(!s.is_empty(), "expected at least one road→river crossing to be bridged, got 0");
        for b in s {
            assert!(
                crate::roads::on_road(b.cx, b.cz),
                "bridge at ({:.1}, {:.1}) is not on a road — bridges must only sit where a path crosses",
                b.cx,
                b.cz
            );
        }
    }

    /// Every placed deck must be a USEFUL, STEPPABLE crossing — the two placement bugs:
    ///  - **useless at a river's end**: river must run along the flow axis on both sides of the
    ///    deck (a head/mouth dead-ends → you'd walk around it);
    ///  - **stuck on the deck**: the flat plank top must be within one walkable step of the land
    ///    at BOTH immediate banks, or the hero can't step off and has to jump.
    #[test]
    fn every_deck_is_a_useful_steppable_crossing() {
        for s in spans() {
            let top = s.base_y + 0.25;
            // Both banks reachable from the deck top within one terrace step.
            for sign in [1.0f32, -1.0] {
                let mut d = s.half - OVERHANG; // back at the water edge
                let mut bank = None;
                while d <= s.half + 1.0 {
                    let (px, pz) =
                        if s.across_x { (s.cx + sign * d, s.cz) } else { (s.cx, s.cz + sign * d) };
                    if let Some(y) = crate::worldmap::ground_at_world(px, pz) {
                        bank = Some(y);
                        break;
                    }
                    d += 0.5;
                }
                let bank = bank.unwrap_or_else(|| panic!("deck end at ({}, {}) has no bank", s.cx, s.cz));
                assert!(
                    (bank - top).abs() <= crate::steer::MAX_STEP,
                    "un-steppable bank {bank} vs deck top {top} at ({}, {}) — hero would be stuck",
                    s.cx,
                    s.cz
                );
            }
            // River continues on both sides → not a dead-end bridge (same perpendicular probe
            // window `crossing_at` uses, so a meandering channel registers). Boardwalks span
            // pool BLOBS — no flow axis, so the gate doesn't apply.
            if s.kind != DeckKind::Bridge {
                continue;
            }
            let cont = |off: f32| {
                [-1.5f32, -0.75, 0.0, 0.75, 1.5].iter().any(|&w| {
                    let (px, pz) =
                        if s.across_x { (s.cx + w, s.cz + off) } else { (s.cx + off, s.cz + w) };
                    is_river_world(px, pz)
                })
            };
            assert!(
                cont(RIVER_CONTINUE) && cont(-RIVER_CONTINUE),
                "dead-end bridge at ({}, {}) — river doesn't continue on both sides",
                s.cx,
                s.cz
            );
        }
    }

    /// A mover standing on a deck must FOOT on the deck, not on the (missing) terrain under it.
    /// `worldmap::ground_at_world` reads `None` over the carved river, so any mover that grounds
    /// off raw terrain freezes its Y at the bank and floats/wedges on the planks (the wildlife +
    /// NPC bridge bug). `steer::footing` ORs the deck in — this asserts that for every span centre
    /// the deck overrides the empty terrain, so the shared footing keeps movers flush on the deck.
    #[test]
    fn movers_foot_on_the_deck_not_the_void() {
        for s in spans() {
            let deck = deck_y_at(s.cx, s.cz).expect("a span centre is on its own deck");
            let foot = crate::steer::footing(s.cx, s.cz).expect("footing falls back to the deck");
            assert!(
                (foot - deck).abs() < 1e-3,
                "footing {foot} != deck {deck} at span ({}, {})",
                s.cx,
                s.cz
            );
        }
    }
}

/// Spawn a deck at each river crossing. Called from `worldmap::build` (after terrain).
pub fn populate(commands: &mut Commands, meshes: &mut Assets<Mesh>, materials: &mut Assets<StandardMaterial>) {
    let mat = materials.add(StandardMaterial { base_color: Color::WHITE, perceptual_roughness: 0.85, ..default() });
    for s in spans() {
        // `deck_mesh` always spans X; rotate 90° about Y for a Z-spanning (across_x = false) deck.
        let rot = if s.across_x {
            Quat::IDENTITY
        } else {
            Quat::from_rotation_y(std::f32::consts::FRAC_PI_2)
        };
        // Stable per-deck weathering seed from its world position (order-independent).
        let seed = s.cx.to_bits() ^ s.cz.to_bits().rotate_left(13) ^ 0x9e37_79b9;
        let mesh = match s.kind {
            DeckKind::Bridge => deck_mesh(s.half, seed),
            DeckKind::Boardwalk => boardwalk_mesh(s.half, seed),
        };
        commands.spawn((
            Mesh3d(meshes.add(mesh)),
            MeshMaterial3d(mat.clone()),
            Transform {
                translation: Vec3::new(s.cx, s.base_y + 0.2, s.cz),
                rotation: rot,
                ..default()
            },
            BiomeEntity,
        ));
    }
}
