//! **Roads** — a map-wide network of natural, curving dirt paths linking the castle to every
//! interesting place on the island. NOT geometry: the network is rasterised **once** into a baked
//! [`RoadField`] (a 2-D strength grid), and every consumer just samples it — O(1), allocation-free:
//!
//! * `worldmap::ground_color` blends [`road_strength`] into the terrain vertex colour (the brown
//!   path), exactly like the old draft — same surface as the lawn, no raised slab.
//! * the biome scatter pass rejects any tree / prop / ground-cover that lands [`on_road`], so
//!   **nothing grows on a path**.
//! * `player::movement` reads [`speed_mult`] for a small road-travel speed bonus.
//!
//! The expensive part (jittered Catmull-Rom curves + brush stamping) runs lazily on the first
//! query — which happens during the world's ground bake — and is cached for the process. The field
//! is pure derived data (deterministic from the world seed + map), so it is neither saved nor reset:
//! it regenerates identically on every world build.
//!
//! Design: `docs/superpowers/specs/2026-06-30-organic-road-network-design.md`.

use crate::worldmap::{ground_at_world, is_river_world};
use bevy::prelude::*;
use std::sync::OnceLock;

// ── Tunables ──────────────────────────────────────────────────────────────────────
/// ARTERY half-width (world units) — the wide main roads (trunks / ring / landmark+camp spurs).
/// Full-strength core within [`EDGE`]·HALF_W, fading to 0 at HALF_W. Also the brush's MAX radius
/// (arteries are the widest curve), so [`RoadField::stamp`]'s bounding box / the pad use it.
/// 1.7 → 2.3 (map-character feedback: "ścieżki mogłyby być ciut szersze, łatwo się zgubić") —
/// a wider packed band is the single cheapest legibility win.
const HALF_W: f32 = 2.3;
/// CAPILLARY half-width — the thin secondary trails that branch off the arteries to thread the
/// space between main routes, so (almost) everywhere is reachable by *some* path without the map
/// becoming all-road. Deliberately ~half an artery: a visible footpath, not a highway.
const CAP_HALF_W: f32 = 1.05;
/// Fraction of the half-width that stays full-strength packed earth before the soft edge begins.
const EDGE: f32 = 0.45;
/// Scatter (trees/props/cover) is rejected where the field exceeds this — keeps paths bare. Kept
/// low so the cleared strip matches the *visible* worn-dirt tint (which shows from strength ≈0.1):
/// at a higher cutoff, props kept growing on the tinted road FRINGE — the swamp's flat moss discs
/// read as "green circles on a broken road", and rock-biome paths never cleared a visible corridor.
const GROW_CUTOFF: f32 = 0.12;
/// Movement speed bonus at a road centreline (player moves a *little* faster on a road).
const SPEED_BONUS: f32 = 0.15;
/// Below this field strength a road gives no speed help (so the soft fringe doesn't buff you).
const SPEED_CUTOFF: f32 = 0.25;
/// One wander waypoint roughly every N units of an edge (more → curvier). Raised 15→20: the old
/// spacing put a wander point every ~15u which, with the old amplitude, read as *too* serpentine —
/// players complained the paths were a maze. Fewer control points = longer, calmer organic sweeps.
const WAYPOINT_SPACING: f32 = 20.0;
/// Lateral wander amplitude (world units), tapered to 0 at both endpoints so curves hit their nodes.
/// Dialled 7.0→4.5: still curves organically (it never goes straight), just stops snaking so hard
/// that a short hop between two places becomes a long detour.
const JITTER: f32 = 4.5;
/// Centreline rasterisation grid cell (world units). Smaller = crisper edges, more memory.
const CELL: f32 = 0.6;
/// At most this many spurs branch off the trunk/ring network to minor POIs (camps). Raised 6→12 so
/// far-flung camps — e.g. the ork camp out at the rocky map edge — actually get a road, not just the
/// handful nearest the network.
const SPUR_CAP: usize = 12;
/// A spur is only drawn if its camp sits within this distance of the existing network. Raised 36→70
/// so an edge-of-map camp in the mountains still connects instead of being left roadless.
const SPUR_MAX_LEN: f32 = 70.0;

// ── Capillary network (thin space-filling trails) ───────────────────────────────────
/// Sprout a capillary off an artery roughly every N units of arc length (jittered). Raised 21→30
/// (and the recursive second-generation forking dropped) after the first pass read as a *random*
/// thicket of stubs: fewer, single, clean side-trails that clearly lead off the road look
/// purposeful, not like noise — while still putting most of the woods a short walk from a path.
const CAP_SPACING: f32 = 30.0;
/// Capillary branch length range (world units) — long enough to push into the woods between
/// arteries, short enough to stay a side-trail, not a second trunk.
const CAP_LEN: (f32, f32) = (15.0, 27.0);
/// Hard cap on total capillaries (safety bound so a future denser network can't explode the bake).
const CAP_CAP: usize = 160;

