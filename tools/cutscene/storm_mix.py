"""P.1 "Piorun": the soundtrack of `FOREST_SCENE=storm` (src/scenes/storm.rs), mixed offline against the
scene clock. No score: the storm, the sea, the orks, Wat's breath, one line from Maud.

Also cuts the uploaded takes into the asset tree (Wat's breaths `assets/audio/vo/campaign/p1_wat_NN.ogg`,
the cutscene SFX `assets/audio/sfx/cutscene/cs_*.ogg`) and writes the mix the in-game cue plays
(`assets/audio/cutscene/storm_mix.ogg`).

Usage:  python tools/cutscene/storm_mix.py <out_dir> [video.mp4 out.mp4] [--cut]
        (with a video: muxes the mix under it; --cut re-cuts the uploaded takes into assets/)

Timeline constants mirror storm.rs (`at::*`, `STRIKES`) -- keep the two in step.
"""
import os, subprocess, sys
import numpy as np, soundfile as sf
from scipy.signal import butter, sosfilt, sosfiltfilt, fftconvolve

OUT = sys.argv[1]
os.makedirs(OUT, exist_ok=True)
REPO = os.path.dirname(os.path.dirname(os.path.dirname(os.path.abspath(__file__))))
A = os.path.join(REPO, "assets", "audio") + "/"
U = "C:/Users/skibi/.claude/uploads/1beee172-f53a-4969-af9e-54051690752a/"   # P.1 takes (2026-10-26)
U0 = "C:/Users/skibi/.claude/uploads/8e2e82b7-28fb-4c50-9c5e-c561e0e56d41/"  # P.0 thunder takes
SR = 48000
END = 59.6
N = int(END * SR)
rng = np.random.default_rng(1)

# ---- the scene's timeline (storm.rs) ---------------------------------------------------------
DECK, BEACH, STAND, HARROW, HELM, CODA = (4.0, 7.7), (12.0, 20.6), (21.6, 28.0), (28.6, 33.5), (37.6, 44.3), (46.6, 58.4)
STRIKES = [(4.05, 1.0), (5.95, 0.55), (12.05, 1.0), (16.6, 0.42), (20.22, 0.9), (21.62, 1.0), (22.62, 0.65), (23.35, 0.5),
           (24.95, 0.95), (25.75, 0.75), (28.65, 1.0), (30.9, 0.8), (37.65, 0.85), (40.9, 0.8), (46.65, 0.6), (50.0, 0.35), (56.7, 0.9)]


def load(p, ch=2):
    raw = subprocess.run(["ffmpeg", "-v", "error", "-i", p, "-ac", str(ch), "-ar", str(SR), "-f", "f32le", "-"],
                         capture_output=True, check=True).stdout
    x = np.frombuffer(raw, np.float32).copy()
    return x if ch == 1 else x.reshape(-1, ch)


def db(x):
    return 10 ** (x / 20)


def st(x):
    return x if x.ndim == 2 else np.stack([x, x], 1)


def lufs(x, target, tag):
    """Loudness-normalise (ffmpeg loudnorm, two channels)."""
    p = f"{OUT}/_{tag}.wav"
    sf.write(p, st(x), SR)
    subprocess.run(["ffmpeg", "-y", "-v", "error", "-i", p, "-af", f"loudnorm=I={target}:TP=-1.5:LRA=20:linear=true",
                    "-ar", str(SR), f"{OUT}/_{tag}_n.wav"], check=True)
    y, _ = sf.read(f"{OUT}/_{tag}_n.wav")
    return st(y.astype(np.float32))


def filt(x, kind, hz, order=2):
    sos = butter(order, hz, kind, fs=SR, output="sos")
    return sosfiltfilt(sos, x, axis=0).astype(np.float32)


def place(buf, x, t0, gain=1.0):
    x = st(x)
    i = int(t0 * SR)
    j0 = max(0, -i)
    n = min(len(x) - j0, len(buf) - max(i, 0))
    if n > 0:
        buf[max(i, 0):max(i, 0) + n] += x[j0:j0 + n] * gain


def sched(points):
    """[(t, dB), ...] -> per-sample gain."""
    ts = np.array([p[0] for p in points]) * SR
    ds = np.array([p[1] for p in points])
    return db(np.interp(np.arange(N), ts, ds)).astype(np.float32)[:, None]


