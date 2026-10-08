//! Env-only engine integration checks for the first three campaign cycles.
//!
//! `FOREST_CAMPAIGN_VERIFY=raid|defend|skip` drives real muster, build, bell, rescue,
//! worker assignment, purchases and wave spawning. Teleports and `Dying` markers are
//! scripted stimuli: this verifies wiring and outcomes, not combat balance or navigation.
//! Both prepared routes fund their purchases from ordinary kill rewards and dawn tithe.
//! Run with an isolated `XDG_DATA_HOME` because normal dawn autosaves still run.
//! `FOREST_CAMPAIGN_VERIFY_HEADLESS=1` runs the same ECS systems without a renderer.

use std::collections::HashSet;
use std::time::{Duration, Instant};

use bevy::input::InputSystems;
use bevy::prelude::*;
use bevy::time::TimeUpdateStrategy;
use tileworld_core::campaign::Objective;
use tileworld_core::town_store::BuildKind;

use crate::game_state::{AppState, Modal};
use crate::orks::{Ork, OrkVariant, WaveInvader};
use crate::player::{Hero, HeroState, PlayerRes};
use crate::quest::CampaignRes;
use crate::siege::{GamePhase, Siege};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Route { Raid, Defend, Skip }

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Step {
    Boot, StartRun, Ready, Rally, Rescue, Home, OpenBuild, SelectFarm,
    PlaceFarm, Feed, Bell(usize), Wave(usize), BuyDefense, ThirdPlan,
    Ritual, Finished,
}

#[derive(Resource)]
struct Verify {
    route: Route,
    step: Step,
    frames: u32,
    entered: u32,
    pending_key: Option<KeyCode>,
    camp: usize,
    initial_pop: u32,
    cleared: bool,
    wave_entities: HashSet<Entity>,
    wave_variants: Vec<OrkVariant>,
    wave_raid: bool,
    started: Instant,
    timeout: Duration,
}

impl Verify {
    fn advance(&mut self, step: Step) {
        info!("CAMPAIGN_VERIFY checkpoint {:?} -> {:?}", self.step, step);
        self.step = step;
        self.entered = self.frames;
    }
    fn age(&self) -> u32 { self.frames - self.entered }
    fn key(&mut self, key: KeyCode) { self.pending_key = Some(key); }
}

/// Called by main after the game's plugins have registered their resources.
pub fn install(app: &mut App) {
    let Ok(value) = std::env::var("FOREST_CAMPAIGN_VERIFY") else { return };
    let route = match value.as_str() {
        "raid" | "1" => Route::Raid,
        "defend" => Route::Defend,
        "skip" => Route::Skip,
        _ => panic!("FOREST_CAMPAIGN_VERIFY must be raid, defend or skip"),
    };
    let timeout = std::env::var("FOREST_CAMPAIGN_VERIFY_TIMEOUT")
        .ok().and_then(|v| v.parse::<u64>().ok()).unwrap_or(1200);
    app.insert_resource(Verify {
        route, step: Step::Boot, frames: 0, entered: 0, pending_key: None,
        camp: 0, initial_pop: 0, cleared: false,
        wave_entities: HashSet::new(), wave_variants: Vec::new(), wave_raid: false,
        started: Instant::now(), timeout: Duration::from_secs(timeout),
    })
    .insert_resource(crate::player::ScriptedHero)
    .insert_resource(TimeUpdateStrategy::ManualDuration(Duration::from_millis(100)))
    .add_systems(PreUpdate, inject_input.after(InputSystems))
    .add_systems(Last, observe);
}

fn inject_input(mut verify: ResMut<Verify>, mut keys: ResMut<ButtonInput<KeyCode>>,
    game: Option<ResMut<crate::siege::GameTime>>) {
    keys.reset_all();
    if let Some(key) = verify.pending_key.take() { keys.press(key); }
    // The world keeps its ordinary dt for workers/animation. Accelerate only the
    // siege timeline so waiting for spawn intervals does not dominate a render check.
    if !matches!(verify.step, Step::Boot | Step::StartRun | Step::Finished) {
        if let Some(mut game) = game { game.0 += 1.0; }
    }
}

