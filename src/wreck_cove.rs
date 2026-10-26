//! **Wreck Cove** — the bay on the forest's SW coast where the Crown ship that brought the hero
//! came to grief (campaign master spec §6.1; the bay itself is carved by `worldmap::cove_sd`). The
//! island's landing beach: the King's Road (`roads.rs`) runs from here to the fort.
//!
//! The wreck (`wreck_cove.glb`, `tools/blender_forest/wreck_cove.py`) lies broadside on the bay's
//! sand head, driven ashore by the storm: heeled a little onto her port bilge toward the beach,
//! starboard (the reef's breach) to the sea, mainmast over the port side, foremast standing. The
//! ship is a PLAY SPACE, not a backdrop (the prologue's opening is aboard her): her main deck is a
//! walkable surface ([`footing_over`], ORed into the hero's and the steering footing like a bridge
//! deck) reached by the survivors' gangway through the torn port bulwark; her hull, castles, masts
//! and hatch are solids. Around her: the ship's boat, cargo the sea threw up, and reef rocks
//! breaking the surface at the bay mouth. One discoverable (`landmarks::attach_story`).
//!
//! The ship's numbers below MIRROR `wreck_cove.py` (`SHIP`, `DECK_DROP`, `HEEL`, `PITCH`, the
//! printed `SHIP_SHIFT_Y` / `RAMP`) — change them together.

use std::f32::consts::TAU;
use std::sync::OnceLock;

use bevy::camera::visibility::VisibilityRange;
use bevy::prelude::*;

use crate::biome::BiomeEntity;

// ── the ship, mirrored from wreck_cove.py ──────────────────────────────────────────────
const SHIP_L: f32 = 16.5;
const SHIP_B: f32 = 5.6;
const SHIP_D: f32 = 3.0;
const SHEER: (f32, f32) = (0.95, 0.9);
const FULL: (f32, f32) = (2.0, 2.4);
const STERN_TOP: f32 = 9.0;
const TRANSOM: f32 = -0.93;
const DECK_DROP: f32 = 0.6;
const HEEL: f32 = 0.13;
const PITCH: f32 = 0.035;
const SHIFT_Y: f32 = -0.4109;
/// Gangway ends (model space): top on the deck, foot on the sand.
const RAMP_TOP: Vec3 = Vec3::new(-2.701, 1.663, -0.485);
const RAMP_FOOT: Vec3 = Vec3::new(-7.301, 0.020, -0.785);
const RAMP_HALF_W: f32 = 0.5;
/// The open main deck runs between the castles' front bulkheads (stations, −1 stern … +1 bow).
const DECK_T: (f32, f32) = (-0.55, 0.74);
const RAMP_T: f32 = -0.05;
/// Model-local x of the ship's midline offshore of the waterline, so the port bilge sits on the sand.
const SEAWARD: f32 = 2.4;

const LORE: &str = "A Crown carrack, driven broadside onto the sand by the storm: her back unbroken, her \
    starboard bilge stove in by the reef, her mainmast over the side. The gangway the survivors laid \
    from her torn bulwark is still there.";

fn ysheer(t: f32) -> f32 {
    SHIP_D + SHEER.0 * t.max(0.0).powi(2) + SHEER.1 * (-t).max(0.0).powi(2)
}

/// Upright half-breadth at station `t`, girth `u` (`wreck_cove.py::Hull.hb`).
fn hb(t: f32, u: f32) -> f32 {
    let p = if t > 0.0 { FULL.0 } else { FULL.1 + (STERN_TOP - FULL.1) * u * u };
    SHIP_B / 2.0 * (1.0 - t.abs().powf(p)).max(0.0).sqrt()
}

fn ship_rot() -> Quat {
    Quat::from_rotation_x(-PITCH) * Quat::from_rotation_z(HEEL)
}

/// Upright-frame → model space (heel, pitch, settle).
fn ship_xf(p: Vec3) -> Vec3 {
    ship_rot() * p + Vec3::new(0.0, SHIFT_Y, 0.0)
}

