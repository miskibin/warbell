//! **RTS skirmish HUD** ("Potyczka"). Three persistent, `Modal`-ungated pieces spawned once after
//! the icon atlas loads (so they stay visible through pause / panels):
//!
//!  1. **Top bar** (top-centre): the four resource counters (wood/stone/gold/food) + population
//!     `cur/cap`, read live off [`RtsBanks`]/[`RtsPop`] for the player side. Reuses the campaign
//!     stat-row chrome (`crate::hud`).
//!  2. **Build strip** (bottom-right): one button per placeable [`BuildingKind`] (all but the free
//!     Town Hall), each with a cost row; clicking sets [`Placing`] (click again = cancel).
//!     Unaffordable buttons grey out; the active one is highlighted. Template: `town::spawn_build_strip`.
//!  3. **Selection panel** (bottom-left): hidden with nothing selected. Units → per-kind icon +
//!     count. A single building → its name + HP bar; a barracks additionally shows the two train
//!     buttons (Miecznik / Łucznik), the FIFO queue slots, and a training progress bar.
//!
//! Toasts reuse the shared [`Notice`] queue for the two blocked HUD interactions ("Za mało
//! surowców" / "Limit populacji").
//!
//! Everything here is a `bevy_ui` `Button`/`Interaction`, so pointer events over the HUD are
//! consumed by the widgets (the world-pick siblings additionally gate on cursor-over-UI / `Placing`).

use bevy::ecs::relationship::RelatedSpawnerCommands;
use bevy::prelude::*;

use crate::ui::fonts::{label, UiFonts};
use crate::ui::notice::Notice;
use crate::ui::theme::*;
use crate::ui::widgets::{self, border};
use crate::ui::IconAtlas;

use super::{
    building_def, in_skirmish, train_cost, BuildingKind, Cost, Placing, RtsBanks, RtsBuilding,
    RtsPop, RtsUnit, Selected, Side, TrainOrder, TrainQueue, UnitKind, TRAIN_QUEUE_DEPTH, TRAIN_SECS,
};

/// The shared vitals component RTS bodies/buildings carry (the campaign combat `Health`, re-exported
/// crate-wide from `player`). Reads are defensive (`Option`), so a missing component just renders a
/// full bar — and the integrator can repoint this one import if the build/units siblings settled on
/// a different `Health` type.
use crate::player::Health;

// ── per-resource + panel colours ──────────────────────────────────────────────────────────────
const WOOD_COL: Color = rgb(190, 150, 100);
const FOOD_COL: Color = rgb(150, 200, 120);
const POP_COL: Color = rgb(235, 224, 180);
/// Warm iron pill bg used by the top bar (matches the campaign stat bar).
const PILL_BG: Color = rgb(27, 22, 16);
/// Dark wash laid over an unaffordable/disabled button so it visibly recedes.
const DIM_BG: Color = rgba(0, 0, 0, 0.32);
const DIM_ALPHA: f32 = 0.3;

// ── markers ─────────────────────────────────────────────────────────────────────────────────

/// Which top-bar counter a `Text` node feeds (one query drives all five).
#[derive(Component, Clone, Copy, PartialEq)]
enum ResKind {
    Wood,
    Stone,
    Gold,
    Food,
    Pop,
}
#[derive(Component)]
struct ResText(ResKind);

/// A build-strip button → the building it places.
#[derive(Component, Clone, Copy)]
struct BuildStripBtn(BuildingKind);

/// The two bottom-left panels; exactly one is shown at a time (or neither).
#[derive(Component)]
struct SelUnitPanel;
#[derive(Component)]
struct SelBldgPanel;
/// A rebuilt-each-frame unit-count chip inside [`SelUnitPanel`].
#[derive(Component)]
struct SelUnitRow;

/// The building panel's `Text` nodes (one query, matched by variant).
#[derive(Component, Clone, Copy)]
enum BldgText {
    Name,
    Hp,
    Queue(usize),
}
/// The building panel's live-`Node` bars (one query, matched by variant).
#[derive(Component, Clone, Copy)]
enum BldgBar {
    /// HP fill quad (drives `width`).
    HpFill,
    /// The whole train section (barracks-only; drives `display`).
    Section,
    /// Training progress fill quad (drives `width`).
    Prog,
}
/// A train button → the soldier it enqueues.
#[derive(Component, Clone, Copy)]
struct TrainBtn(UnitKind);

