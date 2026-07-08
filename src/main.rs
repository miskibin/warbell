// Release builds use the Windows GUI subsystem so NO console/terminal window opens alongside the
// game window for players who install it. Debug builds keep the console so `cargo run` still shows
// tracing logs. (The attribute is a no-op on non-Windows targets.)
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

//! Warbell (formerly "D: Tileworld") — a Bevy 0.18 game. A knight defends a central
//! castle against night-wave ork sieges across a five-biome island: real-time combat,
//! economy, an upgrade tree, inventory, villagers, bloodline succession, and wildlife, on
//! one enlarged landmass ringed by open ocean (with drifting boats).
//!
//! Each `mod` below is a self-contained `Plugin` that does its own `Startup` spawn +
//! `Update`/`FixedUpdate` systems; the `add_plugins` calls in `main` are the assembly list
//! (split into tuples because the `Plugins` trait maxes out at arity 15). The world-sim is
//! gated behind the freeze-gate state machine in `game_state` — see `CLAUDE.md` for the
//! conventions and `docs/superpowers/specs/` for the parity roadmap.

mod aftermath;
mod ambient_life;
mod atmospherics;
mod meadow;
mod melee_ring;
mod meshkit; // shared low-poly mesh helpers (tinted/merged/flat_shaded) — see the module doc
mod audio;
mod banner;
mod biped;
mod biome;
mod bridges;
mod biome_desert;
mod biome_forest;
mod biome_rocky;
mod biome_snow;
mod biome_swamp;
mod blockers;
mod boats;
mod boss;
mod build_fx;
mod camps;
mod capture;
mod cinematic;
mod castle;
mod castle_decor;
mod chest;
mod combat_fx;
mod compass;
mod controls;
mod creature;
mod creature_anim;
mod critters;
mod debug_panel;
mod debug_stats;
mod decor;
mod demo;
mod defenses;
mod distant_isles;
mod dof;
mod dying;
mod economy;
mod tree_ui;
mod firelight;
mod fish;
mod footstep_fx;
mod game_state;
mod godrays;
mod grade;
mod groundcover;
mod groundtest; // FOREST_GROUNDTEST=1: floating flat grass plane to isolate the terrain shader
mod hints;
mod hud;
mod interaction;
mod inventory;
mod landmark_models;
mod landmarks;
mod loading;
mod lumberjack;
mod mainmenu;
mod miner;
mod navgrid;
mod nightsky;
mod orbs;
mod ork_fortress;
mod orks;
mod outline;
mod palette;
mod particles;
mod perftest;
mod previs_knight;
mod peasant_model;
mod player;
mod postfx;
mod projectile;
mod props;
mod quadruped;
mod quality;
mod quest;
mod roads;
mod rival;
mod ruins;
mod savegame;
mod scenes;
mod scene;
mod separation;
mod shutters;
mod siege;
mod snowman;
mod steer;
mod subtitles;
mod succession;
mod succession_alert;
mod succession_fx;
mod terrain;
mod town;
mod town_meshes;
mod training_dummies;
mod trees;
mod tutorial;
mod ui;
mod verbs;
mod vignettes;
mod bog;
mod poi;
mod villagers;
mod vista;
mod visual;
mod viewer;
mod warlord;
mod water;
mod wayside;
mod wildlife;
mod wind;
mod window_icon;
mod worldmap;

use bevy::audio::{AudioPlugin, SpatialScale};
use bevy::prelude::*;

