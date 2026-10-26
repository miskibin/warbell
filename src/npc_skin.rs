//! Realistic skinned townsfolk: MakeHuman (MPFB) characters with Quaternius UAL clips, replacing the
//! procedural 14-joint studio body for people who carry a [`SkinRequest`].
//!
//! Content: `assets/npc/npc_<look>.glb` (53-bone skin + the clip set baked onto that look's rig) and
//! its BC7 maps (`_albedo` / `_normal` / `_orm` atlas + RGBA hair / brow / lash / beard cards), all
//! produced by `tools/npc_mpfb/export_game.py` and packed by `examples/pack_forest_textures.rs`
//! (`NPC_SRC=`). The glTF materials are placeholders matched by NAME (`npc_body`, `npc_cloth`, ...).
//!
//! Runtime contract: the gameplay brains are untouched. They keep writing [`BipedDrive`] (and the
//! studio rig still exists, hidden, so joint lookups keep working). [`drive_skins`] reads the drive
//! plus the root's real ground speed and cross-fades the matching clip on the glTF `AnimationPlayer`.
//! `FOREST_NOSKIN=1` turns the whole thing off (studio bodies everywhere).

use std::collections::HashMap;
use std::time::Duration;

use bevy::camera::visibility::{DynamicSkinnedMeshBounds, VisibleEntities};
use bevy::gltf::{Gltf, GltfMaterialName};
use bevy::prelude::*;
use bevy::world_serialization::{WorldAsset, WorldAssetRoot, WorldInstanceReady};

use crate::biped::{BipedDrive, BipedRig};

/// Cast order = the look index a [`SkinRequest`] carries.
pub const LOOKS: [&str; 15] = [
    "farmer", "woodcutter", "miner", "guard", "archer", "villager_m", "villager_f1", "villager_f2",
    "elder_m", "elder_f", "boy", "girl", "priest", "merchant", "maud",
];
pub const FARMER: u8 = 0;
pub const WOODCUTTER: u8 = 1;
pub const MINER: u8 = 2;
pub const GUARD: u8 = 3;
pub const ARCHER: u8 = 4;
pub const VILLAGER_M: u8 = 5;
pub const VILLAGER_F1: u8 = 6;
pub const VILLAGER_F2: u8 = 7;
pub const ELDER_M: u8 = 8;
pub const ELDER_F: u8 = 9;
pub const BOY: u8 = 10;
pub const GIRL: u8 = 11;
pub const PRIEST: u8 = 12;
pub const MERCHANT: u8 = 13;
/// Maud, the colony's matron (campaign cast): her own authored outfit + grey braid bun, never re-dyed.
pub const MAUD: u8 = 14;

/// World units per real metre: a 1.75 m adult stands the studio villager's 1.46 u.
pub const WORLD_PER_M: f32 = 0.834;
/// Alpha-test cutoff of the hair / brow / lash cards (the packer keeps mip coverage for this).
pub const CARD_CUTOFF: f32 = 0.5;
/// Ground speed (real m/s) each look's `Walk_Loop` covers at 1x: its stance foot's backward slide,
/// measured by forward kinematics over the exported GLB (the clip is baked onto every rig, so a
/// child's short legs cover far less ground per cycle). Cast order, like [`LOOKS`].
const WALK_M: [f32; 15] = [1.34, 1.43, 1.31, 1.52, 1.41, 1.32, 1.19, 1.17, 1.28, 1.11, 0.90, 0.90, 1.35, 1.35, 1.14];
/// The same for the Mixamo `Walk_Loop` (a shorter, more natural stride) when the look's private clip
/// set is installed (see [`NpcCast::private`]).
const WALK_MX: [f32; 15] = [1.03, 1.10, 1.01, 1.16, 1.09, 1.03, 0.88, 0.87, 0.95, 0.86, 0.67, 0.67, 1.02, 1.02, 0.87];
/// The other locomotion clips' ground speed as a fraction of the same rig's walk (measured on the
/// adult villager: carry 0.83, push 0.33, jog ~6.5 m/s against a 1.32 m/s walk — the UAL jog is a
/// long-flight run). Carry / push stay UAL clips under the Mixamo set, so they are always paced off
/// the UAL walk (`Skinned::ual`).
const CARRY_K: f32 = 0.63;
const PUSH_K: f32 = 0.25;
const JOG_K: f32 = 4.9;
/// Mixamo set, against its own walk (every look: slow run 2.8–2.9×, sad walk 0.75×).
const JOG_KX: f32 = 2.85;
const SAD_KX: f32 = 0.75;
/// Past this camera distance a skin's pose freezes and its animation graph is detached — a paused
/// clip still costs `animate_targets` a full per-bone evaluation every frame, a graph-less player one
/// lookup. Re-attached inside `ANIM_WAKE2` (hysteresis, so it can't flicker).
const ANIM_CULL2: f32 = 90.0 * 90.0;
const ANIM_WAKE2: f32 = 82.0 * 82.0;
/// Skinned townsfolk stop drawing past 160 u (they skip frustum culling, see `skin_ready`).
const SKIN_RANGE: bevy::camera::visibility::VisibilityRange = bevy::camera::visibility::VisibilityRange {
    start_margin: 0.0..0.0,
    end_margin: 160.0..160.0,
    use_aabb: false,
};
const FADE: Duration = Duration::from_millis(280);

pub struct NpcSkinPlugin;

impl Plugin for NpcSkinPlugin {
    fn build(&self, app: &mut App) {
        if !enabled() {
            return;
        }
        app.init_resource::<crate::anim_lod::SkinLodCache>();
        app.add_systems(Startup, load_cast)
            .add_systems(Update, (finish_cast, attach_skins, reveal_skins, hide_studio_rigs, copy_tools, drive_skins, carry_loads).chain());
        if std::env::var_os("FOREST_GAITCHECK").is_some() {
            app.add_systems(PostUpdate, gait_check.after(bevy::transform::TransformSystems::Propagate));
        }
    }
}

