//! **Keyboard / gamepad panel navigation.** Mark any clickable panel node [`Focusable`]; while a
//! panel is open (i.e. focusables exist), arrow keys / d-pad move a gold focus ring between them
//! (nearest-in-direction by node centre) and **Enter / E / gamepad South** activates, emitting
//! [`FocusActivate`]. Panel interact systems treat that message exactly like
//! `Interaction::Pressed`, so mouse and keys share one code path — see
//! [`activated`] for the idiomatic check. Mouse hover also moves focus, which makes
//! hover == focus the single driver for detail strips and tooltips.
//!
//! Ungated on purpose: panels open inside `Modal::*` where the world-sim is frozen but UI must
//! stay live. The world's own E-interact / WASD systems are sim-gated, so keys don't double-fire.

use bevy::ecs::relationship::RelatedSpawnerCommands;
use bevy::prelude::*;
// UI nodes carry `UiGlobalTransform` (an Affine2), NOT `GlobalTransform` — a query asking for
// `GlobalTransform` on a UI node silently matches nothing. This bit once: every focus query came
// back empty, so focus reset to None each frame and hover/arrows/tooltips were all dead.
use bevy::ui::UiGlobalTransform;

use super::theme::{rgba, GOLD};

/// Marker: this node participates in arrow-key/d-pad focus navigation.
#[derive(Component, Default)]
pub struct Focusable;

/// The currently focused panel node (None when no panel is open or mouse took over).
#[derive(Resource, Default)]
pub struct UiFocus {
    pub current: Option<Entity>,
}

/// "The focused node was activated" (Enter / E / gamepad South) — consumed by panel interact
/// systems alongside `Interaction::Pressed`.
#[derive(Message)]
pub struct FocusActivate(pub Entity);

/// Sugar for interact systems: did this button get a real click OR a focus activation?
pub fn activated(
    entity: Entity,
    interaction: &Interaction,
    acts: &mut MessageReader<FocusActivate>,
) -> bool {
    *interaction == Interaction::Pressed || acts.read().any(|a| a.0 == entity)
}

pub struct FocusPlugin;

impl Plugin for FocusPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<UiFocus>().add_message::<FocusActivate>().add_systems(
            Update,
            (hover_steals_focus, key_nav, emit_activate, draw_ring).chain(),
        );
    }
}

/// Mouse hover moves focus, so the ring/detail strip never disagree with the cursor.
fn hover_steals_focus(
    mut focus: ResMut<UiFocus>,
    q: Query<(Entity, &Interaction), (With<Focusable>, Changed<Interaction>)>,
) {
    for (e, i) in &q {
        if *i == Interaction::Hovered || *i == Interaction::Pressed {
            focus.current = Some(e);
        }
    }
}

/// Centre of a focusable in physical px (consistent units are all the scoring needs).
fn centers(q: &Query<(Entity, &ComputedNode, &UiGlobalTransform), With<Focusable>>) -> Vec<(Entity, Vec2)> {
    q.iter().map(|(e, _, gt)| (e, gt.translation)).collect()
}

fn key_nav(
    mut focus: ResMut<UiFocus>,
    keys: Res<ButtonInput<KeyCode>>,
    pads: Query<&Gamepad>,
    q: Query<(Entity, &ComputedNode, &UiGlobalTransform), With<Focusable>>,
) {
    if q.is_empty() {
        focus.current = None;
        return;
    }
    let pad = |b: GamepadButton| pads.iter().any(|p| p.just_pressed(b));
    let dir: Vec2 = if keys.just_pressed(KeyCode::ArrowLeft) || pad(GamepadButton::DPadLeft) {
        Vec2::new(-1.0, 0.0)
    } else if keys.just_pressed(KeyCode::ArrowRight) || pad(GamepadButton::DPadRight) {
        Vec2::new(1.0, 0.0)
    } else if keys.just_pressed(KeyCode::ArrowUp) || pad(GamepadButton::DPadUp) {
        Vec2::new(0.0, -1.0)
    } else if keys.just_pressed(KeyCode::ArrowDown) || pad(GamepadButton::DPadDown) {
        Vec2::new(0.0, 1.0)
    } else {
        // Keep `current` valid even if the focused node despawned (e.g. rebuilt panel).
        if let Some(c) = focus.current {
            if q.get(c).is_err() {
                focus.current = None;
            }
        }
        return;
    };

    let pts = centers(&q);
    let cur = focus.current.and_then(|c| pts.iter().find(|(e, _)| *e == c).copied());
    focus.current = Some(match cur {
        // Nothing focused yet: start at the top-left-most node.
        None => {
            pts.iter().min_by(|a, b| (a.1.y + a.1.x).total_cmp(&(b.1.y + b.1.x))).unwrap().0
        }
        Some((ce, cp)) => {
            // Nearest node in the pressed direction: forward distance + weighted sideways drift.
            pts.iter()
                .filter(|(e, _)| *e != ce)
                .filter_map(|(e, p)| {
                    let d = *p - cp;
                    let fwd = d.dot(dir);
                    (fwd > 4.0).then(|| (*e, fwd + 1.8 * d.dot(dir.perp()).abs()))
                })
                .min_by(|a, b| a.1.total_cmp(&b.1))
                .map(|(e, _)| e)
                .unwrap_or(ce)
        }
    });
}

