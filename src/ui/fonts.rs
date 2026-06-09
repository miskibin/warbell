//! **Fonts.** The original game used Inter (weights 400/600/700/800) for all chrome and a Palatino
//! serif for the upgrade-tree board. We bundle Inter static TTFs + EB Garamond (a free OFL serif) and
//! load them into [`UiFonts`] at startup. Bevy's `TextFont` selects a face by `Handle<Font>` (no
//! weight axis), so each weight is a separate file.

use bevy::prelude::*;

/// Loaded font faces, indexed by the weights the UI actually uses.
#[derive(Resource, Clone)]
pub struct UiFonts {
    pub regular: Handle<Font>,  // 400
    pub semibold: Handle<Font>, // 600
    pub bold: Handle<Font>,     // 700
    pub extrabold: Handle<Font>, // 800
    pub serif: Handle<Font>,    // EB Garamond — upgrade board only
}

impl UiFonts {
    pub fn load(assets: &AssetServer) -> Self {
        Self {
            regular: assets.load("fonts/Inter-Regular.ttf"),
            semibold: assets.load("fonts/Inter-SemiBold.ttf"),
            bold: assets.load("fonts/Inter-Bold.ttf"),
            extrabold: assets.load("fonts/Inter-ExtraBold.ttf"),
            serif: assets.load("fonts/EBGaramond.ttf"),
        }
    }
}

/// A text bundle in a chosen face/size/colour. `font` is one of the `UiFonts` handles, cloned.
pub fn label(font: &Handle<Font>, s: impl Into<String>, size: f32, color: Color) -> impl Bundle {
    (
        Text::new(s),
        TextFont { font: font.clone(), font_size: size, ..default() },
        TextColor(color),
    )
}
