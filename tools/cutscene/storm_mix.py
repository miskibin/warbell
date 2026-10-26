"""P.1 "Piorun": the soundtrack of `FOREST_SCENE=storm` (src/scenes/storm.rs), mixed offline against the
scene clock. No score: the storm, the sea, the orks, Wat's breath, one line from Maud.

Also cuts the uploaded takes into the asset tree (Wat's breaths `assets/audio/vo/campaign/p1_wat_NN.ogg`,
the cutscene SFX `assets/audio/sfx/cutscene/cs_*.ogg`) and writes the mix the in-game cue plays
(`assets/audio/cutscene/storm_mix.ogg`).

Usage:  python tools/cutscene/storm_mix.py <out_dir> [video.mp4 out.mp4]
        (with a video: muxes the mix under it)

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
END = 47.5
N = int(END * SR)
rng = np.random.default_rng(1)

# ---- the scene's timeline (storm.rs) ---------------------------------------------------------
DECK, BEACH, HARROW, HELM, CODA = (4.0, 7.7), (12.0, 20.6), (22.0, 26.9), (31.0, 37.7), (40.0, 45.7)
STRIKES = [(4.05, 1.0), (5.95, 0.55), (12.05, 1.0), (16.6, 0.42), (20.22, 0.9), (22.05, 1.0), (24.3, 0.8),
           (31.05, 0.85), (34.3, 0.8), (40.05, 0.6), (43.4, 0.35)]


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


# ---- the takes --------------------------------------------------------------------------------
# Wat (ElevenLabs v3, Sully, one generation of the three breaths, cut at its own gaps):
WAT = U + "f7a87374-ElevenLabs_2026-10-26T07_12_30_Sully_-_Mature_Deep_and_Intriguing_pvc_sp100_s50_sb75_v3.mp3"
w = load(WAT, 1)
c1, c2, c3a, c3b = snap(w, 3.62), snap(w, 8.42, 0.05), snap(w, 9.05, 0.08), snap(w, 11.36, 0.06)
wat1 = fade(w[:int(c1 * SR)])               # p1_wat_01  [gasps sharply] Hhh! [panting] Hhh... hhh...   (waking to the reef)
wat2 = fade(w[int(c1 * SR):int(c2 * SR)])   # p1_wat_02  [coughing] Khh- khh. [gasping for air] Hhhah... (on his knees in the sand)
wat3 = fade(w[int(c3a * SR):int(c3b * SR)])  # p1_wat_03  [strained, quiet] Hnnh. [exhales]           (the strap pulled home)
VO = A + "vo/campaign/"
for k, x in (("01", wat1), ("02", wat2), ("03", wat3)):
    export(x, VO + f"p1_wat_{k}.ogg", True, -16)
wat1, wat2, wat3 = (lufs(x, -16, f"w{i}") for i, x in enumerate((wat1, wat2, wat3)))

CS = A + "sfx/cutscene/"
orks = load(U + "43a98b74-Firefly_audio__a_war_band_of_a_dozen_orcs_charging_across_wet_sa_variation2.wav")
plate = load(U + "e0f5e0e4-Firefly_audio_a_heavy_steel_shoulder_plate_lifted_from_wet_sand__variation4.wav")
sailors = load(U + "31391482-several_sailors_shou_2-1792998821354.mp3")
bell = load(U + "b3201920-a_ship_s_brass_bell__3-1792998942570.mp3")[: int(2.85 * SR)]
for name, x in (("cs_orks_charge", orks), ("cs_plate_wet", plate), ("cs_sailors_panic", sailors), ("cs_ship_bell_alarm", bell)):
    export(x, CS + name + ".ogg", False, -18)
th_crack = load(U0 + "ef362061-Firefly_audio_Powerful_realistic_thunderclap_deep_explosive_cra_variation4.wav")
th_roll = load(U0 + "f728e8b1-Firefly_audio_Powerful_realistic_thunderclap_deep_explosive_cra_variation3.wav")
maud = load(A + "vo/campaign/a1_maud_04.ogg", 1)        # "Ser Godric's up! Behind him, all of you!"
tighter = load(A + "vo/campaign/p0_cabin_01.ogg", 1)    # Harrow, P.0: "Tighter."

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

# ---- the beats --------------------------------------------------------------------------------
SFX = np.zeros((N, 2), np.float32)


def thunder(t, k):
    """A strike's thunder: close ones crack with the flash, weaker ones roll in behind it."""
    if k >= 0.8:
        place(SFX, lufs(th_crack, -15 - 6 * (1 - k), f"tc{t}"), t + 0.03)
    else:
        place(SFX, lufs(filt(th_roll, "low", 900 + 2500 * k), -26 + 10 * k, f"tr{t}"), t + 0.35)


for ts, k in STRIKES:
    thunder(ts, k)

