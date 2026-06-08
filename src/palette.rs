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
pub const TREE_TRUNK: u32 = 0x5a3a22;
pub const FOLIAGE_DARK: u32 = 0x2f7a36;
pub const FOLIAGE_MID: u32 = 0x3a9442;
pub const FOLIAGE_LIGHT: u32 = 0x4cb358;
pub const BIRCH_TRUNK: u32 = 0xece8d8;
pub const BIRCH_MARK: u32 = 0x2a261e;
pub const BIRCH_DARK: u32 = 0x3a8c34;
pub const BIRCH_LIGHT: u32 = 0x7dc04a;
pub const DEAD_WOOD: u32 = 0x6e6258;
pub const DEAD_WOOD_DARK: u32 = 0x4a4238;