def loop_to(x, n, xf=0.5):
    x = st(x)
    xf = int(xf * SR)
    out = np.zeros((n, 2), np.float32)
    pos = 0
    r = np.linspace(0, 1, xf)[:, None]
    while pos < n:
        seg = x.copy()
        if pos > 0:
            seg[:xf] *= r
            out[pos:pos + xf] *= (1 - r)[: len(out[pos:pos + xf])]
        k = min(len(seg), n - pos)
        out[pos:pos + k] += seg[:k]
        pos += len(seg) - xf
    return out


def fade(x, a=0.01, b=0.03):
    x = st(x).copy()
    na, nb = int(a * SR), int(b * SR)
    x[:na] *= np.linspace(0, 1, na)[:, None]
    x[-nb:] *= np.linspace(1, 0, nb)[:, None]
    return x


def noise(n, color):
    """Stereo noise shaped in the spectrum: 'pink' 1/f, 'brown' 1/f^2 (decorrelated channels)."""
    out = []
    for _ in range(2):
        w = rng.standard_normal(n)
        f = np.fft.rfft(w)
        k = np.arange(len(f)) + 1.0
        f /= k ** (0.5 if color == "pink" else 1.0)
        y = np.fft.irfft(f, n)
        out.append(y / np.abs(y).max())
    return np.stack(out, 1).astype(np.float32)


