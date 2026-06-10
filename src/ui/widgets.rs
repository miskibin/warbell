//! **Widget paint kits.** Small bundles of the *visual* components (colours, border, radius, shadow,
//! hover) that compose onto a caller-owned [`Node`]. Keeping `Node` (layout) separate from the paint
//! avoids fighting bundle merges — call sites stay in control of sizing while the look stays central.
//!
//! Borders only show if the caller's `Node` sets a `border` thickness (e.g. `UiRect::all(Val::Px(1.))`),
//! and rounded corners are a `Node` field too (`border_radius: ui::radius(px)`) — neither is a
//! standalone component in Bevy 0.18, so the paint kits below carry only the real components
//! (`BackgroundColor`/`BorderColor`/`BoxShadow`/`Button`/`UiTransform`/`Hoverable`).

use bevy::prelude::*;
use bevy::ui::{BackgroundGradient, ColorStop, Gradient, LinearGradient};

use super::anim::Hoverable;
use super::theme::*;

/// Full-screen centred modal backdrop (the world-freeze scrim).
pub fn scrim(z: i32) -> impl Bundle {
    (
        Node {
            position_type: PositionType::Absolute,
            top: Val::Px(0.0),
            left: Val::Px(0.0),
            width: Val::Percent(100.0),
            height: Val::Percent(100.0),
            flex_direction: FlexDirection::Column,
            align_items: AlignItems::Center,
            justify_content: JustifyContent::Center,
            ..default()
        },
        BackgroundColor(SCRIM),
        GlobalZIndex(z),
    )
}

/// Modal card paint (caller owns the `Node`: padding/min-width/gap/`border`/`border_radius`).
pub fn card_paint() -> impl Bundle {
    (BackgroundColor(PANEL), BorderColor::all(BORDER_SOFT), shadow_card())
}

/// Primary action button paint (solid blue — Play / Resume / Again).
pub fn btn_primary_paint() -> impl Bundle {
    (
        Button,
        Interaction::default(),
        BackgroundColor(BLUE),
        BorderColor::all(BLUE_BORDER),
        UiTransform::IDENTITY,
        Hoverable {
            rest_bg: BLUE,
            hover_bg: BLUE_HI,
            rest_border: BLUE_BORDER,
            hover_border: BLUE_BORDER,
            lift: 2.0,
        },
    )
}

/// Danger button paint (solid red — destructive confirms like Overwrite save).
pub fn btn_danger_paint() -> impl Bundle {
    (
        Button,
        Interaction::default(),
        BackgroundColor(RED),
        BorderColor::all(RED_BORDER),
        UiTransform::IDENTITY,
        Hoverable {
            rest_bg: RED,
            hover_bg: RED_HI,
            rest_border: RED_BORDER,
            hover_border: RED_BORDER,
            lift: 2.0,
        },
    )
}

/// Inventory/quick-slot cell paint (square; hover lightens the border + lifts).
pub fn slot_paint() -> impl Bundle {
    (
        Button,
        Interaction::default(),
        BackgroundColor(SLOT_BG),
        BorderColor::all(SLOT_BORDER),
        UiTransform::IDENTITY,
        Hoverable {
            rest_bg: SLOT_BG,
            hover_bg: SLOT_BG,
            rest_border: SLOT_BORDER,
            hover_border: SLOT_BORDER_HOVER,
            lift: 2.0,
        },
    )
}

/// Keycap (kbd) paint — a small raised key.
pub fn keycap_paint() -> impl Bundle {
    (BackgroundColor(rgba(48, 58, 80, 0.95)), BorderColor::all(rgba(255, 255, 255, 0.14)))
}

/// A square icon image node.
pub fn icon(handle: Handle<Image>, px: f32) -> impl Bundle {
    (Node { width: Val::Px(px), height: Val::Px(px), ..default() }, ImageNode::new(handle))
}

/// Vertical (top→bottom) linear gradient fill — for HP/XP/stamina bars and gradient buttons.
pub fn vgrad(top: Color, bot: Color) -> BackgroundGradient {
    BackgroundGradient(vec![Gradient::Linear(LinearGradient::new(
        std::f32::consts::PI, // 0 = up; π points down so `top` sits at the top
        vec![ColorStop::new(top, Val::Percent(0.0)), ColorStop::new(bot, Val::Percent(100.0))],
    ))])
}

/// `UiRect` border of uniform thickness — shorthand for call sites.
pub fn border(px: f32) -> UiRect {
    UiRect::all(Val::Px(px))
}
