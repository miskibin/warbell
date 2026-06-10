//! **UI design system** — the palette, radii, and chrome presets ported from the original
//! three.js game's `hud.css`. Every panel/HUD element pulls its colours and shapes from here so the
//! look stays coherent (and is tuned in one place). CSS hex values are converted to sRGB floats.
//!
//! Construction note: colours are `const` via the [`Color::Srgba`] struct-literal form (the
//! `Color::srgb*` helpers aren't all `const`), so they can live as associated constants.

use bevy::color::Srgba;
use bevy::prelude::*;
use bevy::ui::{BorderRadius, BoxShadow, ShadowStyle, Val};

/// sRGB colour from 0-255 channel bytes + alpha float — `const`-friendly.
pub const fn rgba(r: u8, g: u8, b: u8, a: f32) -> Color {
    Color::Srgba(Srgba {
        red: r as f32 / 255.0,
        green: g as f32 / 255.0,
        blue: b as f32 / 255.0,
        alpha: a,
    })
}
/// Opaque sRGB colour from 0-255 channel bytes.
pub const fn rgb(r: u8, g: u8, b: u8) -> Color {
    rgba(r, g, b, 1.0)
}

// ── Palette (from hud.css) ───────────────────────────────────────────────────────────────
pub const GOLD: Color = rgb(255, 213, 140); // #ffd58c — currency, titles, accents
pub const GOLD_DEEP: Color = rgb(224, 168, 74); // #e0a84a — button gradient base
pub const STONE: Color = rgb(205, 211, 218); // #cdd3da — stone tally
pub const GREEN: Color = rgb(155, 232, 138); // #9be88a — buffs / success / toast accent
pub const GREY: Color = rgb(138, 142, 152); // #8a8e98 — labels, disabled, hints
pub const RED: Color = rgb(214, 58, 58); // #d63a3a — HP / danger
pub const TEXT: Color = rgb(243, 243, 245); // #f3f3f5 — body
pub const TEXT_DIM: Color = rgb(185, 194, 212); // #b9c2d4 — secondary
pub const TEXT_FAINT: Color = rgba(147, 161, 184, 1.0); // #93a1b8 — tertiary

// Panels / chrome
pub const PANEL: Color = rgba(22, 28, 40, 0.95); // modal card bg
pub const PANEL_HUD: Color = rgba(20, 26, 38, 0.78); // persistent-HUD chrome bg
pub const SCRIM: Color = rgba(8, 8, 14, 0.62); // modal backdrop (blur approximated darker)
pub const BORDER_SOFT: Color = rgba(255, 255, 255, 0.10);
pub const BTN_BG: Color = rgba(255, 255, 255, 0.04);
pub const BTN_BG_HOVER: Color = rgba(255, 255, 255, 0.09);

// Slots / cells
pub const SLOT_BG: Color = rgba(140, 140, 150, 0.16);
pub const SLOT_BORDER: Color = rgba(80, 84, 92, 0.9);
pub const SLOT_BORDER_HOVER: Color = rgba(180, 184, 196, 0.95);

// Bars
pub const HP_TOP: Color = rgb(214, 58, 58);
pub const HP_BOT: Color = rgb(138, 31, 31);
pub const XP_TOP: Color = rgb(98, 198, 232); // #62c6e8
pub const XP_BOT: Color = rgb(58, 123, 213); // #3a7bd5
pub const STAM_TOP: Color = rgb(143, 168, 200); // #8fa8c8
pub const STAM_BOT: Color = rgb(74, 102, 144); // #4a6690

// Primary action button (Play / Resume)
pub const BLUE: Color = rgb(74, 111, 200); // #4a6fc8
pub const BLUE_HI: Color = rgb(98, 134, 219); // #6286db
pub const BLUE_BORDER: Color = rgb(130, 162, 234); // #82a2ea

// Danger action button (Overwrite save / destructive confirms)
pub const RED_HI: Color = rgb(232, 82, 82); // hover
pub const RED_BORDER: Color = rgb(240, 120, 120);

// Start-screen warm accent
pub const KICKER: Color = rgb(199, 155, 106); // #c79b6a

// Upgrade-tree parchment board
pub const PARCHMENT: Color = rgb(231, 216, 176); // #e7d8b0
pub const INK: Color = rgb(36, 27, 12); // #241b0c
pub const INK_SOFT: Color = rgb(90, 69, 36); // #5a4524
pub const BRANCH_ECON: Color = rgb(92, 122, 52); // #5c7a34
pub const BRANCH_DEF: Color = rgb(76, 100, 126); // #4c647e
pub const BRANCH_HERO: Color = rgb(142, 55, 44); // #8e372c
pub const BRANCH_ARSENAL: Color = rgb(138, 94, 42); // #8a5e2a

// ── Shapes ───────────────────────────────────────────────────────────────────────────────
pub const R_PANEL: f32 = 10.0;
pub const R_CARD: f32 = 8.0;
pub const R_BTN: f32 = 6.0;
pub const R_CELL: f32 = 4.0;
pub const R_SLOT: f32 = 3.0;

pub fn radius(px: f32) -> BorderRadius {
    BorderRadius::all(Val::Px(px))
}

/// The standard modal-card drop shadow (`0 16px 40px rgba(0,0,0,0.55)`).
pub fn shadow_card() -> BoxShadow {
    BoxShadow(vec![ShadowStyle {
        color: rgba(0, 0, 0, 0.55),
        x_offset: Val::Px(0.0),
        y_offset: Val::Px(16.0),
        spread_radius: Val::Px(0.0),
        blur_radius: Val::Px(40.0),
    }])
}

/// The lighter HUD-chrome shadow (`0 8px 30px rgba(0,0,0,0.35)`).
pub fn shadow_hud() -> BoxShadow {
    BoxShadow(vec![ShadowStyle {
        color: rgba(0, 0, 0, 0.35),
        x_offset: Val::Px(0.0),
        y_offset: Val::Px(8.0),
        spread_radius: Val::Px(0.0),
        blur_radius: Val::Px(30.0),
    }])
}