// ── selection state (local to the HUD) ───────────────────────────────────────────────────────

#[derive(Clone, Copy, PartialEq)]
enum SelMode {
    None,
    Units,
    Building,
}

/// What the selection panel is currently showing. `building` is the single selected structure when
/// `mode == Building` (units take priority in a mixed selection, RTS-convention).
#[derive(Resource)]
struct HudSel {
    mode: SelMode,
    building: Option<Entity>,
}
impl Default for HudSel {
    fn default() -> Self {
        HudSel { mode: SelMode::None, building: None }
    }
}

pub struct RtsHudPlugin;

impl Plugin for RtsHudPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<HudSel>().add_systems(
            Update,
            (
                setup_rts_hud,
                update_top_bar,
                update_build_strip,
                build_strip_click,
                sync_selection_mode,
                update_unit_summary,
                update_building_panel,
                update_train_buttons,
                train_click,
            )
                .run_if(in_skirmish),
        );
    }
}

/// Buildings the player can raise from the strip (everything but the free starter Town Hall).
const PLACEABLE: [BuildingKind; 6] = [
    BuildingKind::House,
    BuildingKind::Sawmill,
    BuildingKind::Quarry,
    BuildingKind::GoldMine,
    BuildingKind::Farm,
    BuildingKind::Barracks,
];

// ── setup (spawn once, after the icon atlas has loaded) ───────────────────────────────────────

/// Spawn the three HUD roots a single time. Runs in `Update` (gated `in_skirmish`; skirmish boots
/// straight into `Playing`) with a `Local` latch, waiting for the Twemoji stat icons the way the
/// campaign `setup_stat_bar` does — so the resource/cost icons resolve instead of rendering blank.
fn setup_rts_hud(
    mut done: Local<bool>,
    atlas: Res<IconAtlas>,
    fonts: Res<UiFonts>,
    mut commands: Commands,
) {
    if *done || atlas.get("stat:gold").is_none() {
        return;
    }
    *done = true;
    spawn_top_bar(&mut commands, &fonts, &atlas);
    spawn_build_strip(&mut commands, &fonts, &atlas);
    spawn_selection_panels(&mut commands, &fonts);
}

/// Top-centre resource bar: `[Potyczka] wood stone gold food  pop`.
fn spawn_top_bar(commands: &mut Commands, fonts: &UiFonts, atlas: &IconAtlas) {
    commands
        // Full-width wrapper so the pill self-centres; passes pointer events through.
        .spawn((
            Node {
                position_type: PositionType::Absolute,
                top: Val::Px(12.0),
                left: Val::Px(0.0),
                width: Val::Percent(100.0),
                flex_direction: FlexDirection::Row,
                justify_content: JustifyContent::Center,
                ..default()
            },
            GlobalZIndex(60),
            bevy::ui::FocusPolicy::Pass,
        ))
        .with_children(|w| {
            w.spawn((
                Node {
                    flex_direction: FlexDirection::Row,
                    align_items: AlignItems::Center,
                    column_gap: Val::Px(18.0),
                    padding: UiRect::axes(Val::Px(16.0), Val::Px(7.0)),
                    border: border(2.0),
                    border_radius: radius(R_BTN),
                    ..default()
                },
                BackgroundColor(PILL_BG),
                BorderColor::all(rgba(224, 168, 74, 0.3)),
                shadow_hud(),
            ))
            .with_children(|row| {
                row.spawn(label(&fonts.display, "Skirmish", 13.0, rgba(224, 168, 74, 0.9)));
                res_cell(row, fonts, atlas, "stat:wood", WOOD_COL, "100", ResKind::Wood);
                res_cell(row, fonts, atlas, "stat:stone", STONE, "60", ResKind::Stone);
                res_cell(row, fonts, atlas, "stat:gold", GOLD, "40", ResKind::Gold);
                res_cell(row, fonts, atlas, "stat:food", FOOD_COL, "60", ResKind::Food);
                res_cell(row, fonts, atlas, "stat:pop", POP_COL, "0/6", ResKind::Pop);
            });
        });
}

