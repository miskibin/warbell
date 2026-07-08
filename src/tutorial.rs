//! **Tutorial / "How to Play" panel** — a tabbed help screen reachable any time with **H**
//! while playing, and from the start menu's **HOW TO PLAY** button (so new players actually
//! find it before their first night). In-game it's the `Modal::Tutorial` sub-state, reusing
//! the freeze gate for free; on the start screen (where `Modal` doesn't exist) the same panel
//! is spawned/despawned directly. `Esc`/`H`/the header ✕ close it in both contexts.
//!
//! A near-fullscreen (90%) sheet in the medieval chrome, organised as **visual cards** rather
//! than a key-list: the Combat tab draws the ork roster with real HP/damage bars straight from
//! `tileworld_core::ork_config`, the Stronghold tab lists buildings with their true costs from
//! `town_store`, Economy shows where each resource comes from and goes, Survival draws the
//! day/night loop and the heir chain. Five tabs — **Basics / Combat / Stronghold / Economy /
//! Survival**. Switching a tab rebuilds the panel in place (the satchel pattern).

use bevy::ecs::relationship::RelatedSpawnerCommands;
use bevy::prelude::*;
use bevy::ui::FocusPolicy;

use tileworld_core::ork_config::{ork_config, OrkVariant};
use tileworld_core::town_store::{BuildKind, HOUSE_COST, POP_PER_HOUSE};

use crate::audio::AudioCue;
use crate::game_state::{AppState, Modal};
use crate::ui::anim::{anim, AnimKind};
use crate::ui::focus::{FocusActivate, Focusable};
use crate::ui::fonts::{label, UiFonts};
use crate::ui::texture::UiTextures;
use crate::ui::theme::*;
use crate::ui::widgets::{self, border};
use crate::ui::IconAtlas;
use crate::game_state::SimAppExt;

/// Active tab (index into [`TAB_NAMES`]). Reset to Basics each time the panel opens.
#[derive(Resource, Default)]
pub struct TutorialTab(usize);

#[derive(Component)]
struct TutorialUi;
/// A tab button, tagged with its tab index.
#[derive(Component)]
struct TabButton(usize);
/// The header ✕.
#[derive(Component)]
struct HelpCloseBtn;
/// The start-menu "HOW TO PLAY" button (spawned by `game_state`'s start screen).
#[derive(Component)]
pub struct StartHelpButton;

pub struct TutorialPlugin;

impl Plugin for TutorialPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<TutorialTab>()
            // Open with H — only while playing with no other panel up.
            .add_sim_systems(open_tutorial)
            .add_systems(OnEnter(Modal::Tutorial), spawn_tutorial)
            .add_systems(OnExit(Modal::Tutorial), despawn_tutorial)
            // Start-menu guide: the HOW TO PLAY button (or H) opens the same panel without the
            // Modal machinery (`Modal` only exists inside Playing); OnExit cleans it up if the
            // player starts a run with the guide still open.
            .add_systems(Update, menu_help.run_if(in_state(AppState::StartScreen)))
            .add_systems(OnExit(AppState::StartScreen), despawn_tutorial)
            // Tabs + ✕ + close keys work in both contexts (panel-presence gated, not Modal-gated).
            .add_systems(Update, tutorial_interact.run_if(any_with_component::<TutorialUi>));
    }
}

fn open_tutorial(
    keys: Res<ButtonInput<KeyCode>>,
    mut next: ResMut<NextState<Modal>>,
    mut tab: ResMut<TutorialTab>,
    mut auto_done: Local<bool>,
) {
    // Screenshot hook: `FOREST_PANEL=help` opens the guide once under the capture harness; a
    // trailing digit picks the tab (`help`/`help1`/.../`help4`). No effect in normal play.
    let staged = (!*auto_done)
        .then(|| std::env::var("FOREST_PANEL").ok())
        .flatten()
        .filter(|v| v.starts_with("help"));
    if let Some(v) = staged {
        *auto_done = true;
        tab.0 = v.trim_start_matches("help").parse::<usize>().unwrap_or(0).min(TAB_NAMES.len() - 1);
        next.set(Modal::Tutorial);
        return;
    }
    if keys.just_pressed(KeyCode::KeyH) {
        tab.0 = 0; // always land on Basics
        next.set(Modal::Tutorial);
    }
}

/// Start screen: the HOW TO PLAY button (click) or H opens the guide directly — no `Modal`
/// exists outside `Playing`, so the panel is just spawned; `tutorial_interact` closes it.
fn menu_help(
    keys: Res<ButtonInput<KeyCode>>,
    btns: Query<&Interaction, (With<StartHelpButton>, Changed<Interaction>)>,
    panel: Query<(), With<TutorialUi>>,
    mut tab: ResMut<TutorialTab>,
    mut cues: MessageWriter<AudioCue>,
    mut commands: Commands,
    fonts: Res<UiFonts>,
    atlas: Res<IconAtlas>,
    tex: Res<UiTextures>,
) {
    if !panel.is_empty() {
        return; // already open — closing belongs to tutorial_interact
    }
    let clicked = btns.iter().any(|i| *i == Interaction::Pressed);
    if clicked || keys.just_pressed(KeyCode::KeyH) {
        tab.0 = 0;
        cues.write(AudioCue::UiSelect);
        build_panel(&mut commands, 0, &fonts, &atlas, &tex);
    }
}