// ── Mulberry32 (same deterministic RNG the scatter uses) ────────────────────────────
struct Rng(u32);
impl Rng {
    fn next(&mut self) -> f32 {
        self.0 = self.0.wrapping_add(0x6d2b_79f5);
        let mut t = self.0;
        t = (t ^ (t >> 15)).wrapping_mul(t | 1);
        t ^= t.wrapping_add((t ^ (t >> 7)).wrapping_mul(t | 61));
        ((t ^ (t >> 14)) as f32) / 4_294_967_296.0
    }
}

// ── The baked field ─────────────────────────────────────────────────────────────────
/// A 2-D grid of road strength `[0,1]` covering the network's bounding box. `data[z*w + x]`.
struct RoadField {
    ox: f32,
    oz: f32,
    w: usize,
    h: usize,
    data: Vec<f32>,
}

impl RoadField {
    /// Bilinear lookup at world `(wx, wz)`; 0 outside the grid.
    fn sample(&self, wx: f32, wz: f32) -> f32 {
        let fx = (wx - self.ox) / CELL;
        let fz = (wz - self.oz) / CELL;
        if fx < 0.0 || fz < 0.0 || fx >= (self.w - 1) as f32 || fz >= (self.h - 1) as f32 {
            return 0.0;
        }
        let x0 = fx.floor() as usize;
        let z0 = fz.floor() as usize;
        let tx = fx - x0 as f32;
        let tz = fz - z0 as f32;
        let g = |x: usize, z: usize| self.data[z * self.w + x];
        let top = g(x0, z0) * (1.0 - tx) + g(x0 + 1, z0) * tx;
        let bot = g(x0, z0 + 1) * (1.0 - tx) + g(x0 + 1, z0 + 1) * tx;
        top * (1.0 - tz) + bot * tz
    }

    /// Max-blend a round brush (radius `half`, full-strength core `core`) centred at `pt`. The
    /// half-width is per-curve now (wide arteries vs thin capillaries), so it's passed in.
    fn stamp(&mut self, pt: Vec2, half: f32, core: f32) {
        let r = half;
        let minx = (((pt.x - r - self.ox) / CELL).floor() as i32).max(0);
        let maxx = (((pt.x + r - self.ox) / CELL).ceil() as i32).min(self.w as i32 - 1);
        let minz = (((pt.y - r - self.oz) / CELL).floor() as i32).max(0);
        let maxz = (((pt.y + r - self.oz) / CELL).ceil() as i32).min(self.h as i32 - 1);
        for cz in minz..=maxz {
            for cx in minx..=maxx {
                let c = Vec2::new(self.ox + cx as f32 * CELL, self.oz + cz as f32 * CELL);
                let d = c.distance(pt);
                if d > r {
                    continue;
                }
                let s = if d <= core { 1.0 } else { 1.0 - (d - core) / (r - core) };
                let i = cz as usize * self.w + cx as usize;
                if s > self.data[i] {
                    self.data[i] = s;
                }
            }
        }
    }
}

// ── Wheel-rut texture bake (map-character overhaul pass 2) ─────────────────────────
/// Twin cart-wheel grooves pressed either side of every ARTERY centreline: distance of each rut
/// from the centreline and its half-width (world units). Rendered per-FRAGMENT via a baked
/// texture the terrain shader samples — the ~0.5u grooves are far below the terrain mesh's
/// 1-unit vertex-colour resolution, so painting them into vertex colours (or the 0.6u strength
/// field) just aliases them away.
const RUT_D: f32 = 0.72;
const RUT_W: f32 = 0.34;
/// Rut-texture resolution (world units per texel).
const RUT_CELL: f32 = 0.35;