/// One `[icon number]` counter cell in the top bar.
fn res_cell(
    p: &mut RelatedSpawnerCommands<ChildOf>,
    fonts: &UiFonts,
    atlas: &IconAtlas,
    icon_id: &str,
    col: Color,
    initial: &str,
    kind: ResKind,
) {
    p.spawn(Node {
        flex_direction: FlexDirection::Row,
        align_items: AlignItems::Center,
        column_gap: Val::Px(5.0),
        ..default()
    })
    .with_children(|c| {
        if let Some(e) = atlas.get_tintable(icon_id) {
            c.spawn(widgets::icon_tinted(e, 17.0, col));
        }
        c.spawn((label(&fonts.bold, initial, 14.0, col), ResText(kind)));
    });
}

/// Bottom-right build strip: a column of one button per placeable building.
fn spawn_build_strip(commands: &mut Commands, fonts: &UiFonts, atlas: &IconAtlas) {
    commands
        .spawn((
            Node {
                position_type: PositionType::Absolute,
                bottom: Val::Px(18.0),
                right: Val::Px(18.0),
                flex_direction: FlexDirection::Column,
                align_items: AlignItems::FlexEnd,
                row_gap: Val::Px(6.0),
                padding: UiRect::all(Val::Px(8.0)),
                border: border(2.0),
                border_radius: radius(R_PANEL),
                ..default()
            },
            widgets::card_paint(),
            GlobalZIndex(60),
            bevy::ui::FocusPolicy::Pass,
        ))
        .with_children(|col| {
            col.spawn(label(&fonts.display, "Build", 12.0, GOLD));
            for kind in PLACEABLE {
                let def = building_def(kind);
                col.spawn((
                    Button,
                    Interaction::default(),
                    Node {
                        width: Val::Px(196.0),
                        flex_direction: FlexDirection::Row,
                        align_items: AlignItems::Center,
                        justify_content: JustifyContent::SpaceBetween,
                        column_gap: Val::Px(8.0),
                        padding: UiRect::axes(Val::Px(9.0), Val::Px(6.0)),
                        border: border(2.0),
                        border_radius: radius(R_CARD),
                        ..default()
                    },
                    BackgroundColor(BTN_BG),
                    BorderColor::all(BORDER_SOFT),
                    BuildStripBtn(kind),
                ))
                .with_children(|b| {
                    b.spawn(label(&fonts.semibold, def.name, 14.0, Color::WHITE));
                    b.spawn(Node {
                        flex_direction: FlexDirection::Row,
                        align_items: AlignItems::Center,
                        column_gap: Val::Px(6.0),
                        ..default()
                    })
                    .with_children(|cr| spawn_cost_chips(cr, fonts, Some(atlas), &def.cost));
                });
            }
        });
}

