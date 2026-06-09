//! **Villagers** — the castle town's ambient population, ported from the TS `Villager.tsx`
//! mesh tree. Box-mesh humanoids that idle and stroll around the courtyard, with a few posted
//! at the keep + spilling out through the gates. The scene is a viewer, so there's no day
//! schedule, no guard combat and no upgrades — just lived-in townsfolk.
//!
//! Same ambient-biped pattern as `orks.rs`: a static **torso** plus articulated **parts**
//! (2 legs, 2 arms, a head) that swing via the shared sin trick; navigation is the shared
//! local steering (`steer.rs`). Meshes are merged, flat-shaded and vertex-coloured against one
//! white material so the whole town batches into few draw calls.
//!
//! Variants: peasants (varied skin/tunic, some in the conical hat) + a couple of armoured
//! guards (helmet + sword) by the keep. Placed inside `worldmap::build` after the castle, so
//! the castle's wall/keep/house blockers are already registered and townsfolk route around them.

use std::f32::consts::TAU;

use bevy::mesh::MeshBuilder;
use bevy::prelude::*;

use crate::biome::BiomeEntity;
use crate::critters::PartKind;
use crate::palette::lin;
use crate::steer;
use crate::worldmap;

/// Townsfolk turn at a relaxed rate. rad/s.
const VIL_MAX_TURN: f32 = 3.0;
const SCALE: f32 = 0.63; // the TS villager group scale, ×1.15 (townsfolk read a bit bigger now)
/// Child villagers — the same rig scaled right down, so the suburbs have kids underfoot.
const KID_SCALE: f32 = SCALE * 0.6;

// Palette (sRGB hex, from Villager.tsx).
const SKIN: [u32; 3] = [0xdca78a, 0xc08866, 0xa36b4a];
const TUNIC: [u32; 4] = [0x5a8fc8, 0x7a3a26, 0x4a6a3a, 0x8a6a3a];
const PANT: u32 = 0x3a2a18;
const HAT: u32 = 0xa02a26;
const HAIR: u32 = 0x3a2418;
const EYE: u32 = 0x141414;
const ARMOR: u32 = 0x9aa0aa;
const SWORD_BLADE: u32 = 0xd8dde6;
const SWORD_GUARD: u32 = 0xcaa23a;
/// Rich dyed robes that mark the wandering market traders apart from the drab peasants.
const MERCHANT_ROBE: [u32; 2] = [0x2f6f6a, 0x7a2f3a];
/// Dusky cloak of the wandering pilgrims who trek between the island's old landmarks.
const PILGRIM_ROBE: u32 = 0x6a5a8a;

// ── Components ───────────────────────────────────────────────────────────────────

#[derive(Clone, Copy, PartialEq)]
enum Mode {
    Idle,
    Walk,
}

#[derive(Component)]
pub struct Villager {
    home: Vec2,
    target: Vec2,
    pos: Vec2,
    facing: f32,
    speed: f32,
    wander_r: f32,
    gait: f32,
    swing: f32,
    bob: f32,
    body_r: f32,
    phase: f32,
    moving: bool,
    mode: Mode,
    timer: f32,
    rng: u32,
    /// True while walking to a gathering spot (so the brain lingers longer on arrival).
    gathering: bool,
}

#[derive(Component)]
struct VilPart {
    kind: PartKind,
}

/// A child villager — wanders fast in short bursts around a small play patch (and skips the
/// adults' gathering/chore behaviour). Otherwise a normal villager (same rig, smaller scale),
/// so the night curfew and [`villager_brain`] handle it for free.
#[derive(Component)]
pub(crate) struct Kid;

/// Town "gathering spots" — the well, woodpile, market, keep steps. Idle adults occasionally
/// drift to one and linger, so the suburbs cluster into little knots instead of all wandering
/// solo. Filled by [`populate`]; empty before then (init'd so [`villager_brain`] never misses it).
#[derive(Resource, Default)]
struct TownSpots(Vec<Vec2>);

/// A wandering **pilgrim** — a villager whose brain ([`pilgrim_brain`]) walks it between the
/// island's landmarks (and back to town) instead of milling the courtyard. Hail it with **F**
/// ([`pilgrim_hint`]) for a nudge toward the nearest landmark you've yet to find.
#[derive(Component)]
pub struct Pilgrim {
    /// Current destination (a landmark or a town point), world XZ.
    target: Vec2,
    /// Dwell timer at a destination before choosing the next.
    pause: f32,
    /// Throttle so repeated F-mashing doesn't spam the same hint.
    hint_cd: f32,
    rng: u32,
}

/// A castle town-guard: a villager that fights invaders during a wave (chase → strike, trading
/// blows), goes **down** at 0 HP (lies still), and is revived + walks back to its post at dawn.
#[derive(Component)]
pub struct Guard {
    hp: f32,
    max: f32,
    atk_cd: f32,
    pub downed: bool,
    post: Vec2,
}

impl Guard {
    /// Down guards aren't worth an invader's attention (read by the invader AI's targeting).
    pub fn is_downed(&self) -> bool {
        self.downed
    }
}

/// Marks the **town's working-and-fighting population** (the labour/militia pool), as opposed to
/// the purely-ambient set-dressing NPCs (gate-folk, market traders, pilgrims, kids). A townsperson
/// is, at any instant, either a [`Guard`] (idle reserve standing post / fighting at night) **or** a
/// [`crate::town::Worker`] (staffing a producer by day) — never both. The town auto-assign swaps
/// `Guard → Worker` to employ one by day; [`muster_townsfolk`] strips `Worker` at dusk and
/// [`rearm_townsfolk`] re-arms any pool member that has neither role, so the whole town defends at
/// night and goes back to work at dawn. This is what makes "grow population via farms" matter:
/// more townsfolk = more day-workforce AND more night-defenders.
#[derive(Component)]
pub struct Townsfolk;

/// (Re)arm a town pool member `e` as a [`Guard`] posted at `post` — the bundle the spawn helper and
/// [`rearm_townsfolk`] use so the militia construction lives in one place (Guard's fields are
/// module-private). `try_insert` per the despawn-race convention.
fn arm_as_guard(commands: &mut Commands, e: Entity, post: Vec2) {
    commands.entity(e).try_insert((
        Guard { hp: GUARD_MAX_HP, max: GUARD_MAX_HP, atk_cd: 0.0, downed: false, post },
        crate::navgrid::NavPath::default(),
    ));
}

// Guard combat tuning. Guards now take damage ONLY from invaders that actually strike them
// (via [`GuardDamage`]) — no more self-inflicted melt — so they're beefier + hit harder and a
// pair can win a 1v1 but a wave still overwhelms them.
const GUARD_MAX_HP: f32 = 65.0;
const GUARD_DAMAGE: f32 = 9.0;
const GUARD_DEFEND_RADIUS: f32 = 12.0;
const GUARD_MELEE: f32 = 1.6;
const GUARD_SPEED: f32 = 2.4;
const GUARD_ATTACK_CD: f32 = 1.0;
/// A guard's strike only emits a clash SFX when this near the hero — a small earshot so distant
/// skirmishes across the field stay silent and only the fight beside you is heard.
const GUARD_SFX_RADIUS: f32 = 14.0;
/// Past this distance from its post a guard (a freed captive marching in from a razed camp) routes
/// home by A* through the gates; nearer than this a revived town-guard just steers in directly.
const GUARD_PATH_RANGE: f32 = 4.0;

