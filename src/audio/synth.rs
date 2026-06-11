//! **Procedural SFX synth** — bakes the gameplay stings forest has no clip for (ore shatter,
//! chest, shop-buy, rescue, level-up, gold, low-HP) into in-memory WAV `AudioSource`s at
//! startup. Pure Rust, no asset files, deterministic (xorshift noise). Lifted from the old
//! game's `sfx.ts` recipes via the bevy port's `audio.rs` synth. (Forage and the war bell
//! started here too but have since been replaced by recorded clips — see `sfx.rs`; a sting
//! graduates OUT of this file the moment a real recording lands in `assets/audio/`.)

use bevy::audio::AudioSource;
use bevy::prelude::*;

const SAMPLE_RATE: u32 = 44_100;

/// One synthesised sting, keyed by event.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Sting {
    OreShatter,
    ChestOpen,
    LevelUp,
    Gold,
    ShopBuy,
    CampRescue,
    LowHp,
}

impl Sting {
    pub const ALL: [Sting; 7] = [
        Sting::OreShatter,
        Sting::ChestOpen,
        Sting::LevelUp,
        Sting::Gold,
        Sting::ShopBuy,
        Sting::CampRescue,
        Sting::LowHp,
    ];
    /// Per-sting output gain (mirrors the old game's category levels).
    pub fn volume(self) -> f32 {
        match self {
            Sting::OreShatter => 0.45,
            Sting::ChestOpen => 0.45,
            Sting::LevelUp => 0.50,
            Sting::Gold => 0.40,
            Sting::ShopBuy => 0.40,
            Sting::CampRescue => 0.50,
            Sting::LowHp => 0.45,
        }
    }
}

/// The baked sting `AudioSource` handles (one per [`Sting`], in `Sting::ALL` order).
#[derive(Resource, Default)]
pub struct StingBank {
    handles: Vec<Handle<AudioSource>>,
}
impl StingBank {
    pub fn handle(&self, s: Sting) -> Option<Handle<AudioSource>> {
        Sting::ALL.iter().position(|&x| x == s).and_then(|i| self.handles.get(i).cloned())
    }
}

/// Bake every sting into an `AudioSource` at startup.
pub fn bake_stings(mut bank: ResMut<StingBank>, mut sources: ResMut<Assets<AudioSource>>) {
    bank.handles =
        Sting::ALL.iter().map(|&s| sources.add(AudioSource { bytes: wav_bytes(&synth(s)).into() })).collect();
}

/// `FOREST_AUDIOTEST=1` plays every baked sting once at boot — verifies the WAV bytes actually
/// decode (the bake can't catch a bad format; the decode only happens on play).
pub fn debug_play_stings(bank: Res<StingBank>, mut commands: Commands, mut done: Local<bool>) {
    if *done {
        return;
    }
    *done = true;
    if std::env::var("FOREST_AUDIOTEST").is_err() {
        return;
    }
    for s in Sting::ALL {
        if let Some(h) = bank.handle(s) {
            commands.spawn((
                bevy::audio::AudioPlayer(h),
                bevy::audio::PlaybackSettings { mode: bevy::audio::PlaybackMode::Despawn, ..default() },
            ));
        }
    }
}

// ── War drums (looping, spatial — see ambience.rs) ───────────────────────────────────

/// Handle to the ork war-drum loop: a recorded ~20 s, 140 BPM deep-tom tribal siege march
/// (`assets/audio/war-drums.ogg`, downmixed to mono so it positions cleanly as a 3D source).
/// Every camp's spatial loop rides this one source (`ambience::attach_war_drum_audio`), silent by
/// day and faded in as the assault musters — the diegetic "the wave is coming" the HUD abstracts.
#[derive(Resource)]
pub struct WarDrumLoop(pub Handle<AudioSource>);

