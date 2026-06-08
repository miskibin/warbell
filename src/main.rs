//! Tileworld Biomes — a single static Bevy 0.18 scene recreating the TS game's
//! biomes as 32×32 grid patches with matching ground, models and post-processing. No
//! player, no day/night, no gameplay — just the scene. Press keys **1–5** to switch
//! biome (Forest / Snow / Rocky / Desert / Swamp).

mod audio;
mod biome;
mod biome_desert;
mod biome_forest;
mod biome_rocky;
mod biome_snow;
mod biome_swamp;
mod blockers;
mod camps;
mod capture;
mod castle;
mod combat_fx;
mod controls;
mod critters;
mod debug_panel;
mod decor;
mod defenses;
mod dying;
mod distant;
mod economy;
mod game_state;
mod grade;
mod groundcover;
mod hud;
mod icons;
mod inventory;
mod navgrid;
mod orbs;
mod orks;
mod outline;
mod palette;
mod particles;
mod player;
mod projectile;
mod props;
mod ruins;
mod scene;
mod sea;
mod siege;
mod steer;
mod succession;
mod terrain;
mod training_dummies;
mod trees;
mod verbs;
mod villagers;
mod visual;
mod water;
mod wildlife;
mod wind;
mod worldmap;

use bevy::audio::{AudioPlugin, SpatialScale};
use bevy::prelude::*;

fn main() {
    // Screenshot harness window: render at a fixed high resolution + scale-factor 1.0 so the
    // captured PNG is crisp. (A small/low-res capture minifies the ground detail texture to a
    // washed-out pale mean — the real game at native res looks lush.)
    let mut window = Window { title: "Tileworld Biomes — Bevy".into(), ..default() };
    if std::env::var("FOREST_SHOT").is_ok() {
        window.resolution =
            bevy::window::WindowResolution::new(1920, 1080).with_scale_factor_override(1.0);
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
            distant::DistantPlugin,
        ))
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
            icons::IconsPlugin, // procedural item icons (satchel + shop)
            grade::GradePlugin, // reactive low-HP/hit vignette
            training_dummies::TrainingDummiesPlugin, // courtyard practice pells (hit feedback)
            dying::DyingPlugin, // shared death-fade for orks + wildlife
            visual::VisualPlugin, // volumetric god-rays region, pollen motes, prop specular + panel knobs
            outline::OutlinePlugin, // toon edge-outline post pass (crisp object silhouettes)
        ))
        .run();
}
