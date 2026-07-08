//! **Contextual interaction** — ports the 3js game's single-key `E` interact. Instead of global
//! hotkeys, the player walks up to a thing and presses **E**: near the **keep** → upgrades, near the
//! **merchant stall** → shop, near the **war bell** (prep only) → ring in the night, right after a
//! villager's jab → **talk back** (fires the offered comeback chain). The nearest
//! in-range interactable wins (the keep and bell zones overlap), and a screen-space "E" prompt names
//! it. Proximity only — no facing check, matching the original.
//!
//! Treasure **chests** join this E resolver too: near an unopened chest the prompt shows
//! **E "Open chest"** and a press emits `chest::OpenChest` (the actual loot + lid swing stays in
//! `chest.rs`). Other verbs keep their own keys: **F** forage/rescue (`verbs`/`villagers`),
//! **R** recruit (`villagers`), **I** satchel, **Q/Y/T** quick-use (`inventory`).

use bevy::ecs::system::SystemParam;
use bevy::prelude::*;

use crate::audio::AudioCue;
use crate::combat_fx::HitFeedback;
use crate::game_state::Modal;
use crate::landmarks::{Landmark, LandmarkInteract};
use crate::player::HeroState;
use crate::siege::{GamePhase, Siege};
use crate::ui::fonts::{label, UiFonts};
use crate::ui::theme::*;
use crate::ui::widgets::{self, border};
use crate::game_state::SimAppExt;

/// Interaction ranges (world units), from the 3js `cityPlan`/`Shop` constants.
const KEEP_DIST: f32 = 4.2;
const BELL_DIST: f32 = 4.2;
const SHOP_DIST: f32 = 3.5;
/// Talk-back range: a bit over the villager-chatter trigger (`npc::NEAR_DIST` 7.0) so stepping
/// back half a pace during the jab doesn't lose the prompt.
const TALK_DIST: f32 = 8.0;
/// Stand this close to a discovered landmark to get its `[E]` prompt (challenge trial or pray).
const LANDMARK_DIST: f32 = 3.6;

#[derive(Clone, Copy, PartialEq, Eq)]
enum InteractKind {
    Upgrades,
    Shop,
    WarBell,
    /// A villager jabbed at the hero and a comeback is on offer (`director::OfferedReply`).
    TalkBack,
    /// Standing next to an unopened treasure chest — **E** opens it (the resolver emits
    /// `chest::OpenChest`; `chest.rs` does the actual loot + lid swing).
    Chest,
    /// Standing at the unbroken gate of Gnashfang Hold — **E** breaks it open and wakes the
    /// garrison + Warlord (`ork_fortress::breach_gate`). The game's win condition.
    BreachGate,
    /// At a discovered landmark whose signature gear is still SEALED — **E** begins its
    /// Hold-the-Rune trial (`landmarks::start_rune_trial` via [`LandmarkInteract`]).
    TrialChallenge,
    /// At a landmark whose gear is already won (or a vignette with none) — **E** prays at its
    /// shrine for a timed buff (`landmarks::shrine`).
    Shrine,
}
impl InteractKind {
    fn prompt(self) -> &'static str {
        match self {
            InteractKind::Upgrades => "Upgrades",
            InteractKind::Shop => "Shop",
            InteractKind::WarBell => "Ring the bell",
            InteractKind::TalkBack => "Talk back",
            InteractKind::Chest => "Open chest",
            InteractKind::BreachGate => "Break the gate",
            InteractKind::TrialChallenge => "Challenge the guardians",
            InteractKind::Shrine => "Pray at the shrine",
        }
    }
    /// The keycap shown on the prompt chip. Every contextual action — chests included — is on **E**.
    fn key_label(self) -> &'static str {
        "E"
    }
}

/// The interactable the hero is currently in range of (nearest wins), or `None`.
#[derive(Resource, Default)]
struct ActiveInteraction {
    kind: Option<InteractKind>,
    /// A player-facing "can't act yet" reason for the active interaction (red prompt + detail),
    /// or `None` when actionable. No current interaction gates on cost — building's afford check
    /// moved to the HUD-button build mode (`town::build_place`) — so this stays `None` today, but
    /// the prompt machinery is kept for the next gated interaction.
    blocked: Option<String>,
}