fn observe(world: &mut World) {
    world.resource_scope(|world, mut v: Mut<Verify>| {
        if v.step == Step::Finished { return; }
        v.frames += 1;
        if v.frames > 8000 || v.started.elapsed() > v.timeout {
            fail(world, &mut v, "watchdog: no successful completion");
            return;
        }
        if let Err(reason) = advance(world, &mut v) { fail(world, &mut v, &reason); }
    });
}

fn fail(world: &mut World, v: &mut Verify, reason: &str) {
    error!("CAMPAIGN_VERIFY FAILED route={:?} step={:?} frames={} reason={reason}", v.route, v.step, v.frames);
    v.step = Step::Finished;
    world.write_message(AppExit::error());
}

fn advance(world: &mut World, v: &mut Verify) -> Result<(), String> {
    let state = *world.resource::<State<AppState>>().get();
    if v.step == Step::Boot {
        if !world.resource::<crate::biome::WorldReady>().0 { return Ok(()); }
        if state != AppState::StartScreen {
            world.resource_mut::<NextState<AppState>>().set(AppState::StartScreen);
        }
        v.advance(Step::StartRun);
        return Ok(());
    }
    if v.step == Step::StartRun {
        if state != AppState::StartScreen { return Ok(()); }
        world.resource_mut::<NextState<AppState>>().set(AppState::Playing);
        v.advance(Step::Ready);
        return Ok(());
    }
    if state != AppState::Playing {
        if v.step == Step::Ready { return Ok(()); }
        return Err(format!("unexpected app state {state:?}"));
    }
    if !world.resource::<State<Modal>>().get().eq(&Modal::None) {
        return Err("an unexpected modal interrupted the scenario".into());
    }
    match v.step {
        Step::Ready if v.age() >= 3 => {
            ensure(world.resource::<Siege>().phase == GamePhase::Prep, "fresh run is not in preparation")?;
            ensure(world.resource::<CampaignRes>().0.objective(0, false) == Objective::Rally, "fresh lessons did not reset")?;
            v.initial_pop = world.resource::<crate::town::TownRes>().0.population;
            ensure(world.resource::<crate::economy::Bank>().0.wood() >= 24.0, "fresh-run timber stipend missing")?;
            v.camp = crate::camps::intro_target().ok_or("no introductory camp")?;
            let centre = crate::camps::camp_centre(v.camp).ok_or("introductory camp has no centre")?;
            let roster: Vec<_> = world.query_filtered::<&Ork, Without<crate::dying::Dying>>()
                .iter(world).filter(|o| o.home().distance(centre) < 1.0).map(|o| o.variant).collect();
            ensure(roster.len() == 2 && roster.iter().all(|v| *v == OrkVariant::Scout), "introductory camp must contain two Scouts")?;
            // An overdue clock still cannot begin an untimed lesson automatically.
            world.resource_mut::<crate::siege::GameTime>().0 += 400.0;
            pin_hero(world, Vec2::new(-5.0, 5.0));
            if v.route == Route::Skip { ring(world, v, 0); }
            else { v.key(KeyCode::KeyK); v.advance(Step::Rally); }
        }
        Step::Rally => {
            ensure(world.resource::<Siege>().phase == GamePhase::Prep, "untimed day expired")?;
            if rallied(world) > 0 && world.resource::<CampaignRes>().0.rallied {
                ensure(world.resource::<CampaignRes>().0.objective(0, false) == Objective::Rescue, "rally did not reveal the rescue")?;
                let centre = crate::camps::camp_centre(v.camp).ok_or("missing rescue centre")?;
                pin_hero(world, centre + (-centre).normalize_or_zero() * 6.0);
                v.cleared = false;
                v.advance(Step::Rescue);
            }
        }
        Step::Rescue => {
            if !v.cleared && v.age() >= 3 {
                let seen = world.resource::<crate::villagers::RescuedCamps>().seen.get(v.camp).copied().unwrap_or(false);
                ensure(seen, "rescue clear happened before the camp was observed alive")?;
                clear_camp(world, v.camp)?;
                v.cleared = true;
            }
            if v.cleared && world.resource::<CampaignRes>().0.rescued {
                ensure(world.resource::<crate::villagers::RescuedCamps>().done.get(v.camp).copied().unwrap_or(false), "campaign rescue credited without a real cage rescue")?;
                ensure(world.resource::<crate::town::TownRes>().0.population >= v.initial_pop + crate::villagers::CAMP_RESCUE_POP, "rescue did not increase the population")?;
                let doors_open = world.query::<&crate::camps::CageDoor>().iter(world)
                    .any(|d| d.key == crate::camps::CageKey::Camp(v.camp) && d.open);
                ensure(doors_open, "rescued cage door stayed closed")?;
                pin_hero(world, Vec2::new(-5.0, 5.0));
                v.key(KeyCode::KeyK);
                v.advance(Step::Home);
            }
        }
        Step::Home if rallied(world) == 0 && world.resource::<CampaignRes>().0.returned => {
            v.key(KeyCode::KeyB);
            v.advance(Step::OpenBuild);
        }
        Step::OpenBuild if world.resource::<crate::town::BuildMode>().active => {
            v.key(KeyCode::ArrowRight);
            v.advance(Step::SelectFarm);
        }
        Step::SelectFarm => {
            let mode = world.resource::<crate::town::BuildMode>();
            if mode.kind() == crate::town::BuildType::Producer(BuildKind::Farm) && mode.target.is_some() {
                v.key(KeyCode::Enter);
                v.advance(Step::PlaceFarm);
            }
        }
        Step::PlaceFarm => {
            if world.resource::<crate::town::TownRes>().0.plots.iter().any(|p| p.is_built() && p.kind == Some(BuildKind::Farm)) {
                v.key(KeyCode::KeyB);
                v.advance(Step::Feed);
            }
        }
        Step::Feed => {
            let town = &world.resource::<crate::town::TownRes>().0;
            if town.food_rate() > 0.0 && world.resource::<CampaignRes>().0.farm_worked {
                ensure(world.query_filtered::<&crate::town::Worker, Without<crate::dying::Dying>>().iter(world).any(|w| w.at_post), "farm lesson passed without a posted worker")?;
                ensure(world.resource::<CampaignRes>().0.objective(0, false) == Objective::CallNight, "working farm did not reveal the bell")?;
                ring(world, v, 0);
            }
        }
        Step::Bell(night) => {
            let siege = world.resource::<Siege>();
            if siege.phase == GamePhase::Wave && siege.wave_index == night as i32 {
                v.wave_entities.clear();
                v.wave_variants.clear();
                v.wave_raid = crate::camps::raid_cleared(&world.resource::<CampaignRes>().0, night);
                v.advance(Step::Wave(night));
            } else if v.age() > 10 { return Err(format!("real E interaction did not start night {}", night + 1)); }
        }
        Step::Wave(night) => {
            collect_and_clear_wave(world, v);
            let siege = world.resource::<Siege>();
            if siege.phase == GamePhase::Prep && siege.wave_index == night as i32 {
                check_wave(world, v, night)?;
                ensure(world.resource::<CampaignRes>().0.objective(night + 1, false) != Objective::HoldKeep, "finished night retained combat advice")?;
                match night {
                    0 if v.route != Route::Skip => v.advance(Step::BuyDefense),
                    0 => ring(world, v, 1),
                    1 => v.advance(Step::ThirdPlan),
                    2 => {
                        ensure(!world.resource::<CampaignRes>().0.learning_day(3), "day four still has the teaching clock")?;
                        ensure(world.resource::<CampaignRes>().0.objective(3, false) == Objective::FreePlay, "three cycles did not release the player")?;
                        if v.route == Route::Skip {
                            let c = &world.resource::<CampaignRes>().0;
                            ensure(!c.rescued && !c.farm_worked && !c.defense_bought, "skip route accidentally completed preparations")?;
                        }
                        info!("CAMPAIGN_VERIFY SUCCESS route={:?} frames={} scripted combat outcomes; real rescue/work/build/bell/purchase/waves", v.route, v.frames);
                        v.advance(Step::Finished);
                        world.write_message(AppExit::Success);
                    }
                    _ => return Err("unexpected night number".into()),
                }
            }
        }
        Step::BuyDefense => {
            if world.resource::<crate::economy::Upgrades>().0.is_purchased("def_armor_1") {
                if world.resource::<CampaignRes>().0.defense_bought {
                    ensure(world.resource::<crate::economy::Defenses>().villager_arms_tier > 0, "purchase did not strengthen the militia")?;
                    ring(world, v, 1);
                }
            } else if world.resource::<PlayerRes>().0.gold >= tileworld_core::upgrade_store::node_by_id("def_armor_1").unwrap().cost() {
                ensure(purchase(world, "def_armor_1"), "real guard upgrade purchase failed")?;
            }
        }
        Step::ThirdPlan if v.age() >= 3 => {
            ensure(world.resource::<Siege>().phase == GamePhase::Prep, "third learning day expired")?;
            match v.route {
                Route::Skip => ring(world, v, 2),
                Route::Defend => {
                    if !world.resource::<crate::economy::Defenses>().keep_archers {
                        let cost = tileworld_core::upgrade_store::node_by_id("def_keep_archers").unwrap().cost();
                        if world.resource::<PlayerRes>().0.gold >= cost {
                            ensure(purchase(world, "def_keep_archers"), "real ranged purchase failed")?;
                        } else if v.age() > 120 {
                            return Err(format!("normal early income cannot fund the ranged alternative: {}g, need {cost}g", world.resource::<PlayerRes>().0.gold));
                        }
                    } else if world.resource::<CampaignRes>().0.ranged_prepared {
                        ensure(world.resource::<CampaignRes>().0.objective(2, false) == Objective::SurviveRitual, "ranged alternative did not satisfy the third preparation")?;
                        ring(world, v, 2);
                    }
                }
                Route::Raid => {
                    v.camp = crate::camps::raid_target(2).ok_or("no ritual raid target")?;
                    let centre = crate::camps::camp_centre(v.camp).ok_or("ritual target lacks centre")?;
                    pin_hero(world, centre + (-centre).normalize_or_zero() * 6.0);
                    v.cleared = false;
                    v.advance(Step::Ritual);
                }
            }
        }
        Step::Ritual => {
            if !v.cleared && v.age() >= 3 { clear_camp(world, v.camp)?; v.cleared = true; }
            if world.resource::<CampaignRes>().0.shaman_cleared {
                ensure(crate::camps::raid_cleared(&world.resource::<CampaignRes>().0, 2), "ritual lesson completed without same-day raid credit")?;
                ring(world, v, 2);
            }
        }
        _ => {}
    }
    Ok(())
}