fn spawn_tutorial(
    mut commands: Commands,
    tab: Res<TutorialTab>,
    fonts: Res<UiFonts>,
    atlas: Res<IconAtlas>,
    tex: Res<UiTextures>,
) {
    build_panel(&mut commands, tab.0, &fonts, &atlas, &tex);
}

fn despawn_tutorial(mut commands: Commands, q: Query<Entity, With<TutorialUi>>) {
    for e in &q {
        commands.entity(e).try_despawn();
    }
}

/// H/Esc or the ✕ close the panel; clicking (or Enter/E-activating) a tab switches content,
/// rebuilding the panel in place. Runs whenever the panel exists — in `Modal::Tutorial` closing
/// goes through the state (its OnExit despawns); on the start screen it despawns directly.
#[allow(clippy::too_many_arguments)]
fn tutorial_interact(
    keys: Res<ButtonInput<KeyCode>>,
    modal: Option<Res<State<Modal>>>,
    mut tab: ResMut<TutorialTab>,
    mut next: ResMut<NextState<Modal>>,
    mut commands: Commands,
    fonts: Res<UiFonts>,
    atlas: Res<IconAtlas>,
    tex: Res<UiTextures>,
    mut cues: MessageWriter<AudioCue>,
    mut acts: MessageReader<FocusActivate>,
    tabs: Query<(Entity, &Interaction, &TabButton)>,
    close: Query<(Entity, &Interaction), With<HelpCloseBtn>>,
    panel: Query<Entity, With<TutorialUi>>,
) {
    let keyed: Vec<Entity> = acts.read().map(|a| a.0).collect();
    // Raw `Pressed` (no `Changed` filter) is safe here: closing despawns the panel, which stops
    // this system, so the press can't re-fire.
    let close_hit =
        close.iter().any(|(e, i)| *i == Interaction::Pressed || keyed.contains(&e));

    if keys.just_pressed(KeyCode::KeyH) || keys.just_pressed(KeyCode::Escape) || close_hit {
        if modal.as_ref().map_or(false, |m| *m.get() == Modal::Tutorial) {
            next.set(Modal::None); // OnExit(Modal::Tutorial) despawns
        } else {
            for e in &panel {
                commands.entity(e).try_despawn();
            }
        }
        return;
    }

    let mut pick = None;
    for (e, interaction, btn) in &tabs {
        if *interaction == Interaction::Pressed || keyed.contains(&e) {
            pick = Some(btn.0);
            break;
        }
    }
    if let Some(t) = pick {
        if t != tab.0 {
            tab.0 = t;
            cues.write(AudioCue::UiSelect);
            for e in &panel {
                commands.entity(e).try_despawn();
            }
            build_panel(&mut commands, t, &fonts, &atlas, &tex);
        }
    }
}

// ─── Content model ──────────────────────────────────────────────────────────────────

/// Tab labels + their icon key (tinted game-icons via [`IconAtlas`]).
const TAB_NAMES: [(&str, &str); 5] = [
    ("Basics", "buff:haste"),
    ("Combat", "hero_dmg_1"),
    ("Stronghold", "def_reinforce"),
    ("Economy", "sym:gold"),
    ("Survival", "sym:sun"),
];

/// Card-title gold (a touch brighter than `KICKER` so titles pop on the dark sub-cards).
const CARD_TITLE: Color = rgb(216, 178, 114);
const CARD_BG: Color = rgba(146, 122, 86, 0.07);

// ─── Small builders ─────────────────────────────────────────────────────────────────

/// A keycap chip (a small raised key, e.g. `E` or `LMB`).
fn keycap(font: &Handle<Font>, k: &str) -> impl Bundle {
    (
        Node {
            padding: UiRect::axes(Val::Px(8.0), Val::Px(4.0)),
            border: border(1.0),
            border_radius: radius(5.0),
            align_items: AlignItems::Center,
            justify_content: JustifyContent::Center,
            flex_shrink: 0.0,
            ..default()
        },
        widgets::keycap_paint(),
        children![label(font, k, 13.0, rgb(240, 226, 192))],
    )
}

/// A framed sub-card with a small-caps gold title; `f` fills the body.
fn section(
    p: &mut RelatedSpawnerCommands<ChildOf>,
    fonts: &UiFonts,
    title: &str,
    f: impl FnOnce(&mut RelatedSpawnerCommands<ChildOf>),
) {
    p.spawn((
        Node {
            width: Val::Percent(100.0),
            flex_direction: FlexDirection::Column,
            row_gap: Val::Px(11.0),
            padding: UiRect::all(Val::Px(16.0)),
            border: border(1.0),
            border_radius: radius(R_CARD),
            // Stretch to share the column height — a 90% sheet with top-huddled cards reads empty.
            flex_grow: 1.0,
            ..default()
        },
        BackgroundColor(CARD_BG),
        BorderColor::all(BORDER_SOFT),
    ))
    .with_children(|c| {
        c.spawn(label(&fonts.display, title, 14.0, CARD_TITLE));
        f(c);
    });
}

