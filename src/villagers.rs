//! **Villagers** — the castle town's population, ported from the TS `Villager.tsx` mesh tree.
//! Box-mesh humanoids that idle and stroll around the courtyard, with a few posted at the keep +
//! spilling out through the gates. The town pool ([`Townsfolk`]) is a full combat participant
//! now: every member carries [`NpcHp`], guards hunt orks *and* predators near their post, passive
//! workers fight back when struck ([`FightBack`]), and death is permanent (replacements are grown
//! from the food surplus by day — see `town.rs`).
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
use crate::biped::{BipedDrive, BipedMeshes, BipedRig};
use crate::creature::{surf, Surf};
use crate::critters::PartKind;
use crate::palette::{lin, lin_scaled};
use crate::peasant_model::{peasant_biped_meshes, PeasantKind};
use crate::steer;
use crate::worldmap;

/// Townsfolk turn at a relaxed rate. rad/s.
const VIL_MAX_TURN: f32 = 3.0;
/// Root scale of a townsperson on the shared studio biped rig (`biped.rs`). The studio skeleton is
/// ~1.8u tall at scale 1.0; this keeps adults a touch shorter than the hero (close to the orks'
/// in-world height). Bumped ×1.35 (was 0.6) alongside the hero/ork/house rescale. `FOREST_VILLINE`.
const SCALE: f32 = 0.81;
/// Child villagers — the same rig scaled right down (+ a bigger head, see [`build_biped_body`]) so
/// the suburbs have visibly childlike kids underfoot.
const KID_SCALE: f32 = SCALE * 0.55;
/// Drop the biped rig so the feet sit on the ground (matches the `peasant` model-viewer offset).
const VIL_RIG_OFF: f32 = -0.06;

// Palette (sRGB hex, from Villager.tsx) — widened for per-villager cosmetic variety.
const SKIN: [u32; 6] = [0xe8c4a0, 0xdca78a, 0xc89070, 0xc08866, 0xa36b4a, 0x8a5638];
const TUNIC: [u32; 6] = [0x5a8fc8, 0x7a3a26, 0x4a6a3a, 0x8a6a3a, 0x3a7a72, 0x6a4a6a];
const PANT_TONES: [u32; 4] = [0x3a2a18, 0x2e2620, 0x4a3a2a, 0x33302a];
const PANT: u32 = 0x3a2a18; // == PANT_TONES[0] (kept as an alias)
/// Natural hair colours: brown / dark-brown / near-black / chestnut / auburn / sandy / ash / dark-ash.
const HAIR_TONES: [u32; 8] = [0x3a2418, 0x2a1c12, 0x1c1410, 0x6b4a2a, 0x7a3b1e, 0x8a7a5a, 0xa9854e, 0x5a4636];
const HAIR: u32 = 0x3a2418; // == HAIR_TONES[0] (kept as an alias)
const GREY_HAIR: [u32; 3] = [0x9a9488, 0xcfc8be, 0x7d7872]; // elder grey / white / steel
const HAT: u32 = 0xa02a26;
const EYE: u32 = 0x141414;
const LIP: u32 = 0x7a4a44; // mouth / lip box
const ARMOR: u32 = 0x9aa0aa;
const SWORD_BLADE: u32 = 0xd8dde6;
const SWORD_GUARD: u32 = 0xcaa23a;
const BELT: u32 = 0x4a3526; // leather belt
const BUCKLE: u32 = 0xb9962e; // brass buckle
const BOOT: u32 = 0x33241a; // dark leather boot
const SOOT: u32 = 0x2a241f; // miner grime on the lower face
const MINER_LAMP: u32 = 0xffd27a; // brass lamp clip on the miner cap
const STRAP: u32 = 0x3a2e22; // guard helmet chin-strap leather
/// Dusky cloak of the wandering pilgrims who trek between the island's old landmarks.
const PILGRIM_ROBE: u32 = 0x6a5a8a;

// ── Components ───────────────────────────────────────────────────────────────────

#[derive(Clone, Copy, PartialEq)]
enum Mode {
    Idle,
    Walk,
}

/// The shared villager pose/locomotion state. The gameplay-relevant fields are `pub(crate)` so
/// the sibling NPC brains (`lumberjack.rs` chop/flee steering, the predator AI in `wildlife.rs`)
/// can drive/read the same pose the in-module brains do.
#[derive(Component)]
pub struct Villager {
    home: Vec2,
    target: Vec2,
    pub(crate) pos: Vec2,
    pub(crate) facing: f32,
    pub(crate) speed: f32,
    wander_r: f32,
    pub(crate) gait: f32,
    swing: f32,
    pub(crate) bob: f32,
    pub(crate) body_r: f32,
    pub(crate) phase: f32,
    pub(crate) moving: bool,
    mode: Mode,
    timer: f32,
    rng: u32,
    /// True while walking to a gathering spot (so the brain lingers longer on arrival).
    gathering: bool,
    /// Timestamp (`elapsed_secs`) of the last melee strike; [`villager_limbs`] plays a weapon-swing
    /// for [`ATTACK_ANIM_DUR`] after it. `0` = never struck. Stamped by [`guard_combat`] and
    /// [`npc_fight_back`] when a blow lands — the townsfolk mirror of the ork rig's `atk_anim`.
    pub(crate) atk_anim: f32,
    /// Smoothed local head yaw (radians) — eased toward the idle look-around scan, or toward the
    /// passing hero when he's near, so a standing villager turns to watch you go by (see
    /// [`villager_limbs`]'s greeting logic).
    head_yaw: f32,
    /// Greeting-nod envelope, `1.0` → `0.0` over [`GREET_NOD_DUR`]; kicked once each time the hero
    /// walks up close (re-armed by [`Villager::greet_armed`] when he leaves).
    greet: f32,
    /// True while the hero is out of greeting range, arming the next nod so it fires once per
    /// approach rather than continuously while he lingers nearby.
    greet_armed: bool,
}

impl Villager {
    /// Body centre (world XZ) + collision radius — read by the hero's body-collision pass so he
    /// can't clip through townsfolk (the same one-way shove the orks/animals get).
    pub fn body(&self) -> (Vec2, f32) {
        (self.pos, self.body_r)
    }
}

/// An articulated body part under a villager root. `pub(crate)` so the staged-scene mime driver
/// (`scenes::drive_scene_mason`) can pose limbs directly on actors that `villager_limbs` skips.
#[derive(Component)]
pub(crate) struct VilPart {
    pub(crate) kind: PartKind,
}

/// A child villager — wanders fast in short bursts around a small play patch (and skips the
/// adults' gathering/chore behaviour). Otherwise a normal villager (same rig, smaller scale), so
/// the night curfew ([`townsfolk_curfew`]) and [`villager_brain`] handle it for free: it simply
/// disappears with the rest of the ambient townsfolk when the wave falls, and reappears at dawn.
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
/// blows) and, in peacetime, sallies out after any ork or predator that prowls near its post
/// (short detect, short leash — see [`guard_combat`]), walking back to the post after the kill.
/// Its hit points live in the shared [`NpcHp`]; at 0 HP it dies for good like anyone else.
#[derive(Component)]
pub struct Guard {
    atk_cd: f32,
    post: Vec2,
}

/// A guard that has answered the **muster** (the player pressed `K`): it abandons its fixed post and
/// follows the hero. [`rally_follow`] rewrites its [`Guard::post`] each frame to a reserved slot in a
/// loose blob around him, so the *existing* [`guard_combat`] follow-the-post + peel-to-fight +
/// regroup logic makes the war party trail and protect him with no separate movement code. `slot`
/// (join order) places it via a phyllotaxis spiral so dozens pack evenly without stacking; `home` is
/// its real post, restored on stand-down. Transient battlefield state — never saved (like timed
/// `Buffs` / live invaders); a loaded or new run boots unrallied (and `K` re-rallies).
#[derive(Component, Clone, Copy)]
pub struct Rallied {
    slot: usize,
    home: Vec2,
}

/// Hit points for any town-pool NPC ([`Townsfolk`] — guard or worker alike). Death is
/// **permanent**: at 0 HP the body crumples (`dying.rs`) and the town's population drops by one.
/// Replacements are grown from the food surplus by day (`town::population_system`) — nobody is
/// revived at dawn any more. The one backstop is the castle larder (core `town_store`): the
/// first two peasants eat free and an emptied town regrows to that pair organically by day,
/// so someone can always work the farm again.
#[derive(Component)]
pub struct NpcHp {
    pub hp: f32,
    pub max: f32,
}

/// A **passive** townsperson (a worker — no [`Guard`] role) that has been struck in anger: it
/// stands its ground and trades blows with its attacker until one of them is dead, then goes back
/// to work. Inserted by [`npc_damage_apply`], driven by [`npc_fight_back`]. Guards never carry
/// this — their own combat brain handles retaliation.
#[derive(Component)]
pub struct FightBack {
    target: Entity,
    atk_cd: f32,
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
        Guard { atk_cd: 0.0, post },
        crate::navgrid::NavPath::default(),
    ));
}

/// Enlist one town-pool member `e` into the muster (war party) at ring `slot`. A worker downs tools
/// (sheds [`crate::town::Worker`] + any chop/mine job) and is armed; an existing guard keeps its
/// post. Either way the member is tagged [`Rallied`] with `home` = the courtyard spot it walks back
/// to on stand-down (its current post for a guard, its home/standing spot for a freed worker —
/// mirrors [`rearm_townsfolk`]). Shared by [`muster_keys`] and the `FOREST_MUSTER` staging hook.
fn rally_one(commands: &mut Commands, e: Entity, guard: Option<&Guard>, v: &Villager, is_worker: bool, slot: usize) {
    let home = match guard {
        Some(g) => g.post,
        None => if v.pos.length() > 26.0 { v.home } else { v.pos },
    };
    if is_worker {
        commands
            .entity(e)
            .try_remove::<crate::town::Worker>()
            .try_remove::<crate::lumberjack::ChopJob>()
            .try_remove::<crate::miner::MineJob>();
    }
    if guard.is_none() {
        arm_as_guard(commands, e, home);
    }
    commands.entity(e).try_insert(Rallied { slot, home });
}

// NPC combat tuning. Townsfolk take damage ONLY through the [`NpcDamage`] channel (ork blades
// from `siege.rs`, predator bites from `wildlife.rs`) — no self-inflicted melt — so guards are
// beefy enough that a pair wins a 1v1 but a wave still overwhelms them.
const NPC_MAX_HP: f32 = 210.0; // raised from 140: defender HP is flat but the night dmg ramp is steep, so late nights gangs shredded peasants — bigger pool lets the militia survive a night-4+ swarm

/// Flat max-HP the militia gains for nights already survived: peasants harden *a bit* as the siege
/// wears on so the steep night dmg ramp doesn't shred them faster each night. Grows per night,
/// capped so it never dwarfs the base pool. Derived from the (saved) `wave_index`, so it round-trips
/// loads for free; applied in [`guard_arms_upkeep`] which only ever raises `max`.
const NIGHT_HP_PER: f32 = 14.0;
const NIGHT_HP_CAP: f32 = 140.0;
fn night_hp_bonus(wave_index: i32) -> f32 {
    ((wave_index + 1).max(0) as f32 * NIGHT_HP_PER).min(NIGHT_HP_CAP)
}
/// Base unarmed-guard strike. Each `villager_arms_tier` (the Defense "Guard Arms" line) lifts this
/// to the advertised figure via [`guard_damage`]; the guard-vigor line adds flat HP atop NPC_MAX_HP.
const GUARD_DAMAGE: f32 = 6.3; // −30% off the old 9: guards trade slower, lean on staying power

