//! **Tutorial / "How to Play" panel** — a tabbed help screen reachable any time with **H**
//! while playing, and from the start menu's **HOW TO PLAY** button (so new players actually
//! find it before their first night). In-game it's the `Modal::Tutorial` sub-state, reusing
//! the freeze gate for free; on the start screen (where `Modal` doesn't exist) the same panel
//! is spawned/despawned directly. `Esc`/`H`/the header ✕ close it in both contexts.
//!
//! Five tabs — **Basics / Combat / Stronghold / Economy / Survival** — in the medieval chrome
//! (linen + gold hairline frame, Cinzel header, tinted game-icons). The **Stronghold** tab
//! explains the town-building RTS loop: rescue villagers → build on plots → food grows the
//! population → workers gather → militia and walls hold it at night. Switching a tab rebuilds
//! the panel in place, the same despawn-and-rebuild pattern the satchel uses.

use bevy::ecs::relationship::RelatedSpawnerCommands;
use bevy::prelude::*;
use bevy::ui::FocusPolicy;

use crate::audio::AudioCue;
use crate::game_state::{AppState, Modal};
use crate::ui::anim::{anim, AnimKind};
use crate::ui::focus::{FocusActivate, Focusable};
use crate::ui::fonts::{label, UiFonts};
use crate::ui::texture::UiTextures;
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
            .add_systems(Update, open_tutorial.run_if(in_state(Modal::None)))
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

/// An optional illustrative mini-diagram for a tab.
#[derive(Clone, Copy, PartialEq)]
enum Diagram {
    None,
    /// Combat: a sample HP bar (rendered after the rows).
    HpBar,
    /// Survival: the day → night loop bar (rendered at the top).
    DayNight,
    /// Stronghold: the rescue → build → feed → defend town loop (rendered at the top).
    TownLoop,
}

/// Tab labels + their icon key (tinted game-icons via [`IconAtlas`]).
const TAB_NAMES: [(&str, &str); 5] = [
    ("Basics", "buff:haste"),
    ("Combat", "hero_dmg_1"),
    ("Stronghold", "def_reinforce"),
    ("Economy", "sym:gold"),
    ("Survival", "sym:sun"),
];

use Chip::{Icon, Key};

const BASICS: &[Row] = &[
    Row { chips: &[Key("W"), Key("A"), Key("S"), Key("D")], title: "Move", desc: "Walk your knight around the island." },
    Row { chips: &[Key("`")], title: "Camera", desc: "Toggle free fly-cam \u{2194} follow-cam. Hold RMB in fly-cam to look around." },
    Row { chips: &[Key("E")], title: "Interact", desc: "One key for everything nearby \u{2014} keep, merchant, build plot, war bell. A prompt names it." },
    Row { chips: &[Key("F")], title: "Loot & rescue", desc: "Open chests, forage plants and free caged villagers \u{2014} they walk home and join your town." },
    Row { chips: &[Key("I")], title: "Satchel", desc: "Open your bag to eat, equip and pin quick-slot items." },
    Row { chips: &[Key("R")], title: "Recruit", desc: "Rally nearby villagers to fight at your side." },
    Row { chips: &[Key("H")], title: "Help", desc: "Open this guide any time." },
];

const COMBAT: &[Row] = &[
    Row { chips: &[Key("LMB")], title: "Attack", desc: "Swing your weapon. Levels, gear and crits all raise your damage." },
    Row { chips: &[Key("RMB")], title: "Block", desc: "Raise your shield to cut incoming damage \u{2014} it drains stamina, so let it recover." },
    Row { chips: &[Icon("hero_dmg_1")], title: "Know your enemy", desc: "Grunts hit hard, scouts run fast, berserkers hit harder, shamans lob bolts from range." },
    Row { chips: &[Icon("hero_hp_1")], title: "Stay alive", desc: "Watch your HP and eat food (Q) to heal between fights." },
];

const STRONGHOLD: &[Row] = &[
    Row { chips: &[Key("F")], title: "Rescue villagers", desc: "Caged villagers are scattered across the island. Free them and they walk home to your town." },
    Row { chips: &[Key("E")], title: "Build on plots", desc: "Stand on an empty plot by the castle: houses raise the population cap, farms grow food, woodcutter yards and miner camps gather wood and stone." },
    Row { chips: &[Icon("stat:food")], title: "Feed the town", desc: "Villagers eat from the larder. A food surplus draws in new settlers; famine drives them off." },
    Row { chips: &[Icon("stat:wood")], title: "Workers", desc: "Villagers staff buildings on their own \u{2014} woodcutters fell real trees, miners cart ore, all straight into your stores." },
    Row { chips: &[Key("R")], title: "Militia", desc: "Rallied villagers and posted guards fight the raiders beside you." },
    Row { chips: &[Icon("def_walls")], title: "Hold it at night", desc: "Night raiders split off to torch your buildings. Walls and guards protect them; damage mends by day." },
];

