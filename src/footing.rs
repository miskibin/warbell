//! **Footing** — what is underfoot, and when a foot comes down. One place answers both so the
//! step SOUND (`audio::footsteps`), the kicked-up DUST / SPLASH (`footstep_fx`) and the boot PRINTS
//! (`footprints`) can never disagree about a footfall:
//!
//! - [`surface_at`] classifies a world point + foot height into a [`Surface`]: a bridge / boardwalk
//!   deck or any raised floor (wood, stone inside the castle), the shallows at the single water
//!   plane `worldmap::SEA_Y` (water), wet river / pool banks (mud), the castle cobbles once the walls
//!   stand (stone), roads + the trodden yard (dirt), then the biome tile (snow / sand / swamp mud /
//!   rock / grass), with beach sand split out of the grass tiles.
//! - [`hero_footfalls`] fires a [`Footfall`] on each gait half-cycle (`walk_phase` crossing kπ —
//!   the animator lands the boots there) plus a touchdown on landing; the planted foot is the lower
//!   of the rig's `FootL` / `FootR` joints, so prints alternate left / right on the real feet.
//! - [`npc_footfalls`] gives townsfolk, orks and the rival's men footfalls from distance walked
//!   (no per-rig phase to read), alternating feet about their travel line, near the hero only.
//!
//! Emission is sim-gated (`add_sim_systems`): a frozen world takes no steps.

use std::collections::HashMap;
use std::f32::consts::PI;

use bevy::prelude::*;

use crate::biome::Biome;
use crate::game_state::SimAppExt;
use crate::player::{Hero, HeroPart, Joint};
use crate::worldmap;

/// Ground surface under a foot — picks the step clip set, the dust tint and whether a print stays.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub enum Surface {
    Grass,
    Dirt,
    Mud,
    Water,
    Wood,
    Stone,
    Snow,
    Sand,
}

impl Surface {
    /// Ground that keeps a boot print: a deep relief in mud / snow / sand, a shallow scuff on packed
    /// earth, a bruised patch of crushed blades on the sward (stone, planks and water take none).
    pub fn takes_prints(self) -> bool {
        matches!(self, Surface::Mud | Surface::Snow | Surface::Sand | Surface::Dirt | Surface::Grass)
    }
}

/// Feet above the water plane by less than this are in the shallows (the eroded banks run the
/// walkable ground right down to the waterline).
const WADE_ABOVE_SEA: f32 = 0.16;
/// Bank ground this close to river / lake / pool water (world units) is wet soil → mud.
const WET_BANK: f32 = 1.3;
/// A foot this far above the terrain is standing on something built (deck, floor, wall walk).
const RAISED: f32 = 0.35;

/// Classify the footing at world XZ `p` for a foot at height `foot_y`. `paved` = the castle walls
/// stand (their cobble aprons + gate passages exist only then).
pub fn surface_at(p: Vec2, foot_y: f32, paved: bool) -> Surface {
    let (x, z) = (p.x, p.y);
    if let Some(deck) = crate::bridges::deck_y_at(x, z) {
        if (foot_y - deck).abs() < 0.45 {
            return Surface::Wood;
        }
    }
    let ground = worldmap::ground_at_world(x, z);
    match ground {
        // Raised above the terrain: a built floor (wall walk, house floor, stilt deck).
        Some(g) if foot_y > g + RAISED => {
            return if crate::castle::in_footprint(x, z) { Surface::Stone } else { Surface::Wood };
        }
        // No terrain under this boot: the wreck's deck, or a foot planted just past the drawn
        // shoreline (the hero's centre stands on the bank lip) — the water's edge.
        None => {
            if crate::wreck_cove::footing_over(x, z, None).is_some() {
                return Surface::Wood;
            }
            return if foot_y < worldmap::SEA_Y + 0.45 { Surface::Water } else { Surface::Mud };
        }
        _ => {}
    }
    if foot_y < worldmap::SEA_Y + WADE_ABOVE_SEA {
        return Surface::Water;
    }
    if paved && crate::castle::on_cobbles(x, z) {
        return Surface::Stone;
    }
    let biome = worldmap::biome_at_world(x, z);
    let wet_bank = worldmap::shore_sample(x, z).is_some_and(|(sd, _, _)| sd < WET_BANK);
    match biome {
        Some(Biome::Snow) => Surface::Snow,
        Some(Biome::Desert) => Surface::Sand,
        Some(Biome::Swamp) => Surface::Mud,
        _ if wet_bank => Surface::Mud,
        _ if crate::roads::on_road(x, z) || crate::castle::yard_strength(x, z) > 0.5 => Surface::Dirt,
        Some(Biome::Rocky) => Surface::Stone,
        None if worldmap::is_sand_world(x, z) => Surface::Sand,
        _ => Surface::Grass,
    }
}