/// Bake the rut mask: R8 texel data + the world→UV mapping (`xy` = min corner, `zw` = 1/extent),
/// same contract as `worldmap::bake_shore_distance`. Consumed by `terrain::make_material`.
pub fn bake_rut_mask() -> (Vec<u8>, usize, usize, Vec4) {
    let curves = network();
    let mut lo = Vec2::splat(f32::MAX);
    let mut hi = Vec2::splat(f32::MIN);
    for (c, _) in curves {
        for p in c {
            lo = lo.min(*p);
            hi = hi.max(*p);
        }
    }
    let pad = RUT_D + RUT_W + 1.0;
    lo -= pad;
    hi += pad;
    let w = (((hi.x - lo.x) / RUT_CELL).ceil() as usize) + 1;
    let h = (((hi.y - lo.y) / RUT_CELL).ceil() as usize) + 1;
    let mut grid = vec![0.0_f32; w * h];
    for (c, half) in curves {
        if *half < HALF_W - 0.01 {
            continue; // cart arteries only — footpaths and mountain trails carry no wheel ruts
        }
        for win in c.windows(2) {
            let (p0, p1) = (win[0], win[1]);
            let steps = (p0.distance(p1) / (RUT_CELL * 0.7)).ceil().max(1.0) as usize;
            for s in 0..=steps {
                let pt = p0.lerp(p1, s as f32 / steps as f32);
                let r = RUT_D + RUT_W;
                let minx = (((pt.x - r - lo.x) / RUT_CELL).floor() as i32).max(0);
                let maxx = (((pt.x + r - lo.x) / RUT_CELL).ceil() as i32).min(w as i32 - 1);
                let minz = (((pt.y - r - lo.y) / RUT_CELL).floor() as i32).max(0);
                let maxz = (((pt.y + r - lo.y) / RUT_CELL).ceil() as i32).min(h as i32 - 1);
                for cz in minz..=maxz {
                    for cx in minx..=maxx {
                        let tc = Vec2::new(lo.x + cx as f32 * RUT_CELL, lo.y + cz as f32 * RUT_CELL);
                        let d = tc.distance(pt);
                        let rs = 1.0 - ((d - RUT_D).abs() / RUT_W).min(1.0);
                        let i = cz as usize * w + cx as usize;
                        if rs > grid[i] {
                            grid[i] = rs;
                        }
                    }
                }
            }
        }
    }
    let data: Vec<u8> = grid.iter().map(|v| (v.clamp(0.0, 1.0) * 255.0) as u8).collect();
    let region = Vec4::new(lo.x, lo.y, 1.0 / (w as f32 * RUT_CELL), 1.0 / (h as f32 * RUT_CELL));
    (data, w, h, region)
}

/// The built network — `(centreline, half_width)` per curve — cached for the process. BOTH the
/// rasterised strength field AND bridge placement derive from this, so they agree on where roads
/// run (a deck only lands where an artery actually crosses a river).
fn network() -> &'static [(Vec<Vec2>, f32)] {
    static NET: OnceLock<Vec<(Vec<Vec2>, f32)>> = OnceLock::new();
    NET.get_or_init(build_curves)
}

/// Process-lifetime cache. Built on first query (during the ground bake) and reused thereafter.
fn field() -> &'static RoadField {
    static FIELD: OnceLock<RoadField> = OnceLock::new();
    FIELD.get_or_init(build_field)
}

/// World-XZ midpoints of every spot where an ARTERY centreline crosses river water. `bridges.rs`
/// consumes these: a deck is laid at each, so a bridge exists ONLY where a path crosses the river,
/// and every such crossing gets one. Capillaries are excluded — they're kept off the water (no
/// orphan path plunging into a river without a deck), so only the wide main routes get bridged.
pub fn river_crossings() -> Vec<Vec2> {
    water_crossings(&|x, z| is_river_world(x, z))
}

/// Artery↔bog-pool crossings (same contract as [`river_crossings`], sampling the swamp pools):
/// `bridges` lays a BOARDWALK deck at each, so the swamp roads cross standing water on planks.
pub fn pool_crossings() -> Vec<Vec2> {
    water_crossings(&|x, z| crate::worldmap::is_pool_world(x, z))
}

/// Midpoints of every contiguous run where an ARTERY centreline crosses `wet` water.
fn water_crossings(wet_at: &dyn Fn(f32, f32) -> bool) -> Vec<Vec2> {
    let mut out: Vec<Vec2> = Vec::new();
    for (c, half) in network() {
        if *half < HALF_W - 0.01 {
            continue; // arteries only
        }
        // Walk the polyline; each contiguous run of water samples is one crossing → its midpoint.
        let mut wet: Vec<Vec2> = Vec::new();
        let flush = |wet: &mut Vec<Vec2>, out: &mut Vec<Vec2>| {
            if !wet.is_empty() {
                let mid = wet.iter().fold(Vec2::ZERO, |a, &b| a + b) / wet.len() as f32;
                out.push(mid);
                wet.clear();
            }
        };
        for w in c.windows(2) {
            let steps = (w[0].distance(w[1]) / 0.6).ceil().max(1.0) as usize;
            for s in 0..=steps {
                let p = w[0].lerp(w[1], s as f32 / steps as f32);
                if wet_at(p.x, p.y) {
                    wet.push(p);
                } else {
                    flush(&mut wet, &mut out);
                }
            }
        }
        flush(&mut wet, &mut out);
    }
    out
}

// ── Public query API (all O(1) field samples) ──────────────────────────────────────
/// Road strength `[0,1]` at world `(wx, wz)` — 1 on a centreline, fading to 0 off the path.
/// `worldmap::ground_color` blends this into the terrain as the worn-dirt path tint.
pub fn road_strength(wx: f32, wz: f32) -> f32 {
    field().sample(wx, wz)
}

/// Is world `(wx, wz)` on a path (strongly enough that nothing should grow there)? The biome
/// scatter pass calls this to keep trees / props / ground-cover off the roads.
pub fn on_road(wx: f32, wz: f32) -> bool {
    field().sample(wx, wz) > GROW_CUTOFF
}

