//! Shared colour helpers + the forest palette, lifted verbatim (hex) from the TS
//! game so every module tints from one source of truth.
//!
//! Mesh `ATTRIBUTE_COLOR` is LINEAR rgba, so models bake colours via [`lin`]. UI /
//! material base colours that are sRGB use [`srgb`].

use bevy::prelude::*;

/// sRGB `Color` from a `0xRRGGBB` literal.
pub fn srgb(hex: u32) -> Color {
    Color::srgb_u8(((hex >> 16) & 0xff) as u8, ((hex >> 8) & 0xff) as u8, (hex & 0xff) as u8)
}

/// LINEAR `[r,g,b,1]` from a `0xRRGGBB` literal — for mesh `ATTRIBUTE_COLOR`.
pub fn lin(hex: u32) -> [f32; 4] {
    let l = srgb(hex).to_linear();
    [l.red, l.green, l.blue, 1.0]
}

/// Linear colour scaled by `v` (per-instance brightness tint), alpha kept.
pub fn lin_scaled(hex: u32, v: f32) -> [f32; 4] {
    let l = srgb(hex).to_linear();
    [l.red * v, l.green * v, l.blue * v, 1.0]
}

// ── Forest ground ──────────────────────────────────────────────────────────
pub const FOREST_GROUND: u32 = 0x6cb14a; // grass base (vision.ts TOP_SPECS.grass)

// ── Tree foliage / trunks (forest-tree spec) ────────────────────────────────
// Foliage nudged ~8% darker + cooler off the original spec greens (0x2f7a36 /
// 0x3a9442 / 0x4cb358): canopy and grass sat in the same hue+value family, so the
// forest read as one neon-green mass — the cool dark canopy is what lets the bright
// warm meadow read against it.
pub const TREE_TRUNK: u32 = 0x5a3a22;
pub const FOLIAGE_DARK: u32 = 0x2a6f3c;
pub const FOLIAGE_MID: u32 = 0x33874a;
pub const FOLIAGE_LIGHT: u32 = 0x43a45e;
pub const BIRCH_TRUNK: u32 = 0xece8d8;
pub const BIRCH_MARK: u32 = 0x2a261e;
pub const BIRCH_DARK: u32 = 0x3a8c34;
pub const BIRCH_LIGHT: u32 = 0x7dc04a;
pub const DEAD_WOOD: u32 = 0x6e6258;
pub const DEAD_WOOD_DARK: u32 = 0x4a4238;
// Autumn broadleaf — real russet/orange/gold foliage (the warm-season tree). NOT
// reachable by tinting the greens above (a multiply can only darken toward green), so
// these are their own base tones: deep russet base mass → burnt orange body → gold cap.
pub const AUTUMN_DARK: u32 = 0x8a3b18;
pub const AUTUMN_MID: u32 = 0xc8651f;
pub const AUTUMN_LIGHT: u32 = 0xe7a72f;
// Extra autumn tones so a turning crown is dappled (leaves changing at different rates),
// not one flat orange ball: a deep brick red, a sunlit gold cap, and a lingering olive.
pub const AUTUMN_RED: u32 = 0x9c3514;
pub const AUTUMN_GOLD: u32 = 0xf2c63c;
pub const AUTUMN_OLIVE: u32 = 0x7a8a2e;
// Fresh sawn cut-face on a stump/log: pale ringed heartwood, brighter than the bark.
pub const CUT_WOOD: u32 = 0xc79a63;