/// A guard's per-strike damage for arms tier `t`. Honours the upgrade-tree copy: tier 1 → 16,
/// tier 2 → 23, tier 3 ("Town Champions") → 32, +9 per tier beyond. Tier 0 is the unarmed base.
/// (Previously the tier was unwired — guards always hit [`GUARD_DAMAGE`] no matter the upgrades.)
fn guard_damage(tier: u32) -> f32 {
    match tier {
        0 => GUARD_DAMAGE,
        1 => 16.0,
        2 => 23.0,
        n => 23.0 + (n - 2) as f32 * 9.0,
    }
}
/// How far a guard will march to hunt an invader at night. Large enough to cover the whole
/// defended area, so the reserve actively goes out to meet the wave instead of standing idle until
/// an ork wanders within arm's reach (the old 12-unit passive radius left guards doing nothing).
const GUARD_HUNT_RADIUS: f32 = 60.0;
/// Peacetime: a hostile (ork or predator) this near a guard's POST is engaged on sight…
const GUARD_DETECT: f32 = 15.0;
/// …and chased only while it stays this near the post — past the leash the guard lets it go and
/// ambles home, so wildlife can't kite the militia across the island.
const GUARD_LEASH: f32 = 25.0;
/// Peacetime mend rate (HP/s). Replaces the old instant dawn full-heal: a guard that brawls a
/// wolf pack back-to-back stays wounded for a while, and a dead one stays dead.
const GUARD_REGEN: f32 = 3.0;
const GUARD_MELEE: f32 = 1.6;
const GUARD_SPEED: f32 = 2.9; // a touch quicker so the muster keeps pace with the hero (was 2.4)
const GUARD_ATTACK_CD: f32 = 1.0;
/// A passive townsperson's self-defence swing: weak (a hoe, an axe haft) but real. −30% off the old 6.
const NPC_DEFEND_DMG: f32 = 4.2;
const NPC_DEFEND_CD: f32 = 1.2;
const NPC_MELEE: f32 = 1.5;
/// A defender gives up the brawl when its attacker breaks off this far away.
const NPC_GIVE_UP: f32 = 12.0;
/// A guard's strike only emits a clash SFX when this near the hero — a small earshot so distant
/// skirmishes across the field stay silent and only the fight beside you is heard.
const GUARD_SFX_RADIUS: f32 = 14.0;
/// Past this distance from its post a guard (a freed captive marching in from a razed camp) routes
/// home by A* through the gates; nearer than this a revived town-guard just steers in directly.
const GUARD_PATH_RANGE: f32 = 4.0;

/// Rally formation (the **muster**): the war party packs into a loose blob around the hero via a
/// phyllotaxis spiral — slot `n` sits at radius `BASE + SPREAD·√n`, angle `n·GOLDEN`. This spaces
/// any number of guards evenly without stacking and keeps the blob tight (slot 0 ≈2.6u out, slot 30
/// ≈7.5u). The hero-relative point becomes each [`Rallied`] guard's `post`, so the tested guard AI
/// follows it.
const RALLY_BASE_R: f32 = 2.6;
const RALLY_SPREAD: f32 = 0.9;
/// Golden angle (rad) — successive rally slots step by this so the blob fills evenly, not in spokes.
const RALLY_GOLDEN: f32 = 2.399_963_2;
/// How near the hero a hostile must be for a RALLIED guard to peel off and fight it. Generous
/// (covers the loose blob + a little reach) so the war party engages the hero's foe — wave ork,
/// Hold garrison, or the Warlord — without chasing distant fights it can't see.
const RALLY_ENGAGE_RADIUS: f32 = 30.0;

/// One blow landed on a townsperson this frame. `attacker` lets the victim retaliate
/// ([`FightBack`] for passive folk) — `None` only for source-less damage.
pub struct NpcHit {
    pub victim: Entity,
    pub amount: f32,
    pub attacker: Option<Entity>,
}

/// Damage dealt to town NPCs this frame, pushed by the invader AI in `siege.rs` and the predator
/// AI in `wildlife.rs`, drained into [`NpcHp`] by [`npc_damage_apply`]. The mirror of the hero's
/// [`crate::player::PendingHeroDamage`] — combat stays store-mediated, no collision events.
#[derive(Resource, Default)]
pub struct NpcDamage(pub Vec<NpcHit>);

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
/// Peasants freed when a camp's warband falls — a whole **cage** of captives, not one. With organic
/// town growth deliberately slowed (core `SETTLE_FOOD` raised), clearing camps in daylight is now
/// the main way the town grows: 5 camps × 3 = +15, the army's backbone. Raw-added (uncapped, like
/// the old +1) — the realistic max (camps + start ≈ 17) stays under the 24 house cap, and a farm
/// feeds far more than the headcount, so over-house peasants don't starve.
const CAMP_RESCUE_POP: u32 = 3;

impl Plugin for VillagersPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<RescuedCamps>()
            .init_resource::<NpcDamage>()
            .init_resource::<TownSpots>()
            // `villager_drive` maps each townsperson's brain state → its `BipedDrive`; the shared
            // `biped::animate_biped` then poses the studio skeleton (locomotion/attack/work strokes).
            // `villager_limbs` no longer poses limbs (the biped does), but still runs to keep the
            // head-greeting/idle-fidget bookkeeping warm for a future biped head-track. Both ungated
            // so a frozen/paused world still draws the town animated.
            .add_systems(Update, (villager_drive, villager_limbs))
            // Ungated so they fire on the day↔night edge even if the world is frozen (panel open):
            .add_systems(Update, (townsfolk_curfew, muster_townsfolk))
            // Ungated: disband a leftover war party on any load (fires off `GameLoaded`). Ordered
            // after rally_follow and before guard_combat for the SAME reason muster_keys is — its
            // post-restore is a direct write but the `Rallied` drop is deferred, so rally_follow must
            // run first (and have its clobber overwritten) or the disbanded guards strand off-post.
            .add_systems(
                Update,
                disband_on_load.after(rally_follow).before(guard_combat),
            )
            .add_systems(OnExit(crate::game_state::AppState::StartScreen), reset_rescues)
            .add_systems(OnExit(crate::game_state::AppState::GameOver), reset_rescues)
            .add_systems(
                Update,
                (
                    villager_brain,
                    worker_steer,
                    pilgrim_brain,
                    pilgrim_hint,
                    npc_damage_apply,
                    npc_fight_back,
                    guard_arms_upkeep,
                    // Order is load-bearing: rally_follow → muster_keys → guard_combat. rally_follow
                    // writes each rallied guard's hero-relative post; muster_keys' stand-down then
                    // overwrites it with the real home post — and MUST win, because dropping the
                    // `Rallied` marker is a *deferred* command that only flushes next frame, so if
                    // muster_keys ran first rally_follow would re-clobber the restored post and the
                    // disbanded guards would strand at a stale midfield point instead of going home.
                    rally_follow.before(muster_keys),
                    muster_keys.before(guard_combat),
                    stage_muster,
                    guard_combat,
                    rearm_townsfolk,
                    reskin_townsfolk,
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
            // Down tools entirely: a woodcutter/miner drops its tree/boulder job too, or it
            // would keep the work steering alongside the guard brain it's about to get.
            commands
                .entity(e)
                .try_remove::<crate::town::Worker>()
                .try_remove::<crate::lumberjack::ChopJob>()
                .try_remove::<crate::miner::MineJob>();
        }
    }
}

/// Keep every idle pool member armed: any `Townsfolk` with neither a [`Guard`] role nor a
/// [`crate::town::Worker`] job (a fresh recruit, a just-mustered worker, or one whose plot collapsed)
/// is (re)posted as a guard where it stands. The standing reserve thus always defends by day and
/// fights by night; employment is the only thing that pulls one off guard duty.
fn rearm_townsfolk(
    mut commands: Commands,
    idle: Query<
        (Entity, &Villager),
        (With<Townsfolk>, Without<Guard>, Without<crate::town::Worker>, Without<crate::dying::Dying>),
    >,
) {
    for (e, v) in &idle {
        // A woodcutter mustered deep in the woods at dusk gets posted back HOME (its courtyard
        // spot), not where it stands — otherwise the militia ends up scattered through the forest.
        let post = if v.pos.length() > 26.0 { v.home } else { v.pos };
        // Shed any stale tree/boulder job (e.g. the plot collapsed mid-work) before taking up arms.
        commands
            .entity(e)
            .try_remove::<crate::lumberjack::ChopJob>()
            .try_remove::<crate::miner::MineJob>();
        arm_as_guard(&mut commands, e, post);
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
    creature_mats: &mut Assets<crate::creature::CreatureMaterial>,
    seed: u32,
) {
    let mat = crate::creature::make_creature_material(creature_mats);
    let mut rng = seed | 1;
    let half = crate::castle::courtyard_half();
    let home = courtyard_spot(&mut rng, half, &[]).unwrap_or(Vec2::new(0.0, 5.0));
    let kind = Kind::Guard { skin: SKIN[(seed as usize) % SKIN.len()], tunic: TUNIC[1] };
    spawn(commands, meshes, &mat, kind, home, home, 2.7, 1.4, SCALE, next_u32(&mut rng), false); // peasant base walk a touch quicker (was 2.3)
}

/// Spawn a **rival** soldier body for `rival.rs` — a helmeted, sword-bearing guard biped in the
/// rival's **desert** garb (sandy-ochre tunic, NOT the player militia's colours), anchored at `home`,
/// spawned at `pos`. It reuses the guard MODEL (`Kind::Guard` → `PeasantKind::Guard`) but is NOT a
/// town guard: the town-pool identity `spawn` attaches (`Guard`/`NpcHp`/`Townsfolk`) is stripped
/// here, so none of the player's town/militia systems touch it. The caller (`rival.rs`) adds the
/// `RivalSoldier` marker, `Health`, and its own combat brain (which drives the `Villager` pose fields
/// → `villager_drive`/`animate_biped`).
pub fn spawn_rival_soldier(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    creature_mats: &mut Assets<crate::creature::CreatureMaterial>,
    home: Vec2,
    pos: Vec2,
    seed: u32,
) -> Entity {
    let mat = crate::creature::make_creature_material(creature_mats);
    // Desert ochre tunic so the rival's men read instantly as "not ours" — the distinction the
    // textured-sandstone fort no longer carries on its own.
    let kind = Kind::Guard { skin: SKIN[(seed as usize) % SKIN.len()], tunic: 0xbf9a55 }; // desert garb
    let root = spawn(commands, meshes, &mat, kind, home, pos, 2.6, 2.0, SCALE, seed, true); // desert = keffiyeh + cloak
    commands.entity(root).remove::<(Guard, NpcHp, Townsfolk)>();
    root
}

/// Spawn a **rival** worker body for `rival.rs` — a desert-garbed tradesman (farmer / woodcutter /
/// miner) in the same keffiyeh + cloak as the soldiers, carrying their trade's tool. Reuses the town
/// worker MODEL (`Kind::Worker` → the studio peasant) but, like the soldier, gains NONE of the
/// player's town/militia components (`Kind::Worker` adds none in `spawn`). The caller (`rival.rs`)
/// adds its own `RivalWorker` marker + cosmetic brain. `home`/`pos` anchor it near its building.
pub fn spawn_rival_worker(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    creature_mats: &mut Assets<crate::creature::CreatureMaterial>,
    trade: Trade,
    home: Vec2,
    pos: Vec2,
    seed: u32,
) -> Entity {
    let mat = crate::creature::make_creature_material(creature_mats);
    let kind = Kind::Worker { trade, skin: SKIN[(seed as usize) % SKIN.len()], tunic: 0xbf9a55 }; // desert garb
    spawn(commands, meshes, &mat, kind, home, pos, 2.2, 1.6, SCALE, seed, true)
}

/// Spawn a plain **peasant** for a staged trailer scene at `pos`/`facing`, tagged
/// [`crate::scenes::SceneActor`] so the wander brain leaves it alone (the scene poses it). Returns
/// the root so the caller can drive it. `held` picks a baked tool (None / a farmer's hoe, etc.).
pub fn spawn_scene_peasant(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    creature_mats: &mut Assets<crate::creature::CreatureMaterial>,
    pos: Vec2,
    facing: f32,
    worker: Option<Trade>,
    seed: u32,
) -> Entity {
    let mat = crate::creature::make_creature_material(creature_mats);
    let mut rng = seed | 1;
    let skin = SKIN[(seed as usize) % SKIN.len()];
    let tunic = TUNIC[(seed as usize >> 3) % TUNIC.len()];
    let kind = match worker {
        Some(trade) => Kind::Worker { trade, skin, tunic },
        None => Kind::Peasant { skin, tunic, hat: seed & 1 == 0 },
    };
    let e = spawn(commands, meshes, &mat, kind, pos, pos, 1.4, 1.0, SCALE, next_u32(&mut rng), false);
    commands
        .entity(e)
        .insert(crate::scenes::SceneActor)
        .insert(Transform {
            translation: Vec3::new(pos.x, worldmap::ground_at_world(pos.x, pos.y).unwrap_or(0.0), pos.y),
            rotation: Quat::from_rotation_y(facing),
            scale: Vec3::splat(SCALE),
        });
    e
}

/// Clear a camp's warband and its captives are **automatically** freed (the TS behaviour): a cage
/// of [`CAMP_RESCUE_POP`] joins the castle as militia (new guards) and grows the bloodline, with a
/// float over the cage so you see it happen. `seen` gates against freeing a camp before its orks
/// have even spawned.
#[allow(clippy::too_many_arguments)]
fn camp_rescue(
    mut town: ResMut<crate::town::TownRes>,
    mut rescued: ResMut<RescuedCamps>,
    orks: Query<&crate::orks::Ork, Without<crate::orks::WaveInvader>>,
    cages_q: Query<(Entity, &crate::camps::Cage, &Transform)>,
    mut floats: ResMut<crate::combat_fx::FloatQueue>,
    mut cues: MessageWriter<crate::audio::AudioCue>,
    mut speak: MessageWriter<crate::audio::Speak>,
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
        // The freed captives join the town's population (guards appear in the courtyard via
        // `sync_population_bodies`) — and the bloodline with them: heirs ARE the headcount.
        town.0.population += CAMP_RESCUE_POP;
        floats.0.push(crate::combat_fx::FloatReq {
            world: Vec3::new(cage.x, y + 1.8, cage.y),
            text: format!("Captives freed!  +{CAMP_RESCUE_POP} townsfolk"),
            color: Color::srgb(0.5, 1.0, 0.6),
            scale: 1.2,
        });
        cues.write(crate::audio::AudioCue::CampRescue);
        speak.write(crate::audio::Speak::new(crate::audio::Concept::FirstRescue));
    }
}

