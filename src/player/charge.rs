//! **Heavy-Strike charge bar** — the bottom-centre loading bar that fills while you hold LMB to
//! wind up a [`combat`](super::combat) Heavy Strike, plus the one-time "Hold LMB" discovery tip.
//!
//! The bar is purely a *readout* of `Hero::charge_t` (owned by `combat::player_attack`): shown only
//! once the hold passes `CHARGE_GRACE` (so a normal tap never flashes it), filling `GRACE→THRESHOLD`,
//! gold when there's stamina to spend on release and greyed-red when there isn't, and flashing
//! "RELEASE!" once full. It is the feature's main affordance — the moment a player holds a beat too
//! long the bar appears and teaches itself, no help page required.

use bevy::prelude::*;

use crate::game_state::AppState;
use crate::ui::fonts::{label, UiFonts};
use crate::ui::notice::Notice;
use crate::ui::theme::*;
use crate::ui::widgets;

use super::combat::{CHARGE_GRACE, CHARGE_THRESHOLD, HEAVY_STAMINA_COST};
use super::{Hero, HeroHealth, HeroState, PlayMode};

/// Root container (shown/hidden per charge state).
#[derive(Component)]
pub(crate) struct ChargeBarRoot;
/// The growing fill quad (width + colour driven by the charge).
#[derive(Component)]
pub(crate) struct ChargeBarFill;
/// The caption under the bar ("HEAVY" / "RELEASE!" / "LOW STAMINA").
#[derive(Component)]
pub(crate) struct ChargeBarCaption;

const BAR_W: f32 = 220.0;

/// Spawn the (initially hidden) charge bar, centred just above the bottom HUD edge.
pub(crate) fn spawn_charge_bar(mut commands: Commands, fonts: Res<UiFonts>) {
    commands
        .spawn((
            Node {
                position_type: PositionType::Absolute,
                bottom: Val::Px(86.0),
                left: Val::Percent(50.0),
                margin: UiRect::left(Val::Px(-BAR_W / 2.0)),
                width: Val::Px(BAR_W),
                flex_direction: FlexDirection::Column,
                align_items: AlignItems::Center,
                row_gap: Val::Px(4.0),
                display: Display::None, // shown only while charging
                ..default()
            },
            GlobalZIndex(30),
            bevy::ui::FocusPolicy::Pass,
            ChargeBarRoot,
        ))
        .with_children(|root| {
            // Track + fill.
            root.spawn((
                Node {
                    width: Val::Px(BAR_W),
                    height: Val::Px(13.0),
                    border: widgets::border(1.0),
                    border_radius: radius(R_SLOT),
                    overflow: Overflow::clip(),
                    ..default()
                },
                BackgroundColor(SLOT_BG),
                BorderColor::all(SLOT_BORDER),
            ))
            .with_children(|track| {
                track.spawn((
                    Node { width: Val::Percent(0.0), height: Val::Percent(100.0), ..default() },
                    BackgroundColor(GOLD),
                    ChargeBarFill,
                ));
            });
            // Caption.
            root.spawn((label(&fonts.bold, "HEAVY", 11.0, GOLD), ChargeBarCaption));
        });
}

/// Drive the bar each frame off `Hero::charge_t` + stamina: show/hide, fill width, colour, caption.
#[allow(clippy::type_complexity)]
pub(crate) fn sync_charge_bar(
    time: Res<Time>,
    mode: Res<PlayMode>,
    app: Res<State<AppState>>,
    hero_q: Query<(&Hero, &HeroHealth)>,
    mut root_q: Query<&mut Node, (With<ChargeBarRoot>, Without<ChargeBarFill>)>,
    mut fill_q: Query<(&mut Node, &mut BackgroundColor), With<ChargeBarFill>>,
    mut cap_q: Query<(&mut Text, &mut TextColor), With<ChargeBarCaption>>,
) {
    let Ok(mut root) = root_q.single_mut() else { return };
    let playing = *mode == PlayMode::Play && *app.get() == AppState::Playing;
    let charge_t = hero_q.single().map(|(h, _)| h.charge_t).unwrap_or(-1.0);
    if !playing || charge_t <= CHARGE_GRACE {
        root.display = Display::None;
        return;
    }
    root.display = Display::Flex;
    let Ok((hero, hh)) = hero_q.single() else { return };

    // Fill maps GRACE→THRESHOLD onto 0→1 so the bar appears empty and tops out exactly at the
    // qualifying hold.
    let frac = ((hero.charge_t - CHARGE_GRACE) / (CHARGE_THRESHOLD - CHARGE_GRACE)).clamp(0.0, 1.0);
    let affordable = hh.stamina >= HEAVY_STAMINA_COST;
    let ready = frac >= 1.0;

    if let Ok((mut fnode, mut fbg)) = fill_q.single_mut() {
        fnode.width = Val::Percent(frac * 100.0);
        fbg.0 = if !affordable {
            rgb(176, 96, 84) // dull red — not enough stamina to spend on release
        } else if ready {
            // Pulse bright at the ready point so the release cue reads at a glance.
            let pulse = 0.7 + 0.3 * (time.elapsed_secs() * 9.0).sin().abs();
            Color::WHITE.mix(&GOLD, 1.0 - pulse)
        } else {
            GOLD
        };
    }
    if let Ok((mut text, mut tcol)) = cap_q.single_mut() {
        let (msg, col) = if !affordable {
            ("LOW STAMINA", rgb(220, 150, 140))
        } else if ready {
            ("RELEASE!", rgb(255, 244, 210))
        } else {
            ("HEAVY", GOLD)
        };
        if text.0 != msg {
            text.0 = msg.into();
        }
        tcol.0 = col;
    }
}

/// One-time discovery hint: the first time the hero is near an enemy in play, surface
/// "Hold [LMB] for a Heavy Strike" via the [`Notice`] queue. A `Local` flag fires it once per
/// process — a Continue is in-process so it won't re-nag; a fresh New Game relaunches the exe, where
/// reminding once more is fine. Backstop on top of the self-teaching bar.
pub(crate) fn heavy_tip(
    time: Res<Time>,
    mode: Res<PlayMode>,
    app: Res<State<AppState>>,
    hero: Res<HeroState>,
    orks: Query<&GlobalTransform, (With<crate::orks::Ork>, Without<crate::dying::Dying>)>,
    mut notice: ResMut<Notice>,
    mut shown: Local<bool>,
) {
    if *shown || *mode != PlayMode::Play || *app.get() != AppState::Playing || !hero.alive {
        return;
    }
    // Trigger once an ork is within striking distance — combat is imminent, so the hint is timely.
    let near = orks.iter().any(|gt| {
        let p = gt.translation();
        Vec2::new(p.x, p.z).distance(hero.pos) <= 10.0
    });
    if near {
        *shown = true;
        notice.push("Hold [LMB] for a Heavy Strike", time.elapsed_secs_f64());
    }
}