const ECONOMY: &[Row] = &[
    Row { chips: &[Icon("sym:gold")], title: "Gold", desc: "Dropped by kills and chests. Spends at the merchant and the War Table." },
    Row { chips: &[Icon("sym:stone")], title: "Stone", desc: "Mine ore veins with your attack \u{2014} pays for walls, towers and buildings." },
    Row { chips: &[Icon("stat:wood")], title: "Wood", desc: "Chop trees yourself or let your woodcutters haul it. Houses and farms need it." },
    Row { chips: &[Icon("def_reinforce")], title: "War Table", desc: "Press E at the keep: four branches \u{2014} Prosperity, Bulwark, Champion, Armoury." },
    Row { chips: &[Icon("branch:arsenal")], title: "Merchant", desc: "Press E at the stall to trade for weapons, armor and potions." },
    Row { chips: &[Key("Q"), Key("Z"), Key("X"), Key("C")], title: "Quick-bar", desc: "Q eats food \u{00b7} Z resist \u{00b7} X power \u{00b7} C haste. Pin items from the satchel." },
];

const SURVIVAL: &[Row] = &[
    Row { chips: &[Icon("sym:sun")], title: "Day & night", desc: "By day you loot, build and prepare; at dusk the orks come." },
    Row { chips: &[Icon("def_reinforce")], title: "Defend the keep", desc: "If the keep's HP hits zero the run ends. Walls and towers buy you time." },
    Row { chips: &[Key("E")], title: "Ring the bell", desc: "Done preparing? Ring the war bell to call the night early." },
    Row { chips: &[Icon("buff:power")], title: "Succession", desc: "When your hero falls an heir takes up the blade. Run out of heirs and the run ends." },
    Row { chips: &[Icon("branch:economy")], title: "Five biomes", desc: "Forest, desert, snow, swamp and rock ring the island, each with its own bounty and beasts." },
];

/// Rows + diagram for a tab index.
fn tab_content(tab: usize) -> (&'static [Row], Diagram) {
    match tab {
        1 => (COMBAT, Diagram::HpBar),
        2 => (STRONGHOLD, Diagram::TownLoop),
        3 => (ECONOMY, Diagram::None),
        4 => (SURVIVAL, Diagram::DayNight),
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
        children![label(font, k, 12.0, rgb(240, 226, 192))],
    )
}