/// Who took the step.
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Stepper {
    Hero,
    /// The hero's touchdown after a real drop; `fall` = height fallen (world units).
    HeroLanding { fall: f32 },
    /// A townsperson / ork / rival soldier. `audible` = close enough to the hero to be heard.
    Npc { audible: bool },
}

/// One foot coming down.
#[derive(Message, Clone, Copy, Debug)]
pub struct Footfall {
    /// Where the boot lands (world, on the walking surface).
    pub at: Vec3,
    /// Unit travel direction in world XZ (the print's toe points along it).
    pub dir: Vec2,
    /// +1 = right foot, -1 = left foot.
    pub side: f32,
    pub surface: Surface,
    /// 0 = walk … 1 = full sprint (from ground speed) — scales step loudness + dust.
    pub gait: f32,
    pub who: Stepper,
}

/// The hero's walk / sprint speeds (world u/s) — the gait 0..1 ramp between them.
const WALK_SPEED: f32 = 1.4;
const SPRINT_SPEED: f32 = crate::player::SPEED * crate::player::SPRINT_MULT;

pub fn gait_from_speed(speed: f32) -> f32 {
    ((speed - WALK_SPEED) / (SPRINT_SPEED - WALK_SPEED)).clamp(0.0, 1.0)
}

/// Right-hand perpendicular of a travel direction (Y-up, right-handed: facing +Z, right is -X).
fn right_of(dir: Vec2) -> Vec2 {
    Vec2::new(-dir.y, dir.x)
}

/// `FOREST_FOOTLOG=1`: log every hero footfall (surface, foot, position) — for checking the
/// surface probe and the left / right alternation from a capture run's log.
fn footlog() -> bool {
    static ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ON.get_or_init(|| std::env::var_os("FOREST_FOOTLOG").is_some())
}

/// A drop shorter than this lands clean — no landing footfall (matches the old dust threshold).
const LAND_MIN_FALL: f32 = 0.6;

