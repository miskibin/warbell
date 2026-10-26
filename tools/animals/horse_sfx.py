"""Cut the mount's SFX (`assets/audio/horse/`) out of CC0 Freesound HQ previews (credits in `assets/CREDITS.md`).

    python horse_sfx.py <downloads dir> <out dir>

* `hoof-1..6`  single clops (onset-detected, isolated, 170 ms, 30 ms fade-out, RMS-matched) from
  craigsmith's "Foley Horse Hooves Walking" (481917); `horse.rs` fires one per hoof plant, so the
  same set reads as a walk or a four-beat gallop.
* `snort`      the loudest burst of bruno.auzet's "horse snort" (538697).
* `whinny-1/2` craigsmith's "Perfect Horse Whinny" (437110) and Salsero_classic's "Horse Neigh" (826753),
  silence-trimmed.
Needs ffmpeg + numpy.
"""
import os, subprocess, sys, wave
import numpy as np

SRC, OUT = sys.argv[1], sys.argv[2]
os.makedirs(OUT, exist_ok=True)
SR = 44100


def load(sound_id):
    d = next(x for x in os.listdir(SRC) if x.startswith("sfx_freesound_") and x.endswith("_" + sound_id))
    raw = subprocess.run(["ffmpeg", "-loglevel", "error", "-i", os.path.join(SRC, d, sound_id + ".mp3"),
                          "-ac", "1", "-ar", str(SR), "-f", "s16le", "-"], capture_output=True, check=True).stdout
    return np.frombuffer(raw, np.int16).astype(np.float32) / 32768


def env_db(x, hop=441):
    n = len(x) // hop
    return 20 * np.log10(np.sqrt(np.mean(x[:n * hop].reshape(n, hop) ** 2, axis=1)) + 1e-9)


def write(name, x, rms_db=None, fade=0.03):
    x = x.copy()
    if rms_db is not None:
        x *= 10 ** ((rms_db - 20 * np.log10(np.sqrt(np.mean(x ** 2)) + 1e-9)) / 20)
    nf = int(fade * SR)
    x[-nf:] *= np.linspace(1, 0, nf)
    x[:64] *= np.linspace(0, 1, 64)
    x = np.clip(x, -1, 1)
    tmp = os.path.join(OUT, name + ".wav")
    with wave.open(tmp, "wb") as w:
        w.setnchannels(1); w.setsampwidth(2); w.setframerate(SR)
        w.writeframes((x * 32767).astype(np.int16).tobytes())
    subprocess.run(["ffmpeg", "-loglevel", "error", "-y", "-i", tmp, "-c:a", "libvorbis", "-q:a", "5",
                    os.path.join(OUT, name + ".ogg")], check=True)
    os.remove(tmp)
    print(f"{name}: {len(x) / SR:.2f} s")


# ---- hooves: isolated clops (quiet before, nothing else for 200 ms after), loudest first
x = load("481917")
db = env_db(x)            # 10 ms frames
floor = np.percentile(db, 10)
cands = []
for i in range(5, len(db) - 25):
    if db[i] - db[i - 2] > 12 and db[i - 5:i - 1].max() < floor + 10 and db[i + 3:i + 20].max() < db[i:i + 3].max() - 3 \
            and not (np.diff(db[i + 2:i + 20]) > 9).any():
        cands.append((db[i:i + 3].max(), i))
cands.sort(reverse=True)
picked = []
for pk, i in cands:
    if all(abs(i - j) > 50 for j in picked):
        picked.append(i)
    if len(picked) == 6:
        break
for k, i in enumerate(sorted(picked), 1):
    s = i * 441 - int(0.004 * SR)
    write(f"hoof-{k}", x[s:s + int(0.17 * SR)], rms_db=-18)

# ---- snort: loudest 1.1 s window
x = load("538697")
db = env_db(x)
win = 110
e = np.convolve(10 ** (db / 10), np.ones(win), "valid")
i = int(np.argmax(e))
on = max(0, i + int(np.argmax(np.diff(db[i:i + win]) > 6)) - 3)
write("snort", x[on * 441:(on + 115) * 441], rms_db=-20, fade=0.15)

# ---- whinnies: trim to the loud span
for name, sid in (("whinny-1", "437110"), ("whinny-2", "826753")):
    x = load(sid)
    db = env_db(x)
    # longest run of loud frames (gaps under 0.3 s bridged) — skips the vintage reel's slate click
    loud = np.where(db > db.max() - 22)[0]
    runs, s0 = [], loud[0]
    for p, q in zip(loud, loud[1:]):
        if q - p > 30:
            runs.append((s0, p)); s0 = q
    runs.append((s0, loud[-1]))
    a, b = max(runs, key=lambda r: r[1] - r[0])
    a, b = max(0, a - 8), min(len(db), b + 30)
    write(name, x[a * 441:b * 441], rms_db=-19, fade=0.2)