/// Damage invaders have dealt town-guards this frame (`(guard entity, amount)`), pushed by the
/// invader AI in `siege.rs` and drained into guard HP by [`guard_combat`]. The mirror of the
/// hero's [`crate::player::PendingHeroDamage`] — combat stays store-mediated, no collision events.
#[derive(Resource, Default)]
pub struct GuardDamage(pub Vec<(Entity, f32)>);

/// Invader melee is blunted this much against an armoured guard (vs the hero) — guards soak.
pub const GUARD_ARMOR_MULT: f32 = 0.6;

// ── Plugin + systems ───────────────────────────────────────────────────────────────

pub struct VillagersPlugin;

/// Per planned camp: whether its captives were freed (`done`) and whether it was ever seen
/// populated by a living warband (`seen` — guards against auto-freeing before camps spawn).
/// `pub(crate)` so the save system snapshots/restores `done` (a world flag).
#[derive(Resource, Default)]
pub(crate) struct RescuedCamps {
    pub(crate) done: Vec<bool>,
    pub(crate) seen: Vec<bool>,
}

const CAMP_HOME_R: f32 = 6.0;

impl Plugin for VillagersPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<RescuedCamps>()
            .init_resource::<GuardDamage>()
            .init_resource::<TownSpots>()
            .add_systems(Update, villager_limbs) // limb anim keeps running while frozen
            // Ungated so they fire on the day↔night edge even if the world is frozen (panel open):
            .add_systems(Update, (townsfolk_curfew, muster_townsfolk))
            .add_systems(OnExit(crate::game_state::AppState::StartScreen), reset_rescues)
            .add_systems(OnExit(crate::game_state::AppState::GameOver), reset_rescues)
            .add_systems(
                Update,
                (
                    villager_brain,
                    worker_steer,
                    pilgrim_brain,
                    pilgrim_hint,
                    guard_combat,
                    rearm_townsfolk,
                    camp_rescue,
                    recruit,
                )
                    .run_if(in_state(crate::game_state::Modal::None)),
            );
    }
}

/// Night curfew: while a wave is on, the pure **non-combatant** ambient NPCs — gate-folk, market
/// traders, courtyard peasants, pilgrims, kids — clear off the streets and reappear at dawn. The
/// `Townsfolk` pool is exempt (`Without<Townsfolk>`): they muster and fight instead of fleeing.
/// Only the root visibility flips, on the phase edge; their wander brains idle on, invisibly, until
/// morning. Ungated so it also holds while the world is frozen (paused / a panel open) mid-wave.
fn townsfolk_curfew(
    siege: Option<Res<crate::siege::Siege>>,
    mut last: Local<Option<bool>>,
    mut q: Query<&mut Visibility, (With<Villager>, Without<Guard>, Without<Townsfolk>)>,
) {
    let wave = siege.is_some_and(|s| s.phase == crate::siege::GamePhase::Wave);
    if *last == Some(wave) {
        return; // only touch visibility on the day↔night edge
    }
    *last = Some(wave);
    let vis = if wave { Visibility::Hidden } else { Visibility::Visible };
    for mut v in &mut q {
        *v = vis;
    }
}

/// Dusk muster: when the wave begins, every employed townsperson downs tools and takes up arms —
/// strip its [`crate::town::Worker`] (its plot goes unstaffed → production halts) so
/// [`rearm_townsfolk`] re-arms it as a [`Guard`] next frame and it joins the wall. At dawn the town
/// auto-assign re-employs the idle reserve. Edge-triggered on the day↔night flip, like the curfew.
fn muster_townsfolk(
    siege: Option<Res<crate::siege::Siege>>,
    mut last: Local<Option<bool>>,
    mut commands: Commands,
    workers: Query<Entity, With<crate::town::Worker>>,
) {
    let wave = siege.is_some_and(|s| s.phase == crate::siege::GamePhase::Wave);
    if *last == Some(wave) {
        return;
    }
    *last = Some(wave);
    if wave {
        for e in &workers {
            commands.entity(e).try_remove::<crate::town::Worker>();
        }
    }
}

/// Keep every idle pool member armed: any `Townsfolk` with neither a [`Guard`] role nor a
/// [`crate::town::Worker`] job (a fresh recruit, a just-mustered worker, or one whose plot collapsed)
/// is (re)posted as a guard where it stands. The standing reserve thus always defends by day and
/// fights by night; employment is the only thing that pulls one off guard duty.
fn rearm_townsfolk(
    mut commands: Commands,
    idle: Query<(Entity, &Villager), (With<Townsfolk>, Without<Guard>, Without<crate::town::Worker>)>,
) {
    for (e, v) in &idle {
        arm_as_guard(&mut commands, e, v.pos);
    }
}

fn reset_rescues(mut r: ResMut<RescuedCamps>) {
    for b in &mut r.done {
        *b = false;
    }
    for b in &mut r.seen {
        *b = false;
    }
}

/// Spawn a fresh town **guard** at an open courtyard spot — the body the District/rescue/recruit
/// systems add to the castle's defenders (and the bloodline).
pub fn spawn_courtyard_guard(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
    seed: u32,
) {
    let mat = materials.add(StandardMaterial { base_color: Color::WHITE, perceptual_roughness: 0.85, ..default() });
    let mut rng = seed | 1;
    let half = crate::castle::courtyard_half();
    let home = courtyard_spot(&mut rng, half, &[]).unwrap_or(Vec2::new(0.0, 5.0));
    let kind = Kind::Guard { skin: SKIN[(seed as usize) % SKIN.len()], tunic: TUNIC[1] };
    spawn(commands, meshes, &mat, kind, home, home, 1.6, 1.4, SCALE, next_u32(&mut rng));
}

/// Spawn a freed captive as a **settler/militia** at `from` (the camp cage), homed to a courtyard
/// post so it marches across to the keep and defends it at night — the visible "prisoner walks
/// out and heads home" the rescue should read as.
pub fn spawn_settler(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
    from: Vec2,
    seed: u32,
) {
    let mat = materials.add(StandardMaterial { base_color: Color::WHITE, perceptual_roughness: 0.85, ..default() });
    let mut rng = seed | 1;
    let half = crate::castle::courtyard_half();
    let post = courtyard_spot(&mut rng, half, &[]).unwrap_or(Vec2::new(0.0, 5.0));
    let kind = Kind::Guard { skin: SKIN[(seed as usize) % SKIN.len()], tunic: TUNIC[1] };
    // home = post (the courtyard) → the Guard's post is the castle; pos = from (the cage) → it
    // spawns at the camp and `guard_combat` walks it back to its post.
    spawn(commands, meshes, &mat, kind, post, from, 1.6, 1.4, SCALE, next_u32(&mut rng));
}

