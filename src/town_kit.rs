//! The castle town kit (zamek v2, phase 3d — `docs/superpowers/specs/2026-10-11-zamek-v2-design.md`
//! §3/§4): granary on staddle stones, smithy, the tavern "The Drowned Squire", Maud's house with
//! the edict board, infirmary + herb garden, the courtyard fire pit, the empty grave of the drowned
//! squire, the Crown set (banners, painted crates, tent, gallows, depot) and four timber-frame
//! houses; for spec v3 §2 the podgrodzie kit (two narrow street-gable houses, one a shop with a
//! fold-down counter; wattle / picket fence modules 3 u + gates, garden, woodpile, shed, coop,
//! privy, dung heap, hay rick) and four lean-tos for inside the walls (stable, barracks, store,
//! cart shed: mono-pitch, back flat at local -Z to butt onto the curtain). Built by
//! `tools/blender_forest/town_kit.py` → `town_kit.glb` (node names, footprints and heights are
//! listed in that script's docstring).
//!
//! Every piece has its pivot on the ground under the centre of its footprint with the door / front
//! toward local +Z (`crown_banner_wall` hangs down from its bracket at the origin instead), so a
//! layout only needs [`spawn_piece`] with a ground-snapped transform. The castle layout itself is
//! placed elsewhere (`castle.rs` / `market.rs`, phase 4); this module only carries the handle and a
//! screenshot showcase: `FOREST_TOWNKIT_SHOWCASE=1` lays every piece out on the open ground north of
//! the castle (see [`SHOWCASE`]); park the hero beside one with `FOREST_HERO="x,z"` for scale.

use bevy::camera::visibility::VisibilityRange;
use bevy::prelude::*;

use std::f32::consts::{FRAC_PI_2, PI};

use crate::biome::BiomeEntity;

pub struct TownKitPlugin;

impl Plugin for TownKitPlugin {
    fn build(&self, app: &mut App) {
        if std::env::var("FOREST_TOWNKIT_SHOWCASE").is_ok_and(|v| v != "0") {
            app.add_systems(Update, spawn_showcase);
        }
    }
}

/// The pieces `town_kit.glb` carries, in the script's priority order.
pub const PIECES: [&str; 89] = [
    "town_hut_0",
    "town_hut_1",
    "town_hut_2",
    "town_hut_3",
    "town_hut_4",
    "town_hut_5",
    "town_hut_6",
    "town_hut_7",
    "town_hut_8",
    "town_hut_9",
    "town_hut_10",
    "town_hut_11",
    "town_granary",
    "town_smithy",
    "town_tavern",
    "town_maud_house",
    "town_infirmary",
    "town_firepit",
    "town_grave",
    "crown_banner_wall",
    "crown_banner_pole",
    "crown_crates",
    "crown_tent",
    "crown_gallows",
    "crown_depot",
    "town_house_a",
    "town_house_b",
    "town_house_c",
    "town_house_d",
    "town_carpenter",
    "town_archery",
    // v3 (spec §2): the podgrodzie outside the W gate (street-gable houses on burgage plots +
    // plot dressing) and the lean-tos against the curtain (back flat at local -Z).
    "town_street_house_a",
    "town_street_house_b",
    "town_fence_wattle",
    "town_fence_wattle_gate",
    "town_fence_picket",
    "town_fence_picket_gate",
    "town_garden",
    "town_woodpile",
    "town_shed",
    "town_coop",
    "town_outhouse",
    "town_dung",
    "town_hayrick",
    "castle_stable",
    "castle_barracks",
    "castle_store",
    "castle_cartshed",
    // The producer yards on the town's build plots (`town::spawn_building`).
    "town_farm",
    "town_woodcutter",
    "town_mine",
    // The castle yard corners + well (`castle::build`).
    "yard_wood",
    "yard_hay",
    "yard_cart",
    "yard_well",
    // Day-one castle furniture (`castle_decor::build`).
    "decor_notice_board",
    "decor_trough",
    "decor_bench",
    "gate_brazier",
    "training_dummy",
    "town_merchant_shop",
    "meadow_hay_cart",
    "meadow_bale",
    "meadow_scarecrow",
    "meadow_beehive",
    "meadow_firewood_rack",
    "meadow_crates",
    // The last procedural pieces (`tools/blender_forest/town_finish.py`).
    "meadow_fence",
    "meadow_fence_post",
    "castle_lantern",
    "castle_ballista",
    "town_plot",
    "town_house_site",
    "town_rubble",
    "gate_leaf",
    "gate_barricade",
    "decor_armory",
    "decor_armory_veteran",
    "decor_axe_display",
    "decor_sword_display",
    "decor_grindstone",
    "decor_tax_booth",
    "decor_shrine",
    "decor_bounty_board",
    "decor_scaffold",
    "decor_stone_pile",
    "decor_guild_banner",
    "decor_guild_goods",
    "decor_laundry",
];