/// The chest read + open-request, bundled so `drive_interaction` stays under Bevy's 16-param cap.
#[derive(SystemParam)]
struct ChestIo<'w, 's> {
    chests: Query<'w, 's, (Entity, &'static crate::chest::Chest, &'static Transform)>,
    open: MessageWriter<'w, crate::chest::OpenChest>,
}

/// Landmark read + interact-request, bundled to keep `drive_interaction` under the param cap.
#[derive(SystemParam)]
struct LandmarkIo<'w, 's> {
    landmarks: Query<'w, 's, (Entity, &'static Landmark, &'static Transform)>,
    interact: MessageWriter<'w, LandmarkInteract>,
    /// The in-flight Rune-Trial — so the `[E] Challenge the guardians` prompt disappears once the
    /// defence has started (you can't begin a second trial mid-fight).
    trial: Res<'w, crate::landmarks::RuneTrial>,
}

/// The centred hint row. Holds up to three sibling chips — `[B] Build`, the contextual `[E] …`, and
/// `[K] Follow me` — each toggled independently by [`update_prompt`]; the row shows if any do. Near
/// the castle all three can appear together; out in the field it collapses to just the `E` chip.
#[derive(Component)]
struct PromptRoot;
/// The bordered contextual chip (`[E] …`) — recoloured red when the active interaction is blocked.
#[derive(Component)]
struct PromptChip;
#[derive(Component)]
struct PromptLabel;
/// The keycap glyph inside the contextual chip — "E" for every contextual action.
#[derive(Component)]
struct PromptKey;
/// The always-"[B] Build" chip (shown near the castle in Prep, when not already building).
#[derive(Component)]
struct BuildHintChip;
/// The "[K] Follow me / Stand down" chip (shown near the castle); its label flips on rally state.
#[derive(Component)]
struct MusterHintChip;
#[derive(Component)]
struct MusterHintLabel;

pub struct InteractionPlugin;

impl Plugin for InteractionPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<ActiveInteraction>()
            .add_systems(Startup, setup_prompt)
            .add_sim_systems(drive_interaction)
            .add_systems(Update, update_prompt);
    }
}

/// The merchant stall's world XZ — `castle::gate_centers()[0] + (2.5, -5.0)`, matching where
/// `villagers` builds the market.
fn shop_anchor() -> Vec2 {
    crate::castle::gate_centers()[0] + Vec2::new(2.5, -5.0)
}

