//! Shared creature material: an `ExtendedMaterial<StandardMaterial, CreatureExt>` that all
//! animated rigs (hero, orks, wildlife) draw against — replacing their plain white
//! `StandardMaterial`s. Hue still lives in `ATTRIBUTE_COLOR.rgb`; the **alpha** channel now
//! carries a per-vertex SURFACE CODE that `assets/shaders/creature.wgsl` reads to apply a
//! subtle procedural texture (fur/scale/stone/metal/hide/bone/cloth) in model space, plus a
//! per-surface roughness/spec response. Props/trees/scatter keep their own white material —
//! this is creatures only.

use bevy::pbr::{ExtendedMaterial, MaterialExtension, MaterialPlugin};
use bevy::prelude::*;
use bevy::render::render_resource::{AsBindGroup, ShaderType};
use bevy::shader::ShaderRef;

const CREATURE_SHADER: &str = "shaders/creature.wgsl";

pub type CreatureMaterial = ExtendedMaterial<StandardMaterial, CreatureExt>;

#[derive(Clone, Copy, ShaderType, Debug)]
pub struct CreatureParams {
    /// x = texture strength (luminance ±), y = micro-relief (normal perturb),
    /// z = metal spec lift, w = spare.
    pub params: Vec4,
}

#[derive(Asset, AsBindGroup, Clone, TypePath, Debug)]
pub struct CreatureExt {
    #[uniform(100)]
    pub params: CreatureParams,
}

impl MaterialExtension for CreatureExt {
    fn fragment_shader() -> ShaderRef {
        CREATURE_SHADER.into()
    }
}

/// Surface family a mesh primitive reads as. Packed into the vertex-colour alpha so the shader
/// can branch its procedural texture. `Skin` is the neutral default (≈ the old flat look).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Surf {
    Skin,
    Fur,
    Scale,
    Stone,
    Metal,
    Cloth,
    Bone,
}

/// The alpha value (`0..1`) that encodes a surface family. Mid-band values so the shader can
/// read it with a tolerant bucket and never sit on a band edge.
pub fn surf_code(s: Surf) -> f32 {
    match s {
        Surf::Skin => 0.07,
        Surf::Fur => 0.21,
        Surf::Scale => 0.36,
        Surf::Stone => 0.50,
        Surf::Metal => 0.64,
        Surf::Cloth => 0.79,
        Surf::Bone => 0.93,
    }
}

/// Rewrite every vertex's colour-alpha of `mesh` to the surface code for `s`, leaving rgb (the
/// hue) untouched. Call AFTER the mesh has its `ATTRIBUTE_COLOR` set and BEFORE it is merged
/// into a group (merge concatenates the attribute, so tagging per-part-primitive then merging
/// preserves per-primitive surfaces). No-op if the mesh has no colour attribute.
pub fn surf(mut mesh: Mesh, s: Surf) -> Mesh {
    use bevy::mesh::VertexAttributeValues as V;
    let code = surf_code(s);
    if let Some(V::Float32x4(cols)) = mesh.attribute_mut(Mesh::ATTRIBUTE_COLOR) {
        for c in cols.iter_mut() {
            c[3] = code;
        }
    }
    mesh
}

/// Build the shared creature material. All rigs call this; combat_fx then clones it per-entity
/// for the hurt-flash (unchanged behaviour, now on this type).
pub fn make_creature_material(mats: &mut Assets<CreatureMaterial>) -> Handle<CreatureMaterial> {
    mats.add(ExtendedMaterial {
        base: StandardMaterial {
            base_color: Color::WHITE, // vertex colour rgb carries the hue
            perceptual_roughness: 0.7, // per-surface response is applied in the shader
            ..default()
        },
        extension: CreatureExt {
            // strength 0.22 (subtle-moderate grain), relief 0.25, spec-lift 0.35.
            params: CreatureParams { params: Vec4::new(0.22, 0.25, 0.35, 0.0) },
        },
    })
}

pub struct CreaturePlugin;

impl Plugin for CreaturePlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins(MaterialPlugin::<CreatureMaterial>::default());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn surf_codes_are_distinct_and_in_range() {
        let all = [
            Surf::Skin,
            Surf::Fur,
            Surf::Scale,
            Surf::Stone,
            Surf::Metal,
            Surf::Cloth,
            Surf::Bone,
        ];
        let mut codes: Vec<f32> = all.iter().map(|s| surf_code(*s)).collect();
        for c in &codes {
            assert!(*c > 0.0 && *c < 1.0, "code {c} out of (0,1)");
        }
        codes.sort_by(|a, b| a.partial_cmp(b).unwrap());
        for w in codes.windows(2) {
            assert!(w[1] - w[0] > 0.08, "codes {:?} too close to bucket apart", w);
        }
    }

    /// The load-bearing contract: each `surf_code` must decode back to its own family under the
    /// shader's bucket thresholds. `classify` mirrors the if-ladder in `assets/shaders/creature.wgsl`
    /// — keep the two in sync. Without this, retuning a code (or adding a surface) silently
    /// mis-textures vertices with no compile error.
    #[test]
    fn surf_codes_land_in_their_shader_band() {
        // Mirror of creature.wgsl's decode ladder (surf<0.14||surf>0.965 → Skin, then ascending).
        fn classify(surf: f32) -> Surf {
            if surf < 0.14 || surf > 0.965 {
                Surf::Skin
            } else if surf < 0.28 {
                Surf::Fur
            } else if surf < 0.43 {
                Surf::Scale
            } else if surf < 0.57 {
                Surf::Stone
            } else if surf < 0.71 {
                Surf::Metal
            } else if surf < 0.86 {
                Surf::Cloth
            } else {
                Surf::Bone
            }
        }
        for s in [Surf::Skin, Surf::Fur, Surf::Scale, Surf::Stone, Surf::Metal, Surf::Cloth, Surf::Bone] {
            assert_eq!(classify(surf_code(s)), s, "surf_code({s:?}) decodes to the wrong shader band");
        }
    }

    #[test]
    fn surf_rewrites_only_alpha() {
        let mut m = bevy::prelude::Mesh::from(bevy::math::primitives::Cuboid::new(1.0, 1.0, 1.0));
        let n = m.count_vertices();
        m.insert_attribute(Mesh::ATTRIBUTE_COLOR, vec![[0.2, 0.4, 0.6, 1.0]; n]);
        let m = surf(m, Surf::Metal);
        if let Some(bevy::mesh::VertexAttributeValues::Float32x4(cols)) =
            m.attribute(Mesh::ATTRIBUTE_COLOR)
        {
            for c in cols {
                assert_eq!(&c[0..3], &[0.2, 0.4, 0.6]); // hue untouched
                assert!((c[3] - surf_code(Surf::Metal)).abs() < 1e-6);
            }
        } else {
            panic!("color attribute lost");
        }
    }
}