/// A spawned town-kit model root.
#[derive(Component)]
pub struct TownPiece;

/// Spawn town-kit piece `name` at `tf` (pivot = the ground under its footprint centre, front to
/// local +Z). `None` when the kit isn't loaded yet or has no such piece. The caller adds its own
/// tags (`BiomeEntity`, colliders); the piece fades out past ~150 u like the market.
pub fn spawn_piece(commands: &mut Commands, name: &str, tf: Transform) -> Option<Entity> {
    let kit = crate::realforest::get().filter(|k| k.has(name))?;
    let range = VisibilityRange { start_margin: 0.0..0.0, end_margin: 140.0..160.0, use_aabb: true };
    let e = kit.spawn_static_with(commands, name, tf, range)?;
    commands.entity(e).insert(TownPiece);
    Some(e)
}

/// Showcase layout: (piece, world XZ, yaw, collider half-extents of the main body in the piece's
/// own frame, or zero). Rows on the flat ground north of the castle, fronts facing south (+Z)
/// toward the north wall so a camera just outside the wall sees them at 15-20 u: the landmarks +
/// the Crown set at z -29, small props at z -37.5, Maud's house, the infirmary and the houses at
/// z -46. Spec v3 pieces: two mock burgage plots south of the castle, east of the S gate (a road
/// along z ~23 under the south wall, the street-gable houses fronting it at z 24 facing north,
/// plots 6 u wide running back south to a fence at z 37.6: wattle plot A x 8..14, picket plot B
/// x 14..20), and the four lean-tos against the OUTSIDE face of the east curtain (x 28.65), fronts
/// east: stand-ins for their place inside the walls (backs flat on the stone, the same fit; the
/// curtain shows once the walls are built, e.g. `FOREST_DEFEND=1`).
pub const SHOWCASE: [(&str, Vec2, f32, Vec2); 95] = [
    ("town_granary", Vec2::new(-25.0, -29.0), 0.0, Vec2::new(1.75, 2.25)),
    ("town_smithy", Vec2::new(-16.0, -29.0), 0.0, Vec2::ZERO),
    ("town_tavern", Vec2::new(-5.5, -29.0), 0.0, Vec2::new(3.3, 2.5)),
    ("town_grave", Vec2::new(3.0, -29.0), 0.0, Vec2::ZERO),
    ("crown_gallows", Vec2::new(7.5, -29.0), 0.0, Vec2::new(1.1, 0.8)),
    ("crown_crates", Vec2::new(11.0, -29.0), 0.0, Vec2::ZERO),
    ("crown_tent", Vec2::new(14.3, -29.0), 0.0, Vec2::new(1.2, 1.4)),
    ("crown_banner_pole", Vec2::new(17.0, -29.0), 0.0, Vec2::ZERO),
    ("crown_depot", Vec2::new(21.0, -29.0), 0.0, Vec2::new(1.7, 1.3)),
    // On the depot's west plank wall (x = 21 - 1.6), facing west, the bracket 2.1 up.
    ("crown_banner_wall", Vec2::new(19.32, -29.2), -std::f32::consts::FRAC_PI_2, Vec2::ZERO),
    ("town_firepit", Vec2::new(-25.0, -37.5), 0.0, Vec2::ZERO),
    ("town_carpenter", Vec2::new(-18.0, -37.5), 0.0, Vec2::ZERO),
    ("town_archery", Vec2::new(-12.0, -37.5), 0.0, Vec2::ZERO),
    ("town_maud_house", Vec2::new(-24.0, -46.0), 0.0, Vec2::new(2.7, 2.0)),
    ("town_infirmary", Vec2::new(-12.5, -46.0), 0.0, Vec2::new(3.7, 1.9)),
    ("town_house_a", Vec2::new(-1.0, -46.0), 0.0, Vec2::new(2.4, 1.8)),
    ("town_house_b", Vec2::new(7.0, -46.0), 0.0, Vec2::new(2.9, 1.9)),
    ("town_house_c", Vec2::new(15.0, -46.0), 0.0, Vec2::new(2.0, 1.7)),
    ("town_house_d", Vec2::new(22.0, -46.0), 0.0, Vec2::new(2.5, 1.9)),
    // Burgage plot A (wattle, x 8..14): house fronting the road, woodpile on its back wall, shed
    // on the shared fence, garden, a gate in the back fence to the fields.
    ("town_street_house_a", Vec2::new(11.0, 27.8), PI, Vec2::new(2.3, 3.9)),
    ("town_woodpile", Vec2::new(12.4, 32.1), 0.0, Vec2::ZERO),
    ("town_shed", Vec2::new(13.05, 35.5), -FRAC_PI_2, Vec2::ZERO),
    ("town_garden", Vec2::new(10.0, 35.0), 0.0, Vec2::ZERO),
    ("town_fence_wattle", Vec2::new(8.0, 33.1), FRAC_PI_2, Vec2::ZERO),
    ("town_fence_wattle", Vec2::new(8.0, 36.1), FRAC_PI_2, Vec2::ZERO),
    ("town_fence_wattle", Vec2::new(9.5, 37.6), 0.0, Vec2::ZERO),
    ("town_fence_wattle_gate", Vec2::new(12.5, 37.6), 0.0, Vec2::ZERO),
    ("town_fence_picket", Vec2::new(14.0, 33.1), FRAC_PI_2, Vec2::ZERO),
    ("town_fence_picket", Vec2::new(14.0, 36.1), FRAC_PI_2, Vec2::ZERO),
    // Plot B (picket, x 14..20): the shop house, coop, hay rick, dung heap, the privy.
    ("town_street_house_b", Vec2::new(17.0, 27.5), PI, Vec2::new(2.5, 3.6)),
    ("town_coop", Vec2::new(15.2, 32.3), 0.0, Vec2::ZERO),
    ("town_hayrick", Vec2::new(17.8, 34.5), 0.0, Vec2::new(1.2, 1.2)),
    ("town_dung", Vec2::new(15.3, 36.3), 0.0, Vec2::ZERO),
    ("town_outhouse", Vec2::new(19.0, 32.0), -FRAC_PI_2, Vec2::ZERO),
    ("town_fence_picket", Vec2::new(20.0, 33.1), -FRAC_PI_2, Vec2::ZERO),
    ("town_fence_picket", Vec2::new(20.0, 36.1), -FRAC_PI_2, Vec2::ZERO),
    ("town_fence_picket", Vec2::new(15.5, 37.6), 0.0, Vec2::ZERO),
    ("town_fence_picket_gate", Vec2::new(18.5, 37.6), 0.0, Vec2::ZERO),
    // Lean-tos on the east curtain's outer face (x 28.65 + depth / 2), fronts east.
    ("castle_stable", Vec2::new(30.39, -13.6), FRAC_PI_2, Vec2::ZERO),
    ("castle_barracks", Vec2::new(30.49, -1.9), FRAC_PI_2, Vec2::ZERO),
    ("castle_store", Vec2::new(30.29, 5.6), FRAC_PI_2, Vec2::ZERO),
    ("castle_cartshed", Vec2::new(30.59, 13.0), FRAC_PI_2, Vec2::ZERO),
    // The producer yards (barn / saw shed / pit-head on the -X half), a row at z -55.
    ("town_farm", Vec2::new(-8.0, -55.0), 0.0, Vec2::ZERO),
    ("town_woodcutter", Vec2::new(0.0, -55.0), 0.0, Vec2::ZERO),
    ("town_mine", Vec2::new(8.0, -55.0), 0.0, Vec2::ZERO),
    ("yard_wood", Vec2::new(14.0, -55.0), 0.0, Vec2::ZERO),
    ("yard_hay", Vec2::new(17.0, -55.0), 0.0, Vec2::ZERO),
    ("yard_cart", Vec2::new(20.0, -55.0), 0.0, Vec2::ZERO),
    ("yard_well", Vec2::new(23.5, -55.0), 0.0, Vec2::ZERO),
    ("decor_notice_board", Vec2::new(26.5, -55.0), 0.0, Vec2::ZERO),
    ("decor_trough", Vec2::new(28.5, -55.0), 0.0, Vec2::ZERO),
    ("decor_bench", Vec2::new(30.5, -55.0), 0.0, Vec2::ZERO),
    ("gate_brazier", Vec2::new(32.5, -55.0), 0.0, Vec2::ZERO),
    ("training_dummy", Vec2::new(34.5, -55.0), 0.0, Vec2::ZERO),
    ("town_merchant_shop", Vec2::new(38.5, -55.0), 0.0, Vec2::new(1.6, 1.05)),
    ("meadow_hay_cart", Vec2::new(-8.0, -61.0), 0.0, Vec2::ZERO),
    ("meadow_bale", Vec2::new(-4.0, -61.0), 0.0, Vec2::ZERO),
    ("meadow_scarecrow", Vec2::new(-1.0, -61.0), 0.0, Vec2::ZERO),
    ("meadow_beehive", Vec2::new(1.5, -61.0), 0.0, Vec2::ZERO),
    ("meadow_firewood_rack", Vec2::new(4.0, -61.0), 0.0, Vec2::ZERO),
    ("meadow_crates", Vec2::new(7.0, -61.0), 0.0, Vec2::ZERO),
    // town_finish.py: rows at z -67 (fence, lantern, ballista, plots, ruin, gate) and z -74 (decor).
    ("meadow_fence", Vec2::new(-8e+00, -67.0), 0.0, Vec2::ZERO),
    ("meadow_fence_post", Vec2::new(-6e+00, -67.0), 0.0, Vec2::ZERO),
    ("castle_lantern", Vec2::new(-4e+00, -67.0), 0.0, Vec2::ZERO),
    ("castle_ballista", Vec2::new(-1e+00, -67.0), 0.0, Vec2::ZERO),
    ("town_plot", Vec2::new(4e+00, -67.0), 0.0, Vec2::ZERO),
    ("town_house_site", Vec2::new(1e+01, -67.0), 0.0, Vec2::ZERO),
    ("town_rubble", Vec2::new(2e+01, -67.0), 0.0, Vec2::ZERO),
    ("gate_leaf", Vec2::new(2e+01, -67.0), 0.0, Vec2::ZERO),
    ("gate_barricade", Vec2::new(3e+01, -67.0), 0.0, Vec2::ZERO),
    ("decor_armory", Vec2::new(-8e+00, -74.0), 0.0, Vec2::ZERO),
    ("decor_armory_veteran", Vec2::new(-4e+00, -74.0), 0.0, Vec2::ZERO),
    ("decor_axe_display", Vec2::new(-2e+00, -74.0), 0.0, Vec2::ZERO),
    ("decor_sword_display", Vec2::new(-0.5, -74.0), 0.0, Vec2::ZERO),
    ("decor_grindstone", Vec2::new(2e+00, -74.0), 0.0, Vec2::ZERO),
    ("decor_tax_booth", Vec2::new(4e+00, -74.0), 0.0, Vec2::ZERO),
    ("decor_shrine", Vec2::new(8e+00, -74.0), 0.0, Vec2::ZERO),
    ("decor_bounty_board", Vec2::new(1e+01, -74.0), 0.0, Vec2::ZERO),
    ("decor_scaffold", Vec2::new(1e+01, -74.0), 0.0, Vec2::ZERO),
    ("decor_stone_pile", Vec2::new(2e+01, -74.0), 0.0, Vec2::ZERO),
    ("decor_guild_banner", Vec2::new(2e+01, -74.0), 0.0, Vec2::ZERO),
    ("decor_guild_goods", Vec2::new(2e+01, -74.0), 0.0, Vec2::ZERO),
    ("decor_laundry", Vec2::new(2e+01, -74.0), 0.0, Vec2::ZERO),
    // village_huts.py: the twelve settler huts in a row at z -82, front (+Z) toward the castle.
    ("town_hut_0", Vec2::new(-33.0, -82.0), 0.0, Vec2::ZERO),
    ("town_hut_1", Vec2::new(-27.0, -82.0), 0.0, Vec2::ZERO),
    ("town_hut_2", Vec2::new(-21.0, -82.0), 0.0, Vec2::ZERO),
    ("town_hut_3", Vec2::new(-15.0, -82.0), 0.0, Vec2::ZERO),
    ("town_hut_4", Vec2::new(-9.0, -82.0), 0.0, Vec2::ZERO),
    ("town_hut_5", Vec2::new(-3.0, -82.0), 0.0, Vec2::ZERO),
    ("town_hut_6", Vec2::new(3.0, -82.0), 0.0, Vec2::ZERO),
    ("town_hut_7", Vec2::new(9.0, -82.0), 0.0, Vec2::ZERO),
    ("town_hut_8", Vec2::new(15.0, -82.0), 0.0, Vec2::ZERO),
    ("town_hut_9", Vec2::new(21.0, -82.0), 0.0, Vec2::ZERO),
    ("town_hut_10", Vec2::new(27.0, -82.0), 0.0, Vec2::ZERO),
    ("town_hut_11", Vec2::new(33.0, -82.0), 0.0, Vec2::ZERO),
];