/// World-space junction points of the road network — the forks where a spur leaves an artery and
/// the trunk/ring endpoints at the biome anchors. `wayside::populate` plants a signpost at each.
/// (Collected as `build_curves` runs; touching [`network`] first guarantees they're baked.)
pub fn junctions() -> Vec<Vec2> {
    let _ = network();
    JUNCTIONS.lock().expect("junctions poisoned").clone()
}
static JUNCTIONS: std::sync::Mutex<Vec<Vec2>> = std::sync::Mutex::new(Vec::new());

/// Record a junction (dedup within 8u; skip the castle's own yard — the gates have their own
/// dressing). Called only from within the one-time `build_curves` bake.
fn note_junction(p: Vec2) {
    if p.length() < 28.0 {
        return;
    }
    let mut j = JUNCTIONS.lock().expect("junctions poisoned");
    if j.iter().all(|q| q.distance(p) > 8.0) {
        j.push(p);
    }
}

/// The full centreline network for wayside furniture placement: every ARTERY polyline (wide
/// routes only — furniture on capillary footpaths reads as clutter).
pub fn artery_polylines() -> Vec<Vec<Vec2>> {
    network().iter().filter(|(_, half)| *half >= HALF_W - 0.01).map(|(c, _)| c.clone()).collect()
}

/// Is `(wx, wz)` on OR within `pad` of a path? Probes the centre + four cardinal offsets, so a WIDE
/// flat cover disc (swamp moss/lily "plates") whose CENTRE sits just off the road but whose body
/// overhangs it is still rejected — `on_road` alone (centre-only) let those plates lap onto trails.
pub fn near_road(wx: f32, wz: f32, pad: f32) -> bool {
    on_road(wx, wz)
        || on_road(wx + pad, wz)
        || on_road(wx - pad, wz)
        || on_road(wx, wz + pad)
        || on_road(wx, wz - pad)
}

/// "Openness" `[0,1]` for vegetation density: **1 in the open woods** (no road anywhere near),
/// tapering to **0 at a path edge**. The scatter pass multiplies its extra density by this so the
/// forest thickens *between* the roads while the ground right beside a trail stays a touch clearer
/// (a natural cleared margin, not undergrowth crowding the path). Since the field is 0 beyond a
/// road's half-width, this is 1 across almost the whole map — only the ~road-width fringe tapers.
pub fn openness(wx: f32, wz: f32) -> f32 {
    1.0 - (road_strength(wx, wz) / GROW_CUTOFF).clamp(0.0, 1.0)
}

/// Movement multiplier at world `(wx, wz)`: 1.0 off-road, ramping to `1 + SPEED_BONUS` on a
/// centreline. The player moves a little faster when travelling by road.
pub fn speed_mult(wx: f32, wz: f32) -> f32 {
    let s = field().sample(wx, wz);
    if s <= SPEED_CUTOFF {
        1.0
    } else {
        1.0 + SPEED_BONUS * ((s - SPEED_CUTOFF) / (1.0 - SPEED_CUTOFF))
    }
}

// ── Network construction (runs once, inside `build_field`) ──────────────────────────
/// The five biome region centres in WORLD space — now read from `worldmap` (was a hardcoded
/// mirror of `REGIONS` that silently went stale on every region retune). Centres are the ring
/// SKELETON only; actual road endpoints go through `worldmap::biome_road_target` /
/// `biome_ring_node`, which swap a cliffy mesa's centre for a pass MOUTH so no road paints up
/// the shelf walls.
fn biome_centres() -> [Vec2; 5] {
    crate::worldmap::biome_centres_world()
}

/// Pull a wander waypoint back onto walkable land if it strayed onto water/off-map. Bounded; falls
/// back to the straight line. NB: roads no longer special-case bridges here — the dependency now
/// runs roads → bridges (a deck is placed wherever an artery crosses a river, see [`river_crossings`]),
/// so the road just keeps its control points on land and the spline crosses narrow channels between
/// them; the crossing is then bridged.
fn nudge(p: Vec2, toward: Vec2) -> Vec2 {
    let mut q = p;
    for _ in 0..6 {
        if ground_at_world(q.x, q.y).is_some() {
            return q;
        }
        q = q.lerp(toward, 0.45);
    }
    toward
}