/// One explained point inside a card: a keycap **or** icon in a small gutter, then
/// `title — desc` text that wraps.
fn point(
    p: &mut RelatedSpawnerCommands<ChildOf>,
    fonts: &UiFonts,
    atlas: &IconAtlas,
    key: Option<&str>,
    icon: Option<&str>,
    title: &str,
    desc: &str,
) {
    p.spawn(Node {
        flex_direction: FlexDirection::Row,
        align_items: AlignItems::Center,
        column_gap: Val::Px(10.0),
        ..default()
    })
    .with_children(|r| {
        r.spawn(Node {
            min_width: Val::Px(40.0),
            justify_content: JustifyContent::Center,
            align_items: AlignItems::Center,
            flex_shrink: 0.0,
            ..default()
        })
        .with_children(|g| {
            if let Some(k) = key {
                g.spawn(keycap(&fonts.bold, k));
            } else if let Some(entry) = icon.and_then(|i| atlas.get_tintable(i)) {
                g.spawn(widgets::icon_tinted(entry, 24.0, GOLD));
            }
        });
        r.spawn(Node { flex_direction: FlexDirection::Column, row_gap: Val::Px(2.0), flex_grow: 1.0, ..default() })
            .with_children(|tc| {
                tc.spawn(label(&fonts.semibold, title, 15.0, TEXT));
                tc.spawn(label(&fonts.regular, desc, 13.0, TEXT_DIM));
            });
    });
}

/// A horizontal sample bar (HP / stamina / enemy-stat tracks).
fn hbar(p: &mut RelatedSpawnerCommands<ChildOf>, w: Val, frac: f32, top: Color, bot: Color) {
    p.spawn((
        Node {
            width: w,
            height: Val::Px(13.0),
            border: border(1.0),
            border_radius: radius(R_SLOT),
            overflow: Overflow::clip(),
            flex_shrink: 0.0,
            ..default()
        },
        BackgroundColor(SLOT_BG),
        BorderColor::all(SLOT_BORDER),
    ))
    .with_children(|track| {
        track.spawn((
            Node { width: Val::Percent(frac * 100.0), height: Val::Percent(100.0), ..default() },
            widgets::vgrad(top, bot),
        ));
    });
}

/// A small tinted pill (branch / biome chips).
fn pill(p: &mut RelatedSpawnerCommands<ChildOf>, font: &Handle<Font>, text: &str, tint: Color) {
    p.spawn((
        Node {
            padding: UiRect::axes(Val::Px(10.0), Val::Px(4.0)),
            border: border(1.0),
            border_radius: radius(R_CELL),
            ..default()
        },
        BackgroundColor(tint.with_alpha(0.18)),
        BorderColor::all(tint.with_alpha(0.55)),
    ))
    .with_children(|c| {
        c.spawn(label(font, text, 12.5, Color::WHITE.mix(&tint, 0.35)));
    });
}

/// A `2 wood · 4 stone` cost chip row.
fn cost_chips(p: &mut RelatedSpawnerCommands<ChildOf>, fonts: &UiFonts, atlas: &IconAtlas, wood: f64, stone: f64) {
    p.spawn(Node {
        flex_direction: FlexDirection::Row,
        align_items: AlignItems::Center,
        column_gap: Val::Px(8.0),
        flex_shrink: 0.0,
        ..default()
    })
    .with_children(|row| {
        let mut chip = |icon: &str, n: f64| {
            if n <= 0.0 {
                return;
            }
            row.spawn(Node {
                flex_direction: FlexDirection::Row,
                align_items: AlignItems::Center,
                column_gap: Val::Px(3.0),
                ..default()
            })
            .with_children(|c| {
                if let Some(entry) = atlas.get_tintable(icon) {
                    c.spawn(widgets::icon_tinted(entry, 15.0, GOLD));
                }
                c.spawn(label(&fonts.bold, format!("{}", n as i64), 13.0, GOLD));
            });
        };
        chip("stat:wood", wood);
        chip("stat:stone", stone);
    });
}

/// A column that takes an equal share of a row.
fn col() -> Node {
    Node {
        flex_direction: FlexDirection::Column,
        row_gap: Val::Px(14.0),
        flex_grow: 1.0,
        flex_basis: Val::Px(0.0),
        ..default()
    }
}

// ─── Panel shell ────────────────────────────────────────────────────────────────────

