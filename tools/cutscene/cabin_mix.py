"""P.0 Below Deck: cut the one-take ElevenLabs recording into lines, lay them out with the scene beats,
put the hymn (looped), the music and the SFX under, and export the per-line clips (`assets/audio/vo/campaign/p0_cabin_NN.ogg`).
Usage: python tools/cutscene/cabin_mix.py <out_dir>   (source paths are set at the top; Whisper word times: whisper_words.py)"""
import json, subprocess, sys
import numpy as np, soundfile as sf
from scipy.ndimage import uniform_filter1d

U = "C:/Users/skibi/.claude/uploads/8e2e82b7-28fb-4c50-9c5e-c561e0e56d41/"
VOICE = U + "bc14a3d6-ElevenLabs_2026-10-21T08_02_13_Sully_-_Mature_Deep_and_Intriguing_pvc_sp100_s50_sb75_v4.mp3"
MUSIC = U + "04e4bf18-Silence_Between_Notes.mp3"
OUT = sys.argv[1]
SR = 44100

def load(p, ch):
    raw = subprocess.run(["ffmpeg", "-v", "error", "-i", p, "-ac", str(ch), "-ar", str(SR), "-f", "f32le", "-"], capture_output=True).stdout
    x = np.frombuffer(raw, np.float32)
    return x if ch == 1 else x.reshape(-1, ch)

v = load(VOICE, 1)
m = load(MUSIC, 2)