/// Ask for a realistic body: the look index into [`LOOKS`]. Inserted by the spawner; the studio
/// rig is hidden and the skin attached as soon as the look's assets have loaded. Re-inserting a
/// different look (a job change) swaps the outfit.
#[derive(Component, Clone, Copy)]
pub struct SkinRequest(pub u8);

/// Clip hint from a non-locomotion brain (village life acts, vendors): `""` = let the drive pick.
#[derive(Component, Default, Clone, Copy, PartialEq)]
pub struct SkinClip(pub &'static str);

/// With a [`SkinClip`]: hold that clip at this playhead (seconds) instead of playing it — a cutscene
/// scrubbing a one-shot (a kneel, a lift) off its own clock.
#[derive(Component, Clone, Copy, PartialEq)]
pub struct ClipSeek(pub f32);

/// A root whose skin child is attached. `player` / hands fill in once the glTF instance is ready.
#[derive(Component)]
pub struct Skinned {
    look: u8,
    inst: Entity,
    tools_done: bool,
    player: Option<Entity>,
    clip: &'static str,
    last: Vec3,
    speed: f32,
    /// World u/s this person's `Walk_Loop` covers at 1x (look's stride × their height): the
    /// locomotion clips play at ground speed / this, so the planted foot stays planted.
    stride: f32,
    /// The same for the UAL `Walk_Loop` (carry / push are UAL clips in either set).
    ual: f32,
    /// The look plays the private Mixamo clip set.
    mx: bool,
    /// The `foot_l` / `foot_r` joints (read by the `FOREST_GAITCHECK` skate meter).
    feet: [Option<Entity>; 2],
    pub hand_r: Option<Entity>,
    pub hand_l: Option<Entity>,
    /// The skin's pelvis joint (read by the `FOREST_LIFECHECK` seat check).
    pub pelvis: Option<Entity>,
    /// The body node carrying the `jaw_open` morph weight (driven by `lipsync`).
    mouth: Option<Entity>,
    /// Hand-held props copied from the studio rig: (entity, how it's held, grip -> head distance).
    props: Vec<(Entity, Held, f32)>,
    /// Which grip the props are in (true = the two-handed work / fight grip).
    work_grip: bool,
    /// Far from / unseen by the camera: the player's graph is detached (see `ANIM_CULL2`).
    parked: bool,
    /// One of the skin's mesh entities — its presence in the camera's visible set says whether
    /// this person is on screen (see `crate::anim_lod`).
    body: Option<Entity>,
    /// Seconds the body has been off screen.
    unseen: f32,
    /// Every mesh entity of the skin (shadow LOD) and whether they currently cast.
    meshes: Vec<Entity>,
    casting: bool,
    /// Drawing the meshopt far twins (`anim_lod::swap_meshes`).
    lod_far: bool,
}

impl Skinned {
    /// The spawned glTF instance (the visible body) under the gameplay root.
    pub fn inst(&self) -> Entity {
        self.inst
    }
    /// The entity whose `MorphWeights` open this person's mouth (once the skin is in).
    pub fn mouth(&self) -> Option<Entity> {
        self.mouth
    }
}

impl Skinned {
    /// World ground speed a locomotion clip covers at 1x on this skin (`None`: not a locomotion clip).
    fn loco_native(&self, clip: &str) -> Option<f32> {
        match clip {
            "Walk_Loop" => Some(self.stride),
            "Walk_Carry_Loop" => Some(self.ual * CARRY_K),
            "Push_Loop" => Some(self.ual * PUSH_K),
            "Jog_Fwd_Loop" if self.mx => Some(self.stride * JOG_KX),
            "Jog_Fwd_Loop" => Some(self.ual * JOG_K),
            "Sad_Walk_Loop" if self.mx => Some(self.stride * SAD_KX),
            _ => None,
        }
    }
}

/// Whether the realistic skins are on (`FOREST_NOSKIN` keeps the studio bodies everywhere).
pub fn enabled() -> bool {
    std::env::var_os("FOREST_NOSKIN").is_none()
}

/// Points the glTF instance back at its gameplay root (read by the ready observer).
#[derive(Component)]
struct SkinOf(Entity);

struct LookAssets {
    scene: Handle<WorldAsset>,
    graph: Handle<AnimationGraph>,
    nodes: HashMap<String, AnimationNodeIndex>,
    mats: HashMap<&'static str, Handle<StandardMaterial>>,
    /// The private Mixamo clips replaced the committed UAL clips of the same names.
    mixamo: bool,
}

#[derive(Resource)]
struct NpcCast {
    gltf: Vec<Handle<Gltf>>,
    /// Per look: `assets/npc_private/npc_<look>_anims.glb`, the Mixamo clip set baked onto that look's
    /// rig (`tools/npc_mpfb/bake_mixamo.py` + `export_anims.py`). Mixamo's licence allows the clips
    /// inside the game but not as files in the public repo, so the folder is gitignored and filled by
    /// the build machine; a checkout without it simply keeps the UAL clips. `FOREST_UALANIM=1` ignores
    /// it (A/B the two sets).
    private: Vec<Option<Handle<Gltf>>>,
    mats: Vec<HashMap<&'static str, Handle<StandardMaterial>>>,
    /// Per look: every texture its materials read (a skin is only shown once all are in, see `Reveal`).
    imgs: Vec<Vec<Handle<Image>>>,
    /// Per look: dyed-cloth variants, then hair-shade variants (index 0 = the look's own).
    vars: Vec<(Vec<Handle<StandardMaterial>>, Vec<Handle<StandardMaterial>>)>,
    looks: Vec<Option<LookAssets>>,
}

/// Cloth dyes a village could afford (multiplied over the look's own albedo): undyed, madder
/// russet, woad blue, weld/olive, a dark walnut-dyed, a sun-faded grey. Townsfolk sharing a look
/// still read as different people across a street.
const CLOTH_DYES: [Vec3; 6] = [
    Vec3::new(1.0, 1.0, 1.0),
    Vec3::new(1.12, 0.86, 0.78),
    Vec3::new(0.82, 0.92, 1.1),
    Vec3::new(1.02, 1.0, 0.76),
    Vec3::new(0.7, 0.66, 0.62),
    Vec3::new(1.05, 1.05, 1.05),
];
/// Multipliers over every townsperson's baked albedo (player: the folk read garish — bright
/// lemon / scarlet cloth and pink skin against the muted world). Cloth is darkened and pulled off
/// yellow (blue kept a touch higher); skin loses a little red so bare torsos aren't salmon.
const CLOTH_MUTE: Color = Color::srgb(0.76, 0.74, 0.82);
const SKIN_MUTE: Color = Color::srgb(0.84, 0.86, 0.88);
/// Hair shades over the card texture: own, darker brown, near-black, sandy, greying.
const HAIR_SHADES: [Vec3; 5] = [
    Vec3::new(1.0, 1.0, 1.0),
    Vec3::new(0.62, 0.55, 0.5),
    Vec3::new(0.38, 0.35, 0.34),
    Vec3::new(1.35, 1.18, 0.9),
    Vec3::new(1.25, 1.25, 1.25),
];

/// Pins a person's hair to one of [`HAIR_SHADES`] (a cast member whose look must not depend on the
/// per-person hash, e.g. a cutscene's lead).
#[derive(Component, Clone, Copy)]
pub struct HairShade(pub usize);

/// A stable per-person hash (outfit dye, hair, height) from the root entity.
fn person_hash(e: Entity) -> u32 {
    let mut h = e.index_u32().wrapping_mul(0x9E37_79B9) ^ 0x5bd1_e995;
    h ^= h >> 15;
    h = h.wrapping_mul(0x2c1b_3c6d);
    h ^ (h >> 12)
}

/// Livery looks keep their colours (a guard's tabard, the archer's kit, the priest's habit).
fn uniform_look(look: u8) -> bool {
    matches!(look, GUARD | ARCHER | PRIEST | MAUD)
}

fn card_key(k: &str) -> &'static str {
    match k {
        "hair" => "npc_hair",
        "beard" => "npc_beard",
        "brow" => "npc_brow",
        _ => "npc_lash",
    }
}

