//! **Item / upgrade / status icons.** The original game drew these as emoji. We ship the matching
//! **Twemoji** PNG rasters (CC-BY 4.0, see `assets/icons/twemoji/ATTRIBUTION.txt`) and load one per
//! id into [`IconAtlas`]. Both `ItemDef` and `UpgradeNode` already carry their emoji string in core,
//! so the atlas resolves them generically by converting the emoji to its Twemoji filename
//! (`emoji → lowercase codepoints, U+FE0F dropped`). A handful of named status/branch/buff symbols
//! are registered too. If an emoji has no matching PNG, the item falls back to the old procedurally
//! rasterised shape so nothing ever renders blank.

use bevy::asset::RenderAssetUsages;
use bevy::prelude::*;
use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};
use std::collections::HashMap;

use tileworld_core::inventory::{IconRgb, IconShape, IconSpec, ITEM_DEFS};
use tileworld_core::upgrade_store::UPGRADE_NODES;

/// id → its icon texture. Keys are item ids, upgrade-node ids, and `sym:*` / `branch:*` / `buff:*`
/// named symbols.
#[derive(Resource, Default)]
pub struct IconAtlas(HashMap<String, Handle<Image>>);

impl IconAtlas {
    pub fn get(&self, id: &str) -> Option<Handle<Image>> {
        self.0.get(id).cloned()
    }
}

/// Convert an emoji grapheme to its Twemoji filename stem (`"⚔️" → "2694"`, `"🏠" → "1f3e0"`).
/// Drops the U+FE0F presentation selector to match the Twemoji asset naming.
pub fn emoji_to_codepoint(emoji: &str) -> Option<String> {
    let parts: Vec<String> = emoji
        .chars()
        .filter(|c| *c as u32 != 0xFE0F)
        .map(|c| format!("{:x}", c as u32))
        .collect();
    if parts.is_empty() {
        None
    } else {
        Some(parts.join("-"))
    }
}

/// Named status / branch / buff symbols → emoji (resolved to Twemoji like everything else).
const SYMBOLS: &[(&str, &str)] = &[
    ("sym:gold", "⭐"),
    ("sym:stone", "🪨"),
    // Top-left stat-bar icons (all shipped as Twemoji PNGs).
    ("stat:gold", "💰"),
    ("stat:stone", "🪨"),
    ("stat:wood", "🪓"),
    ("stat:pop", "🏠"),
    ("stat:food", "🌾"),
    ("sym:lock", "🔒"),
    ("sym:warn", "⚠"),
    ("sym:sun", "☀"),
    ("sym:audio_on", "🔊"),
    ("sym:audio_off", "🔇"),
    ("sym:fullscreen", "🖥️"),
    ("buff:resist", "🛡️"),
    ("buff:power", "⚔️"),
    ("buff:haste", "💨"),
    ("buff:food", "🍖"),
    ("branch:economy", "🌾"),
    ("branch:defense", "🛡️"),
    ("branch:hero", "⚔️"),
    ("branch:arsenal", "🏪"),
];

pub struct IconsPlugin;

impl Plugin for IconsPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<IconAtlas>().add_systems(Startup, build_icons);
    }
}

fn build_icons(
    assets: Res<AssetServer>,
    mut images: ResMut<Assets<Image>>,
    mut atlas: ResMut<IconAtlas>,
) {
    let load = |emoji: &str| -> Option<Handle<Image>> {
        emoji_to_codepoint(emoji).map(|cp| assets.load(format!("icons/twemoji/{cp}.png")))
    };

    // Items: prefer the emoji PNG, fall back to the procedural shape.
    for def in ITEM_DEFS {
        let handle = load(def.icon).unwrap_or_else(|| images.add(rasterise(def.icon_spec())));
        atlas.0.insert(def.id.to_string(), handle);
    }
    // Upgrade-tree nodes carry their own emoji.
    for node in UPGRADE_NODES {
        if let Some(h) = load(node.icon) {
            atlas.0.insert(node.id.to_string(), h);
        }
    }
    // Named symbols.
    for (key, emoji) in SYMBOLS {
        if let Some(h) = load(emoji) {
            atlas.0.insert((*key).to_string(), h);
        }
    }
}

