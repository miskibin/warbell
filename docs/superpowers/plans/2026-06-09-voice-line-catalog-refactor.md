# Voice-Line Catalog Refactor — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Replace the four hand-coded per-module voice drivers (`voice.rs`, `hero_remarks.rs`, `npc.rs`, `ork.rs`) with ONE data catalog of voice lines + ONE pure resolver, so every spoken line records its speaker, transcript, interruptibility, priority, and reply-chains as first-class data — and subtitles gain speaker attribution.

**Architecture:** A Valve-style **criteria-bark** model (per the 2026-06-09 deep-research report), self-rolled, NOT a dialogue tree. Pure data + decision logic live in a Bevy-free `src/audio/lines.rs` (unit-tested with `cargo test`, mirroring the crate's core-vs-render split). A thin Bevy `src/audio/director.rs` gathers world facts, calls the resolver, spawns the audio sink, and pushes the subtitle. Triggers emit a `Speak{concept}` message; "when a concept fires" stays as code in the existing `detect_*` systems ("what is said" = data, "when" = code — the cheap 80%). Migration is incremental: build the core, migrate hero → villager → ork one speaker at a time, deleting each old driver as its lines move, `cargo check` between.

**Tech Stack:** Rust, Bevy 0.18.1 (`Message` API, `AudioPlayer`/`PlaybackSettings`, ECS Resources/Systems), existing `crate::subtitles`, `crate::biome::Biome`.

---

## Background / source of truth

- Research report: the workflow output summarized in this session (Valve GDC2012 dynamic-dialog, Pixel Crushers bark priority, Ink/Yarn per-line metadata, Source closed-captions). Key takeaways baked into this design:
  - **Per-line `interruptible` + `priority`**; new line cuts current only if `current.interruptible && new.priority >= current.priority`. Default death cry = priority 255.
  - **Chains via "then" concept dispatch**, no conversation entity. A finished line may dispatch a follow-up concept at a target speaker, who looks up its own `reply_to` lines. If none match the still-current world facts, the chain silently dies (free Valve self-terminate).
  - **Stable string `id` keys {clip, caption}** — never key on file path. `id` doubles as the `.ogg` stem AND the transcript record (satisfies the CLAUDE.md "every spoken line carries its transcript" mandate, now as data not a comment).
- Current code being replaced (read before each migration task):
  - `src/audio/mod.rs` — `AudioCue` enum, `HeroEvent`, `HeroLineGates`, the cooldown resources (`HeroLineCooldown`/`HeroSpeaking`/`OthersSpeaking`), `HeroLineAnchor`, `HeroMouthTag`, the `detect_*` trigger systems, and the shared RNG helpers (`frand`/`pick`/`jitter`/`next_rng`).
  - `src/audio/voice.rs` — hero "one mouth": biome musings + `HeroEvent` reactions + combat grunts.
  - `src/audio/hero_remarks.rs` — hero observational remarks (`REMARKS` table already has transcripts as data — the model to generalize).
  - `src/audio/npc.rs` — villager ambient (`AMBIENT_KEYS`/`AMBIENT_TEXT` parallel arrays) + event lines, shuffle-bag.
  - `src/audio/ork.rs` — ork battle barks + death snarl.
  - `src/subtitles.rs` — `Subtitles::say(now, text, dur)`, `read_secs(text)`.

## File Structure

- **Create `src/audio/lines.rs`** — Bevy-free. Owns: `Speaker`, `Concept`, `Line`, `Chain`, the `LINES` catalog const, `SPEAKERS` registry, and the pure resolver fns (`candidates`, `can_play`, `pick_line`). Has a `#[cfg(test)] mod tests`. This is the one file that gets real TDD.
- **Create `src/audio/director.rs`** — Bevy glue. Owns: `Speak` message, `VoiceManager` resource (active-line-per-speaker + per-line last-played + per-speaker cadence + rng), the `speak_director` system (drain `Speak`, resolve, spawn sink, push subtitle, record active), the `tick_chains` system (fire `then` dispatch when a line ends), and helpers for spatial placement (nearest villager/ork). Replaces the per-module mouth/cooldown bookkeeping.
- **Modify `src/audio/mod.rs`** — register the two new modules + `Speak` message + `VoiceManager`; keep the `detect_*` trigger systems but have them write `Speak{concept}` instead of `AudioCue::HeroEvent`/`HeroLine`; retire `HeroEvent`, `HeroLineGates` once-per-run gates fold into the catalog/manager, and the three cooldown resources as each speaker migrates. Keep combat grunts (`HeroGruntSwing`/`HeroJump`/`HeroHurt`/`HeroDeath`) and all non-voice cues on `AudioCue` untouched.
- **Modify `src/audio/voice.rs`** — strip the biome/event line logic (moves to catalog+director); keep ONLY the combat exertion grunts + death cry (short reflexes, not catalog lines). Rename to reflect it's now just grunts, or keep the file with the line code deleted.
- **Delete (by emptying into the catalog) `src/audio/hero_remarks.rs`, the line bodies of `npc.rs` and `ork.rs`** — their transcripts + keys become `LINES` entries; their selection/cooldown logic becomes the resolver + `VoiceManager`. Their *trigger* conditions (proximity, phase, earshot) move into small `detect_*`-style systems that emit `Speak`.
- **Modify `src/subtitles.rs`** — add optional speaker name to `say` (e.g. `say_as(now, speaker, text, dur)`), render `Name: text`.

---

## Phase A — Pure core (TDD with `cargo test`)

### Task A1: Data types + speaker registry

**Files:**
- Create: `src/audio/lines.rs`
- Modify: `src/audio/mod.rs` (add `mod lines; pub(crate) use lines::*;` near the other `mod` lines, ~line 15-24)

- [ ] **Step 1: Create `src/audio/lines.rs` with the data model**

```rust
//! Bevy-free voice-line catalog + pure resolver. Every spoken line in the game is one [`Line`]
//! entry here: its speaker, transcript (the on-screen subtitle AND our in-code record of the
//! quote, per CLAUDE.md), whether it can be cut off, its barge-in priority, and optional reply
//! chains. The Bevy glue that actually plays clips lives in `director.rs`; this module is pure
//! data + decision logic so it can be unit-tested without spinning up an App.
//!
//! Model is the Valve "dynamic dialog" bark scheme (see
//! `docs/superpowers/plans/2026-06-09-voice-line-catalog-refactor.md`): a concept fires, the
//! resolver gathers candidate lines for it, filters by a per-line replay floor, and picks one.

use crate::biome::Biome;

/// Who owns a line — selects voice routing (head-locked vs spatial) via [`SPEAKERS`].
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum Speaker {
    Hero,
    Villager,
    Ork,
}

/// How a speaker's voice is routed. Looked up from [`SPEAKERS`] by the director.
#[derive(Clone, Copy)]
pub struct SpeakerVoice {
    /// Head-locked (hero) vs world-positioned (villager/ork).
    pub spatial: bool,
    /// Base gain multiplier (× `AudioConfig.voice_vol`).
    pub gain: f32,
    /// Display name shown in the subtitle (`None` = no prefix, e.g. the hero's own musings).
    pub name: Option<&'static str>,
}

/// The voice registry: one entry per [`Speaker`]. Linear-scanned (3 entries).
pub const SPEAKERS: &[(Speaker, SpeakerVoice)] = &[
    (Speaker::Hero, SpeakerVoice { spatial: false, gain: 1.0, name: None }),
    (Speaker::Villager, SpeakerVoice { spatial: true, gain: 1.4, name: Some("Townsfolk") }),
    (Speaker::Ork, SpeakerVoice { spatial: true, gain: 0.85, name: None }),
];

pub fn speaker_voice(s: Speaker) -> SpeakerVoice {
    SPEAKERS.iter().find(|(k, _)| *k == s).map(|(_, v)| *v).expect("every Speaker is registered")
}

/// A situation that asks for a line. Triggers (`detect_*` systems) emit one of these; the
/// resolver maps it to candidate [`Line`]s. Biome musings carry the biome so one concept covers
/// all five. The `Reply*` variants are chain targets dispatched by a finished line's `then`.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Concept {
    // ── Hero event reactions (was `HeroEvent`) ──
    FirstStone,
    ChestOpen,
    FirstRescue,
    NightWarning,
    LowHp,
    Home,
    Equip,
    LevelUp,
    WaveSurvived,
    FirstKill,
    GoldRich,
    Broke,
    KeepHurt,
    ShrineHeal,
    // ── Hero biome musing (was `HeroLine(Biome)`) ──
    BiomeEntered(Biome),
    // ── Hero observational remarks (was `Trig`) ──
    Intro,
    NearTown,
    NearKids,
    NearPet,
    NearGuard,
    InKeep,
    NightMusing,
    QuietMusing,
    KillMusing,
    // ── Villager ──
    Greeting,
    SiegeFalls,
    Dawn,
    Rescued,
    // ── Ork ──
    OrkSpot,
    OrkDeath,
    // ── Chain reply concepts ──
    ReplyToVillagerJab,
}

/// A follow-up dispatched when a line finishes: ask `target` to look up a line whose
/// `reply_to == Some(concept)`. If none matches the (now-current) facts, nothing plays — the
/// chain self-terminates (the Valve "no explicit interruption" property).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Chain {
    pub concept: Concept,
    pub target: Speaker,
}

/// One voice line — the whole record.
#[derive(Clone, Copy)]
pub struct Line {
    /// Stable key; also the clip stem at `audio/vo/<dir>/<id>.ogg` (dir per speaker).
    pub id: &'static str,
    pub speaker: Speaker,
    pub concept: Concept,
    /// Transcript: the on-screen subtitle AND our in-code record of the quote.
    pub text: &'static str,
    /// May a louder/just-as-loud new line cut this off mid-clip?
    pub interruptible: bool,
    /// Barge-in priority: a new line plays over a playing one only if `new.priority >= cur.priority`.
    pub priority: u8,
    /// If set, this line is a valid reply to a dispatched chain `Concept`.
    pub reply_to: Option<Concept>,
    /// If set, dispatch this chain when the line finishes.
    pub then: Option<Chain>,
}

/// Convenience constructor for the common case (no reply_to / no then, interruptible, prio 10).
const fn line(id: &'static str, speaker: Speaker, concept: Concept, text: &'static str) -> Line {
    Line { id, speaker, concept, text, interruptible: true, priority: 10, reply_to: None, then: None }
}

/// THE catalog. Filled in across the migration tasks (Phase C). Starts with a single hero line so
/// Phase A/B have something real to resolve and test against.
pub const LINES: &[Line] = &[
    line("levelup", Speaker::Hero, Concept::LevelUp, "Stronger. The blade feels lighter than it did."),
];
```

- [ ] **Step 2: Register the module**

In `src/audio/mod.rs`, add alongside the existing `mod` declarations (the block at lines ~15-24):

```rust
mod director;
mod lines;
```

And re-export for the trigger systems (near the top, after the `mod` block):

```rust
pub(crate) use lines::{Concept, Line, Speaker};
```

(`mod director;` will fail to compile until Task B1 creates the file — do A1 and B1 in the same checkpoint, or temporarily comment `mod director;` until B1. Note this in the commit.)

- [ ] **Step 3: Verify it type-checks**

Run: `cargo check`
Expected: compiles (with `director` commented out if B1 not yet done). Warnings about unused `LINES`/`speaker_voice` are fine at this stage.

- [ ] **Step 4: Commit**

```bash
git add src/audio/lines.rs src/audio/mod.rs
git commit -m "feat(audio): voice-line catalog data model (Speaker/Concept/Line)"
```

---

### Task A2: Resolver — `candidates` + `pick_line` (TDD)

**Files:**
- Modify: `src/audio/lines.rs` (add resolver fns + tests)

- [ ] **Step 1: Write the failing tests**

Append to `src/audio/lines.rs`:

```rust
/// All catalog lines for a concept, in declaration order.
pub fn candidates(concept: Concept) -> impl Iterator<Item = &'static Line> {
    LINES.iter().filter(move |l| l.concept == concept)
}

/// All catalog lines that are a valid reply to a dispatched chain concept.
pub fn replies_to(concept: Concept) -> impl Iterator<Item = &'static Line> {
    LINES.iter().filter(move |l| l.reply_to == Some(concept))
}

/// xorshift — same as the audio module's RNG, duplicated here to keep `lines` Bevy/​dep-free.
fn next_rng(s: &mut u32) -> u32 {
    if *s == 0 {
        *s = 0x9e37_79b9;
    }
    *s ^= *s << 13;
    *s ^= *s >> 17;
    *s ^= *s << 5;
    *s
}
fn frand(s: &mut u32) -> f32 {
    (next_rng(s) & 0x00ff_ffff) as f32 / 0x00ff_ffff as f32
}

/// Pick a line for `concept`: among candidates, drop any played more recently than `floor`
/// seconds ago (per-line replay floor, keyed by `id` in `last`), random pick of the rest.
/// Returns `None` if the concept has no candidates or all are still floored.
pub fn pick_line(
    concept: Concept,
    last: &std::collections::HashMap<&'static str, f32>,
    now: f32,
    floor: f32,
    rng: &mut u32,
) -> Option<&'static Line> {
    let fresh: Vec<&'static Line> = candidates(concept)
        .filter(|l| now - *last.get(l.id).unwrap_or(&f32::NEG_INFINITY) >= floor)
        .collect();
    if fresh.is_empty() {
        return None;
    }
    let i = (frand(rng) * fresh.len() as f32) as usize % fresh.len();
    Some(fresh[i])
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    #[test]
    fn candidates_filters_by_concept() {
        // The seed catalog has exactly one LevelUp line and no ChestOpen line yet.
        assert_eq!(candidates(Concept::LevelUp).count(), 1);
        assert_eq!(candidates(Concept::ChestOpen).count(), 0);
    }

    #[test]
    fn pick_line_none_when_no_candidates() {
        let last = HashMap::new();
        let mut rng = 1;
        assert!(pick_line(Concept::ChestOpen, &last, 100.0, 300.0, &mut rng).is_none());
    }

    #[test]
    fn pick_line_respects_replay_floor() {
        let mut last = HashMap::new();
        last.insert("levelup", 50.0);
        let mut rng = 1;
        // 10s later, floor 300 → still floored → None.
        assert!(pick_line(Concept::LevelUp, &last, 60.0, 300.0, &mut rng).is_none());
        // 400s later → floor cleared → returns the line.
        assert_eq!(pick_line(Concept::LevelUp, &last, 450.0, 300.0, &mut rng).unwrap().id, "levelup");
    }

    #[test]
    fn pick_line_first_play_ignores_floor() {
        let last = HashMap::new();
        let mut rng = 1;
        assert!(pick_line(Concept::LevelUp, &last, 0.0, 300.0, &mut rng).is_some());
    }

    #[test]
    fn every_speaker_is_registered() {
        for s in [Speaker::Hero, Speaker::Villager, Speaker::Ork] {
            let _ = speaker_voice(s); // must not panic
        }
    }
}
```

- [ ] **Step 2: Run tests to verify they pass**

Run: `cargo test -p tileworld_bevy_forest lines::tests`
(If the binary crate name differs, use `cargo test lines::tests`. First run recompiles Bevy at opt-level 3 — slow once, ~2-3 min.)
Expected: 5 passed. (These are written to pass immediately — the logic is small; the value is locking behavior before the catalog grows and the director consumes it.)

- [ ] **Step 3: Commit**

```bash
git add src/audio/lines.rs
git commit -m "feat(audio): pure line resolver (candidates/pick_line) + tests"
```

---

### Task A3: Resolver — `can_play` barge-in gate (TDD)

**Files:**
- Modify: `src/audio/lines.rs`

- [ ] **Step 1: Write the failing test + impl together**

Append to `src/audio/lines.rs` (before the `#[cfg(test)]` module):

```rust
/// What a speaker is currently saying (tracked by the director's `VoiceManager`).
#[derive(Clone, Copy, Debug)]
pub struct Active {
    pub id: &'static str,
    /// `elapsed_secs` when the clip is estimated to finish.
    pub ends_at: f32,
    pub priority: u8,
    pub interruptible: bool,
    /// Chain to dispatch when it finishes (consumed once).
    pub then: Option<Chain>,
}

/// May a new line of `new_priority` start now, given the speaker's current `active` line?
/// Rule (Pixel Crushers): play if the speaker is idle, its line already finished, or the current
/// line is interruptible AND the newcomer is at least as important.
pub fn can_play(active: Option<&Active>, now: f32, new_priority: u8) -> bool {
    match active {
        None => true,
        Some(a) if now >= a.ends_at => true,
        Some(a) => a.interruptible && new_priority >= a.priority,
    }
}
```

Add these tests inside the existing `#[cfg(test)] mod tests`:

```rust
    fn active(prio: u8, interruptible: bool, ends_at: f32) -> Active {
        Active { id: "x", ends_at, priority: prio, interruptible, then: None }
    }

    #[test]
    fn can_play_when_idle() {
        assert!(can_play(None, 0.0, 0));
    }

    #[test]
    fn can_play_when_current_finished() {
        let a = active(255, false, 5.0);
        assert!(can_play(Some(&a), 6.0, 0)); // past ends_at → even a non-interruptible line is done
    }

    #[test]
    fn cannot_interrupt_protected_line() {
        let a = active(50, false, 100.0);
        assert!(!can_play(Some(&a), 1.0, 255)); // not interruptible → blocked regardless of priority
    }

    #[test]
    fn interrupt_needs_equal_or_higher_priority() {
        let a = active(50, true, 100.0);
        assert!(!can_play(Some(&a), 1.0, 49)); // lower → blocked
        assert!(can_play(Some(&a), 1.0, 50)); // equal → allowed
        assert!(can_play(Some(&a), 1.0, 200)); // higher → allowed
    }
```

- [ ] **Step 2: Run tests**

Run: `cargo test lines::tests`
Expected: 9 passed (5 from A2 + 4 here).

- [ ] **Step 3: Commit**

```bash
git add src/audio/lines.rs
git commit -m "feat(audio): can_play barge-in gate (priority + interruptible) + tests"
```

---

## Phase B — Director (Bevy glue, `cargo check` + run)

### Task B1: `Speak` message + `VoiceManager` resource + plugin wiring

**Files:**
- Create: `src/audio/director.rs`
- Modify: `src/audio/mod.rs` (uncomment `mod director;`, register message + resource + systems)

- [ ] **Step 1: Create `src/audio/director.rs`**

```rust
//! The voice director — the ONE Bevy system family that turns a [`Speak`] request into a playing
//! clip + subtitle, enforcing one line at a time per speaker with priority-gated barge-in, and
//! firing reply chains when a line ends. Replaces the bespoke mouth/cooldown bookkeeping that
//! used to live in `voice.rs`/`npc.rs`/`ork.rs`/`hero_remarks.rs`.

use std::collections::HashMap;

use bevy::audio::{PlaybackMode, Volume};
use bevy::prelude::*;

use super::lines::{
    can_play, pick_line, replies_to, speaker_voice, Active, Chain, Concept, Line, Speaker,
};
use super::AudioConfig;

/// A request to speak. Triggers (`detect_*` systems) write these; the director decides if/what
/// actually plays. `at` positions a spatial speaker (villager/ork); ignored for the head-locked
/// hero. `floor` is the per-line replay floor for this request's concept.
#[derive(Message, Clone, Copy)]
pub struct Speak {
    pub concept: Concept,
    pub at: Option<Vec3>,
    pub floor: f32,
}

impl Speak {
    pub fn new(concept: Concept) -> Self {
        Self { concept, at: None, floor: 0.0 }
    }
    pub fn at(concept: Concept, pos: Vec3) -> Self {
        Self { concept, at: Some(pos), floor: 0.0 }
    }
    pub fn floored(mut self, floor: f32) -> Self {
        self.floor = floor;
        self
    }
}

/// Marks a playing voice sink so a barge-in can stop it. Carries the speaker so we only stop the
/// right mouth.
#[derive(Component)]
pub struct VoiceSink(pub Speaker);

/// One-line-at-a-time bookkeeping for every speaker, the per-line replay floor, and the rng.
#[derive(Resource)]
pub struct VoiceManager {
    pub active: HashMap<Speaker, Active>,
    pub last_played: HashMap<&'static str, f32>,
    pub rng: u32,
    /// Pending chain dispatches: (fire_at, chain, position) queued when a line with `then` starts.
    pub pending_chains: Vec<(f32, Chain, Option<Vec3>)>,
}

impl Default for VoiceManager {
    fn default() -> Self {
        Self {
            active: HashMap::new(),
            last_played: HashMap::new(),
            rng: 0x1234_5678,
            pending_chains: Vec::new(),
        }
    }
}

impl VoiceManager {
    /// Is any NON-hero speaker mid-line right now? (Replaces the old `OthersSpeaking` resource —
    /// the hero's observational lines defer while a villager/ork is talking.)
    pub fn others_speaking(&self, now: f32) -> bool {
        self.active
            .iter()
            .any(|(s, a)| *s != Speaker::Hero && now < a.ends_at)
    }
    /// Is the hero mid-line? (Replaces `HeroSpeaking` — villagers/orks defer to him.)
    pub fn hero_speaking(&self, now: f32) -> bool {
        self.active.get(&Speaker::Hero).is_some_and(|a| now < a.ends_at)
    }
}
```

- [ ] **Step 2: Add the play helper + the director system to `director.rs`**

```rust
/// Estimate a clip's spoken length from its transcript (same model as the subtitle reader, so the
/// mouth-busy window matches the caption).
fn line_secs(text: &str) -> f32 {
    crate::subtitles::read_secs(text)
}

/// Resolve `Speak` requests into clips. For each request: pick a fresh line for the concept, check
/// the speaker's barge-in gate, and if clear, stop any current sink for that speaker and play.
pub fn speak_director(
    time: Res<Time>,
    cfg: Res<AudioConfig>,
    asset: Res<AssetServer>,
    mut commands: Commands,
    mut mgr: ResMut<VoiceManager>,
    mut reqs: MessageReader<Speak>,
    sinks: Query<(Entity, &VoiceSink)>,
    hero: Query<&crate::player::Hero>,
) {
    let now = time.elapsed_secs();
    let hero_pos = hero.single().ok().map(|h| Vec3::new(h.pos.x, 1.6, h.pos.y));

    for req in reqs.read() {
        // Pull rng out across the immutable `pick_line` borrow.
        let mut rng = mgr.rng;
        let chosen = pick_line(req.concept, &mgr.last_played, now, req.floor, &mut rng).copied();
        mgr.rng = rng;
        let Some(line) = chosen else { continue };

        if !can_play(mgr.active.get(&line.speaker), now, line.priority) {
            continue;
        }
        play_line(
            &mut commands,
            &asset,
            &cfg,
            &mut mgr,
            &sinks,
            now,
            &line,
            req.at.or(hero_pos),
        );
    }
}

/// The shared "actually play it" path (used by the director AND chain replies).
#[allow(clippy::too_many_arguments)]
fn play_line(
    commands: &mut Commands,
    asset: &AssetServer,
    cfg: &AudioConfig,
    mgr: &mut VoiceManager,
    sinks: &Query<(Entity, &VoiceSink)>,
    now: f32,
    line: &Line,
    pos: Option<Vec3>,
) {
    let voice = speaker_voice(line.speaker);
    // One mouth per speaker: stop whatever this speaker had going.
    for (e, s) in sinks {
        if s.0 == line.speaker {
            commands.entity(e).try_despawn();
        }
    }
    let dir = match line.speaker {
        Speaker::Hero => "hero",
        Speaker::Villager => "npc",
        Speaker::Ork => "ork",
    };
    let clip: Handle<AudioSource> = asset.load(format!("audio/vo/{dir}/{}.ogg", line.id));
    let dur = line_secs(line.text);
    let vol = voice.gain * cfg.voice_vol;

    let mut ent = commands.spawn((
        AudioPlayer(clip),
        PlaybackSettings {
            mode: PlaybackMode::Despawn,
            volume: Volume::Linear(vol),
            spatial: voice.spatial,
            ..default()
        },
        VoiceSink(line.speaker),
    ));
    if voice.spatial {
        ent.insert(Transform::from_translation(pos.unwrap_or(Vec3::ZERO)));
    }

    mgr.active.insert(
        line.speaker,
        Active {
            id: line.id,
            ends_at: now + dur,
            priority: line.priority,
            interruptible: line.interruptible,
            then: line.then,
        },
    );
    mgr.last_played.insert(line.id, now);
    if let Some(chain) = line.then {
        mgr.pending_chains.push((now + dur, chain, pos));
    }

    // Subtitle with speaker attribution.
    // (filled in Task D1 — for now push the bare text)
    // subs.say_as(now, voice.name, line.text, dur);
}
```

NOTE: `play_line` needs `Subtitles` — add the `subs: ResMut<crate::subtitles::Subtitles>` param to `speak_director` and thread it into `play_line` in Task D1. For B1, leave the subtitle line commented as shown so the director compiles standalone.

- [ ] **Step 3: Add the chain-tick system to `director.rs`**

```rust
/// When a line with a `then` chain finishes, dispatch the follow-up concept to its target speaker.
/// The reply is resolved against CURRENT facts here (not when the prompt started), so a stale
/// chain finds no matching reply and silently dies — the Valve "no explicit interrupt" property.
pub fn tick_chains(
    time: Res<Time>,
    cfg: Res<AudioConfig>,
    asset: Res<AssetServer>,
    mut commands: Commands,
    mut mgr: ResMut<VoiceManager>,
    sinks: Query<(Entity, &VoiceSink)>,
) {
    let now = time.elapsed_secs();
    // Take the chains whose time has come.
    let mut due: Vec<(Chain, Option<Vec3>)> = Vec::new();
    mgr.pending_chains.retain(|(at, chain, pos)| {
        if now >= *at {
            due.push((*chain, *pos));
            false
        } else {
            true
        }
    });
    for (chain, pos) in due {
        // Pick the highest-priority reply that's off its floor; replies use a short floor (30s).
        let mut rng = mgr.rng;
        // collect reply candidates and reuse pick_line by concept is not possible (reply_to, not
        // concept), so pick manually: prefer the first off-floor reply.
        let pick = replies_to(chain.concept)
            .filter(|l| now - *mgr.last_played.get(l.id).unwrap_or(&f32::NEG_INFINITY) >= 30.0)
            .max_by_key(|l| l.priority)
            .copied();
        mgr.rng = rng; // (rng unused here yet; kept for symmetry / future random tie-break)
        let _ = &mut rng;
        let Some(reply) = pick else { continue };
        if can_play(mgr.active.get(&reply.speaker), now, reply.priority) {
            play_line(&mut commands, &asset, &cfg, &mut mgr, &sinks, now, &reply, pos);
        }
    }
}
```

- [ ] **Step 4: Wire into `src/audio/mod.rs`**

Uncomment `mod director;` (from A1). In `GameAudioPlugin::build`, add:

```rust
.init_resource::<director::VoiceManager>()
.add_message::<director::Speak>()
```

and in the main `Update` tuple add `director::speak_director` and `director::tick_chains` (both `.run_if(in_state(crate::game_state::AppState::Playing))`).

- [ ] **Step 5: Verify it compiles**

Run: `cargo check`
Expected: compiles. Unused-warning on `rng` in `tick_chains` is acceptable (kept for future tie-break).

- [ ] **Step 6: Commit**

```bash
git add src/audio/director.rs src/audio/mod.rs
git commit -m "feat(audio): voice director + VoiceManager (one-mouth, barge-in, chains)"
```

---

## Phase C — Migrate each speaker into the catalog

> Each task: (a) move that speaker's lines into `LINES`, (b) replace its trigger logic with a `detect_*` system that emits `Speak`, (c) delete the old driver, (d) `cargo check`, (e) run + listen.

### Task C1: Migrate the hero event reactions (`voice.rs` `HeroEvent` lines)

**Files:**
- Modify: `src/audio/lines.rs` (add the 14 hero-event lines + `Intro`)
- Modify: `src/audio/mod.rs` (the `detect_*` systems write `Speak` instead of `AudioCue::HeroEvent`; retire `HeroEvent`/`HeroLineGates` once-per-run handling into the manager/catalog)
- Modify: `src/audio/voice.rs` (delete the `HeroEvent` + `HeroLine` arms; keep grunts/death)

- [ ] **Step 1: Add the hero event + biome + remark lines to `LINES`**

Replace the seed `LINES` const in `lines.rs` with the full hero set (transcripts copied verbatim from `voice.rs` and `hero_remarks.rs` so no quote is lost — these ARE the CLAUDE.md record now). Use `line(...)` for the interruptible prio-10 default; set `priority`/`interruptible` explicitly where the old code had special behavior:

```rust
pub const LINES: &[Line] = &[
    // ── Hero event reactions (priority 20: a deliberate beat, louder than ambient musings) ──
    line("stone", Speaker::Hero, Concept::FirstStone, "[older clip — text not transcribed]"),
    line("chest", Speaker::Hero, Concept::ChestOpen, "[older clip — text not transcribed]"),
    line("rescue", Speaker::Hero, Concept::FirstRescue, "[older clip — text not transcribed]"),
    Line { priority: 30, ..line("night", Speaker::Hero, Concept::NightWarning, "[older clip — text not transcribed]") }, // the warning must land
    line("hurt", Speaker::Hero, Concept::LowHp, "[older clip — text not transcribed]"),
    line("home", Speaker::Hero, Concept::Home, "[older clip — text not transcribed]"),
    line("equip", Speaker::Hero, Concept::Equip, "Mm, new armor. I should look it over in my satchel."),
    line("levelup", Speaker::Hero, Concept::LevelUp, "Stronger. The blade feels lighter than it did."),
    line("wave_survived", Speaker::Hero, Concept::WaveSurvived, "Dawn. We held. ...this time."),
    line("first_kill", Speaker::Hero, Concept::FirstKill, "Down it goes. Plenty more where that came from."),
    line("gold_rich", Speaker::Hero, Concept::GoldRich, "Coin enough to make the merchant smile. Good."),
    line("broke", Speaker::Hero, Concept::Broke, "Pockets empty. Steel will have to do the talking."),
    Line { priority: 25, ..line("keep_hurt", Speaker::Hero, Concept::KeepHurt, "The keep's taking a beating. Get to the walls.") },
    line("shrine_heal", Speaker::Hero, Concept::ShrineHeal, "The old stones still have mercy in them."),
    // ── Hero biome musings (one concept, five biomes; quieter — narration volume handled by speaker gain later) ──
    line("forest", Speaker::Hero, Concept::BiomeEntered(Biome::Forest), "[older clip — text not transcribed]"),
    line("snow", Speaker::Hero, Concept::BiomeEntered(Biome::Snow), "[older clip — text not transcribed]"),
    line("rock", Speaker::Hero, Concept::BiomeEntered(Biome::Rocky), "[older clip — text not transcribed]"),
    line("desert", Speaker::Hero, Concept::BiomeEntered(Biome::Desert), "[older clip — text not transcribed]"),
    line("swamp", Speaker::Hero, Concept::BiomeEntered(Biome::Swamp), "[older clip — text not transcribed]"),
];
```

(Hero observational remarks + villager + ork lines are added in C2/C3/C4. The biome clips load from `audio/vo/hero/<id>.ogg` under the new scheme — the old paths were `audio/vo/forest.ogg`. **Either** move the files to `assets/audio/vo/hero/` **or** special-case the hero biome/event dir. Decision: move the existing `assets/audio/vo/*.ogg` into `assets/audio/vo/hero/` so the `<speaker>/<id>.ogg` rule is uniform. List + move them in Step 2.)

- [ ] **Step 2: Relocate the existing hero VO assets to the uniform path**

The old hero biome/event clips live directly in `assets/audio/vo/`. Move them under `hero/` so `play_line`'s `audio/vo/hero/<id>.ogg` finds them:

```powershell
$src = "assets/audio/vo"
$dst = "assets/audio/vo/hero"
New-Item -ItemType Directory -Force $dst
foreach ($f in @("forest","snow","rock","desert","swamp","stone","chest","rescue","night","hurt","home","equip","levelup","wave_survived","first_kill","gold_rich","broke","keep_hurt","shrine_heal")) {
    $p = Join-Path $src "$f.ogg"
    if (Test-Path $p) { Move-Item $p (Join-Path $dst "$f.ogg") -Force }
}
```

(Verify which actually exist first with `Get-ChildItem assets/audio/vo/*.ogg`. `hero_remarks.rs` already used `audio/vo/hero/<key>.ogg`, so that dir exists.)

- [ ] **Step 3: Point the trigger systems at `Speak`**

In `src/audio/mod.rs`, change every `cues.write(AudioCue::HeroEvent(HeroEvent::X))` to `speak.write(Speak::new(Concept::X).floored(EVENT_FLOOR))` (add `mut speak: MessageWriter<director::Speak>` to `detect_player_events`, `detect_home_return`, `detect_siege_voice`, `detect_equip`, and the biome one to `Speak::new(Concept::BiomeEntered(b))`). The once-per-run gates (`first_stone`/`first_rescue`/`home`/`equip`/`first_kill`/`gold_rich`) stay in `HeroLineGates` and keep their edge-detection — they just emit `Speak` instead. Define `const EVENT_FLOOR: f32 = 300.0;` (the old `EVENT_REPLAY_GAP`); use `0.0` for once-per-run lines (the gate already limits them) and the night warning.

- [ ] **Step 4: Strip the migrated arms from `voice.rs`**

Delete the `AudioCue::HeroLine(b)` and `AudioCue::HeroEvent(ev)` match arms and the `VoiceBank.lines`/`events` fields + their loads. Keep `swings`/`jump`/`hurts`/`deaths` and the grunt/death arms — those stay reflex sfx (NOT catalog lines; they have no transcript). `HeroDeath` should now ALSO clear `VoiceManager.active.get(Hero)` so a death cry interrupts a musing: simplest is to have `HeroDeath` emit nothing to the manager but still despawn `VoiceSink(Hero)` — OR leave the death cry on the old grunt path and add a `Concept`-free direct stop. Keep it on the grunt path; add to the `HeroDeath` arm: `for (e,s) in &sinks { if s.0==Speaker::Hero { commands.entity(e).try_despawn(); } }` (requires querying `VoiceSink`).

- [ ] **Step 5: Verify**

Run: `cargo check`
Expected: compiles. Then `cargo run` and confirm: level-up / first-kill / biome-entry lines still speak, and a death cry cuts a musing.

- [ ] **Step 6: Commit**

```bash
git add -A
git commit -m "refactor(audio): migrate hero event + biome lines to the catalog"
```

---

### Task C2: Migrate hero observational remarks (`hero_remarks.rs`)

**Files:**
- Modify: `src/audio/lines.rs` (add the `REMARKS` + `INTRO` lines under their concepts)
- Create: a `detect_hero_remarks` system (in `mod.rs` or a small `remarks_trigger.rs`) that computes the proximity/phase trigger and emits `Speak` with the matching `Concept` + a `LINE_FLOOR` of 300.
- Delete: `src/audio/hero_remarks.rs` (and its `setup`/`tick`/`reset` registrations in `mod.rs`)

- [ ] **Step 1: Add remark lines to `LINES`**

For each `(Trig, key, text)` in the old `REMARKS` table, add `line(key, Speaker::Hero, <Concept>, text)` where `Trig::Town→NearTown`, `Kids→NearKids`, `Pet→NearPet`, `Guard→NearGuard`, `Keep→InKeep`, `Night→NightMusing`, `Quiet→QuietMusing`, `Kill→KillMusing`. Add the two `INTRO` lines under `Concept::Intro` with `Line { priority: 5, interruptible: true, ..}` (low priority so anything interrupts the tutorial, but it plays in calm). Copy transcripts verbatim from `hero_remarks.rs:53-99`.

- [ ] **Step 2: Port the trigger into a `detect_hero_remarks` system**

Lift the proximity/phase logic from `hero_remarks::tick` (the `near_kids`/`near_guard`/`near_town`/`near_pet`/`in_keep`/`orks_near` computation + the `trig` selection + the intro timing) into a new system that, instead of playing, emits `Speak::new(Concept::X).floored(300.0)`. Drop the `play()`/`RemarkBank`/`RemarkState` machinery — the director owns playback + the floor now (`VoiceManager.last_played`). The `HeroLineAnchor` "cut when he walks away" behavior: re-add as an optional `anchor` on `Active` if you want to keep it (see Task D2); for C2 it's acceptable to drop the walk-away cut and rely on the line finishing — note this in the commit as a deliberate simplification to revisit in D2.

- [ ] **Step 3: Delete `hero_remarks.rs` + deregister**

Remove `mod hero_remarks;`, `hero_remarks::setup/tick/reset` from the plugin, add the new `detect_hero_remarks` to the `Playing`-gated Update set.

- [ ] **Step 4: Verify**

Run: `cargo check` then `cargo run`. Confirm remarks fire near townsfolk/kids/guards and in the keep, and the intro plays once at start. (Remark VO may be absent assets — the director's `asset.load` of a missing file plays nothing, which matches the old "inert until oggs exist" behavior, BUT the subtitle will now show. If silent-clip-with-subtitle is unwanted, gate on `Assets::<AudioSource>::get` like the old `play()` did — add that check in D1.)

- [ ] **Step 5: Commit**

```bash
git add -A
git commit -m "refactor(audio): migrate hero observational remarks to the catalog"
```

---

### Task C3: Migrate villager lines (`npc.rs`)

**Files:**
- Modify: `src/audio/lines.rs` (21 ambient + 3 event villager lines)
- Create: `detect_villager_voice` system (proximity ambient + phase/rescue events) emitting `Speak::at(concept, who_pos)`
- Delete: the line/selection bodies of `npc.rs` (keep `nearest_villager` helper — move to `director.rs` or the new trigger module)

- [ ] **Step 1: Add villager lines to `LINES`**

Add the 21 ambient lines as `line(key, Speaker::Villager, Concept::Greeting, text)` (all under one `Greeting` concept; the shuffle-bag behavior becomes the resolver's random pick + the per-line floor). Copy keys from `AMBIENT_KEYS` and text from `AMBIENT_TEXT` (now unified into one entry each — this kills the fragile parallel arrays). Add the 3 event lines: `siege_fear→SiegeFalls`, `dawn_relief→Dawn`, `rescued→Rescued`. **Make `pa_*` jabs chainable** (the demo for D3): on `pa_hero`/`pa_chosen` etc. set `then: Some(Chain { concept: Concept::ReplyToVillagerJab, target: Speaker::Hero })`.

- [ ] **Step 2: Port the trigger**

New `detect_villager_voice`: keep `npc_ambient`'s gating (nearest worker within `NEAR_DIST`, `AMBIENT_GAP` global cadence, `SPEAK_CHANCE`, the `NEEDS_WEAPON` skip), but instead of playing, emit `Speak::at(Concept::Greeting, who_pos3).floored(600.0)`. The cadence gate (`next_ambient`) stays in this system (it's a trigger concern, not a manager concern). Event lines (`npc_events`) → emit `Speak::at(Concept::SiegeFalls/Dawn/Rescued, who_pos)`. The `armed`/weapon gate for `pa_sword`: since all greetings share one concept now, keep the weapon check by *not emitting* on a frame the bag would pick it — simpler: drop the per-line weapon gate for v1 (note in commit) OR add a `requires_weapon` bool to `Line` and filter in `pick_line` (cleaner; add it if quick).

- [ ] **Step 3: Delete `npc.rs` bodies + deregister**

Remove `npc_ambient`/`npc_events`/`NpcVoiceBank`/`NpcVoiceState`/the shuffle-bag. Move `nearest_villager` to the trigger module. Deregister `npc::*` from the plugin, register `detect_villager_voice`.

- [ ] **Step 4: Verify**

Run: `cargo check` then `cargo run`. Walk near a worker — confirm chatter still fires, subtitled as "Townsfolk: …", not over the hero, and no two nearby villagers overlap (the manager's one-Villager-mouth covers this globally; the old `CLOSE_SPEAKER_DIST` per-pair check is now a global one-villager-at-a-time — note this behavior change).

- [ ] **Step 5: Commit**

```bash
git add -A
git commit -m "refactor(audio): migrate villager ambient + event lines to the catalog"
```

---

### Task C4: Migrate ork barks (`ork.rs`)

**Files:**
- Modify: `src/audio/lines.rs` (7 battle barks + death snarl)
- Create: `detect_ork_voice` system (nearest ork in earshot, global bark cadence, death-snarl chance) emitting `Speak::at`
- Delete: the bodies of `ork.rs`

- [ ] **Step 1: Add ork lines to `LINES`**

Add `line(key, Speaker::Ork, Concept::OrkSpot, text)` for the 7 `BATTLE_KEYS` (transcripts from `ork.rs:38-45`), and `line("death", Speaker::Ork, Concept::OrkDeath, "Not done.")`. Pitch-jitter (`PlaybackSettings::speed`) is a director concern — add an optional `pitch_jitter: f32` field to the `Speak` request (default 0), set it in the ork trigger, and apply `speed = 1.0 + (frand-0.5)*2*jitter` in `play_line`. (Keep `pitch_jitter` on `Speak`, not `Line`, since it's a play-time effect.)

- [ ] **Step 2: Port the trigger**

New `detect_ork_voice`: keep `ork_voices`'s `BARK_GAP`+jitter cadence, `EARSHOT`, `DEATH_CHANCE`. Emit `Speak::at(Concept::OrkDeath, gt)` for a fresh death (chance-gated) else `Speak::at(Concept::OrkSpot, nearest_pos)`. Set `pitch_jitter: 0.18`.

- [ ] **Step 3: Delete `ork.rs` bodies + deregister**, register `detect_ork_voice`.

- [ ] **Step 4: Verify**

Run: `cargo check` then `cargo run` (or `FOREST_WAVE=1` shot). Trigger a wave — confirm ork barks/death snarls still play, pitch-varied, spatial, deferring to the hero.

- [ ] **Step 5: Commit**

```bash
git add -A
git commit -m "refactor(audio): migrate ork barks to the catalog; retire per-module voice drivers"
```

---

## Phase D — Polish: subtitle attribution, anchors, chain demo

### Task D1: Subtitle speaker attribution + missing-clip guard

**Files:**
- Modify: `src/subtitles.rs` (add `say_as`)
- Modify: `src/audio/director.rs` (`play_line` pushes `say_as`; skip subtitle if the clip isn't loaded)

- [ ] **Step 1: Add `say_as` to `subtitles.rs`**

```rust
impl Subtitles {
    /// Show `text` attributed to `speaker` (e.g. "Townsfolk: …"); `None` = no prefix (the hero).
    pub fn say_as(&mut self, now: f32, speaker: Option<&str>, text: &str, dur: f32) {
        self.text = match speaker {
            Some(name) => format!("{name}: {text}"),
            None => text.to_string(),
        };
        self.until = now + dur;
    }
}
```

(Keep the old `say` as `self.say_as(now, None, text, dur)` for any remaining callers.)

- [ ] **Step 2: Wire it into `play_line`**

Add `subs: &mut crate::subtitles::Subtitles` param to `play_line` and `subs: ResMut<...>` to `speak_director`/`tick_chains`. Before spawning, guard on the clip being loaded so a missing ogg neither plays a silent sink nor shows a caption-without-voice (matches old `hero_remarks` behavior):

```rust
// requires `sources: &Assets<AudioSource>` param threaded from the systems
if sources.get(&clip).is_none() {
    return; // asset not present/loaded yet → stay silent, no subtitle
}
```

Then after spawning: `subs.say_as(now, voice.name, line.text, dur);`

- [ ] **Step 3: Verify**

Run: `cargo check` then `cargo run`. Confirm villager/ork lines now show "Townsfolk: …" captions (orks had none before), hero lines show bare text, and a speaker with no ogg yet shows no caption.

- [ ] **Step 4: Commit**

```bash
git add -A
git commit -m "feat(audio): subtitle speaker attribution + missing-clip guard"
```

---

### Task D2: Restore the walk-away anchor (optional, if C2 dropped it)

**Files:**
- Modify: `src/audio/lines.rs` (add `anchor: Option<Anchor>` to `Active`; define `Anchor`)
- Modify: `src/audio/director.rs` (a system that fades/cuts an anchored active line when the hero leaves)

- [ ] **Step 1:** Re-introduce `HeroLineAnchor`'s two cases as a `lines::Anchor { Near{pos:[f32;2], r:f32}, Biome(Biome) }`, carry it on `Active` (set from a new optional `Line.anchor` or computed in the trigger and passed via `Speak`). Port `stop_displaced_hero_lines` + `fade_out_hero_lines` from `mod.rs` to act on `VoiceSink(Hero)` whose `Active.anchor` says he's left. Keep `FINISH_GRACE`.

- [ ] **Step 2:** `cargo check` + `cargo run`; confirm a biome musing cuts when he crosses out, a remark cuts when he walks off.

- [ ] **Step 3: Commit** `refactor(audio): restore walk-away anchor on catalog lines`

---

### Task D3: Wire ONE call-and-response chain end to end (the demo)

**Files:**
- Modify: `src/audio/lines.rs` (add 2-3 hero `reply_to: Some(ReplyToVillagerJab)` lines)

- [ ] **Step 1: Add hero replies**

```rust
    // ── Hero replies to a villager's passive-aggressive jab (chained via `then`) ──
    Line { reply_to: Some(Concept::ReplyToVillagerJab), priority: 15, ..line("reply_jab_a", Speaker::Hero, Concept::ReplyToVillagerJab, "Mm. Remind me again who sleeps behind these walls.") },
    Line { reply_to: Some(Concept::ReplyToVillagerJab), priority: 15, ..line("reply_jab_b", Speaker::Hero, Concept::ReplyToVillagerJab, "Keep talking. The orks find it funny too.") },
```

(`concept` and `reply_to` both `ReplyToVillagerJab` is fine — `concept` is unused for reply-only lines, `replies_to()` keys on `reply_to`.)

- [ ] **Step 2: Verify the chain**

Run: `cargo run`. Stand near a worker until a `pa_*` jab fires (it has `then: ReplyToVillagerJab → Hero`); ~that line's length later, the hero should fire a reply IF he's still idle and nothing changed. Walk away mid-jab and confirm the reply does NOT fire (chain self-terminates). Add the hero reply oggs to `assets/audio/vo/hero/reply_jab_*.ogg` (or accept subtitle-only until recorded — the guard in D1 keeps it silent-but-captioned... actually silent+no-caption; to preview the chain logic before VO exists, temporarily comment the missing-clip guard).

- [ ] **Step 3: Commit** `feat(audio): call-and-response chain (villager jab → hero retort)`

---

## Self-Review notes (author checklist, done)

- **Spec coverage:** (1) speaker/owner → `Line.speaker` + `SPEAKERS` (A1); (2) transcript as data → `Line.text`, surfaced via `say_as` (A1/D1); (3) interruptible+priority → `Line` fields + `can_play` (A1/A3); (4) chains → `Chain`/`then`/`reply_to` + `tick_chains` (A1/B1/D3). All four covered.
- **Type consistency:** `Speaker`/`Concept`/`Line`/`Chain`/`Active` defined once in A1/A3; `pick_line`/`can_play`/`candidates`/`replies_to` signatures fixed in A2/A3 and used unchanged in B1. `speak_director`/`play_line`/`tick_chains` share `play_line`. `Speak` builder (`new`/`at`/`floored`) consistent across C1-C4; `pitch_jitter` added to `Speak` in C4.
- **Known deliberate simplifications (called out in their tasks):** villager one-mouth becomes global (was per-pair distance); per-line weapon gate for `pa_sword` may drop unless `requires_weapon` added; walk-away anchor deferred to D2. None block a working build.
- **Asset note:** C1 Step 2 moves existing hero oggs to `assets/audio/vo/hero/`; villager (`vo/npc/`) and ork (`vo/ork/`) dirs already match the `<speaker>/<id>.ogg` scheme.
```
