//! **The War Table — upgrade-tree panel** (`Modal::UpgradeTree`). A real tree *graph* on a framed
//! parchment sheet: per-branch tier rows computed from `prereq_id` depth, 46px icon medallions
//! joined by ink prereq lines (faded until the parent is owned), and a fixed detail strip that
//! shows the hovered/focused node's name/desc/cost — so the board itself stays compact enough to
//! fit 1280×720. Purchase logic stays in `economy.rs` (`try_purchase`); this module is only the
//! panel. State language (shared with the shop): owned = branch-colour fill · buyable = bright
//! vellum + gold ring · too-poor = red cost · locked = faded + padlock badge.

use bevy::ecs::relationship::RelatedSpawnerCommands;
use bevy::prelude::*;
use bevy::ui::widget::NodeImageMode;
use tileworld_core::upgrade_store::{node_by_id, UpgradeBranch, UpgradeNode, UPGRADE_NODES};

use crate::economy::{try_purchase, Bank, Defenses, EconomyState, Upgrades};
use crate::game_state::{AppState, Modal};
use crate::player::PlayerRes;
use crate::siege::KeepHp;
use crate::ui::anim::{anim, AnimKind};
use crate::ui::focus::{FocusActivate, Focusable, UiFocus};
use crate::ui::fonts::{label, UiFonts};
use crate::ui::texture::UiTextures;
use crate::ui::theme::*;
use crate::ui::widgets::{self, border};
use crate::ui::IconAtlas;

pub struct TreeUiPlugin;

impl Plugin for TreeUiPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Update, open_tree.run_if(in_state(Modal::None)))
            .add_systems(OnEnter(Modal::UpgradeTree), spawn_tree)
            .add_systems(OnExit(Modal::UpgradeTree), despawn_tree)
            .add_systems(
                Update,
                (tree_interact, tree_paint, tree_detail, tree_close)
                    .run_if(in_state(Modal::UpgradeTree)),
            );
        // Temp diagnostics for the hover-dead-on-medallions bug (FOREST_UILOG=1).
        if std::env::var("FOREST_UILOG").is_ok() {
            app.add_systems(Update, debug_uilog.run_if(in_state(Modal::UpgradeTree)));
        }
    }
}

/// `FOREST_UILOG=1`: dump cursor position, any non-None medallion interactions, the focus
/// target, and one medallion's resolved geometry — to find why hover dies on the tree.
fn debug_uilog(
    windows: Query<&Window>,
    btns: Query<(Entity, &Interaction, &TreeNodeButton, &ComputedNode, &bevy::ui::UiGlobalTransform)>,
    focus: Res<crate::ui::focus::UiFocus>,
    mut every: Local<u32>,
) {
    *every += 1;
    if *every % 15 != 0 {
        return;
    }
    let cur = windows.single().ok().and_then(|w| w.cursor_position());
    let hot: Vec<&str> =
        btns.iter().filter(|(_, i, ..)| **i != Interaction::None).map(|(_, _, b, ..)| b.0).collect();
    let probe = btns
        .iter()
        .find(|(_, _, b, ..)| b.0 == "def_walls")
        .map(|(_, _, _, cn, gt)| (gt.translation, cn.size(), cn.inverse_scale_factor()));
    info!("UILOG cursor={cur:?} hot={hot:?} focus={:?} walls={probe:?}", focus.current);
}

// The tree opens via the contextual **E** near the keep (see `interaction.rs`); this system only
// keeps the `FOREST_PANEL=tree` screenshot hook alive.
fn open_tree(
    app: Res<State<AppState>>,
    mut next: ResMut<NextState<Modal>>,
    mut auto_done: Local<bool>,
) {
    let force = !*auto_done && std::env::var("FOREST_PANEL").ok().as_deref() == Some("tree");
    if force {
        *auto_done = true;
    }
    if *app.get() == AppState::Playing && force {
        next.set(Modal::UpgradeTree);
    }
}