/// Build one organic curve from `a` to `b`: jittered waypoints (tapered at the ends), smoothed
/// through a Catmull-Rom spline. Control points are kept on land; where the spline crosses a narrow
/// river between them, [`river_crossings`] picks it up and `bridges.rs` lays a deck there.
fn wander(a: Vec2, b: Vec2, seed: u32) -> Vec<Vec2> {
    let len = a.distance(b);
    let dir = (b - a).normalize_or_zero();
    if dir == Vec2::ZERO {
        return vec![a];
    }
    let perp = Vec2::new(-dir.y, dir.x);
    let n = (len / WAYPOINT_SPACING).floor() as i32;
    let mut rng = Rng(seed);

    // (t, point) controls between the endpoints — jittered waypoints, nudged onto land.
    let mut mids: Vec<(f32, Vec2)> = Vec::new();
    for i in 1..=n {
        let t = i as f32 / (n as f32 + 1.0);
        let base = a.lerp(b, t);
        let amp = JITTER * (std::f32::consts::PI * t).sin();
        let off = perp * ((rng.next() * 2.0 - 1.0) * amp);
        mids.push((t, nudge(base + off, base)));
    }
    mids.sort_by(|x, y| x.0.partial_cmp(&y.0).unwrap_or(std::cmp::Ordering::Equal));

    let mut ctrl = Vec::with_capacity(mids.len() + 2);
    ctrl.push(a);
    ctrl.extend(mids.into_iter().map(|(_, p)| p));
    ctrl.push(b);
    catmull(&ctrl)
}

/// Sample a Catmull-Rom spline through `ctrl` into a dense polyline (step ≈ [`CELL`]).
fn catmull(ctrl: &[Vec2]) -> Vec<Vec2> {
    if ctrl.len() < 3 {
        return ctrl.to_vec();
    }
    let mut out = Vec::new();
    for i in 0..ctrl.len() - 1 {
        let p0 = ctrl[i.saturating_sub(1)];
        let p1 = ctrl[i];
        let p2 = ctrl[i + 1];
        let p3 = ctrl[(i + 2).min(ctrl.len() - 1)];
        let steps = (p1.distance(p2) / CELL).ceil().max(1.0) as usize;
        for s in 0..steps {
            let t = s as f32 / steps as f32;
            let t2 = t * t;
            let t3 = t2 * t;
            out.push(
                (p1 * 2.0
                    + (p2 - p0) * t
                    + (p0 * 2.0 - p1 * 5.0 + p2 * 4.0 - p3) * t2
                    + (-p0 + p1 * 3.0 - p2 * 3.0 + p3) * t3)
                    * 0.5,
            );
        }
    }
    out.push(*ctrl.last().unwrap());
    out
}