/// Clear a camp's warband and its captives are **automatically** freed (the TS behaviour): one
/// joins the castle as militia (a new guard) and grows the bloodline, with a float over the cage
/// so you see it happen. `seen` gates against freeing a camp before its orks have even spawned.
#[allow(clippy::too_many_arguments)]
fn camp_rescue(
    mut lives: ResMut<crate::succession::Lives>,
    mut rescued: ResMut<RescuedCamps>,
    orks: Query<&crate::orks::Ork, Without<crate::orks::WaveInvader>>,
    cages_q: Query<(Entity, &crate::camps::Cage, &Transform)>,
    mut floats: ResMut<crate::combat_fx::FloatQueue>,
    mut cues: MessageWriter<crate::audio::AudioCue>,
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    let cages = crate::camps::cage_positions();
    if rescued.done.len() != cages.len() {
        rescued.done = vec![false; cages.len()];
        rescued.seen = vec![false; cages.len()];
    }
    for (i, (cage, centre)) in cages.iter().enumerate() {
        if rescued.done[i] {
            continue;
        }
        if orks.iter().any(|o| o.home().distance(*centre) < CAMP_HOME_R) {
            rescued.seen[i] = true; // warband still alive — this camp IS populated
            continue;
        }
        if !rescued.seen[i] {
            continue; // never seen populated (camps not spawned yet) — don't auto-free
        }
        // Warband wiped → free the captives.
        rescued.done[i] = true;
        let y = crate::worldmap::ground_at_world(cage.x, cage.y).unwrap_or(0.0);
        // Open the cage IN PLACE: swap the closed prop for the opened husk at the same pose.
        let mut cage_tf = Transform::from_xyz(cage.x, y, cage.y);
        for (e, c, tf) in &cages_q {
            if c.camp == i {
                cage_tf = *tf;
                commands.entity(e).try_despawn();
            }
        }
        crate::camps::open_cage(&mut commands, &mut meshes, &mut materials, cage_tf);
        // The captive walks out as a settler/militia, marching from the cage to the castle.
        spawn_settler(&mut commands, &mut meshes, &mut materials, *cage, 0x5e5c_0000u32.wrapping_add(i as u32 * 97));
        lives.heirs += 1;
        floats.0.push(crate::combat_fx::FloatReq {
            world: Vec3::new(cage.x, y + 1.8, cage.y),
            text: "Captive freed!  +1 settler".into(),
            color: Color::srgb(0.5, 1.0, 0.6),
            scale: 1.2,
        });
        cues.write(crate::audio::AudioCue::CampRescue);
        cues.write(crate::audio::AudioCue::HeroEvent(crate::audio::HeroEvent::FirstRescue));
    }
}

/// **R** inside the castle spends a Mercenary Contract (from chests) to hire a sellsword — a new
/// guard + an heir.
fn recruit(
    keys: Res<ButtonInput<KeyCode>>,
    hero: Res<crate::player::HeroState>,
    mut inv: ResMut<crate::inventory::Inventory>,
    mut lives: ResMut<crate::succession::Lives>,
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    if keys.just_pressed(KeyCode::KeyR)
        && hero.alive
        && crate::castle::in_footprint(hero.pos.x, hero.pos.y)
        && inv.0.consume_item("mercenary_contract", 1)
    {
        lives.heirs += 1;
        spawn_courtyard_guard(&mut commands, &mut meshes, &mut materials, 0x4ec5_0000);
    }
}

/// Ambient townsfolk wander — guards (their AI is [`guard_combat`]) and pilgrims (their AI is
/// [`pilgrim_brain`]) are excluded.
fn villager_brain(
    time: Res<Time>,
    spots: Res<TownSpots>,
    mut q: Query<(&mut Villager, &mut Transform, Has<Kid>), (Without<Guard>, Without<Pilgrim>, Without<crate::town::Worker>)>,
) {
    let dt = time.delta_secs().min(0.05);
    let tw = time.elapsed_secs_wrapped();

    for (mut v, mut tf, is_kid) in &mut q {
        v.timer -= dt;
        match v.mode {
            Mode::Idle => {
                v.moving = false;
                if v.timer <= 0.0 {
                    pick_walk(&mut v, &spots.0, is_kid);
                }
            }
            Mode::Walk => {
                let dist = (v.target - v.pos).length();
                if dist < 0.3 || v.timer <= 0.0 {
                    v.mode = Mode::Idle;
                    // Linger at a gathering spot (a chat or a chore); else just a short pause.
                    v.timer = if v.gathering {
                        rng_range(&mut v.rng, 6.0, 14.0)
                    } else {
                        rng_range(&mut v.rng, 1.5, 4.5)
                    };
                    v.moving = false;
                } else {
                    let cur_y = worldmap::ground_at_world(v.pos.x, v.pos.y).unwrap_or(tf.translation.y);
                    match steer::advance(v.pos, v.facing, v.target, v.speed * dt, v.body_r, cur_y, VIL_MAX_TURN * dt) {
                        Some(s) => {
                            v.facing = s.facing;
                            v.pos = s.pos;
                            v.moving = s.moving;
                        }
                        None => {
                            v.mode = Mode::Idle;
                            v.timer = rng_range(&mut v.rng, 0.4, 1.0);
                            v.moving = false;
                        }
                    }
                }
            }
        }

        let gy = worldmap::ground_at_world(v.pos.x, v.pos.y).unwrap_or(tf.translation.y);
        let bob = if v.moving { (tw * v.gait + v.phase).sin().abs() * v.bob } else { 0.0 };
        tf.translation = Vec3::new(v.pos.x, gy + bob, v.pos.y);
        tf.rotation = Quat::from_rotation_y(v.facing);
    }
}

/// Steer assigned workers to their building, then hold post (sets `at_post`).
/// Lives here because it pokes the private `Villager` fields. Workers inherit
/// `townsfolk_curfew` (no `Guard`), so they flee at night automatically.
fn worker_steer(
    time: Res<Time>,
    spots: Res<crate::town::PlotSpots>,
    mut q: Query<(&mut crate::town::Worker, &mut Villager, &mut Transform)>,
) {
    let dt = time.delta_secs().min(0.05);
    let tw = time.elapsed_secs_wrapped();
    for (mut worker, mut v, mut tf) in &mut q {
        let Some(post) = spots.0.get(worker.idx).copied() else { continue };
        let to = post - v.pos;
        let dist = to.length();
        if dist < 1.6 {
            worker.at_post = true;
            v.moving = false;
            // Turn to face the building/field so the hoeing reads (villager_limbs swings
            // the arms once `at_post`). `to` points from the worker to the plot centre.
            if to.length_squared() > 1e-4 {
                v.facing = to.x.atan2(to.y);
            }
        } else {
            worker.at_post = false;
            v.target = post;
            let cur_y = worldmap::ground_at_world(v.pos.x, v.pos.y).unwrap_or(tf.translation.y);
            if let Some(s) = steer::advance(v.pos, v.facing, v.target, v.speed * dt, v.body_r, cur_y, VIL_MAX_TURN * dt) {
                v.facing = s.facing;
                v.pos = s.pos;
                v.moving = s.moving;
            }
        }
        let gy = worldmap::ground_at_world(v.pos.x, v.pos.y).unwrap_or(tf.translation.y);
        let bob = if v.moving { (tw * v.gait + v.phase).sin().abs() * v.bob } else { 0.0 };
        tf.translation = Vec3::new(v.pos.x, gy + bob, v.pos.y);
        tf.rotation = Quat::from_rotation_y(v.facing);
    }
}

