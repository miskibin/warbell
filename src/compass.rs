//! **Strip compass** — a thin Elden-Ring-style heading bar pinned top-centre, above the rest of the
//! HUD. A fixed gold needle marks the way the camera faces; behind it a scale of tick marks and the
//! four cardinal letters (N/E/S/W) slides as the player turns. Two landmark icons ride the same scale
//! — a gold **house** for the home keep (world origin) and a red **axe** for **Gnashfang Hold** — so
//! they only swing into view when you turn toward them (and slide off the edges when you don't).
//!
//! Everything is a flat coloured `Node` (the two landmarks are tinted icon silhouettes); the strip
//! clips its own overflow, so ticks/icons just slide out at the ends. One `Update` system repositions
//! every child each frame from the camera yaw.

use std::f32::consts::{PI, TAU};

use bevy::prelude::*;

use crate::boss::Boss;
use crate::chest::Chest;
use crate::dying::Dying;
use crate::game_state::AppState;
use crate::orks::Ork;
use crate::player::Hero;
use crate::ui::fonts::{label, UiFonts, FONT_CAPTION};
use crate::ui::theme::*;
use crate::warlord::Warlord;

// ── Layout (px) ──────────────────────────────────────────────────────────────────────
const STRIP_W: f32 = 440.0;
const STRIP_H: f32 = 36.0;
/// Degrees of heading visible to each side of the needle (the half-window the strip spans).
const HALF_SPAN: f32 = 70.0;
const CENTER_X: f32 = STRIP_W / 2.0;
const PX_PER_DEG: f32 = (STRIP_W / 2.0) / HALF_SPAN;
const BASELINE_Y: f32 = 14.0;
/// Minor tick every this many degrees; cardinals (every 90°) get a taller tick + a letter.
const TICK_STEP_DEG: f32 = 22.5;
/// Landmark icon size + how far below the baseline it sits.
const PIP: f32 = 15.0;
const PIP_TOP: f32 = 20.0;

// ── Live radar blips (enemies + loot) ────────────────────────────────────────────────
// Three kinds of moving/discoverable target ride the same scale as the fixed landmarks, but
// only when the hero is within range — so the strip stays a heading bar that lights up with
// what's actually near. Each kind has a small pool of pre-spawned nodes refilled every frame
// from the nearest N targets; spare pool nodes hide.
/// Orks (camp warbands + night-wave invaders) show as red dots within this range (world units).
const ORK_RANGE: f32 = 46.0;
/// Bosses (biome wardens + the Warlord) show as a danger mark — only once you're this close.
const BOSS_RANGE: f32 = 78.0;
/// Unopened loot chests show as a gold coin within this range.
const CHEST_RANGE: f32 = 50.0;
/// Pool sizes (max blips drawn per kind — the nearest ones win; the rest are silently dropped).
const ORK_POOL: usize = 14;
const BOSS_POOL: usize = 5;
const CHEST_POOL: usize = 6;

#[derive(Clone, Copy, PartialEq)]
enum Blip {
    Ork,
    Boss,
    Chest,
}
impl Blip {
    fn range(self) -> f32 {
        match self {
            Blip::Ork => ORK_RANGE,
            Blip::Boss => BOSS_RANGE,
            Blip::Chest => CHEST_RANGE,
        }
    }
}

/// A pooled live-radar blip: its `kind` decides which target list fills it each frame.
#[derive(Component)]
struct CompassBlip {
    kind: Blip,
}

#[derive(Component)]
struct CompassRoot;
/// A tick or cardinal letter pinned to an absolute world bearing (radians); `half_w` centres it.
#[derive(Component)]
struct CompassMark {
    bearing: f32,
    half_w: f32,
}
/// A landmark icon: `home` rides to the keep (origin), else to Gnashfang Hold's gate.
#[derive(Component)]
struct CompassPip {
    home: bool,
}