// ── Procedural fallback (verbatim from the previous icons.rs) ──────────────────────────────
const N: i32 = 48;

fn put(buf: &mut [u8], x: i32, y: i32, c: IconRgb) {
    if x < 0 || y < 0 || x >= N || y >= N {
        return;
    }
    let i = ((y * N + x) * 4) as usize;
    buf[i] = c.0;
    buf[i + 1] = c.1;
    buf[i + 2] = c.2;
    buf[i + 3] = 255;
}

fn disc(buf: &mut [u8], cx: i32, cy: i32, r: i32, c: IconRgb) {
    for y in (cy - r)..=(cy + r) {
        for x in (cx - r)..=(cx + r) {
            let (dx, dy) = (x - cx, y - cy);
            if dx * dx + dy * dy <= r * r {
                put(buf, x, y, c);
            }
        }
    }
}

fn rect(buf: &mut [u8], x0: i32, y0: i32, x1: i32, y1: i32, c: IconRgb) {
    for y in y0..=y1 {
        for x in x0..=x1 {
            put(buf, x, y, c);
        }
    }
}

/// Rasterise one icon recipe into a 48² RGBA image (transparent background).
fn rasterise(spec: IconSpec) -> Image {
    let mut buf = vec![0u8; (N * N * 4) as usize];
    let fg = spec.fg;
    let ac = spec.accent;
    let stem: IconRgb = (110, 72, 40);
    match spec.shape {
        IconShape::Apple => {
            disc(&mut buf, 22, 27, 16, fg);
            disc(&mut buf, 30, 27, 13, fg);
            rect(&mut buf, 23, 6, 25, 13, stem);
            disc(&mut buf, 31, 9, 5, ac);
        }
        IconShape::Orb => {
            disc(&mut buf, 24, 24, 18, fg);
            disc(&mut buf, 18, 18, 5, ac);
        }
        IconShape::Food => {
            disc(&mut buf, 24, 26, 18, fg);
            disc(&mut buf, 24, 26, 18 - 3, ac);
            disc(&mut buf, 24, 26, 18, fg);
            rect(&mut buf, 8, 24, 40, 27, ac);
        }
        IconShape::Meat => {
            disc(&mut buf, 22, 24, 16, fg);
            disc(&mut buf, 30, 28, 12, fg);
            disc(&mut buf, 40, 33, 5, ac);
        }
        IconShape::Herb => {
            for dx in [-9, -3, 3, 9] {
                let bx = 24 + dx;
                rect(&mut buf, bx - 1, 10, bx + 1, 40, fg);
            }
            disc(&mut buf, 24, 40, 6, ac);
        }
        IconShape::Potion => {
            disc(&mut buf, 24, 30, 14, fg);
            rect(&mut buf, 20, 10, 28, 22, fg);
            rect(&mut buf, 19, 5, 29, 11, ac);
            disc(&mut buf, 19, 27, 4, (255, 255, 255));
        }
        IconShape::Blade => {
            for y in 6..36 {
                let w = (y - 6) / 4;
                rect(&mut buf, 24 - w, y, 24 + w, y, fg);
            }
            rect(&mut buf, 16, 35, 32, 39, ac);
            rect(&mut buf, 22, 39, 26, 45, ac);
        }
        IconShape::Shield => {
            for y in 8..44 {
                let t = (y - 8) as f32 / 36.0;
                let half = (18.0 * (1.0 - t * t)) as i32;
                rect(&mut buf, 24 - half, y, 24 + half, y, fg);
            }
            rect(&mut buf, 7, 22, 41, 26, ac);
        }
        IconShape::Scroll => {
            rect(&mut buf, 11, 13, 37, 35, fg);
            rect(&mut buf, 8, 10, 40, 15, ac);
            rect(&mut buf, 8, 33, 40, 38, ac);
        }
    }
    Image::new(
        Extent3d { width: N as u32, height: N as u32, depth_or_array_layers: 1 },
        TextureDimension::D2,
        buf,
        TextureFormat::Rgba8UnormSrgb,
        RenderAssetUsages::default(),
    )
}