/// Pilgrim AI: trek between the island's landmarks (and back to town now and then), dwelling a
/// few seconds at each. Drives the same [`Villager`] pose fields as [`villager_brain`] but steers
/// toward a real destination instead of wandering a home radius. Excluded from `villager_brain`.
#[allow(clippy::type_complexity)]
fn pilgrim_brain(
    time: Res<Time>,
    mut q: Query<
        (&mut Pilgrim, &mut Villager, &mut Transform),
        (Without<Guard>, Without<crate::landmarks::Landmark>, Without<crate::town::Worker>),
    >,
    marks: Query<&Transform, With<crate::landmarks::Landmark>>,
) {
    let dt = time.delta_secs().min(0.05);
    let tw = time.elapsed_secs_wrapped();
    // Candidate destinations: every landmark, plus a town point so they circle back home.
    let mut dests: Vec<Vec2> =
        marks.iter().map(|t| Vec2::new(t.translation.x, t.translation.z)).collect();
    dests.push(Vec2::new(0.0, 9.0));

    for (mut pil, mut v, mut tf) in &mut q {
        if pil.hint_cd > 0.0 {
            pil.hint_cd -= dt;
        }
        if pil.pause > 0.0 {
            pil.pause -= dt;
            v.moving = false;
        } else if (pil.target - v.pos).length() < 1.2 {
            // Arrived — dwell, then pick the next destination.
            pil.pause = rng_range(&mut pil.rng, 4.0, 9.0);
            pil.target = dests[(next_u32(&mut pil.rng) as usize) % dests.len()];
            v.moving = false;
        } else {
            let cur_y = worldmap::ground_at_world(v.pos.x, v.pos.y).unwrap_or(tf.translation.y);
            match steer::advance(v.pos, v.facing, pil.target, v.speed * dt, v.body_r, cur_y, VIL_MAX_TURN * dt) {
                Some(s) => {
                    v.facing = s.facing;
                    v.pos = s.pos;
                    v.moving = s.moving;
                }
                None => {
                    // Wedged — dwell briefly, then strike out for a different landmark.
                    pil.pause = rng_range(&mut pil.rng, 0.5, 1.5);
                    pil.target = dests[(next_u32(&mut pil.rng) as usize) % dests.len()];
                    v.moving = false;
                }
            }
        }

        let gy = worldmap::ground_at_world(v.pos.x, v.pos.y).unwrap_or(tf.translation.y);
        let bob = if v.moving { (tw * v.gait + v.phase).sin().abs() * v.bob } else { 0.0 };
        tf.translation = Vec3::new(v.pos.x, gy + bob, v.pos.y);
        tf.rotation = Quat::from_rotation_y(v.facing);
    }
}

/// **F** beside a pilgrim → a spoken nudge toward the nearest landmark you've yet to find (with a
/// compass direction + a few coins for the road); once all are found, a parting line instead.
#[allow(clippy::too_many_arguments)]
fn pilgrim_hint(
    keys: Res<ButtonInput<KeyCode>>,
    hero: Res<crate::player::HeroState>,
    mut player: ResMut<crate::player::PlayerRes>,
    mut floats: ResMut<crate::combat_fx::FloatQueue>,
    mut cues: MessageWriter<crate::audio::AudioCue>,
    mut pilgrims: Query<(&mut Pilgrim, &Transform)>,
    marks: Query<(&Transform, &crate::landmarks::Landmark)>,
) {
    if !keys.just_pressed(KeyCode::KeyF) || !hero.alive {
        return;
    }
    for (mut pil, ptf) in &mut pilgrims {
        let pp = ptf.translation;
        if Vec2::new(pp.x, pp.z).distance(hero.pos) > 2.5 || pil.hint_cd > 0.0 {
            continue;
        }
        pil.hint_cd = 8.0;
        let head = Vec3::new(pp.x, pp.y + 2.2, pp.z);
        // Nearest landmark the hero hasn't discovered yet.
        let mut best: Option<(f32, Vec2, &'static str)> = None;
        for (t, lm) in &marks {
            if lm.is_discovered() {
                continue;
            }
            let lp = Vec2::new(t.translation.x, t.translation.z);
            let d = lp.distance(hero.pos);
            if best.is_none_or(|(bd, _, _)| d < bd) {
                best = Some((d, lp, lm.name));
            }
        }
        if let Some((_, lp, name)) = best {
            let dir = compass(hero.pos, lp);
            floats.0.push(crate::combat_fx::FloatReq {
                world: head,
                text: format!("\"{name} lies to the {dir}.\""),
                color: Color::srgb(0.9, 0.95, 0.7),
                scale: 1.0,
            });
            player.0.add_gold(5);
            cues.write(crate::audio::AudioCue::Forage);
        } else {
            floats.0.push(crate::combat_fx::FloatReq {
                world: head,
                text: "\"You've seen all the old places, wanderer.\"".into(),
                color: Color::srgb(0.9, 0.95, 0.7),
                scale: 1.0,
            });
            cues.write(crate::audio::AudioCue::UiSelect);
        }
        return; // one hail per press
    }
}

/// 8-wind compass word from `from` toward `to` (world XZ; −Z is north, +X is east).
fn compass(from: Vec2, to: Vec2) -> &'static str {
    let d = to - from;
    if d.length_squared() < 1e-3 {
        return "here";
    }
    let a = d.x.atan2(-d.y);
    let mut i = (a / std::f32::consts::FRAC_PI_4).round() as i32 % 8;
    if i < 0 {
        i += 8;
    }
    ["north", "northeast", "east", "southeast", "south", "southwest", "west", "northwest"][i as usize]
}