/// Build (or rebuild) the whole tutorial panel for the given active `tab`.
fn build_panel(
    commands: &mut Commands,
    tab: usize,
    fonts: &UiFonts,
    atlas: &IconAtlas,
    tex: &UiTextures,
) {
    // `FocusPolicy::Block` so the scrim swallows pointer picks — on the start screen the menu
    // buttons sit *under* it and must not stay clickable through the backdrop.
    commands.spawn((widgets::scrim(60), FocusPolicy::Block, TutorialUi)).with_children(|root| {
        root.spawn((
            Node {
                width: Val::Percent(90.0),
                height: Val::Percent(90.0),
                flex_direction: FlexDirection::Column,
                row_gap: Val::Px(12.0),
                padding: UiRect::axes(Val::Px(30.0), Val::Px(22.0)),
                border: border(2.0),
                border_radius: radius(R_PANEL),
                ..default()
            },
            widgets::card_paint(),
            anim(AnimKind::PopIn, 0.0, 0.26),
        ))
        .with_children(|card| {
            widgets::chrome_layers(card, tex.linen.clone());
            // ── Header ──
            card.spawn(Node {
                flex_direction: FlexDirection::Row,
                justify_content: JustifyContent::SpaceBetween,
                align_items: AlignItems::Center,
                padding: UiRect::bottom(Val::Px(9.0)),
                border: UiRect::bottom(Val::Px(1.0)),
                flex_shrink: 0.0,
                ..default()
            })
            .insert(BorderColor::all(BORDER_SOFT))
            .with_children(|h| {
                h.spawn(label(&fonts.display, "HOW TO PLAY", 22.0, GOLD));
                h.spawn(Node {
                    flex_direction: FlexDirection::Row,
                    align_items: AlignItems::Center,
                    column_gap: Val::Px(10.0),
                    ..default()
                })
                .with_children(|right| {
                    right.spawn(label(&fonts.semibold, "H / Esc to close", 11.0, GREY));
                    widgets::close_button(right, &fonts.bold, HelpCloseBtn, false);
                });
            });

            // ── Tab row ──
            card.spawn(Node {
                flex_direction: FlexDirection::Row,
                column_gap: Val::Px(7.0),
                flex_shrink: 0.0,
                ..default()
            })
            .with_children(|tabs| {
                for (i, (name, icon_key)) in TAB_NAMES.iter().enumerate() {
                    let on = i == tab;
                    tabs.spawn((
                        Button,
                        Interaction::default(),
                        Focusable,
                        Node {
                            flex_direction: FlexDirection::Row,
                            align_items: AlignItems::Center,
                            column_gap: Val::Px(8.0),
                            padding: UiRect::axes(Val::Px(19.0), Val::Px(10.0)),
                            border: border(1.0),
                            border_radius: radius(R_BTN),
                            ..default()
                        },
                        BackgroundColor(if on { PRIMARY } else { BTN_BG }),
                        BorderColor::all(if on { PRIMARY_BORDER } else { BORDER_SOFT }),
                        TabButton(i),
                    ))
                    .with_children(|b| {
                        if let Some(entry) = atlas.get_tintable(icon_key) {
                            b.spawn(widgets::icon_tinted(entry, 17.0, if on { INK } else { GOLD }));
                        }
                        b.spawn(label(&fonts.bold, *name, 14.5, if on { INK } else { TEXT_DIM }));
                    });
                }
            });

            // ── Body (fills the sheet; each tab lays out its own cards) ──
            card.spawn(Node {
                flex_direction: FlexDirection::Column,
                row_gap: Val::Px(12.0),
                padding: UiRect::top(Val::Px(4.0)),
                flex_grow: 1.0,
                min_height: Val::Px(0.0),
                overflow: Overflow::clip_y(),
                ..default()
            })
            .with_children(|body| match tab {
                1 => tab_combat(body, fonts, atlas),
                2 => tab_stronghold(body, fonts, atlas),
                3 => tab_economy(body, fonts, atlas),
                4 => tab_survival(body, fonts, atlas),
                _ => tab_basics(body, fonts, atlas),
            });

            // ── Footer ──
            card.spawn((
                Node { flex_shrink: 0.0, ..default() },
                children![label(
                    &fonts.regular,
                    "Click a tab or use \u{2190} \u{2192} + Enter to switch",
                    11.0,
                    GREY
                )],
            ));
        });
    });
}

/// Two equal columns side by side; the closures fill each.
fn two_cols(
    body: &mut RelatedSpawnerCommands<ChildOf>,
    left: impl FnOnce(&mut RelatedSpawnerCommands<ChildOf>),
    right: impl FnOnce(&mut RelatedSpawnerCommands<ChildOf>),
) {
    body.spawn(Node {
        flex_direction: FlexDirection::Row,
        column_gap: Val::Px(14.0),
        align_items: AlignItems::Stretch,
        width: Val::Percent(100.0),
        flex_grow: 1.0,
        ..default()
    })
    .with_children(|row| {
        row.spawn(col()).with_children(left);
        row.spawn(col()).with_children(right);
    });
}

// ─── Tabs ───────────────────────────────────────────────────────────────────────────

fn tab_basics(body: &mut RelatedSpawnerCommands<ChildOf>, fonts: &UiFonts, atlas: &IconAtlas) {
    two_cols(
        body,
        |l| {
            section(l, fonts, "ONE KEY FOR EVERYTHING \u{2014} E", |c| {
                c.spawn(label(
                    &fonts.regular,
                    "Walk up to anything important and press E \u{2014} a prompt names what you're about to do.",
                    13.0,
                    TEXT_DIM,
                ));
                point(c, fonts, atlas, None, Some("def_reinforce"), "At the keep", "Opens the War Table \u{2014} the upgrade tree.");
                point(c, fonts, atlas, None, Some("branch:arsenal"), "At the merchant stall", "Opens the shop \u{2014} weapons, armor, potions.");
                point(c, fonts, atlas, None, Some("stat:pop"), "On an empty plot", "Opens the build menu \u{2014} farms and worker yards rise on that exact plot (a gold ring marks it).");
                point(c, fonts, atlas, None, Some("stat:pop"), "At the timber site in the walls", "Raises a house on the spot \u{2014} beds for two more villagers.");
                point(c, fonts, atlas, None, Some("buff:power"), "At the war bell", "Rings in the night early, once you're ready.");
            });
            section(l, fonts, "CAMERA", |c| {
                point(c, fonts, atlas, Some("`"), None, "Toggle the fly-cam", "Follow-cam \u{2194} free camera. In fly-cam: hold RMB to look, Space/Ctrl for up/down, Shift to sprint.");
            });
        },
        |r| {
            section(r, fonts, "SCAVENGE \u{2014} F", |c| {
                point(c, fonts, atlas, None, Some("sym:gold"), "Chests", "Crack them open for gold, gear and supplies.");
                point(c, fonts, atlas, None, Some("stat:food"), "Forage", "Pick herbs, apples and other plants for the satchel.");
                point(c, fonts, atlas, None, Some("sym:lock"), "Caged villagers", "Free them \u{2014} they walk home and join your town.");
            });
            section(r, fonts, "SATCHEL & QUICK-BAR", |c| {
                point(c, fonts, atlas, Some("I"), None, "Satchel", "Your bag \u{2014} eat, equip weapons and armor, pin items.");
                point(c, fonts, atlas, Some("Q"), None, "Eat", "Chews the best food in the bag to heal.");
                point(c, fonts, atlas, Some("Y"), None, "Quick-slots  Y / T", "Resist and power potions. In the satchel, hover an item and press a key to pin it.");
                point(c, fonts, atlas, Some("R"), None, "Recruit", "Rallies nearby villagers to fight at your side.");
            });
        },
    );
}

