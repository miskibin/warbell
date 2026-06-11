//! **Widget paint kits.** Small bundles of the *visual* components (colours, border, radius, shadow,
//! hover) that compose onto a caller-owned [`Node`]. Keeping `Node` (layout) separate from the paint
//! avoids fighting bundle merges — call sites stay in control of sizing while the look stays central.
//!
//! Borders only show if the caller's `Node` sets a `border` thickness (e.g. `UiRect::all(Val::Px(1.))`),
//! and rounded corners are a `Node` field too (`border_radius: ui::radius(px)`) — neither is a
//! standalone component in Bevy 0.18, so the paint kits below carry only the real components
//! (`BackgroundColor`/`BorderColor`/`BoxShadow`/`Button`/`UiTransform`/`Hoverable`).

use bevy::ecs::relationship::RelatedSpawnerCommands;
use bevy::prelude::*;
use bevy::ui::widget::NodeImageMode;
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
/// Pair with [`chrome_layers`] inside `.with_children` for the full medieval frame.
pub fn card_paint() -> impl Bundle {
    (BackgroundColor(PANEL), BorderColor::all(IRON_EDGE), shadow_card())
}

/// The medieval chrome frame layers: tiled linen weave, an inset gold hairline, and four gold
/// corner notches. Spawn these FIRST inside a panel's `.with_children` — they're all
/// `PositionType::Absolute`, so they overlay the panel without touching flex layout, and painting
/// order keeps them under content spawned after. The caller's panel `Node` should set
/// `border: border(2.0)` + a radius and use [`card_paint`]/`PANEL_HUD`.
pub fn chrome_layers(p: &mut RelatedSpawnerCommands<ChildOf>, linen: Handle<Image>) {
    let fill = |node: Node, extra_bg: Color| (node, BackgroundColor(extra_bg));
    // Linen weave (alpha baked into the texture).
    p.spawn((
        Node {
            position_type: PositionType::Absolute,
            top: Val::Px(0.0),
            left: Val::Px(0.0),
            right: Val::Px(0.0),
            bottom: Val::Px(0.0),
            ..default()
        },
        ImageNode::new(linen).with_mode(NodeImageMode::Tiled {
            tile_x: true,
            tile_y: true,
            stretch_value: 1.0,
        }),
    ));
    // Inset gold hairline.
    p.spawn((
        Node {
            position_type: PositionType::Absolute,
            top: Val::Px(3.0),
            left: Val::Px(3.0),
            right: Val::Px(3.0),
            bottom: Val::Px(3.0),
            border: border(1.0),
            border_radius: radius(R_PANEL - 4.0),
            ..default()
        },
        BorderColor::all(GOLD_HAIRLINE),
    ));
    // Corner notches.
    for (t, l) in [(true, true), (true, false), (false, true), (false, false)] {
        let mut node = Node {
            position_type: PositionType::Absolute,
            width: Val::Px(6.0),
            height: Val::Px(6.0),
            ..default()
        };
        if t { node.top = Val::Px(1.0) } else { node.bottom = Val::Px(1.0) }
        if l { node.left = Val::Px(1.0) } else { node.right = Val::Px(1.0) }
        p.spawn(fill(node, GOLD_NOTCH));
    }
}