/// Town-guard AI: during a wave, chase the nearest invader inside the defend radius and trade
/// blows in melee (the guard's strikes wound the invader's `Health`, the invader's wound the
/// guard); a downed guard lies still. In peacetime guards heal up + amble back to their post.
#[allow(clippy::type_complexity)]
fn guard_combat(
    time: Res<Time>,
    siege: Res<crate::siege::Siege>,
    mut incoming: ResMut<GuardDamage>,
    mut commands: Commands,
    mut cues: MessageWriter<crate::audio::AudioCue>,
    hero: Query<&crate::player::Hero>,
    mut guards: Query<
        (Entity, &mut Guard, &mut Villager, &mut Transform, &mut crate::navgrid::NavPath),
        Without<crate::orks::WaveInvader>,
    >,
    mut invaders: Query<
        (Entity, &Transform, &mut crate::player::Health),
        (With<crate::orks::WaveInvader>, Without<Guard>, Without<crate::dying::Dying>),
    >,
) {
    let dt = time.delta_secs().min(0.05);
    let tw = time.elapsed_secs_wrapped();
    let now = time.elapsed_secs();
    let in_wave = siege.phase == crate::siege::GamePhase::Wave;
    // Hero (≈ listener) position, for the small-earshot gate on guard clash SFX.
    let hero_pos = hero.single().ok().map(|h| h.pos);
    let inv: Vec<(Entity, Vec2)> =
        invaders.iter().map(|(e, tf, _)| (e, Vec2::new(tf.translation.x, tf.translation.z))).collect();
    let mut dealt: Vec<(Entity, f32)> = Vec::new();

    // Sum the invader strikes landed on each guard this frame, then drain the channel.
    let mut hurt: std::collections::HashMap<Entity, f32> = std::collections::HashMap::new();
    for (e, dmg) in incoming.0.drain(..) {
        *hurt.entry(e).or_insert(0.0) += dmg;
    }

    for (self_e, mut g, mut v, mut tf, mut path) in &mut guards {
        // Take any strikes invaders landed on this guard this frame.
        if let Some(d) = hurt.get(&self_e) {
            g.hp -= *d;
            if g.hp <= 0.0 {
                g.downed = true;
            }
        }
        if !in_wave {
            // Dawn: heal, rise, and head back to the post.
            g.hp = g.max;
            g.downed = false;
            let to_post = g.post - v.pos;
            if to_post.length() > 0.4 {
                // Far from home (a freed captive marching in from a razed camp) → follow an A*
                // route to the courtyard post so it threads the river crossing and the castle
                // GATE instead of wedging on the wall. Near home (a revived town-guard) → cheap
                // direct steer, no pathing churn. Mirrors the invader keep-march in `siege.rs`.
                let step_target = if to_post.length() > GUARD_PATH_RANGE {
                    if path.cursor >= path.waypoints.len()
                        || now >= path.next_replan
                        || path.goal_cached.distance(g.post) > 2.0
                    {
                        path.waypoints = crate::navgrid::path_to(v.pos, g.post);
                        path.cursor = 0;
                        path.goal_cached = g.post;
                        // Stagger replans so freed captives don't all path on one frame.
                        path.next_replan = now + 0.75 + (self_e.to_bits() % 16) as f32 * 0.05;
                    }
                    while path.cursor < path.waypoints.len()
                        && v.pos.distance(path.waypoints[path.cursor]) < 1.2
                    {
                        path.cursor += 1;
                    }
                    path.waypoints.get(path.cursor).copied().unwrap_or(g.post)
                } else {
                    path.waypoints.clear();
                    path.cursor = 0;
                    g.post
                };
                let cur_y = worldmap::ground_at_world(v.pos.x, v.pos.y).unwrap_or(tf.translation.y);
                if let Some(s) = steer::advance(v.pos, v.facing, step_target, GUARD_SPEED * 0.6 * dt, v.body_r, cur_y, VIL_MAX_TURN * dt) {
                    v.facing = s.facing;
                    v.pos = s.pos;
                    v.moving = s.moving;
                } else {
                    v.moving = false;
                }
            } else {
                v.moving = false;
            }
        } else if g.downed {
            v.moving = false;
        } else {
            g.atk_cd -= dt;
            // Nearest invader within the defend radius.
            let mut best: Option<(Entity, Vec2, f32)> = None;
            for (e, p) in &inv {
                let d = v.pos.distance(*p);
                if d < GUARD_DEFEND_RADIUS && best.is_none_or(|(_, _, bd)| d < bd) {
                    best = Some((*e, *p, d));
                }
            }
            if let Some((te, tp, d)) = best {
                if d < GUARD_MELEE {
                    v.moving = false;
                    let to = tp - v.pos;
                    if to.length_squared() > 1e-4 {
                        let want = to.x.atan2(to.y);
                        v.facing += steer::wrap_pi(want - v.facing).clamp(-VIL_MAX_TURN * 2.0 * dt, VIL_MAX_TURN * 2.0 * dt);
                    }
                    if g.atk_cd <= 0.0 {
                        g.atk_cd = GUARD_ATTACK_CD;
                        dealt.push((te, GUARD_DAMAGE));
                        // Clash SFX, but only when the fight is close to the hero (small earshot).
                        if hero_pos.is_some_and(|hp| v.pos.distance(hp) < GUARD_SFX_RADIUS) {
                            let at = Vec3::new(v.pos.x, tf.translation.y + 1.0, v.pos.y);
                            cues.write(crate::audio::AudioCue::GuardStrike(at));
                        }
                    }
                } else {
                    let cur_y = worldmap::ground_at_world(v.pos.x, v.pos.y).unwrap_or(tf.translation.y);
                    if let Some(s) = steer::advance(v.pos, v.facing, tp, GUARD_SPEED * dt, v.body_r, cur_y, VIL_MAX_TURN * 2.0 * dt) {
                        v.facing = s.facing;
                        v.pos = s.pos;
                        v.moving = s.moving;
                    } else {
                        v.moving = false;
                    }
                }
            } else {
                v.moving = false;
            }
        }

        // Ground-follow (guards own their full transform since they're out of villager_brain).
        let gy = worldmap::ground_at_world(v.pos.x, v.pos.y).unwrap_or(tf.translation.y);
        let bob = if v.moving { (tw * v.gait + v.phase).sin().abs() * v.bob } else { 0.0 };
        // A downed guard sinks to the ground and keels over.
        if g.downed {
            tf.translation = Vec3::new(v.pos.x, gy + 0.1, v.pos.y);
            tf.rotation = Quat::from_rotation_y(v.facing) * Quat::from_rotation_x(std::f32::consts::FRAC_PI_2);
        } else {
            tf.translation = Vec3::new(v.pos.x, gy + bob, v.pos.y);
            tf.rotation = Quat::from_rotation_y(v.facing);
        }
    }

    // Apply guard strikes to invader Health; reap the slain.
    for (e, dmg) in dealt {
        if let Ok((_, _, mut hp)) = invaders.get_mut(e) {
            if hp.hp > 0.0 {
                hp.hp -= dmg;
                if hp.hp <= 0.0 {
                    crate::dying::begin_dying(&mut commands, e, time.elapsed_secs());
                }
            }
        }
    }
}

/// Squared camera distance past which limb animation is skipped (fog/DoF hide the joints).
const LIMB_CULL2: f32 = 70.0 * 70.0;

fn villager_limbs(
    time: Res<Time>,
    cam: Query<&GlobalTransform, With<Camera3d>>,
    vils: Query<(&Villager, &Children, &GlobalTransform, Option<&crate::town::Worker>)>,
    mut parts: Query<(&VilPart, &mut Transform)>,
) {
    let tw = time.elapsed_secs_wrapped();
    let cam_p = cam.single().ok().map(|g| g.translation());
    for (v, children, gt, worker) in &vils {
        if let Some(cp) = cam_p {
            if gt.translation().distance_squared(cp) > LIMB_CULL2 {
                continue;
            }
        }
        let t = tw + v.phase;
        // A posted worker hoes the field: both arms swing forward-down together on a
        // ~1.4s cycle (legs planted, a small synced head-nod). `hoe` ∈ ~[-0.2, 1.2] rad.
        let working = worker.is_some_and(|w| w.at_post);
        let hoe = 0.5 + 0.7 * (t * 4.5).sin();
        for &child in children {
            let Ok((part, mut tf)) = parts.get_mut(child) else { continue };
            tf.rotation = match part.kind {
                PartKind::Leg(sign) => {
                    let s = if v.moving { (t * v.gait).sin() * v.swing } else { (t * 0.8).sin() * 0.02 };
                    Quat::from_rotation_x(sign * s)
                }
                PartKind::Arm(sign) => {
                    if working {
                        // Both arms together (ignore the L/R sign) — a two-handed hoe stroke.
                        Quat::from_rotation_x(hoe)
                    } else {
                        let s = if v.moving { -(t * v.gait).sin() * 0.5 } else { (t * 1.2).sin() * 0.06 };
                        Quat::from_rotation_x(sign * s)
                    }
                }
                PartKind::Head => {
                    if working {
                        Quat::from_rotation_x((t * 4.5).sin() * 0.06) // small nod toward the work
                    } else {
                        let scan = if v.moving { 0.0 } else { (t * 0.7).sin() * 0.18 };
                        Quat::from_rotation_y(scan)
                    }
                }
                PartKind::Tail => Quat::IDENTITY, // villagers have no tail
            };
        }
    }
}