fn tab_combat(body: &mut RelatedSpawnerCommands<ChildOf>, fonts: &UiFonts, atlas: &IconAtlas) {
    two_cols(
        body,
        |l| {
            section(l, fonts, "KNOW YOUR ENEMY", |c| {
                c.spawn(label(
                    &fonts.regular,
                    "Each night's horde mixes four breeds. Health and hit numbers below are night one \u{2014} they grow every wave.",
                    13.0,
                    TEXT_DIM,
                ));
                let max_hp = ork_config(OrkVariant::Berserker).hp;
                for (v, name, trait_) in [
                    (OrkVariant::Grunt, "Grunt", "The line-filler. Slow, steady, everywhere."),
                    (OrkVariant::Scout, "Scout", "Fast and fragile \u{2014} slips past the walls."),
                    (OrkVariant::Berserker, "Berserker", "Huge and brutal. Do not trade blows."),
                    (OrkVariant::Shaman, "Shaman", "Lobs bolts from range and heals its kin \u{2014} kill it first."),
                ] {
                    let cfg = ork_config(v);
                    c.spawn(Node {
                        flex_direction: FlexDirection::Column,
                        row_gap: Val::Px(3.0),
                        ..default()
                    })
                    .with_children(|row| {
                        row.spawn(Node {
                            flex_direction: FlexDirection::Row,
                            justify_content: JustifyContent::SpaceBetween,
                            align_items: AlignItems::Center,
                            ..default()
                        })
                        .with_children(|head| {
                            head.spawn(label(&fonts.semibold, name, 15.0, TEXT));
                            head.spawn(label(
                                &fonts.bold,
                                format!("{} HP \u{00b7} {} dmg", cfg.hp as i64, cfg.damage as i64),
                                12.0,
                                GREY,
                            ));
                        });
                        hbar(row, Val::Percent(100.0), (cfg.hp / max_hp) as f32, HP_TOP, HP_BOT);
                        row.spawn(label(&fonts.regular, trait_, 12.5, TEXT_DIM));
                    });
                }
            });
        },
        |r| {
            section(r, fonts, "SWORDPLAY", |c| {
                point(c, fonts, atlas, Some("LMB"), None, "Attack", "Swing your blade \u{2014} chained swings combo (1-2-3), each step faster and harder. Levels, weapons and crits raise the damage.");
                point(c, fonts, atlas, Some("RMB"), None, "Block / Parry", "Raise the shield to stop incoming damage \u{2014} it drains stamina. Raise it the INSTANT a blow lands to PARRY: free, staggers the foe, and your next strike counters for a guaranteed crit.");
                point(c, fonts, atlas, Some("Alt"), None, "Dodge roll", "Tuck and roll clear of a blow \u{2014} brief invulnerability at the start, costs stamina. No direction held rolls you backward.");
                c.spawn(Node {
                    flex_direction: FlexDirection::Row,
                    align_items: AlignItems::Center,
                    column_gap: Val::Px(8.0),
                    ..default()
                })
                .with_children(|s| {
                    hbar(s, Val::Px(170.0), 0.55, STAM_TOP, STAM_BOT);
                    s.spawn(label(&fonts.regular, "stamina \u{2014} recovers when you lower the shield", 11.0, GREY));
                });
            });
            section(r, fonts, "STAY ALIVE", |c| {
                c.spawn(Node {
                    flex_direction: FlexDirection::Row,
                    align_items: AlignItems::Center,
                    column_gap: Val::Px(8.0),
                    ..default()
                })
                .with_children(|s| {
                    if let Some(entry) = atlas.get_tintable("hero_hp_1") {
                        s.spawn(widgets::icon_tinted(entry, 18.0, RED));
                    }
                    hbar(s, Val::Px(170.0), 0.68, HP_TOP, HP_BOT);
                    s.spawn(label(&fonts.regular, "your health", 11.0, GREY));
                });
                point(c, fonts, atlas, Some("Q"), None, "Eat to heal", "Food from foraging, chests and farms. Stock up before dusk.");
                point(c, fonts, atlas, None, Some("buff:resist"), "Potions", "Resist (Z), power (X) and haste (C) turn a bad night around.");
            });
            section(r, fonts, "GEAR UP", |c| {
                point(c, fonts, atlas, None, Some("sword_iron"), "Weapons", "Buy at the merchant, equip in the satchel. Each tier hits harder.");
                point(c, fonts, atlas, None, Some("iron_armor"), "Armor", "Soaks a share of every hit \u{2014} the cheapest way to survive late nights.");
            });
        },
    );
}

