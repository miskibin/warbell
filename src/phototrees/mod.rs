//! **EXPERIMENTAL photoreal trees** — `FOREST_PHOTOTREES=1`.
//!
//! Swaps the game's faceted low-poly trees for procedurally-grown ones with photographic leaf
//! sprigs, ported from the sibling `bevy-world-editor` project. Off by default; when off this
//! module builds nothing at all (no atlas, no meshes, no material) and costs one env read.
//!
//! ## What was ported, and what was deliberately not
//!
//! Ported: the Weber-Penn-style skeleton generator (`gen/`, vendored verbatim — pure, zero-dep),
//! the CPU mesh builders (`mesh.rs`), and the foliage atlas (`atlas.rs`).
//!
//! **NOT ported: the whole LOD/streaming machinery, and both wind shaders.** Those omissions are
//! the design, not shortcuts:
//!
//! - **No `leaves.wgsl` / `leaves_prepass.wgsl`.** Upstream sways leaves in a vertex shader, which
//!   forces three fragile invariants: the main and prepass sway math must stay *byte-identical* or
//!   the depth-equal test discards every leaf pixel ("frosted trees"); the prepass must write
//!   `visibility_range_dither` or the LOD band does the same thing locally; and the sway amplitude
//!   is a hardcoded absolute `0.055` world units, which on 4 m trees instead of 20 m ones reads ~5×
//!   too strong. This game already sways trees on the CPU via `wind::Sway` rotating the whole
//!   transform, so all three problems simply vanish by using a plain `StandardMaterial`.
//!   It also dodges a real latent bug: upstream's prepass shader never writes
//!   `previous_world_position`, which is harmless there (depth+normal prepass only) but this game
//!   runs a **MotionVector** prepass — dropping it in as-is would have produced garbage motion
//!   vectors on every leaf.
//! - **No per-tree LOD tiers.** Upstream affords 4 entities per tree only inside a streamed 360 m
//!   radius; this game spawns all ~15–20k trees permanently up front, where that shape measured
//!   102 ms/frame upstream. See `mesh::build_tree` for the one-mesh-one-material trick that keeps
//!   the existing one-entity-per-tree contract instead. Distance culling is the game's existing
//!   abrupt 180u `VisibilityRange`, unchanged.
//! - **No bark JPGs.** Trunks sample the atlas's procedural bark strip, so nothing is downloaded or
//!   shipped. Upstream's photographic bark also carries normal maps that are a silent no-op there
//!   (the tube meshes have no `ATTRIBUTE_TANGENT`), so the real loss is only plate/fissure detail.
//!
//! ## Asset licence
//!
//! `assets/textures/leaves/{pineL,oakL,aspenL}.png` are leaf-sprig cutouts from
//! **dgreenheck/ez-tree, MIT licensed**. Keep this attribution with them. (`pineL` serves both Pine
//! and Spruce upstream; Spruce is differentiated by vertex tint only.)
//!
//! ## VERDICT (2026-07-27): evaluated over two passes — it looks WORSE than the low-poly art.
//!
//! Kept behind the flag, off by default, because the port itself is sound and the vendored
//! generator is reusable — but **do not enable this expecting an improvement.** Measured against a
//! low-poly control on genuinely fixed cameras (sky-region diff 0.0005 between pairs, i.e. identical
//! lighting and grade, only the trees differing):
//!
//! | | photo | low-poly | faceted bushes |
//! |---|---|---|---|
//! | crown hue | 45.9° | 68.7° | 66–69° |
//! | warm pixels (H<50) | 62.8% | 1.2% | 10% |
//! | canopy mass vs control | 77% (upper crown 64%) | — | — |
//!
//! 88% of photo tree pixels fall below 60° hue against 85% of low-poly ones in 60–80°: the trees
//! are a different colour family from every other green in the game.
//!
//! **What is fixable, and the two traps that make it harder than it looks:**
//! 1. `assets/textures/leaves/aspenL.png` — the **Birch** sprig — is an **autumn aspen photograph**
//!    (mean hue 39.9°, *100%* of opaque pixels warm). Birch is 2/7 of `SPECIES_MIX`, so ~29% of all
//!    trees read as autumn no matter how they are tinted. Upstream ships an unused `ashL.png` that
//!    may be a greener substitute; the robust fix is to treat sprigs as luminance masks and colour
//!    them procedurally.
//! 2. **Vertex-colour tints multiply in LINEAR space, not sRGB.** The `mesh::foliage_tint` re-grade
//!    was solved in sRGB and therefore under-delivered badly — birch var0 was designed at ~6% warm
//!    and actually lands at 64.6%. Any future tint pass must be computed in linear space.
//!
//! **What is NOT fixable by parameters** (and is why this verdict is unlikely to move): the whole
//! game's look is hard flat-shaded facet edges — orks, peasants, buildings, terraced terrain, bushes,
//! rocks. Photographic leaf noise has no facets. Density and colour were the tunable axes; the
//! shading-language mismatch is inherent to the approach.
//!
//! **Presentation gap if you do enable it:** the swap happens ONLY in `biome::scatter_region`.
//! `meadow.rs` (the castle clearing — the area the player sees most), `ork_fortress.rs`,
//! `rts/deposits.rs` and the orchard all still build faceted `trees::build_tree_mesh` output, so
//! frames show both styles side by side. Fix those call sites before judging it again.
//!
//! **Cost:** ~1596 verts/tree, which is actually CHEAPER than the faceted broadleaf it replaces
//! (~2350 verts — `duplicate_vertices` forces 3 verts/tri for flat shading). Geometry is not the
//! constraint. The unmeasured risk is overdraw: `AlphaMode::Mask` + `double_sided` +
//! `cull_mode: None` on thousands of interleaved cards, shaded through the Depth/Normal/MotionVector
//! prepass, SSAO and every shadow cascade. Get a `FOREST_PERFTEST` number before trusting the
//! vertex count as the budget.