/// **R** inside the castle spends a Mercenary Contract (from chests) to hire a sellsword — a new
/// townsperson (a guard appears via `sync_population_bodies`), which is also a new heir.
fn recruit(
    keys: Res<ButtonInput<KeyCode>>,
    hero: Res<crate::player::HeroState>,
    mut inv: ResMut<crate::inventory::Inventory>,
    mut town: ResMut<crate::town::TownRes>,
    mut floats: ResMut<crate::combat_fx::FloatQueue>,
) {
    if !(keys.just_pressed(KeyCode::KeyR) && hero.alive && crate::castle::in_footprint(hero.pos.x, hero.pos.y)) {
        return;
    }
    // Refuse when housing is full — population over `pop_cap` just starves back off (see
    // `population_tick`), so spending a contract there silently wastes it. Tell the player to build.
    if town.0.population >= town.0.pop_cap() {
        if inv.0.has_item("mercenary_contract") {
            floats.0.push(crate::combat_fx::FloatReq {
                world: Vec3::new(hero.pos.x, hero.y + 2.0, hero.pos.y),
                text: "No room — build a house first".into(),
                color: Color::srgb(1.0, 0.7, 0.4),
                scale: 1.0,
            });
        }
        return;
    }
    if inv.0.consume_item("mercenary_contract", 1) {
        town.0.population += 1;
    }
}

/// Ambient townsfolk wander — guards (their AI is [`guard_combat`]) and pilgrims (their AI is
/// [`pilgrim_brain`]) are excluded.
#[allow(clippy::type_complexity)]
fn villager_brain(
    time: Res<Time>,
    spots: Res<TownSpots>,
    mut q: Query<
        (&mut Villager, &mut Transform, Has<Kid>),
        (
            Without<Guard>,
            Without<Pilgrim>,
            Without<crate::town::Worker>,
            Without<FightBack>,
            Without<crate::lumberjack::Fleeing>,
            Without<crate::dying::Dying>,
            // Staged-scene villagers (trailer tableaus) are posed by `scenes.rs`, not the brain.
            Without<crate::scenes::SceneActor>,
            // Rival soldiers + workers carry `Villager` (for locomotion/anim) but are driven by
            // `rival.rs`'s own brains, not the town's ambient wander.
            Without<crate::rival::RivalSoldier>,
            Without<crate::rival::RivalWorker>,
        ),
    >,
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
                    let cur_y = crate::steer::footing(v.pos.x, v.pos.y).unwrap_or(tf.translation.y);
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

        let gy = crate::steer::footing(v.pos.x, v.pos.y).unwrap_or(tf.translation.y);
        let bob = if v.moving { (tw * v.gait + v.phase).sin().abs() * v.bob } else { 0.0 };
        tf.translation = Vec3::new(v.pos.x, gy + bob, v.pos.y);
        tf.rotation = Quat::from_rotation_y(v.facing);
    }
}