/// Assemble the whole network as `(centreline, half_width)` pairs: wide ARTERIES — trunks (castle
/// gate → each major place), a ring linking adjacent biomes, landmark + capped camp spurs — plus a
/// space-filling layer of thin CAPILLARY trails branching off the arteries so nearly everywhere is
/// within a short walk of a path without the whole island reading as road.
fn build_curves() -> Vec<(Vec<Vec2>, f32)> {
    let gates = crate::castle::gate_centers();
    let biomes = biome_centres();
    let seed = 0x51ED_2A37u32;
    let mut curves: Vec<Vec<Vec2>> = Vec::new();

    // Trunks: each BIOME reached from whichever castle gate faces it. Cliffy mesas are
    // trunked to their castle-facing pass MOUTH, not the (summit) centre.
    for (i, t) in biomes.iter().enumerate() {
        let gate = *gates
            .iter()
            .min_by(|a, b| a.distance(*t).partial_cmp(&b.distance(*t)).unwrap())
            .unwrap();
        let target = crate::worldmap::biome_road_target(i);
        note_junction(target);
        curves.push(avoid_clearings(wander(gate, target, seed ^ (i as u32).wrapping_mul(0x9E37_79B9))));
    }

    // Coast reaches: from each FLAT biome's road target, one artery out toward the nearest shore
    // so the outer/coastal half of the biome isn't roadless (players noted parts of the island were
    // communicationally cut off). Cliffy mesas (snow, rock) are SKIPPED — reaching their coast would
    // paint straight up the shelf walls; their routes are the pass trails. Made ARTERIES so a river
    // between the biome and its coast gets bridged like any trunk crossing.
    for (i, _) in biomes.iter().enumerate() {
        if crate::worldmap::region_is_cliffy(i) {
            continue;
        }
        let target = crate::worldmap::biome_road_target(i);
        if let Some(coast) = crate::worldmap::coast_reach_world(target) {
            note_junction(coast);
            curves.push(avoid_clearings(wander(target, coast, seed ^ (0x00E0_0000 + i as u32))));
        }
    }

    // Ring: connect biome centres to their angular neighbours so you can circle the island.
    // Centres only SORT the ring; each segment's actual endpoints come from `biome_ring_node`,
    // which swaps a cliffy mesa for the pass mouth facing the segment's other end — the
    // desert↔rock leg threads the N slot canyon gate instead of climbing the mesa.
    let mut ring: Vec<(usize, Vec2)> = biomes.iter().copied().enumerate().collect();
    ring.sort_by(|a, b| a.1.y.atan2(a.1.x).partial_cmp(&b.1.y.atan2(b.1.x)).unwrap());
    for i in 0..ring.len() {
        let (ia, ca) = ring[i];
        let (ib, cb) = ring[(i + 1) % ring.len()];
        let a = crate::worldmap::biome_ring_node(ia, cb);
        let b = crate::worldmap::biome_ring_node(ib, ca);
        note_junction(a);
        note_junction(b);
        curves.push(avoid_clearings(wander(a, b, seed ^ (0x00B5_0000 + i as u32))));
    }

    // Fortress + rival keep: reached as a SPUR off the NEAREST existing road, NOT a separate gate
    // trunk. Both sit in/near the NE desert, so a full trunk from a gate to each ran nearly PARALLEL
    // to the desert biome trunk — the "double path" across the desert between our castle and the
    // rival keep. Branching each off the closest road gives one clean fork instead. (Fortress stops
    // at its GATE on the wall line, not its centre, so road_dirt doesn't bury the Blight courtyard.)
    {
        let net: Vec<Vec2> = curves.iter().flatten().copied().collect();
        for (k, t) in [crate::ork_fortress::GATE, crate::rival::RIVAL_CENTRE].into_iter().enumerate() {
            let near = *net
                .iter()
                .min_by(|a, b| a.distance(t).partial_cmp(&b.distance(t)).unwrap())
                .unwrap();
            note_junction(near);
            curves.push(avoid_clearings(wander(near, t, seed ^ (0x00A0_0000 + k as u32))));
        }
    }

    // Landmark spurs: every biome landmark gets its own path off the nearest network point. These
    // are always drawn (only 5) — a landmark is a destination worth a road. Sites are pre-chosen
    // from the terrain, so they're known here at bake time.
    let net: Vec<Vec2> = curves.iter().flatten().copied().collect();
    for (k, site) in crate::ruins::landmark_sites().iter().enumerate() {
        let near = *net
            .iter()
            .min_by(|a, b| a.distance(site.pos).partial_cmp(&b.distance(site.pos)).unwrap())
            .unwrap();
        note_junction(near);
        curves.push(avoid_clearings(wander(near, site.pos, seed ^ (0x00C0_0000 + k as u32))));
    }

    // Spurs: shortest connections from the network to nearby camps, capped to stay organic-not-busy.
    let net: Vec<Vec2> = curves.iter().flatten().copied().collect();
    let mut cand: Vec<(f32, Vec2, Vec2)> = crate::camps::cage_positions()
        .iter()
        .map(|(_, camp)| {
            let near = *net
                .iter()
                .min_by(|a, b| a.distance(*camp).partial_cmp(&b.distance(*camp)).unwrap())
                .unwrap();
            (near.distance(*camp), near, *camp)
        })
        .collect();
    cand.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap());
    for (k, (d, near, camp)) in cand.into_iter().enumerate() {
        if k >= SPUR_CAP || d > SPUR_MAX_LEN {
            break; // sorted ascending — once one is too far, the rest are too.
        }
        note_junction(near);
        // NB: no `avoid_clearings` here — the camp IS the destination; the spur is supposed to
        // end inside its clearing.
        curves.push(wander(near, camp, seed ^ (0x0077_0000 + k as u32)));
    }

    // Everything built so far is an ARTERY (wide). Now sprout the thin capillary layer off them,
    // then tag widths and return the combined typed list. Capillaries that blunder into a camp
    // clearing are dropped outright (they're space-fillers; no detour is worth it).
    let mut caps = sprout_capillaries(&curves, seed ^ 0x0CA9_F00D);
    caps.retain(|c| !polyline_hits_clearing(c));
    let mut out: Vec<(Vec<Vec2>, f32)> = curves.into_iter().map(|c| (c, HALF_W)).collect();
    out.extend(caps.into_iter().map(|c| (c, CAP_HALF_W)));
    // Pass trails LAST and deliberately outside the spur/capillary source net: the mesa ramp
    // corridors get a painted mid-width track (mouth → summit), but nothing may branch off one
    // sideways — a spur or capillary sprouting off a ramp would paint straight up a cliff face.
    out.extend(crate::worldmap::pass_trails_world().into_iter().map(|c| (c, 1.4)));
    out
}

/// Build the thin space-filling trails. Walk every artery and, at jittered arc-length intervals,
/// branch a short trail off into the land beside it (alternating sides). A fraction of those sprout
/// one shorter second-generation branch off their tip, pushing coverage into the pockets between
/// arteries — like a river delta. Endpoints are nudged back onto walkable land; degenerate (all-
/// water) branches are dropped. Capped at [`CAP_CAP`] for a bounded bake.
fn sprout_capillaries(arteries: &[Vec<Vec2>], seed: u32) -> Vec<Vec<Vec2>> {
    let mut rng = Rng(seed);
    let mut caps: Vec<Vec<Vec2>> = Vec::new();

    for (li, line) in arteries.iter().enumerate() {
        // March along the polyline by arc length, dropping a root every CAP_SPACING (jittered).
        let mut acc = 0.0;
        let mut next = CAP_SPACING * (0.4 + rng.next() * 0.6); // random phase per artery
        let mut ri = 0u32;
        for w in line.windows(2) {
            let seg = w[1] - w[0];
            let seglen = seg.length();
            if seglen < 1e-3 {
                continue;
            }
            let tan = seg / seglen;
            let perp = Vec2::new(-tan.y, tan.x);
            acc += seglen;
            while acc >= next {
                if caps.len() >= CAP_CAP {
                    return caps;
                }
                let pt = w[0].lerp(w[1], ((seglen - (acc - next)) / seglen).clamp(0.0, 1.0));
                // Alternate sides so trails fan out both ways; tilt mostly perpendicular with a
                // little forward/back lean so they don't all leave at a rigid right angle.
                let side = if ri & 1 == 0 { 1.0 } else { -1.0 };
                let tilt = (rng.next() - 0.5) * 1.1;
                let dir = perp * side + tan * tilt;
                let salt = (0xC0_0000 ^ (li as u32) << 8) ^ ri;
                if let Some((poly, _tip)) = cap_branch(&mut rng, pt, dir, seed, salt) {
                    caps.push(poly);
                }
                ri += 1;
                next += CAP_SPACING * (0.7 + rng.next() * 0.6);
            }
        }
    }
    caps
}