fn load_cast(mut commands: Commands, server: Res<AssetServer>, mut materials: ResMut<Assets<StandardMaterial>>) {
    let mut gltf = Vec::new();
    let mut private = Vec::new();
    let mut mats = Vec::new();
    let mut vars = Vec::new();
    let mut imgs = Vec::new();
    let ual_only = std::env::var_os("FOREST_UALANIM").is_some();
    for name in LOOKS {
        gltf.push(server.load(format!("npc/npc_{name}.glb")));
        let anims = format!("npc_private/npc_{name}_anims.glb");
        private.push((!ual_only && std::path::Path::new("assets").join(&anims).exists()).then(|| server.load(anims)));
        let tex = |m: &str| -> Handle<Image> { server.load(format!("npc/npc_{name}_{m}.dds")) };
        // Maud's layered outfit (blouse under apron, pushed off the body) bakes a blotchy AO into
        // the cloth islands that renders as black patches: her cloth skips the occlusion read.
        let layered = name == "maud";
        let atlas = |double: bool| StandardMaterial {
            base_color: if double { CLOTH_MUTE } else { SKIN_MUTE },
            base_color_texture: Some(tex("albedo")),
            normal_map_texture: Some(tex("normal")),
            metallic_roughness_texture: Some(tex("orm")),
            occlusion_texture: if double && layered { None } else { Some(tex("orm")) },
            perceptual_roughness: 1.0,
            metallic: 1.0, // x the ORM blue channel (0): the factor only enables the texture read
            reflectance: 0.35,
            double_sided: double,
            cull_mode: if double { None } else { Some(bevy::render::render_resource::Face::Back) },
            ..default()
        };
        let card = |k: &str, rough: f32| StandardMaterial {
            base_color_texture: Some(tex(k)),
            alpha_mode: AlphaMode::Mask(CARD_CUTOFF),
            perceptual_roughness: rough,
            reflectance: 0.3,
            double_sided: true,
            cull_mode: None,
            ..default()
        };
        let mut m = HashMap::new();
        m.insert("npc_body", materials.add(atlas(false)));
        m.insert("npc_cloth", materials.add(atlas(true)));
        // teeth, tongue and the dark mouth bag (`add_face_morphs.py`), coloured per vertex
        // (dimmed: inside a mouth the teeth sit in the lips' shadow, which no light here casts)
        m.insert("npc_mouth", materials.add(StandardMaterial { base_color: Color::srgb(0.62, 0.6, 0.58), perceptual_roughness: 0.55, reflectance: 0.25, ..default() }));
        for (k, rough) in [("hair", 0.62), ("beard", 0.7), ("brow", 0.7), ("lash", 0.7)] {
            // not every look has every card (no beard on the women / kids)
            if std::path::Path::new(&format!("assets/npc/npc_{name}_{k}.dds")).exists() {
                m.insert(card_key(k), materials.add(card(k, rough)));
            }
        }
        let tint = |base: StandardMaterial, k: Vec3| {
            let m = base.base_color.to_srgba();
            StandardMaterial { base_color: Color::srgb(k.x * m.red, k.y * m.green, k.z * m.blue), ..base }
        };
        let cloth: Vec<_> = CLOTH_DYES.iter().map(|&k| materials.add(tint(atlas(true), k))).collect();
        let hair: Vec<_> = if m.contains_key("npc_hair") {
            HAIR_SHADES.iter().map(|&k| materials.add(tint(card("hair", 0.62), k))).collect()
        } else {
            Vec::new()
        };
        vars.push((cloth, hair));
        let mut look_imgs: Vec<Handle<Image>> = ["albedo", "normal", "orm"].iter().map(|k| tex(k)).collect();
        look_imgs.extend(["hair", "beard", "brow", "lash"].iter().filter(|k| m.contains_key(card_key(k))).map(|k| tex(k)));
        imgs.push(look_imgs);
        mats.push(m);
    }
    commands.insert_resource(NpcCast { looks: (0..LOOKS.len()).map(|_| None).collect(), gltf, private, mats, imgs, vars });
}