fn emit_activate(
    focus: Res<UiFocus>,
    keys: Res<ButtonInput<KeyCode>>,
    pads: Query<&Gamepad>,
    mut out: MessageWriter<FocusActivate>,
) {
    let Some(e) = focus.current else { return };
    if keys.just_pressed(KeyCode::Enter)
        || keys.just_pressed(KeyCode::NumpadEnter)
        || keys.just_pressed(KeyCode::KeyE)
        || pads.iter().any(|p| p.just_pressed(GamepadButton::South))
    {
        out.write(FocusActivate(e));
    }
}

/// Gold outline on the focused node, cleared everywhere else. `Outline` doesn't affect layout.
fn draw_ring(
    focus: Res<UiFocus>,
    mut commands: Commands,
    q: Query<(Entity, Option<&Outline>), With<Focusable>>,
) {
    for (e, outline) in &q {
        let focused = focus.current == Some(e);
        if focused && outline.is_none() {
            commands.entity(e).try_insert(Outline {
                width: Val::Px(2.0),
                offset: Val::Px(2.0),
                color: GOLD,
            });
        } else if !focused && outline.is_some() {
            commands.entity(e).try_remove::<Outline>();
        }
    }
}

// ── Tooltip ──────────────────────────────────────────────────────────────────────────────

/// Attach to any `Focusable`: while it's focused/hovered, the global tooltip shows this text.
#[derive(Component)]
pub struct Tip(pub String);

#[derive(Component)]
struct TipRoot;
#[derive(Component)]
struct TipText;

pub struct TooltipPlugin;

impl Plugin for TooltipPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Startup, spawn_tip).add_systems(Update, drive_tip.after(draw_ring));
    }
}

fn spawn_tip(mut commands: Commands, fonts: Res<super::fonts::UiFonts>) {
    commands
        .spawn((
            Node {
                position_type: PositionType::Absolute,
                padding: UiRect::axes(Val::Px(10.0), Val::Px(6.0)),
                border: UiRect::all(Val::Px(1.0)),
                border_radius: super::theme::radius(super::theme::R_BTN),
                max_width: Val::Px(260.0),
                ..default()
            },
            BackgroundColor(rgba(20, 16, 11, 0.96)),
            BorderColor::all(super::theme::GOLD_HAIRLINE),
            GlobalZIndex(220),
            Visibility::Hidden,
            TipRoot,
        ))
        .with_children(|t: &mut RelatedSpawnerCommands<ChildOf>| {
            t.spawn((
                super::fonts::label(&fonts.regular, "", 12.5, super::theme::TEXT),
                TipText,
            ));
        });
}

fn drive_tip(
    focus: Res<UiFocus>,
    tips: Query<(&Tip, &ComputedNode, &UiGlobalTransform)>,
    mut root: Query<(&mut Node, &mut Visibility), With<TipRoot>>,
    mut text: Query<&mut Text, With<TipText>>,
    windows: Query<&Window>,
) {
    let Ok((mut node, mut vis)) = root.single_mut() else { return };
    let shown = focus.current.and_then(|e| tips.get(e).ok());
    let Some((tip, cn, gt)) = shown else {
        *vis = Visibility::Hidden;
        return;
    };
    let Ok(window) = windows.single() else { return };
    let sf = cn.inverse_scale_factor();
    let centre = gt.translation * sf;
    let half = cn.size() * sf * 0.5;
    // Below the node, clamped on-screen (the tooltip's own size isn't known pre-layout; the
    // 270px right margin keeps even a max-width tip inside the window).
    let x = (centre.x - 60.0).clamp(8.0, (window.width() - 270.0).max(8.0));
    let y = (centre.y + half.y + 8.0).min(window.height() - 80.0);
    node.left = Val::Px(x);
    node.top = Val::Px(y);
    if let Ok(mut t) = text.single_mut() {
        **t = tip.0.clone();
    }
    *vis = Visibility::Visible;
}