#[derive(Component)]
struct TreeUi;
/// The clickable medallion (the `Button` + `Focusable`).
#[derive(Component)]
struct TreeNodeButton(&'static str);
/// The medallion's icon image (tint flips light on owned).
#[derive(Component)]
struct NodeIcon(&'static str);
/// The padlock badge (visible while prereq unmet).
#[derive(Component)]
struct LockBadge(&'static str);
/// The cost text under a medallion.
#[derive(Component)]
struct CostLabel(&'static str);
/// One segment of a prereq line; solid once the named child's parent is owned.
#[derive(Component)]
struct EdgeOf(&'static str);
#[derive(Component)]
struct TreeCloseBtn;
#[derive(Component)]
struct HeaderGold;
#[derive(Component)]
struct HeaderStone;
#[derive(Component)]
struct DetailName;
#[derive(Component)]
struct DetailDesc;
#[derive(Component)]
struct DetailMeta;

fn branch_title(b: UpgradeBranch) -> &'static str {
    match b {
        UpgradeBranch::Economy => "Prosperity",
        UpgradeBranch::Defense => "Bulwark",
        UpgradeBranch::Hero => "Champion",
        UpgradeBranch::Arsenal => "Armoury",
    }
}

fn branch_sigil(b: UpgradeBranch) -> &'static str {
    match b {
        UpgradeBranch::Economy => "branch:economy",
        UpgradeBranch::Defense => "branch:defense",
        UpgradeBranch::Hero => "branch:hero",
        UpgradeBranch::Arsenal => "branch:arsenal",
    }
}

fn branch_color(b: UpgradeBranch) -> Color {
    match b {
        UpgradeBranch::Economy => BRANCH_ECON,
        UpgradeBranch::Defense => BRANCH_DEF,
        UpgradeBranch::Hero => BRANCH_HERO,
        UpgradeBranch::Arsenal => BRANCH_ARSENAL,
    }
}

// ── Graph geometry ──────────────────────────────────────────────────────────────────────
const PITCH_X: f32 = 60.0; // column pitch
const PITCH_Y: f32 = 100.0; // tier-row pitch (room for the name + cost block under each node)
const NODE: f32 = 46.0; // medallion diameter
const LABEL_H: f32 = 38.0; // name (≤2 wrapped lines) + cost under a medallion

struct Laid {
    node: &'static UpgradeNode,
    col: f32,
    depth: usize,
    parent: Option<(f32, usize)>, // parent's (col, depth) for the prereq line
}

/// Tidy-tree layout for one branch: leaves claim columns left→right (DFS in table order),
/// parents centre over their children. Depth = prereq-chain length. Returns nodes + extents.
fn layout_branch(branch: UpgradeBranch) -> (Vec<Laid>, f32, usize) {
    let in_branch: Vec<&'static UpgradeNode> =
        UPGRADE_NODES.iter().filter(|n| n.branch == branch).collect();
    let children =
        |id: &str| in_branch.iter().copied().filter(move |n| n.prereq_id == Some(id)).collect::<Vec<_>>();

    fn place(
        node: &'static UpgradeNode,
        depth: usize,
        parent: Option<(f32, usize)>,
        next_col: &mut f32,
        out: &mut Vec<Laid>,
        children: &dyn Fn(&str) -> Vec<&'static UpgradeNode>,
    ) -> f32 {
        let kids = children(node.id);
        let col = if kids.is_empty() {
            let c = *next_col;
            *next_col += 1.0;
            c
        } else {
            // Reserve our slot index BEFORE descending? No — classic tidy layout: centre over kids.
            let mut lo = f32::MAX;
            let mut hi = f32::MIN;
            // placeholder; children fill in below
            let mut cols = Vec::new();
            for kid in &kids {
                // parent pos isn't known yet; patch after
                cols.push(place(kid, depth + 1, None, next_col, out, children));
            }
            for c in &cols {
                lo = lo.min(*c);
                hi = hi.max(*c);
            }
            let me = (lo + hi) / 2.0;
            // Patch the kids' parent reference now that our column is known.
            for l in out.iter_mut() {
                if l.node.prereq_id == Some(node.id) {
                    l.parent = Some((me, depth));
                }
            }
            me
        };
        out.push(Laid { node, col, depth, parent });
        col
    }

    let mut out = Vec::new();
    let mut next_col = 0.0;
    for root in in_branch.iter().filter(|n| n.prereq_id.is_none()) {
        place(root, 0, None, &mut next_col, &mut out, &children);
    }
    let rows = out.iter().map(|l| l.depth).max().unwrap_or(0) + 1;
    (out, next_col.max(1.0), rows)
}

fn cost_text(node: &UpgradeNode) -> String {
    if node.stone_cost > 0 {
        format!("{}g + {}s", node.cost(), node.stone_cost)
    } else {
        format!("{}g", node.cost())
    }
}

// ── Spawn ───────────────────────────────────────────────────────────────────────────────

fn spawn_tree(
    mut commands: Commands,
    fonts: Res<UiFonts>,
    atlas: Res<IconAtlas>,
    tex: Res<UiTextures>,
) {
    let ink_faint = rgba(36, 27, 12, 0.55);
    commands
        .spawn((widgets::scrim(60), TreeUi))
        .with_children(|root| {
            root.spawn((
                Node {
                    flex_direction: FlexDirection::Column,
                    row_gap: Val::Px(12.0),
                    padding: UiRect::axes(Val::Px(26.0), Val::Px(18.0)),
                    border: border(2.0),
                    border_radius: radius(R_PANEL),
                    max_width: Val::Px(1190.0),
                    ..default()
                },
                BackgroundColor(PARCHMENT),
                BorderColor::all(IRON_EDGE),
                shadow_card(),
                anim(AnimKind::PopIn, 0.0, 0.22),
            ))
            .with_children(|sheet| {
                parchment_layers(sheet, tex.parchment.clone());

                // Header: title block + treasury chips.
                sheet
                    .spawn(Node {
                        flex_direction: FlexDirection::Row,
                        justify_content: JustifyContent::SpaceBetween,
                        align_items: AlignItems::Center,
                        width: Val::Percent(100.0),
                        ..default()
                    })
                    .with_children(|head| {
                        head.spawn(Node { flex_direction: FlexDirection::Column, ..default() })
                            .with_children(|t| {
                                t.spawn(label(&fonts.display, "WAR TABLE", 12.0, rgb(138, 106, 46)));
                                t.spawn(label(&fonts.display, "Expand the Keep", 27.0, INK));
                            });
                        head.spawn(Node {
                            flex_direction: FlexDirection::Row,
                            align_items: AlignItems::Center,
                            column_gap: Val::Px(8.0),
                            ..default()
                        })
                        .with_children(|chips| {
                            treasury_chip(chips, &fonts, &atlas, "sym:gold", HeaderGold);
                            treasury_chip(chips, &fonts, &atlas, "sym:stone", HeaderStone);
                            widgets::close_button(chips, &fonts.bold, TreeCloseBtn, true);
                        });
                    });

                // The four branch graphs.
                sheet
                    .spawn(Node {
                        flex_direction: FlexDirection::Row,
                        column_gap: Val::Px(22.0),
                        align_items: AlignItems::FlexStart,
                        justify_content: JustifyContent::Center,
                        ..default()
                    })
                    .with_children(|row| {
                        for branch in [
                            UpgradeBranch::Economy,
                            UpgradeBranch::Defense,
                            UpgradeBranch::Hero,
                            UpgradeBranch::Arsenal,
                        ] {
                            spawn_branch(row, branch, &fonts, &atlas, ink_faint);
                        }
                    });

                // Detail strip — fixed height so the board never reflows.
                sheet
                    .spawn((
                        Node {
                            flex_direction: FlexDirection::Column,
                            row_gap: Val::Px(3.0),
                            width: Val::Percent(100.0),
                            height: Val::Px(78.0),
                            padding: UiRect::axes(Val::Px(14.0), Val::Px(9.0)),
                            border: UiRect::top(Val::Px(1.0)),
                            border_radius: radius(R_CELL),
                            ..default()
                        },
                        BackgroundColor(rgba(86, 58, 24, 0.08)),
                        BorderColor::all(rgba(86, 58, 24, 0.35)),
                    ))
                    .with_children(|d| {
                        d.spawn((label(&fonts.serif, "The War Table", 18.0, INK), DetailName));
                        d.spawn((
                            label(
                                &fonts.regular,
                                "Hover or arrow onto an upgrade to read its charter.",
                                12.5,
                                INK_SOFT,
                            ),
                            DetailDesc,
                        ));
                        d.spawn((label(&fonts.bold, "", 12.5, rgb(154, 110, 22)), DetailMeta));
                    });

                sheet.spawn((
                    Node { align_self: AlignSelf::Center, ..default() },
                    children![label(
                        &fonts.serif,
                        "Arrows move · Enter or E to purchase · U / Esc closes the plans",
                        12.0,
                        rgb(138, 106, 46),
                    )],
                ));
            });
        });
}

/// Parchment-sheet chrome: tiled grain + inset ink hairline + ink corner notches
/// (the dark-panel `chrome_layers` in gold doesn't read on vellum).
fn parchment_layers(p: &mut RelatedSpawnerCommands<ChildOf>, grain: Handle<Image>) {
    p.spawn((
        Node {
            position_type: PositionType::Absolute,
            top: Val::Px(0.0),
            left: Val::Px(0.0),
            right: Val::Px(0.0),
            bottom: Val::Px(0.0),
            ..default()
        },
        ImageNode::new(grain).with_mode(NodeImageMode::Tiled {
            tile_x: true,
            tile_y: true,
            stretch_value: 1.0,
        }),
    ));
    p.spawn((
        Node {
            position_type: PositionType::Absolute,
            top: Val::Px(4.0),
            left: Val::Px(4.0),
            right: Val::Px(4.0),
            bottom: Val::Px(4.0),
            border: border(1.0),
            border_radius: radius(R_PANEL - 4.0),
            ..default()
        },
        BorderColor::all(rgba(86, 58, 24, 0.4)),
    ));
    for (t, l) in [(true, true), (true, false), (false, true), (false, false)] {
        let mut node = Node {
            position_type: PositionType::Absolute,
            width: Val::Px(6.0),
            height: Val::Px(6.0),
            ..default()
        };
        if t { node.top = Val::Px(2.0) } else { node.bottom = Val::Px(2.0) }
        if l { node.left = Val::Px(2.0) } else { node.right = Val::Px(2.0) }
        p.spawn((node, BackgroundColor(rgba(86, 58, 24, 0.55))));
    }
}

fn treasury_chip(
    p: &mut RelatedSpawnerCommands<ChildOf>,
    fonts: &UiFonts,
    atlas: &IconAtlas,
    icon_id: &str,
    marker: impl Component,
) {
    p.spawn((
        Node {
            flex_direction: FlexDirection::Row,
            align_items: AlignItems::Center,
            column_gap: Val::Px(6.0),
            padding: UiRect::axes(Val::Px(12.0), Val::Px(5.0)),
            border: border(1.0),
            border_radius: radius(R_CELL),
            ..default()
        },
        BackgroundColor(rgba(255, 246, 218, 0.65)),
        BorderColor::all(rgba(86, 58, 24, 0.3)),
    ))
    .with_children(|c| {
        if let Some(e) = atlas.get_tintable(icon_id) {
            c.spawn(widgets::icon_tinted(e, 14.0, rgb(122, 90, 28)));
        }
        c.spawn((label(&fonts.bold, "0", 14.0, rgb(58, 42, 14)), marker));
    });
}

fn spawn_branch(
    row: &mut RelatedSpawnerCommands<ChildOf>,
    branch: UpgradeBranch,
    fonts: &UiFonts,
    atlas: &IconAtlas,
    ink_faint: Color,
) {
    let (laid, cols, rows) = layout_branch(branch);
    let w = cols * PITCH_X;
    let h = (rows - 1) as f32 * PITCH_Y + NODE + 18.0;

    row.spawn(Node {
        flex_direction: FlexDirection::Column,
        row_gap: Val::Px(10.0),
        align_items: AlignItems::Center,
        ..default()
    })
    .with_children(|col| {
        // Heraldic banner heading.
        col.spawn((
            Node {
                flex_direction: FlexDirection::Row,
                align_items: AlignItems::Center,
                justify_content: JustifyContent::Center,
                column_gap: Val::Px(8.0),
                width: Val::Percent(100.0),
                padding: UiRect::axes(Val::Px(12.0), Val::Px(7.0)),
                border: UiRect::bottom(Val::Px(2.0)),
                border_radius: radius(R_CELL),
                ..default()
            },
            BackgroundColor(branch_color(branch)),
            BorderColor::all(rgba(0, 0, 0, 0.25)),
        ))
        .with_children(|banner| {
            if let Some(e) = atlas.get_tintable(branch_sigil(branch)) {
                banner.spawn(widgets::icon_tinted(e, 15.0, rgb(253, 243, 216)));
            }
            banner.spawn(label(&fonts.display, branch_title(branch), 13.0, rgb(253, 243, 216)));
        });

        // The graph canvas (absolute-positioned nodes + edges).
        col.spawn(Node {
            width: Val::Px(w),
            height: Val::Px(h),
            ..default()
        })
        .with_children(|canvas| {
            // Edges first (under the medallions); they start below the parent's label block.
            for l in laid.iter() {
                let Some((pcol, pdepth)) = l.parent else { continue };
                let (px, py) =
                    (pcol * PITCH_X + PITCH_X / 2.0, pdepth as f32 * PITCH_Y + NODE + LABEL_H);
                let (cx, cy) = (l.col * PITCH_X + PITCH_X / 2.0, l.depth as f32 * PITCH_Y);
                let midy = (py + cy) / 2.0;
                let seg = |x: f32, y: f32, w: f32, h: f32| {
                    (
                        Node {
                            position_type: PositionType::Absolute,
                            left: Val::Px(x),
                            top: Val::Px(y),
                            width: Val::Px(w),
                            height: Val::Px(h),
                            ..default()
                        },
                        BackgroundColor(ink_faint),
                        EdgeOf(l.node.id),
                    )
                };
                canvas.spawn(seg(px - 1.0, py, 2.0, midy - py));
                let (lo, hi) = (px.min(cx), px.max(cx));
                canvas.spawn(seg(lo - 1.0, midy - 1.0, hi - lo + 2.0, 2.0));
                canvas.spawn(seg(cx - 1.0, midy, 2.0, cy - midy));
            }
            // Medallions + cost labels.
            for l in laid.iter() {
                let x = l.col * PITCH_X + (PITCH_X - NODE) / 2.0;
                let y = l.depth as f32 * PITCH_Y;
                canvas
                    .spawn((
                        Button,
                        Interaction::default(),
                        Focusable,
                        Node {
                            position_type: PositionType::Absolute,
                            left: Val::Px(x),
                            top: Val::Px(y),
                            width: Val::Px(NODE),
                            height: Val::Px(NODE),
                            align_items: AlignItems::Center,
                            justify_content: JustifyContent::Center,
                            border: border(1.5),
                            border_radius: BorderRadius::all(Val::Percent(50.0)),
                            ..default()
                        },
                        BackgroundColor(rgba(255, 251, 238, 0.6)),
                        BorderColor::all(rgba(86, 58, 24, 0.35)),
                        TreeNodeButton(l.node.id),
                    ))
                    .with_children(|m| {
                        if let Some(e) = atlas.get_tintable(l.node.id) {
                            m.spawn((widgets::icon_tinted(e, NODE * 0.6, INK), NodeIcon(l.node.id)));
                        }
                        m.spawn((
                            Node {
                                position_type: PositionType::Absolute,
                                right: Val::Px(-3.0),
                                bottom: Val::Px(-3.0),
                                width: Val::Px(15.0),
                                height: Val::Px(15.0),
                                align_items: AlignItems::Center,
                                justify_content: JustifyContent::Center,
                                border_radius: BorderRadius::all(Val::Percent(50.0)),
                                ..default()
                            },
                            BackgroundColor(rgba(231, 216, 176, 0.95)),
                            Visibility::Hidden,
                            LockBadge(l.node.id),
                            children![match atlas.get_tintable("sym:lock") {
                                Some(e) => widgets::icon_tinted(e, 9.0, INK_SOFT),
                                None => widgets::icon_tinted(
                                    (Handle::default(), false),
                                    9.0,
                                    INK_SOFT
                                ),
                            }],
                        ));
                    });
                // Name + cost block — the "basic info" readable without hovering. The name wraps
                // inside one column pitch so neighbouring labels can't collide.
                canvas.spawn((
                    Node {
                        position_type: PositionType::Absolute,
                        left: Val::Px(l.col * PITCH_X - 2.0),
                        top: Val::Px(y + NODE + 1.0),
                        width: Val::Px(PITCH_X + 4.0),
                        flex_direction: FlexDirection::Column,
                        align_items: AlignItems::Center,
                        row_gap: Val::Px(0.0),
                        ..default()
                    },
                    children![
                        (
                            label(&fonts.semibold, l.node.name, 9.5, INK),
                            bevy::text::TextLayout::new_with_justify(bevy::text::Justify::Center),
                        ),
                        (
                            label(&fonts.bold, cost_text(l.node), 10.0, rgb(154, 110, 22)),
                            CostLabel(l.node.id)
                        )
                    ],
                ));
            }
        });
    });
}

fn despawn_tree(mut commands: Commands, q: Query<Entity, With<TreeUi>>) {
    for e in &q {
        commands.entity(e).despawn();
    }
}

/// The header ✕ — click or Enter/E while focused closes the plans (same as U/Esc).
fn tree_close(
    mut next: ResMut<NextState<Modal>>,
    mut acts: MessageReader<FocusActivate>,
    btns: Query<(Entity, &Interaction), (With<TreeCloseBtn>, Changed<Interaction>)>,
    all: Query<Entity, With<TreeCloseBtn>>,
) {
    let keyed: Vec<Entity> = acts.read().map(|a| a.0).collect();
    let clicked = btns.iter().any(|(_, i)| *i == Interaction::Pressed)
        || all.iter().any(|e| keyed.contains(&e));
    if clicked {
        next.set(Modal::None);
    }
}

// ── Runtime systems ─────────────────────────────────────────────────────────────────────

/// Purchases: a real click or a focus activation (Enter/E) both buy the node.
#[allow(clippy::too_many_arguments)]
fn tree_interact(
    mut up: ResMut<Upgrades>,
    mut player: ResMut<PlayerRes>,
    mut bank: ResMut<Bank>,
    mut def: ResMut<Defenses>,
    mut eco: ResMut<EconomyState>,
    mut keep: ResMut<KeepHp>,
    mut acts: MessageReader<FocusActivate>,
    buttons: Query<(Entity, &Interaction, &TreeNodeButton)>,
) {
    let activated: Vec<Entity> = acts.read().map(|a| a.0).collect();
    for (e, interaction, btn) in &buttons {
        if *interaction == Interaction::Pressed || activated.contains(&e) {
            try_purchase(btn.0, &mut up, &mut player, &mut bank, &mut def, &mut eco, &mut keep);
            break;
        }
    }
}

/// Re-paint every node by state, fade/solidify prereq lines, refresh the treasury chips.
#[allow(clippy::type_complexity)]
fn tree_paint(
    up: Res<Upgrades>,
    player: Res<PlayerRes>,
    bank: Res<Bank>,
    mut nodes: Query<(&TreeNodeButton, &mut BackgroundColor, &mut BorderColor)>,
    mut icons: Query<(&NodeIcon, &mut ImageNode)>,
    mut locks: Query<(&LockBadge, &mut Visibility)>,
    mut costs: Query<(&CostLabel, &mut TextColor)>,
    mut edges: Query<(&EdgeOf, &mut BackgroundColor), Without<TreeNodeButton>>,
    mut gold_t: Query<&mut Text, (With<HeaderGold>, Without<HeaderStone>)>,
    mut stone_t: Query<&mut Text, (With<HeaderStone>, Without<HeaderGold>)>,
) {
    let gold = player.0.gold;
    let stone = bank.0.stone() as i64;

    #[derive(PartialEq, Clone, Copy)]
    enum S {
        Owned,
        Buyable,
        Poor,
        Locked,
    }
    let state = |id: &str| -> S {
        let Some(node) = node_by_id(id) else { return S::Locked };
        if up.0.is_purchased(id) {
            S::Owned
        } else if up.0.can_buy(node, gold, stone, false) {
            S::Buyable
        } else if node.prereq_id.is_some_and(|r| !up.0.is_purchased(r)) {
            S::Locked
        } else {
            S::Poor
        }
    };

    for (btn, mut bg, mut bc) in &mut nodes {
        let Some(node) = node_by_id(btn.0) else { continue };
        let (b, r) = match state(btn.0) {
            S::Owned => (branch_color(node.branch), rgba(36, 27, 12, 0.55)),
            S::Buyable => (rgba(255, 253, 244, 0.97), GOLD_DEEP),
            S::Poor => (rgba(245, 238, 222, 0.6), rgba(86, 58, 24, 0.35)),
            S::Locked => (rgba(235, 226, 205, 0.38), rgba(86, 58, 24, 0.22)),
        };
        bg.0 = b;
        *bc = BorderColor::all(r);
    }
    for (ic, mut img) in &mut icons {
        // Only monochrome icons carry tint (spawned non-WHITE); Twemoji fallbacks stay untinted.
        if img.color != Color::WHITE {
            img.color = match state(ic.0) {
                S::Owned => rgb(247, 240, 222),
                S::Locked => rgba(36, 27, 12, 0.38),
                _ => INK,
            };
        }
    }
    for (lk, mut vis) in &mut locks {
        *vis = if state(lk.0) == S::Locked { Visibility::Visible } else { Visibility::Hidden };
    }
    for (cl, mut tc) in &mut costs {
        tc.0 = match state(cl.0) {
            S::Owned => rgba(90, 69, 36, 0.45),
            S::Buyable => rgb(154, 110, 22),
            S::Poor => BRANCH_HERO,
            S::Locked => rgba(36, 27, 12, 0.35),
        };
    }
    for (ed, mut bg) in &mut edges {
        let solid = node_by_id(ed.0)
            .and_then(|n| n.prereq_id)
            .is_some_and(|req| up.0.is_purchased(req));
        bg.0 = if solid { rgba(36, 27, 12, 0.7) } else { rgba(36, 27, 12, 0.25) };
    }
    if let Ok(mut t) = gold_t.single_mut() {
        **t = format!("{gold}");
    }
    if let Ok(mut t) = stone_t.single_mut() {
        **t = format!("{stone}");
    }
}

/// Fill the detail strip from the focused/hovered node (hover == focus, see `ui::focus`).
fn tree_detail(
    focus: Res<UiFocus>,
    up: Res<Upgrades>,
    player: Res<PlayerRes>,
    bank: Res<Bank>,
    buttons: Query<&TreeNodeButton>,
    mut name_t: Query<(&mut Text, &mut TextColor), (With<DetailName>, Without<DetailDesc>, Without<DetailMeta>)>,
    mut desc_t: Query<&mut Text, (With<DetailDesc>, Without<DetailName>, Without<DetailMeta>)>,
    mut meta_t: Query<(&mut Text, &mut TextColor), (With<DetailMeta>, Without<DetailName>, Without<DetailDesc>)>,
) {
    let node = focus
        .current
        .and_then(|e| buttons.get(e).ok())
        .and_then(|b| node_by_id(b.0));
    let (Ok((mut name, mut name_c)), Ok(mut desc), Ok((mut meta, mut meta_c))) =
        (name_t.single_mut(), desc_t.single_mut(), meta_t.single_mut())
    else {
        return;
    };
    let Some(node) = node else {
        **name = "The War Table".into();
        name_c.0 = INK;
        **desc = "Hover or arrow onto an upgrade to read its charter.".into();
        **meta = "".into();
        return;
    };
    **name = node.name.into();
    name_c.0 = branch_color(node.branch);
    **desc = node.desc.into();
    let gold = player.0.gold;
    let stone = bank.0.stone() as i64;
    if up.0.is_purchased(node.id) {
        **meta = "Owned — already enacted.".into();
        meta_c.0 = rgb(92, 122, 52);
    } else if up.0.can_buy(node, gold, stone, false) {
        **meta = format!("Cost {} · Enter / E or click to purchase", cost_text(node));
        meta_c.0 = rgb(154, 110, 22);
    } else if let Some(req) = node.prereq_id.filter(|r| !up.0.is_purchased(r)) {
        let req_name = node_by_id(req).map(|n| n.name).unwrap_or(req);
        **meta = format!("Locked — requires {req_name}");
        meta_c.0 = INK_SOFT;
    } else {
        **meta = format!("Cost {} — the treasury can't cover it yet", cost_text(node));
        meta_c.0 = BRANCH_HERO;
    }
}