pub mod atlas;
pub mod treegen;
pub mod mesh;

use bevy::prelude::*;

use treegen::tree::{Species, ALL_SPECIES};

/// Mesh variants generated per species (matches upstream's `VARIANTS`).
const VARIANTS: usize = 4;

/// Is the experimental mode on? Read once — the world is built at `Startup`, so flipping this
/// mid-process could not take effect anyway.
pub fn enabled() -> bool {
    use std::sync::OnceLock;
    static ON: OnceLock<bool> = OnceLock::new();
    *ON.get_or_init(|| std::env::var("FOREST_PHOTOTREES").is_ok())
}

/// Built assets, reachable from anywhere via [`get`].
///
/// A `OnceLock` rather than a Bevy `Resource` on purpose: the only consumer is
/// `biome::scatter_region`, which is called deep inside `worldmap`'s `Startup` build chain
/// (`build` → `bs_scatter_biome` → `scatter_region`). Passing a `Res` down would mean adding a
/// parameter to every link of that chain — a wide, permanent diff in load-bearing world-gen code
/// for a feature that is off by default. `Handle<T>` is `Send + Sync + Clone`, so a write-once
/// global is safe here, and it keeps the experiment's footprint to a single read.
static ASSETS: std::sync::OnceLock<PhotoTrees> = std::sync::OnceLock::new();

/// The built photo-tree assets, or `None` when the mode is off (or before `PreStartup` ran).
pub fn get() -> Option<&'static PhotoTrees> {
    ASSETS.get()
}

/// The generated tree meshes + the one shared atlas material. Absent unless [`enabled`].
pub struct PhotoTrees {
    /// Indexed `[species_index][variant]`. One merged mesh each — trunk and canopy together.
    pub meshes: Vec<Vec<Handle<Mesh>>>,
    /// The single atlas material every photo tree shares, so they all batch.
    pub mat: Handle<StandardMaterial>,
}