#[allow(clippy::too_many_arguments)]
fn drive_interaction(
    keys: Res<ButtonInput<KeyCode>>,
    hero: Res<HeroState>,
    build_mode: Res<crate::town::BuildMode>,
    mut siege: ResMut<Siege>,
    mut active: ResMut<ActiveInteraction>,
    mut next_modal: ResMut<NextState<Modal>>,
    mut cues: MessageWriter<AudioCue>,
    mut feedback: ResMut<HitFeedback>,
    mut bell_ring: ResMut<crate::castle::BellRing>,
    time: Res<Time>,
    mut offered: ResMut<crate::audio::director::OfferedReply>,
    mut voices: ResMut<crate::audio::director::VoiceManager>,
    mut chest_io: ChestIo,
    mut landmark_io: LandmarkIo,
    assault: Res<crate::ork_fortress::AssaultState>,
    mut breach: MessageWriter<crate::ork_fortress::BreachGate>,
) {
    let p = hero.pos;

    // While placing buildings, the build palette owns E + its own on-spot prompt — don't also
    // surface keep/shop/bell/chest prompts (they'd fight `town::build_place` for the key).
    if build_mode.active {
        active.kind = None;
        active.blocked = None;
        return;
    }

    // (kind, position, radius, available?)
    let mut candidates: Vec<(InteractKind, Vec2, f32, bool)> = vec![
        (InteractKind::Upgrades, Vec2::ZERO, KEEP_DIST, true),
        (InteractKind::Shop, shop_anchor(), SHOP_DIST, true),
        (InteractKind::WarBell, crate::castle::BELL_POS, BELL_DIST, siege.phase == GamePhase::Prep),
    ];
    // A villager jab on offer: the prompt anchors where the speaker stood (expiry is handled by
    // `tick_chains`; here we only range-gate it).
    if let Some(offer) = offered.0 {
        let at = offer.pos.map_or(p, |v| Vec2::new(v.x, v.z));
        candidates.push((InteractKind::TalkBack, at, TALK_DIST, true));
    }
    // Nearest unopened treasure chest in reach — joins the E nearest-wins arbitration. We remember
    // the entity so an E press opens exactly the one the prompt named (chest.rs does the open).
    let mut nearest_chest: Option<(Entity, Vec2, f32)> = None;
    for (e, chest, tf) in &chest_io.chests {
        if chest.opened {
            continue;
        }
        let at = Vec2::new(tf.translation.x, tf.translation.z);
        let d = p.distance(at);
        if d < crate::chest::CHEST_INTERACT_DIST && nearest_chest.map_or(true, |(_, _, bd)| d < bd) {
            nearest_chest = Some((e, at, d));
        }
    }
    if let Some((_, at, _)) = nearest_chest {
        candidates.push((InteractKind::Chest, at, crate::chest::CHEST_INTERACT_DIST, true));
    }
    // The gate of Gnashfang Hold — available until it's broken (the win-condition raid).
    candidates.push((
        InteractKind::BreachGate,
        crate::ork_fortress::GATE,
        crate::ork_fortress::BREACH_RANGE,
        !assault.breached,
    ));
    // Nearest discovered landmark in reach: a sealed-gear one offers its trial, an already-claimed
    // (or gear-less vignette) one offers its shrine. Remember the entity so the E press targets it.
    // While a trial is actually running, drop the landmark prompt entirely — the hero is mid-defence
    // and the `[E] Challenge the guardians` chip would be a no-op (a second trial can't start).
    let trial_running = landmark_io.trial.is_active();
    let mut nearest_landmark: Option<(Entity, Vec2, f32, bool)> = None; // (e, at, dist, sealed)
    for (e, lm, tf) in &landmark_io.landmarks {
        if !lm.is_discovered() || trial_running {
            continue;
        }
        let at = Vec2::new(tf.translation.x, tf.translation.z);
        let d = p.distance(at);
        if d < LANDMARK_DIST && nearest_landmark.map_or(true, |(_, _, bd, _)| d < bd) {
            let sealed = lm.has_gear() && !lm.is_gear_claimed();
            nearest_landmark = Some((e, at, d, sealed));
        }
    }
    if let Some((_, at, _, sealed)) = nearest_landmark {
        let kind = if sealed { InteractKind::TrialChallenge } else { InteractKind::Shrine };
        candidates.push((kind, at, LANDMARK_DIST, true));
    }

    // Pick the nearest in-range, available interactable.
    let mut best: Option<(InteractKind, f32)> = None;
    for (kind, pos, radius, ok) in candidates {
        if !ok {
            continue;
        }
        let d = p.distance(pos);
        if d < radius && best.map_or(true, |(_, bd)| d < bd) {
            best = Some((kind, d));
        }
    }
    active.kind = best.map(|(k, _)| k);
    active.blocked = None; // no interaction carries a cost gate now (building moved to build mode)

    if let (true, Some(kind)) = (keys.just_pressed(KeyCode::KeyE), active.kind) {
        match kind {
            InteractKind::Upgrades => next_modal.set(Modal::UpgradeTree),
            InteractKind::Shop => next_modal.set(Modal::Shop),
            InteractKind::WarBell => {
                siege.request_prep_skip();
                cues.write(AudioCue::WarBell);
                bell_ring.0 = Some(time.elapsed_secs()); // rock the bronze (castle::swing_bell)
                feedback.trauma = (feedback.trauma + 0.3).min(1.0);
            }
            InteractKind::TalkBack => {
                if let Some(offer) = offered.0.take() {
                    voices.accept_reply(offer, time.elapsed_secs());
                }
            }
            // Ask chest.rs to open exactly the chest the resolver picked as nearest-in-range.
            InteractKind::Chest => {
                if let Some((e, _, _)) = nearest_chest {
                    chest_io.open.write(crate::chest::OpenChest(e));
                }
            }
            // Break into the Hold — `ork_fortress::breach_gate` wakes the garrison + Warlord.
            InteractKind::BreachGate => {
                breach.write(crate::ork_fortress::BreachGate);
            }
            // Landmark — fire the request at exactly the landmark the resolver named; `landmarks.rs`
            // decides trial-vs-shrine from its state.
            InteractKind::TrialChallenge | InteractKind::Shrine => {
                if let Some((e, _, _, _)) = nearest_landmark {
                    landmark_io.interact.write(LandmarkInteract(e));
                }
            }
        }
    }
}