/// Load the war-drum loop at startup. (It was a procedural synth bake — a two-bar tom march —
/// replaced by a real recording; still looped via `PlaybackMode::Loop`, and at 140 BPM the 20 s
/// clip repeats cleanly enough that the seam doesn't read.)
pub fn load_war_drums(mut commands: Commands, asset: Res<AssetServer>) {
    commands.insert_resource(WarDrumLoop(asset.load("audio/war-drums.ogg")));
}

// ── The synth (verbatim from the bevy port's audio.rs) ──────────────────────────────

#[derive(Clone, Copy)]
enum Wave {
    Square,
    Triangle,
    Sine,
}
#[derive(Clone, Copy)]
#[allow(dead_code)] // full synth filter set; not every primitive is used by the baked stings
enum Filter {
    Low,
    High,
    Band,
}

struct Synth {
    buf: Vec<f32>,
    rng: u32,
}

impl Synth {
    fn new() -> Self {
        Synth { buf: Vec::new(), rng: 0x1234_5678 }
    }
    fn noise_sample(&mut self) -> f32 {
        self.rng ^= self.rng << 13;
        self.rng ^= self.rng >> 17;
        self.rng ^= self.rng << 5;
        (self.rng as f32 / u32::MAX as f32) * 2.0 - 1.0
    }
    fn ensure(&mut self, n: usize) {
        if self.buf.len() < n {
            self.buf.resize(n, 0.0);
        }
    }
    fn envelope(dt: f32, dur: f32) -> f32 {
        const ATTACK: f32 = 0.008;
        if dt < 0.0 || dt > dur {
            0.0
        } else if dt < ATTACK {
            (dt / ATTACK).powf(0.6)
        } else {
            let k = (dt - ATTACK) / (dur - ATTACK).max(1e-4);
            (1.0 - k).max(0.0).powf(2.2)
        }
    }
    fn tone(&mut self, wave: Wave, freq: f32, t0: f32, dur: f32, peak: f32, end_freq: Option<f32>) {
        let start = (t0 * SAMPLE_RATE as f32) as usize;
        let len = (dur * SAMPLE_RATE as f32) as usize;
        self.ensure(start + len + 1);
        let f1 = end_freq.unwrap_or(freq).max(1.0);
        let mut phase = 0.0_f32;
        for i in 0..len {
            let dt = i as f32 / SAMPLE_RATE as f32;
            let frac = if dur > 0.0 { dt / dur } else { 0.0 };
            let f = freq * (f1 / freq).powf(frac);
            phase += f / SAMPLE_RATE as f32;
            let ph = phase.fract();
            let osc = match wave {
                Wave::Square => {
                    if ph < 0.5 {
                        1.0
                    } else {
                        -1.0
                    }
                }
                Wave::Triangle => 4.0 * (ph - 0.5).abs() - 1.0,
                Wave::Sine => (ph * std::f32::consts::TAU).sin(),
            };
            let env = Self::envelope(dt, dur);
            self.buf[start + i] += osc * env * peak;
        }
    }
    fn noise(&mut self, t0: f32, dur: f32, peak: f32, filter: Filter, f0: f32, f1: f32) {
        let start = (t0 * SAMPLE_RATE as f32) as usize;
        let len = (dur * SAMPLE_RATE as f32) as usize;
        self.ensure(start + len + 1);
        let mut lp = 0.0_f32;
        let mut prev = 0.0_f32;
        for i in 0..len {
            let dt = i as f32 / SAMPLE_RATE as f32;
            let frac = if dur > 0.0 { dt / dur } else { 0.0 };
            let cutoff = f0 * (f1.max(40.0) / f0).powf(frac);
            let a = (cutoff / (cutoff + SAMPLE_RATE as f32 / std::f32::consts::TAU)).clamp(0.0, 1.0);
            let n = self.noise_sample();
            lp += a * (n - lp);
            let out = match filter {
                Filter::Low => lp,
                Filter::High => n - lp,
                Filter::Band => lp - prev,
            };
            prev = lp;
            let env = Self::envelope(dt, dur);
            self.buf[start + i] += out * env * peak;
        }
    }
    fn finish(self) -> Vec<f32> {
        self.buf.into_iter().map(|s| s.clamp(-1.0, 1.0)).collect()
    }
}