pub struct CompassPlugin;
impl Plugin for CompassPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Startup, setup_compass)
            .add_systems(Update, (update_compass, update_blips));
    }
}

/// Bearing of an XZ direction: 0 = North (−Z), +90° = East (+X), 180° = South, 270° = West.
fn bearing_of(dir: Vec2) -> f32 {
    dir.x.atan2(-dir.y)
}

/// Fold an angle into [−π, π].
fn wrap_pi(a: f32) -> f32 {
    let mut x = a % TAU;
    if x > PI {
        x -= TAU;
    } else if x < -PI {
        x += TAU;
    }
    x
}

fn setup_compass(mut commands: Commands, fonts: Res<UiFonts>, assets: Res<AssetServer>) {
    // Tintable game-icon silhouettes (solid colour → high contrast over any scene behind the strip).
    let home_icon = assets.load("icons/gameicons/stat_pop.png"); // a house → the home keep
    let ork_icon = assets.load("icons/gameicons/axe.png"); // an axe → Gnashfang Hold (the orks)
    let boss_icon = assets.load("icons/gameicons/sym_warn.png"); // a warning mark → a nearby boss
    let coin_icon = assets.load("icons/gameicons/stat_gold.png"); // coins → an unopened loot chest
    // Full-width wrapper centres the fixed-width strip on screen.
    commands
        .spawn(Node {
            position_type: PositionType::Absolute,
            top: Val::Px(10.0),
            left: Val::Px(0.0),
            width: Val::Percent(100.0),
            flex_direction: FlexDirection::Row,
            justify_content: JustifyContent::Center,
            ..default()
        })
        .with_children(|wrap| {
            wrap.spawn((
                Node {
                    width: Val::Px(STRIP_W),
                    height: Val::Px(STRIP_H),
                    overflow: Overflow::clip(),
                    ..default()
                },
                GlobalZIndex(95), // above the rest of the HUD — the compass owns the very top
                CompassRoot,
            ))
            .with_children(|s| {
                // Baseline.
                s.spawn((
                    Node {
                        position_type: PositionType::Absolute,
                        left: Val::Px(0.0),
                        top: Val::Px(BASELINE_Y),
                        width: Val::Percent(100.0),
                        height: Val::Px(1.5),
                        ..default()
                    },
                    BackgroundColor(rgba(235, 225, 205, 0.30)),
                ));
                // Fixed gold heading needle at dead centre.
                s.spawn((
                    Node {
                        position_type: PositionType::Absolute,
                        left: Val::Px(CENTER_X - 1.0),
                        top: Val::Px(6.0),
                        width: Val::Px(2.0),
                        height: Val::Px(15.0),
                        ..default()
                    },
                    BackgroundColor(GOLD),
                ));
                // Tick marks at every TICK_STEP_DEG; cardinals are taller.
                let n = (360.0 / TICK_STEP_DEG) as i32;
                for k in 0..n {
                    let deg = k as f32 * TICK_STEP_DEG;
                    let cardinal = (deg % 90.0).abs() < 0.01;
                    let h = if cardinal { 9.0 } else { 5.0 };
                    s.spawn((
                        Node {
                            position_type: PositionType::Absolute,
                            top: Val::Px(BASELINE_Y - (h - 5.0)),
                            width: Val::Px(1.5),
                            height: Val::Px(h),
                            ..default()
                        },
                        BackgroundColor(rgba(235, 225, 205, if cardinal { 0.8 } else { 0.5 })),
                        CompassMark { bearing: deg.to_radians(), half_w: 0.75 },
                    ));
                }
                // Cardinal letters above the line.
                for (deg, ch) in [(0.0, "N"), (90.0, "E"), (180.0, "S"), (270.0, "W")] {
                    s.spawn((
                        label(&fonts.semibold, ch, FONT_CAPTION, TEXT),
                        Node { position_type: PositionType::Absolute, top: Val::Px(0.0), ..default() },
                        CompassMark { bearing: (deg as f32).to_radians(), half_w: 3.5 },
                    ));
                }
                // Landmark icons below the line (gold house = keep, red axe = Gnashfang Hold).
                for (home, icon, tint) in [(true, home_icon, GOLD), (false, ork_icon, RED)] {
                    let mut img = ImageNode::new(icon);
                    img.color = tint;
                    s.spawn((
                        Node {
                            position_type: PositionType::Absolute,
                            top: Val::Px(PIP_TOP),
                            width: Val::Px(PIP),
                            height: Val::Px(PIP),
                            ..default()
                        },
                        img,
                        CompassPip { home },
                    ));
                }

                // ── Live-radar blip pools (start hidden; `update_blips` shows the nearest few) ──
                // Orks: small red dots straddling the baseline.
                for _ in 0..ORK_POOL {
                    s.spawn((
                        Node {
                            position_type: PositionType::Absolute,
                            top: Val::Px(BASELINE_Y - 3.0),
                            width: Val::Px(7.0),
                            height: Val::Px(7.0),
                            border_radius: BorderRadius::all(Val::Percent(50.0)),
                            display: Display::None,
                            ..default()
                        },
                        BackgroundColor(RED),
                        CompassBlip { kind: Blip::Ork },
                    ));
                }
                // Bosses: a danger mark below the line, larger so it reads over the dots.
                for _ in 0..BOSS_POOL {
                    let mut img = ImageNode::new(boss_icon.clone());
                    img.color = RED;
                    s.spawn((
                        Node {
                            position_type: PositionType::Absolute,
                            top: Val::Px(PIP_TOP - 2.0),
                            width: Val::Px(18.0),
                            height: Val::Px(18.0),
                            display: Display::None,
                            ..default()
                        },
                        img,
                        CompassBlip { kind: Blip::Boss },
                    ));
                }
                // Chests: a gold coin below the line.
                for _ in 0..CHEST_POOL {
                    let mut img = ImageNode::new(coin_icon.clone());
                    img.color = GOLD;
                    s.spawn((
                        Node {
                            position_type: PositionType::Absolute,
                            top: Val::Px(PIP_TOP + 1.0),
                            width: Val::Px(13.0),
                            height: Val::Px(13.0),
                            display: Display::None,
                            ..default()
                        },
                        img,
                        CompassBlip { kind: Blip::Chest },
                    ));
                }
            });
        });
}