/// One short capillary branch from `pt` leaving along `dir`, length in [`CAP_LEN`]. Returns the
/// built centreline plus its tip (for a possible second generation), or `None` if the endpoint
/// nudged back onto the start (surrounded by water / off-map), which would be a degenerate stub.
fn cap_branch(rng: &mut Rng, pt: Vec2, dir: Vec2, seed: u32, salt: u32) -> Option<(Vec<Vec2>, Vec2)> {
    let len = CAP_LEN.0 + rng.next() * (CAP_LEN.1 - CAP_LEN.0);
    let end = nudge(pt + dir.normalize_or_zero() * len, pt);
    if end.distance(pt) < 5.0 {
        return None;
    }
    let poly = wander(pt, end, seed ^ salt.wrapping_mul(0x9E37_79B9));
    // Capillaries never get a bridge/boardwalk (only arteries are decked), so a trail must not
    // cross a river or a swamp bog pool — it would dead-end at the water or read as a path
    // plunging in. Drop any that touches water.
    if poly.iter().any(|p| is_river_world(p.x, p.y) || crate::worldmap::is_pool_world(p.x, p.y)) {
        return None;
    }
    Some((poly, end))
}

/// How close (world units) a through-road may come to an ork-camp CENTRE. Much wider than the
/// clearing itself (half-extent 3.6): the v1 detour only dodged the clearing box, which left
/// camps sitting right on the road margin ("dalej są obozy zbyt blisko ścieżek") — a war camp
/// should read as a place you *approach off the road*, via its own spur.
const CAMP_KEEP_OUT: f32 = 11.0;

/// Does any point of the polyline come within [`CAMP_KEEP_OUT`] of an ork-camp centre?
fn polyline_hits_clearing(c: &[Vec2]) -> bool {
    let camps: Vec<Vec2> = crate::camps::cage_positions().iter().map(|(_, c)| *c).collect();
    c.windows(2).any(|w| {
        let steps = (w[0].distance(w[1]) / 1.2).ceil().max(1.0) as usize;
        (0..=steps).any(|s| {
            let p = w[0].lerp(w[1], s as f32 / steps as f32);
            camps.iter().any(|cc| cc.distance(p) < CAMP_KEEP_OUT * 0.8)
        })
    })
}

/// Detour a through-route around ork camps — a trunk/ring road must never run through OR hug a
/// camp: for each stretch of the polyline inside a camp's [`CAMP_KEEP_OUT`] ring, insert a
/// waypoint pushed RADIALLY out of the ring (away from the camp centre — a perpendicular push
/// can slide along the ring's tangent and stay inside). A few fixpoint rounds handle a detour
/// that clips a second camp. (Camp SPURS skip this — the camp is their destination.) Safe to
/// call inside `build_curves`: `camps::plan` never queries roads.
fn avoid_clearings(mut c: Vec<Vec2>) -> Vec<Vec2> {
    let camps: Vec<Vec2> = crate::camps::cage_positions().iter().map(|(_, c)| *c).collect();
    for _ in 0..6 {
        let mut fix: Option<(usize, Vec2)> = None;
        'scan: for (i, w) in c.windows(2).enumerate() {
            let steps = (w[0].distance(w[1]) / 1.2).ceil().max(1.0) as usize;
            for s in 0..=steps {
                let p = w[0].lerp(w[1], s as f32 / steps as f32);
                let Some(camp) = camps.iter().find(|cc| cc.distance(p) < CAMP_KEEP_OUT) else {
                    continue;
                };
                let away = (p - *camp).normalize_or_zero();
                for reach in [2.5_f32, 6.0] {
                    let q = *camp + away * (CAMP_KEEP_OUT + reach);
                    if ground_at_world(q.x, q.y).is_some()
                        && camps.iter().all(|cc| cc.distance(q) >= CAMP_KEEP_OUT)
                    {
                        fix = Some((i + 1, q));
                        break 'scan;
                    }
                }
            }
        }
        match fix {
            Some((i, q)) => c.insert(i, q),
            None => break,
        }
    }
    c
}