/// Build each look's animation graph once its GLB (and private clip set, if any) is in: clip name ->
/// graph node, a private clip replacing the committed one of the same name.
fn finish_cast(
    mut cast: ResMut<NpcCast>,
    server: Res<AssetServer>,
    gltfs: Res<Assets<Gltf>>,
    mut graphs: ResMut<Assets<AnimationGraph>>,
) {
    if cast.looks.iter().all(Option::is_some) {
        return;
    }
    for i in 0..LOOKS.len() {
        if cast.looks[i].is_some() {
            continue;
        }
        let Some(g) = gltfs.get(&cast.gltf[i]) else { continue };
        let Some(scene) = g.scenes.first().cloned() else { continue };
        let mut clips: HashMap<String, Handle<AnimationClip>> =
            g.named_animations.iter().map(|(n, h)| (n.to_string(), h.clone())).collect();
        let mut mixamo = false;
        if let Some(p) = &cast.private[i] {
            match gltfs.get(p) {
                Some(pg) => {
                    clips.extend(pg.named_animations.iter().map(|(n, h)| (n.to_string(), h.clone())));
                    mixamo = true;
                }
                // a broken private file falls back to the committed clips instead of stalling the cast
                None if server.load_state(p).is_failed() => {}
                None => continue,
            }
        }
        let mut names: Vec<_> = clips.into_iter().collect();
        names.sort_by(|a, b| a.0.cmp(&b.0));
        let (graph, idx) = AnimationGraph::from_clips(names.iter().map(|(_, h)| h.clone()));
        let nodes = names.into_iter().map(|(n, _)| n).zip(idx).collect();
        let mats = cast.mats[i].clone();
        cast.looks[i] = Some(LookAssets { scene, graph: graphs.add(graph), nodes, mats, mixamo });
    }
}

fn attach_skins(
    mut commands: Commands,
    cast: Res<NpcCast>,
    reqs: Query<(Entity, &SkinRequest, &Transform, Option<&Skinned>)>,
) {
    for (e, req, tf, had) in &reqs {
        if let Some(sk) = had {
            if sk.look == req.0 {
                continue;
            }
            // job change (`villagers::reskin_townsfolk`): drop the old outfit's instance + props
            commands.entity(sk.inst).try_despawn();
        }
        let Some(look) = cast.looks.get(req.0 as usize).and_then(Option::as_ref) else { continue };
        // Cancel the root's studio scale (kids / women / per-look build): the cast carries its own
        // real proportions, a child model is simply a shorter person.
        let s = WORLD_PER_M / tf.scale.x.max(1e-3);
        // Nobody is the same size: ±5% height, ±6% build (feet stay on the ground — the pivot).
        let h = person_hash(e);
        let tall = 0.95 + 0.1 * ((h >> 4) & 255) as f32 / 255.0;
        let broad = 0.94 + 0.12 * ((h >> 12) & 255) as f32 / 255.0;
        let child = commands
            // Hidden until `skin_ready` has swapped in the game materials: re-materialing a mesh the
            // renderer has already binned (an opaque placeholder becoming an alpha-masked hair card)
            // trips Bevy's "already binned" panic.
            .spawn((
                WorldAssetRoot(look.scene.clone()),
                Transform::from_scale(Vec3::new(s * broad, s * tall, s * broad)),
                Visibility::Hidden,
                SkinOf(e),
            ))
            .observe(skin_ready)
            .id();
        // Parent it from a world command: a root despawned this frame (a reset, a death) would make a
        // plain `add_child` panic; then the orphaned instance goes too.
        commands.queue(move |world: &mut World| {
            if world.get_entity(e).is_ok() {
                world.entity_mut(e).add_child(child);
            } else if let Ok(c) = world.get_entity_mut(child) {
                c.despawn();
            }
        });
        commands.entity(e).try_insert(Skinned {
            look: req.0,
            inst: child,
            tools_done: false,
            player: None,
            clip: "",
            last: tf.translation,
            speed: 0.0,
            feet: [None; 2],
            stride: if look.mixamo { WALK_MX } else { WALK_M }[req.0 as usize] * WORLD_PER_M * tall,
            ual: WALK_M[req.0 as usize] * WORLD_PER_M * tall,
            mx: look.mixamo,
            hand_r: None,
            hand_l: None,
            pelvis: None,
            mouth: None,
            props: Vec::new(),
            work_grip: false,
            parked: false,
            body: None,
            unseen: 0.0,
            meshes: Vec::new(),
            casting: true,
            lod_far: false,
        });
    }
}

/// glTF instance spawned: hook up the graph, swap in the game materials, find the hand bones.
fn skin_ready(
    ready: On<WorldInstanceReady>,
    mut commands: Commands,
    cast: Res<NpcCast>,
    of: Query<&SkinOf>,
    children: Query<&Children>,
    players: Query<(), With<AnimationPlayer>>,
    mat_names: Query<&GltfMaterialName>,
    names: Query<&Name>,
    morphs: Query<(), With<bevy::mesh::morph::MorphWeights>>,
    shades: Query<&HairShade>,
    mut roots: Query<&mut Skinned>,
) {
    let inst = ready.entity;
    let Ok(SkinOf(root)) = of.get(inst) else { return };
    let Ok(mut sk) = roots.get_mut(*root) else { return };
    if sk.inst != inst {
        return; // a stale outfit finishing after a job change
    }
    let Some(look) = cast.looks[sk.look as usize].as_ref() else { return };
    for d in children.iter_descendants(inst) {
        if players.contains(d) {
            commands.entity(d).insert((AnimationGraphHandle(look.graph.clone()), AnimationTransitions::new()));
            sk.player = Some(d);
            sk.parked = false; // a fresh player starts with its graph attached
        }
        if morphs.contains(d) {
            sk.mouth = Some(d);
        }
        if let Ok(mn) = mat_names.get(d) {
            // This person's own dye and hair shade (a stable pick per root entity).
            let h = person_hash(*root);
            let (cloth, hair) = &cast.vars[sk.look as usize];
            let var = match mn.0.as_str() {
                "npc_cloth" if !uniform_look(sk.look) => cloth.get((h % cloth.len() as u32) as usize),
                "npc_hair" if shades.get(*root).is_ok() => hair.get(shades.get(*root).map_or(0, |s| s.0)),
                "npc_hair" if !matches!(sk.look, ELDER_M | ELDER_F | MERCHANT | WOODCUTTER | MAUD) => hair.get(((h >> 8) % hair.len().max(1) as u32) as usize),
                _ => None,
            };
            if let Some(m) = var.or_else(|| look.mats.get(mn.0.as_str())) {
                // Frustum-culled on joint-driven bounds (`DynamicSkinnedMeshBounds`, the Bevy 0.20 glTF
                // default — a crouch / sit / swing grows the box with the pose). Was `NoFrustumCulling`,
                // which drew every townsperson within range behind the camera too, in the main pass,
                // the prepass and every shadow cascade (~6M skinned indices in the village).
                // Past the haze a townsperson is a few pixels: stop drawing them (and their shadow).
                commands.entity(d).insert((MeshMaterial3d(m.clone()), DynamicSkinnedMeshBounds, SKIN_RANGE, crate::quality::SmallDetail));
                sk.body.get_or_insert(d);
                sk.meshes.push(d);
            }
        }
        if let Ok(n) = names.get(d) {
            match n.as_str() {
                "hand_r" => sk.hand_r = Some(d),
                "hand_l" => sk.hand_l = Some(d),
                "pelvis" => sk.pelvis = Some(d),
                "foot_l" => sk.feet[0] = Some(d),
                "foot_r" => sk.feet[1] = Some(d),
                _ => {}
            }
        }
    }
    commands.entity(inst).insert(Reveal(0));
    sk.clip = "";
    sk.tools_done = false;
}

