//! **Tutorial / "How to Play" panel** — a tabbed help screen reachable any time with **H**
//! while playing. It's a `Modal::Tutorial` sub-state, so it reuses the whole freeze gate for
//! free: opening it stops the world-sim (everything is gated on `Modal::None`), and
//! `player::camera` frees the cursor whenever a modal is up so the tab buttons are clickable.
//! `Esc` closes it via the shared `game_state::pause_toggle`; `H` toggles it shut too.
//!
//! Four tabs — **Basics / Combat / Economy / Survival** — built entirely from the existing UI
//! kit (Twemoji icons from [`IconAtlas`] + keycap chips), so it ships no new art. Switching a
//! tab rebuilds the panel in place, the same despawn-and-rebuild pattern the satchel uses.

use bevy::ecs::relationship::RelatedSpawnerCommands;
use bevy::prelude::*;

use crate::audio::AudioCue;
use crate::game_state::Modal;
use crate::ui::anim::{anim, AnimKind};
use crate::ui::fonts::{label, UiFonts};
use crate::ui::theme::*;
use crate::ui::widgets::{self, border};
use crate::ui::IconAtlas;

/// Active tab (index into [`TAB_NAMES`]). Reset to Basics each time the panel opens.
#[derive(Resource, Default)]
pub struct TutorialTab(usize);

#[derive(Component)]
struct TutorialUi;
/// A tab button, tagged with its tab index.
#[derive(Component)]
struct TabButton(usize);

pub struct TutorialPlugin;

impl Plugin for TutorialPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<TutorialTab>()
            // Open with H — only while playing with no other panel up.
            .add_systems(Update, open_tutorial.run_if(in_state(Modal::None)))
            .add_systems(OnEnter(Modal::Tutorial), spawn_tutorial)
            .add_systems(OnExit(Modal::Tutorial), despawn_tutorial)
            .add_systems(Update, tutorial_interact.run_if(in_state(Modal::Tutorial)));
    }
}

fn open_tutorial(
    keys: Res<ButtonInput<KeyCode>>,
    mut next: ResMut<NextState<Modal>>,
    mut tab: ResMut<TutorialTab>,
    mut auto_done: Local<bool>,
) {
    // Screenshot hook: `FOREST_PANEL=help` opens the guide once under the capture harness; a
    // trailing digit picks the tab (`help`/`help1`/`help2`/`help3`). No effect in normal play.
    let staged = (!*auto_done)
        .then(|| std::env::var("FOREST_PANEL").ok())
        .flatten()
        .filter(|v| v.starts_with("help"));
    if let Some(v) = staged {
        *auto_done = true;
        tab.0 = v.trim_start_matches("help").parse::<usize>().unwrap_or(0).min(3);
        next.set(Modal::Tutorial);
        return;
    }
    if keys.just_pressed(KeyCode::KeyH) {
        tab.0 = 0; // always land on Basics
        next.set(Modal::Tutorial);
    }
}

fn spawn_tutorial(
    mut commands: Commands,
    tab: Res<TutorialTab>,
    fonts: Res<UiFonts>,
    atlas: Res<IconAtlas>,
) {
    build_panel(&mut commands, tab.0, &fonts, &atlas);
}

fn despawn_tutorial(mut commands: Commands, q: Query<Entity, With<TutorialUi>>) {
    for e in &q {
        commands.entity(e).despawn();
    }
}

/// H toggles the panel shut; clicking a tab switches content (rebuilds the panel in place).
#[allow(clippy::too_many_arguments)]
fn tutorial_interact(
    keys: Res<ButtonInput<KeyCode>>,
    mut tab: ResMut<TutorialTab>,
    mut next: ResMut<NextState<Modal>>,
    mut commands: Commands,
    fonts: Res<UiFonts>,
    atlas: Res<IconAtlas>,
    mut cues: MessageWriter<AudioCue>,
    buttons: Query<(&Interaction, &TabButton), Changed<Interaction>>,
    panel: Query<Entity, With<TutorialUi>>,
) {
    if keys.just_pressed(KeyCode::KeyH) {
        next.set(Modal::None);
        return;
    }
    let mut pick = None;
    for (interaction, btn) in &buttons {
        if *interaction == Interaction::Pressed {
            pick = Some(btn.0);
            break;
        }
    }
    if let Some(t) = pick {
        if t != tab.0 {
            tab.0 = t;
            cues.write(AudioCue::UiSelect);
            for e in &panel {
                commands.entity(e).despawn();
            }
            build_panel(&mut commands, t, &fonts, &atlas);
        }
    }
}