fn pick_walk(v: &mut Villager, spots: &[Vec2], is_kid: bool) {
    // Adults sometimes drift to a shared gathering spot (well / woodpile / market / keep steps)
    // and linger there, so the town clusters into little knots instead of all wandering solo.
    // Kids skip it — they just scamper around their own small play patch.
    if !is_kid && !spots.is_empty() && rng01(&mut v.rng) < 0.42 {
        let s = spots[(rng01(&mut v.rng) * spots.len() as f32) as usize % spots.len()];
        let ang = rng01(&mut v.rng) * TAU;
        let r = rng_range(&mut v.rng, 0.9, 1.7); // ring just outside the prop's blocker
        v.target = s + Vec2::new(ang.cos() * r, ang.sin() * r);
        v.gathering = true;
    } else {
        let ang = rng01(&mut v.rng) * TAU;
        let r = rng_range(&mut v.rng, v.wander_r * 0.3, v.wander_r);
        v.target = v.home + Vec2::new(ang.cos() * r, ang.sin() * r);
        v.gathering = false;
    }
    v.mode = Mode::Walk;
    v.timer = rng_range(&mut v.rng, 3.0, 7.0);
}

// ── Models (ported from Villager.tsx) ────────────────────────────────────────────────

#[derive(Clone, Copy)]
enum Kind {
    Peasant { skin: u32, tunic: u32, hat: bool },
    Guard { skin: u32, tunic: u32 },
}

struct PartDef {
    kind: PartKind,
    pivot: Vec3,
    mesh: Mesh,
}
struct VSpec {
    torso: Mesh,
    parts: Vec<PartDef>,
}

fn spec(kind: Kind) -> VSpec {
    let (skin_hex, tunic_hex) = match kind {
        Kind::Peasant { skin, tunic, .. } => (skin, tunic),
        Kind::Guard { skin, tunic } => (skin, tunic),
    };
    let guard = matches!(kind, Kind::Guard { .. });
    let hat = matches!(kind, Kind::Peasant { hat: true, .. });
    let skin = lin(skin_hex);
    let tunic = lin(tunic_hex);
    let pant = lin(PANT);
    let hair = lin(HAIR);
    let armor = lin(ARMOR);

    // Static torso: tunic + (guard) chestplate.
    let mut torso_parts = vec![bx(0.42, 0.48, 0.26, v(0.0, 0.7, 0.0), tunic)];
    if guard {
        torso_parts.push(bx(0.46, 0.4, 0.3, v(0.0, 0.7, 0.0), armor));
    }
    let torso = group(torso_parts);

    // Head: skull + hair + eyes + (guard helmet / peasant hat).
    let mut head_parts = vec![
        bx(0.3, 0.3, 0.3, Vec3::ZERO, skin),
        bx(0.31, 0.08, 0.31, v(0.0, 0.13, 0.0), hair),
        bx(0.04, 0.04, 0.02, v(-0.07, 0.03, 0.16), lin(EYE)),
        bx(0.04, 0.04, 0.02, v(0.07, 0.03, 0.16), lin(EYE)),
    ];
    if guard {
        head_parts.push(bx(0.34, 0.16, 0.34, v(0.0, 0.16, 0.0), armor)); // helmet
        head_parts.push(cone(0.1, 0.16, v(0.0, 0.3, 0.0), Quat::IDENTITY, armor)); // crest spike
    } else if hat {
        head_parts.push(cone(0.22, 0.2, v(0.0, 0.22, 0.0), Quat::IDENTITY, lin(HAT)));
    }
    let head = group(head_parts);

    // Legs (top at the hip pivot).
    let leg = || group(vec![bx(0.16, 0.36, 0.18, v(0.0, -0.18, 0.0), pant)]);

    // Arms — the right arm carries a sword for guards.
    let arm = |with_sword: bool| {
        let mut p = vec![
            bx(0.13, 0.36, 0.22, v(0.0, -0.18, 0.0), tunic), // sleeve
            bx(0.12, 0.1, 0.2, v(0.0, -0.42, 0.0), skin),    // hand
        ];
        if guard {
            p.push(bx(0.18, 0.16, 0.26, v(0.0, 0.02, 0.0), armor)); // pauldron
        }
        if with_sword {
            let so = v(0.0, -0.46, 0.1);
            p.push(bx(0.18, 0.06, 0.05, so, lin(SWORD_GUARD)));
            p.push(bx(0.05, 0.06, 0.5, so + v(0.0, 0.0, 0.32), lin(SWORD_BLADE)));
        }
        group(p)
    };

    let parts = vec![
        PartDef { kind: PartKind::Leg(1.0), pivot: v(-0.11, 0.34, 0.0), mesh: leg() },
        PartDef { kind: PartKind::Leg(-1.0), pivot: v(0.11, 0.34, 0.0), mesh: leg() },
        PartDef { kind: PartKind::Arm(1.0), pivot: v(0.27, 0.92, 0.0), mesh: arm(guard) }, // right (+sword)
        PartDef { kind: PartKind::Arm(-1.0), pivot: v(-0.27, 0.92, 0.0), mesh: arm(false) },
        PartDef { kind: PartKind::Head, pivot: v(0.0, 1.12, 0.0), mesh: head },
    ];
    VSpec { torso, parts }
}

// ── Placement ────────────────────────────────────────────────────────────────────