fn ensure(condition: bool, message: &str) -> Result<(), String> {
    if condition { Ok(()) } else { Err(message.to_string()) }
}

fn pin_hero(world: &mut World, pos: Vec2) {
    let y = crate::worldmap::ground_at_world(pos.x, pos.y).unwrap_or(0.0);
    for (mut hero, mut tf) in world.query::<(&mut Hero, &mut Transform)>().iter_mut(world) {
        hero.pos = pos; hero.y = y; hero.moving = false; hero.moving_amt = 0.0;
        tf.translation = Vec3::new(pos.x, y, pos.y);
    }
    let mut state = world.resource_mut::<HeroState>();
    state.pos = pos; state.y = y;
}

fn rallied(world: &mut World) -> usize {
    world.query_filtered::<(), (With<crate::villagers::Rallied>, Without<crate::dying::Dying>)>().iter(world).count()
}

fn ring(world: &mut World, v: &mut Verify, night: usize) {
    pin_hero(world, crate::castle::BELL_POS);
    v.key(KeyCode::KeyE);
    v.advance(Step::Bell(night));
}

fn clear_camp(world: &mut World, camp: usize) -> Result<(), String> {
    let centre = crate::camps::camp_centre(camp).ok_or("cannot clear a missing camp")?;
    let victims: Vec<_> = world.query_filtered::<(Entity, &Ork), (Without<WaveInvader>, Without<crate::dying::Dying>)>()
        .iter(world).filter(|(_, o)| o.home().distance(centre) < 1.0).map(|(e, o)| (e, o.variant)).collect();
    ensure(!victims.is_empty(), "target camp already empty before scripted attack")?;
    info!("CAMPAIGN_VERIFY scripted camp clear id={camp} defenders={}", victims.len());
    for (e, variant) in victims { scripted_kill(world, e, variant); }
    Ok(())
}