fn tab_stronghold(body: &mut RelatedSpawnerCommands<ChildOf>, fonts: &UiFonts, atlas: &IconAtlas) {
    town_loop(body, fonts, atlas);
    two_cols(
        body,
        |l| {
            section(l, fonts, "BUILD ON PLOTS \u{2014} E", |c| {
                c.spawn(label(
                    &fonts.regular,
                    "Empty plots ring the castle. Stand on one and press E \u{2014} a gold ring marks the plot you're building on. Every producer needs a villager to staff it.",
                    13.0,
                    TEXT_DIM,
                ));
                // (icon, name, cost, effect) — costs straight from the parity-tested core.
                let rows: [(&str, &str, f64, f64, String); 4] = [
                    ("stat:pop", "House", HOUSE_COST.wood, HOUSE_COST.stone, format!("Shelters {POP_PER_HOUSE} more. Raised INSIDE the walls \u{2014} press E at the timber site pad.")),
                    ("stat:food", "Farm", BuildKind::Farm.cost().wood, BuildKind::Farm.cost().stone, "Grows food while staffed.".into()),
                    ("stat:wood", "Woodcutter", BuildKind::Lumber.cost().wood, BuildKind::Lumber.cost().stone, "Its worker fells real trees \u{2014} wood per tree.".into()),
                    ("stat:stone", "Stone Miner", BuildKind::Mine.cost().wood, BuildKind::Mine.cost().stone, "Its worker picks apart ore boulders \u{2014} stone per haul.".into()),
                ];
                for (icon, name, wood, stone, effect) in rows {
                    c.spawn(Node {
                        flex_direction: FlexDirection::Row,
                        align_items: AlignItems::Center,
                        column_gap: Val::Px(10.0),
                        ..default()
                    })
                    .with_children(|row| {
                        if let Some(entry) = atlas.get_tintable(icon) {
                            row.spawn(widgets::icon_tinted(entry, 24.0, GOLD));
                        }
                        row.spawn(Node {
                            flex_direction: FlexDirection::Column,
                            row_gap: Val::Px(2.0),
                            flex_grow: 1.0,
                            ..default()
                        })
                        .with_children(|tc| {
                            tc.spawn(label(&fonts.semibold, name, 15.0, TEXT));
                            tc.spawn(label(&fonts.regular, effect, 12.5, TEXT_DIM));
                        });
                        cost_chips(row, fonts, atlas, wood, stone);
                    });
                }
                c.spawn(label(
                    &fonts.regular,
                    "Wood and stone bootstrap each other: the Woodcutter costs only stone, the Miner only wood.",
                    12.5,
                    GREY,
                ));
            });
        },
        |r| {
            section(r, fonts, "POPULATION", |c| {
                point(c, fonts, atlas, Some("F"), None, "Rescue", "Freed villagers walk home and join the workforce.");
                point(c, fonts, atlas, None, Some("stat:food"), "Food decides growth", "Villagers eat every day. A surplus draws in new settlers \u{2014} famine drives them off.");
                point(c, fonts, atlas, None, Some("stat:pop"), "Houses cap it", "No beds, no settlers. Build houses to grow past the founding pair.");
            });
            section(r, fonts, "THE NIGHT TEST", |c| {
                point(c, fonts, atlas, None, Some("sym:warn"), "Raiders torch buildings", "Part of every wave splits off to burn your town instead of the keep.");
                point(c, fonts, atlas, Some("R"), None, "Militia", "Rallied villagers and posted guards fight back. Dead villagers stay dead \u{2014} only food surplus replaces them.");
                point(c, fonts, atlas, None, Some("def_walls"), "Walls & towers", "Bought at the War Table \u{2014} they keep the horde off your producers. Damage mends itself by day.");
            });
        },
    );
}

