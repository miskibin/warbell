//! **Procedural item icons.** The TS HUD drew items as emoji, which the Bevy default font can't
//! render (they show as tofu). Instead each item's `tileworld_core::inventory::IconSpec` (a shape
//! + two colours) is rasterised here into a 48² RGBA texture at startup, keyed by item id in the
//! [`IconAtlas`]. The satchel + shop panels show these instead of relying on glyphs.

use bevy::asset::RenderAssetUsages;
use bevy::prelude::*;
use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};
use std::collections::HashMap;

use tileworld_core::inventory::{IconShape, IconSpec, IconRgb, ITEM_DEFS};

/// item id → its rasterised 48² icon texture.
#[derive(Resource, Default)]
pub struct IconAtlas(HashMap<&'static str, Handle<Image>>);

impl IconAtlas {
    pub fn get(&self, id: &str) -> Option<Handle<Image>> {
        self.0.get(id).cloned()
    }
}

pub struct IconsPlugin;

impl Plugin for IconsPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<IconAtlas>().add_systems(Startup, build_icons);
    }
}

fn build_icons(mut images: ResMut<Assets<Image>>, mut atlas: ResMut<IconAtlas>) {
    for def in ITEM_DEFS {
        let handle = images.add(rasterise(def.icon_spec()));
        atlas.0.insert(def.id, handle);
    }
}

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
            rect(&mut buf, 23, 6, 25, 13, stem); // stem
            disc(&mut buf, 31, 9, 5, ac); // leaf
        }
        IconShape::Orb => {
            disc(&mut buf, 24, 24, 18, fg);
            disc(&mut buf, 18, 18, 5, ac); // highlight
        }
        IconShape::Food => {
            disc(&mut buf, 24, 26, 18, fg);
            disc(&mut buf, 24, 26, 18 - 3, ac); // inner crumb
            disc(&mut buf, 24, 26, 18, fg); // re-rim
            rect(&mut buf, 8, 24, 40, 27, ac); // score line
        }
        IconShape::Meat => {
            disc(&mut buf, 22, 24, 16, fg);
            disc(&mut buf, 30, 28, 12, fg);
            disc(&mut buf, 40, 33, 5, ac); // bone nub
        }
        IconShape::Herb => {
            for dx in [-9, -3, 3, 9] {
                let bx = 24 + dx;
                rect(&mut buf, bx - 1, 10, bx + 1, 40, fg);
            }
            disc(&mut buf, 24, 40, 6, ac); // base clump
        }
        IconShape::Potion => {
            disc(&mut buf, 24, 30, 14, fg); // body
            rect(&mut buf, 20, 10, 28, 22, fg); // neck
            rect(&mut buf, 19, 5, 29, 11, ac); // cork
            disc(&mut buf, 19, 27, 4, (255, 255, 255)); // sheen
        }
        IconShape::Blade => {
            // Blade: a triangle tapering to a point at the top.
            for y in 6..36 {
                let w = (y - 6) / 4; // widens downward
                rect(&mut buf, 24 - w, y, 24 + w, y, fg);
            }
            rect(&mut buf, 16, 35, 32, 39, ac); // crossguard
            rect(&mut buf, 22, 39, 26, 45, ac); // grip
        }
        IconShape::Shield => {
            for y in 8..44 {
                // Heater shield: full width up top, tapering to a point at the bottom.
                let t = (y - 8) as f32 / 36.0;
                let half = (18.0 * (1.0 - t * t)) as i32;
                rect(&mut buf, 24 - half, y, 24 + half, y, fg);
            }
            rect(&mut buf, 7, 22, 41, 26, ac); // band
        }
        IconShape::Scroll => {
            rect(&mut buf, 11, 13, 37, 35, fg); // sheet
            rect(&mut buf, 8, 10, 40, 15, ac); // top roll
            rect(&mut buf, 8, 33, 40, 38, ac); // bottom roll
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