/// Model-space deck height over model `(mx, mz)`, or `None` off the open deck (the castles, the
/// hatch and the bulwarks aren't floor). Solves the heel/pitch back to the upright deck by a few
/// fixed-point steps (the angles are small).
fn deck_model_y(mx: f32, mz: f32) -> Option<f32> {
    let (mut ux, mut uz) = (mx, mz);
    let mut m = Vec3::ZERO;
    for _ in 0..4 {
        let t = uz / (SHIP_L / 2.0);
        m = ship_xf(Vec3::new(ux, ysheer(t) - DECK_DROP, uz));
        ux += mx - m.x;
        uz += mz - m.z;
    }
    let t = uz / (SHIP_L / 2.0);
    if !(DECK_T.0..DECK_T.1).contains(&t) {
        return None;
    }
    // Inside the bulwarks with a body's margin (the deck's real half-width at its height is a
    // touch under the sheer breadth).
    // Inside the bulwarks with a body's margin; at the gangway the torn bulwark lets you to the edge.
    let gap = if ux < 0.0 && (t - RAMP_T).abs() < 0.07 { 0.0 } else { 0.35 };
    if ux.abs() > hb(t, 0.92) * 0.9 - gap {
        return None;
    }
    Some(m.y)
}

/// Model-space gangway height over model `(mx, mz)`, or `None` off it.
fn ramp_model_y(mx: f32, mz: f32) -> Option<f32> {
    let a = Vec2::new(RAMP_TOP.x, RAMP_TOP.z);
    let b = Vec2::new(RAMP_FOOT.x, RAMP_FOOT.z);
    let d = b - a;
    let f = (Vec2::new(mx, mz) - a).dot(d) / d.length_squared();
    if !(-0.2..=1.0).contains(&f) {
        return None;
    }
    let off = (Vec2::new(mx, mz) - (a + d * f.clamp(0.0, 1.0))).length();
    (off <= RAMP_HALF_W).then(|| RAMP_TOP.y + (RAMP_FOOT.y - RAMP_TOP.y) * f.clamp(0.0, 1.0) + 0.09)
}

// ── the site ───────────────────────────────────────────────────────────────────────────
#[derive(Clone, Copy, Debug)]
pub struct Site {
    /// Model origin (world XZ) and height.
    pub pos: Vec2,
    pub y: f32,
    /// `Quat::from_rotation_y(yaw)`: local −X (port, the gangway) faces inland, +Z (bow) along the beach.
    pub yaw: f32,
    /// Where the King's Road comes down onto the beach.
    pub landing: Vec2,
}

impl Site {
    fn rot(&self) -> Quat {
        Quat::from_rotation_y(self.yaw)
    }
    /// Model space → world (model −X inland, +Z the bow along the beach, y up from the site's base).
    pub fn to_world(&self, m: Vec3) -> Vec3 {
        let r = self.rot() * m;
        Vec3::new(self.pos.x + r.x, self.y + r.y, self.pos.y + r.z)
    }
    fn to_model(&self, wx: f32, wz: f32) -> Vec2 {
        let r = self.rot().inverse() * Vec3::new(wx - self.pos.x, 0.0, wz - self.pos.y);
        Vec2::new(r.x, r.z)
    }
}

/// The wreck's site: found by walking the bay's inland axis from the basin centre to the waterline
/// — a pure function of the terrain, so the road network (built lazily, possibly before any build
/// phase) and the build phases agree without ordering. `None` in the arena / a map with no cove.
pub fn site() -> Option<Site> {
    static SITE: OnceLock<Option<Site>> = OnceLock::new();
    *SITE.get_or_init(|| {
        if crate::worldmap::is_arena() {
            return None;
        }
        let (c, inland) = crate::worldmap::wreck_cove_axis();
        let mut shore = None;
        let mut d = 0.0;
        while d < 60.0 {
            let p = c + inland * d;
            if crate::worldmap::ground_at_world(p.x, p.y).is_some() {
                shore = Some(p);
                break;
            }
            d += 0.5;
        }
        let shore = shore?;
        let yaw = inland.y.atan2(-inland.x);
        let rot = Quat::from_rotation_y(yaw);
        // Midline offshore by SEAWARD (local +X = seaward).
        let off = rot * Vec3::new(SEAWARD, 0.0, 0.0);
        let pos = shore + Vec2::new(off.x, off.z);
        let foot = rot * RAMP_FOOT;
        let fy = crate::worldmap::ground_at_world(pos.x + foot.x, pos.y + foot.z).unwrap_or(0.0);
        let y = fy - RAMP_FOOT.y - 0.06;
        let landing = shore + inland * 16.0;
        info!("wreck_cove: wreck at {:.1},{:.1} y {:.2} (shore {:.1},{:.1}), landing {:.1},{:.1}", pos.x, pos.y, y,
            shore.x, shore.y, landing.x, landing.y);
        Some(Site { pos, y, yaw, landing })
    })
}