# 1. Below deck, black: the cabin's creak and rain, then the reef. The hull splits under a crack of
#    thunder, the bell goes, men shout; Wat wakes.
HIT = 2.45
place(SFX, lufs(filt(load(A + "wood-crack.ogg"), "low", 2600), -13, "crack"), HIT)
place(SFX, lufs(load(A + "wood-crack.ogg")[::-1].copy(), -24, "crackr"), HIT + 0.35)
place(SFX, lufs(load(A + "body-thud-heavy-1.ogg"), -14, "thud"), HIT)
place(SFX, lufs(th_crack, -14, "hitth"), HIT + 0.02)
boom = np.sin(2 * np.pi * np.cumsum(np.linspace(55, 28, int(1.4 * SR))) / SR) * np.exp(-np.linspace(0, 4, int(1.4 * SR)))
place(sub, boom.astype(np.float32), HIT, 0.5)
place(SFX, wat1, HIT + 0.22)
bell_n = lufs(bell, -24, "bell")
for i, tb in enumerate(np.arange(HIT + 0.6, 10.2, 2.25)):   # rung by hand, unevenly, under everything
    place(SFX, bell_n, tb + rng.uniform(-0.25, 0.25), db(-3 * (i % 2)))
sail = lufs(sailors, -24, "sail")
place(SFX, sail, HIT + 0.4)
place(SFX, sail[::-1].copy() * 0.6, HIT + 5.2)
# the hull grinding on the reef, a sea over the rail at the end of the deck shot
place(SFX, lufs(filt(load(A + "wood-crack.ogg"), "low", 900), -20, "groan1"), 6.6)
place(SFX, lufs(filt(load(A + "wood-crack.ogg"), "low", 700), -19, "groan2"), 8.4)
surge_n = int(2.6 * SR)
surge = filt(noise(surge_n, "pink"), "low", 2400) * np.concatenate([np.linspace(0, 1, int(0.9 * SR)), np.exp(-np.linspace(0, 3, surge_n - int(0.9 * SR)))])[:, None]
place(SFX, lufs(surge, -14, "surge1"), 7.0)
place(SFX, lufs(surge, -12, "surge2"), 9.3)

# 2. The sea takes him: everything under, a held breath of nothing, then the beach.
UNDER, OUT_, SHORE = 9.9, 11.0, 11.62

# the deck mix (built now so it can be dragged under)
deck = (cabin * sched([(0, -60), (0.4, -6), (HIT - 0.05, -2), (HIT + 0.6, -6), (3.7, -60), (END, -60)])
        + wind * sched([(0, -60), (HIT, -60), (3.6, 0), (UNDER, 0), (END, 0)])
        + rain * sched([(0, -60), (HIT, -60), (3.6, 2), (END, 2)])
        + sea * sched([(0, -60), (HIT, -24), (3.6, 1), (END, 1)])
        + SFX)
deck = deck * sched([(0, 0), (UNDER, 0), (OUT_, -6), (SHORE - 0.02, -30), (SHORE, -90), (END, -90)])
under = filt(deck, "low", 260, 4) * sched([(0, -90), (UNDER - 0.15, -90), (UNDER + 0.2, 0), (OUT_, -4), (SHORE - 0.05, -40), (SHORE, -90), (END, -90)])
rumble = filt(noise(N, "brown"), "low", 110) * sched([(0, -90), (UNDER, -90), (UNDER + 0.3, -18), (SHORE - 0.1, -26), (SHORE, -90), (END, -90)])
deck_bus = deck * sched([(0, 0), (UNDER - 0.1, 0), (UNDER + 0.15, -90), (END, -90)]) + under + rumble

# 3. The beach: surf and rain, his coughing; the torches out of the trees, a growl, the charge.
SFX = np.zeros((N, 2), np.float32)
for ts, k in STRIKES:
    if ts > BEACH[1]:
        thunder(ts, k)
# what belongs to the beach shot goes on its own bus, cut dead with the picture
REST, SFX = SFX, np.zeros((N, 2), np.float32)
for ts, k in STRIKES:
    if SHORE < ts < BEACH[1]:
        thunder(ts, k)
place(REST, wat2, SHORE + 0.05)
grunts = [load(A + f"ork-grunt-{i}.ogg") for i in (1, 2, 3)]
for i, tg in enumerate((13.1, 14.4, 15.6)):
    place(SFX, lufs(filt(grunts[i % 3], "low", 1800), -30 + 2 * i, f"g{i}"), tg)
oc = lufs(orks, -17, "orks")
place(SFX, oc * np.linspace(0.3, 1.0, len(oc))[:, None], BEACH[1] - len(oc) / SR + 0.15)
roar = load(A + "ork-roar.ogg")
place(SFX, lufs(roar, -12, "roar1"), 19.95)
place(SFX, lufs(load(A + "monster-growl.ogg"), -16, "growl1"), 20.25)
# the cut: hard, on the roar -- after it, only rain
BEACH_BUS, SFX = SFX * sched([(0, 0), (BEACH[1] - 0.01, 0), (BEACH[1] + 0.01, -90), (END, -90)]), REST

