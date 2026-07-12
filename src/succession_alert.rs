//! **Last-of-the-line alert.** Succession is invisible until it runs out: when the town has no
//! heirs left (`town.population == 0`), the hero's *next* death ends the run, and nothing on screen
//! used to say so. This module makes that unmissable:
//!
//! - a one-time **stinger** ("LAST OF THE LINE") the moment the count hits 0 — which lands exactly
//!   as you possess the final townsperson (`succession::drive_succession` drops the headcount to 0
//!   at the transform), and
//! - a **persistent banner** that stays up the whole time you have 0 heirs and clears itself when
//!   the town regrows one (the larder/food keeps the bloodline coming back).
//!
//! No new saved/derived state — everything is read off `TownRes` each frame.

use bevy::prelude::*;
use bevy::time::Real;

use crate::game_state::AppState;
use crate::ui::fonts::{label, UiFonts, FONT_BODY};
use crate::ui::theme::{radius, rgb, rgba, shadow_card, RED_BORDER, R_CARD};
use crate::ui::widgets::border;

/// Stinger lifetime (real secs) — fade in, hold, fade out.
const STINGER_DUR: f32 = 2.6;

/// The persistent "no heirs" banner root (visibility toggled on `population == 0`).
#[derive(Component)]
struct BannerRoot;

/// The one-shot centered stinger text.
#[derive(Component)]
struct Stinger {
    born: f32,
}

pub struct SuccessionAlertPlugin;

impl Plugin for SuccessionAlertPlugin {
    fn build(&self, app: &mut App) {
        // Campaign-only: the last-heir alert banner is part of the hero succession mechanic.
        app.add_systems(Startup, setup_banner.run_if(crate::rts::in_campaign))
            .add_systems(Update, (watch_heirs, drive_stingers).run_if(crate::rts::in_campaign))
            .add_systems(OnExit(AppState::StartScreen), clear_stingers.run_if(crate::rts::in_campaign))
            .add_systems(OnExit(AppState::GameOver), clear_stingers.run_if(crate::rts::in_campaign));

        // `FOREST_LASTHERO=1`: empty the heir pool a beat after boot so the stinger + persistent
        // banner can be shot in isolation (same staging-hook style as the other `FOREST_*` vars).
        if std::env::var("FOREST_LASTHERO").is_ok() {
            app.add_systems(Update, force_last_hero.run_if(in_state(AppState::Playing)).run_if(crate::rts::in_campaign));
        }
    }
}

/// `FOREST_LASTHERO` staging hook: once, ~2 s in, drop the town to 0 heirs so the alert fires.
fn force_last_hero(
    time: Res<Time>,
    mut town: ResMut<crate::town::TownRes>,
    mut done: Local<bool>,
    mut elapsed: Local<f32>,
) {
    if *done {
        return;
    }
    *elapsed += time.delta_secs();
    if *elapsed < 2.0 {
        return;
    }
    *done = true;
    town.0.population = 0;
}

/// Spawn the (hidden) persistent banner once. `UiFonts` is inserted in `UiKitPlugin::build`, so it
/// exists by `Startup`.
fn setup_banner(mut commands: Commands, fonts: Res<UiFonts>) {
    commands
        .spawn((
            BannerRoot,
            Visibility::Hidden,
            Node {
                position_type: PositionType::Absolute,
                top: Val::Px(100.0),
                left: Val::Percent(50.0),
                margin: UiRect::left(Val::Px(-185.0)),
                width: Val::Px(370.0),
                justify_content: JustifyContent::Center,
                ..default()
            },
            GlobalZIndex(81),
            bevy::ui::FocusPolicy::Pass,
        ))
        .with_children(|root| {
            root.spawn((
                Node {
                    padding: UiRect::axes(Val::Px(20.0), Val::Px(10.0)),
                    border: border(2.0),
                    border_radius: radius(R_CARD),
                    ..default()
                },
                BackgroundColor(rgba(46, 12, 12, 0.92)),
                BorderColor::all(RED_BORDER),
                shadow_card(),
            ))
            .with_children(|p| {
                p.spawn(label(
                    &fonts.bold,
                    "LAST OF THE LINE — if you fall, the war is lost",
                    FONT_BODY,
                    rgb(255, 206, 198),
                ));
            });
        });
}

/// Track the heir count: fire the stinger on the 1→0 edge (while playing) and keep the banner shown
/// for as long as the count sits at 0.
fn watch_heirs(
    rtime: Res<Time<Real>>,
    town: Res<crate::town::TownRes>,
    app: Res<State<AppState>>,
    fonts: Res<UiFonts>,
    mut commands: Commands,
    mut banner: Query<&mut Visibility, With<BannerRoot>>,
    mut prev: Local<Option<u32>>,
) {
    let pop = town.0.population;
    let playing = *app.get() == AppState::Playing;

    // One-time stinger on the drop to zero (only while actually playing — never on a menu/load edge).
    if playing && prev.is_some_and(|p| p > 0) && pop == 0 {
        spawn_stinger(&mut commands, &fonts, rtime.elapsed_secs());
    }
    *prev = Some(pop);

    let want = if pop == 0 && playing { Visibility::Visible } else { Visibility::Hidden };
    if let Ok(mut v) = banner.single_mut() {
        if *v != want {
            *v = want;
        }
    }
}

fn spawn_stinger(commands: &mut Commands, fonts: &UiFonts, now: f32) {
    commands.spawn((
        Stinger { born: now },
        Text::new("LAST OF THE LINE"),
        TextFont { font: fonts.display.clone().into(), font_size: 42.0.into(), ..default() },
        TextColor(rgb(255, 120, 96)),
        TextLayout { justify: Justify::Center, ..default() },
        Node {
            position_type: PositionType::Absolute,
            top: Val::Percent(34.0),
            left: Val::Px(0.0),
            width: Val::Percent(100.0),
            ..default()
        },
        GlobalZIndex(90),
        bevy::ui::FocusPolicy::Pass,
    ));
}

/// Fade the stinger in/hold/out, then despawn.
fn drive_stingers(
    rtime: Res<Time<Real>>,
    mut commands: Commands,
    mut q: Query<(Entity, &Stinger, &mut TextColor)>,
) {
    let now = rtime.elapsed_secs();
    for (e, s, mut col) in &mut q {
        let t = (now - s.born) / STINGER_DUR;
        if t >= 1.0 {
            commands.entity(e).try_despawn();
            continue;
        }
        let a = if t < 0.10 {
            t / 0.10
        } else if t < 0.70 {
            1.0
        } else {
            (1.0 - (t - 0.70) / 0.30).clamp(0.0, 1.0)
        };
        col.0 = col.0.with_alpha(a);
    }
}

fn clear_stingers(mut commands: Commands, q: Query<Entity, With<Stinger>>) {
    for e in &q {
        commands.entity(e).try_despawn();
    }
}