fn hero_footfalls(
    mut last_half: Local<i64>,
    mut was_air: Local<bool>,
    mut alt_side: Local<f32>,
    def: Option<Res<crate::economy::Defenses>>,
    hero_q: Query<&Hero>,
    feet: Query<(&HeroPart, &GlobalTransform)>,
    mut out: MessageWriter<Footfall>,
) {
    let Ok(hero) = hero_q.single() else { return };
    let paved = def.is_some_and(|d| d.walls);
    let half = (hero.walk_phase / PI).floor() as i64;
    let landed = hero.on_ground && *was_air;
    *was_air = !hero.on_ground;

    let mut dir = hero.vel.normalize_or_zero();
    if dir == Vec2::ZERO {
        dir = Vec2::new(hero.facing.sin(), hero.facing.cos());
    }
    // The planted foot = the lower of the rig's two foot joints near the hero (cut-scene actors
    // and previews may also carry `HeroPart`s, hence the proximity gate).
    let planted = || {
        let mut best: Option<Vec3> = None;
        for (part, gt) in &feet {
            if !matches!(part.joint, Joint::FootL | Joint::FootR) {
                continue;
            }
            let p = gt.translation();
            if p.xz().distance(hero.pos) > 1.5 {
                continue;
            }
            if best.is_none_or(|b| p.y < b.y) {
                best = Some(p);
            }
        }
        best
    };
    let mut step = |who: Stepper, out: &mut MessageWriter<Footfall>| {
        let (foot, side) = match planted() {
            Some(f) => {
                let off = f.xz() - hero.pos;
                let s = if off.dot(right_of(dir)) >= 0.0 { 1.0 } else { -1.0 };
                // The joint sits at the ankle: keep its lateral offset, drop the fore/aft swing.
                let lat = off.dot(right_of(dir)).clamp(-0.2, 0.2);
                (hero.pos + right_of(dir) * lat, s)
            }
            None => {
                *alt_side = if *alt_side > 0.0 { -1.0 } else { 1.0 };
                (hero.pos + right_of(dir) * 0.1 * *alt_side, *alt_side)
            }
        };
        let surface = surface_at(foot, hero.y, paved);
        if footlog() {
            info!("footfall {who:?} {surface:?} side {side:+} at ({:.1}, {:.2}, {:.1})", foot.x, hero.y, foot.y);
        }
        out.write(Footfall {
            at: Vec3::new(foot.x, hero.y, foot.y),
            dir,
            side,
            surface,
            gait: gait_from_speed(hero.gait_speed.max(hero.vel.length())),
            who,
        });
    };

    if landed {
        let fall = hero.air_takeoff_y - hero.y;
        if fall > LAND_MIN_FALL {
            step(Stepper::HeroLanding { fall }, &mut out);
        }
        *last_half = half; // swallow the walking step the airborne phase advance would also fire
        return;
    }
    if hero.moving && hero.on_ground {
        if half != *last_half {
            *last_half = half;
            step(Stepper::Hero, &mut out);
        }
    } else {
        *last_half = half; // idle / airborne: resuming a walk must not fire a stale step
    }
}

/// NPCs further than this from the hero take no footfalls (nothing to see or hear).
const NPC_RANGE: f32 = 45.0;
/// …and only these close are heard.
const NPC_EARSHOT: f32 = 13.0;
/// Distance walked per footfall (one step, not one stride).
const NPC_STEP: f32 = 0.72;

#[derive(Default)]
struct NpcStride {
    last: Vec2,
    acc: f32,
    side: f32,
    seen: u32,
}

type Walkers = Or<(
    With<crate::villagers::Villager>,
    With<crate::orks::Ork>,
    With<crate::rival::RivalSoldier>,
    With<crate::rival::RivalWorker>,
    With<crate::rival::RivalRaider>,
)>;

fn npc_footfalls(
    mut strides: Local<HashMap<Entity, NpcStride>>,
    mut frame: Local<u32>,
    def: Option<Res<crate::economy::Defenses>>,
    hero_q: Query<&Hero>,
    q: Query<(Entity, &GlobalTransform), (Walkers, Without<crate::dying::Dying>, Without<Hero>)>,
    mut out: MessageWriter<Footfall>,
) {
    let Ok(hero) = hero_q.single() else { return };
    let paved = def.is_some_and(|d| d.walls);
    *frame = frame.wrapping_add(1);
    let f = *frame;
    for (e, gt) in &q {
        let p3 = gt.translation();
        let p = p3.xz();
        let d_hero = p.distance(hero.pos);
        if d_hero > NPC_RANGE {
            strides.remove(&e);
            continue;
        }
        let s = strides.entry(e).or_insert_with(|| NpcStride { last: p, acc: 0.0, side: 1.0, seen: f });
        s.seen = f;
        let moved = p.distance(s.last);
        let dir = (p - s.last).normalize_or_zero();
        s.last = p;
        // A teleport / respawn / idle shuffle-in-place: no steps.
        if moved > 2.0 || dir == Vec2::ZERO {
            continue;
        }
        s.acc += moved;
        if s.acc < NPC_STEP {
            continue;
        }
        s.acc -= NPC_STEP;
        s.side = -s.side;
        let foot = p + right_of(dir) * 0.1 * s.side;
        let surface = surface_at(foot, p3.y, paved);
        out.write(Footfall {
            at: Vec3::new(foot.x, p3.y, foot.y),
            dir,
            side: s.side,
            surface,
            gait: 0.0,
            who: Stepper::Npc { audible: d_hero < NPC_EARSHOT },
        });
    }
    // Forget walkers that despawned (or died) this frame.
    if f % 60 == 0 {
        strides.retain(|_, s| f.wrapping_sub(s.seen) < 60);
    }
}

