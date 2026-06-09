//! **UI kit** — the shared design system the whole HUD/menus are built on: palette + chrome
//! ([`theme`]), bundled fonts ([`fonts`]), Twemoji item icons ([`icons`]), entrance/hover motion
//! ([`anim`]), widget paint bundles ([`widgets`]), and the [`notice`] queue.
//!
//! `UiKitPlugin` is added **first** (before the HUD/panel plugins) so `UiFonts` and the icon atlas
//! exist by the time those plugins spawn. Its systems are ungated, so chrome animates in every state.

use bevy::prelude::*;

pub mod anim;
pub mod fonts;
pub mod icons;
pub mod notice;
pub mod settings;
pub mod theme;
pub mod widgets;

pub use fonts::{label, UiFonts};
pub use icons::IconAtlas;

pub struct UiKitPlugin;

impl Plugin for UiKitPlugin {
    fn build(&self, app: &mut App) {
        // Load fonts at build time (AssetServer is ready once DefaultPlugins are added) so the
        // `UiFonts` resource exists before any `Startup` spawn system queries it.
        let assets = app.world().resource::<AssetServer>().clone();
        app.insert_resource(UiFonts::load(&assets));
        app.add_plugins((
            icons::IconsPlugin,
            anim::AnimPlugin,
            notice::NoticePlugin,
            settings::SettingsPlugin,
        ));
    }
}