/// Bottom-left selection panels (both start hidden; `sync_selection_mode` toggles `display`).
fn spawn_selection_panels(commands: &mut Commands, fonts: &UiFonts) {
    // ── unit summary (rebuilt each frame) ──
    commands.spawn((
        Node {
            position_type: PositionType::Absolute,
            bottom: Val::Px(18.0),
            left: Val::Px(18.0),
            display: Display::None,
            flex_direction: FlexDirection::Row,
            align_items: AlignItems::Center,
            column_gap: Val::Px(14.0),
            padding: UiRect::axes(Val::Px(14.0), Val::Px(9.0)),
            border: border(2.0),
            border_radius: radius(R_PANEL),
            ..default()
        },
        widgets::card_paint(),
        GlobalZIndex(60),
        bevy::ui::FocusPolicy::Pass,
        SelUnitPanel,
    ));

    // ── single-building panel (persistent nodes, driven live) ──
    commands
        .spawn((
            Node {
                position_type: PositionType::Absolute,
                bottom: Val::Px(18.0),
                left: Val::Px(18.0),
                display: Display::None,
                width: Val::Px(232.0),
                flex_direction: FlexDirection::Column,
                row_gap: Val::Px(7.0),
                padding: UiRect::axes(Val::Px(14.0), Val::Px(11.0)),
                border: border(2.0),
                border_radius: radius(R_PANEL),
                ..default()
            },
            widgets::card_paint(),
            GlobalZIndex(60),
            SelBldgPanel,
        ))
        .with_children(|p| {
            p.spawn((label(&fonts.display, "", 16.0, GOLD), BldgText::Name));
            // HP bar (track + red fill + numeric).
            bar_track(p, 14.0, |t| {
                t.spawn((bar_fill(HP_TOP, HP_BOT), BldgBar::HpFill));
                t.spawn((
                    label(&fonts.bold, "", 10.0, Color::WHITE),
                    TextShadow { offset: Vec2::ZERO, color: rgba(0, 0, 0, 0.8) },
                    BldgText::Hp,
                ));
            });
            // Train section (barracks-only).
            p.spawn((
                Node {
                    display: Display::None,
                    flex_direction: FlexDirection::Column,
                    row_gap: Val::Px(7.0),
                    margin: UiRect::top(Val::Px(3.0)),
                    ..default()
                },
                BldgBar::Section,
            ))
            .with_children(|s| {
                // Two train buttons.
                s.spawn(Node {
                    flex_direction: FlexDirection::Row,
                    column_gap: Val::Px(7.0),
                    ..default()
                })
                .with_children(|r| {
                    train_button(r, fonts, UnitKind::Swordsman, "Swordsman");
                    train_button(r, fonts, UnitKind::Archer, "Archer");
                });
                // Queue slots.
                s.spawn(Node {
                    flex_direction: FlexDirection::Row,
                    column_gap: Val::Px(5.0),
                    ..default()
                })
                .with_children(|q| {
                    for i in 0..TRAIN_QUEUE_DEPTH {
                        q.spawn((
                            Node {
                                width: Val::Px(24.0),
                                height: Val::Px(24.0),
                                align_items: AlignItems::Center,
                                justify_content: JustifyContent::Center,
                                border: border(1.0),
                                border_radius: radius(R_SLOT),
                                ..default()
                            },
                            BackgroundColor(SLOT_BG),
                            BorderColor::all(SLOT_BORDER),
                        ))
                        .with_children(|cell| {
                            cell.spawn((label(&fonts.bold, "", 12.0, GOLD), BldgText::Queue(i)));
                        });
                    }
                });
                // Training progress bar.
                bar_track(s, 8.0, |t| {
                    t.spawn((bar_fill(GOLD, GOLD_DEEP), BldgBar::Prog));
                });
            });
        });
}

/// A single train button (label + cost chips), carrying its [`TrainBtn`] kind.
fn train_button(
    p: &mut RelatedSpawnerCommands<ChildOf>,
    fonts: &UiFonts,
    kind: UnitKind,
    name: &str,
) {
    p.spawn((
        Button,
        Interaction::default(),
        Node {
            flex_direction: FlexDirection::Column,
            align_items: AlignItems::Center,
            row_gap: Val::Px(3.0),
            padding: UiRect::axes(Val::Px(9.0), Val::Px(6.0)),
            border: border(2.0),
            border_radius: radius(R_CARD),
            ..default()
        },
        BackgroundColor(BTN_BG),
        BorderColor::all(BORDER_SOFT),
        TrainBtn(kind),
    ))
    .with_children(|b| {
        b.spawn(label(&fonts.semibold, name, 13.0, Color::WHITE));
        b.spawn(Node {
            flex_direction: FlexDirection::Row,
            align_items: AlignItems::Center,
            column_gap: Val::Px(5.0),
            ..default()
        })
        // No atlas at this call site — cost integers still read fine without their small coin icons.
        .with_children(|cr| spawn_cost_chips(cr, fonts, None, &train_cost(kind)));
    });
}

/// Spawn `[icon N]` cost chips for every non-zero component of `cost` (icons only when `atlas`).
fn spawn_cost_chips(
    p: &mut RelatedSpawnerCommands<ChildOf>,
    fonts: &UiFonts,
    atlas: Option<&IconAtlas>,
    cost: &Cost,
) {
    for (id, col, amt) in [
        ("stat:wood", WOOD_COL, cost.wood),
        ("stat:stone", STONE, cost.stone),
        ("stat:gold", GOLD, cost.gold),
        ("stat:food", FOOD_COL, cost.food),
    ] {
        if amt <= 0.0 {
            continue;
        }
        let entry = atlas.and_then(|a| a.get_tintable(id));
        widgets::cost_chip(p, &fonts.semibold, entry, format!("{}", amt as i64), col, rgba(0, 0, 0, 0.0));
    }
}

/// A rounded bar-track node of the given height, filled by `f`.
fn bar_track(
    p: &mut RelatedSpawnerCommands<ChildOf>,
    h: f32,
    f: impl FnOnce(&mut RelatedSpawnerCommands<ChildOf>),
) {
    p.spawn((
        Node {
            width: Val::Percent(100.0),
            height: Val::Px(h),
            border: border(1.0),
            border_radius: radius(R_CELL),
            align_items: AlignItems::Center,
            justify_content: JustifyContent::Center,
            overflow: Overflow::clip(),
            ..default()
        },
        BackgroundColor(PANEL_HUD),
        BorderColor::all(BORDER_SOFT),
    ))
    .with_children(f);
}

