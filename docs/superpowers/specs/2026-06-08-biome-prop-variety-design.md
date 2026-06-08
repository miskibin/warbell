# Biome prop variety — one new signature prop per biome

**Date:** 2026-06-08
**Status:** approved (fast set: ⭐ one-per-biome)

## Goal

Add one distinctive new scatter prop to each of the five biomes so the world reads
more varied. Stay inside the established mesh recipe (primitive parts → `tinted`
linear `ATTRIBUTE_COLOR` → `Mesh::merge` → `flat_shaded`), batched on the shared
white vertex-colour material the scatter owns. No new assets, no GLTF.

## Scope (the 5 props)

| Biome  | New prop | Where | Wiring |
|--------|----------|-------|--------|
| Forest | **Pine / spruce conifer** — brown trunk + 3 stacked green cone tiers, snow-free (lusher than the snow pine). Forest had no conifer → new silhouette. | `trees.rs` (`TreeKind::Pine` + `build_pine`) | append variant to the Forest tree class; rebalance weights broadleaf .55 / birch .16 / pine .22 / dead .07 |
| Snow   | **Snowman (bałwan)** — 3 stacked white balls, coal eyes+mouth, orange carrot nose, twig arms, red scarf, dark bucket hat. | `biome_snow.rs` (`build_snowman_mesh`) | new non-tree `PropClass`, `chance ≈ 0.007` (rare/special), appended AFTER the mound class (so the mound stays the tree fallback) |
| Desert | **Prickly-pear / opuntia** — stacked flat green paddle pads + red fruit + spine flecks. New cactus shape vs the columnar saguaro. | `biome_desert.rs` (`build_prickly_pear_mesh`) | new non-tree `PropClass`, `chance ≈ 0.018` |
| Rocky  | **Crystal / geode cluster** — saturated amethyst/teal angular crystals jutting from a small grey rock base. Colour pop in a grey biome. Vertex-colour only (stays batched; no emissive). | `biome_rocky.rs` (`build_crystal_mesh`) | new non-tree `PropClass`, `chance ≈ 0.014` |
| Swamp  | **Glowing mushroom cluster** — bioluminescent caps. Needs an **emissive** material, so it canNOT ride the shared-material scatter; spawned from `biome_swamp::landmarks` with its own emissive material, spread deterministically across the patch (same Mulberry32 / wisp approach), skipping river + centre framing. | `biome_swamp.rs` (`build_glowmush_mesh` + spread loop in `landmarks`) | ~12–16 clusters, `BiomeEntity` tagged |

## Constraints / contract

- Every scatter prop is ONE merged mesh, base flush at y=0, `flat_shaded` last.
- Colours live in `ATTRIBUTE_COLOR` (linear via `palette::lin`) — the scatter material is white.
- Don't prepend a class before the first non-tree class (it's the tree-too-close fallback).
- Class `chance` is a cumulative slice of one per-tile roll × `SCATTER_DENSITY` (1.35); keep totals sane.
- Glowmush emissive material mirrors the firefly/wisp pattern (`unlit`, `emissive = colour * k`) so bloom picks it up; `NotShadowCaster`.

## Verification

Per-biome screenshot via the harness: `FOREST_BIOME=<biome> FOREST_SHOT=<png>`; eyeball
each new prop in place. `cargo check` / `cargo build` must stay clean.

## Out of scope

Skybox (deferred by user), second-tier props (the non-⭐ menu items), landmark set-pieces.