/// A skin instance with its game materials in, kept hidden until every texture they read has loaded
/// and had a few frames to reach the GPU. Bevy 0.20 re-queues a mesh left pending (material not yet
/// prepared) without checking it's still visible, so a person shown while their textures stream in
/// and then hidden for a moment (indoors, a job change) got binned twice on reappearing — the
/// intermittent "already binned" panic. Never visible while pending = never double-binned.
#[derive(Component)]
struct Reveal(u8);

fn reveal_skins(
    mut commands: Commands,
    cast: Res<NpcCast>,
    server: Res<AssetServer>,
    roots: Query<&Skinned>,
    mut q: Query<(Entity, &mut Reveal, &SkinOf)>,
) {
    for (e, mut r, of) in &mut q {
        let Ok(sk) = roots.get(of.0) else { continue };
        if r.0 == 0 && !cast.imgs[sk.look as usize].iter().all(|h| server.is_loaded(h.id()) || server.load_state(h.id()).is_failed()) {
            continue;
        }
        r.0 += 1;
        if r.0 >= 8 {
            commands.entity(e).try_remove::<Reveal>().try_insert(Visibility::Inherited);
        }
    }
}

/// The studio body under a skinned root stays (joint lookups, prop sockets) but is never drawn.
fn hide_studio_rigs(mut rigs: Query<(&ChildOf, &mut Visibility), With<BipedRig>>, skinned: Query<(), With<SkinRequest>>) {
    for (parent, mut vis) in &mut rigs {
        if skinned.contains(parent.parent()) && *vis != Visibility::Hidden {
            *vis = Visibility::Hidden;
        }
    }
}

/// A two-handed load (a log, a bundle of planks) carried by a skinned person: a child of the
/// gameplay root that [`carry_loads`] keeps glued between the skin's real hand bones every frame, so
/// the load rides the carry clip's arms (bob, sway and all) instead of floating at a fixed offset.
/// Its mesh is authored in METRES (the skin's own scale); `across` lays it across the body (a log
/// on both palms), otherwise it points the way the carrier faces.
#[derive(Component, Clone, Copy)]
pub struct CarriedLoad {
    pub across: bool,
}

/// Seat every [`CarriedLoad`] on the midpoint of its carrier's hands (+ a hair up and forward, so the
/// fingers wrap the sides rather than the load sinking into the palms), level with the carrier's
/// facing. Reads last frame's bone poses in the ROOT's own frame, so the carrier's walk never
/// skates the load.
fn carry_loads(
    roots: Query<(&Skinned, &GlobalTransform)>,
    mut loads: Query<(&ChildOf, &CarriedLoad, &mut Transform)>,
    bones: Query<&GlobalTransform>,
) {
    for (parent, load, mut tf) in &mut loads {
        let Ok((sk, root_gt)) = roots.get(parent.parent()) else { continue };
        let Some((hr, hl)) = hands(sk) else { continue };
        let (Ok(r), Ok(l)) = (bones.get(hr), bones.get(hl)) else { continue };
        let k = root_gt.scale().x.max(1e-3);
        let mid = root_gt.affine().inverse().transform_point3((r.translation() + l.translation()) * 0.5);
        let m = WORLD_PER_M / k; // metres → this root's local units
        tf.translation = mid + Vec3::new(0.0, 0.02, 0.03) * m;
        tf.rotation = if load.across { Quat::from_rotation_y(std::f32::consts::FRAC_PI_2) } else { Quat::IDENTITY };
        tf.scale = Vec3::splat(m);
    }
}

/// The skin's (right, left) hand bones once its glTF instance is ready.
pub fn hands(sk: &Skinned) -> Option<(Entity, Entity)> {
    Some((sk.hand_r?, sk.hand_l?))
}

/// Re-seat a prop authored in the studio rig's elbow frame (forearm down local -Y, fist at
/// y -0.25, forward +Z) into the matching hand bone: forearm line -> wrist->fingers, forward ->
/// through the fist pinky->index, fist -> fist.
pub fn elbow_to_hand(right: bool) -> Transform {
    let (across, _, centre) = fist(right);
    let y = -Vec3::Y;
    let rot = Quat::from_mat3(&Mat3::from_cols(y.cross(across), y, across));
    Transform { translation: centre - rot * Vec3::new(0.0, -0.25, 0.0), rotation: rot, scale: Vec3::ONE }
}

