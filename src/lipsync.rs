//! Talking mouths. Every townsfolk skin carries a `jaw_open` morph target (baked by
//! `tools/npc_mpfb/add_mouth_morph.py`); while a person speaks, it follows the loudness of the line
//! they're saying — not phonemes, just the jaw dropping on each syllable, which reads as speech at
//! any distance a player sees a face from.
//!
//! Two feeds: the voice director registers each spoken line as an [`Utterance`] at the speaker's
//! spot (the nearest skinned person there gets the mouth), and a scripted scene that mixes its VO
//! offline writes a [`MouthOpen`] on its actor from its own clock with [`sample`].

use bevy::audio::{AudioSource, Decodable, Source};
use bevy::mesh::morph::MorphWeights;
use bevy::prelude::*;
use std::collections::HashMap;
use std::sync::Arc;

/// Envelope sample rate (frames per second of speech).
pub const ENV_HZ: f32 = 30.0;
/// A positional line is mouthed by the skinned person standing within this of where it was spoken.
const NEAR: f32 = 1.6;

pub struct LipSyncPlugin;

impl Plugin for LipSyncPlugin {
    fn build(&self, app: &mut App) {
        // Render-side (a frozen world's speaker still finishes the sentence it's saying).
        app.init_resource::<Utterances>().add_systems(PostUpdate, drive_mouths);
    }
}

/// A line being spoken right now.
#[derive(Clone)]
pub struct Utterance {
    /// World spot it was spoken at (the speaker is the skinned person nearest it).
    pub at: Vec3,
    /// `Time::elapsed_secs` when the clip started.
    pub start: f32,
    /// Playback speed the clip was rolled at.
    pub speed: f32,
    pub env: Arc<Vec<f32>>,
}

#[derive(Resource, Default)]
pub struct Utterances {
    pub live: Vec<Utterance>,
    /// Envelopes per clip id, decoded once.
    cache: HashMap<&'static str, Arc<Vec<f32>>>,
}

impl Utterances {
    /// Start mouthing `id`'s clip at `at` (replaces any line still running: one dialogue channel).
    pub fn speak(&mut self, id: &'static str, src: &AudioSource, at: Vec3, start: f32, speed: f32) {
        let env = self.cache.entry(id).or_insert_with(|| Arc::new(envelope(src, 0.0, f32::MAX))).clone();
        self.live.clear();
        self.live.push(Utterance { at, start, speed, env });
    }
    /// The channel was cut (an interruption / a new run): shut every mouth.
    pub fn hush(&mut self) {
        self.live.clear();
    }
}

/// Scene-driven mouth opening (0..1) on a speaker's gameplay root, written each frame by a script
/// that knows where its own audio is.
#[derive(Component, Default, Clone, Copy)]
pub struct MouthOpen(pub f32);

/// Loudness envelope of `src` between `from..to` seconds: RMS per 1/[`ENV_HZ`] frame, normalised so
/// the clip's loud syllables (its 90th percentile) reach ~1 and room tone sits at 0.
pub fn envelope(src: &AudioSource, from: f32, to: f32) -> Vec<f32> {
    let mut dec = src.decoder();
    let rate = dec.sample_rate().get() as f32;
    let ch = dec.channels().get() as usize;
    let per = ((rate / ENV_HZ) as usize).max(1) * ch;
    let (skip, take) = ((from * rate) as usize * ch, ((to - from).max(0.0) * rate).min(1e9) as usize * ch);
    let mut out = Vec::new();
    let (mut acc, mut n) = (0.0f32, 0usize);
    for s in dec.by_ref().skip(skip).take(take) {
        acc += s * s;
        n += 1;
        if n == per {
            out.push((acc / n as f32).sqrt());
            acc = 0.0;
            n = 0;
        }
    }
    if out.is_empty() {
        return out;
    }
    let mut sorted = out.clone();
    sorted.sort_by(|a, b| a.total_cmp(b));
    let loud = sorted[(sorted.len() * 9 / 10).min(sorted.len() - 1)].max(1e-4);
    let floor = sorted[sorted.len() / 5] * 1.5; // breaths / room tone keep the lips together
    for v in &mut out {
        *v = ((*v - floor) / (loud - floor).max(1e-4)).clamp(0.0, 1.0);
    }
    out
}

/// Envelope value `t` seconds into it (0 outside).
pub fn sample(env: &[f32], t: f32) -> f32 {
    if t < 0.0 || env.is_empty() {
        return 0.0;
    }
    let x = t * ENV_HZ;
    let i = x as usize;
    if i + 1 >= env.len() {
        return if i < env.len() { env[i] } else { 0.0 };
    }
    let f = x - i as f32;
    env[i] * (1.0 - f) + env[i + 1] * f
}

/// Per mouth: the smoothed opening.
#[derive(Component, Default)]
struct Jaw(f32);

fn drive_mouths(
    mut commands: Commands,
    time: Res<Time>,
    mut utt: ResMut<Utterances>,
    people: Query<(&crate::npc_skin::Skinned, &GlobalTransform, Option<&MouthOpen>)>,
    mut jaws: Query<(&mut MorphWeights, Option<&mut Jaw>)>,
) {
    let now = time.elapsed_secs();
    utt.live.retain(|u| (now - u.start) * u.speed < u.env.len() as f32 / ENV_HZ + 0.3);
    // who is speaking each live line: the skinned person nearest its spot
    let speakers: Vec<(Entity, f32)> = utt
        .live
        .iter()
        .filter_map(|u| {
            let (e, d) = people
                .iter()
                .filter_map(|(sk, gt, _)| sk.mouth().map(|m| (m, gt.translation().xz().distance(u.at.xz()))))
                .min_by(|a, b| a.1.total_cmp(&b.1))?;
            (d < NEAR).then(|| (e, sample(&u.env, (now - u.start) * u.speed)))
        })
        .collect();
    let dt = time.delta_secs();
    for (sk, _, scripted) in &people {
        let Some(m) = sk.mouth() else { continue };
        let Ok((mut w, jaw)) = jaws.get_mut(m) else { continue };
        let target = speakers
            .iter()
            .filter(|(e, _)| *e == m)
            .map(|(_, v)| *v)
            .fold(scripted.map_or(0.0, |s| s.0), f32::max);
        let Some(mut jaw) = jaw else {
            commands.entity(m).try_insert(Jaw::default());
            continue;
        };
        // syllables open fast and close a touch slower, like a real jaw
        let rate = if target > jaw.0 { 28.0 } else { 14.0 };
        jaw.0 += (target - jaw.0) * (1.0 - (-rate * dt).exp());
        if jaw.0 < 1e-3 && w.weights().first().is_some_and(|v| *v == 0.0) {
            continue; // shut and already written: don't touch (keeps change detection quiet)
        }
        if let Some(v) = w.weights_mut().first_mut() {
            // (a scripted scene may ask past 1.0: the morph target extrapolates, a wider-open jaw)
            *v = jaw.0.min(4.0) * 0.9;
        }
    }
}