/// Primary action button paint (bronze-gold — Play / Resume / Again). Label text should be `INK`.
pub fn btn_primary_paint() -> impl Bundle {
    (
        Button,
        Interaction::default(),
        BackgroundColor(PRIMARY),
        BorderColor::all(PRIMARY_BORDER),
        UiTransform::IDENTITY,
        Hoverable {
            rest_bg: PRIMARY,
            hover_bg: PRIMARY_HI,
            rest_border: PRIMARY_BORDER,
            hover_border: PRIMARY_BORDER,
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

/// Keycap (kbd) paint — a small raised key (warm iron, gold-dust edge).
pub fn keycap_paint() -> impl Bundle {
    (BackgroundColor(rgba(52, 43, 30, 0.95)), BorderColor::all(rgba(224, 184, 120, 0.22)))
}

/// A square icon image node.
pub fn icon(handle: Handle<Image>, px: f32) -> impl Bundle {
    (Node { width: Val::Px(px), height: Val::Px(px), ..default() }, ImageNode::new(handle))
}

/// A square icon, tinted when the atlas entry is a monochrome game-icon (`tintable`), left
/// untouched when it's a full-colour Twemoji/procedural raster.
pub fn icon_tinted(entry: (Handle<Image>, bool), px: f32, tint: Color) -> impl Bundle {
    let (handle, tintable) = entry;
    let mut img = ImageNode::new(handle);
    if tintable {
        img.color = tint;
    }
    (Node { width: Val::Px(px), height: Val::Px(px), ..default() }, img)
}

/// Spawn a **medallion** — the framed icon disc used by tree nodes, satchel slots, and quickslots.
/// `ring`/`bg` carry the state language (owned/buyable/locked...); `tint` inks monochrome icons.
pub fn medallion(
    p: &mut RelatedSpawnerCommands<ChildOf>,
    entry: Option<(Handle<Image>, bool)>,
    size: f32,
    ring: Color,
    bg: Color,
    tint: Color,
) {
    p.spawn((
        Node {
            width: Val::Px(size),
            height: Val::Px(size),
            align_items: AlignItems::Center,
            justify_content: JustifyContent::Center,
            border: border(1.5),
            border_radius: BorderRadius::all(Val::Percent(50.0)),
            flex_shrink: 0.0,
            ..default()
        },
        BackgroundColor(bg),
        BorderColor::all(ring),
    ))
    .with_children(|m| {
        if let Some(entry) = entry {
            m.spawn(icon_tinted(entry, size * 0.62, tint));
        }
    });
}

/// Spawn a **cost chip** — small pill with a coin/stone icon and a cost string. `tone` colours
/// both icon tint and text (gold = affordable, red = can't afford, faded = locked).
pub fn cost_chip(
    p: &mut RelatedSpawnerCommands<ChildOf>,
    font: &Handle<Font>,
    entry: Option<(Handle<Image>, bool)>,
    text: impl Into<String>,
    tone: Color,
    bg: Color,
) {
    p.spawn((
        Node {
            flex_direction: FlexDirection::Row,
            align_items: AlignItems::Center,
            column_gap: Val::Px(4.0),
            padding: UiRect::axes(Val::Px(7.0), Val::Px(2.0)),
            border_radius: radius(R_CELL),
            ..default()
        },
        BackgroundColor(bg),
    ))
    .with_children(|c| {
        if let Some(entry) = entry {
            c.spawn(icon_tinted(entry, 11.0, tone));
        }
        c.spawn(super::fonts::label(font, text, 12.0, tone));
    });
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

/// Spawn a small **✕ close button** for a panel header. The caller supplies a marker component
/// and handles its click/`FocusActivate` (usually `Modal::None`). `on_parchment` flips the
/// colours from dark-panel gold to parchment ink.
pub fn close_button(
    p: &mut RelatedSpawnerCommands<ChildOf>,
    font: &Handle<Font>,
    marker: impl Component,
    on_parchment: bool,
) {
    let (fg, bg, bd) = if on_parchment {
        (super::theme::INK, rgba(86, 58, 24, 0.12), rgba(86, 58, 24, 0.45))
    } else {
        (GOLD, BTN_BG, GOLD_HAIRLINE)
    };
    p.spawn((
        Button,
        Interaction::default(),
        super::focus::Focusable,
        Node {
            width: Val::Px(28.0),
            height: Val::Px(28.0),
            align_items: AlignItems::Center,
            justify_content: JustifyContent::Center,
            border: border(1.0),
            border_radius: radius(R_BTN),
            flex_shrink: 0.0,
            ..default()
        },
        BackgroundColor(bg),
        BorderColor::all(bd),
        marker,
    ))
    .with_children(|b| {
        b.spawn(super::fonts::label(font, "\u{2715}", 13.0, fg));
    });
}