/// Fist frame in the cast's hand bones (metres; +Y wrist -> fingers, bones X-mirrored left/right),
/// measured off the MPFB `game_engine` finger joints: `across` runs through the closed fist
/// pinky -> index, `palm` points out of the palm, `centre` is the middle of the fist.
fn fist(right: bool) -> (Vec3, Vec3, Vec3) {
    let m = if right { 1.0 } else { -1.0 };
    let across = Vec3::new(0.571 * m, 0.0, -0.821);
    let palm = Vec3::new(-0.821 * m, 0.0, -0.571);
    (across, palm, Vec3::new(0.0, 0.085, 0.0) + palm * 0.03)
}

/// How a hand-held prop sits in the fist — each kind has its own hold (one generic "tool" grip
/// held swords by the blade tip with the hilt dangling, longbows by a limb tip, and swung the
/// buckler through the fist like an axe haft in the fight clip).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Held {
    /// Axe / pick / hoe (right hand): carried by the neck, head up, haft along the leg.
    Tool,
    /// Sword / scimitar / the archer's nocked arrow (right hand): held by the hilt (the origin),
    /// point DOWN along the leg at ease.
    Blade,
    /// The guard's round shield (left hand): face out from the body, upright, in every clip.
    Shield,
    /// The longbow (left hand): held at its grip (the origin), stave upright beside the leg.
    Bow,
}

/// Which hold a copied studio-rig prop takes, from the look (right hand = the `Sword` pivot).
fn held_for(look: u8, right: bool) -> Held {
    match (look, right) {
        (GUARD | ARCHER, true) => Held::Blade,
        (GUARD, false) => Held::Shield,
        (ARCHER, false) => Held::Bow,
        _ => Held::Tool,
    }
}

/// Where a studio prop (`peasant_model::kit_tools`: grip at the origin, haft / blade / stave along
/// +Y, edge toward +X, 1 unit ~ 1 m; the shield a disc facing +Z yawed 1.15 rad) sits in the fist.
/// `work`: a tool / blade / bow runs through the fist pinky -> index (chopping, hoeing, fighting,
/// drawing); otherwise each is carried its own way (see [`Held`]; `head` = grip -> head distance).
/// The fighting grip in a hand bone (a prop's +Y along the haft / blade, its origin in the fist) --
/// for a scene that puts its own weapon in someone's hand.
pub fn fist_grip(right: bool) -> Transform {
    grip(Held::Blade, right, true, 0.0)
}

fn grip(held: Held, right: bool, work: bool, head: f32) -> Transform {
    let (across, palm, centre) = fist(right);
    if held == Held::Shield {
        // Strapped on: face out from the body (the back of the hand), rim up the forearm, the boss
        // just outside the knuckles — never swung through the fist.
        let n0 = Quat::from_rotation_y(1.15) * Vec3::Z; // the mesh's face normal
        let from = Mat3::from_cols(Vec3::Y.cross(n0).normalize(), Vec3::Y, n0);
        let (up, out) = (Vec3::NEG_Y, -palm);
        let to = Mat3::from_cols(up.cross(out).normalize(), up, out);
        let rot = Quat::from_mat3(&(to * from.transpose()));
        return Transform { translation: centre - palm * 0.07, rotation: rot, scale: Vec3::ONE };
    }
    if work {
        // a real hammer grip: the haft runs diagonally from the heel of the hand out past the
        // index knuckle, so the head leans toward the fingers; the edge leads along the knuckles
        let haft = (across + Vec3::Y * 0.55).normalize();
        let edge = (Vec3::Y - haft * haft.y).normalize() * if right { 1.0 } else { -1.0 };
        let rot = Quat::from_mat3(&Mat3::from_cols(edge, haft, edge.cross(haft)));
        return Transform { translation: centre, rotation: rot, scale: Vec3::ONE };
    }
    // Tool: haft (+Y tool) toward the wrist (-Y hand, i.e. head up when the arm hangs), the fist
    // just under the head. Blade: held by the hilt, point toward the fingers (down the leg). Bow:
    // held at its grip, stave along the forearm line.
    let (haft, hold) = match held {
        Held::Blade => (Vec3::Y, 0.0),
        Held::Bow => (Vec3::NEG_Y, 0.0),
        _ => (Vec3::NEG_Y, (head - 0.12).max(0.0)),
    };
    let edge = -palm.cross(haft).normalize() * if right { 1.0 } else { -1.0 };
    let rot = Quat::from_mat3(&Mat3::from_cols(edge, haft, edge.cross(haft)));
    Transform { translation: centre - haft * hold, rotation: rot, scale: Vec3::ONE }
}

fn top_y(m: &Mesh) -> f32 {
    match m.attribute(Mesh::ATTRIBUTE_POSITION) {
        Some(bevy::mesh::VertexAttributeValues::Float32x3(p)) => p.iter().map(|v| v[1]).fold(0.0, f32::max),
        _ => 0.0,
    }
}

/// Put the studio rig's held tool (`Sword` pivot) / off-hand prop (`Shield` pivot) into the skin's
/// hands: same meshes + creature material, so trade outfits keep their exact tools.
fn copy_tools(
    mut commands: Commands,
    mut roots: Query<(Entity, &mut Skinned)>,
    parts: Query<(&crate::biped::BipedPart, &Children)>,
    leaves: Query<(&Mesh3d, &MeshMaterial3d<crate::creature::CreatureMaterial>)>,
    rigs: Query<(), With<BipedRig>>,
    kids: Query<&Children>,
    meshes: Res<Assets<Mesh>>,
) {
    for (root, mut sk) in &mut roots {
        if sk.tools_done {
            continue;
        }
        let (Some(hr), Some(hl)) = (sk.hand_r, sk.hand_l) else { continue };
        // wait for the (queued) studio rig so a tool-less outfit is told apart from a late one
        if !kids.get(root).is_ok_and(|c| c.iter().any(|k| rigs.contains(k))) {
            continue;
        }
        for (part, children) in &parts {
            if part.root != root {
                continue;
            }
            let right = match part.joint {
                crate::player::Joint::Sword => true,
                crate::player::Joint::Shield => false,
                _ => continue,
            };
            for c in children.iter() {
                if let Ok((mesh, mat)) = leaves.get(c) {
                    let head = meshes.get(&mesh.0).map_or(0.0, top_y);
                    let held = held_for(sk.look, right);
                    let prop = commands.spawn((mesh.clone(), mat.clone(), grip(held, right, false, head))).id();
                    commands.entity(if right { hr } else { hl }).add_child(prop);
                    sk.props.push((prop, held, head));
                }
            }
        }
        sk.tools_done = true;
        sk.clip = ""; // re-apply the grip for the current clip
    }
}