fn collect_and_clear_wave(world: &mut World, v: &mut Verify) {
    let living: Vec<_> = world.query_filtered::<(Entity, &Ork), (With<WaveInvader>, Without<crate::dying::Dying>)>()
        .iter(world).map(|(e, o)| (e, o.variant)).collect();
    for (e, variant) in living {
        if v.wave_entities.insert(e) { v.wave_variants.push(variant); }
        scripted_kill(world, e, variant);
    }
}

fn scripted_kill(world: &mut World, entity: Entity, variant: OrkVariant) {
    let now = world.resource::<Time>().elapsed_secs();
    let at = world.get::<Transform>(entity).map(|t| t.translation)
        .unwrap_or(Vec3::ZERO);
    let bounty = world.resource::<PlayerRes>().0.bounty_mult;
    world.resource_mut::<crate::orbs::RewardBursts>().0.push(crate::orbs::RewardBurst {
        at, gold: crate::orks::bounty_gold(variant, bounty), xp: crate::orks::bounty_xp(variant),
    });
    if let Ok(mut e) = world.get_entity_mut(entity) {
        e.insert(crate::dying::Dying { since: now, dir: Vec2::ZERO, power: 1.0 });
    }
}

fn check_wave(world: &World, v: &Verify, night: usize) -> Result<(), String> {
    let siege = world.resource::<Siege>();
    // Dawn may already have reset the camp scratch for the next day. Compare
    // against the preparation result captured when this particular wave began.
    let cleared = v.wave_raid;
    let quota = crate::siege::effective_count(night, crate::siege::mods_for(siege.difficulty));
    let threat = tileworld_core::threat::threat_for(night);
    let expected: Vec<_> = (0..quota).filter_map(|i| threat.slot_after_raid(i, cleared)).collect();
    ensure(v.wave_variants.len() == expected.len(), &format!("night {} spawned {} enemies, expected {}", night + 1, v.wave_variants.len(), expected.len()))?;
    for variant in crate::orks::VARIANTS {
        let actual = v.wave_variants.iter().filter(|v| **v == variant).count();
        let expected = expected.iter().filter(|s| s.variant == crate::orks::core_variant(variant)).count();
        ensure(actual == expected, &format!("night {} {variant:?}: actual={actual}, expected={expected}", night + 1))?;
    }
    if night == 2 {
        let shamans = v.wave_variants.iter().filter(|v| **v == OrkVariant::Shaman).count();
        ensure((v.route == Route::Raid && shamans == 0) || (v.route != Route::Raid && shamans > 0), "ritual raid or defensive alternative produced the wrong shaman outcome")?;
    }
    info!("CAMPAIGN_VERIFY night={} planned={quota} actual={} camp_cleared={cleared} roster={:?}", night + 1, v.wave_variants.len(), v.wave_variants);
    Ok(())
}

fn purchase(world: &mut World, id: &str) -> bool {
    // Exclusive observer: temporarily removing these resources is confined to this
    // call. Use the same affordability/deduction/effect implementation as the UI.
    let mut up = world.remove_resource::<crate::economy::Upgrades>().unwrap();
    let mut player = world.remove_resource::<PlayerRes>().unwrap();
    let mut bank = world.remove_resource::<crate::economy::Bank>().unwrap();
    let mut defenses = world.remove_resource::<crate::economy::Defenses>().unwrap();
    let mut economy = world.remove_resource::<crate::economy::EconomyState>().unwrap();
    let mut keep = world.remove_resource::<crate::siege::KeepHp>().unwrap();
    let bought = crate::economy::try_purchase(id, &mut up, &mut player, &mut bank, &mut defenses, &mut economy, &mut keep);
    world.insert_resource(up); world.insert_resource(player); world.insert_resource(bank);
    world.insert_resource(defenses); world.insert_resource(economy); world.insert_resource(keep);
    bought
}