/// Footfall emission runs in this set so every consumer can order itself `.after` it.
#[derive(SystemSet, Debug, Clone, PartialEq, Eq, Hash)]
pub struct FootfallSet;

pub struct FootingPlugin;

impl Plugin for FootingPlugin {
    fn build(&self, app: &mut App) {
        app.add_message::<Footfall>()
            .add_sim_systems((hero_footfalls, npc_footfalls).in_set(FootfallSet));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gait_ramp() {
        assert_eq!(gait_from_speed(1.0), 0.0);
        assert_eq!(gait_from_speed(WALK_SPEED), 0.0);
        assert!((gait_from_speed(SPRINT_SPEED) - 1.0).abs() < 1e-6);
    }

    #[test]
    fn right_hand_perpendicular() {
        // Facing +Z the right hand is at -X; facing -Z it is at +X.
        assert_eq!(right_of(Vec2::new(0.0, 1.0)), Vec2::new(-1.0, 0.0));
        assert_eq!(right_of(Vec2::new(0.0, -1.0)), Vec2::new(1.0, 0.0));
    }

    /// `FOOTING_PROBE="cx,cz,half,step" cargo test --release footing_probe -- --ignored --nocapture`
    /// prints a letter map of [`surface_at`] (feet on the terrain, walls up) round a point — for
    /// picking capture routes over a given ground. g grass · d dirt · m mud · w water · W wood ·
    /// s stone · S snow · a sand · ~ no footing.
    #[test]
    #[ignore]
    fn footing_probe() {
        let v: Vec<f32> = std::env::var("FOOTING_PROBE")
            .unwrap_or_else(|_| "0,148,20,2".into())
            .split(',')
            .filter_map(|c| c.trim().parse().ok())
            .collect();
        let (cx, cz, half, step) = (v[0], v[1], v[2], v[3]);
        let mut z = cz - half;
        while z <= cz + half {
            let mut row = format!("{z:7.1} ");
            let mut x = cx - half;
            while x <= cx + half {
                let foot = worldmap::ground_at_world(x, z).or_else(|| crate::bridges::deck_y_at(x, z));
                row.push(match foot.map(|y| surface_at(Vec2::new(x, z), y, true)) {
                    None => '~',
                    Some(Surface::Grass) => 'g',
                    Some(Surface::Dirt) => 'd',
                    Some(Surface::Mud) => 'm',
                    Some(Surface::Water) => 'w',
                    Some(Surface::Wood) => 'W',
                    Some(Surface::Stone) => 's',
                    Some(Surface::Snow) => 'S',
                    Some(Surface::Sand) => 'a',
                });
                x += step;
            }
            println!("{row}");
            z += step;
        }
        println!("x from {:.1} step {step}", cx - half);
        println!("bridges: {:?}", crate::bridges::centers());
    }

    #[test]
    fn prints_only_on_yielding_ground() {
        for s in [Surface::Mud, Surface::Snow, Surface::Sand, Surface::Dirt, Surface::Grass] {
            assert!(s.takes_prints());
        }
        for s in [Surface::Water, Surface::Wood, Surface::Stone] {
            assert!(!s.takes_prints());
        }
    }
}