/// Pick + cross-fade each skin's clip from its drive and real ground speed.
fn drive_skins(
    mut commands: Commands,
    time: Res<Time>,
    cam: Query<(&GlobalTransform, &VisibleEntities), With<Camera3d>>,
    cast: Res<NpcCast>,
    mut roots: Query<(&mut Skinned, &BipedDrive, &GlobalTransform, Has<crate::dying::Dying>, Option<&SkinClip>, Option<&ClipSeek>)>,
    mut players: Query<(&mut AnimationPlayer, &mut AnimationTransitions)>,
    mut props: Query<&mut Transform>,
    mut prop_vis: Query<&mut Visibility>,
    mut mesh3d: Query<&mut Mesh3d>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut lod_cache: ResMut<crate::anim_lod::SkinLodCache>,
) {
    let dt = time.delta_secs();
    if dt <= 0.0 {
        return; // paused / hit-stop: hold the pose
    }
    let cam_p = cam.iter().next().map(|(g, _)| g.translation());
    let on_screen = crate::anim_lod::on_screen_set(cam.iter().next().map(|(_, v)| v));
    for (mut sk, d, gt, dying, hint, seek) in &mut roots {
        let Some(pe) = sk.player else { continue };
        let Ok((mut player, mut tr)) = players.get_mut(pe) else { continue };
        let p = gt.translation();
        let v = Vec2::new(p.x - sk.last.x, p.z - sk.last.z).length() / dt;
        sk.last = p;
        sk.speed += (v.min(8.0) - sk.speed) * (dt * 8.0).min(1.0);
        let d2 = cam_p.map_or(0.0, |c| c.distance_squared(p));
        let far = d2 > if sk.parked { ANIM_WAKE2 } else { ANIM_CULL2 };
        let (body, mut unseen) = (sk.body, sk.unseen);
        let far = crate::anim_lod::park(far, d2, body, on_screen, &mut unseen, dt);
        sk.unseen = unseen;
        if let Some(cast) = crate::anim_lod::shadow_flip(sk.casting, d2) {
            sk.casting = cast;
            crate::anim_lod::set_casting(&mut commands, &sk.meshes, cast);
        }
        if let Some(lf) = crate::anim_lod::lod_flip(sk.lod_far, d2) {
            sk.lod_far = lf;
            crate::anim_lod::swap_meshes(&sk.meshes, lf, &mut mesh3d, &mut meshes, &mut lod_cache);
        }
        if far != sk.parked {
            sk.parked = far;
            if far {
                commands.entity(pe).try_remove::<AnimationGraphHandle>();
            } else if let Some(look) = cast.looks[sk.look as usize].as_ref() {
                commands.entity(pe).try_insert(AnimationGraphHandle(look.graph.clone()));
            }
        }
        // Legs follow the body's REAL ground speed (not the AI's intent), with hysteresis so a
        // shuffle to a stop doesn't flicker walk/idle: no stepping on the spot, no gliding off.
        let stepping = sk.loco_native(sk.clip).is_some();
        let moving = sk.speed > if stepping { 0.12 } else { 0.3 };
        let stride = sk.stride;
        let rate_of = |clip: &str| sk.loco_native(clip).map_or(1.0, |n| (sk.speed / n).clamp(0.4, 2.2));
        let jog_native = sk.loco_native("Jog_Fwd_Loop").unwrap_or(stride * JOG_K);
        let elder = sk.mx && matches!(sk.look, ELDER_M | ELDER_F);
        // the Mixamo set's idles take turns per person (~20 s each): weight shifts, arms folded,
        // a bored sway — nobody stands frozen in one loop all day
        let idle = if !sk.mx {
            "Idle_Loop"
        } else if elder {
            "Old_Idle_Loop"
        } else {
            let turn = (time.elapsed_secs() / 21.0) as u32 + person_hash(pe) % 7;
            // (no "Nervously Look Around": its hunched, darting crouch read as a townsperson cowering
            // or lying in wait, not standing about)
            ["Idle_Loop", "Idle_Loop", "Idle_FoldArms_Loop", "Idle_Loop", "Idle_Bored_Loop"][(turn % 5) as usize]
        };
        let (clip, once, rate): (&'static str, bool, f32) = if dying {
            ("Death01", true, 1.0)
        } else if let Some(h) = hint.filter(|h| !h.0.is_empty() && !d.attacking) {
            // a scripted walk / haul clip is paced to the actor's real speed like any other
            let rate = if moving { rate_of(h.0) } else { 1.0 };
            (h.0, h.0.ends_with("_Enter") || h.0.ends_with("_Exit"), rate)
        } else if d.sitting {
            ("Sitting_Idle_Loop", false, 1.0)
        } else if d.attacking {
            ("Sword_Regular_A", false, 1.3)
        } else if d.bow {
            ("Spell_Simple_Shoot", false, 1.0)
        } else if d.carrying && moving {
            let c = if d.pulling { "Push_Loop" } else { "Walk_Carry_Loop" };
            (c, false, rate_of(c))
        } else if moving && (sk.speed > stride * 2.2 || (d.run_amt > 0.5 && sk.speed > stride * 1.3)) {
            // Grown-ups WALK — a brisk pace is a quicker walk, never a trot (a woman jogging up and
            // stopping dead read as absurd). Only a real hurry (a muster, past ~2.2 strides) or a
            // runner by nature (`run_amt`: kids at play, a flight) breaks into the run.
            // a run is never played much below ~half its pace (the UAL jog is a long-flight run)
            ("Jog_Fwd_Loop", false, (sk.speed / jog_native).clamp(0.5, 1.5))
        } else if moving && elder && sk.speed < stride * if sk.clip == "Sad_Walk_Loop" { 1.05 } else { 0.95 } {
            // the old folk's stooped, shorter-stepped walk while they amble
            ("Sad_Walk_Loop", false, rate_of("Sad_Walk_Loop"))
        } else if moving {
            ("Walk_Loop", false, rate_of("Walk_Loop"))
        } else if d.work == 2 {
            ("TreeChopping_Loop", false, 1.0)
        } else if d.work == 1 {
            ("Farm_Harvest", false, 1.0)
        } else {
            (idle, false, 1.0)
        };
        let Some(look) = cast.looks[sk.look as usize].as_ref() else { continue };
        // A scripted / life-act clip (`SkinClip`) puts its own prop in the hands (a hammer, a plank
        // bundle…): the trade tool carried by the outfit would hang there wrong (a hoe held upside
        // down through a kneel), so it is hidden for the act and comes back with the work clips.
        // (a scripted sword clip -- a cutscene's fight -- keeps the sword and shield in hand)
        let acting = !dying && !d.attacking && hint.is_some_and(|h| !h.0.is_empty() && !h.0.starts_with("Sword_"));
        let want_vis = if acting { Visibility::Hidden } else { Visibility::Inherited };
        for &(pe, _, _) in &sk.props {
            if let Ok(mut v) = prop_vis.get_mut(pe) {
                if *v != want_vis {
                    *v = want_vis;
                }
            }
        }
        if clip != sk.clip {
            let Some(&node) = look.nodes.get(clip) else { continue };
            let first = sk.clip.is_empty();
            let a = tr.play(&mut player, node, if first { Duration::ZERO } else { FADE });
            if !once {
                a.repeat();
            }
            if first {
                // de-sync neighbours on the same loop
                let h = (pe.index_u32().wrapping_mul(2_654_435_761) >> 16) as f32 / 65_536.0;
                a.seek_to(h * 2.0);
            }
            sk.clip = clip;
            // two-handed work / fight clips grip the haft through the fist, the rest carry it
            let work = matches!(clip, "TreeChopping_Loop" | "Farm_Harvest" | "Spell_Simple_Shoot") || clip.starts_with("Sword_");
            if work != sk.work_grip || first {
                sk.work_grip = work;
                for &(pe, held, head) in &sk.props {
                    if let Ok(mut t) = props.get_mut(pe) {
                        let right = !matches!(held, Held::Shield | Held::Bow);
                        // only the bow draw puts the stave through the fist; a sword swing keeps the
                        // bow / shield hand as it is
                        let w = work && (held != Held::Bow || clip == "Spell_Simple_Shoot");
                        *t = grip(held, right, w, head);
                    }
                }
            }
        }
        for (_, a) in player.playing_animations_mut() {
            a.set_speed(if far { 0.0 } else { rate });
        }
        // A director scrubbing the scripted clip: its playhead is set, not played (the outgoing
        // clip of a cross-fade keeps running out underneath).
        if let (Some(&ClipSeek(at)), Some(&node)) = (seek, look.nodes.get(sk.clip)) {
            if let Some(a) = player.animation_mut(node) {
                a.seek_to(at).set_speed(0.0);
            }
        }
    }
}