fn synth(s: Sting) -> Vec<f32> {
    let mut y = Synth::new();
    match s {
        Sting::OreShatter => {
            y.tone(Wave::Square, 1320.0, 0.0, 0.07, 0.12, Some(820.0));
            y.tone(Wave::Triangle, 1760.0, 0.0, 0.06, 0.07, None);
            y.noise(0.0, 0.10, 0.10, Filter::High, 4200.0, 1600.0);
        }
        Sting::ChestOpen => {
            y.noise(0.0, 0.22, 0.1, Filter::Band, 500.0, 240.0);
            y.tone(Wave::Sine, 784.0, 0.12, 0.18, 0.1, None);
            y.tone(Wave::Sine, 1175.0, 0.2, 0.24, 0.1, None);
        }
        Sting::LevelUp => {
            for (i, &f) in [523.25_f32, 659.25, 783.99, 1046.5].iter().enumerate() {
                y.tone(Wave::Triangle, f, i as f32 * 0.09, 0.22, 0.13, None);
            }
        }
        Sting::Gold => {
            y.tone(Wave::Square, 880.0, 0.0, 0.08, 0.08, None);
            y.tone(Wave::Square, 1320.0, 0.06, 0.1, 0.08, None);
        }
        Sting::ShopBuy => {
            y.tone(Wave::Sine, 988.0, 0.0, 0.06, 0.10, Some(1320.0));
            y.tone(Wave::Square, 1568.0, 0.07, 0.10, 0.08, None);
            y.noise(0.0, 0.02, 0.03, Filter::High, 6000.0, 4000.0);
        }
        Sting::CampRescue => {
            y.tone(Wave::Sine, 523.25, 0.0, 0.35, 0.10, Some(659.25));
            y.tone(Wave::Sine, 659.25, 0.12, 0.35, 0.10, Some(783.99));
            y.tone(Wave::Triangle, 1046.5, 0.26, 0.40, 0.09, None);
        }
        Sting::LowHp => {
            y.tone(Wave::Square, 330.0, 0.0, 0.10, 0.12, Some(220.0));
            y.tone(Wave::Square, 330.0, 0.16, 0.10, 0.12, Some(220.0));
        }
    }
    y.finish()
}

/// Encode float PCM (-1..1) as a 16-bit mono WAV byte buffer (rodio decodes WAV by default).
fn wav_bytes(samples: &[f32]) -> Vec<u8> {
    let n = samples.len() as u32;
    let bytes_per_sample = 2u32;
    let data_len = n * bytes_per_sample;
    let byte_rate = SAMPLE_RATE * bytes_per_sample;
    let mut out = Vec::with_capacity(44 + data_len as usize);
    out.extend_from_slice(b"RIFF");
    out.extend_from_slice(&(36 + data_len).to_le_bytes());
    out.extend_from_slice(b"WAVE");
    out.extend_from_slice(b"fmt ");
    out.extend_from_slice(&16u32.to_le_bytes());
    out.extend_from_slice(&1u16.to_le_bytes());
    out.extend_from_slice(&1u16.to_le_bytes());
    out.extend_from_slice(&SAMPLE_RATE.to_le_bytes());
    out.extend_from_slice(&byte_rate.to_le_bytes());
    out.extend_from_slice(&(bytes_per_sample as u16).to_le_bytes());
    out.extend_from_slice(&16u16.to_le_bytes());
    out.extend_from_slice(b"data");
    out.extend_from_slice(&data_len.to_le_bytes());
    for &s in samples {
        let v = (s.clamp(-1.0, 1.0) * i16::MAX as f32) as i16;
        out.extend_from_slice(&v.to_le_bytes());
    }
    out
}