# 4. Harrow: the double strike, then the storm drops away to rain and the torch on the sand.
# 5. The helmet: the plate lifted, Harrow's voice once more in Wat's head, the strap pulled home.
place(SFX, lufs(plate, -21, "plate"), 32.2)
place(SFX, lufs(filt(reverb(tighter, 2.2, 0.55), "band", [250, 3200]), -29, "tighter"), 33.15)
place(SFX, wat3 * db(-3), 34.75)
place(SFX, lufs(plate[int(0.3 * SR):], -27, "plate2"), 35.4)
# his breathing through the long black after Harrow
place(SFX, wat1[int(1.2 * SR):] * db(-9), 27.6)
# far off, before the coda, a horn: they're coming again
place(SFX, lufs(filt(load(A + "war-horn.ogg"), "low", 900), -30, "horn"), 38.3)

# 6. Coda: the charge again, Maud behind him, the clash -- cut.
place(SFX, lufs(maud, -17, "maud"), 41.2)
place(SFX, oc * db(-4) * np.linspace(0.4, 1.0, len(oc))[:, None], CODA[1] - len(oc) / SR + 0.1)
place(SFX, lufs(roar, -13, "roar2"), 44.85)
CLASH = CODA[1] - 0.02
place(SFX, lufs(load(A + "sword-hit-1.ogg"), -10, "clash"), CLASH)
place(SFX, lufs(load(A + "body-thud-heavy-1.ogg"), -15, "clashthud"), CLASH)

shore = (rain * sched([(0, -90), (SHORE, 0), (BEACH[1], 0), (BEACH[1] + 0.01, -4), (HARROW[0], -2), (24.9, -2), (26.0, -6), (HELM[0], -6), (END - 2.5, -4), (END, -30)])
         + sea * sched([(0, -90), (SHORE, 2), (BEACH[1], 2), (BEACH[1] + 0.01, -10), (HARROW[0], -4), (24.9, -4), (26.0, -14), (HELM[0], -10), (CODA[0], -3), (CODA[1], -3), (CODA[1] + 0.02, -20), (END, -40)])
         + water * sched([(0, -90), (SHORE, 0), (CODA[1], 0), (CODA[1] + 0.02, -20), (END, -40)])
         + wind * sched([(0, -90), (SHORE, -8), (BEACH[1], -6), (BEACH[1] + 0.01, -14), (CODA[0], -8), (CODA[1], -8), (CODA[1] + 0.02, -30), (END, -40)])
         + fire * sched([(0, -90), (BEACH[0], -90), (BEACH[0] + 0.3, -14), (BEACH[1], 0), (BEACH[1] + 0.01, -90), (HARROW[0], -6), (HARROW[1], -6), (HARROW[1] + 0.01, -90),
                         (HELM[0], -9), (HELM[1], -9), (HELM[1] + 0.01, -90), (CODA[0], -8), (CODA[1], -2), (CODA[1] + 0.01, -90), (END, -90)])
         + SFX + BEACH_BUS)
# after the clash: black, the storm going on without us
shore *= sched([(0, 0), (CLASH + 0.25, 0), (CLASH + 0.6, -10), (END, -16)])

mix = deck_bus + shore + sub
sf.write(OUT + "/_mix_raw.wav", mix, SR)
subprocess.run(["ffmpeg", "-y", "-v", "error", "-i", OUT + "/_mix_raw.wav", "-af", "loudnorm=I=-17:TP=-2:LRA=16,alimiter=limit=0.75",
                "-ar", "48000", OUT + "/storm_mix.wav"], check=True)
os.makedirs(A + "cutscene", exist_ok=True)
subprocess.run(["ffmpeg", "-y", "-v", "error", "-i", OUT + "/storm_mix.wav", "-c:a", "libvorbis", "-q:a", "5", A + "cutscene/storm_mix.ogg"], check=True)
for f in os.listdir(OUT):
    if f.startswith("_"):
        os.remove(os.path.join(OUT, f))
print("mix", OUT + "/storm_mix.wav", "cuts", round(c1, 3), round(c2, 3), round(c3a, 3), round(c3b, 3))

if len(sys.argv) >= 4:
    subprocess.run(["ffmpeg", "-y", "-v", "error", "-i", sys.argv[2], "-i", OUT + "/storm_mix.wav", "-map", "0:v", "-map", "1:a",
                    "-c:v", "copy", "-c:a", "aac", "-b:a", "192k", "-shortest", sys.argv[3]], check=True)
    print("muxed", sys.argv[3])