#[allow(clippy::type_complexity)]
fn update_compass(
    state: Res<State<AppState>>,
    cam_q: Query<&GlobalTransform, With<Camera3d>>,
    hero_q: Query<&GlobalTransform, With<Hero>>,
    mut root_q: Query<&mut Node, (With<CompassRoot>, Without<CompassMark>, Without<CompassPip>)>,
    mut marks: Query<(&CompassMark, &mut Node), Without<CompassPip>>,
    mut pips: Query<(&CompassPip, &mut Node), Without<CompassMark>>,
) {
    // Only ride along during live play; hide on menus / pause / game-over.
    let playing = *state.get() == AppState::Playing;
    if let Ok(mut n) = root_q.single_mut() {
        n.display = if playing { Display::Flex } else { Display::None };
    }
    if !playing {
        return;
    }
    let (Ok(cam), Ok(hero)) = (cam_q.single(), hero_q.single()) else { return };

    let fwd = cam.forward();
    let cam_bearing = bearing_of(Vec2::new(fwd.x, fwd.z));
    // Map a world bearing to an x on the strip (clamps off-strip → clipped by overflow).
    let place = |bearing: f32| CENTER_X + wrap_pi(bearing - cam_bearing).to_degrees() * PX_PER_DEG;

    for (mark, mut node) in &mut marks {
        node.left = Val::Px(place(mark.bearing) - mark.half_w);
    }

    let hero_xz = hero.translation().xz();
    for (pip, mut node) in &mut pips {
        let target = if pip.home { Vec2::ZERO } else { crate::ork_fortress::GATE };
        let d = target - hero_xz;
        // Standing on top of the target → no meaningful heading; park it off-strip.
        if d.length_squared() < 1.0 {
            node.left = Val::Px(-100.0);
            continue;
        }
        node.left = Val::Px(place(bearing_of(d)) - PIP / 2.0);
    }
}