/// Default chip border (gold hairline) — `update_prompt` flips to `RED_BORDER` when blocked.
const PROMPT_BORDER: Color = rgba(255, 213, 140, 0.5);
/// Keycap glyph foreground (cream on the dark keycap).
const KEYCAP_FG: Color = rgba(255, 224, 170, 0.92);

/// The bordered chip box — a flex row holding a keycap + a label. Starts hidden; `update_prompt`
/// toggles each chip's `display`.
fn chip_node() -> Node {
    Node {
        flex_direction: FlexDirection::Row,
        align_items: AlignItems::Center,
        column_gap: Val::Px(8.0),
        padding: UiRect::axes(Val::Px(12.0), Val::Px(7.0)),
        border: border(1.0),
        border_radius: radius(R_CARD),
        display: Display::None,
        ..default()
    }
}

/// The little keycap box around a single glyph.
fn keycap_node() -> Node {
    Node {
        padding: UiRect::axes(Val::Px(8.0), Val::Px(3.0)),
        border: border(1.0),
        border_radius: radius(5.0),
        ..default()
    }
}

fn setup_prompt(mut commands: Commands, fonts: Res<UiFonts>) {
    // A full-width centring band so the chips can grow with their text and stay centred. Holds the
    // three sibling chips B · E · K (each individually toggled); the row itself shows if any do.
    commands
        .spawn((
            PromptRoot,
            Node {
                position_type: PositionType::Absolute,
                bottom: Val::Px(110.0),
                left: Val::Px(0.0),
                right: Val::Px(0.0),
                display: Display::None,
                flex_direction: FlexDirection::Row,
                justify_content: JustifyContent::Center,
                column_gap: Val::Px(10.0),
                ..default()
            },
            bevy::ui::FocusPolicy::Pass,
        ))
        .with_children(|root| {
            // [B] Build — near the castle in Prep (when not already building).
            root.spawn((
                BuildHintChip,
                chip_node(),
                BackgroundColor(PANEL_HUD),
                BorderColor::all(PROMPT_BORDER),
                shadow_hud(),
            ))
            .with_children(|p| {
                p.spawn((keycap_node(), widgets::keycap_paint(), children![label(
                    &fonts.extrabold,
                    "B",
                    12.0,
                    KEYCAP_FG
                )]));
                p.spawn(label(&fonts.bold, "Build", 14.0, GOLD));
            });
            // [E] <contextual> — the nearest-wins interactable (Upgrades/Shop/Bell/Open).
            root.spawn((
                PromptChip,
                chip_node(),
                BackgroundColor(PANEL_HUD),
                BorderColor::all(PROMPT_BORDER),
                shadow_hud(),
            ))
            .with_children(|p| {
                p.spawn((keycap_node(), widgets::keycap_paint()))
                    .with_children(|k| {
                        k.spawn((label(&fonts.extrabold, "E", 12.0, KEYCAP_FG), PromptKey));
                    });
                p.spawn((label(&fonts.bold, "Upgrades", 14.0, GOLD), PromptLabel));
            });
            // [K] Follow me / Stand down — near the castle (label flips on rally state).
            root.spawn((
                MusterHintChip,
                chip_node(),
                BackgroundColor(PANEL_HUD),
                BorderColor::all(PROMPT_BORDER),
                shadow_hud(),
            ))
            .with_children(|p| {
                p.spawn((keycap_node(), widgets::keycap_paint(), children![label(
                    &fonts.extrabold,
                    "K",
                    12.0,
                    KEYCAP_FG
                )]));
                p.spawn((label(&fonts.bold, "Follow me", 14.0, GOLD), MusterHintLabel));
            });
        });
}