/// `FOREST_GAITCHECK=1` (+`_SECS`, default 40): the foot-skate meter. For every skin playing a
/// locomotion clip near the camera, the planted (lower) foot's world ground speed over the body's
/// own — 0 = the foot is glued to the ground, 1 = it glides along with the body. Logs the median
/// per clip, then exits.
fn gait_check(
    time: Res<Time>,
    cam: Query<&GlobalTransform, With<Camera3d>>,
    q: Query<(Entity, &Skinned)>,
    joints: Query<&GlobalTransform>,
    mut last: Local<std::collections::HashMap<Entity, [Vec3; 2]>>,
    mut acc: Local<std::collections::HashMap<&'static str, Vec<f32>>>,
    mut t0: Local<Option<f32>>,
    mut exit: MessageWriter<AppExit>,
) {
    let dt = time.delta_secs();
    let now = time.elapsed_secs();
    let secs = std::env::var("FOREST_GAITCHECK_SECS").ok().and_then(|s| s.parse().ok()).unwrap_or(40.0);
    let Some(c) = cam.iter().next().map(|g| g.translation()) else { return };
    if dt <= 0.0 {
        return;
    }
    // the clock starts at the first frame with skins loaded and walking (boot / loading excluded)
    if t0.is_none() && q.iter().any(|(_, sk)| sk.feet[0].is_some() && sk.speed > 0.3) {
        *t0 = Some(now + 5.0);
    }
    let Some(start) = *t0 else {
        if now > 240.0 {
            info!("GAITCHECK: no walking skin in 240 s");
            exit.write(AppExit::Success);
        }
        return;
    };
    if now < start {
        return;
    }
    for (e, sk) in &q {
        let ([Some(a), Some(b)], Some(_)) = (sk.feet, sk.loco_native(sk.clip)) else {
            last.remove(&e);
            continue;
        };
        let (Ok(fa), Ok(fb)) = (joints.get(a), joints.get(b)) else { continue };
        let p = [fa.translation(), fb.translation()];
        if p[0].distance_squared(c) > ANIM_CULL2 * 0.8 {
            last.remove(&e);
            continue;
        }
        if let Some(prev) = last.insert(e, p) {
            if sk.speed > 0.3 {
                let lo = if p[0].y < p[1].y { 0 } else { 1 };
                let v = Vec2::new(p[lo].x - prev[lo].x, p[lo].z - prev[lo].z).length() / dt;
                acc.entry(sk.clip).or_default().push(v / sk.speed);
            }
        }
    }
    if now > start + secs {
        for (clip, v) in acc.iter_mut() {
            v.sort_by(|a, b| a.total_cmp(b));
            let q = |f: f32| v[((v.len() - 1) as f32 * f) as usize];
            info!("GAITCHECK {clip}: n {} planted-foot slip median {:.2} (p25 {:.2}, p75 {:.2})", v.len(), q(0.5), q(0.25), q(0.75));
        }
        exit.write(AppExit::Success);
    }
}