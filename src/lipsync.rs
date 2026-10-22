//! Talking faces. Every townsfolk skin carries MakeHuman's face morph targets (baked by
//! `tools/npc_mpfb/add_face_morphs.py`: the jaw, 14 visemes, two blinks) plus teeth, a tongue and a dark
//! mouth bag, so lips part on real mouth shapes. Each recorded line has a mouth-shape track made offline by
//! Rhubarb Lip Sync from the audio + its transcript (`tools/lipsync/bake_visemes.py` ->
//! `assets/audio/vo/visemes.txt`); while a person speaks, their face plays that track in step with the
//! clip, each shape easing into the next. A clip without a track falls back to the jaw following the
//! line's loudness. Everyone blinks now and then.
//!
//! Two feeds: the voice director registers each spoken line as an [`Utterance`] at the speaker's
//! spot (the nearest skinned person there gets the mouth), and a scripted scene that mixes its VO
//! offline writes a [`Speech`] (clip + time into it) on its actor from its own clock — or, for a line
//! with no track, a [`MouthOpen`] from [`sample`]d loudness.

use bevy::audio::{AudioSource, Decodable, Source};
use bevy::mesh::morph::MorphWeights;
use bevy::prelude::*;
use std::collections::HashMap;
use std::sync::Arc;

/// Envelope sample rate (frames per second of speech).
pub const ENV_HZ: f32 = 30.0;
/// A positional line is mouthed by the skinned person standing within this of where it was spoken.
const NEAR: f32 = 1.6;

// The morph targets, in the order `add_face_morphs.py` bakes them.
const JAW: usize = 0;
const PP: usize = 1;
const FF: usize = 2;
const DD: usize = 4;
const KK: usize = 5;
const SS: usize = 7;
const NN: usize = 8;
const AA: usize = 10;
const E: usize = 11;
const I: usize = 12;
const O: usize = 13;
const U: usize = 14;
const BLINK_L: usize = 15;
const BLINK_R: usize = 16;
const TARGETS: usize = 17;
/// MakeHuman's visemes are sculpted for a close-up; at the distances a game camera sees a face from they
/// read better pushed a little past their sculpt.
const VISEME_GAIN: f32 = 1.3;
/// Seconds one mouth shape takes to give way to the next (coarticulation; Rhubarb's keys are hard cuts).
const SHAPE_BLEND: f32 = 0.07;

pub struct LipSyncPlugin;

impl Plugin for LipSyncPlugin {
    fn build(&self, app: &mut App) {
        // Render-side (a frozen world's speaker still finishes the sentence it's saying).
        app.init_resource::<Utterances>().insert_resource(Tracks::load()).add_systems(PostUpdate, drive_mouths);
    }
}

/// One clip's mouth shapes: (seconds, Rhubarb shape letter) keys, each held until the next.
pub struct Track(Vec<(f32, u8)>);

impl Track {
    /// Add the face this track makes `t` seconds into its clip (scaled by `gain`) onto `out`.
    fn pose(&self, t: f32, gain: f32, out: &mut [f32; TARGETS]) {
        let i = self.0.partition_point(|k| k.0 <= t);
        if i == 0 {
            return;
        }
        let (t0, now) = self.0[i - 1];
        let k = ((t - t0) / SHAPE_BLEND).clamp(0.0, 1.0);
        let k = k * k * (3.0 - 2.0 * k);
        shape(now, k * gain, out);
        if i >= 2 {
            shape(self.0[i - 2].1, (1.0 - k) * gain, out);
        }
    }
    fn end(&self) -> f32 {
        self.0.last().map_or(0.0, |k| k.0)
    }
}