/// Footing over the wreck: the deck or gangway where they stand above `ground` (the sand under the
/// hull), else `ground`. ORed into the hero's footing and the steering footing.
pub fn footing_over(wx: f32, wz: f32, ground: Option<f32>) -> Option<f32> {
    let Some(s) = site() else { return ground };
    if s.pos.distance_squared(Vec2::new(wx, wz)) > 14.0 * 14.0 {
        return ground;
    }
    let m = s.to_model(wx, wz);
    let walk = deck_model_y(m.x, m.y).or_else(|| ramp_model_y(m.x, m.y));
    match (walk, ground) {
        (Some(w), g) => {
            let wy = s.y + w;
            Some(match g {
                Some(g) if g > wy => g,
                _ => wy,
            })
        }
        (None, g) => g,
    }
}

/// The world point on the open main deck over model `(mx, mz)`, if that is deck (the P.1 cutscene
/// stands the crew and its camera on her).
pub fn deck_point(mx: f32, mz: f32) -> Option<Vec3> {
    let s = site()?;
    let y = deck_model_y(mx, mz)?;
    Some(s.to_world(Vec3::new(mx, y, mz)))
}

/// Every solid of the wreck as a world OBB `(centre, half-extents, yaw)`: the hull's outer
/// silhouette per station (a gap left at the gangway), both castles, both masts and the hatch.
fn solids(s: &Site) -> Vec<(Vec2, f32, f32, f32)> {
    let mut out = Vec::new();
    let mut push = |m0: Vec2, m1: Vec2, half_w: f32| {
        let a = s.to_world(Vec3::new(m0.x, 0.0, m0.y));
        let b = s.to_world(Vec3::new(m1.x, 0.0, m1.y));
        let (a, b) = (Vec2::new(a.x, a.z), Vec2::new(b.x, b.z));
        let d = b - a;
        let len = d.length();
        if len < 1e-3 {
            return;
        }
        // add_obb's yaw rotates local +X; the segment runs along local +Z (hd = half length).
        let yaw = d.x.atan2(d.y);
        out.push(((a + b) / 2.0, half_w, len / 2.0 + 0.05, yaw));
    };
    // Hull silhouette: per station, the model-space x extent over the girth (heel shifts it).
    let n = 22;
    let mut prev: Option<(f32, f32, f32)> = None;
    for k in 0..=n {
        let t = TRANSOM + (0.99 - TRANSOM) * k as f32 / n as f32;
        let (mut lo, mut hi) = (f32::MAX, f32::MIN);
        let mut z = 0.0;
        for j in 0..=10 {
            let u = j as f32 / 10.0;
            let th = u * std::f32::consts::FRAC_PI_2;
            for side in [-1.0f32, 1.0] {
                let x = side * hb(t, u) * th.sin().powf(0.75);
                let yk = 0.6 * t.max(0.0).powi(3) + 0.4 * (-t).max(0.0).powi(3);
                let y = yk + (ysheer(t) - yk) * (1.0 - th.cos());
                let m = ship_xf(Vec3::new(x, y, t * SHIP_L / 2.0));
                if m.y > -0.3 {
                    lo = lo.min(m.x);
                    hi = hi.max(m.x);
                }
                z = m.z;
            }
        }
        if let Some((plo, phi, pz)) = prev {
            push(Vec2::new(hi.max(phi) - 0.15, pz), Vec2::new(hi.max(phi) - 0.15, z), 0.3);
            let mid_t = (t + TRANSOM + (0.99 - TRANSOM) * (k - 1) as f32 / n as f32) / 2.0;
            if (mid_t - RAMP_T).abs() > 0.075 {
                push(Vec2::new(lo.min(plo) + 0.15, pz), Vec2::new(lo.min(plo) + 0.15, z), 0.3);
            }
        }
        prev = Some((lo, hi, z));
    }
    // Castles (floor-to-rail solid), the hatch, the mast stumps.
    let st = |t: f32| t * SHIP_L / 2.0;
    let castle = |t0: f32, t1: f32| {
        let w = hb(t0.max(t1.min(0.0)), 1.0).max(hb(t1, 1.0)) * 0.95;
        (Vec2::new(0.0, st(t0)), Vec2::new(0.0, st(t1)), w)
    };
    for (a, b, w) in [castle(TRANSOM, DECK_T.0 + 0.02), castle(DECK_T.1 - 0.02, 0.99)] {
        push(a, b, w);
    }
    push(Vec2::new(0.0, st(-0.16)), Vec2::new(0.0, st(0.0)), 0.85);
    push(Vec2::new(0.0, st(-0.03)), Vec2::new(0.0, st(0.03)), 0.35);
    push(Vec2::new(0.0, st(0.57)), Vec2::new(0.0, st(0.63)), 0.3);
    out
}