def snap(v, t, w=0.12):
    """Quietest 10 ms frame within +-w of t (a cut inside a breath's gap, not on it)."""
    a, b, f = int((t - w) * SR), int((t + w) * SR), 480
    best, bi = 9e9, a
    for i in range(max(a, 0), min(b, len(v)) - f, 120):
        r = float(np.sqrt((v[i:i + f] ** 2).mean()))
        if r < best:
            best, bi = r, i
    return (bi + f // 2) / SR


def reverb(x, secs=1.6, wet=0.35):
    n = int(secs * SR)
    ir = rng.standard_normal((n, 2)).astype(np.float32) * np.exp(-np.linspace(0, 7, n))[:, None]
    ir = filt(ir, "low", 3500)
    x = st(x)
    y = np.stack([fftconvolve(x[:, c], ir[:, c])[: len(x) + n] for c in range(2)], 1)
    y = np.pad(y, ((0, len(x) + n - len(y)), (0, 0)))
    y /= np.abs(y).max() + 1e-9
    dry = np.pad(x, ((0, n), (0, 0)))
    return (dry * (1 - wet) + y * wet * np.abs(x).max()).astype(np.float32)


def export(x, path, mono, target):
    """A take into the asset tree: loudness-normalised Vorbis."""
    tmp = f"{OUT}/_exp.wav"
    sf.write(tmp, st(x), SR)
    ch = "1" if mono else "2"
    subprocess.run(["ffmpeg", "-y", "-v", "error", "-i", tmp, "-af", f"loudnorm=I={target}:TP=-1:LRA=20,alimiter=limit=0.89",
                    "-ac", ch, "-ar", "44100", "-c:a", "libvorbis", "-q:a", "5", path], check=True)


# ---- the takes (all from the asset tree; storm_mix.py --cut re-cuts the uploads) --------------
VO = A + "vo/campaign/"
CS = A + "sfx/cutscene/"
if "--cut" in sys.argv:
    # Wat (ElevenLabs v3, Sully, one generation of the three breaths, cut at its own gaps):
    WAT = U + "f7a87374-ElevenLabs_2026-10-26T07_12_30_Sully_-_Mature_Deep_and_Intriguing_pvc_sp100_s50_sb75_v3.mp3"
    w = load(WAT, 1)
    c1, c2, c3a, c3b = snap(w, 3.62), snap(w, 8.42, 0.05), snap(w, 9.05, 0.08), snap(w, 11.36, 0.06)
    export(fade(w[:int(c1 * SR)]), VO + "p1_wat_01.ogg", True, -16)                # [gasps sharply] Hhh! [panting] Hhh... hhh...   (waking to the reef)
    export(fade(w[int(c1 * SR):int(c2 * SR)]), VO + "p1_wat_02.ogg", True, -16)    # [coughing] Khh- khh. [gasping for air] Hhhah... (on his knees in the sand)
    export(fade(w[int(c3a * SR):int(c3b * SR)]), VO + "p1_wat_03.ogg", True, -16)  # [strained, quiet] Hnnh. [exhales]           (the strap pulled home)
    for name, f in (("cs_plate_wet", "e0f5e0e4-Firefly_audio_a_heavy_steel_shoulder_plate_lifted_from_wet_sand__variation4.wav"),
                    ("cs_sailors_panic", "31391482-several_sailors_shou_2-1792998821354.mp3")):
        export(load(U + f), CS + name + ".ogg", False, -18)
    export(load(U + "b3201920-a_ship_s_brass_bell__3-1792998942570.mp3")[: int(2.85 * SR)], CS + "cs_ship_bell_alarm.ogg", False, -18)
    # The P.1 dialogue session (ElevenLabs, one export, the voices per line): cut at its gaps.
    dlg = load(U + "93739048-ElevenLabs_2026-10-26T09_11_58_Azrak_-_Menacing_Monster_pvc_sp88_s45_sb37_v4.mp3", 1)
    k = [0.0, snap(dlg, 2.39, 0.2), snap(dlg, 4.88, 0.08), snap(dlg, 7.01, 0.12), snap(dlg, 11.48, 0.15)]
    for (name, target), (a, b) in zip((("p1_sailor_02", -15), ("p1_wat_04", -18), ("p1_sailor_01", -15), ("p1_maud_01", -16)), zip(k, k[1:])):
        export(fade(dlg[int(a * SR):int(b * SR)], 0.01, 0.06), VO + name + ".ogg", True, target)
        # p1_sailor_02 Julian:    [shouting over the storm] "Ring it! All hands, brace!"
        # p1_wat_04    Sully:     [whispering, shaken] "Ser...? Ser Godric."
        # p1_sailor_01 Wilf:      [shouting, panicked] "Reef! Reef off the bow!"
        # p1_maud_01   MoanElisa: [low, urgent] "He's gone, Wat. Put it on. They need a knight standing."
    export(load(U0 + "ef362061-Firefly_audio_Powerful_realistic_thunderclap_deep_explosive_cra_variation4.wav"), CS + "cs_thunder_crack.ogg", False, -16)
    export(load(U0 + "f728e8b1-Firefly_audio_Powerful_realistic_thunderclap_deep_explosive_cra_variation3.wav"), CS + "cs_thunder_roll.ogg", False, -16)


def opt(path, ch=2):
    """A take that may not be recorded yet (the prompts in docs/audio/elevenlabs-p1-piorun.md)."""
    return load(path, ch) if os.path.exists(path) else None


wat1, wat2, wat3 = (lufs(load(VO + f"p1_wat_0{k}.ogg", 1), -16, f"w{k}") for k in (1, 2, 3))
plate = load(CS + "cs_plate_wet.ogg")
sailors = load(CS + "cs_sailors_panic.ogg")
bell = load(CS + "cs_ship_bell_alarm.ogg")
th_crack = load(CS + "cs_thunder_crack.ogg")
th_roll = load(CS + "cs_thunder_roll.ogg")
maud = load(VO + "a1_maud_04.ogg", 1)        # "Ser Godric's up! Behind him, all of you!"
tighter = load(VO + "p0_cabin_01.ogg", 1)    # Harrow, P.0: "Tighter."
# not recorded yet -> skipped
sailor1 = opt(VO + "p1_sailor_01.ogg", 1)    # [shouting, panicked] "Reef! Reef off the bow!"
sailor2 = opt(VO + "p1_sailor_02.ogg", 1)    # [shouting over the storm] "Ring it! All hands, brace!"
wat4 = opt(VO + "p1_wat_04.ogg", 1)          # [whispering, shaken] "Ser...? Ser Godric."
maud1 = opt(VO + "p1_maud_01.ogg", 1)        # [low, urgent] "He's gone, Wat. Put it on. They need a knight standing."
hull = opt(CS + "cs_hull_reef.ogg")
wave_deck = opt(CS + "cs_wave_deck.ogg")
draw = opt(CS + "cs_sword_draw.ogg")
clash = opt(CS + "cs_shield_clash.ogg")
# the orks' voice: the game's ork bank (three ElevenLabs SFX takes cut in seven, `audio/sfx.rs`)
VOICES = {k: load(A + f"ork-voice-{k}.ogg", 1) for k in ("a1", "a2", "a3", "d1", "d2", "w1", "w2")}
ROARS, BARKS = ["a1", "a3", "d1", "w1"], ["a2", "w2"]
grunt1, grunt2 = load(A + "ork-voice-g1.ogg", 1), load(A + "ork-voice-g2.ogg", 1)   # an ork taking a blade
band_far = load(CS + "cs_ork_band_far.ogg")


def rate(x, r):
    """Resample by `r` (>1 higher and shorter): a different throat from the same take."""
    x = st(x)
    n = int(len(x) / r)
    src = np.linspace(0, len(x) - 1, n)
    return np.stack([np.interp(src, np.arange(len(x)), x[:, c]) for c in range(2)], 1).astype(np.float32)


def pan(x, p):
    x = st(x)
    return x * np.array([min(1.0, 1 - p), min(1.0, 1 + p)], np.float32)


def far(x, k):
    """Distance `k` 0 (at the ear) .. 1 (up the beach): duller and quieter."""
    return filt(st(x), "low", 6000 - 5000 * k) * db(-14 * k)


def ork(key, semis, k, p=0.0, target=-18, tag="o"):
    """One ork's cry, made into a throat of its own and put in the scene: pitched (`semis`), doubled
    an octave-and-a-bit down under itself (a chest under the shout, so no single voice actor reads
    through), driven a little into saturation, the fizz above 6.5 kHz taken off, a short room, then
    distance `k` (0 at the ear .. 1 up the beach) and pan `p`."""
    x = st(VOICES[key])
    hi = rate(x, 2 ** (semis / 12))
    lo = rate(x, 2 ** ((semis - 7) / 12))
    n = max(len(hi), len(lo) + int(0.012 * SR))
    y = np.zeros((n, 2), np.float32)
    y[: len(hi)] += hi
    y[int(0.012 * SR): int(0.012 * SR) + len(lo)] += lo * db(-7)
    y = np.tanh(2.2 * y / (np.abs(y).max() + 1e-9)) / np.tanh(2.2)
    y = filt(y, "low", 6500) + 0.35 * filt(y, "low", 220)
    y = reverb(y, 0.7, 0.18)
    return pan(far(lufs(y, target, tag), k), p)


def timed(x, t0, pts):
    """A clip that will be placed at `t0`, under a gain schedule written on the scene clock."""
    x = st(x)
    g = sched(pts)[int(t0 * SR): int(t0 * SR) + len(x)]
    return x[: len(g)] * g


def smear(x, secs=1.4):
    """Blur a bed: duller, a long wash of room under it, two offset copies -- feet you feel, not count."""
    x = filt(st(x), "low", 1600)
    y = x + np.roll(x, int(0.11 * SR), 0) * 0.7 + np.roll(x, int(0.23 * SR), 0) * 0.5
    return reverb(y, secs, 0.45)


def sweep_down(x, t0, t1, lo=240):
    """A low-pass closing from open to `lo` Hz between t0 and t1 (the sea closing over his head)."""
    bands = [x, filt(x, "low", 2200, 4), filt(x, "low", 800, 4), filt(x, "low", lo, 4)]
    u = np.clip((np.arange(N) / SR - t0) / (t1 - t0), 0, 1) * 3
    out = np.zeros_like(x)
    for i, b in enumerate(bands):
        w = np.clip(1 - np.abs(u - i), 0, 1)[:, None]
        out += b * w
    return out


# ---- beds -------------------------------------------------------------------------------------
# Rain on the open: pink noise for the wash, sparse filtered clicks for the drops near the ear.
wash = filt(noise(N, "pink"), "band", [350, 9000])
drops = np.zeros((N, 2), np.float32)
idx = rng.integers(0, N, int(END * 700))
drops[idx, rng.integers(0, 2, len(idx))] = rng.uniform(0.2, 1.0, len(idx))
drops = filt(drops, "high", 2500)
rain = lufs(wash + 0.6 * drops / (np.abs(drops).max() + 1e-9), -24, "rain")
# The sea in a storm: brown-noise body under wave sets, each a slow rise, a crash (foam hiss) and the
# drag back; and the game's own water bed, low.
brown = filt(noise(N, "brown"), "low", 520)
hiss = filt(noise(N, "pink"), "band", [900, 7000])
env, foam = np.zeros(N, np.float32), np.zeros(N, np.float32)
t = 0.3
while t < END:
    per = rng.uniform(5.5, 8.5)
    a, b, c = int(t * SR), min(int((t + 1.6) * SR), N), min(int((t + 1.6 + 3.2) * SR), N)
    env[a:b] = np.maximum(env[a:b], np.linspace(0.25, 1.0, int(1.6 * SR))[: b - a])
    env[b:c] = np.maximum(env[b:c], np.linspace(1.0, 0.25, c - b) ** 1.5)
    foam[b:c] = np.maximum(foam[b:c], np.exp(-np.linspace(0, 5, c - b)))
    t += per
env = np.maximum(env, 0.25)
sea = lufs(brown * env[:, None] + 0.5 * hiss * foam[:, None], -22, "sea")
water = lufs(filt(loop_to(load(A + "water.ogg"), N), "low", 1400), -30, "water")
wind = lufs(loop_to(load(A + "wind.ogg"), N, 1.0), -27, "wind")
cabin = lufs(loop_to(load(A + "sfx/cutscene/cs_ship_cabin.ogg"), N, 0.4), -24, "cabin")
fire = lufs(filt(loop_to(load(A + "campfire-loop.ogg"), N, 0.3), "low", 5000), -30, "fire")
sub = np.zeros((N, 2), np.float32)
# the orks' running feet: one take, smeared and kept low under everything (placed per shot)
run_bed = lufs(smear(loop_to(load(CS + "cs_orks_run.ogg"), int(14 * SR), 0.6)), -27, "run")


def thunder(buf, t, k):
    """A strike's thunder: close ones crack with the flash, weaker ones roll in behind it."""
    if k >= 0.8:
        place(buf, lufs(th_crack, -15 - 6 * (1 - k), f"tc{t}"), t + 0.03)
    else:
        place(buf, lufs(filt(th_roll, "low", 900 + 2500 * k), -26 + 10 * k, f"tr{t}"), t + 0.35)


# ---- 1. below deck and the deck ----------------------------------------------------------------
# Black: the cabin's creak and rain; a shout from above; the reef. The hull splits under a crack of
# thunder, the bell goes, men shout; Wat wakes. Then the deck: bell, wind, the sea over the rail.
HIT = 2.45
D = np.zeros((N, 2), np.float32)
for ts, k in STRIKES:
    if ts < 9.0:
        thunder(D, ts, k)
if sailor1 is not None:
    place(D, filt(lufs(sailor1, -20, "s1"), "low", 1800), HIT - len(sailor1) / SR - 0.15)   # heard through the deck
if hull is not None:
    place(D, lufs(hull, -12, "hull"), HIT - 0.05)
place(D, lufs(filt(load(A + "wood-crack.ogg"), "low", 2600), -13, "crack"), HIT)
place(D, lufs(load(A + "wood-crack.ogg")[::-1].copy(), -24, "crackr"), HIT + 0.35)
place(D, lufs(load(A + "body-thud-heavy-1.ogg"), -14, "thud"), HIT)
place(D, lufs(th_crack, -14, "hitth"), HIT + 0.02)
boom = np.sin(2 * np.pi * np.cumsum(np.linspace(55, 28, int(1.4 * SR))) / SR) * np.exp(-np.linspace(0, 4, int(1.4 * SR)))
place(sub, boom.astype(np.float32), HIT, 0.5)
place(D, wat1, HIT + 0.22)
# the bell, rung by hand, unevenly: under the deck at first, in your face on deck
bell_n = lufs(bell, -18, "bell")
for i, tb in enumerate(np.arange(HIT + 0.55, 10.6, 1.9)):
    on_deck = DECK[0] <= tb < DECK[1] + 1.0
    b = bell_n if on_deck else filt(bell_n, "low", 1500) * db(-6)
    place(D, pan(b, 0.25), tb + rng.uniform(-0.2, 0.2), db(-2 * (i % 2)))
sail = lufs(sailors, -19, "sail")
place(D, filt(sail, "low", 2000) * db(-4), HIT + 0.35)
place(D, pan(sail, -0.3), DECK[0] - 0.2)
if sailor2 is not None:
    place(D, pan(lufs(sailor2, -17, "s2"), 0.3), DECK[0] + 0.5)
# the hull grinding on the reef, a sea over the rail at the end of the deck shot, another in the black
place(D, lufs(filt(load(A + "wood-crack.ogg"), "low", 900), -20, "groan1"), 6.6)
place(D, lufs(filt(load(A + "wood-crack.ogg"), "low", 700), -19, "groan2"), 8.4)
surge_n = int(2.6 * SR)
surge = filt(noise(surge_n, "pink"), "low", 2400) * np.concatenate([np.linspace(0, 1, int(0.9 * SR)), np.exp(-np.linspace(0, 3, surge_n - int(0.9 * SR)))])[:, None]
place(D, lufs(wave_deck if wave_deck is not None else surge, -14, "surge1"), 7.0)
place(D, lufs(wave_deck if wave_deck is not None else surge, -11, "surge2"), 9.25)
deck = (cabin * sched([(0, -60), (0.4, -6), (HIT - 0.05, -2), (HIT + 0.6, -6), (3.7, -60), (END, -60)])
        + wind * sched([(0, -60), (HIT, -60), (3.6, 0), (END, 0)])
        + rain * sched([(0, -60), (HIT, -60), (3.6, 2), (END, 2)])
        + sea * sched([(0, -60), (HIT, -24), (3.6, 1), (END, 1)])
        + D)
# 2. The second sea takes him: the whole deck closes over (a low-pass), sinks, and the beach comes up
#    under it -- no hole of silence, one sound turning into the next.
UNDER, SHORE = (9.6, 11.0), 11.4
deck_bus = sweep_down(deck, *UNDER) * sched([(0, 0), (UNDER[0], 0), (UNDER[1], -10), (SHORE + 0.6, -40), (SHORE + 1.0, -90), (END, -90)])
deck_bus += filt(noise(N, "brown"), "low", 110) * sched([(0, -90), (UNDER[0], -90), (UNDER[1], -20), (SHORE + 0.8, -90), (END, -90)])

# ---- 3. the beach -----------------------------------------------------------------------------
# Surf and rain, his coughing; the torches out of the trees -- the band far off, low snarls while they
# walk, then the lead one's war cry, the band's cries and their feet as they charge. Harrow steps in.
B = np.zeros((N, 2), np.float32)
for ts, k in STRIKES:
    if SHORE < ts < BEACH[1]:
        thunder(B, ts, k)
place(B, timed(pan(far(band_far, 0.6), -0.1), 12.2, [(0, 0), (16.2, 0), (17.4, -12), (END, -12)]), 12.2)   # the band up the beach
for i, (tg, key, se, p) in enumerate(((13.0, "d2", -2, -0.4), (14.1, "d1", -4, 0.3), (15.2, "d2", 1, -0.1), (15.9, "a2", -3, 0.5))):
    place(B, ork(key, se, 0.85, p, -20, f"sn{i}"), tg)                          # snarls up the beach
place(B, ork("w1", 0, 0.5, 0.0, -16, "lead"), 16.7)                             # the lead's cry: the charge
for i in range(9):                                                              # the band, closing
    tg = 17.15 + i * 0.37 + rng.uniform(-0.12, 0.12)
    key = [ROARS, BARKS][i % 2][rng.integers(0, 2)]
    place(B, ork(key, rng.uniform(-4, 3), 0.8 - 0.6 * i / 8, rng.uniform(-0.7, 0.7), -18, f"band{i}"), tg)
place(B, timed(run_bed, 16.4, [(0, -90), (16.4, -90), (17.2, -6), (BEACH[1], 0), (END, 0)]), 16.4)        # their feet, smeared
place(B, ork("a1", -1, 0.25, 0.0, -13, "near"), 19.9)
place(B, lufs(load(A + "footsteps/sand-2.ogg"), -24, "h_step1"), 19.05)        # Harrow's boots past Wat
place(B, lufs(load(A + "footsteps/sand-4.ogg"), -24, "h_step2"), 19.5)
fire_b = fire * sched([(0, -90), (BEACH[0], -90), (BEACH[0] + 0.3, -14), (BEACH[1], 0), (END, 0)])
# the cut: the beach goes in a beat (a fast fade, not a gate) -- the rain stays
B = (B + fire_b) * sched([(0, 0), (BEACH[1] - 0.06, 0), (BEACH[1] + 0.25, -90), (END, -90)])

# ---- 4. Harrow's stand (storm.rs `STAND_BEATS`, `ORK_SWINGS`/`ORK_DEATHS`) ----------------------
H = np.zeros((N, 2), np.float32)
whoosh = [load(A + "player-swing-1.ogg"), load(A + "player-swing-2.ogg")]
bites = [load(A + f"flesh-cut-{k}.ogg") for k in (1, 2, 3)]
for ts, k in STRIKES:
    if STAND[0] - 0.1 < ts < STAND[1]:
        thunder(H, ts, k)
place(H, timed(run_bed, STAND[0] - 0.4, [(0, -90), (STAND[0], -6), (23.5, -10), (24.6, -90), (END, -90)]), STAND[0] - 0.4)
place(H, ork("a2", 0, 0.35, -0.4, -15, "s_o3"), 21.9)                          # the first, at him
place(H, lufs(whoosh[0], -18, "s_w1"), 22.25)                                   # his cut
place(H, lufs(filt(bites[0], "low", 4200), -16, "s_b1"), 22.62)
place(H, pan(lufs(grunt1, -15, "s_g1"), -0.3), 22.66)                           # it takes it
place(H, ork("d1", -2, 0.3, -0.3, -16, "s_die3")[: int(1.4 * SR)], 22.8)
place(H, lufs(load(A + "body-thud-heavy-1.ogg"), -18, "s_fall3"), 23.45)
place(H, ork("w1", -2, 0.25, 0.3, -14, "s_o6"), 23.0)                           # the big one: club on the shield
place(H, lufs(load(A + "block-2.ogg"), -12, "s_blk"), 23.45)
place(H, lufs(filt(load(A + "body-thud-heavy-2.ogg"), "low", 1500), -17, "s_blkt"), 23.45)
place(H, ork("a3", 1, 0.3, 0.5, -15, "s_o4"), 24.4)                             # the third, from his flank
place(H, lufs(load(A + "body-thud-heavy-2.ogg"), -12, "s_hit"), 24.98)           # it lands: he reels
place(H, lufs(filt(bites[2], "low", 3000), -17, "s_hitb"), 24.98)
place(H, ork("a1", -3, 0.2, 0.3, -13, "s_o6b"), 25.35)                          # the big one finishes it
place(H, lufs(load(A + "body-thud-heavy-1.ogg"), -12, "s_kill"), 25.6)
place(H, lufs(load(A + "body-thud-heavy-2.ogg"), -15, "s_down"), 26.55)          # he goes down
place(H, pan(lufs(grunt2, -18, "s_g2"), 0.35), 26.9)                            # and they grunt over him
H = (H + fire * sched([(0, -90), (STAND[0], -10), (STAND[1], -6), (END, -6)])) * sched([(0, -90), (STAND[0] - 0.2, -90), (STAND[0], 0), (STAND[1] - 0.05, 0), (STAND[1] + 0.3, -90), (END, -90)])

# ---- 5. Harrow dead -- 6. the helmet -- the black between ----------------------------------------
S = np.zeros((N, 2), np.float32)
for ts, k in STRIKES:
    if ts > STAND[1]:
        thunder(S, ts, k)
place(S, wat2, SHORE + 0.3)
if wat4 is not None:
    place(S, wat4 * db(-2), 29.6)
place(S, wat1[int(1.2 * SR):] * db(-9), 34.2)                        # his breathing in the long black
if maud1 is not None:
    place(S, lufs(maud1, -18, "maud1"), 38.1)
    place(S, lufs(plate, -21, "plate"), 37.8)
    place(S, wat3 * db(-3), 42.7)
else:
    place(S, lufs(plate, -21, "plate"), 38.8)
    place(S, lufs(filt(reverb(tighter, 2.2, 0.55), "band", [250, 3200]), -29, "tighter"), 39.75)
    place(S, wat3 * db(-3), 41.35)
place(S, lufs(plate[int(0.3 * SR):], -27, "plate2"), 42.5)
place(S, timed(pan(far(band_far, 0.9), 0.4), 44.0, [(0, -90), (44.0, -90), (44.8, -6), (46.2, -2), (END, -2)]), 44.0)   # far off: they come again

# ---- 7. coda: Maud behind him, he draws and steps up; the fight; the camera draws away ----------
if draw is not None:
    place(S, lufs(draw, -18, "draw"), 47.35)
place(S, lufs(maud, -17, "maud"), 47.8)
for i in range(7):                                                             # the band, coming on
    tg = 48.6 + i * 0.48 + rng.uniform(-0.12, 0.12)
    key = [ROARS, BARKS][i % 2][rng.integers(0, 2)]
    place(S, ork(key, rng.uniform(-4, 2), 0.85 - 0.5 * i / 6, rng.uniform(-0.6, 0.6), -18, f"cband{i}"), tg)
place(S, timed(run_bed, 48.0, [(0, -90), (48.2, -90), (49.2, -9), (51.4, -6), (52.0, -14), (END, -14)]), 48.0)
# the fight (storm.rs `FIGHT`, `ORK_SWINGS`, `ORK_DEATHS`): each ork's cry as it swings; the club on his
# shield; each blade a whoosh and a bite and the ork's grunt; each death a groan and the body landing
ORK_SWINGS = [(0, 51.62), (1, 53.68), (2, 54.8), (5, 56.55)]
ORK_DEATHS = [(0, 53.18), (1, 54.2), (2, 56.2), (5, 57.25)]
SWINGS = [52.15, 52.58, 53.0, 53.62, 54.06, 55.25, 55.65, 56.05, 56.7, 57.1]
BLOCKS = [51.5, 54.75]
pull = sched([(0, 0), (54.3, 0), (58.4, -9), (END, -12)])        # the camera drawing away takes the fight with it
F = np.zeros((N, 2), np.float32)
for i, (o, ts) in enumerate(ORK_SWINGS):
    place(F, ork(["a2", "w2", "a2", "w2"][i], rng.uniform(-2, 2), 0.15, 0.25, -14, f"f_o{i}"), ts - 0.15)
for i, b in enumerate(BLOCKS):
    place(F, lufs(load(A + f"block-{1 + i % 2}.ogg"), -13, f"f_blk{i}"), b + 0.3)
    place(F, lufs(filt(load(A + "body-thud-heavy-2.ogg"), "low", 1500), -18, f"f_blkt{i}"), b + 0.3)
for i, sw in enumerate(SWINGS):
    place(F, lufs(whoosh[i % 2], -19, f"f_w{i}"), sw + 0.02)
    place(F, lufs(filt(bites[i % 3], "low", 4200), -17, f"f_b{i}"), sw + 0.19)
    if i % 2 == 0:
        place(F, pan([grunt1, grunt2][(i // 2) % 2], 0.25) * db(-3), sw + 0.22)
for i, (o, ts) in enumerate(ORK_DEATHS):
    place(F, ork(["d1", "a3", "d1", "a3"][i], -3 - i, 0.1, 0.2, -15, f"f_die{i}")[: int(1.5 * SR)], ts - 0.05)
    place(F, lufs(load(A + "body-thud-heavy-1.ogg"), -16, f"f_fall{i}"), ts + 0.75)
if clash is not None:
    place(F, lufs(clash, -11, "clash"), SWINGS[0] + 0.19)
S += F * pull

shore = (rain * sched([(0, -90), (UNDER[1] - 0.2, -90), (SHORE + 0.4, 0), (BEACH[1], 0), (BEACH[1] + 0.3, -3), (HARROW[0], -2), (31.5, -2), (32.6, -6), (HELM[0], -6), (END - 2.5, -4), (END, -30)])
         + sea * sched([(0, -90), (UNDER[1] - 0.3, -90), (SHORE + 0.5, 2), (BEACH[1], 2), (BEACH[1] + 0.3, -4), (STAND[1], -4), (HARROW[0], -4), (31.5, -4), (32.6, -14), (HELM[0], -10), (CODA[0] - 0.2, -10), (CODA[0] + 0.6, -3), (CODA[1], -3), (CODA[1] + 0.4, -20), (END, -40)])
         + water * sched([(0, -90), (UNDER[1], -90), (SHORE + 0.5, 0), (CODA[1], 0), (CODA[1] + 0.4, -20), (END, -40)])
         + wind * sched([(0, -90), (UNDER[1], -90), (SHORE + 0.5, -8), (BEACH[1], -6), (BEACH[1] + 0.3, -10), (STAND[1], -10), (HARROW[0], -14), (CODA[0], -8), (CODA[1], -4), (CODA[1] + 0.5, -30), (END, -40)])
         + fire * sched([(0, -90), (HARROW[0] - 0.3, -90), (HARROW[0], -6), (HARROW[1], -6), (HARROW[1] + 0.3, -90),
                         (HELM[0] - 0.3, -90), (HELM[0], -9), (HELM[1], -9), (HELM[1] + 0.3, -90), (CODA[0] - 0.3, -90), (CODA[0], -8), (54.3, -4), (CODA[1], -14), (CODA[1] + 0.2, -90), (END, -90)])
         + S + B + H)
# the picture goes to black at the end of the pull: the storm going on without us
shore *= sched([(0, 0), (CODA[1] - 0.6, 0), (CODA[1] + 0.4, -8), (END, -16)])

mix = deck_bus + shore + sub
sf.write(OUT + "/_mix_raw.wav", mix, SR)
subprocess.run(["ffmpeg", "-y", "-v", "error", "-i", OUT + "/_mix_raw.wav", "-af", "loudnorm=I=-17:TP=-3:LRA=16,alimiter=limit=0.68",
                "-ar", "48000", OUT + "/storm_mix.wav"], check=True)
os.makedirs(A + "cutscene", exist_ok=True)
subprocess.run(["ffmpeg", "-y", "-v", "error", "-i", OUT + "/storm_mix.wav", "-c:a", "libvorbis", "-q:a", "5", A + "cutscene/storm_mix.ogg"], check=True)
for f in os.listdir(OUT):
    if f.startswith("_"):
        os.remove(os.path.join(OUT, f))
print("mix", OUT + "/storm_mix.wav")

ARGS = [a for a in sys.argv[1:] if not a.startswith('--')]
if len(ARGS) >= 3:
    subprocess.run(["ffmpeg", "-y", "-v", "error", "-i", ARGS[1], "-i", OUT + "/storm_mix.wav", "-map", "0:v", "-map", "1:a",
                    "-c:v", "copy", "-c:a", "aac", "-b:a", "192k", "-shortest", ARGS[2]], check=True)
    print("muxed", ARGS[2])