// ─── Content model ──────────────────────────────────────────────────────────────────

/// One thing shown in a row's left gutter — either a keyboard key or an [`IconAtlas`] icon.
#[derive(Clone, Copy)]
enum Chip {
    Key(&'static str),
    Icon(&'static str),
}

/// A single how-to row: a few chips, a title, and a one-line explanation.
struct Row {
    chips: &'static [Chip],
    title: &'static str,
    desc: &'static str,
}

/// An optional illustrative mini-diagram appended to a tab.
#[derive(Clone, Copy, PartialEq)]
enum Diagram {
    None,
    /// Combat: a sample HP bar.
    HpBar,
    /// Survival: the day → night loop bar (rendered at the top).
    DayNight,
}

/// Tab labels + their icon key (Twemoji via [`IconAtlas`]).
const TAB_NAMES: [(&str, &str); 4] = [
    ("Basics", "buff:haste"),
    ("Combat", "buff:power"),
    ("Economy", "sym:gold"),
    ("Survival", "sym:castle"),
];

use Chip::{Icon, Key};

const BASICS: &[Row] = &[
    Row { chips: &[Key("W"), Key("A"), Key("S"), Key("D")], title: "Move", desc: "Walk your knight around the island." },
    Row { chips: &[Key("`")], title: "Camera", desc: "Toggle free fly-cam \u{2194} follow-cam. Hold RMB in fly-cam to look around." },
    Row { chips: &[Key("E")], title: "Interact", desc: "Walk up and press E: the keep opens upgrades, the stall opens the shop, the war bell starts the night." },
    Row { chips: &[Key("F")], title: "Loot & forage", desc: "Open chests, gather plants, and rescue villagers." },
    Row { chips: &[Key("I")], title: "Satchel", desc: "Open your bag to use or equip items." },
    Row { chips: &[Key("R")], title: "Recruit", desc: "Rally nearby villagers to fight at your side." },
    Row { chips: &[Key("H")], title: "Help", desc: "Open this guide any time." },
];

const COMBAT: &[Row] = &[
    Row { chips: &[Key("LMB")], title: "Attack", desc: "Swing your weapon. Levels, gear and crits all raise your damage." },
    Row { chips: &[Key("RMB")], title: "Block", desc: "Raise your shield to cut incoming damage \u{2014} it drains stamina, so let it recover." },
    Row { chips: &[Icon("buff:power")], title: "Night raids", desc: "Orks besiege the keep each night: grunts, scouts, berserkers and bolt-throwing shamans." },
    Row { chips: &[Icon("sym:hp")], title: "Stay alive", desc: "Watch your HP. Eat food (Q) to heal between fights." },
];

const ECONOMY: &[Row] = &[
    Row { chips: &[Icon("sym:gold")], title: "Gold", desc: "Dropped by kills and chests. Spend it at the shop and War Table." },
    Row { chips: &[Icon("sym:stone")], title: "Stone", desc: "Mine ore with your attack. It pays for walls and defenses." },
    Row { chips: &[Key("F")], title: "Chests", desc: "Crack chests open for gold, gear and supplies." },
    Row { chips: &[Icon("sym:castle")], title: "War Table", desc: "Press E at the keep to buy upgrades across four branches." },
    Row { chips: &[Icon("branch:arsenal")], title: "Merchant", desc: "Press E at the stall to trade for weapons, armor and potions." },
    Row { chips: &[Key("Q"), Key("Z"), Key("X"), Key("C")], title: "Quick-bar", desc: "Q eat food \u{00b7} Z resist \u{00b7} X power \u{00b7} C haste." },
];

const SURVIVAL: &[Row] = &[
    Row { chips: &[Icon("sym:sun")], title: "Day & night", desc: "By day you loot and prepare; by night the orks come." },
    Row { chips: &[Icon("sym:castle")], title: "Defend the keep", desc: "If the keep's HP hits zero, you lose. Walls and towers buy you time." },
    Row { chips: &[Key("E")], title: "Ring the bell", desc: "Done preparing? Ring the war bell to summon the night early." },
    Row { chips: &[Icon("buff:power")], title: "Succession", desc: "When your hero falls an heir takes up the blade. Run out of heirs and the run ends." },
    Row { chips: &[Icon("branch:economy")], title: "Five biomes", desc: "Forest, desert, snow, swamp and rocky lands ring the island, each with its own life." },
];

/// Rows + trailing diagram for a tab index.
fn tab_content(tab: usize) -> (&'static [Row], Diagram) {
    match tab {
        1 => (COMBAT, Diagram::HpBar),
        2 => (ECONOMY, Diagram::None),
        3 => (SURVIVAL, Diagram::DayNight),
        _ => (BASICS, Diagram::None),
    }
}

// ─── Build ──────────────────────────────────────────────────────────────────────────

/// A keycap chip (a small raised key, e.g. `E` or `LMB`).
fn keycap(font: &Handle<Font>, k: &str) -> impl Bundle {
    (
        Node {
            padding: UiRect::axes(Val::Px(8.0), Val::Px(4.0)),
            border: border(1.0),
            border_radius: radius(5.0),
            align_items: AlignItems::Center,
            justify_content: JustifyContent::Center,
            ..default()
        },
        widgets::keycap_paint(),
        children![label(font, k, 12.0, rgb(233, 238, 251))],
    )
}

/// Build (or rebuild) the whole tutorial panel for the given active `tab`.
fn build_panel(commands: &mut Commands, tab: usize, fonts: &UiFonts, atlas: &IconAtlas) {
    let (rows, diagram) = tab_content(tab);

    commands.spawn((widgets::scrim(60), TutorialUi)).with_children(|root| {
        root.spawn((
            Node {
                flex_direction: FlexDirection::Column,
                min_width: Val::Px(600.0),
                max_width: Val::Px(640.0),
                row_gap: Val::Px(14.0),
                padding: UiRect::axes(Val::Px(28.0), Val::Px(24.0)),
                border: border(1.0),
                border_radius: radius(R_PANEL),
                ..default()
            },
            widgets::card_paint(),
            anim(AnimKind::PopIn, 0.0, 0.26),
        ))
        .with_children(|card| {
            // ── Header ──
            card.spawn(Node {
                flex_direction: FlexDirection::Row,
                justify_content: JustifyContent::SpaceBetween,
                align_items: AlignItems::Center,
                padding: UiRect::bottom(Val::Px(10.0)),
                border: UiRect::bottom(Val::Px(1.0)),
                ..default()
            })
            .insert(BorderColor::all(BORDER_SOFT))
            .with_children(|h| {
                h.spawn(label(&fonts.extrabold, "HOW TO PLAY", 19.0, TEXT));
                h.spawn(label(&fonts.semibold, "H / Esc to close", 12.0, GREY));
            });

            // ── Tab row ──
            card.spawn(Node {
                flex_direction: FlexDirection::Row,
                column_gap: Val::Px(6.0),
                ..default()
            })
            .with_children(|tabs| {
                for (i, (name, icon_key)) in TAB_NAMES.iter().enumerate() {
                    let on = i == tab;
                    tabs.spawn((
                        Button,
                        Interaction::default(),
                        Node {
                            flex_direction: FlexDirection::Row,
                            align_items: AlignItems::Center,
                            column_gap: Val::Px(7.0),
                            padding: UiRect::axes(Val::Px(16.0), Val::Px(9.0)),
                            border: border(1.0),
                            border_radius: radius(R_BTN),
                            ..default()
                        },
                        BackgroundColor(if on { GOLD_DEEP } else { BTN_BG }),
                        BorderColor::all(if on { GOLD } else { BORDER_SOFT }),
                        TabButton(i),
                    ))
                    .with_children(|b| {
                        if let Some(handle) = atlas.get(icon_key) {
                            b.spawn(widgets::icon(handle, 18.0));
                        }
                        b.spawn(label(&fonts.bold, *name, 14.0, if on { Color::WHITE } else { TEXT_DIM }));
                    });
                }
            });

            // ── Body (fixed min-height so switching tabs doesn't jump the card) ──
            card.spawn(Node {
                flex_direction: FlexDirection::Column,
                min_height: Val::Px(310.0),
                row_gap: Val::Px(12.0),
                padding: UiRect::top(Val::Px(4.0)),
                ..default()
            })
            .with_children(|body| {
                if diagram == Diagram::DayNight {
                    day_night_bar(body, fonts, atlas);
                }
                for row in rows {
                    body.spawn(Node {
                        flex_direction: FlexDirection::Row,
                        align_items: AlignItems::Center,
                        column_gap: Val::Px(14.0),
                        ..default()
                    })
                    .with_children(|r| {
                        // Left gutter: chips (keycaps / icons), fixed width so titles line up.
                        r.spawn(Node {
                            min_width: Val::Px(118.0),
                            flex_direction: FlexDirection::Row,
                            flex_wrap: FlexWrap::Wrap,
                            align_items: AlignItems::Center,
                            column_gap: Val::Px(4.0),
                            row_gap: Val::Px(4.0),
                            ..default()
                        })
                        .with_children(|gutter| {
                            for chip in row.chips {
                                match chip {
                                    Key(k) => {
                                        gutter.spawn(keycap(&fonts.bold, k));
                                    }
                                    Icon(key) => {
                                        if let Some(handle) = atlas.get(key) {
                                            gutter.spawn(widgets::icon(handle, 24.0));
                                        }
                                    }
                                }
                            }
                        });
                        // Right: title + description.
                        r.spawn(Node {
                            flex_direction: FlexDirection::Column,
                            row_gap: Val::Px(1.0),
                            ..default()
                        })
                        .with_children(|tc| {
                            tc.spawn(label(&fonts.semibold, row.title, 14.5, TEXT));
                            tc.spawn(label(&fonts.regular, row.desc, 12.5, TEXT_DIM));
                        });
                    });
                }
                if diagram == Diagram::HpBar {
                    hp_bar(body, fonts, atlas);
                }
            });

            // ── Footer ──
            card.spawn(label(&fonts.regular, "Press H or Esc to close  \u{00b7}  click a tab to switch", 11.0, GREY));
        });
    });
}

/// The day → night loop bar (Survival tab): a warm "day" segment and a deep-blue "night" one.
fn day_night_bar(body: &mut RelatedSpawnerCommands<ChildOf>, fonts: &UiFonts, atlas: &IconAtlas) {
    body.spawn((
        Node {
            width: Val::Percent(100.0),
            height: Val::Px(36.0),
            flex_direction: FlexDirection::Row,
            border: border(1.0),
            border_radius: radius(R_CARD),
            overflow: Overflow::clip(),
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
            if let Some(handle) = atlas.get("sym:sun") {
                d.spawn(widgets::icon(handle, 18.0));
            }
            d.spawn(label(&fonts.bold, "DAY \u{2014} prepare", 12.0, rgb(48, 34, 10)));
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
            BackgroundColor(rgb(28, 38, 72)),
        ))
        .with_children(|n| {
            if let Some(handle) = atlas.get("buff:power") {
                n.spawn(widgets::icon(handle, 16.0));
            }
            n.spawn(label(&fonts.bold, "NIGHT \u{2014} defend", 12.0, rgb(210, 220, 240)));
        });
    });
}

/// A sample HP bar (Combat tab) showing the health track players watch in the heat of battle.
fn hp_bar(body: &mut RelatedSpawnerCommands<ChildOf>, fonts: &UiFonts, atlas: &IconAtlas) {
    body.spawn(Node {
        flex_direction: FlexDirection::Row,
        align_items: AlignItems::Center,
        column_gap: Val::Px(10.0),
        padding: UiRect::top(Val::Px(2.0)),
        ..default()
    })
    .with_children(|hp| {
        if let Some(handle) = atlas.get("sym:hp") {
            hp.spawn(widgets::icon(handle, 18.0));
        }
        hp.spawn((
            Node {
                width: Val::Px(240.0),
                height: Val::Px(14.0),
                border: border(1.0),
                border_radius: radius(R_SLOT),
                overflow: Overflow::clip(),
                ..default()
            },
            BackgroundColor(SLOT_BG),
            BorderColor::all(SLOT_BORDER),
        ))
        .with_children(|track| {
            track.spawn((
                Node { width: Val::Percent(68.0), height: Val::Percent(100.0), ..default() },
                widgets::vgrad(HP_TOP, HP_BOT),
            ));
        });
        hp.spawn(label(&fonts.regular, "Health \u{2014} eat food (Q) to recover", 12.5, TEXT_DIM));
    });
}