/// Steer assigned workers to their building, then hold post (sets `at_post`).
/// Lives here because it pokes the private `Villager` fields. Workers inherit
/// `townsfolk_curfew` (no `Guard`), so they flee at night automatically.
///
/// Every build plot sits OUTSIDE the wall footprint and off the gate lanes (see
/// `town::PLOT_OFFSETS`), so once the walls are up a straight line from a courtyard
/// home to a post always crosses a wall — the local steer fan can't find a gate
/// ~15 units away. Far legs therefore follow a cached A* `NavPath` through the
/// gates, exactly like the guard post-march in `guard_combat`.
#[allow(clippy::type_complexity)]
fn worker_steer(
    time: Res<Time>,
    spots: Res<crate::town::PlotSpots>,
    town: Res<crate::town::TownRes>,
    mut q: Query<
        (Entity, &mut crate::town::Worker, &mut Villager, &mut Transform, &mut crate::navgrid::NavPath),
        (
            Without<crate::lumberjack::ChopJob>,
            Without<crate::lumberjack::Hauling>,
            Without<crate::miner::MineJob>,
            Without<crate::miner::Carting>,
            Without<crate::lumberjack::Fleeing>,
            Without<FightBack>,
            Without<crate::dying::Dying>,
        ),
    >,
) {
    let dt = time.delta_secs().min(0.05);
    let tw = time.elapsed_secs_wrapped();
    let now = time.elapsed_secs();
    for (self_e, mut worker, mut v, mut tf, mut path) in &mut q {
        let Some(post) = spots.0.get(worker.idx).copied() else { continue };
        // A farmer works ON the tilled field, which `town_meshes::farm_parts` always lays on the
        // +X side of the plot (the barn is −X). So a farmer's work spot is offset onto the field and
        // they face back across the rows toward the barn — never standing off-plot, back to the farm.
        // (Woodcutters/miners leave for trees/rocks, so their plot-centre post is fine.)
        let is_farm = town.0.plots.get(worker.idx).and_then(|p| p.kind)
            == Some(tileworld_core::town_store::BuildKind::Farm);
        let work_pos = if is_farm { post + Vec2::new(1.1, 0.0) } else { post };
        let reach = if is_farm { 0.8 } else { 1.6 };
        let to = work_pos - v.pos;
        let dist = to.length();
        if dist < reach {
            worker.at_post = true;
            v.moving = false;
            if is_farm {
                // Face back across the field toward the barn so the hoeing reads over the crop rows.
                let f = post - v.pos;
                if f.length_squared() > 1e-4 {
                    v.facing = f.x.atan2(f.y);
                }
            } else if to.length_squared() > 1e-4 {
                // Turn to face the building/field so the work stroke reads.
                v.facing = to.x.atan2(to.y);
            }
        } else {
            worker.at_post = false;
            v.target = work_pos;
            // Far from the post: follow the A* route (threads the wall gates). Close in:
            // cheap direct steer, no pathing churn — same split as the guard post-march.
            let step_target = if dist > GUARD_PATH_RANGE {
                if path.cursor >= path.waypoints.len()
                    || now >= path.next_replan
                    || path.goal_cached.distance(work_pos) > 2.0
                {
                    path.waypoints = crate::navgrid::path_to(v.pos, work_pos);
                    path.cursor = 0;
                    path.goal_cached = work_pos;
                    // Stagger replans so a dawn shift-change doesn't path everyone on one frame.
                    path.next_replan = now + 0.75 + (self_e.to_bits() % 16) as f32 * 0.05;
                }
                while path.cursor < path.waypoints.len()
                    && v.pos.distance(path.waypoints[path.cursor]) < 1.2
                {
                    path.cursor += 1;
                }
                path.waypoints.get(path.cursor).copied().unwrap_or(work_pos)
            } else {
                path.waypoints.clear();
                path.cursor = 0;
                work_pos
            };
            let cur_y = crate::steer::footing(v.pos.x, v.pos.y).unwrap_or(tf.translation.y);
            if let Some(s) = steer::advance(v.pos, v.facing, step_target, v.speed * dt, v.body_r, cur_y, VIL_MAX_TURN * dt) {
                v.facing = s.facing;
                v.pos = s.pos;
                v.moving = s.moving;
            }
        }
        let gy = crate::steer::footing(v.pos.x, v.pos.y).unwrap_or(tf.translation.y);
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
        (
            Without<Guard>,
            Without<crate::landmarks::Landmark>,
            Without<crate::town::Worker>,
            Without<crate::dying::Dying>,
        ),
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
            let cur_y = crate::steer::footing(v.pos.x, v.pos.y).unwrap_or(tf.translation.y);
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

        let gy = crate::steer::footing(v.pos.x, v.pos.y).unwrap_or(tf.translation.y);
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

/// Drain the frame's NPC hits (ork blades via `siege.rs`, predator bites via `wildlife.rs`)
/// into townsfolk HP. Death is **permanent**: the body crumples (`dying.rs`) and the town's
/// population drops by one — the food surplus regrows a replacement by day
/// (`town::population_system`); nobody rises at dawn. A surviving *passive* townsperson (no
/// [`Guard`] role) rounds on its attacker ([`FightBack`]): villagers always defend themselves.
#[allow(clippy::type_complexity)]
fn npc_damage_apply(
    time: Res<Time>,
    mut incoming: ResMut<NpcDamage>,
    mut town: ResMut<crate::town::TownRes>,
    mut floats: ResMut<crate::combat_fx::FloatQueue>,
    mut commands: Commands,
    mut q: Query<(&mut NpcHp, &Transform, Has<Guard>), Without<crate::dying::Dying>>,
) {
    let now = time.elapsed_secs();
    for hit in incoming.0.drain(..) {
        let Ok((mut hp, tf, is_guard)) = q.get_mut(hit.victim) else { continue };
        if hp.hp <= 0.0 {
            continue; // already mortally struck this frame
        }
        hp.hp -= hit.amount;
        if hp.hp <= 0.0 {
            crate::dying::begin_dying(&mut commands, hit.victim, now);
            // The headcount is the source of truth — dropping it keeps `sync_population_bodies`
            // from spawning an instant free replacement; growth has to re-earn the body.
            town.0.population = town.0.population.saturating_sub(1);
            floats.0.push(crate::combat_fx::FloatReq {
                world: tf.translation + Vec3::Y * 2.2,
                text: "A townsperson has fallen!".into(),
                color: Color::srgb(1.0, 0.45, 0.35),
                scale: 1.2,
            });
        } else if !is_guard {
            if let Some(att) = hit.attacker {
                // "Always defends": a struck worker stops fleeing/working and turns on its foe.
                commands
                    .entity(hit.victim)
                    .try_remove::<crate::lumberjack::Fleeing>()
                    .try_insert(FightBack { target: att, atk_cd: 0.0 });
            }
        }
    }
}

/// Passive self-defence: a [`FightBack`] townsperson squares up to its attacker and trades weak
/// blows until the attacker is dead, it dies, or the attacker breaks off — then back to work.
#[allow(clippy::type_complexity)]
fn npc_fight_back(
    time: Res<Time>,
    hero: Query<&crate::player::Hero>,
    mut commands: Commands,
    mut cues: MessageWriter<crate::audio::AudioCue>,
    mut kills: MessageWriter<crate::verbs::AnimalKilled>,
    mut npcs: Query<
        (Entity, &mut FightBack, &mut Villager, &mut Transform),
        (Without<Guard>, Without<crate::dying::Dying>),
    >,
    mut hostiles: Query<
        (&Transform, &mut crate::player::Health, Option<&crate::wildlife::Animal>),
        (
            Or<(With<crate::orks::Ork>, With<crate::wildlife::Animal>)>,
            Without<Villager>,
            Without<crate::dying::Dying>,
        ),
    >,
) {
    let dt = time.delta_secs().min(0.05);
    let tw = time.elapsed_secs_wrapped();
    let now = time.elapsed_secs();
    let hero_pos = hero.single().ok().map(|h| h.pos);
    for (self_e, mut fb, mut v, mut tf) in &mut npcs {
        let Ok((ttf, mut thp, animal)) = hostiles.get_mut(fb.target) else {
            commands.entity(self_e).try_remove::<FightBack>(); // foe gone (dead/despawned)
            continue;
        };
        let tp = Vec2::new(ttf.translation.x, ttf.translation.z);
        let d = v.pos.distance(tp);
        if d > NPC_GIVE_UP {
            commands.entity(self_e).try_remove::<FightBack>(); // it broke off — let it go
            continue;
        }
        fb.atk_cd -= dt;
        if d < NPC_MELEE {
            v.moving = false;
            let to = tp - v.pos;
            if to.length_squared() > 1e-4 {
                let want = to.x.atan2(to.y);
                v.facing += steer::wrap_pi(want - v.facing).clamp(-VIL_MAX_TURN * 2.0 * dt, VIL_MAX_TURN * 2.0 * dt);
            }
            if fb.atk_cd <= 0.0 {
                fb.atk_cd = NPC_DEFEND_CD;
                v.atk_anim = now; // fire the tool-swing (read by villager_limbs)
                thp.hp -= NPC_DEFEND_DMG;
                if thp.hp <= 0.0 {
                    crate::dying::begin_dying(&mut commands, fb.target, now);
                    if let Some(a) = animal {
                        // Feed the loot/respawn pipeline like any kill.
                        kills.write(crate::verbs::AnimalKilled { at: ttf.translation, species: a.species });
                    }
                } else if animal.is_some() {
                    // The beast snaps back at whoever is poking it.
                    commands.entity(fb.target).try_insert(crate::wildlife::Struck { by: Some(self_e) });
                }
                if hero_pos.is_some_and(|hp| v.pos.distance(hp) < GUARD_SFX_RADIUS) {
                    let at = Vec3::new(v.pos.x, tf.translation.y + 1.0, v.pos.y);
                    cues.write(crate::audio::AudioCue::GuardStrike(at));
                }
            }
        } else {
            // Close the gap (a touch faster than the work amble — adrenaline).
            let cur_y = crate::steer::footing(v.pos.x, v.pos.y).unwrap_or(tf.translation.y);
            match steer::advance(v.pos, v.facing, tp, v.speed * 1.25 * dt, v.body_r, cur_y, VIL_MAX_TURN * 2.0 * dt) {
                Some(s) => {
                    v.facing = s.facing;
                    v.pos = s.pos;
                    v.moving = s.moving;
                }
                None => v.moving = false,
            }
        }
        let gy = crate::steer::footing(v.pos.x, v.pos.y).unwrap_or(tf.translation.y);
        let bob = if v.moving { (tw * v.gait + v.phase).sin().abs() * v.bob } else { 0.0 };
        tf.translation = Vec3::new(v.pos.x, gy + bob, v.pos.y);
        tf.rotation = Quat::from_rotation_y(v.facing);
    }
}

/// Keep every militia member's max HP in step with the guard-vigor upgrades. The target is
/// `NPC_MAX_HP + guard_hp_bonus`; a townsperson below it (a fresh spawn at the base, or anyone when
/// a `GuardHealth` node is just bought) is raised, and the freshly granted HP is healed in so the
/// upgrade is felt at once. Only ever raises `max` — a wounded guard keeps its current `hp`.
fn guard_arms_upkeep(
    def: Res<crate::economy::Defenses>,
    siege: Res<crate::siege::Siege>,
    mut q: Query<&mut NpcHp, Without<crate::dying::Dying>>,
) {
    let target = NPC_MAX_HP + def.guard_hp_bonus + night_hp_bonus(siege.wave_index);
    for mut hp in &mut q {
        if hp.max < target {
            let gained = target - hp.max;
            hp.max = target;
            hp.hp += gained;
        }
    }
}

/// **K** — Call the Muster. Toggles the war party: if anyone is already rallied, stand the whole
/// party down (restore each to its real post); otherwise the **entire standing town pool** falls in
/// — workers down tools (production pauses) and existing guards leave their posts, all to follow the
/// hero. `auto_assign_workers` skips [`Rallied`] guards, so they stay fallen-in until stood down.
/// Stateless (the toggle reads the live [`Rallied`] set, not a flag), so it self-corrects after a
/// load and needs no reset path. Play-only via the `Modal::None` gate. The `fresh` (`Without<Rallied>`)
/// and `rallied` (`With<Rallied>`) queries are archetype-disjoint, so the borrow checker is happy.
#[allow(clippy::type_complexity)]
fn muster_keys(
    keys: Res<ButtonInput<KeyCode>>,
    hero: Res<crate::player::HeroState>,
    mut commands: Commands,
    mut floats: ResMut<crate::combat_fx::FloatQueue>,
    mut speak: MessageWriter<crate::audio::Speak>,
    fresh: Query<
        (Entity, Option<&Guard>, &Villager, Has<crate::town::Worker>),
        (With<Townsfolk>, Without<Rallied>, Without<crate::dying::Dying>),
    >,
    mut rallied: Query<(Entity, &Rallied, &mut Guard)>,
) {
    if !(keys.just_pressed(KeyCode::KeyK) && hero.alive) {
        return;
    }
    let (text, color) = if rallied.is_empty() {
        // Fall in: the whole pool — guards and (down-tooled) workers alike — joins in slot order.
        for (slot, (e, guard, v, is_worker)) in fresh.iter().enumerate() {
            rally_one(&mut commands, e, guard, v, is_worker, slot);
        }
        // Hero barks a rally order (one of the MusterCall variants; ~20s hero-line spacing throttles spam).
        speak.write(crate::audio::Speak::new(crate::audio::Concept::MusterCall));
        ("To me! Form up!", Color::srgb(0.72, 0.9, 1.0))
    } else {
        // Stand down: restore each guard's real post, then drop the marker — guard_combat walks
        // each back home (the post is no longer hero-tracked) and the day auto-assign re-employs it.
        for (e, r, mut g) in &mut rallied {
            g.post = r.home;
            commands.entity(e).try_remove::<Rallied>();
        }
        speak.write(crate::audio::Speak::new(crate::audio::Concept::MusterStandDown));
        ("Stand down.", Color::srgb(0.92, 0.85, 0.68))
    };
    floats.0.push(crate::combat_fx::FloatReq {
        world: Vec3::new(hero.pos.x, hero.y + 2.2, hero.pos.y),
        text: text.into(),
        color,
        scale: 1.1,
    });
}

/// On every Continue/Load (`GameLoaded`), disband any war party the dead run left rallied: drop the
/// [`Rallied`] marker and restore each guard's real `home` post. Townsfolk bodies are persistent
/// across an in-process load (only resources reset), so without this a save loaded mid-muster would
/// boot already-rallied with a stale home — and `auto_assign_workers` (which skips `Rallied`) would
/// then refuse to employ those guards, stalling town production until the player toggled `K` twice.
/// Fires once per load on EVERY Continue path, mirroring [`crate::inventory`]'s transient-buff sweep.
fn disband_on_load(
    mut ev: MessageReader<crate::savegame::GameLoaded>,
    mut commands: Commands,
    mut rallied: Query<(Entity, &Rallied, &mut Guard)>,
) {
    if ev.read().last().is_none() {
        return;
    }
    for (e, r, mut g) in &mut rallied {
        g.post = r.home;
        commands.entity(e).try_remove::<Rallied>();
    }
}

/// Keep each [`Rallied`] guard's `post` pinned to its slot in the blob around the hero, so
/// [`guard_combat`] (which follows the post and peels to fight foes near it) makes the war party
/// trail and guard him. Runs before `guard_combat` so the post is fresh that frame.
fn rally_follow(
    hero: Res<crate::player::HeroState>,
    mut guards: Query<(&Rallied, &mut Guard), Without<crate::dying::Dying>>,
) {
    if !hero.alive {
        return;
    }
    for (r, mut g) in &mut guards {
        let s = r.slot as f32;
        let radius = RALLY_BASE_R + RALLY_SPREAD * s.sqrt();
        let a = s * RALLY_GOLDEN;
        g.post = hero.pos + Vec2::new(a.cos() * radius, a.sin() * radius);
    }
}

/// Screenshot hook: `FOREST_MUSTER=1` rallies the town guard to the hero at boot (same as pressing
/// `K`), so the capture harness's warmup frames let the war party gather into its blob and a single
/// shot shows the muster. Pairs with `FOREST_TOWN` to stage a full town first. It re-tags any
/// not-yet-rallied guard every frame (idempotent — `Without<Rallied>`) rather than firing once, so
/// it also catches the bodies that `sync_population_bodies` spawns over the following frames. The
/// env read is cached, so the unset case costs nothing after frame one. Slot index continues from
/// the count already rallied so late arrivals extend the blob instead of overlapping it.
#[allow(clippy::type_complexity)]
fn stage_muster(
    mut armed: Local<Option<bool>>,
    mut commands: Commands,
    already: Query<(), With<Rallied>>,
    fresh: Query<
        (Entity, Option<&Guard>, &Villager, Has<crate::town::Worker>),
        (With<Townsfolk>, Without<Rallied>, Without<crate::dying::Dying>),
    >,
) {
    if !*armed.get_or_insert_with(|| std::env::var("FOREST_MUSTER").is_ok()) || fresh.is_empty() {
        return;
    }
    let base = already.iter().count();
    let mut n = 0;
    for (i, (e, guard, v, is_worker)) in fresh.iter().enumerate() {
        rally_one(&mut commands, e, guard, v, is_worker, base + i);
        n = i + 1;
    }
    info!("FOREST_MUSTER: rallied {n} more ({} total)", base + n);
}

/// Town-guard AI. During a wave: chase the nearest invader inside the big hunt radius and trade
/// blows in melee. In peacetime: mend slowly, and sally out after any hostile — an ork **or a
/// predator** (wolf, bear, golem…) — that prowls within [`GUARD_DETECT`] of the post, chasing it
/// only while it stays inside [`GUARD_LEASH`]; after the kill (or the leash) walk back to post.
#[allow(clippy::type_complexity)]
fn guard_combat(
    time: Res<Time>,
    siege: Res<crate::siege::Siege>,
    def: Res<crate::economy::Defenses>,
    mut commands: Commands,
    mut cues: MessageWriter<crate::audio::AudioCue>,
    mut kills: MessageWriter<crate::verbs::AnimalKilled>,
    hero: Query<&crate::player::Hero>,
    mut guards: Query<
        (Entity, &mut Guard, &mut NpcHp, &mut Villager, &mut Transform, &mut crate::navgrid::NavPath, Has<Rallied>),
        Without<crate::dying::Dying>,
    >,
    mut hostiles: Query<
        (
            Entity,
            &Transform,
            &mut crate::player::Health,
            Has<crate::orks::WaveInvader>,
            Option<&crate::wildlife::Animal>,
        ),
        (
            // The Warlord joins the set so a mustered war party can pile onto the boss — he carries
            // neither `Ork` nor `Animal` (a standalone `Warlord`+`Health` entity), so without this
            // he'd be invisible to guard targeting. Only RALLIED guards ever reach him (he stands far
            // off in the Hold, past every non-rallied detect/hunt ring), so this doesn't pull the
            // standing militia south.
            // Rival soldiers join the hostile set so the player's militia (and a mustered war party
            // taken to the desert) fights them like any other foe — they carry `Health` but neither
            // `Ork` nor `Animal`.
            Or<(With<crate::orks::Ork>, With<crate::wildlife::Animal>, With<crate::warlord::Warlord>, With<crate::rival::RivalSoldier>)>,
            Without<Guard>,
            Without<crate::dying::Dying>,
        ),
    >,
) {
    let dt = time.delta_secs().min(0.05);
    let tw = time.elapsed_secs_wrapped();
    let now = time.elapsed_secs();
    let in_wave = siege.phase == crate::siege::GamePhase::Wave;
    // Hero (≈ listener) position, for the small-earshot gate on guard clash SFX.
    let hero_pos = hero.single().ok().map(|h| h.pos);
    // Effective strike for the town's current arms tier (Defense "Guard Arms" upgrades).
    let guard_dmg = guard_damage(def.villager_arms_tier);
    // Each arms tier also widens the watch (the upgrade descs promise "chase from farther" /
    // "wider watch"): +3u detect, +4u leash per tier. Previously unwired — only damage scaled.
    let tier = def.villager_arms_tier as f32;
    let guard_detect = GUARD_DETECT + tier * 3.0;
    let guard_leash = GUARD_LEASH + tier * 4.0;
    // Everything a guard may engage: every ork, plus the predator species only — the militia
    // doesn't slaughter the deer herds.
    let inv: Vec<(Entity, Vec2, bool)> = hostiles
        .iter()
        .filter_map(|(e, tf, _, invader, animal)| {
            let hostile = match animal {
                Some(a) => crate::wildlife::is_hostile_species(a.species),
                None => true, // any ork
            };
            hostile.then_some((e, Vec2::new(tf.translation.x, tf.translation.z), invader))
        })
        .collect();
    let mut dealt: Vec<(Entity, Entity, f32)> = Vec::new(); // (target, guard, dmg)

    for (self_e, mut g, mut hp, mut v, mut tf, mut path, rallied) in &mut guards {
        g.atk_cd -= dt;
        if !in_wave {
            // Peacetime mend — slow, so a mauling leaves a mark (no more instant dawn heal).
            hp.hp = (hp.hp + GUARD_REGEN * dt).min(hp.max);
        }
        // Pick a target. At night: the nearest invader anywhere in the defended area. By day:
        // the nearest hostile near the POST (engage inside the detect ring, finish a fight it's
        // already toe-to-toe with anywhere inside the leash).
        let mut best: Option<(Entity, Vec2, f32)> = None;
        for (e, p, invader) in &inv {
            let d = v.pos.distance(*p);
            if rallied {
                // War party (mustered with `K`): fight WHATEVER is near the hero — a wave ork, the
                // woken Hold garrison, or the Warlord himself — with no invader-only filter and no
                // post leash. The post tracks the hero (`rally_follow`), so gating on distance from
                // the guard keeps the blob piling onto the hero's fight, boss included.
                if d >= RALLY_ENGAGE_RADIUS {
                    continue;
                }
            } else if in_wave {
                if !*invader || d >= GUARD_HUNT_RADIUS {
                    continue;
                }
            } else {
                let dp = p.distance(g.post);
                let engaged = d < GUARD_MELEE * 2.0;
                if dp > if engaged { guard_leash } else { guard_detect } {
                    continue;
                }
            }
            if best.is_none_or(|(_, _, bd)| d < bd) {
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
                    v.atk_anim = now; // fire the sword-swing (read by villager_limbs)
                    dealt.push((te, self_e, guard_dmg));
                    // Clash SFX, but only when the fight is close to the hero (small earshot).
                    if hero_pos.is_some_and(|hp| v.pos.distance(hp) < GUARD_SFX_RADIUS) {
                        let at = Vec3::new(v.pos.x, tf.translation.y + 1.0, v.pos.y);
                        cues.write(crate::audio::AudioCue::GuardStrike(at));
                    }
                }
            } else {
                // Far from the foe → A* toward it (thread walls/gates instead of wedging);
                // close → cheap direct steer. Same pattern as the return-to-post walk.
                let step_target = if d > GUARD_PATH_RANGE {
                    // Replan when the path runs out, OR when the goal has drifted >2u AND this
                    // guard's stagger window has elapsed. The stagger must gate the goal-moved
                    // case too (AND, not OR): a moving target — a fleeing foe, or the hero's ring
                    // post under a rallied muster — would otherwise re-fire every frame and, with
                    // the whole war party crossing the 2u threshold together, cluster island-scale
                    // A* onto the same frames (the "go after me" perf spike). Cursor-exhaust stays
                    // an always-allowed replan; it's naturally spread by per-guard walk progress.
                    if path.cursor >= path.waypoints.len()
                        || (now >= path.next_replan && path.goal_cached.distance(tp) > 2.0)
                    {
                        path.waypoints = crate::navgrid::path_to(v.pos, tp);
                        path.cursor = 0;
                        path.goal_cached = tp;
                        path.next_replan = now + 0.5 + (self_e.to_bits() % 16) as f32 * 0.04;
                    }
                    while path.cursor < path.waypoints.len()
                        && v.pos.distance(path.waypoints[path.cursor]) < 1.2
                    {
                        path.cursor += 1;
                    }
                    path.waypoints.get(path.cursor).copied().unwrap_or(tp)
                } else {
                    path.waypoints.clear();
                    path.cursor = 0;
                    tp
                };
                let cur_y = crate::steer::footing(v.pos.x, v.pos.y).unwrap_or(tf.translation.y);
                if let Some(s) = steer::advance(v.pos, v.facing, step_target, GUARD_SPEED * dt, v.body_r, cur_y, VIL_MAX_TURN * 2.0 * dt) {
                    v.facing = s.facing;
                    v.pos = s.pos;
                    v.moving = s.moving;
                } else {
                    v.moving = false;
                }
            }
        } else {
            // No foe — amble back to the post.
            let to_post = g.post - v.pos;
            if to_post.length() > 0.4 {
                // Far from home (a freed captive marching in from a razed camp, or a guard back
                // from a chase) → follow an A* route to the courtyard post so it threads the
                // river crossing and the castle GATE instead of wedging on the wall. Near home →
                // cheap direct steer, no pathing churn. Mirrors the invader keep-march in `siege.rs`.
                let step_target = if to_post.length() > GUARD_PATH_RANGE {
                    // Goal-moved replan is AND-gated by the stagger (see the chase branch above):
                    // a rallied guard's post tracks the running hero every frame, so an OR here
                    // re-pathed the whole muster on the same frames → spikes. A fixed post (a freed
                    // captive marching home) never trips goal-moved, so it replans only on
                    // cursor-exhaust — unchanged from before.
                    if path.cursor >= path.waypoints.len()
                        || (now >= path.next_replan && path.goal_cached.distance(g.post) > 2.0)
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
                let cur_y = crate::steer::footing(v.pos.x, v.pos.y).unwrap_or(tf.translation.y);
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
        }

        // Ground-follow (guards own their full transform since they're out of villager_brain).
        let gy = crate::steer::footing(v.pos.x, v.pos.y).unwrap_or(tf.translation.y);
        let bob = if v.moving { (tw * v.gait + v.phase).sin().abs() * v.bob } else { 0.0 };
        tf.translation = Vec3::new(v.pos.x, gy + bob, v.pos.y);
        tf.rotation = Quat::from_rotation_y(v.facing);
    }

    // Apply guard strikes to hostile Health; reap the slain, enrage struck beasts.
    for (e, guard_e, dmg) in dealt {
        if let Ok((_, ttf, mut hp, _, animal)) = hostiles.get_mut(e) {
            if hp.hp > 0.0 {
                hp.hp -= dmg;
                if hp.hp <= 0.0 {
                    crate::dying::begin_dying(&mut commands, e, time.elapsed_secs());
                    if let Some(a) = animal {
                        // Feed the loot/respawn pipeline like any kill.
                        kills.write(crate::verbs::AnimalKilled { at: ttf.translation, species: a.species });
                    }
                } else if animal.is_some() {
                    // Struck-enrage, aimed at the guard that landed the blow.
                    commands.entity(e).try_insert(crate::wildlife::Struck { by: Some(guard_e) });
                }
            }
        }
    }
}

/// Squared camera distance past which limb animation is skipped (fog/DoF hide the joints).
const LIMB_CULL2: f32 = 70.0 * 70.0;

/// How long a townsperson's weapon-swing plays after a strike is stamped onto [`Villager::atk_anim`].
const ATTACK_ANIM_DUR: f32 = 0.42;

/// Strike progress `0..1` since `atk_anim`, or `None` when not currently swinging. The townsfolk
/// mirror of the ork rig's `strike_p`.
fn strike_p(atk_anim: f32, now: f32) -> Option<f32> {
    if atk_anim <= 0.0 {
        return None;
    }
    let p = (now - atk_anim) / ATTACK_ANIM_DUR;
    (0.0..1.0).contains(&p).then_some(p)
}

/// Overhead weapon swing on X: raise back (ease-in), fast slash down (ease-out), recover to rest.
/// Same crisp shape as the ork club-chop, so a guard's sword (or a worker's hoe/axe) reads as a
/// real strike on the box-mesh arm.
fn swing_x(p: f32) -> f32 {
    if p < 0.3 {
        let u = p / 0.3;
        -1.5 * (u * u)
    } else if p < 0.55 {
        let u = (p - 0.3) / 0.25;
        let e = 1.0 - (1.0 - u) * (1.0 - u);
        -1.5 + 2.4 * e
    } else {
        let u = (p - 0.55) / 0.45;
        let e = 1.0 - (1.0 - u) * (1.0 - u);
        0.9 * (1.0 - e)
    }
}

/// Idle villagers turn their head to watch the hero when he passes within this range (world u).
const GREET_DIST: f32 = 8.0;
/// Max head yaw off forward while watching the hero (radians) — a turn of the head, not an owl spin.
const HEAD_TURN_MAX: f32 = 0.7;
/// Ease rate of the head yaw toward its target (per second) — a relaxed, lifelike turn.
const HEAD_EASE: f32 = 4.5;
/// Duration of the single welcoming head-nod when the hero walks up (seconds).
const GREET_NOD_DUR: f32 = 0.8;

/// Layered limb offsets (radians) for one idle "fidget" — small gestures a standing villager
/// cycles through so they read as a person idling, not a frozen pathing agent. All zero = rest.
#[derive(Default)]
struct Fidget {
    head_pitch: f32, // + looks down
    head_yaw: f32,   // a glance aside (on top of the look-around scan)
    head_roll: f32,  // a curious head-tilt
    arm: f32,        // free-arm raise — a shift / scratch
    lean: f32,       // weight shift onto one leg
}

/// Cheap stateless hash → `0.0..1.0`, for picking a villager's current idle gesture from the
/// gesture-clock cycle index (no RNG state to thread through the per-frame anim).
fn hash01(x: f32) -> f32 {
    let v = (x * 12.9898).sin() * 43758.5453;
    v - v.floor()
}

/// Which idle fidget a villager is mid-way through, given the global clock + its per-instance
/// `seed` (its `phase`, so neighbours don't fidget in lockstep). Each gesture eases in→peak→out
/// over ~3.3s; one slot is deliberately "stillness" so they aren't perpetually twitching.
fn idle_fidget(tw: f32, seed: f32) -> Fidget {
    let g = tw * 0.3 + seed * 1.7; // ~3.3s per gesture, phase-desynced
    let cyc = g.floor();
    let bell = ((g - cyc) * std::f32::consts::PI).sin(); // 0 → 1 → 0 across the gesture
    let mut f = Fidget::default();
    match (hash01(cyc + seed * 31.0) * 5.0) as i32 {
        1 => f.head_yaw = bell * 0.45,                                   // glance off to the side
        2 => {
            f.head_pitch = bell * 0.26;
            f.head_roll = bell * 0.13;
        } // peer down with a curious tilt
        3 => f.arm = bell * 0.55,                                        // shift / scratch the free arm
        4 => f.lean = bell * 0.12,                                       // shift weight onto one leg
        _ => {}                                                          // a beat of stillness
    }
    f
}

/// Map every townsperson's brain state onto its [`BipedDrive`] each frame — the town's mirror of
/// `orks::ork_drive`. `biped::animate_biped` then turns the drive into the studio clips (idle/walk,
/// a melee swing on a stamped `atk_anim`, a posted worker's tool stroke). Ungated so the town stays
/// animated through pauses/panels.
#[allow(clippy::type_complexity)]
fn villager_drive(
    time: Res<Time>,
    mut q: Query<
        (
            &Villager,
            Option<&crate::town::Worker>,
            Option<&Role>,
            &mut BipedDrive,
            Has<crate::lumberjack::Hauling>,
            Has<crate::miner::Carting>,
        ),
        Without<crate::dying::Dying>,
    >,
) {
    let tw = time.elapsed_secs_wrapped();
    let now = time.elapsed_secs();
    let dt = time.delta_secs();
    for (v, worker, role, mut d, hauling, carting) in &mut q {
        // Ease the idle↔gait blend so starts/stops aren't a snap.
        let target = if v.moving { 1.0 } else { 0.0 };
        d.moving_amt += (target - d.moving_amt) * (dt * 8.0).min(1.0);
        d.run_amt = 0.0; // townsfolk walk (no jog)
        d.walk_phase = (tw + v.phase) * v.gait;
        d.phase = v.phase;

        // Hauling a log / carting stone home → both arms grip the load forward (carry pose).
        d.carrying = hauling || carting;

        // A posted worker plies their trade while standing at post; walking falls through to loco.
        let working = worker.is_some_and(|w| w.at_post) && !v.moving;
        d.work = if working {
            match role {
                Some(Role::Working(Trade::Farmer)) => 1,                    // quick hoe
                Some(Role::Working(Trade::Woodcutter | Trade::Miner)) => 2, // overhead chop/pick
                _ => 0,
            }
        } else {
            0
        };

        // A live strike (stamped by guard_combat / npc_fight_back) drives an overhead swing.
        if let Some(p) = strike_p(v.atk_anim, now) {
            d.attacking = true;
            d.attack_t = p * crate::player::ATTACK_DURATION;
            d.attack_variant = 0;
        } else {
            d.attacking = false;
        }
        d.sitting = false;
    }
}

fn villager_limbs(
    time: Res<Time>,
    cam: Query<&GlobalTransform, With<Camera3d>>,
    hero: Query<&crate::player::Hero>,
    // The staged mason mime owns its WHOLE rig (scenes::drive_scene_mason) — skip it here or the
    // two writers fight over the limb transforms every frame.
    // Live townsfolk are on the studio biped (`BipedDrive` → `villager_drive`/`animate_biped`) and
    // carry no `VilPart`, so this only does real work for any box-rig villager. Excluding `BipedDrive`
    // keeps it from running the greeting/yaw/fidget pass over every biped townsperson each frame for
    // a child lookup that always misses. The mason mime owns its whole rig — skip it too.
    mut vils: Query<
        (&mut Villager, &Children, &GlobalTransform, Option<&crate::town::Worker>, Option<&Role>),
        (Without<crate::scenes::SceneMason>, Without<BipedDrive>),
    >,
    mut parts: Query<(&VilPart, &mut Transform)>,
) {
    let tw = time.elapsed_secs_wrapped();
    let now = time.elapsed_secs();
    let dt = time.delta_secs();
    let cam_p = cam.single().ok().map(|g| g.translation());
    let hero_p = hero.single().ok().map(|h| h.pos);
    for (mut v, children, gt, worker, role) in &mut vils {
        if let Some(cp) = cam_p {
            if gt.translation().distance_squared(cp) > LIMB_CULL2 {
                continue;
            }
        }
        let t = tw + v.phase;
        // A live strike (set by guard_combat / npc_fight_back) drives a weapon-swing on the arm.
        let strike = strike_p(v.atk_anim, now);
        // A posted worker plies their trade: a farmer makes quick forward-down HOE strokes; a
        // woodcutter makes slower, bigger overhead CHOPS. Both arms swing together, legs planted.
        let working = worker.is_some_and(|w| w.at_post);
        // Woodcutter chops, miner swings a pick — both make the slow overhead-down stroke.
        let chopping = matches!(role, Some(Role::Working(Trade::Woodcutter | Trade::Miner)));
        let (arm_work, nod_rate) = if chopping {
            (-0.4 + 1.3 * (0.5 - 0.5 * (t * 3.0).cos()), 3.0) // overhead → down, ~2.1s
        } else {
            (0.5 + 0.7 * (t * 4.5).sin(), 4.5) // quick hoe, ~1.4s
        };

        // ── Greeting + idle fidgets ──────────────────────────────────────────────────────
        // A standing (idle, off-work) villager turns to watch the hero pass and gives one nod as
        // he walks up — the "they noticed me" beat. While greeting, the head tracks the hero and
        // the idle fidgets stand down (they're paying attention to you).
        let idle = !v.moving && !working;
        let hero_d = hero_p.map(|hp| hp.distance(v.pos)).unwrap_or(f32::MAX);
        let greeting = idle && hero_d < GREET_DIST;
        if hero_d > GREET_DIST * 1.4 {
            v.greet_armed = true; // hero left → re-arm the nod for his next approach
        }
        if greeting && v.greet_armed && v.greet <= 0.0 {
            v.greet = 1.0; // fire one nod
            v.greet_armed = false;
        }
        v.greet = (v.greet - dt / GREET_NOD_DUR).max(0.0);
        let greet_nod = (v.greet * std::f32::consts::PI).sin() * 0.30; // a single soft down-up bob

        // Head yaw target (local): toward the hero when greeting, else the gentle look-around scan
        // while idle, else forward (walking). Eased through `head_yaw` so it turns, never snaps.
        let want_yaw = if greeting {
            let to = hero_p.unwrap() - v.pos;
            steer::wrap_pi(to.x.atan2(to.y) - v.facing).clamp(-HEAD_TURN_MAX, HEAD_TURN_MAX)
        } else if idle {
            (t * 0.7).sin() * 0.18
        } else {
            0.0
        };
        v.head_yaw += steer::wrap_pi(want_yaw - v.head_yaw) * (dt * HEAD_EASE).min(1.0);

        let fid = if idle && !greeting { idle_fidget(tw, v.phase) } else { Fidget::default() };

        for &child in children {
            let Ok((part, mut tf)) = parts.get_mut(child) else { continue };
            tf.rotation = match part.kind {
                PartKind::Leg(sign) => {
                    let s = if v.moving { (t * v.gait).sin() * v.swing } else { (t * 0.8).sin() * 0.02 };
                    Quat::from_rotation_x(sign * s + sign * fid.lean) // lean shifts weight onto one leg
                }
                PartKind::Arm(sign) => {
                    // The right arm (sign > 0) carries the weapon (sword / hoe / axe) → a strike
                    // swing overrides the work stroke and the walk sway. Other arms keep working.
                    if sign > 0.0 && strike.is_some() {
                        Quat::from_rotation_x(swing_x(strike.unwrap()))
                    } else if working {
                        // Both arms together (ignore the L/R sign) — a two-handed work stroke.
                        Quat::from_rotation_x(arm_work)
                    } else {
                        let s = if v.moving { -(t * v.gait).sin() * 0.5 } else { (t * 1.2).sin() * 0.06 };
                        // The free (left) arm carries the idle fidget (a shift / scratch); the
                        // weapon hand stays at rest.
                        let fx = if sign < 0.0 { -fid.arm } else { 0.0 };
                        Quat::from_rotation_x(sign * s + fx)
                    }
                }
                PartKind::Head => {
                    if working {
                        Quat::from_rotation_x((t * nod_rate).sin() * 0.06) // small nod toward the work
                    } else {
                        // Yaw = tracked/scanning look; pitch = greeting nod + fidget peer + a gentle
                        // idle breath (so a standing villager visibly breathes); roll = tilt.
                        let (breath, _) = crate::creature_anim::idle_micro(t);
                        let breath = if idle { breath } else { 0.0 };
                        Quat::from_rotation_y(v.head_yaw + fid.head_yaw)
                            * Quat::from_rotation_x(greet_nod + fid.head_pitch + breath)
                            * Quat::from_rotation_z(fid.head_roll)
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
    /// A townsperson posted to a producer — distinct work clothes + a held tool (hoe / axe).
    Worker { trade: Trade, skin: u32, tunic: u32 },
}

/// Which trade a working townsperson plies — drives their outfit, tool and work animation.
#[derive(Component, Clone, Copy, PartialEq, Eq, Debug)]
pub enum Trade {
    Farmer,
    Woodcutter,
    Miner,
}

/// What the right hand carries (baked into the arm so it swings with the limb).
#[derive(Clone, Copy)]
enum Held {
    None,
    Sword,
    Hoe,
    Axe,
    Pick,
}

/// The look a townsperson is currently rendered as (idle guard vs a trade), so `reskin_townsfolk`
/// only rebuilds the body when the role actually changes.
#[derive(Component, Clone, Copy, PartialEq, Eq)]
pub enum Role {
    Guard,
    Working(Trade),
}

/// A townsperson's fixed identity colours (skin + tunic), kept so a re-skin redresses the SAME
/// person in new work clothes rather than spawning a stranger.
#[derive(Component, Clone, Copy)]
pub struct Folk {
    skin: u32,
    tunic: u32,
    /// The villager's cosmetic seed — kept so `reskin_townsfolk` rebuilds the SAME face/build when
    /// the person changes job (guard ↔ worker), instead of becoming a stranger.
    seed: u32,
}

/// Marks a body sub-mesh (torso / limb / head) under a villager root, so a re-skin can despawn
/// exactly the body and rebuild it without touching other children (e.g. spatial voice clips).
#[derive(Component)]
struct VilBodyPart;

/// The villager's body material, kept on the root so a re-skin redresses it with the same handle
/// (no material churn).
#[derive(Component)]
struct BodyMat(Handle<crate::creature::CreatureMaterial>);

// Work-clothes palette.
const STRAW_HAT: u32 = 0xc9a85a;
const APRON: u32 = 0x9a7b4a;
const VEST: u32 = 0x53412a;
const CAP: u32 = 0x4a3a28;
const TOOL_WOOD: u32 = 0x8a6a40;
const TOOL_METAL: u32 = 0x9aa0aa;

struct PartDef {
    kind: PartKind,
    pivot: Vec3,
    mesh: Mesh,
}
struct VSpec {
    torso: Mesh,
    parts: Vec<PartDef>,
}

/// Build a villager body. Cosmetics (colour, face) are deterministic from `seed` via a dedicated
/// COSMETIC rng stream `cr` that NEVER touches the gameplay `Villager.rng`. Every roll is drawn
/// UP FRONT in a fixed, append-only order, so a guard and the worker they're re-skinned into (same
/// seed, different `kind`) get the identical face. `kid` forces a child look (no beard, simple hair).
/// (Legacy box-mesh villager builder, superseded by [`vil_biped_meshes`]; kept for reference.)
#[allow(dead_code)]
fn spec(kind: Kind, seed: u32, kid: bool) -> VSpec {
    let (id_skin, id_tunic) = match kind {
        Kind::Peasant { skin, tunic, .. } => (skin, tunic),
        Kind::Guard { skin, tunic } => (skin, tunic),
        Kind::Worker { skin, tunic, .. } => (skin, tunic),
    };
    let guard = matches!(kind, Kind::Guard { .. });
    let peasant_hat = matches!(kind, Kind::Peasant { hat: true, .. });
    let trade = match kind {
        Kind::Worker { trade, .. } => Some(trade),
        _ => None,
    };
    // The right hand carries the role's tool.
    let held = match kind {
        Kind::Guard { .. } => Held::Sword,
        Kind::Worker { trade: Trade::Farmer, .. } => Held::Hoe,
        Kind::Worker { trade: Trade::Woodcutter, .. } => Held::Axe,
        Kind::Worker { trade: Trade::Miner, .. } => Held::Pick,
        _ => Held::None,
    };

    // ── Cosmetic rolls (fixed order, drawn unconditionally so re-skin keeps the same face) ──
    let mut cr = (seed ^ 0x9E37_79B9) | 1;
    let r_skin = next_u32(&mut cr);
    let r_tunic = next_u32(&mut cr);
    let r_pant = next_u32(&mut cr);
    let r_weather = next_u32(&mut cr);
    let r_hair = next_u32(&mut cr);
    let r_elder = next_u32(&mut cr);
    let r_grey = next_u32(&mut cr);
    let r_style = next_u32(&mut cr);
    let r_tonsure = next_u32(&mut cr);
    let r_mood = next_u32(&mut cr);
    let r_es = next_u32(&mut cr);
    let r_ex = next_u32(&mut cr);
    let r_ey = next_u32(&mut cr);
    let r_squint = next_u32(&mut cr);
    let r_mouth = next_u32(&mut cr);
    let r_facial = next_u32(&mut cr);
    let r_fstyle = next_u32(&mut cr);

    // Guards/workers keep their identity colours (a re-skin stays the same person); ambient
    // peasants/kids reroll skin+tunic for crowd variety.
    let has_identity = guard || trade.is_some();
    let skin_hex = if has_identity { id_skin } else { SKIN[r_skin as usize % SKIN.len()] };
    let tunic_hex = if has_identity { id_tunic } else { TUNIC[r_tunic as usize % TUNIC.len()] };
    let pant_hex = PANT_TONES[r_pant as usize % PANT_TONES.len()];
    let weathered = !has_identity && r_weather % 3 == 0; // ~1/3 of ambient cloth reads worn
    let elder = !kid && r_elder % 100 < 14; // ~14% grey-haired elders
    let hair_hex = if elder {
        GREY_HAIR[r_grey as usize % GREY_HAIR.len()]
    } else {
        HAIR_TONES[r_hair as usize % HAIR_TONES.len()]
    };

    let skin = lin(skin_hex);
    let tunic = if weathered { lin_scaled(tunic_hex, 0.82) } else { lin(tunic_hex) };
    let pant = lin(pant_hex);
    let hair = lin(hair_hex);
    let armor = lin(ARMOR);

    // Static torso: tunic + a cinched belt + a role overlay (guard chestplate / farmer apron /
    // woodcutter vest). Cloth surface on the fabric, Metal on the guard plate.
    let mut torso_parts = vec![
        surf(bx(0.42, 0.48, 0.26, v(0.0, 0.7, 0.0), tunic), Surf::Cloth),
        surf(bx(0.06, 0.18, 0.27, v(0.0, 0.72, 0.0), lin(0xece8e0)), Surf::Cloth), // tunic placket
        surf(bx(0.44, 0.07, 0.28, v(0.0, 0.5, 0.0), lin(BELT)), Surf::Cloth), // belt
        surf(bx(0.09, 0.06, 0.02, v(0.0, 0.5, 0.135), lin(BUCKLE)), Surf::Metal), // belt buckle
    ];
    match (guard, trade) {
        (true, _) => torso_parts.push(surf(bx(0.46, 0.4, 0.3, v(0.0, 0.7, 0.0), armor), Surf::Metal)),
        (_, Some(Trade::Farmer)) => torso_parts.push(surf(bx(0.4, 0.4, 0.28, v(0.0, 0.62, 0.04), lin(APRON)), Surf::Cloth)),
        (_, Some(Trade::Woodcutter)) => torso_parts.push(surf(bx(0.44, 0.42, 0.29, v(0.0, 0.72, 0.0), lin(VEST)), Surf::Cloth)),
        (_, Some(Trade::Miner)) => torso_parts.push(surf(bx(0.45, 0.44, 0.3, v(0.0, 0.7, 0.0), lin(VEST)), Surf::Cloth)),
        _ => {}
    }
    let torso = group(torso_parts);

    // ── Head: deterministic face kit (skull + ears + nose + varied eyes/brows/mouth/hair/beard) ──
    // Bare-skin/hair/beard parts stay untagged (→ Skin in the shader). `bare` = no headgear, so
    // clip-prone tall hair only spawns then.
    let bare = !(guard || peasant_hat || trade.is_some());
    let mut head_parts = vec![
        bx(0.3, 0.3, 0.3, Vec3::ZERO, skin),            // skull
        bx(0.04, 0.08, 0.06, v(-0.16, 0.0, 0.0), skin), // ears
        bx(0.04, 0.08, 0.06, v(0.16, 0.0, 0.0), skin),
        bx(0.06, 0.05, 0.05, v(0.0, -0.02, 0.16), skin), // nose
    ];
    // Eyes — varied size / spacing / height, occasional squint.
    let es = 0.035 + (r_es % 3) as f32 * 0.012;
    let ex = 0.06 + (r_ex % 3) as f32 * 0.015;
    let ey = 0.02 + (r_ey % 3) as f32 * 0.012;
    let eh = if r_squint % 7 == 0 { 0.025 } else { es };
    head_parts.push(bx(es, eh, 0.02, v(-ex, ey, 0.16), lin(EYE)));
    head_parts.push(bx(es, eh, 0.02, v(ex, ey, 0.16), lin(EYE)));
    // Brows — hair-coloured, angled by mood (guards never look surprised).
    let mood = if guard { r_mood % 2 } else { r_mood % 3 };
    let (bl, brr) = match mood {
        1 => (0.5_f32, -0.5), // angry V (inner ends low)
        2 => (-0.30, 0.30),   // raised / surprised
        _ => (0.0, 0.0),      // neutral
    };
    let by = ey + 0.055;
    head_parts.push(bxr(0.10, 0.035, 0.04, v(-ex, by, 0.155), Quat::from_rotation_z(bl), hair));
    head_parts.push(bxr(0.10, 0.035, 0.04, v(ex, by, 0.155), Quat::from_rotation_z(brr), hair));
    // Mouth — guards grim, kids smile, others vary.
    let mshape = if guard {
        r_mouth % 2
    } else if kid {
        3
    } else {
        r_mouth % 4
    };
    let (mw, my) = match mshape {
        1 => (0.07, -0.10),  // grim
        2 => (0.09, -0.095), // frown
        3 => (0.10, -0.08),  // smile
        _ => (0.08, -0.085), // flat
    };
    head_parts.push(bx(mw, 0.022, 0.02, v(0.0, my, 0.16), lin(LIP)));
    // Hair style — CROP / MOP / BALD / LONG; clip-prone LONG falls back to CROP under headgear.
    let mut style = r_style % 4;
    if !bare && style == 3 {
        style = 0;
    }
    match style {
        1 => {
            // MOP — shaggy: top + side flaps + back.
            head_parts.push(bx(0.32, 0.10, 0.32, v(0.0, 0.14, 0.0), hair));
            head_parts.push(bx(0.06, 0.16, 0.30, v(-0.155, 0.04, 0.0), hair));
            head_parts.push(bx(0.06, 0.16, 0.30, v(0.155, 0.04, 0.0), hair));
            head_parts.push(bx(0.30, 0.14, 0.06, v(0.0, 0.02, -0.155), hair));
        }
        2 => {
            // BALD — bare skull, sometimes a tonsure rim.
            if r_tonsure % 2 == 0 {
                head_parts.push(bx(0.31, 0.03, 0.31, v(0.0, 0.10, 0.0), hair));
            }
        }
        3 => {
            // LONG — top + a back curtain to the neck (bare heads only).
            head_parts.push(bx(0.31, 0.08, 0.31, v(0.0, 0.13, 0.0), hair));
            head_parts.push(bx(0.30, 0.26, 0.07, v(0.0, -0.06, -0.155), hair));
        }
        _ => {
            // CROP — the classic top slab.
            head_parts.push(bx(0.31, 0.08, 0.31, v(0.0, 0.13, 0.0), hair));
        }
    }
    // Beard / moustache / stubble — a minority; forced for woodcutters + elders, never for kids.
    let has_facial = !kid
        && (r_facial % 20 < 11 || elder || matches!(trade, Some(Trade::Woodcutter)));
    if has_facial {
        let beard = if elder { lin(GREY_HAIR[r_grey as usize % GREY_HAIR.len()]) } else { hair };
        match r_fstyle % 4 {
            0 => head_parts.push(bx(0.20, 0.10, 0.04, v(0.0, -0.10, 0.145), lin_scaled(skin_hex, 0.78))), // stubble
            1 => head_parts.push(bx(0.14, 0.035, 0.03, v(0.0, -0.055, 0.155), beard)), // moustache
            2 => {
                head_parts.push(bx(0.22, 0.12, 0.06, v(0.0, -0.12, 0.13), beard)); // short beard
                head_parts.push(bx(0.05, 0.12, 0.10, v(-0.13, -0.10, 0.08), beard));
                head_parts.push(bx(0.05, 0.12, 0.10, v(0.13, -0.10, 0.08), beard));
            }
            _ => {
                head_parts.push(bx(0.22, 0.12, 0.06, v(0.0, -0.12, 0.13), beard)); // long beard
                head_parts.push(bx(0.05, 0.12, 0.10, v(-0.13, -0.10, 0.08), beard));
                head_parts.push(bx(0.05, 0.12, 0.10, v(0.13, -0.10, 0.08), beard));
                head_parts.push(bx(0.18, 0.16, 0.07, v(0.0, -0.22, 0.11), beard));
            }
        }
    }
    // Per-type headgear (pushed AFTER the face) + small recognition props.
    match (guard, trade, peasant_hat) {
        (true, _, _) => {
            head_parts.push(surf(bx(0.34, 0.16, 0.34, v(0.0, 0.16, 0.0), armor), Surf::Metal)); // helmet
            head_parts.push(surf(cone(0.1, 0.16, v(0.0, 0.3, 0.0), Quat::IDENTITY, armor), Surf::Metal)); // crest spike
            head_parts.push(surf(bx(0.04, 0.20, 0.05, v(-0.155, 0.0, 0.05), lin(STRAP)), Surf::Cloth)); // chin-strap
            head_parts.push(surf(bx(0.04, 0.20, 0.05, v(0.155, 0.0, 0.05), lin(STRAP)), Surf::Cloth));
        }
        (_, Some(Trade::Farmer), _) => {
            head_parts.push(surf(bx(0.5, 0.04, 0.5, v(0.0, 0.18, 0.0), lin(STRAW_HAT)), Surf::Cloth)); // wide straw brim
            head_parts.push(surf(cone(0.18, 0.14, v(0.0, 0.2, 0.0), Quat::IDENTITY, lin(STRAW_HAT)), Surf::Cloth)); // crown
        }
        (_, Some(Trade::Woodcutter), _) => {
            head_parts.push(surf(bx(0.34, 0.1, 0.34, v(0.0, 0.18, 0.0), lin(CAP)), Surf::Cloth)); // flat cap
            head_parts.push(surf(bx(0.34, 0.05, 0.14, v(0.0, 0.16, 0.2), lin(CAP)), Surf::Cloth)); // peak
        }
        (_, Some(Trade::Miner), _) => {
            head_parts.push(surf(bx(0.35, 0.13, 0.35, v(0.0, 0.16, 0.0), lin(CAP)), Surf::Cloth)); // skullcap
            head_parts.push(bx(0.30, 0.14, 0.04, v(0.0, -0.08, 0.155), lin(SOOT))); // soot smudge (Skin)
            head_parts.push(surf(bx(0.05, 0.05, 0.05, v(0.0, 0.16, 0.18), lin(MINER_LAMP)), Surf::Metal)); // brass lamp clip
        }
        (_, _, true) => head_parts.push(surf(cone(0.22, 0.2, v(0.0, 0.22, 0.0), Quat::IDENTITY, lin(HAT)), Surf::Cloth)),
        _ => {}
    }
    let head = group(head_parts);

    // Legs (top at the hip pivot) — trousers + a leather boot at the foot (added detail).
    let leg = || {
        group(vec![
            surf(bx(0.16, 0.36, 0.18, v(0.0, -0.18, 0.0), pant), Surf::Cloth),
            bx(0.17, 0.1, 0.22, v(0.0, -0.36, 0.03), lin(BOOT)), // boot (leather → Skin default)
        ])
    };

    // Arms — sleeve (Cloth) + bare hand (Skin) + the role's held item; metal heads tagged Metal.
    let arm = |held: Held| {
        let mut p = vec![
            surf(bx(0.13, 0.36, 0.22, v(0.0, -0.18, 0.0), tunic), Surf::Cloth), // sleeve
            bx(0.12, 0.1, 0.2, v(0.0, -0.42, 0.0), skin),                       // hand
        ];
        if guard {
            p.push(surf(bx(0.18, 0.16, 0.26, v(0.0, 0.02, 0.0), armor), Surf::Metal)); // pauldron
        }
        let hand = v(0.0, -0.46, 0.1);
        match held {
            Held::None => {}
            Held::Sword => {
                p.push(surf(bx(0.18, 0.06, 0.05, hand, lin(SWORD_GUARD)), Surf::Metal));
                p.push(surf(bx(0.05, 0.06, 0.5, hand + v(0.0, 0.0, 0.32), lin(SWORD_BLADE)), Surf::Metal));
            }
            Held::Hoe => {
                // A long shaft forward-down + a small blade at the tip.
                p.push(bx(0.05, 0.05, 0.66, hand + v(0.0, -0.06, 0.28), lin(TOOL_WOOD)));
                p.push(surf(bx(0.16, 0.04, 0.1, hand + v(0.0, -0.12, 0.6), lin(TOOL_METAL)), Surf::Metal));
            }
            Held::Axe => {
                // A shaft + a wedge head near the tip.
                p.push(bx(0.05, 0.05, 0.56, hand + v(0.0, -0.04, 0.24), lin(TOOL_WOOD)));
                p.push(surf(bx(0.16, 0.18, 0.06, hand + v(0.0, 0.0, 0.5), lin(TOOL_METAL)), Surf::Metal));
            }
            Held::Pick => {
                // A shaft + a crossways double-pointed pick head (a bar across the tip).
                p.push(bx(0.05, 0.05, 0.58, hand + v(0.0, -0.04, 0.25), lin(TOOL_WOOD)));
                p.push(surf(bx(0.46, 0.05, 0.05, hand + v(0.0, 0.02, 0.52), lin(TOOL_METAL)), Surf::Metal));
            }
        }
        group(p)
    };

    let parts = vec![
        PartDef { kind: PartKind::Leg(1.0), pivot: v(-0.11, 0.34, 0.0), mesh: leg() },
        PartDef { kind: PartKind::Leg(-1.0), pivot: v(0.11, 0.34, 0.0), mesh: leg() },
        PartDef { kind: PartKind::Arm(1.0), pivot: v(0.27, 0.92, 0.0), mesh: arm(held) }, // right (+tool)
        PartDef { kind: PartKind::Arm(-1.0), pivot: v(-0.27, 0.92, 0.0), mesh: arm(Held::None) },
        PartDef { kind: PartKind::Head, pivot: v(0.0, 1.12, 0.0), mesh: head },
    ];
    VSpec { torso, parts }
}

// ── Placement ────────────────────────────────────────────────────────────────────

/// Spawn the castle town's **ambient** props + flavour NPCs: the market stall, well, woodpile,
/// gather spots, two travelling pilgrims and a few scampering kids. Called from `worldmap::build`
/// after the castle. The town's actual **population** (the 4 starting peasants, and any grown or
/// rescued since) are NOT spawned here — `town::sync_population_bodies` reconciles those bodies to
/// `town.population`, so the headcount you see always equals the headcount the HUD reports.
pub fn populate(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
    creature_mats: &mut Assets<crate::creature::CreatureMaterial>,
) {
    // `mat` dresses the static PROPS (stall/well/woodpile) — they stay on the plain white
    // material. `body_mat` is the textured creature material the villager BODIES draw against.
    let mat = materials.add(StandardMaterial { base_color: Color::WHITE, perceptual_roughness: 0.85, ..default() });
    let body_mat = crate::creature::make_creature_material(creature_mats);
    let mut rng: u32 = 0x5117_aced;
    let mut placed: Vec<Vec2> = Vec::new();
    let half = crate::castle::courtyard_half();

    // A little market just outside the south gate: a striped stall (the visible counterpart to the
    // menu shop). The keep gate is -Z. (No idle trader bodies — only real population walks the town.)
    let south = crate::castle::gate_centers()[0];
    let market = south + Vec2::new(2.5, -5.0);
    let my = worldmap::ground_at_world(market.x, market.y).unwrap_or(0.0);
    commands.spawn((
        Mesh3d(meshes.add(market_stall_mesh())),
        MeshMaterial3d(mat.clone()),
        Transform::from_xyz(market.x, my, market.y),
        BiomeEntity,
    ));
    // Solid stall — the 1.8-wide counter + posts block; the hero walks around it.
    crate::blockers::add_box(market.x, market.y, 0.95, 0.4);

    // Two wandering pilgrims who trek between the island's landmarks, hinting the way (see
    // `pilgrim_brain` / `pilgrim_hint`). They start just outside a gate and head off.
    let gates = crate::castle::gate_centers();
    for i in 0..2 {
        let g = gates[i % gates.len()];
        let home = g + (-g).normalize_or_zero() * 2.0;
        let kind = Kind::Peasant { skin: SKIN[i % SKIN.len()], tunic: PILGRIM_ROBE, hat: true };
        let e = spawn(commands, meshes, &body_mat, kind, home, home, 2.0, 2.0, SCALE, next_u32(&mut rng), false);
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

    // ── Kids: a few small villagers scampering around a play patch just inside the south gate.
    // High speed + tiny wander radius + the `Kid` marker → short darting bursts that read as play
    // (they skip the adults' gathering behaviour). The night curfew hides them with the rest of the
    // ambient townsfolk — they just disappear when the wave falls. ──
    let play = gates[0] + (-gates[0]).normalize_or_zero() * 3.0;
    for i in 0..4 {
        let home = play + Vec2::new(rng_range(&mut rng, -1.6, 1.6), rng_range(&mut rng, -1.6, 1.6));
        let kind = Kind::Peasant { skin: SKIN[i % SKIN.len()], tunic: TUNIC[(i + 1) % TUNIC.len()], hat: i % 2 == 0 };
        let e = spawn(commands, meshes, &body_mat, kind, home, home, 2.8, 1.7, KID_SCALE, next_u32(&mut rng), false);
        commands.entity(e).insert(Kid);
    }

    // Screenshot hook: `FOREST_VILLINE="x,z"` parks one of each townsperson look in a line at the
    // given world XZ for model/texture close-ups (mirrors `FOREST_ORKLINE`/`FOREST_WILDLINE`):
    // peasant · farmer · woodcutter · miner · guard. Tiny wander radius so they idle in place.
    if let Ok(s) = std::env::var("FOREST_VILLINE") {
        let p: Vec<f32> = s.split(',').filter_map(|t| t.trim().parse().ok()).collect();
        if p.len() == 2 {
            let kinds = [
                Kind::Peasant { skin: SKIN[0], tunic: TUNIC[0], hat: false },
                Kind::Worker { trade: Trade::Farmer, skin: SKIN[1], tunic: TUNIC[2] },
                Kind::Worker { trade: Trade::Woodcutter, skin: SKIN[2], tunic: TUNIC[1] },
                Kind::Worker { trade: Trade::Miner, skin: SKIN[0], tunic: TUNIC[3] },
                Kind::Guard { skin: SKIN[1], tunic: TUNIC[0] },
            ];
            for (i, k) in kinds.into_iter().enumerate() {
                let x = p[0] + i as f32 * 1.5 - 3.0;
                let home = Vec2::new(x, p[1]);
                // speed 0 + wander 0 → they stand stock-still for the capture.
                let e = spawn(commands, meshes, &body_mat, k, home, home, 0.0, 0.0, SCALE, 50 + i as u32 * 13, false);
                // Face the camera (−Z) + tag SceneActor so the wander brain leaves the transform
                // alone and the facing sticks (clean front-on face shots).
                let y = worldmap::ground_at_world(x, p[1]).unwrap_or(0.0);
                commands.entity(e).insert((
                    crate::scenes::SceneActor,
                    Transform {
                        translation: Vec3::new(x, y, p[1]),
                        rotation: Quat::from_rotation_y(std::f32::consts::PI),
                        scale: Vec3::splat(SCALE),
                    },
                ));
            }
        }
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
    mat: &Handle<crate::creature::CreatureMaterial>,
    kind: Kind,
    home: Vec2,
    pos: Vec2,
    speed: f32,
    wander_r: f32,
    scale: f32,
    seed: u32,
    desert: bool,
) -> Entity {
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
        body_r: 0.22,
        phase,
        moving: false,
        mode: Mode::Idle,
        timer: rng_range(&mut r, 0.5, 4.0),
        rng: r,
        gathering: false,
        atk_anim: 0.0,
        head_yaw: 0.0,
        greet: 0.0,
        greet_armed: true,
    };

    let root = commands
        .spawn((
            Transform { translation: Vec3::new(pos.x, y, pos.y), rotation: Quat::from_rotation_y(facing), scale: Vec3::splat(scale) },
            Visibility::Visible,
            vil,
            BipedDrive { phase, ..default() },
            BiomeEntity,
        ))
        .id();
    // Kids pass KID_SCALE (< SCALE) — detect that here so they get the childlike build.
    build_biped_body(commands, root, kind, seed, scale < SCALE, desert, mat, meshes);

    // Armoured townsfolk double as town guards — they fight invaders at night and can be pulled to
    // staff a producer by day (the `Townsfolk` pool). They carry their identity colours [`Folk`] +
    // current [`Role`] + body material so `reskin_townsfolk` can redress them as a farmer/woodcutter
    // when they take a job. The NavPath caches an A* route home for a freed captive (see `guard_combat`).
    if let Kind::Guard { skin, tunic } = kind {
        commands.entity(root).insert((
            Guard { atk_cd: 0.0, post: home },
            NpcHp { hp: NPC_MAX_HP, max: NPC_MAX_HP },
            crate::navgrid::NavPath::default(),
            Townsfolk,
            Folk { skin, tunic, seed },
            Role::Guard,
            BodyMat(mat.clone()),
        ));
    }
    root
}

/// Map a villager [`Kind`] (+ cosmetic `seed`) to a studio peasant biped mesh set. The town keeps
/// its per-villager skin/tunic variety; the trouser tone is picked deterministically from the seed.
fn vil_biped_meshes(kind: Kind, seed: u32, kid: bool, desert: bool) -> BipedMeshes {
    let trouser = PANT_TONES[(seed as usize) % PANT_TONES.len()];
    let (pk, skin, tunic) = match kind {
        Kind::Guard { skin, tunic } => (PeasantKind::Guard, skin, tunic),
        Kind::Worker { trade, skin, tunic } => (
            match trade {
                Trade::Farmer => PeasantKind::Farmer,
                Trade::Woodcutter => PeasantKind::Woodcutter,
                Trade::Miner => PeasantKind::Miner,
            },
            skin,
            tunic,
        ),
        Kind::Peasant { skin, tunic, .. } => (PeasantKind::Unemployed, skin, tunic),
    };
    peasant_biped_meshes(pk, skin, tunic, trouser, kid, desert)
}

/// Build a townsperson's body on the shared studio biped skeleton (`biped.rs`): one [`spawn_biped`]
/// call whose `rig` child carries [`BipedRig`] so a re-skin can drop + rebuild the whole skeleton in
/// place. Shared by [`spawn`] (fresh) + [`reskin_townsfolk`] (job change). `kid` bumps the head a
/// touch for a childlike build (the root scale already shrinks them).
fn build_biped_body(
    commands: &mut Commands,
    root: Entity,
    kind: Kind,
    seed: u32,
    kid: bool,
    desert: bool,
    mat: &Handle<crate::creature::CreatureMaterial>,
    meshes: &mut Assets<Mesh>,
) {
    let h = vil_biped_meshes(kind, seed, kid, desert).upload(meshes);
    // Kids get an oversized head on a downscaled body (chibi proportions) so they read as children,
    // not just small adults.
    let head_scale = if kid { 1.55 } else { 1.06 };
    // No off-hand prop: peasants carry only their trade tool, so the shield/pouch slot is empty.
    crate::biped::spawn_biped(commands, root, mat, h, head_scale, 1.0, 0.15, 0.3, VIL_RIG_OFF, None);
}

/// Spawn a villager's body (torso + limbs + head) as children of `root`, each tagged
/// [`VilBodyPart`] so a re-skin can despawn exactly the body. (Legacy box-mesh rig, superseded by
/// [`build_biped_body`]; kept for reference / the staged-scene mime path.)
#[allow(dead_code)]
fn build_body(root: &mut bevy::ecs::system::EntityCommands, s: VSpec, mat: &Handle<crate::creature::CreatureMaterial>, meshes: &mut Assets<Mesh>) {
    let torso = meshes.add(s.torso);
    let parts: Vec<(PartKind, Vec3, Handle<Mesh>)> =
        s.parts.into_iter().map(|p| (p.kind, p.pivot, meshes.add(p.mesh))).collect();
    root.with_children(|p| {
        p.spawn((Mesh3d(torso), MeshMaterial3d(mat.clone()), Transform::default(), VilBodyPart));
        for (kind, pivot, mesh) in parts {
            p.spawn((
                Mesh3d(mesh),
                MeshMaterial3d(mat.clone()),
                Transform::from_translation(pivot),
                VilPart { kind },
                VilBodyPart,
            ));
        }
    });
}

/// Redress a townsperson when their role changes: a farmer when posted to a Farm, a woodcutter on a
/// Woodcutter, an armed guard otherwise. Rebuilds only the body (despawns the [`VilBodyPart`]
/// children, respawns from the new outfit), reusing the same identity + material — so it's the same
/// person in new work clothes. Cheap (rebuilds only on a role change, a few times a day per worker).
#[allow(clippy::type_complexity)]
fn reskin_townsfolk(
    town: Res<crate::town::TownRes>,
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    rigs: Query<(), With<BipedRig>>,
    mut folk: Query<(Entity, &Folk, &mut Role, &BodyMat, &Children, Option<&crate::town::Worker>), With<Townsfolk>>,
) {
    use tileworld_core::town_store::BuildKind;
    for (e, f, mut role, body_mat, children, worker) in &mut folk {
        let desired = match worker.and_then(|w| town.0.plots.get(w.idx)).and_then(|p| p.kind) {
            Some(BuildKind::Farm) => Role::Working(Trade::Farmer),
            Some(BuildKind::Lumber) => Role::Working(Trade::Woodcutter),
            Some(BuildKind::Mine) => Role::Working(Trade::Miner),
            _ => Role::Guard,
        };
        if *role == desired {
            continue;
        }
        // Drop the whole studio skeleton (the `rig` child carries `BipedRig`) and rebuild it in the
        // new outfit, reusing the same identity + material — the same person in new work clothes.
        for &c in children {
            if rigs.get(c).is_ok() {
                commands.entity(c).try_despawn();
            }
        }
        let kind = match desired {
            Role::Guard => Kind::Guard { skin: f.skin, tunic: f.tunic },
            Role::Working(trade) => Kind::Worker { trade, skin: f.skin, tunic: f.tunic },
        };
        build_biped_body(&mut commands, e, kind, f.seed, false, false, &body_mat.0, &mut meshes);
        *role = desired;
    }
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
fn bxr(w: f32, h: f32, d: f32, off: Vec3, rot: Quat, c: [f32; 4]) -> Mesh {
    tinted(Cuboid::new(w, h, d).mesh().build().rotated_by(rot).translated_by(off), c)
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