/// A full-height absolute gradient fill quad (width driven live by the update systems).
fn bar_fill(top: Color, bot: Color) -> impl Bundle {
    (
        Node {
            position_type: PositionType::Absolute,
            left: Val::Px(0.0),
            top: Val::Px(0.0),
            width: Val::Percent(100.0),
            height: Val::Percent(100.0),
            ..default()
        },
        widgets::vgrad(top, bot),
    )
}

// ── top bar sync ──────────────────────────────────────────────────────────────────────────────

fn update_top_bar(banks: Res<RtsBanks>, pop: Res<RtsPop>, mut q: Query<(&ResText, &mut Text)>) {
    let b = *banks.side(Side::Player);
    let ps = pop.0[Side::Player.ix()];
    for (r, mut t) in &mut q {
        let s = match r.0 {
            ResKind::Wood => format!("{}", b.wood as i64),
            ResKind::Stone => format!("{}", b.stone as i64),
            ResKind::Gold => format!("{}", b.gold as i64),
            ResKind::Food => format!("{}", b.food as i64),
            ResKind::Pop => format!("{}/{}", ps.count, ps.cap),
        };
        if **t != s {
            **t = s; // change-detection guard: only re-layout when the numeral actually moved
        }
    }
}

// ── build strip ───────────────────────────────────────────────────────────────────────────────

/// Highlight the active `Placing` button, and grey (dark wash + faded contents) any building the
/// player can't afford — re-checked live against the bank so it un-greys the instant a haul lands.
fn update_build_strip(
    placing: Res<Placing>,
    banks: Res<RtsBanks>,
    mut rows: Query<(Entity, &BuildStripBtn, &mut BorderColor, &mut BackgroundColor)>,
    kids: Query<&Children>,
    mut texts: Query<&mut TextColor>,
    mut imgs: Query<&mut ImageNode>,
) {
    let bank = banks.side(Side::Player);
    for (e, btn, mut bc, mut bg) in &mut rows {
        let afford = bank.can_afford(&building_def(btn.0).cost);
        let active = placing.0 == Some(btn.0);
        *bc = BorderColor::all(if active {
            GOLD
        } else if afford {
            BORDER_SOFT
        } else {
            BORDER_SOFT.with_alpha(0.06)
        });
        bg.0 = if active {
            BTN_BG_HOVER
        } else if afford {
            BTN_BG
        } else {
            DIM_BG
        };
        set_subtree_alpha(e, if afford { 1.0 } else { DIM_ALPHA }, &kids, &mut texts, &mut imgs);
    }
}

/// Toggle `Placing` on click (again on the active building cancels); an unaffordable click toasts.
fn build_strip_click(
    time: Res<Time>,
    banks: Res<RtsBanks>,
    mut placing: ResMut<Placing>,
    mut notice: ResMut<Notice>,
    q: Query<(&Interaction, &BuildStripBtn), Changed<Interaction>>,
) {
    for (interaction, btn) in &q {
        if *interaction != Interaction::Pressed {
            continue;
        }
        if placing.0 == Some(btn.0) {
            placing.0 = None; // clicking the armed building again disarms
        } else if banks.side(Side::Player).can_afford(&building_def(btn.0).cost) {
            placing.0 = Some(btn.0);
            // Hint the controls — the ghost follows the cursor; the player picks the spot.
            notice.push("Move to a spot · click to place · R rotate · right-click cancel", time.elapsed_secs_f64());
        } else {
            notice.push("Not enough resources", time.elapsed_secs_f64());
        }
    }
}

// ── selection panel ───────────────────────────────────────────────────────────────────────────