/// Spawn the castle town's villagers. Called from `worldmap::build` after the castle (so its
/// wall/keep/house blockers are registered). ~10 total: 2 guards by the keep, 4 spilling out
/// the gates, the rest milling the courtyard.
pub fn populate(commands: &mut Commands, meshes: &mut Assets<Mesh>, materials: &mut Assets<StandardMaterial>) {
    let mat = materials.add(StandardMaterial { base_color: Color::WHITE, perceptual_roughness: 0.85, ..default() });
    let mut rng: u32 = 0x5117_aced;
    let mut placed: Vec<Vec2> = Vec::new();

    // Two guards posted in front of the keep door (+Z side).
    for (i, (gx, gz)) in [(-2.3f32, 3.8f32), (2.3, 3.8)].into_iter().enumerate() {
        let kind = Kind::Guard { skin: SKIN[i % SKIN.len()], tunic: TUNIC[1] };
        let home = Vec2::new(gx, gz);
        spawn(commands, meshes, &mat, kind, home, home, 1.6, 1.1, SCALE, next_u32(&mut rng));
        placed.push(home);
    }

    // One peasant by each gate, homed just inside so they wander in and out through the gap.
    for (i, g) in crate::castle::gate_centers().into_iter().enumerate() {
        let home = g + (-g).normalize_or_zero() * 1.5;
        let kind = Kind::Peasant { skin: SKIN[i % SKIN.len()], tunic: TUNIC[i % TUNIC.len()], hat: i % 2 == 0 };
        spawn(commands, meshes, &mat, kind, home, home, 1.6, 3.6, SCALE, next_u32(&mut rng));
        placed.push(home);
    }

    // The rest mill around the open courtyard.
    let half = crate::castle::courtyard_half();
    for i in 0..4 {
        let Some(home) = courtyard_spot(&mut rng, half, &placed) else { continue };
        placed.push(home);
        let kind = Kind::Peasant { skin: SKIN[(i + 1) % SKIN.len()], tunic: TUNIC[(i + 2) % TUNIC.len()], hat: i % 2 == 1 };
        spawn(commands, meshes, &mat, kind, home, home, 1.6, 3.0, SCALE, next_u32(&mut rng));
    }

    // A little market just outside the south gate: a striped stall + two robed traders who
    // wander around it (the visible counterpart to the menu shop). The keep gate is -Z.
    let south = crate::castle::gate_centers()[0];
    let market = south + Vec2::new(2.5, -5.0);
    let my = worldmap::ground_at_world(market.x, market.y).unwrap_or(0.0);
    commands.spawn((
        Mesh3d(meshes.add(market_stall_mesh())),
        MeshMaterial3d(mat.clone()),
        Transform::from_xyz(market.x, my, market.y),
        BiomeEntity,
    ));
    // Solid stall — the 1.8-wide counter + posts block; traders/hero walk around it.
    crate::blockers::add_box(market.x, market.y, 0.95, 0.4);
    for i in 0..2 {
        let home = market + Vec2::new(if i == 0 { -1.6 } else { 1.6 }, 0.8);
        let kind = Kind::Peasant { skin: SKIN[i % SKIN.len()], tunic: MERCHANT_ROBE[i % 2], hat: false };
        spawn(commands, meshes, &mat, kind, home, home, 1.3, 2.4, SCALE, next_u32(&mut rng));
    }

    // Two wandering pilgrims who trek between the island's landmarks, hinting the way (see
    // `pilgrim_brain` / `pilgrim_hint`). They start just outside a gate and head off.
    let gates = crate::castle::gate_centers();
    for i in 0..2 {
        let g = gates[i % gates.len()];
        let home = g + (-g).normalize_or_zero() * 2.0;
        let kind = Kind::Peasant { skin: SKIN[i % SKIN.len()], tunic: PILGRIM_ROBE, hat: true };
        let e = spawn(commands, meshes, &mat, kind, home, home, 1.5, 2.0, SCALE, next_u32(&mut rng));
        commands.entity(e).insert(Pilgrim {
            target: home,
            pause: 0.0,
            hint_cd: 0.0,
            rng: next_u32(&mut rng) | 1,
        });
    }

    // ── Gathering stations: a well + a woodpile the townsfolk cluster around for a chat or a
    // chore. The market and the keep steps round out the gather spots. Props are solid (small
    // blockers); the gather ring in `pick_walk` sits just outside them so nobody gets stuck. ──
    let mut spots: Vec<Vec2> = vec![market, Vec2::new(0.0, 3.4)]; // market stall + keep steps
    if let Some(well) = courtyard_spot(&mut rng, half, &placed) {
        let wy = worldmap::ground_at_world(well.x, well.y).unwrap_or(0.0);
        commands.spawn((
            Mesh3d(meshes.add(well_mesh())),
            MeshMaterial3d(mat.clone()),
            Transform::from_xyz(well.x, wy, well.y),
            BiomeEntity,
        ));
        crate::blockers::add_box(well.x, well.y, 0.5, 0.5);
        placed.push(well);
        spots.push(well);
    }
    if let Some(pile) = courtyard_spot(&mut rng, half, &placed) {
        let py = worldmap::ground_at_world(pile.x, pile.y).unwrap_or(0.0);
        commands.spawn((
            Mesh3d(meshes.add(woodpile_mesh())),
            MeshMaterial3d(mat.clone()),
            Transform::from_xyz(pile.x, py, pile.y),
            BiomeEntity,
        ));
        crate::blockers::add_box(pile.x, pile.y, 0.6, 0.5);
        placed.push(pile);
        spots.push(pile);
    }
    commands.insert_resource(TownSpots(spots));

    // ── Kids: a few small villagers scampering around a play patch just outside the south gate.
    // High speed + tiny wander radius + the `Kid` marker → short darting bursts that read as play
    // (they skip the adults' gathering behaviour). Curfew hides them at night like any townsfolk. ──
    let play = gates[0] + (-gates[0]).normalize_or_zero() * 3.0;
    for i in 0..4 {
        let home = play + Vec2::new(rng_range(&mut rng, -1.6, 1.6), rng_range(&mut rng, -1.6, 1.6));
        let kind = Kind::Peasant { skin: SKIN[i % SKIN.len()], tunic: TUNIC[(i + 1) % TUNIC.len()], hat: i % 2 == 0 };
        let e = spawn(commands, meshes, &mat, kind, home, home, 2.8, 1.7, KID_SCALE, next_u32(&mut rng));
        commands.entity(e).insert(Kid);
    }
}

/// A small market stall: four posts, a striped awning, a plank counter, and a few goods crates.
/// One merged vertex-coloured mesh against the shared white material.
fn market_stall_mesh() -> Mesh {
    const WOOD: u32 = 0x6b4a2a;
    const DARK: u32 = 0x4a3322;
    const RED: u32 = 0xb33a32;
    const CREAM: u32 = 0xe7d8b0;
    let mut parts = vec![
        // counter
        bx(1.8, 0.5, 0.6, v(0.0, 0.25, 0.0), lin(WOOD)),
        bx(1.8, 0.08, 0.66, v(0.0, 0.5, 0.0), lin(DARK)),
    ];
    // four corner posts
    for (px, pz) in [(-0.85f32, -0.28f32), (0.85, -0.28), (-0.85, 0.28), (0.85, 0.28)] {
        parts.push(bx(0.08, 1.3, 0.08, v(px, 0.65, pz), lin(DARK)));
    }
    // striped awning (alternating red/cream slats) tilted forward
    for s in 0..5 {
        let c = if s % 2 == 0 { RED } else { CREAM };
        let z = -0.5 + s as f32 * 0.26;
        parts.push(tilt_slat(z, c));
    }
    // a couple of goods crates on the counter
    parts.push(bx(0.3, 0.3, 0.3, v(-0.5, 0.65, 0.0), lin(0x8a6a3a)));
    parts.push(bx(0.26, 0.26, 0.26, v(0.45, 0.63, 0.05), lin(0x9c7a44)));
    group(parts)
}