/// Nearest `cap` targets within `range` of `hero`, each as `(distance, bearing)`, closest first.
fn nearest(
    targets: impl Iterator<Item = Vec2>,
    hero: Vec2,
    range: f32,
    cap: usize,
) -> Vec<(f32, f32)> {
    let mut v: Vec<(f32, f32)> = targets
        .filter_map(|p| {
            let d = p - hero;
            let dist = d.length();
            // Skip anything on top of the hero (no meaningful heading) or out of range.
            (dist > 0.5 && dist < range).then_some((dist, bearing_of(d)))
        })
        .collect();
    v.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal));
    v.truncate(cap);
    v
}

/// Fills the live-radar blip pools each frame from the nearest orks / bosses / loot chests, so the
/// compass lights up with what's actually near and goes quiet when nothing is. Spare pool nodes hide.
#[allow(clippy::type_complexity)]
fn update_blips(
    state: Res<State<AppState>>,
    cam_q: Query<&GlobalTransform, With<Camera3d>>,
    hero_q: Query<&GlobalTransform, With<Hero>>,
    ork_q: Query<&GlobalTransform, (With<Ork>, Without<Dying>)>,
    boss_q: Query<&GlobalTransform, (With<Boss>, Without<Dying>)>,
    warlord_q: Query<&GlobalTransform, (With<Warlord>, Without<Dying>)>,
    chest_q: Query<(&GlobalTransform, &Chest)>,
    mut blips: Query<(&CompassBlip, &mut Node, Option<&mut BackgroundColor>)>,
) {
    if *state.get() != AppState::Playing {
        return; // the pool nodes are children of the strip, which `update_compass` already hides.
    }
    let (Ok(cam), Ok(hero)) = (cam_q.single(), hero_q.single()) else { return };
    let hero_xz = hero.translation().xz();

    let fwd = cam.forward();
    let cam_bearing = bearing_of(Vec2::new(fwd.x, fwd.z));
    let place = |bearing: f32| CENTER_X + wrap_pi(bearing - cam_bearing).to_degrees() * PX_PER_DEG;

    let orks = nearest(ork_q.iter().map(|t| t.translation().xz()), hero_xz, ORK_RANGE, ORK_POOL);
    let bosses = nearest(
        boss_q.iter().chain(warlord_q.iter()).map(|t| t.translation().xz()),
        hero_xz,
        BOSS_RANGE,
        BOSS_POOL,
    );
    let chests = nearest(
        chest_q.iter().filter(|(_, c)| !c.opened).map(|(t, _)| t.translation().xz()),
        hero_xz,
        CHEST_RANGE,
        CHEST_POOL,
    );

    let (mut oi, mut bi, mut ci) = (0usize, 0usize, 0usize);
    for (blip, mut node, bg) in &mut blips {
        let (list, idx, half) = match blip.kind {
            Blip::Ork => (&orks, &mut oi, 3.5),
            Blip::Boss => (&bosses, &mut bi, 9.0),
            Blip::Chest => (&chests, &mut ci, 6.5),
        };
        let i = *idx;
        *idx += 1;
        match list.get(i) {
            Some(&(dist, bearing)) => {
                node.display = Display::Flex;
                node.left = Val::Px(place(bearing) - half);
                // Ork dots fade with distance so a far-off camp reads fainter than one on your heels.
                if blip.kind == Blip::Ork {
                    if let Some(mut c) = bg {
                        let a = 0.4 + 0.6 * (1.0 - dist / blip.kind.range()).clamp(0.0, 1.0);
                        *c = BackgroundColor(RED.with_alpha(a));
                    }
                }
            }
            None => node.display = Display::None,
        }
    }
}