/// Rasterise every curve into the strength grid (the one-time expensive step).
fn build_field() -> RoadField {
    let curves = network();
    let mut lo = Vec2::splat(f32::MAX);
    let mut hi = Vec2::splat(f32::MIN);
    for (c, _) in curves {
        for p in c {
            lo = lo.min(*p);
            hi = hi.max(*p);
        }
    }
    let pad = HALF_W + 3.0; // arteries are the widest brush — pad by the max half-width.
    lo -= pad;
    hi += pad;
    let w = (((hi.x - lo.x) / CELL).ceil() as usize) + 1;
    let h = (((hi.y - lo.y) / CELL).ceil() as usize) + 1;
    let mut f = RoadField { ox: lo.x, oz: lo.y, w, h, data: vec![0.0; w * h] };

    for (c, half) in curves {
        let core = EDGE * half;
        for win in c.windows(2) {
            let (p0, p1) = (win[0], win[1]);
            let steps = (p0.distance(p1) / (CELL * 0.7)).ceil().max(1.0) as usize;
            for s in 0..=steps {
                f.stamp(p0.lerp(p1, s as f32 / steps as f32), *half, core);
            }
        }
    }
    f
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn catmull_hits_endpoints() {
        let pts = catmull(&[Vec2::ZERO, Vec2::new(5.0, 2.0), Vec2::new(10.0, 0.0)]);
        assert!(pts.first().unwrap().distance(Vec2::ZERO) < 1e-3);
        assert!(pts.last().unwrap().distance(Vec2::new(10.0, 0.0)) < 1e-3);
        // The smoothed curve should be denser than the 3 control points.
        assert!(pts.len() > 3);
    }

    #[test]
    fn wander_is_deterministic() {
        let a = Vec2::new(-40.0, 0.0);
        let b = Vec2::new(40.0, 10.0);
        let p = wander(a, b, 12345);
        let q = wander(a, b, 12345);
        assert_eq!(p.len(), q.len());
        assert!(p.iter().zip(&q).all(|(x, y)| x.distance(*y) < 1e-6));
    }

    #[test]
    fn stamp_peaks_at_centre_and_decays() {
        let mut f = RoadField { ox: -5.0, oz: -5.0, w: 17, h: 17, data: vec![0.0; 17 * 17] };
        f.stamp(Vec2::ZERO, HALF_W, EDGE * HALF_W);
        let centre = f.sample(0.0, 0.0);
        let edge = f.sample(HALF_W * 0.9, 0.0);
        let off = f.sample(HALF_W + 2.0, 0.0);
        assert!(centre > 0.9, "centre {centre}");
        assert!(edge < centre && edge > 0.0, "edge {edge}");
        assert!(off.abs() < 1e-6, "off {off}");
    }

    /// The baked rut mask must groove BESIDE artery centrelines, not on them — and the
    /// world→UV region mapping the shader uses must land those texels. Samples the mask
    /// exactly the way `terrain.wgsl` does (uv → texel), at real artery points ±RUT_D.
    #[test]
    fn rut_mask_grooves_sit_beside_arteries() {
        let (data, w, h, region) = bake_rut_mask();
        assert_eq!(data.len(), w * h);
        let strong = data.iter().filter(|&&v| v > 200).count();
        assert!(strong > 1000, "expected long rut grooves, got {strong} strong texels");
        // Shader-equivalent lookup: uv = (wp - region.xy) * region.zw, texel = uv * dims.
        let sample = |wp: Vec2| -> u8 {
            let u = (wp.x - region.x) * region.z;
            let v = (wp.y - region.y) * region.w;
            if !(0.0..1.0).contains(&u) || !(0.0..1.0).contains(&v) {
                return 0;
            }
            let x = ((u * w as f32) as usize).min(w - 1);
            let z = ((v * h as f32) as usize).min(h - 1);
            data[z * w + x]
        };
        // Walk a long artery; at points along it the groove (±RUT_D perpendicular) must be
        // strong and the exact centreline weak.
        let arteries = artery_polylines();
        let poly = arteries.iter().max_by_key(|p| p.len()).expect("arteries exist");
        let (mut hits, mut probes) = (0, 0);
        for win in poly.windows(2).step_by(4) {
            let d = win[1] - win[0];
            if d.length() < 0.5 {
                continue;
            }
            let mid = (win[0] + win[1]) * 0.5;
            let perp = Vec2::new(-d.y, d.x).normalize();
            probes += 1;
            if sample(mid + perp * RUT_D).max(sample(mid - perp * RUT_D)) > 150 {
                hits += 1;
            }
        }
        assert!(probes > 5, "artery too short to probe");
        assert!(
            hits * 10 >= probes * 8,
            "grooves missing beside the artery: {hits}/{probes} probes hit (mapping broken?)"
        );
    }
}