fn tab_economy(body: &mut RelatedSpawnerCommands<ChildOf>, fonts: &UiFonts, atlas: &IconAtlas) {
    // Three resource cards: where it comes from → what it buys.
    body.spawn(Node {
        flex_direction: FlexDirection::Row,
        column_gap: Val::Px(14.0),
        align_items: AlignItems::Stretch,
        width: Val::Percent(100.0),
        flex_grow: 1.0,
        ..default()
    })
    .with_children(|row| {
        for (icon, name, from, spend) in [
            ("sym:gold", "GOLD", "Ork bounties \u{00b7} the dawn tithe (every villager pays after a survived night) \u{00b7} chests (caches restock at dawn).", "Merchant gear & potions \u{00b7} War Table upgrades."),
            ("sym:stone", "STONE", "Smash ore boulders with your attack; Stone Miner workers cart it home.", "Walls, towers & defenses \u{00b7} the Woodcutter yard."),
            ("stat:wood", "WOOD", "Chop trees yourself; Woodcutter workers fell them all day.", "Houses & farms \u{00b7} the Miner camp."),
        ] {
            row.spawn((
                col(),
                BackgroundColor(CARD_BG),
                BorderColor::all(BORDER_SOFT),
            ))
            .insert(Node {
                flex_direction: FlexDirection::Column,
                row_gap: Val::Px(8.0),
                padding: UiRect::all(Val::Px(14.0)),
                border: border(1.0),
                border_radius: radius(R_CARD),
                flex_grow: 1.0,
                flex_basis: Val::Px(0.0),
                ..default()
            })
            .with_children(|c| {
                c.spawn(Node {
                    flex_direction: FlexDirection::Row,
                    align_items: AlignItems::Center,
                    column_gap: Val::Px(8.0),
                    ..default()
                })
                .with_children(|h| {
                    if let Some(entry) = atlas.get_tintable(icon) {
                        h.spawn(widgets::icon_tinted(entry, 27.0, GOLD));
                    }
                    h.spawn(label(&fonts.display, name, 15.5, CARD_TITLE));
                });
                c.spawn(label(&fonts.bold, "EARN", 10.5, GREY));
                c.spawn(label(&fonts.regular, from, 13.0, TEXT_DIM));
                c.spawn(label(&fonts.bold, "SPEND", 10.5, GREY));
                c.spawn(label(&fonts.regular, spend, 13.0, TEXT_DIM));
            });
        }
    });
    two_cols(
        body,
        |l| {
            section(l, fonts, "THE WAR TABLE \u{2014} E AT THE KEEP", |c| {
                c.spawn(label(
                    &fonts.regular,
                    "Four branches of permanent upgrades. Early picks shape your whole run \u{2014} walls before night two is the classic opener.",
                    13.0,
                    TEXT_DIM,
                ));
                c.spawn(Node {
                    flex_direction: FlexDirection::Row,
                    flex_wrap: FlexWrap::Wrap,
                    column_gap: Val::Px(6.0),
                    row_gap: Val::Px(6.0),
                    ..default()
                })
                .with_children(|chips| {
                    pill(chips, &fonts.bold, "Prosperity \u{2014} income & town", BRANCH_ECON);
                    pill(chips, &fonts.bold, "Bulwark \u{2014} walls & towers", BRANCH_DEF);
                    pill(chips, &fonts.bold, "Champion \u{2014} your knight", BRANCH_HERO);
                    pill(chips, &fonts.bold, "Armoury \u{2014} weapons & shop", BRANCH_ARSENAL);
                });
            });
        },
        |r| {
            section(r, fonts, "THE MERCHANT \u{2014} E AT THE STALL", |c| {
                point(c, fonts, atlas, None, Some("sword_gold"), "Weapons & armor", "Straight power for gold. New stock unlocks via Armoury upgrades.");
                point(c, fonts, atlas, None, Some("potion"), "Potions & food", "Heals and buffs \u{2014} pin them to Y/T from the satchel.");
            });
        },
    );
}

fn tab_survival(body: &mut RelatedSpawnerCommands<ChildOf>, fonts: &UiFonts, atlas: &IconAtlas) {
    day_night_bar(body, fonts, atlas);
    two_cols(
        body,
        |l| {
            section(l, fonts, "THE KEEP IS THE RUN", |c| {
                c.spawn(Node {
                    flex_direction: FlexDirection::Row,
                    align_items: AlignItems::Center,
                    column_gap: Val::Px(8.0),
                    ..default()
                })
                .with_children(|s| {
                    if let Some(entry) = atlas.get_tintable("def_reinforce") {
                        s.spawn(widgets::icon_tinted(entry, 18.0, GOLD));
                    }
                    hbar(s, Val::Px(190.0), 0.8, HP_TOP, HP_BOT);
                    s.spawn(label(&fonts.regular, "keep HP", 11.0, GREY));
                });
                point(c, fonts, atlas, None, Some("sym:warn"), "If it falls, the run ends", "Orks that reach the keep batter it down. Walls, towers and your sword are what stand between.");
                point(c, fonts, atlas, None, Some("def_walls"), "It heals by day", "Keep and town damage mend during daylight \u{2014} survive the night and regroup.");
            });
            section(l, fonts, "RING THE BELL", |c| {
                point(c, fonts, atlas, Some("E"), None, "Start the night early", "Done preparing? The war bell by the keep calls the horde now \u{2014} less waiting, same rewards.");
            });
        },
        |r| {
            section(r, fonts, "SUCCESSION", |c| {
                c.spawn(Node {
                    flex_direction: FlexDirection::Row,
                    align_items: AlignItems::Center,
                    column_gap: Val::Px(8.0),
                    ..default()
                })
                .with_children(|chain| {
                    for (i, a) in [1.0_f32, 0.66, 0.4].into_iter().enumerate() {
                        if i > 0 {
                            chain.spawn(children![label(&fonts.bold, "\u{2192}", 13.0, rgba(224, 168, 74, 0.6))]);
                        }
                        widgets::medallion(
                            chain,
                            atlas.get_tintable("branch:hero"),
                            30.0,
                            rgba(224, 168, 74, 0.5 * a),
                            rgba(146, 122, 86, 0.18),
                            GOLD.with_alpha(a),
                        );
                    }
                    chain.spawn(label(&fonts.regular, "the bloodline", 11.0, GREY));
                });
                point(c, fonts, atlas, None, Some("branch:hero"), "Death is not the end", "When your knight falls, an heir takes up the blade \u{2014} the run continues. Run out of heirs and it's over.");
            });
            section(r, fonts, "FIVE BIOMES", |c| {
                c.spawn(Node {
                    flex_direction: FlexDirection::Row,
                    flex_wrap: FlexWrap::Wrap,
                    column_gap: Val::Px(6.0),
                    row_gap: Val::Px(6.0),
                    ..default()
                })
                .with_children(|chips| {
                    pill(chips, &fonts.bold, "Forest", rgb(92, 142, 62));
                    pill(chips, &fonts.bold, "Desert", rgb(214, 178, 94));
                    pill(chips, &fonts.bold, "Snow", rgb(196, 214, 228));
                    pill(chips, &fonts.bold, "Swamp", rgb(96, 124, 84));
                    pill(chips, &fonts.bold, "Rock", rgb(148, 148, 152));
                });
                point(c, fonts, atlas, None, Some("branch:economy"), "Each has its own bounty", "Different plants, prey, predators and chests \u{2014} ranging farther pays, but watch the time of day.");
            });
        },
    );
}