/// Build (or rebuild) the whole tutorial panel for the given active `tab`.
fn build_panel(
    commands: &mut Commands,
    tab: usize,
    fonts: &UiFonts,
    atlas: &IconAtlas,
    tex: &UiTextures,
) {
    let (rows, diagram) = tab_content(tab);

    // `FocusPolicy::Block` so the scrim swallows pointer picks — on the start screen the menu
    // buttons sit *under* it and must not stay clickable through the backdrop.
    commands.spawn((widgets::scrim(60), FocusPolicy::Block, TutorialUi)).with_children(|root| {
        root.spawn((
            Node {
                flex_direction: FlexDirection::Column,
                min_width: Val::Px(620.0),
                max_width: Val::Px(660.0),
                row_gap: Val::Px(12.0),
                padding: UiRect::axes(Val::Px(28.0), Val::Px(22.0)),
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
                padding: UiRect::bottom(Val::Px(8.0)),
                border: UiRect::bottom(Val::Px(1.0)),
                ..default()
            })
            .insert(BorderColor::all(BORDER_SOFT))
            .with_children(|h| {
                h.spawn(label(&fonts.display, "HOW TO PLAY", 16.0, GOLD));
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
                column_gap: Val::Px(6.0),
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
                            column_gap: Val::Px(7.0),
                            padding: UiRect::axes(Val::Px(13.0), Val::Px(8.0)),
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
                            b.spawn(widgets::icon_tinted(entry, 15.0, if on { INK } else { GOLD }));
                        }
                        b.spawn(label(&fonts.bold, *name, 13.0, if on { INK } else { TEXT_DIM }));
                    });
                }
            });

            // ── Body (fixed min-height so switching tabs doesn't jump the card) ──
            card.spawn(Node {
                flex_direction: FlexDirection::Column,
                min_height: Val::Px(330.0),
                row_gap: Val::Px(11.0),
                padding: UiRect::top(Val::Px(4.0)),
                ..default()
            })
            .with_children(|body| {
                match diagram {
                    Diagram::DayNight => day_night_bar(body, fonts, atlas),
                    Diagram::TownLoop => town_loop(body, fonts, atlas),
                    _ => {}
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
                            min_width: Val::Px(112.0),
                            flex_direction: FlexDirection::Row,
                            flex_wrap: FlexWrap::Wrap,
                            align_items: AlignItems::Center,
                            column_gap: Val::Px(4.0),
                            row_gap: Val::Px(4.0),
                            flex_shrink: 0.0,
                            ..default()
                        })
                        .with_children(|gutter| {
                            for chip in row.chips {
                                match chip {
                                    Key(k) => {
                                        gutter.spawn(keycap(&fonts.bold, k));
                                    }
                                    Icon(key) => {
                                        if let Some(entry) = atlas.get_tintable(key) {
                                            gutter.spawn(widgets::icon_tinted(entry, 22.0, GOLD));
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
                            tc.spawn(label(&fonts.semibold, row.title, 14.0, TEXT));
                            tc.spawn(label(&fonts.regular, row.desc, 12.5, TEXT_DIM));
                        });
                    });
                }
                if diagram == Diagram::HpBar {
                    hp_bar(body, fonts, atlas);
                }
            });

            // ── Footer ──
            card.spawn(label(&fonts.regular, "Click a tab or use \u{2190} \u{2192} + Enter to switch", 11.0, GREY));
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
            if let Some(entry) = atlas.get_tintable("sym:sun") {
                d.spawn(widgets::icon_tinted(entry, 18.0, rgb(74, 52, 16)));
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
            BackgroundColor(rgb(24, 28, 52)),
        ))
        .with_children(|n| {
            if let Some(entry) = atlas.get_tintable("buff:power") {
                n.spawn(widgets::icon_tinted(entry, 15.0, rgb(176, 188, 220)));
            }
            n.spawn(label(&fonts.bold, "NIGHT \u{2014} defend", 12.0, rgb(204, 214, 238)));
        });
    });
}

/// The Stronghold tab's loop strip: rescue → build → feed → defend, as framed medallions.
fn town_loop(body: &mut RelatedSpawnerCommands<ChildOf>, fonts: &UiFonts, atlas: &IconAtlas) {
    const STEPS: [(&str, &str); 4] = [
        ("sym:lock", "RESCUE"),
        ("stat:pop", "BUILD"),
        ("stat:food", "FEED"),
        ("def_walls", "DEFEND"),
    ];
    body.spawn((
        Node {
            width: Val::Percent(100.0),
            flex_direction: FlexDirection::Row,
            align_items: AlignItems::Center,
            justify_content: JustifyContent::Center,
            column_gap: Val::Px(14.0),
            padding: UiRect::axes(Val::Px(10.0), Val::Px(8.0)),
            border: border(1.0),
            border_radius: radius(R_CARD),
            ..default()
        },
        BackgroundColor(rgba(146, 122, 86, 0.08)),
        BorderColor::all(BORDER_SOFT),
    ))
    .with_children(|strip| {
        for (i, (icon_key, name)) in STEPS.iter().enumerate() {
            if i > 0 {
                strip.spawn((
                    Node { margin: UiRect::bottom(Val::Px(14.0)), ..default() },
                    children![label(&fonts.bold, "\u{2192}", 15.0, rgba(224, 168, 74, 0.65))],
                ));
            }
            strip
                .spawn(Node {
                    flex_direction: FlexDirection::Column,
                    align_items: AlignItems::Center,
                    row_gap: Val::Px(4.0),
                    ..default()
                })
                .with_children(|step| {
                    widgets::medallion(
                        step,
                        atlas.get_tintable(icon_key),
                        34.0,
                        rgba(224, 168, 74, 0.5),
                        rgba(146, 122, 86, 0.18),
                        GOLD,
                    );
                    step.spawn(label(&fonts.bold, *name, 10.0, TEXT_DIM));
                });
        }
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
        if let Some(entry) = atlas.get_tintable("hero_hp_1") {
            hp.spawn(widgets::icon_tinted(entry, 18.0, RED));
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