/// Lays the showcase out once the kit has loaded; a world rebuild clears it (`BiomeEntity`) and
/// this lays it out again.
fn spawn_showcase(mut commands: Commands, existing: Query<(), With<TownPiece>>) {
    if !existing.is_empty() || !crate::realforest::get().is_some_and(|k| k.has(PIECES[0])) {
        return;
    }
    let ground = |p: Vec2| crate::worldmap::ground_at_world(p.x, p.y).unwrap_or(0.0);
    let mut n = 0;
    for (name, p, yaw, half) in SHOWCASE {
        let y = if name == "crown_banner_wall" { ground(Vec2::new(21.0, -29.0)) + 2.1 } else { ground(p) - 0.02 };
        let tf = Transform::from_xyz(p.x, y, p.y).with_rotation(Quat::from_rotation_y(yaw));
        let Some(e) = spawn_piece(&mut commands, name, tf) else { continue };
        commands.entity(e).insert(BiomeEntity);
        if half != Vec2::ZERO {
            crate::blockers::add_obb(p.x, p.y, half.x, half.y, yaw);
        }
        n += 1;
    }
    info!("town kit showcase: {n} pieces laid out north of the castle");
}

#[cfg(test)]
mod tests {
    use super::*;

    /// World-aligned half extents of a collider turned by `yaw`.
    fn aabb_half(half: Vec2, yaw: f32) -> Vec2 {
        let (s, c) = (yaw.sin().abs(), yaw.cos().abs());
        Vec2::new(c * half.x + s * half.y, s * half.x + c * half.y)
    }

    /// The showcase lays out every piece at least once (the fence modules repeat), names only kit
    /// pieces, and the pieces with colliders don't overlap.
    #[test]
    fn showcase_covers_every_piece_once() {
        for name in PIECES {
            assert!(SHOWCASE.iter().any(|s| s.0 == name), "{name}");
        }
        for s in SHOWCASE {
            assert!(PIECES.contains(&s.0), "{}", s.0);
        }
        for (i, a) in SHOWCASE.iter().enumerate() {
            for b in &SHOWCASE[i + 1..] {
                if a.3 != Vec2::ZERO && b.3 != Vec2::ZERO {
                    let gap = (a.1 - b.1).abs() - (aabb_half(a.3, a.2) + aabb_half(b.3, b.2));
                    assert!(gap.x > 0.5 || gap.y > 0.5, "{} overlaps {}", a.0, b.0);
                }
            }
        }
    }
}