// ─── Diagrams ───────────────────────────────────────────────────────────────────────

/// The day → night loop bar (Survival tab): a warm "day" segment and a deep-blue "night" one.
fn day_night_bar(body: &mut RelatedSpawnerCommands<ChildOf>, fonts: &UiFonts, atlas: &IconAtlas) {
    body.spawn((
        Node {
            width: Val::Percent(100.0),
            height: Val::Px(52.0),
            flex_direction: FlexDirection::Row,
            border: border(1.0),
            border_radius: radius(R_CARD),
            overflow: Overflow::clip(),
            flex_shrink: 0.0,
            ..default()
        },
        BorderColor::all(BORDER_SOFT),
    ))
    .with_children(|bar| {
        // Day.
        bar.spawn((
            Node {
                width: Val::Percent(58.0),
                height: Val::Percent(100.0),
                flex_direction: FlexDirection::Row,
                align_items: AlignItems::Center,
                justify_content: JustifyContent::Center,
                column_gap: Val::Px(8.0),
                ..default()
            },
            widgets::vgrad(rgb(247, 206, 120), GOLD_DEEP),
        ))
        .with_children(|d| {
            if let Some(entry) = atlas.get_tintable("sym:sun") {
                d.spawn(widgets::icon_tinted(entry, 21.0, rgb(74, 52, 16)));
            }
            d.spawn(label(&fonts.bold, "DAY \u{2014} loot, build, upgrade", 14.5, rgb(48, 34, 10)));
        });
        // Night.
        bar.spawn((
            Node {
                width: Val::Percent(42.0),
                height: Val::Percent(100.0),
                flex_direction: FlexDirection::Row,
                align_items: AlignItems::Center,
                justify_content: JustifyContent::Center,
                column_gap: Val::Px(8.0),
                ..default()
            },
            BackgroundColor(rgb(24, 28, 52)),
        ))
        .with_children(|n| {
            if let Some(entry) = atlas.get_tintable("buff:power") {
                n.spawn(widgets::icon_tinted(entry, 18.0, rgb(176, 188, 220)));
            }
            n.spawn(label(&fonts.bold, "NIGHT \u{2014} the orks come", 14.5, rgb(204, 214, 238)));
        });
    });
}

/// The Stronghold tab's loop strip: rescue → build → feed → defend, as framed medallions.
fn town_loop(body: &mut RelatedSpawnerCommands<ChildOf>, fonts: &UiFonts, atlas: &IconAtlas) {
    const STEPS: [(&str, &str, &str); 4] = [
        ("sym:lock", "RESCUE", "free caged villagers"),
        ("stat:pop", "BUILD", "houses, farms, yards"),
        ("stat:food", "FEED", "surplus grows the town"),
        ("def_walls", "DEFEND", "hold it through the night"),
    ];
    body.spawn((
        Node {
            width: Val::Percent(100.0),
            flex_direction: FlexDirection::Row,
            align_items: AlignItems::Center,
            justify_content: JustifyContent::Center,
            column_gap: Val::Px(20.0),
            padding: UiRect::axes(Val::Px(12.0), Val::Px(10.0)),
            border: border(1.0),
            border_radius: radius(R_CARD),
            flex_shrink: 0.0,
            ..default()
        },
        BackgroundColor(rgba(146, 122, 86, 0.08)),
        BorderColor::all(BORDER_SOFT),
    ))
    .with_children(|strip| {
        for (i, (icon_key, name, sub)) in STEPS.iter().enumerate() {
            if i > 0 {
                strip.spawn((
                    Node { margin: UiRect::bottom(Val::Px(26.0)), ..default() },
                    children![label(&fonts.bold, "\u{2192}", 18.0, rgba(224, 168, 74, 0.65))],
                ));
            }
            strip
                .spawn(Node {
                    flex_direction: FlexDirection::Column,
                    align_items: AlignItems::Center,
                    row_gap: Val::Px(3.0),
                    ..default()
                })
                .with_children(|step| {
                    widgets::medallion(
                        step,
                        atlas.get_tintable(icon_key),
                        44.0,
                        rgba(224, 168, 74, 0.5),
                        rgba(146, 122, 86, 0.18),
                        GOLD,
                    );
                    step.spawn(label(&fonts.bold, *name, 12.5, TEXT));
                    step.spawn(label(&fonts.regular, *sub, 11.0, GREY));
                });
        }
    });
}