def snap(t, w=0.10):
    """Quietest 10 ms frame within +-w of t (cut inside the pause, not on a breath/consonant)."""
    a, b = int((t - w) * SR), int((t + w) * SR)
    f = 441
    best, bi = 9, a
    for i in range(a, b - f, 110):
        r = np.sqrt((v[i:i + f] ** 2).mean())
        if r < best: best, bi = r, i
    return (bi + f // 2) / SR

LEAD = 6.5   # silence + title + establishing shot before "Tighter."
BREAK = 9.5  # dialogue pause: the hymn comes forward (ship shot)
# (id, who, src start, src end, gap before in the scene [s], line)
L = [
 ("01", "H", 0.00, 0.78, 2.5 + LEAD, "Tighter."),
 ("02", "H", 0.78, 9.30, 0.7, "Left buckle's always stiff. Fifteen years you've strapped me into this iron, Wat, and you still treat it like it's going to bite you."),
 ("03", "W", 73.82, 77.50, 0.4, "Rust from the salt, Ser. Hnn. There."),
 ("04", "H", 9.30, 13.80, 0.7, "Stand up. You're making my knees ache just looking at you."),
 ("05a", "H", 13.80, 16.84, 1.8, "Listen to them. Singing."),
 ("05b", "H", 16.84, 19.08, BREAK, "Wait till they see the island."),
 ("06", "W", 77.50, 78.88, 0.45, "Have you seen it, Ser? Kell?"),
 ("07", "H", 19.08, 24.45, 0.35, "Nobody's seen Kell in a hundred years, Wat. That's why the King's giving it to me."),
 ("08", "H", 24.45, 33.27, 0.35, "You take one city too roughly in the east, and suddenly the Crown discovers a desperate need for your talents on a wet rock at the edge of the map."),
 ("09", "W", 78.88, 81.82, 0.35, "They made you Lord Commissioner, Ser."),
 ("10", "H", 33.27, 38.95, 0.35, "Over forty seasick turnip-farmers and a village Reeve. A glorious command."),
 ("11", "H", 38.95, 44.30, 0.9, "Take the parchment to the woman downstairs. The Reeve. Tell her we sight land at dawn."),
 ("12", "W", 81.82, 83.30, 2.6, "Which one, Ser?"),
 ("13", "H", 44.30, 49.73, 0.45, "The one without the royal wax, Wat. I keep forgetting I didn't hire you for your reading."),
 ("14", "H", 49.73, 52.82, 2.0, "Did she ask about me? When you brought the water down?"),
 ("15", "W", 83.30, 88.07, 0.55, "She asked why the great Ser Godric Harrow hasn't come out of his cabin since we sailed."),
 ("16", "H", 52.82, 55.65, 0.35, "And what did my loyal squire tell her?"),
 ("17", "W", 88.07, 90.67, 0.45, "That you were resting after the campaign, Ser."),
 ("18", "H", 55.65, 66.10, 0.9, "Good man. Let them keep singing to the suit of armour in their heads. The moment I walk down there without that helmet on, I'm just an old butcher with bad knees."),
 ("19", "H", 66.10, 73.82, 0.8, "Nobody looks at the man inside the iron, Wat. They just want to know the iron is there. Go on. Stop hovering."),
]
OUTRO = 20.0
fade = int(0.008 * SR)
clips, timeline, t = [], [], 0.0
for cid, who, a, b, gap, txt in L:
    a2 = snap(a) if a > 0 else 0.0
    b2 = snap(b) if b < 90 else len(v) / SR
    c = v[int(a2 * SR):int(b2 * SR)].copy()
    c[:fade] *= np.linspace(0, 1, fade); c[-fade:] *= np.linspace(1, 0, fade)
    t += gap
    clips.append((t, c)); timeline.append((cid, who, round(t, 2), round(t + len(c) / SR, 2), txt))
    t += len(c) / SR
total = t + OUTRO
N = int(total * SR)
dry = np.zeros(N, np.float32)
for st, c in clips:
    i = int(st * SR); dry[i:i + len(c)] += c
# voice: -16 LUFS, limiter-ish peak guard
def lufs(x, st=False):
    import pyloudnorm  # noqa
sf.write(OUT + "/_dry_raw.wav", dry, SR)
subprocess.run(["ffmpeg", "-y", "-v", "error", "-i", OUT + "/_dry_raw.wav", "-af", "loudnorm=I=-16:TP=-1.5:LRA=7,alimiter=limit=0.89", "-ar", str(SR), OUT + "/_dry.wav"], check=True)
dry, _ = sf.read(OUT + "/_dry.wav"); dry = dry.astype(np.float32)
if dry.ndim > 1: dry = dry[:, 0]
dry = np.pad(dry, (0, max(0, N - len(dry))))[:N]


# ---------------------------------------------------------------- mix v3: music + hymn + sfx
from scipy.signal import butter, sosfiltfilt
AS = "C:/Users/skibi/Documents/Codex/warbell/assets/audio/"
SONG = U + "7695d4b3-Swallow_the_Salt.mp3"
T = {r[0]: (r[2], r[3]) for r in timeline}  # id -> (start, end)

def db(x): return 10 ** (x / 20)
def loadst(p):
    return load(p, 2)
def lp(x, hz, order=4):
    sos = butter(order, hz, "low", fs=SR, output="sos")
    return sosfiltfilt(sos, x, axis=0).astype(np.float32)
def norm_lufs(x, target, tag):
    sf.write(OUT + f"/_{tag}.wav", x, SR)
    subprocess.run(["ffmpeg", "-y", "-v", "error", "-i", OUT + f"/_{tag}.wav", "-af", f"loudnorm=I={target}:TP=-2:LRA=20:linear=false", "-ar", str(SR), OUT + f"/_{tag}_n.wav"], check=True)
    y, _ = sf.read(OUT + f"/_{tag}_n.wav"); y = y.astype(np.float32)
    return y if y.ndim > 1 else np.stack([y, y], 1)
def place(buf, x, t0, gain=1.0):
    i = int(t0 * SR); n = min(len(x), len(buf) - i)
    if n > 0 and i >= 0: buf[i:i + n] += x[:n] * gain
def sched(points, N):
    """piecewise-linear dB schedule [(t,dB),...] -> linear gain array"""
    ts = np.array([p[0] for p in points]) * SR; ds = np.array([p[1] for p in points])
    return db(np.interp(np.arange(N), ts, ds)).astype(np.float32)
def loop_to(x, n, xf=int(1.5 * SR), start=0):
    out = np.zeros((n, 2), np.float32); pos = 0; first = True
    while pos < n:
        seg = x[start if first else 0:].copy() if True else x
        seg = x.copy()
        if pos > 0:
            r = np.linspace(0, 1, xf)[:, None]; seg[:xf] *= r
            out[pos:pos + xf] *= (1 - r)[: len(out[pos:pos + xf])]
        k = min(len(seg), n - pos); out[pos:pos + k] += seg[:k]
        pos += len(seg) - xf; first = False
    return out

# ---- the hymn: the whole song from the first breath before "Over", looped for the entire scene
THU = [U + "ef362061-Firefly_audio_Powerful_realistic_thunderclap_deep_explosive_cra_variation4.wav",
       U + "f728e8b1-Firefly_audio_Powerful_realistic_thunderclap_deep_explosive_cra_variation3.wav"]
song = loadst(SONG)
hy = song[:].copy()
hy[:int(0.01 * SR)] *= np.linspace(0, 1, int(0.01 * SR))[:, None]
sf.write(OUT + "/swallow_full.wav", hy, SR)
hy = norm_lufs(hy, -16, "hy")
XF = int(2.0 * SR); P = len(hy) - XF            # loop period (copies overlap by XF)

N = int(total * SR)
t_h0 = T["05a"][1] + 0.4          # the pause after "Singing." -- the shot of the hold
t_end19 = T["19"][1]
def tile_loop(anchor):
    """song copies every P seconds, one starting exactly at  (so the break hears 'Over the grey water' from the top)"""
    out = np.zeros((N, 2), np.float32)
    ramp = np.linspace(0, 1, XF)[:, None]
    k0 = -int(np.ceil(anchor * SR / P)) - 1
    k = k0
    while True:
        st = int(anchor * SR) + k * P
        if st >= N: break
        c = hy.copy(); c[:XF] *= ramp; c[-XF:] *= ramp[::-1]
        a, b = max(st, 0), min(st + len(c), N)
        if b > a: out[a:b] += c[a - st:b - st]
        k += 1
    return out
LOOP = tile_loop(t_h0 - 0.6)   # the first line ('The king, he gave us twenty leagues of mud') lands as the break opens
WALL = lp(LOOP, 1100, 2)                                   # through the planks: only the low body of the voices
HATCH = LOOP                       # open: no low-pass at all in the 'ship shot' and the pull-back
dry_v = LOOP
t_L = T['05a'][0]; t_b = T['05b'][0]
gA = sched([(0, -90), (5.0 + LEAD, -90), (15.5 + LEAD, -7), (t_L - 0.5, -7), (t_L + 1.5, -26), (t_b - 2.0, -34), (t_b + 1.5, -7),
            (46.0 + LEAD, -7), (60.0 + LEAD, -90), (T['18'][0] - 4.0, -90), (T['19'][0] + 2.0, -15), (t_end19, -15), (t_end19 + 3.5, -34), (total - 0.2, -90)], N)
gB = sched([(0, -90), (t_L - 0.8, -90), (t_L + 1.2, -9), (t_h0 + 1.5, -3), (t_b - 2.0, -3), (t_b + 1.5, -90), (t_end19 - 0.3, -90), (t_end19 + 3.5, -2), (total - 4.5, -2), (total - 0.2, -90)], N)
HYM = WALL * gA[:, None] + HATCH * gB[:, None]

# ---- music: A Silence Between Notes bed, looped to length; held down whenever the hymn is in the room
mu = loadst(MUSIC)
if len(mu) < N:
    xf = int(4 * SR); tail = mu[int(30 * SR):]; r = np.linspace(0, 1, xf)[:, None]
    mu = np.concatenate([mu[:-xf], mu[-xf:] * (1 - r) + tail[:xf] * r, tail[xf:]])
mu = mu[:N].copy()
env = np.ones(N, np.float32); fi, fo = int(3 * SR), int(4.5 * SR)
env[:fi] = np.linspace(0, 1, fi) ** 1.5; env[-fo:] = np.linspace(1, 0, fo) ** 1.5
act = uniform_filter1d(np.abs(dry), int(0.12 * SR)) > 0.012
act = uniform_filter1d(act.astype(np.float32), int(0.5 * SR))
duck = db(-6.0 * np.clip(act * 1.6, 0, 1))
brk = sched([(0, 0), (5.0 + LEAD, 0), (14.0 + LEAD, -24), (52.0 + LEAD, -24), (64.0 + LEAD, 0), (T['18'][0] - 6.0, 0), (T['18'][0] + 3.0, -24), (total, -24)], N)
mu = mu * (env * duck * brk)[:, None]
mu = norm_lufs(mu, -29, "mu")[:N]
mu = np.pad(mu, ((0, N - len(mu)), (0, 0)))

# ---- SFX we have: cabin storm loop, wind, timber, wooden boots, + the user's thunder
cab = loadst(AS + "sfx/cutscene/cs_ship_cabin.ogg"); cab = norm_lufs(cab, -31, "cab")
CAB = loop_to(cab, N, xf=int(0.4 * SR))[:N]
CAB *= sched([(0, -90), (0.5, -30), (4.6, 0), (t_h0, 0), (t_h0 + 1.5, 4), (t_h0 + BREAK, 4), (t_h0 + BREAK + 3, 0), (total - 2.5, 0), (total - 0.5, -90)], N)[:, None]
wi = loadst(AS + "wind.ogg"); wi = norm_lufs(wi, -38, "wi")
WI = loop_to(wi, N, xf=int(1.0 * SR))[:N]
WI *= sched([(0, -6), (4.6, 0), (t_h0, 0), (t_h0 + 1.5, 3), (t_h0 + BREAK, 3), (t_h0 + BREAK + 3, 0), (total - 0.2, -90)], N)[:, None]
SFX = np.zeros((N, 2), np.float32)
def peak_to(x, pk): return x * (db(pk) / max(1e-6, np.abs(x).max()))
crack = loadst(AS + "wood-crack.ogg")
for t, pk in [(1.3, -33), (T["04"][1] + 0.2, -31), (t_h0 + 2.8, -30), (t_h0 + 7.0, -32), (T["12"][0] - 1.4, -31), (T["14"][0] - 1.1, -31)]:
    place(SFX, peak_to(lp(crack, 1400), pk), t)
steps = [loadst(AS + f"footsteps/wood-{n}.ogg") for n in range(1, 5)]
def walk(t0, n, pk=-24.5):
    for k in range(n):
        s = peak_to(steps[k % 4], pk); pan = 0.15 if k % 2 == 0 else -0.15
        place(SFX, s * np.array([1 - pan, 1 + pan], np.float32), t0 + k * 0.52)
walk(T["11"][1] + 0.25, 4)      # to the table for the letter
walk(T["13"][1] + 0.25, 3)      # to the door; the lightning stops him
walk(t_end19 + 1.0, 6)          # out into the passage
# thunder (the user's two takes: 3 s each, var4 = the loud crack, var3 = the softer roll)
th4, th3 = loadst(THU[0]), loadst(THU[1])
def thunder(t, x, lufs, lpf=None):
    y = lp(x, lpf) if lpf else x
    place(SFX, norm_lufs(y, lufs, f"th{int(t)}"), t)
FLASHES = [  # (flash t, strength, thunder take, thunder LUFS, lowpass) -- thunder rolls in 1.2 s behind the flash; the scene reads the same list
    (5.4, 0.55, th3, -40, 700), (16.0, 0.25, th3, -44, 600), (36.8, 0.8, th3, -27, 3000), (49.0, 0.3, th3, -44, 700),
    (68.0, 0.35, th3, -42, 800), (89.6, 0.6, th3, -27, 2500), (99.5, 0.3, th3, -42, 800), (113.6, 1.0, th4, -24, None),
    (126.4, 0.5, th3, -33, 1200), (129.9, 0.9, th4, -26, None), (136.0, 0.3, th3, -41, 900), (141.0, 0.4, th3, -38, 1000)]
for ft, k, take, lufs, lpf in FLASHES:
    thunder(ft + 1.2, take, lufs, lpf)
json.dump([(f[0], f[1]) for f in FLASHES], open(OUT + "/flashes.json", "w"))

# ---- export the lines for the scene (mouths + the offline mix both use the same takes)
import os
VOD = "C:/Users/skibi/Documents/Codex/warbell/assets/audio/vo/campaign/"
for cid, who, st, en, txt in timeline:
    a, b = int((st - 0.03) * SR), int((en + 0.06) * SR)
    sf.write(OUT + f"/_c_{cid}.wav", dry[a:b], SR)
    subprocess.run(["ffmpeg", "-y", "-v", "error", "-i", OUT + f"/_c_{cid}.wav", "-ac", "1", "-ar", "44100", "-c:a", "libvorbis", "-q:a", "5", VOD + f"p0_cabin_{cid}.ogg"], check=True)
    os.remove(OUT + f"/_c_{cid}.wav")

# ---- final sum
voice = np.stack([dry, dry], 1)
mix = voice + mu + HYM + (CAB + WI) + SFX
pk = np.abs(mix).max()
mix *= db(-2.0) / pk if pk > db(-2.0) else 1.0
sf.write(OUT + "/p0_cabin_mix_v8.wav", mix, SR)
print("total", round(total, 2), "break", round(t_h0, 2), "->", round(t_h0 + BREAK, 2), "peak before", round(20 * np.log10(pk), 1))
json.dump([dict(id=a, who=b, start=c, end=d, text=e) for a, b, c, d, e in timeline], open(OUT + "/timeline_v8.json", "w"), indent=1)