/// Drive the near-castle hint row. The contextual `[E]` chip shows the active interactable (only
/// while playing with no panel open); a *blocked* interaction recolours it red + appends the
/// shortfall. The `[B] Build` and `[K] Follow me / Stand down` chips show whenever the hero is near
/// the castle (the same town zone build mode uses) — B in Prep when not already building, K when the
/// town has anyone to rally — so the muster key is discoverable instead of invisible. The row itself
/// shows if any chip does.
#[allow(clippy::type_complexity, clippy::too_many_arguments)]
fn update_prompt(
    active: Res<ActiveInteraction>,
    modal: Option<Res<State<Modal>>>,
    build_mode: Res<crate::town::BuildMode>,
    siege: Option<Res<Siege>>,
    hero: Res<HeroState>,
    rallied_q: Query<(), With<crate::villagers::Rallied>>,
    townsfolk_q: Query<(), With<crate::villagers::Townsfolk>>,
    mut root_q: Query<
        &mut Node,
        (With<PromptRoot>, Without<PromptChip>, Without<BuildHintChip>, Without<MusterHintChip>),
    >,
    mut e_node_q: Query<
        &mut Node,
        (With<PromptChip>, Without<PromptRoot>, Without<BuildHintChip>, Without<MusterHintChip>),
    >,
    mut b_node_q: Query<
        &mut Node,
        (With<BuildHintChip>, Without<PromptRoot>, Without<PromptChip>, Without<MusterHintChip>),
    >,
    mut k_node_q: Query<
        &mut Node,
        (With<MusterHintChip>, Without<PromptRoot>, Without<PromptChip>, Without<BuildHintChip>),
    >,
    mut chip_q: Query<(&mut BorderColor, &mut BackgroundColor), With<PromptChip>>,
    mut label_q: Query<(&mut Text, &mut TextColor), (With<PromptLabel>, Without<PromptKey>, Without<MusterHintLabel>)>,
    mut key_q: Query<&mut Text, (With<PromptKey>, Without<PromptLabel>, Without<MusterHintLabel>)>,
    mut muster_label_q: Query<&mut Text, (With<MusterHintLabel>, Without<PromptKey>, Without<PromptLabel>)>,
) {
    let playing = modal.map_or(false, |m| *m.get() == Modal::None);
    let near = crate::town::in_town(hero.pos);
    let prep = siege.map_or(true, |s| s.phase == GamePhase::Prep);

    let kind = if playing { active.kind } else { None };
    let b_show = playing && near && prep && !build_mode.active;
    let any_rallied = !rallied_q.is_empty();
    // K: anywhere near the castle with townsfolk to lead (the K action itself works game-wide; this
    // is just the discoverability hint, parked by your settlement where the war party lives).
    let k_show = playing && near && hero.alive && !townsfolk_q.is_empty();

    let disp = |show: bool| if show { Display::Flex } else { Display::None };
    if let Ok(mut n) = e_node_q.single_mut() {
        n.display = disp(kind.is_some());
    }
    if let Ok(mut n) = b_node_q.single_mut() {
        n.display = disp(b_show);
    }
    if let Ok(mut n) = k_node_q.single_mut() {
        n.display = disp(k_show);
    }
    if let Ok(mut node) = root_q.single_mut() {
        node.display = disp(kind.is_some() || b_show || k_show);
    }

    if k_show {
        if let Ok(mut t) = muster_label_q.single_mut() {
            let want = if any_rallied { "Stand down" } else { "Follow me" };
            if t.as_str() != want {
                **t = want.to_string();
            }
        }
    }

    let Some(k) = kind else { return };
    let blocked = active.blocked.as_deref();

    // Name the key on the chip (E for contextual, F for chest) — only rewrite on a change.
    if let Ok(mut kt) = key_q.single_mut() {
        if kt.as_str() != k.key_label() {
            **kt = k.key_label().to_string();
        }
    }

    if let Ok((mut t, mut col)) = label_q.single_mut() {
        let want = match blocked {
            Some(detail) => format!("{}  \u{2014}  {detail}", k.prompt()),
            None => k.prompt().to_string(),
        };
        if t.as_str() != want {
            **t = want;
        }
        col.0 = if blocked.is_some() { RED_HI } else { GOLD };
    }
    if let Ok((mut bc, mut bg)) = chip_q.single_mut() {
        if blocked.is_some() {
            *bc = BorderColor::all(RED_BORDER);
            bg.0 = rgba(64, 18, 18, 0.88); // red-tinted HUD chrome — reads "can't"
        } else {
            *bc = BorderColor::all(PROMPT_BORDER);
            bg.0 = PANEL_HUD;
        }
    }
}