impl PhotoTrees {
    /// Pick a (species, variant) from a uniform roll in `[0, 1)`. Callers pass a draw from the
    /// scatter's existing per-tile RNG, so placement stays deterministic and unchanged.
    pub fn pick(&self, roll: f32) -> (Handle<Mesh>, Handle<StandardMaterial>) {
        let n = SPECIES_MIX.len();
        let i = ((roll * n as f32) as usize).min(n - 1);
        let sp = SPECIES_MIX[i];
        // Second decorrelated draw for the variant, so species and variant don't move in lockstep.
        let var = ((roll * 977.0) as usize) % VARIANTS;
        let si = mesh::species_index(sp);
        (self.meshes[si][var].clone(), self.mat.clone())
    }
}

/// Species drawn from, weighted by repetition. Spruce appears once against the others' two because
/// it is by far the heaviest mesh (see `mesh::detail`); Pine and Broadleaf carry the mix.
const SPECIES_MIX: [Species; 7] = [
    Species::Pine,
    Species::Pine,
    Species::Broadleaf,
    Species::Broadleaf,
    Species::Birch,
    Species::Birch,
    Species::Spruce,
];

pub struct PhotoTreesPlugin;

impl Plugin for PhotoTreesPlugin {
    fn build(&self, app: &mut App) {
        if !enabled() {
            return;
        }
        // Must land before the world build populates the scatter — `biome::scatter_region` reads
        // the resource when spawning each tree.
        app.add_systems(PreStartup, build_assets);
    }
}

fn build_assets(
    mut meshes: ResMut<Assets<Mesh>>,
    mut mats: ResMut<Assets<StandardMaterial>>,
    mut images: ResMut<Assets<Image>>,
) {
    let t0 = std::time::Instant::now();
    let atlas_img = images.add(atlas::build_atlas());

    let mat = mats.add(StandardMaterial {
        base_color: Color::WHITE, // per-species/variant tint rides in ATTRIBUTE_COLOR
        base_color_texture: Some(atlas_img),
        // Mask, never Blend: alpha-blended foliage needs back-to-front sorting that thousands of
        // interleaved crowns cannot provide. 0.33 per upstream — a higher cutoff erodes the mip'd
        // leaf silhouette at distance, which is what the atlas's coverage-rescaled mips fight.
        alpha_mode: AlphaMode::Mask(0.33),
        perceptual_roughness: 0.9,
        reflectance: 0.12,
        // The cards are flat quads seen from both sides, so unlike the game's other foliage these
        // genuinely DO need back faces (contrast `trees::foliage_material`, where transmission
        // alone needed no such thing).
        double_sided: true,
        cull_mode: None,
        // Same cue as `trees::foliage_material`, at upstream's stronger value: cards lit from
        // behind glow instead of going black.
        diffuse_transmission: 0.4,
        // Upstream leaves `metallic` at its 0.5 default while feeding a greyscale roughness map
        // into `metallic_roughness_texture`, so its bark renders ~0.3–0.45 metallic — plasticky.
        // We bind no such map, but pin this anyway so bark and leaves read as dielectric.
        metallic: 0.0,
        ..default()
    });

    let mut by_species = Vec::with_capacity(ALL_SPECIES.len());
    let mut total_verts = 0usize;
    for sp in ALL_SPECIES {
        let mut vars = Vec::with_capacity(VARIANTS);
        for var in 0..VARIANTS {
            let m = mesh::build_tree(sp, var);
            total_verts += m.count_vertices();
            vars.push(meshes.add(m));
        }
        by_species.push(vars);
    }

    info!(
        "phototrees: {} meshes, {} verts total (avg {}/tree), built in {:?}",
        ALL_SPECIES.len() * VARIANTS,
        total_verts,
        total_verts / (ALL_SPECIES.len() * VARIANTS),
        t0.elapsed()
    );
    let _ = ASSETS.set(PhotoTrees { meshes: by_species, mat });
}
