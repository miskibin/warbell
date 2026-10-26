// Release builds use the Windows GUI subsystem so NO console/terminal window opens alongside the
// game window for players who install it. Debug builds keep the console so `cargo run` still shows
// tracing logs. (The attribute is a no-op on non-Windows targets.)
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

//! Warbell (formerly "D: Tileworld") — a Bevy 0.19 game. A knight defends a central
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
mod par; // scoped data-parallel map for the world build
mod alpine; // snow / meadow / scree / rock terrain layers + snow-dusted kit scatter (FOREST_ALPINE=0 disables)
mod ambient_life;
mod atmospherics;
mod meadow;
mod countryside; // the farmed countryside between the castle meadow and the biomes (Oct 2026)
mod pasture; mod drill_yard; mod watch_beacon; mod caravan; // WP4 meadow sites (spec 2026-10-13 §2 D–E)
mod melee_ring;
mod meshkit; // shared low-poly mesh helpers (tinted/merged/flat_shaded) — see the module doc
mod audio;
mod banner;
mod biped;
mod npc_skin; // realistic skinned townsfolk (MPFB cast + UAL clips) over the studio rig
mod lipsync; // talking mouths: a speaker's jaw_open morph follows the loudness of their line
mod animal_skin; // realistic skinned red deer (CC0 CDmir) over the studio quad rig for the Deer / Elk herds
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
mod captives; // caged captives' idle life (pose override on the shared biped rig)
mod campaign_verify; // opt-in first-three-cycles engine integration checks
mod capture;
mod cinematic;
mod castle;
mod castle_decor;
mod gates; // castle gate mechanics: shut at dusk (oak leaves / wagon barricade), orks hack them
mod market; // castle market: kit stalls + cart + barrels, vendors behind the counters, shoppers
mod town_kit; // castle town kit (town_kit.glb): spawn_piece handle + FOREST_TOWNKIT_SHOWCASE row
mod castle_ground; // lived-in castle yard ground: lanes, ruts, plaza, worn grass (baked into the terrain masks)
mod chest;
mod combat_fx;
mod combat_marks; // shared procedural ground-decal shader: target ring, shockwave, blood, scorch
mod compass;
mod controls;
mod creature;
mod creature_anim;
mod critters;
mod cursor;
mod debug_panel;
mod debug_stats;
mod decor;
mod demo;
#[cfg(test)]
mod density_report; // test-only 40u planned-site density audit (living-island spec §1)
mod defenses;
mod distant_isles;
mod dof;
mod filmgrade;
mod dying;
mod economy;
mod tree_ui;
mod firelight;
mod flame_fx; // crossed flame cards for braziers / fire baskets (transform-only flicker)
mod fish;
mod footing; // what's underfoot + when a foot lands: the shared Footfall stream (sound / dust / prints)
mod footprints; // lingering boot-print decals in mud / snow / sand
mod footstep_fx;
mod grass_trample; // undergrowth parts round passing legs + lies flat where feet came down
mod game_state;
mod godrays;
mod grade;
mod groundcover;
mod groundtest; // FOREST_GROUNDTEST=1: floating flat grass plane to isolate the terrain shader
mod hints;
mod horse; // the rideable horse: L calls it (fade), WASD steers, a fight throws the knight off and it bolts
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
mod clouds; // realistic high cloud deck: far-plane sky dome, ray-cast Perlin-Worley deck, live sun/moon light
mod orbs;
mod ork_fortress;
mod orks;
mod outline;
mod palette;
mod particles;
mod anim_lod; // skinned crowd animation LOD: park far / off-screen skins (npc_skin, animal_skin)
mod anim_budget; // animation budget (UE URO): far / off-screen skins re-pose every few frames, staggered
mod autolod; // meshopt auto-LOD: simplified far twins of kit props / statics / skins
mod audio_capture; // FOREST_CLIP: the game's own audio mirrored offline, frame-locked, into <dir>/audio.wav
mod water_ssr; // water screen-space reflections: rivers/lake/sea mirror the banks, trees and buildings
mod shadow_lod; // shadow LOD: small static meshes stop casting sun shadows past a size-scaled distance
mod matcheck; // FOREST_MATCHECK=1: log mesh entities carrying two material types (double-bin panic)
mod perftest;
#[cfg(feature = "renderdoc")]
mod rdoc; // FOREST_RDOC=<secs>: one RenderDoc GPU frame capture (`--features renderdoc`)
mod pipeline_sync; // prepass waits for the main-pass pipeline (no sky-coloured foliage holes)
mod previs_knight;
mod peasant_model;
mod player;
mod postfx;
mod projectile;
mod props;
mod quadruped;
mod quality;
mod quest;
mod phototrees; // FOREST_PHOTOTREES=1: experimental procedural/photographic trees (off by default)
mod realforest; // Blender-authored PBR forest kit (trees / undergrowth / props / ground); FOREST_PBR=0 disables
mod rc;
mod traps; // trap-pocket analysis + rim mask (ground you can drop into but not climb out of)
mod road_route; // slope-aware A* footpath planner for the road network
mod roads;
mod rival;
mod rts;
mod ruins;
mod savegame;
mod scenes;
mod scene;
mod separation;
mod shutters;
mod siege;
mod snowman;
mod oddities;
mod stashes;
mod castaway;
mod wreck_cove;
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
mod sites_warm;
mod stories_cold;
mod castle_kit;
mod stories_warm;
mod wild_sites;
mod war_party;
mod witch; // Old Seffa at the Witch's Hut: chores, the silent-trade stone, her shop (living-island WP2)
mod ork_graveyard; // the Field of the Answered: a fixed field of ork graves, mourners, offering vs grave goods
mod villagers;
mod village_life; // daily life of the courtyard + street village: schedule, benches, chores, market, chats
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
    // Unattended verification should not activate its window over the user's current app.
    // NB: Windows throttles an UNFOCUSED window's presents to 60 Hz, so an unfocused PERFTEST is
    // pinned at 16.67 ms no matter how cheap the frame is. `FOREST_PERFFOCUS=1` keeps the window
    // focused for real frame-rate measurement (it will grab the foreground while it runs).
    if (std::env::var("FOREST_PERFTEST").is_ok() && std::env::var("FOREST_PERFFOCUS").is_err()) || std::env::var("FOREST_SHOT").is_ok()
        || std::env::var("FOREST_RTS_ECOTEST").is_ok() || std::env::var("FOREST_CAMPAIGN_VERIFY").is_ok() {
        window.focused = false;
    }
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
    // FOREST_FRAMELAT=<n>: swapchain frames the CPU may run ahead of the GPU (wgpu default 2).
    if let Some(n) = std::env::var("FOREST_FRAMELAT").ok().and_then(|v| v.parse::<u32>().ok()) {
        window.desired_maximum_frame_latency = std::num::NonZeroU32::new(n);
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
    let headless_verify = std::env::var("FOREST_CAMPAIGN_VERIFY_HEADLESS").as_deref() == Ok("1")
        && std::env::var("FOREST_CAMPAIGN_VERIFY").is_ok();
    let mut render_settings = bevy::render::settings::WgpuSettings::default();
    if std::env::var("WGPU_SETTINGS_PRIO").as_deref() == Ok("webgpu") {
        // Bevy's atmosphere LUTs sample Rgba32Float through filtering samplers. Keep this
        // required feature when explicitly limiting optional features for software captures.
        render_settings.features |= wgpu::Features::FLOAT32_FILTERABLE;
    }
    let mut plugins = DefaultPlugins
                .set(WindowPlugin {
                    primary_window: Some(window),
                    ..default()
                })
                .set(bevy::render::RenderPlugin {
                    render_creation: bevy::render::settings::RenderCreation::Automatic(Box::new(render_settings)),
                    // Compile one pipeline at a time on memory-limited capture machines.
                    synchronous_pipeline_compilation: std::env::var("FOREST_RENDER_SYNC").as_deref() == Ok("1"),
                    ..default()
                })
                // Shrink the world→audio distance scale so spatial falloff is gentle enough
                // that animals within `audio::AUDIBLE_RANGE` are actually audible (at scale
                // 1.0 a 30-unit distance is near-silent). Tune alongside per-species volume.
                .set(AudioPlugin { default_spatial_scale: SpatialScale::new(0.15), ..default() });
    let mut app = App::new();
    if headless_verify {
        // Exercise the same world and simulation systems without GPU pipeline compilation.
        // Keep the logical window for UI layout; screenshots use the ordinary renderer.
        plugins = plugins.set(bevy::render::RenderPlugin {
            render_creation: bevy::render::settings::RenderCreation::Automatic(Box::new(
                bevy::render::settings::WgpuSettings { backends: None, ..default() },
            )),
            ..default()
        }).disable::<bevy::winit::WinitPlugin>();
        app.add_plugins((
            bevy::app::ScheduleRunnerPlugin::run_loop(std::time::Duration::ZERO),
            // Render components still install removal hooks in a backend-free app.
            bevy::extract::sync_world::SyncWorldPlugin::<bevy::render::RenderApp>::default(),
        ));
    }
    app
        .add_plugins(plugins)
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
        .add_plugins(distant_isles::DistantIslesPlugin) // horizon mountains: aerial-perspective haze driver
        .add_plugins(filmgrade::FilmGradePlugin) // final RDR2-style look pass: split-tone, vibrance, S-curve, sharpen
        .add_plugins(gates::GatesPlugin) // gates shut at dusk, open at dawn; ork-only solid, hackable
        .add_plugins(market::MarketPlugin) // the living market on the west lane: stalls, vendors, vendor greetings
        .add_plugins(flame_fx::FlameFxPlugin) // brazier / fire-basket flame cards
        .add_plugins(town_kit::TownKitPlugin) // town-kit pieces (showcase only until the phase-4 layout)
        .add_plugins((witch::WitchPlugin, ork_graveyard::OrkGraveyardPlugin)) // WP2 swamp: Old Seffa + the ork graveyard
        .add_plugins(bog::BogPlugin) // swamp-pool will-o'-wisp hover anim (dressing spawned at build)
        .add_plugins(poi::PoiPlugin) // micro-POI flags: fortress smoke column + wheeling crows
        .add_plugins(vista::VistaPlugin) // waterfall foam/mist animation
        .add_plugins(ambient_life::AmbientLifePlugin) // daytime butterflies + sky bird flock (visual charm)
        .add_plugins(village_life::VillageLifePlugin) // residents' daily life: benches, chores, market, chats, kids
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
            savegame::SaveGamePlugin, // dawn + 10-min autosave, 5 manual slots, Continue/Load
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
            // shared studio rig+animator for non-hero bipeds (orcs/peasants) + the realistic skinned
            // townsfolk drawn over it
            (biped::BipedPlugin, npc_skin::NpcSkinPlugin, lipsync::LipSyncPlugin, animal_skin::AnimalSkinPlugin, anim_budget::AnimBudgetPlugin),
            quadruped::QuadrupedPlugin, // shared studio quadruped rig+animator (wildlife mammals)
            chest::ChestPlugin, // scattered loot chests: Wood/Relic tiers + juicy opens + Gnashfang mimics
            boss::BossPlugin, // Biome Wardens: per-biome world bosses + boon rewards + reward dialog
            warlord::WarlordPlugin, // the Warlord of Gnashfang Hold: the final boss + win condition
            mainmenu::MainMenuPlugin, // title-screen ambiance: orbit cam + dusk + embers/fireflies + credits
            loading::LoadingPlugin, // branded boot veil over the first ~1s blank frame
            trees::TreeDebugPlugin, // FOREST_TREELINE="x,z" parks one of each tree kind for model shots
            hints::HintsPlugin, // bottom-right affordance toasts: spend/equip nudges (Prep-only)
            quest::QuestPlugin, // situation-led first three cycles + optional J details
            groundtest::GroundTestPlugin, // debug: FOREST_GROUNDTEST=1 floating ground-shader test plane
            compass::CompassPlugin, // top-centre strip compass: heading + keep/Gnashfang landmark pips
            perftest::PerftestPlugin, // FOREST_PERFTEST=<secs>: headless leak instrumentation (off otherwise)
        ))
        // Keep each mesh out of the depth prepass until its main-pass pipeline has compiled too
        // (cold shader cache: depth-only leaves = sky-coloured holes). See `pipeline_sync`.
        .add_plugins(war_party::WarPartyPlugin) // the outcast, war-trail posts, captive escorts (spec 2026-10-13 §2 E)
        .add_plugins(pipeline_sync::PipelineSyncPlugin)
        // Screen-space god-rays post pass (light scattering toward the sun). Toggled per-preset via
        // quality.rs (`god_rays`); same custom-post-pass family as dof/outline. Standalone call
        // because the tuples above are at the `Plugins` arity-15 cap.
        .add_plugins(godrays::GodRaysPlugin)
        // Feathered procedural combat ground marks (target ring / shockwave / blood / scorch) —
        // standalone for the same arity reason.
        .add_plugins(combat_marks::CombatMarksPlugin)
        // Footfalls (surface classifier + hero / NPC step events) and the boot prints they leave —
        // standalone for the same arity reason.
        .add_plugins((footing::FootingPlugin, footprints::FootprintsPlugin, grass_trample::GrassTramplePlugin))
        // Experimental photoreal trees (`FOREST_PHOTOTREES=1`) — builds the procedural tree meshes
        // + foliage atlas at PreStartup so the world build can spawn them. A no-op unless the env
        // var is set. Standalone call for the same reason as godrays: the tuples above are at the
        // `Plugins` arity-15 cap, and appending to one is what broke the build first try.
        .add_plugins(phototrees::PhotoTreesPlugin)
        .add_plugins(realforest::RealForestPlugin)
        // Alpine terrain layers + the snow-dusted kit materials (standalone: tuple arity cap).
        .add_plugins(alpine::AlpinePlugin)
        // Cinematic atmospherics post pass (height fog + sun in-scatter + cloud light patches).
        // Toggled per-preset alongside god-rays in quality.rs; same custom-post-pass family.
        .add_plugins(atmospherics::AtmosphericsPlugin)
        // Castle-meadow dressing (props spawned by worldmap::build_step phase 29; this plugin
        // only runs the campfire rest-heal).
        .add_plugins(meadow::MeadowPlugin)
        // The farmed countryside's grazing horses (the rest is static, spawned by the world build).
        .add_plugins(countryside::CountrysidePlugin)
        .add_plugins(horse::HorsePlugin) // rideable horse: L to mount / dismount, bolts when a fight starts
        .add_plugins((pasture::PasturePlugin, drill_yard::DrillYardPlugin, watch_beacon::WatchBeaconPlugin, caravan::CaravanPlugin))
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
        // RTS skirmish mode "Potyczka" (FOREST_RTS=1): iso camera, base-building, mirrored AI.
        // Inert in Campaign — every system inside is gated on `rts::in_skirmish`.
        .add_plugins(rts::RtsPlugin)
        // Remote-control bridge for agent/manual testing (FOREST_RC=<dir>): JSON commands in,
        // JSON game-state snapshots out. Inert without the env var.
        .add_plugins(rc::RcPlugin)
        // Game-wide custom cursor (gold reticle replacing the OS pointer in every state).
        .add_plugins(cursor::CursorPlugin)
        // Landmark set-piece animators (mill sails, orbiting shards, glow pulses). Standalone —
        // the tuples above are at the `Plugins` arity-15 cap.
        .add_plugins(ruins::RuinsFxPlugin)
        // Ambush snowmen: static snow-biome decor that wakes + slams when the hero nears or strikes
        // it. Standalone — the tuples above are at the `Plugins` arity-15 cap.
        .add_plugins(snowman::SnowmanPlugin)
        // Caged captives' idle life: rocking, rubbing, slumping, trips to the bars, flinches.
        // Standalone — the tuples above are at the `Plugins` arity-15 cap.
        .add_plugins(captives::CaptivesPlugin)
        // WP3 wilds: the five biome creatures, the biome stashes + wolverine dens, Tobin the castaway.
        .add_plugins((oddities::OdditiesPlugin, stashes::StashesPlugin, castaway::CastawayPlugin));
    campaign_verify::install(&mut app);
    if std::env::var("FOREST_RENDER_SYNC").as_deref() == Ok("1") {
        if let Some(render) = app.get_sub_app_mut(bevy::render::RenderApp) {
            // Default mesh slabs grow to 512MiB and retain the old buffer during copying.
            // The allocator lives in RenderApp, so configure its resource there.
            let mut meshes = bevy::render::mesh::allocator::MeshAllocatorSettings::default();
            meshes.slab_allocator_settings.max_slab_size = 64 * 1024 * 1024;
            meshes.slab_allocator_settings.large_threshold = 32 * 1024 * 1024;
            render.insert_resource(meshes);
        }
    }
    app.add_plugins((shadow_lod::ShadowLodPlugin, audio_capture::AudioCapturePlugin, water_ssr::WaterSsrPlugin));
    app.add_plugins(matcheck::MatCheckPlugin);
    #[cfg(feature = "renderdoc")]
    app.add_plugins(rdoc::RdocPlugin);
    app.run();
}