/// A stone well: a squat curb with a dark water hole, two posts, a crossbar, and a bucket.
fn well_mesh() -> Mesh {
    const STONE: u32 = 0x8a8f96;
    const DARKW: u32 = 0x2a3338; // water in the shaft
    const WOOD: u32 = 0x6b4a2a;
    const DARK: u32 = 0x4a3322;
    let parts = vec![
        bx(0.9, 0.5, 0.9, v(0.0, 0.25, 0.0), lin(STONE)),  // curb
        bx(0.6, 0.46, 0.6, v(0.0, 0.29, 0.0), lin(DARKW)), // water hole (sits proud of the curb top)
        bx(0.1, 1.05, 0.1, v(-0.38, 1.0, 0.0), lin(WOOD)), // post
        bx(0.1, 1.05, 0.1, v(0.38, 1.0, 0.0), lin(WOOD)),  // post
        bx(0.92, 0.1, 0.12, v(0.0, 1.5, 0.0), lin(DARK)),  // crossbar
        bx(0.22, 0.24, 0.22, v(0.0, 1.06, 0.0), lin(WOOD)), // hanging bucket
    ];
    group(parts)
}

/// A woodpile: stacked logs (boxes lying along Z), a pyramid 3-2-1.
fn woodpile_mesh() -> Mesh {
    const LOG: u32 = 0x7a5a32;
    const LOG2: u32 = 0x6b4a2a;
    let mut parts = Vec::new();
    let rows: [(f32, &[f32]); 3] = [(0.15, &[-0.5, 0.0, 0.5]), (0.42, &[-0.25, 0.25]), (0.69, &[0.0])];
    for (y, xs) in rows {
        for (i, x) in xs.iter().enumerate() {
            let c = if i % 2 == 0 { LOG } else { LOG2 };
            parts.push(bx(0.24, 0.24, 1.0, v(*x, y, 0.0), lin(c)));
        }
    }
    group(parts)
}

/// One awning slat, tilted forward over the counter at row offset `z`.
fn tilt_slat(z: f32, c: u32) -> Mesh {
    tinted(
        Cuboid::new(1.9, 0.05, 0.28)
            .mesh()
            .build()
            .rotated_by(Quat::from_rotation_x(-0.35))
            .translated_by(v(0.0, 1.4 + z * 0.18, z)),
        lin(c),
    )
}

/// Reject-sample an open courtyard tile (inside the walls, off the keep/houses, spread out).
fn courtyard_spot(rng: &mut u32, half: (f32, f32), placed: &[Vec2]) -> Option<Vec2> {
    for _ in 0..400 {
        let x = rng_range(rng, -(half.0 - 2.0), half.0 - 2.0);
        let z = rng_range(rng, -(half.1 - 2.0), half.1 - 2.0);
        let p = Vec2::new(x, z);
        if crate::blockers::is_blocked(x, z) || p.length() < 4.5 {
            continue; // skip props/walls/houses and the central keep
        }
        if placed.iter().any(|q| q.distance(p) < 2.0) {
            continue;
        }
        return Some(p);
    }
    None
}

#[allow(clippy::too_many_arguments)]
fn spawn(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    mat: &Handle<StandardMaterial>,
    kind: Kind,
    home: Vec2,
    pos: Vec2,
    speed: f32,
    wander_r: f32,
    scale: f32,
    seed: u32,
) -> Entity {
    let s = spec(kind);
    let torso = meshes.add(s.torso);
    let parts: Vec<(PartKind, Vec3, Handle<Mesh>)> =
        s.parts.into_iter().map(|p| (p.kind, p.pivot, meshes.add(p.mesh))).collect();

    let mut r = seed | 1;
    let phase = rng01(&mut r) * TAU;
    let facing = rng01(&mut r) * TAU;
    let y = worldmap::ground_at_world(pos.x, pos.y).unwrap_or(0.0);

    let vil = Villager {
        home,
        target: pos,
        pos,
        facing,
        speed,
        wander_r,
        gait: 8.0,
        swing: 0.5,
        bob: 0.045,
        body_r: 0.28,
        phase,
        moving: false,
        mode: Mode::Idle,
        timer: rng_range(&mut r, 0.5, 4.0),
        rng: r,
        gathering: false,
    };

    let root = commands
        .spawn((
            Transform { translation: Vec3::new(pos.x, y, pos.y), rotation: Quat::from_rotation_y(facing), scale: Vec3::splat(scale) },
            Visibility::Visible,
            vil,
            BiomeEntity,
        ))
        .id();
    commands.entity(root).with_children(|p| {
        p.spawn((Mesh3d(torso), MeshMaterial3d(mat.clone()), Transform::default()));
        for (kind, pivot, mesh) in parts {
            p.spawn((Mesh3d(mesh), MeshMaterial3d(mat.clone()), Transform::from_translation(pivot), VilPart { kind }));
        }
    });

    // Armoured townsfolk double as town guards — they fight invaders at night and can be pulled to
    // staff a producer by day (the `Townsfolk` pool). The NavPath caches an A* route home for a
    // freed captive marching in from a far camp (see `guard_combat`).
    if matches!(kind, Kind::Guard { .. }) {
        commands.entity(root).insert((
            Guard { hp: GUARD_MAX_HP, max: GUARD_MAX_HP, atk_cd: 0.0, downed: false, post: home },
            crate::navgrid::NavPath::default(),
            Townsfolk,
        ));
    }
    root
}

// ── Mesh helpers ─────────────────────────────────────────────────────────────────

fn v(x: f32, y: f32, z: f32) -> Vec3 {
    Vec3::new(x, y, z)
}
fn tinted(mut m: Mesh, c: [f32; 4]) -> Mesh {
    let n = m.count_vertices();
    m.insert_attribute(Mesh::ATTRIBUTE_COLOR, vec![c; n]);
    m
}
fn group(parts: Vec<Mesh>) -> Mesh {
    let mut it = parts.into_iter();
    let mut base = it.next().expect("at least one part");
    for p in it {
        base.merge(&p).expect("villager parts share attributes");
    }
    base.duplicate_vertices();
    base.compute_flat_normals();
    base
}
fn bx(w: f32, h: f32, d: f32, off: Vec3, c: [f32; 4]) -> Mesh {
    tinted(Cuboid::new(w, h, d).mesh().build().translated_by(off), c)
}
fn cone(r: f32, h: f32, off: Vec3, rot: Quat, c: [f32; 4]) -> Mesh {
    tinted(Cone { radius: r, height: h }.mesh().build().rotated_by(rot).translated_by(off), c)
}

// ── Deterministic mulberry32 RNG ─────────────────────────────────────────────────────

fn next_u32(s: &mut u32) -> u32 {
    *s = s.wrapping_add(0x6d2b_79f5);
    let mut t = *s;
    t = (t ^ (t >> 15)).wrapping_mul(t | 1);
    t ^= t.wrapping_add((t ^ (t >> 7)).wrapping_mul(t | 61));
    t ^ (t >> 14)
}
fn rng01(s: &mut u32) -> f32 {
    next_u32(s) as f32 / 4_294_967_296.0
}
fn rng_range(s: &mut u32, lo: f32, hi: f32) -> f32 {
    lo + rng01(s) * (hi - lo)
}