/// Build phase 4 (first of the planned pieces): reserve the beach so the scatter, roads' side
/// trails and the story layers keep clear, and make the hull solid.
pub fn plan() {
    let Some(s) = site() else { return };
    let mid = s.to_world(Vec3::new(-2.0, 0.0, 0.0));
    crate::blockers::reserve_visual(mid.x, mid.z, 14.0);
    for (c, hw, hd, yaw) in solids(&s) {
        crate::blockers::add_obb(c.x, c.y, hw, hd, yaw);
    }
}

/// Build phase 32: spawn the wreck, the ship's boat, the spilled cargo and the reef.
pub fn populate(commands: &mut Commands) {
    let Some(s) = site() else { return };
    let Some(kit) = crate::realforest::get().filter(|k| k.has("cove_wreck")) else {
        warn!("wreck_cove: wreck_cove.glb not in the kit — no wreck");
        return;
    };
    let range = VisibilityRange { start_margin: 0.0..0.0, end_margin: 260.0..300.0, use_aabb: true };
    let tf = Transform::from_xyz(s.pos.x, s.y, s.pos.y).with_rotation(s.rot());
    if let Some(e) = kit.spawn_static_with(commands, "cove_wreck", tf, range.clone()) {
        commands.entity(e).insert(BiomeEntity);
        crate::landmarks::attach_story(commands, e, "Wreck Cove", LORE);
    }
    // Beached pieces, model-local (−X inland): placed on the sand where it's land, else dropped.
    let small = VisibilityRange { start_margin: 0.0..0.0, end_margin: 110.0..130.0, use_aabb: true };
    for (name, m, yaw, solid) in [
        ("cove_boat", Vec3::new(-8.0, 0.0, 8.6), 0.5f32, Some((0.8, 2.3))),
        ("cove_spill_a", Vec3::new(-6.0, 0.0, -4.8), 1.1, None),
        ("cove_spill_b", Vec3::new(-6.8, 0.0, 5.8), -0.4, None),
    ] {
        let w = s.to_world(m);
        let Some(gy) = crate::worldmap::ground_at_world(w.x, w.z) else { continue };
        let rot = s.rot() * Quat::from_rotation_y(yaw);
        let tf = Transform::from_xyz(w.x, gy, w.z).with_rotation(rot);
        if let Some(e) = kit.spawn_static_with(commands, name, tf, small.clone()) {
            commands.entity(e).insert(BiomeEntity);
        }
        if let Some((hw, hd)) = solid {
            crate::blockers::add_obb(w.x, w.z, hw, hd, s.yaw + yaw);
        }
    }
    spawn_reef(commands, kit, &s);
}