/// Decide what the selection panel shows this frame and toggle the two panels' visibility. Units
/// win over buildings in a mixed selection; a lone selected building drives the building panel.
#[allow(clippy::type_complexity)]
fn sync_selection_mode(
    mut sel: ResMut<HudSel>,
    units: Query<(), (With<RtsUnit>, With<Selected>)>,
    bldgs: Query<Entity, (With<RtsBuilding>, With<Selected>)>,
    mut unit_panel: Query<&mut Node, (With<SelUnitPanel>, Without<SelBldgPanel>)>,
    mut bldg_panel: Query<&mut Node, (With<SelBldgPanel>, Without<SelUnitPanel>)>,
) {
    let n_units = units.iter().count();
    let mut b_iter = bldgs.iter();
    let first_b = b_iter.next();
    let one_building = first_b.is_some() && b_iter.next().is_none();

    let mode = if n_units > 0 {
        SelMode::Units
    } else if one_building {
        SelMode::Building
    } else {
        SelMode::None
    };
    sel.mode = mode;
    sel.building = if matches!(mode, SelMode::Building) { first_b } else { None };

    if let Ok(mut n) = unit_panel.single_mut() {
        n.display = if matches!(mode, SelMode::Units) { Display::Flex } else { Display::None };
    }
    if let Ok(mut n) = bldg_panel.single_mut() {
        n.display = if matches!(mode, SelMode::Building) { Display::Flex } else { Display::None };
    }
}

/// Rebuild the per-kind unit count chips when units are selected (no buttons here, so a full
/// rebuild each frame is safe — mirrors the campaign buff-pip pattern).
fn update_unit_summary(
    sel: Res<HudSel>,
    atlas: Res<IconAtlas>,
    fonts: Res<UiFonts>,
    mut commands: Commands,
    units: Query<&RtsUnit, With<Selected>>,
    root_q: Query<Entity, With<SelUnitPanel>>,
    rows_q: Query<Entity, With<SelUnitRow>>,
) {
    if sel.mode != SelMode::Units {
        return;
    }
    for e in &rows_q {
        commands.entity(e).try_despawn();
    }
    // Count by kind in a fixed display order (Worker / Swordsman / Archer).
    let mut counts = [0u32; 3];
    for u in &units {
        counts[unit_ix(u.kind)] += 1;
    }
    let Ok(root) = root_q.single() else { return };
    commands.entity(root).with_children(|p| {
        // Icons: axe for workers, ⚔️ for swordsmen (full-colour Twemoji), 🏹 for archers.
        for (ix, (icon_id, tint)) in
            [("stat:wood", WOOD_COL), ("buff:power", Color::WHITE), ("def_keep_archers", GOLD)]
                .into_iter()
                .enumerate()
        {
            if counts[ix] == 0 {
                continue;
            }
            p.spawn((
                SelUnitRow,
                Node {
                    flex_direction: FlexDirection::Row,
                    align_items: AlignItems::Center,
                    column_gap: Val::Px(6.0),
                    ..default()
                },
            ))
            .with_children(|c| {
                if let Some(e) = atlas.get_tintable(icon_id) {
                    c.spawn(widgets::icon_tinted(e, 24.0, tint));
                }
                c.spawn(label(&fonts.bold, format!("{}", counts[ix]), 16.0, Color::WHITE));
            });
        }
    });
}

fn unit_ix(kind: UnitKind) -> usize {
    match kind {
        UnitKind::Worker => 0,
        UnitKind::Swordsman => 1,
        UnitKind::Archer => 2,
    }
}

/// Drive the single-building panel: name, HP bar, and (for a barracks) the train section's queue
/// slots + progress bar. The train BUTTONS' grey-state lives in `update_train_buttons`.
#[allow(clippy::type_complexity)]
fn update_building_panel(
    sel: Res<HudSel>,
    bldgs: Query<(&RtsBuilding, Option<&Health>, Option<&TrainQueue>)>,
    mut texts: Query<(&BldgText, &mut Text)>,
    mut bars: Query<(&BldgBar, &mut Node)>,
) {
    if sel.mode != SelMode::Building {
        return;
    }
    let Some(b) = sel.building else { return };
    let Ok((rb, health, tq)) = bldgs.get(b) else { return };
    let def = building_def(rb.kind);

    let (hp, max) = match health {
        Some(h) => (h.hp.max(0.0), h.max.max(1.0)),
        None => (def.hp, def.hp), // no live Health component → read as full
    };
    let hp_pct = (hp / max * 100.0).clamp(0.0, 100.0);
    let is_barracks = tq.is_some();
    let prog_pct = tq.map(|q| (q.progress / TRAIN_SECS).clamp(0.0, 1.0) * 100.0).unwrap_or(0.0);

    for (bt, mut text) in &mut texts {
        let s = match *bt {
            BldgText::Name => def.name.to_string(),
            BldgText::Hp => format!("{} / {}", hp as i64, max as i64),
            BldgText::Queue(i) => tq
                .and_then(|q| q.queue.get(i))
                .map(|k| unit_glyph(*k).to_string())
                .unwrap_or_default(),
        };
        if **text != s {
            **text = s;
        }
    }
    for (bar, mut node) in &mut bars {
        match *bar {
            BldgBar::HpFill => node.width = Val::Percent(hp_pct),
            BldgBar::Prog => node.width = Val::Percent(prog_pct),
            BldgBar::Section => {
                node.display = if is_barracks { Display::Flex } else { Display::None }
            }
        }
    }
}