/// Rhubarb's mouth shapes as viseme mixes (its docs: A = M B P, B = most consonants with the teeth
/// together, C = EH AE, D = AA, E = AO ER, F = UW OW W, G = F V, H = L, X = rest).
fn shape(s: u8, w: f32, out: &mut [f32; TARGETS]) {
    let mix: &[(usize, f32)] = match s {
        b'A' => &[(PP, 1.0)],
        b'B' => &[(KK, 0.55), (SS, 0.45), (I, 0.3)],
        b'C' => &[(E, 1.0), (AA, 0.3)],
        b'D' => &[(AA, 1.0), (JAW, 0.12)],
        b'E' => &[(O, 1.0), (AA, 0.25)],
        b'F' => &[(U, 1.0)],
        b'G' => &[(FF, 1.0)],
        b'H' => &[(NN, 0.6), (DD, 0.4), (AA, 0.3)],
        _ => &[],
    };
    for &(i, k) in mix {
        out[i] += k * w;
    }
}

/// Every clip's track, read once from `assets/audio/vo/visemes.txt`.
#[derive(Resource, Default)]
pub struct Tracks(HashMap<String, Arc<Track>>);

impl Tracks {
    fn load() -> Self {
        let path = crate::realforest::load::asset_path("audio/vo/visemes.txt");
        let Ok(text) = std::fs::read_to_string(&path) else {
            warn!("lipsync: no mouth-shape table at {} (mouths follow loudness only)", path.display());
            return Self::default();
        };
        let mut map = HashMap::new();
        for line in text.lines().filter(|l| !l.starts_with('#')) {
            let Some((clip, keys)) = line.split_once('\t') else { continue };
            let keys = keys
                .split_whitespace()
                .filter_map(|k| {
                    let (ms, s) = k.split_once(':')?;
                    Some((ms.parse::<f32>().ok()? / 1000.0, *s.as_bytes().first()?))
                })
                .collect();
            map.insert(clip.to_string(), Arc::new(Track(keys)));
        }
        Self(map)
    }
    pub fn get(&self, clip: &str) -> Option<&Arc<Track>> {
        self.0.get(clip)
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
    /// The clip's path under `assets/` (its mouth-shape track's key).
    pub clip: String,
}

#[derive(Resource, Default)]
pub struct Utterances {
    pub live: Vec<Utterance>,
    /// Envelopes per clip id, decoded once.
    cache: HashMap<&'static str, Arc<Vec<f32>>>,
}

impl Utterances {
    /// Start mouthing `id`'s clip (`clip` = its path under `assets/`) at `at` (replaces any line still
    /// running: one dialogue channel).
    pub fn speak(&mut self, id: &'static str, clip: String, src: &AudioSource, at: Vec3, start: f32, speed: f32) {
        let env = self.cache.entry(id).or_insert_with(|| Arc::new(envelope(src, 0.0, f32::MAX))).clone();
        self.live.clear();
        self.live.push(Utterance { at, start, speed, env, clip });
    }
    /// The channel was cut (an interruption / a new run): shut every mouth.
    pub fn hush(&mut self) {
        self.live.clear();
    }
}

/// Scene-driven mouth opening (0..1) on a speaker's gameplay root, written each frame by a script
/// that knows where its own audio is (for a line with no mouth-shape track).
#[derive(Component, Default, Clone, Copy)]
pub struct MouthOpen(pub f32);

/// The scripted line an actor is saying right now: its clip (path under `assets/`) and the time into
/// that clip (s), written each frame by the scene; `None` between lines.
#[derive(Component, Default, Clone, Copy)]
pub struct Speech(pub Option<(&'static str, f32)>);

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

/// Per face: the morph weights as last written, and when the next blink comes.
#[derive(Component)]
struct Face {
    w: [f32; TARGETS],
    blink_at: f32,
    seed: u32,
}

fn next_blink(seed: &mut u32, now: f32) -> f32 {
    *seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
    now + 2.2 + (*seed >> 8) as f32 / (1u32 << 24) as f32 * 4.5
}

/// A blink's lid closure `s` seconds after it starts: shut in 70 ms, open again over 110 ms.
fn blink(s: f32) -> f32 {
    if !(0.0..=0.18).contains(&s) {
        0.0
    } else if s < 0.07 {
        s / 0.07
    } else {
        1.0 - (s - 0.07) / 0.11
    }
}

/// The loudness fallback: the jaw (and a little "ah") opening with the voice.
fn open_by_loudness(open: f32, want: &mut [f32; TARGETS]) {
    want[JAW] = want[JAW].max(0.22 * open);
    want[AA] = want[AA].max(0.45 * open);
}

fn drive_mouths(
    mut commands: Commands,
    time: Res<Time>,
    mut utt: ResMut<Utterances>,
    tracks: Res<Tracks>,
    people: Query<(&crate::npc_skin::Skinned, &GlobalTransform, Option<&MouthOpen>, Option<&Speech>)>,
    mut faces: Query<(&mut MorphWeights, Option<&mut Face>)>,
) {
    let now = time.elapsed_secs();
    utt.live.retain(|u| {
        let len = tracks.get(&u.clip).map_or(0.0, |t| t.end()).max(u.env.len() as f32 / ENV_HZ);
        (now - u.start) * u.speed < len + 0.3
    });
    // who is saying each live line: the skinned person nearest its spot
    let speakers: Vec<(Entity, &Utterance)> = utt
        .live
        .iter()
        .filter_map(|u| {
            let (e, d) = people
                .iter()
                .filter_map(|(sk, gt, ..)| sk.mouth().map(|m| (m, gt.translation().xz().distance(u.at.xz()))))
                .min_by(|a, b| a.1.total_cmp(&b.1))?;
            (d < NEAR).then_some((e, u))
        })
        .collect();
    let dt = time.delta_secs();
    for (sk, _, opened, speech) in &people {
        let Some(m) = sk.mouth() else { continue };
        let Ok((mut weights, face)) = faces.get_mut(m) else { continue };
        let Some(mut face) = face else {
            let seed = (m.to_bits() as u32) ^ 0x9e37_79b9;
            commands.entity(m).try_insert(Face { w: [0.0; TARGETS], blink_at: now + (seed % 4000) as f32 / 1000.0, seed });
            continue;
        };
        let mut want = [0.0f32; TARGETS];
        for (_, u) in speakers.iter().filter(|(s, _)| *s == m) {
            let t = (now - u.start) * u.speed;
            match tracks.get(&u.clip) {
                Some(track) => track.pose(t, VISEME_GAIN, &mut want),
                None => open_by_loudness(sample(&u.env, t), &mut want),
            }
        }
        if let Some(Speech(Some((clip, t)))) = speech {
            if let Some(track) = tracks.get(*clip) {
                track.pose(*t, VISEME_GAIN, &mut want);
            }
        }
        if let Some(MouthOpen(open)) = opened {
            open_by_loudness(*open, &mut want);
        }
        // a blink now and then (both lids)
        if now > face.blink_at + 0.18 {
            let mut seed = face.seed;
            face.blink_at = next_blink(&mut seed, now);
            face.seed = seed;
        }
        let lid = blink(now - face.blink_at);
        want[BLINK_L] = lid;
        want[BLINK_R] = lid;
        // shapes come on fast and let go a touch slower, like real lips; lids snap
        let mut moved = false;
        for i in 0..TARGETS {
            let next = if i >= BLINK_L {
                want[i]
            } else {
                let rate = if want[i] > face.w[i] { 30.0 } else { 18.0 };
                face.w[i] + (want[i] - face.w[i]) * (1.0 - (-rate * dt).exp())
            };
            let next = if next < 1e-3 && want[i] == 0.0 { 0.0 } else { next };
            moved |= next != face.w[i];
            face.w[i] = next;
        }
        if !moved {
            continue; // nothing changed: don't touch the weights (keeps change detection quiet)
        }
        if weights.weights().len() >= TARGETS {
            weights.weights_mut()[..TARGETS].copy_from_slice(&face.w);
        } else if let Some(v) = weights.weights_mut().first_mut() {
            // an old skin with only the hand-made jaw: open it as far as the face would be open
            *v = (face.w[JAW] * 2.5 + face.w[AA] + face.w[E] * 0.6 + face.w[O] * 0.6).min(1.2);
        }
    }
}