/// Reef rocks breaking the surface at the bay mouth, off both horns: water a few units off land.
fn spawn_reef(commands: &mut Commands, kit: &crate::realforest::Kit, s: &Site) {
    const ROCKS: [&str; 6] = ["rock_01_b", "rock_01_d", "rock_01_f", "rock_02_a", "rock_02_c", "rock_02_e"];
    let (c, inland) = crate::worldmap::wreck_cove_axis();
    let range = VisibilityRange { start_margin: 0.0..0.0, end_margin: 220.0..260.0, use_aabb: true };
    let mut seed = 0x0C0F_E5EEu32;
    let mut rnd = || {
        seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        (seed >> 8) as f32 / 16_777_216.0
    };
    let land_near = |p: Vec2, r: f32| {
        (0..8).any(|k| {
            let a = k as f32 * TAU / 8.0;
            crate::worldmap::ground_at_world(p.x + a.cos() * r, p.y + a.sin() * r).is_some()
        })
    };
    let mut placed: Vec<Vec2> = Vec::new();
    for k in 0..240 {
        if placed.len() >= 16 {
            break;
        }
        let a = k as f32 * 0.61803 * TAU;
        let dir = Vec2::new(a.cos(), a.sin());
        // The seaward half of the bay only (the mouth and the horns' outer tips).
        if dir.dot(inland) > -0.15 {
            continue;
        }
        let p = c + dir * (18.0 + rnd() * 26.0);
        if crate::worldmap::ground_at_world(p.x, p.y).is_some()
            || !land_near(p, 7.0)
            || p.distance(s.pos) < 14.0
            || placed.iter().any(|q| q.distance(p) < 3.2)
        {
            continue;
        }
        let name = ROCKS[(rnd() * ROCKS.len() as f32) as usize % ROCKS.len()];
        let sc = 1.8 + rnd() * 2.2;
        let tf = Transform::from_xyz(p.x, crate::worldmap::SEA_Y - 0.35 * sc, p.y)
            .with_rotation(Quat::from_rotation_y(rnd() * TAU) * Quat::from_rotation_x((rnd() - 0.5) * 0.4))
            .with_scale(Vec3::splat(sc));
        if let Some(e) = kit.spawn_static_with(commands, name, tf, range.clone()) {
            commands.entity(e).insert(BiomeEntity);
        }
        placed.push(p);
    }
    info!("wreck_cove: {} reef rocks", placed.len());
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn deck_is_walkable_and_meets_the_gangway() {
        // The gangway's top sits on the deck within a step, its foot near the sand line.
        let top = deck_model_y(RAMP_TOP.x + 0.3, RAMP_TOP.z).expect("deck under the gangway top");
        let r = ramp_model_y(RAMP_TOP.x - 0.2, RAMP_TOP.z).unwrap();
        assert!((top - r).abs() < crate::steer::MAX_STEP, "deck {top} vs ramp {r}");
        let foot = ramp_model_y(RAMP_FOOT.x + 0.1, RAMP_FOOT.z).unwrap();
        assert!(foot < 0.4, "ramp foot {foot}");
        // The deck is a floor a man's height above the sand, gently heeled.
        let port = deck_model_y(-1.5, 2.0).unwrap();
        let stbd = deck_model_y(1.5, 2.0).unwrap();
        assert!(port > 1.0 && stbd > port, "port {port} stbd {stbd}");
        assert!((stbd - port) / 3.0 < 0.3, "deck too steep");
        // Off the deck: the castles and outboard.
        assert!(deck_model_y(0.0, -6.0).is_none());
        assert!(deck_model_y(0.0, 7.5).is_none());
        assert!(deck_model_y(3.5, 0.0).is_none());
    }

    #[test]
    fn walking_the_gangway_never_steps_more_than_max_step() {
        let mut prev: Option<f32> = None;
        for k in 0..=60 {
            let p = Vec3::new(RAMP_FOOT.x, 0.0, RAMP_FOOT.z).lerp(Vec3::new(RAMP_TOP.x + 1.2, 0.0, RAMP_TOP.z), k as f32 / 60.0);
            let y = deck_model_y(p.x, p.z).or_else(|| ramp_model_y(p.x, p.z)).expect("continuous footing");
            if let Some(q) = prev {
                assert!((y - q).abs() < 0.2, "jump {q} -> {y} at {p:?}");
            }
            prev = Some(y);
        }
    }
}

#[cfg(test)]
mod road_tests {
    #[test]
    fn kings_road_reaches_the_landing_beach() {
        let s = super::site().expect("the home island has a Wreck Cove");
        assert!(crate::roads::on_road(s.landing.x, s.landing.y), "no road at the landing {:?}", s.landing);
        // ...and the wreck's gangway foot is on dry land.
        let foot = s.to_world(super::RAMP_FOOT);
        assert!(crate::worldmap::ground_at_world(foot.x, foot.z).is_some(), "gangway foot in the sea at {foot:?}");
    }
}