fn main() {
    // `FOREST_VIEW=<model>` boots the standalone model viewer (a minimal app — clean stage, no
    // world/gameplay/HUD) instead of the full game, for fast visual inspection of one model.
    if std::env::var("FOREST_VIEW").is_ok() {
        viewer::run();
        return;
    }

    // Screenshot harness window: render at a fixed high resolution + scale-factor 1.0 so the
    // captured PNG is crisp. (A small/low-res capture minifies the ground detail texture to a
    // washed-out pale mean — the real game at native res looks lush.)
    let mut window = Window { title: "Warbell".into(), ..default() };
    if std::env::var("FOREST_SHOT").is_ok() {
        window.resolution =
            bevy::window::WindowResolution::new(1920, 1080).with_scale_factor_override(1.0);
    } else if std::env::var("FOREST_CLIP").is_ok() {
        // Clip capture renders a PNG per frame, so 720p keeps the encode fast while still giving
        // ffmpeg a crisp source to downscale into a GIF (and a clean 720p MP4).
        window.resolution =
            bevy::window::WindowResolution::new(1280, 720).with_scale_factor_override(1.0);
    }
    // FOREST_NOVSYNC=1: uncap the frame rate (default is Fifo / vsync, which pins frame time to the
    // monitor refresh and masks how fast the low-poly scene actually renders). Profiling only.
    if std::env::var("FOREST_NOVSYNC").is_ok() {
        window.present_mode = bevy::window::PresentMode::AutoNoVsync;
    }
    // FOREST_RES="WxH": force a render resolution. Used to make a fast discrete GPU artificially
    // FRAGMENT-bound (like a weak iGPU) so per-fragment optimizations can be A/B-measured locally
    // (the main_opaque pass cost then scales with on-screen pixels, as it does on the iGPU).
    if let Ok(s) = std::env::var("FOREST_RES") {
        let p: Vec<u32> = s.split('x').filter_map(|v| v.trim().parse().ok()).collect();
        if p.len() == 2 {
            window.resolution =
                bevy::window::WindowResolution::new(p[0], p[1]).with_scale_factor_override(1.0);
        }
    }
    App::new()
        .add_plugins(
            DefaultPlugins
                .set(WindowPlugin {
                    primary_window: Some(window),
                    ..default()
                })
                // Shrink the world→audio distance scale so spatial falloff is gentle enough
                // that animals within `audio::AUDIBLE_RANGE` are actually audible (at scale
                // 1.0 a 30-unit distance is near-silent). Tune alongside per-species volume.
                .set(AudioPlugin { default_spatial_scale: SpatialScale::new(0.15), ..default() }),
        )
        // Split across two calls: a single tuple of all of these exceeds the arity the
        // `Plugins` trait is implemented for (≤15).
        .add_plugins((
            game_state::GameStatePlugin, // AppState/Modal states + freeze gate + screens
            economy::EconomyPlugin,      // gold (on PlayerRes) + stone bank
            tree_ui::TreeUiPlugin,       // the War Table upgrade-tree graph panel (U / keep E)
            inventory::InventoryPlugin,  // bag + buffs + pickup toasts (quick-bar Q/Z/X/C)
            verbs::VerbsPlugin,          // biome verbs: ore mining (HeroSwing) → stone
            defenses::DefensePlugin,     // towers/archers/ballista/shrine + war bell (upgrade-gated)
            succession::SuccessionPlugin, // bloodline heir pool: fall → next heir; empty → Defeat
            orbs::OrbsPlugin,            // reward orbs (gold/xp motes) from kills
            scene::ScenePlugin,
            terrain::TerrainPlugin, // registers the terrain material
            water::WaterPlugin,     // registers the water material
            biome::BiomePlugin,     // orchestrates ground/scatter/backdrop/particles
            particles::ParticlePlugin,
            decor::DecorPlugin, // firefly bob system (decor itself spawned per-biome)
            dof::DofPlugin,     // custom CoC bokeh depth-of-field post pass (player-focused)
        ))
        .add_plugins(bog::BogPlugin) // swamp-pool will-o'-wisp hover anim (dressing spawned at build)
        .add_plugins(poi::PoiPlugin) // micro-POI flags: fortress smoke column + wheeling crows
        .add_plugins(vista::VistaPlugin) // waterfall foam/mist animation
        .add_plugins(ambient_life::AmbientLifePlugin) // daytime butterflies + sky bird flock (visual charm)
        .add_plugins(fish::FishPlugin) // fish that glide under water near the hero + occasionally leap
        .add_plugins((
            wind::WindPlugin,
            wildlife::WildlifePlugin,   // ambient animals: wander/graze/startle + limb anim
            audio::GameAudioPlugin,     // event-driven SFX/voice/music/ambience (wav feature on)
            castle::CastlePlugin,       // central castle (built in worldmap) + chimney smoke
            orks::OrksPlugin,           // camp warbands: idle/patrol AI + biped limb anim
            projectile::ProjectilePlugin, // shaman homing bolts (drains BoltSpawns)
            camps::CampsPlugin,         // ork camps (built in worldmap): campfire flicker + smoke
            villagers::VillagersPlugin, // castle townsfolk: idle/stroll AI + biped limb anim
            debug_panel::DebugPanelPlugin, // live egui tuning panel (toggle: F1)
            controls::ControlsPlugin,
            capture::CapturePlugin,
            player::PlayerPlugin, // playable knight: locomotion + follow-cam (` toggles free-roam)
            hud::HudPlugin,       // minimal HP + block-stamina bars
            combat_fx::CombatFxPlugin, // floating numbers, ork HP bars/hurt-flash, hero hit feedback
            siege::SiegePlugin,   // night-wave assault: phases, spawn ring, invader AI, keep HP
        ))
        .add_plugins((
            boats::BoatsPlugin, // background sailboats drifting on the ocean
            ui::UiKitPlugin,    // shared UI kit: theme + fonts + Twemoji icons + motion + notices
            grade::GradePlugin, // reactive low-HP/hit vignette
            training_dummies::TrainingDummiesPlugin, // courtyard practice pells (hit feedback)
            succession_fx::SuccessionFxPlugin, // graves + soul-wisp on each fallen heir
            dying::DyingPlugin, // shared death-fade for orks + wildlife
            visual::VisualPlugin, // volumetric god-rays region, pollen motes, prop specular + panel knobs
            outline::OutlinePlugin, // toon edge-outline post pass (crisp object silhouettes)
            landmarks::LandmarksPlugin, // landmark POIs: discovery caches + shrine buffs + beacons
            footstep_fx::FootstepFxPlugin, // dust puffs / water ripples under the hero's feet
            interaction::InteractionPlugin, // contextual E (keep→upgrades, merchant→shop, bell→night)
            debug_stats::DebugStatsPlugin, // read-only perf/state telemetry overlay (toggle: F2)
            quality::QualityPlugin, // explicit Low/High graphics presets (set in Settings)
            subtitles::SubtitlePlugin, // bottom-centre captions for spoken villager lines
            tutorial::TutorialPlugin, // tabbed "How to Play" help panel (toggle: H)
        ))
        .add_plugins((
            nightsky::NightSkyPlugin, // stars + moon dome that fade in after dark
            firelight::FireLightPlugin, // flickering point-lights on campfires + torches (night)
            shutters::ShutterPlugin, // house window shutters swing shut at dusk (curfew read)
            banner::BannerPlugin, // fluttering cloth flags (keep spire, towers, ork camps)
            aftermath::AftermathPlugin, // persistent battle traces (stains, gear, scorches)
            town::TownPlugin, // city-building: plots, build menu, economy, burn/repair
            lumberjack::LumberjackPlugin, // woodcutters fell real trees (safe zone + threat sense)
            miner::MinerPlugin, // stone miners work real boulders + cart the stone home (ranges far)
            savegame::SaveGamePlugin, // dawn autosave + Continue/New Game (one slot)
            demo::DemoPlugin, // scripted clip scenarios (FOREST_DEMO=explore|defend; build→town.rs)
            separation::SeparationPlugin, // orks + townsfolk shove apart so bodies don't interpenetrate
            castle_decor::CastleDecorPlugin, // courtyard dressing + upgrade-bought set pieces
            ork_fortress::OrkFortressPlugin, // Gnashfang Hold + the Blight: fortress, patrols, hero-firing towers
            cinematic::CinematicPlugin, // keyframed camera paths for trailer shots (FOREST_SHOT_ID)
            build_fx::BuildFxPlugin, // construction pop-in + dust when structures raise
        ))
        .add_plugins((
            scenes::ScenesPlugin, // hand-staged looped trailer tableaus (F1 Director → Scenes)
            creature::CreaturePlugin, // registers the shared creature ExtendedMaterial (hero/orks/wildlife)
            biped::BipedPlugin, // shared studio rig+animator for non-hero bipeds (orcs/peasants)
            quadruped::QuadrupedPlugin, // shared studio quadruped rig+animator (wildlife mammals)
            chest::ChestPlugin, // scattered loot chests: Wood/Relic tiers + juicy opens + Gnashfang mimics
            boss::BossPlugin, // Biome Wardens: per-biome world bosses + boon rewards + reward dialog
            warlord::WarlordPlugin, // the Warlord of Gnashfang Hold: the final boss + win condition
            mainmenu::MainMenuPlugin, // title-screen ambiance: orbit cam + dusk + embers/fireflies + credits
            loading::LoadingPlugin, // branded boot veil over the first ~1s blank frame
            trees::TreeDebugPlugin, // FOREST_TREELINE="x,z" parks one of each tree kind for model shots
            hints::HintsPlugin, // bottom-right affordance toasts: spend/equip nudges (Prep-only)
            quest::QuestPlugin, // tutorial quest chain: right-center tracker + J explainer card
            groundtest::GroundTestPlugin, // debug: FOREST_GROUNDTEST=1 floating ground-shader test plane
            compass::CompassPlugin, // top-centre strip compass: heading + keep/Gnashfang landmark pips
            perftest::PerftestPlugin, // FOREST_PERFTEST=<secs>: headless leak instrumentation (off otherwise)
        ))
        // Screen-space god-rays post pass (light scattering toward the sun). Toggled per-preset via
        // quality.rs (`god_rays`); same custom-post-pass family as dof/outline. Standalone call
        // because the tuples above are at the `Plugins` arity-15 cap.
        .add_plugins(godrays::GodRaysPlugin)
        // Cinematic atmospherics post pass (height fog + sun in-scatter + cloud light patches).
        // Toggled per-preset alongside god-rays in quality.rs; same custom-post-pass family.
        .add_plugins(atmospherics::AtmosphericsPlugin)
        // Castle-meadow dressing (props spawned by worldmap::build_step phase 29; this plugin
        // only runs the campfire rest-heal).
        .add_plugins(meadow::MeadowPlugin)
        // Last-of-the-line alert (heir-count stinger + persistent banner). Standalone: the tuples
        // above are at the `Plugins` arity-15 cap.
        .add_plugins(succession_alert::SuccessionAlertPlugin)
        // Brand the running window (taskbar / title-bar / Alt-Tab). winit doesn't take the icon from
        // the exe's embedded resource (that's only the FILE icon), so we set it at runtime.
        .add_plugins(window_icon::WindowIconPlugin)
        // Cinematic lens: vignette + chromatic aberration (per-preset, in quality.rs) + film grain.
        // The per-biome colour grade itself rides in scene::advance_sky.
        .add_plugins(postfx::PostFxPlugin)
        // Rival AI stronghold: a second castle in the desert (Stronghold-Crusader-style opponent).
        // Standalone: the tuples above are at the `Plugins` arity-15 cap.
        .add_plugins(rival::RivalPlugin)
        // Landmark set-piece animators (mill sails, orbiting shards, glow pulses). Standalone —
        // the tuples above are at the `Plugins` arity-15 cap.
        .add_plugins(ruins::RuinsFxPlugin)
        // Ambush snowmen: static snow-biome decor that wakes + slams when the hero nears or strikes
        // it. Standalone — the tuples above are at the `Plugins` arity-15 cap.
        .add_plugins(snowman::SnowmanPlugin)
        .run();
}