/// Short glyph for a queued soldier kind (Worker / Swordsman / Archer).
fn unit_glyph(kind: UnitKind) -> &'static str {
    match kind {
        UnitKind::Worker => "W",
        UnitKind::Swordsman => "S",
        UnitKind::Archer => "A",
    }
}

/// Grey the train buttons when the selected barracks' queue is full or the player can't afford the
/// unit (dark wash + faded contents), so "can't train" reads before the click.
fn update_train_buttons(
    sel: Res<HudSel>,
    banks: Res<RtsBanks>,
    queues: Query<&TrainQueue>,
    mut buttons: Query<(Entity, &TrainBtn, &mut BackgroundColor)>,
    kids: Query<&Children>,
    mut texts: Query<&mut TextColor>,
    mut imgs: Query<&mut ImageNode>,
) {
    let bank = banks.side(Side::Player);
    let full = sel
        .building
        .and_then(|b| queues.get(b).ok())
        .map(|q| q.queue.len() >= TRAIN_QUEUE_DEPTH)
        .unwrap_or(false);
    for (e, tb, mut bg) in &mut buttons {
        let ok = !full && bank.can_afford(&train_cost(tb.0));
        bg.0 = if ok { BTN_BG } else { DIM_BG };
        set_subtree_alpha(e, if ok { 1.0 } else { DIM_ALPHA }, &kids, &mut texts, &mut imgs);
    }
}

/// Enqueue a soldier on click. The HUD only *validates for feedback* + emits [`TrainOrder`]; the
/// units sibling is the authority that spends the bank and consumes an idle worker.
#[allow(clippy::too_many_arguments)]
fn train_click(
    time: Res<Time>,
    sel: Res<HudSel>,
    banks: Res<RtsBanks>,
    pop: Res<RtsPop>,
    queues: Query<&TrainQueue>,
    mut orders: MessageWriter<TrainOrder>,
    mut notice: ResMut<Notice>,
    q: Query<(&Interaction, &TrainBtn), Changed<Interaction>>,
) {
    let Some(b) = sel.building else { return };
    for (interaction, tb) in &q {
        if *interaction != Interaction::Pressed {
            continue;
        }
        // Queue full → the button is greyed; swallow the click silently.
        if queues.get(b).map(|qq| qq.queue.len() >= TRAIN_QUEUE_DEPTH).unwrap_or(false) {
            continue;
        }
        let now = time.elapsed_secs_f64();
        if !banks.side(Side::Player).can_afford(&train_cost(tb.0)) {
            notice.push("Not enough resources", now);
            continue;
        }
        let ps = pop.0[Side::Player.ix()];
        if ps.count >= ps.cap {
            notice.push("Population limit", now);
            continue;
        }
        orders.write(TrainOrder { building: b, kind: tb.0 });
    }
}

/// Walk `root`'s descendant tree and set the alpha of every `TextColor`/`ImageNode` — the greyed-out
/// look for a disabled button (absolute alpha, so it restores cleanly). Ported from the campaign
/// `build_strip_update`.
fn set_subtree_alpha(
    root: Entity,
    a: f32,
    kids: &Query<&Children>,
    texts: &mut Query<&mut TextColor>,
    imgs: &mut Query<&mut ImageNode>,
) {
    let mut stack = vec![root];
    while let Some(e) = stack.pop() {
        if let Ok(mut t) = texts.get_mut(e) {
            t.0.set_alpha(a);
        }
        if let Ok(mut img) = imgs.get_mut(e) {
            img.color.set_alpha(a);
        }
        if let Ok(c) = kids.get(e) {
            stack.extend(c.iter());
        }
    }
}
