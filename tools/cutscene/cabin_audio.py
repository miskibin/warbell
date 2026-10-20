#!/usr/bin/env python3
"""P.0 "Below Deck" (ShipCabin) audio: the cue list + the offline soundtrack mix.

VIDEO rig only: the scene exists to be filmed (FOREST_SCENE=cabin + FOREST_CLIP), never triggered in game.

    python tools/cutscene/cabin_audio.py gen                 # write assets/cutscene/ship_cabin.cues
    python tools/cutscene/cabin_audio.py mix in.mp4 out.mp4  # mux the soundtrack onto a captured clip

Cue format as in fort_audio.py, plus `lowpass=<Hz>` (offline mix only: a sound heard through the deck).
Timings mirror `src/scenes/cabin.rs` (VO, STRIKES, `wat_track`, `door_open`, FADE_OUT / END) — keep
them in step. Cues whose file isn't in the repo yet are written anyway (the scene is built as if they
existed) and skipped by `mix` / the game with a "missing" note; the list of what's still to be made is
docs/superpowers/specs/2026-10-20-p0-below-deck-cutscene.md.
"""
import math
import subprocess
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
ASSETS = ROOT / "assets"
CUES = ASSETS / "cutscene" / "ship_cabin.cues"
END = 86.0
BLACK = 85.5  # fade-to-black done (cabin.rs FADE_OUT)
MUSIC = "audio/cutscene/below_deck.ogg"  # A1-1 "Below Deck" (docs/audio/cutscene-music-suno.md) — MISSING
# The settlers' hymn in the hold below (the in-game day hymn), heard up through the deck.
HYMN = "audio/day-hymn-1.ogg"

# Recorded Act I lines (docs/audio/elevenlabs-akt-1.md) at their scene times — cabin.rs VO / LINES:
# (start s, clip, trim from, trim to). The takes are cut at their own pauses so the beats are the scene's.
VC = "audio/vo/campaign/"
VO = [
    (12.2, VC + "a1_harrow_01.ogg", 0.0, 0.9),    # Harrow: "Tighter."
    (14.85, VC + "a1_harrow_01.ogg", 2.4, 7.6),   # Harrow: "You've done that strap every day for fifteen years and it's still wrong."
    (21.3, VC + "a1_wat_01.ogg", 0.0, 0.94),      # Wat: "Yes, Ser."
    (23.6, VC + "a1_wat_05.ogg", 0.0, 2.0),       # Wat: (effort) "Hnn." (exhales) "There."
    (26.6, VC + "a1_harrow_07.ogg", 0.0, 5.92),   # Harrow: (sighs) "The letter. To the Reeve. Today, if the sea allows."
    (39.0, VC + "a1_harrow_02.ogg", 0.0, 2.6),    # Harrow: "Forty peasants and a Reeve."
    (42.4, VC + "a1_harrow_02.ogg", 3.0, 7.2),    # Harrow: "That's what the Crown sends a man after the eastern campaign."
    (49.6, VC + "a1_harrow_03.ogg", 0.0, 2.95),   # Harrow: "Do you know what they said about me in the city?"
    (53.6, VC + "a1_harrow_03.ogg", 3.2, 4.43),   # Harrow: "Before it burned?"
    (57.0, VC + "a1_wat_02.ogg", 0.0, 0.92),      # Wat: "No, Ser."
    (59.4, VC + "a1_harrow_04.ogg", 0.0, 1.5),    # Harrow: "Neither do I."
    (62.6, VC + "a1_harrow_04.ogg", 2.9, 5.95),   # Harrow: "Nobody looks at the man behind the shield, Wat."
    (66.6, VC + "a1_harrow_04.ogg", 6.4, 7.84),   # Harrow: "That's the point of you."
    (72.2, VC + "a1_harrow_08.ogg", 0.0, 1.51),   # Harrow: "Stop hovering."
]
# Lightning (cabin.rs STRIKES): (time, strength). Thunder rolls in after each, a far storm.
STRIKES = [(5.4, 0.55), (18.9, 0.2), (47.6, 0.35), (53.1, 1.0), (80.6, 0.6)]
# Wat's walks (cabin.rs `wat_track`): (from, to) seconds — round the lantern to the table, to the
# door, out into the passage.
WALKS = [(30.5, 32.1), (33.6, 35.5), (75.1, 78.1)]

SFX = "audio/sfx/cutscene/"


def gen():
    rows = []  # (t, path, vol, flags)
    # The storm the whole way — the wind alone on the black under the title card, then the cabin
    # around it (the 6.5 s cabin loop: timbers groaning, rain drumming overhead, the hull slapped,
    # the lantern squeaking on its hook); the cabin dips with the fade out, the wind stays.
    rows.append((0.0, "audio/wind.ogg", 0.16, "video loop"))
    rows.append((0.0, SFX + "cs_ship_cabin.ogg", 0.55, f"video loop fadein=4.6 fadeout={BLACK - 1.6:.1f},1.6"))
    # Green water slamming the quarter: once on the black, under the window insert, over Harrow alone.
    for t, v, pan in [(1.2, 0.5, -0.3), (47.1, 0.4, 0.25), (79.3, 0.4, -0.2)]:
        rows.append((t, SFX + "cs_wave_slam.ogg", v, f"video pan={pan}"))  # MISSING
    # Thunder ~1.5 s behind each flash (a few km off), louder with the strike.
    for t, k in STRIKES:
        rows.append((round(t + 1.2 + 0.6 * (1 - k), 2), SFX + "cs_thunder_far.ogg", round(0.25 + 0.5 * k, 2), "video"))  # MISSING
    # Wat's work at the greave: a pull that won't sit, the haul after "Tighter.", the buckle home on "Hnn."
    for t, v in [(10.4, 0.4), (13.4, 0.5), (24.0, 0.55)]:
        rows.append((t, SFX + "cs_strap_cinch.ogg", v, "video"))  # MISSING
    # Harrow's harness as he turns: to the table, to the windows, back to Wat, away again.
    for t, v in [(26.1, 0.2), (46.7, 0.35), (58.9, 0.3), (69.7, 0.3)]:
        rows.append((t, SFX + "cs_armour_shift.ogg", v, "video"))  # MISSING
    # Wat's boots on the planking, one per half-stride.
    n = 0
    for a, b in WALKS:
        t = a
        while t < b:
            rows.append((round(t, 2), f"audio/footsteps/wood-{n % 4 + 1}.ogg", 0.32, f"video pan={-0.15 if n % 2 else 0.15}"))
            t += 0.52
            n += 1
    # The letter taken off the table; the door's latch and hinges as he goes (cabin.rs `door_open`).
    rows.append((32.65, SFX + "cs_paper_rustle.ogg", 0.4, "video"))  # MISSING
    rows.append((74.4, SFX + "cs_door_creak.ogg", 0.5, "video pan=-0.3"))  # MISSING
    # The hymn below: heard through the deck as Wat crosses to the door (it's what stops him there),
    # under "Forty peasants…", gone when Harrow turns to the windows; then plainer through the open
    # door once Wat has gone, staying with Harrow alone into the black.
    rows.append((31.6, HYMN, 0.3, "video trim=8,26 lowpass=420 fadein=3.0 fadeout=46.0,2.4"))
    rows.append((74.6, HYMN, 0.24, f"video trim=24,40 lowpass=1100 pan=-0.25 fadein=1.4 fadeout={BLACK - 3.0:.1f},3.0"))
    # Music: A1-1 enters as Harrow turns to the storm for the city (~-28 dB, ducked under the voices
    # in `mix`), and goes out with the picture.
    rows.append((46.6, MUSIC, 0.22, f"video fadein=3.0 fadeout={BLACK - 2.5:.1f},2.5"))
    for t, p, a, b in VO:
        rows.append((t, p, 1.0, f"trim={a},{b}"))

    rows.sort(key=lambda r: r[0])
    out = [
        "# P.0 'Below Deck' (ShipCabin) audio cues — GENERATED by tools/cutscene/cabin_audio.py gen.",
        "# VIDEO rig only — never triggered in game (FOREST_SCENE=cabin). `#` lines are skipped by scenes.rs::parse_cues.",
        "# Files not in the repo yet are listed anyway and skipped (see docs/superpowers/specs/2026-10-20-p0-below-deck-cutscene.md).",
        "# time  path(under assets/)  volume  [video] [loop] [pan=] [fadein=] [fadeout=t,s] [trim=a,b] [lowpass=Hz]   (video = offline mix only)",
    ]
    out += [f"{t:6.2f}  {p}  {v}  {f}".rstrip() for t, p, v, f in rows]
    CUES.write_text("\n".join(out) + "\n", encoding="utf-8")
    missing = sorted({p for _, p, _, _ in rows if not (ASSETS / p).exists()})
    print(f"wrote {CUES} ({len(rows)} cues)")
    for m in missing:
        print("  missing (cue kept):", m)


def read_cues():
    rows = []
    for line in CUES.read_text(encoding="utf-8").splitlines():
        line = line.strip()
        if not line or line.startswith("#"):
            continue
        f = line.split()
        rows.append((float(f[0]), f[1], float(f[2]), f[3:]))
    return rows


def flag(flags, name):
    return next((x[len(name) + 1:] for x in flags if x.startswith(name + "=")), None)


def mix(video, out):
    rows = [r for r in read_cues() if (ASSETS / r[1]).exists()]
    for m in sorted({r[1] for r in read_cues() if not (ASSETS / r[1]).exists()}):
        print("skip (missing):", m)
    inputs, filters, labels = [], [], []
    vo_spans = [(t, t + b - a) for t, p, a, b in VO if (ASSETS / p).exists()]
    for i, (t, path, vol, flags) in enumerate(rows):
        loop = "loop" in flags
        if loop:
            inputs += ["-stream_loop", "-1"]
        inputs += ["-i", str(ASSETS / path)]
        f = f"[{i + 1}:a]"
        if loop:
            f += f"atrim=0:{END},"
        trim = flag(flags, "trim")
        if trim:
            a, b = (float(x) for x in trim.split(","))
            f += f"atrim={a}:{b},asetpts=PTS-STARTPTS,afade=t=in:d=0.015,afade=t=out:st={b - a - 0.015:.3f}:d=0.015,"
        lp = flag(flags, "lowpass")
        if lp:
            f += f"lowpass=f={lp},lowpass=f={lp},"
        pan = flag(flags, "pan")
        if pan is None:
            f += "aformat=sample_rates=48000:channel_layouts=stereo"
        else:
            a = (float(pan) + 1) * math.pi / 4
            f += f"aformat=sample_rates=48000:channel_layouts=mono,pan=stereo|c0={math.cos(a):.3f}*c0|c1={math.sin(a):.3f}*c0"
        gain = f"volume={vol}"
        fi = flag(flags, "fadein")
        if fi:
            gain += f",afade=t=in:st=0:d={fi}"
        fo = flag(flags, "fadeout")
        if fo:
            st, d = (float(x) for x in fo.split(","))
            gain += f",afade=t=out:st={max(st - t, 0):.2f}:d={d}"
        if path in (MUSIC, HYMN) or path.endswith("cs_ship_cabin.ogg"):
            # duck the music, the hymn and the hull bed under the spoken lines on one smooth envelope
            env = "0"
            for a, b in vo_spans:
                g = f"clip((t-{a - 0.5 - t:.2f})/0.5,0,1)*clip(({b + 1.2 - t:.2f}-t)/1.2,0,1)"
                env = f"max({env},{g})"
            depth = 0.45 if path == MUSIC else 0.3
            gain += f",volume='1-{depth}*{env}':eval=frame"
        delay = int(t * 1000)
        filters.append(f"{f},{gain},adelay={delay}|{delay}[a{i}]")
        labels.append(f"[a{i}]")
    chain = ";".join(filters) + f";{''.join(labels)}amix=inputs={len(labels)}:normalize=0:dropout_transition=0,"
    chain += f"alimiter=limit=0.95,atrim=0:{END},afade=t=out:st={END - 1.0}:d=1.0[mix]"
    cmd = ["ffmpeg", "-y", "-loglevel", "error", "-i", video] + inputs + [
        "-filter_complex", chain, "-map", "0:v", "-map", "[mix]", "-c:v", "copy",
        "-c:a", "aac", "-b:a", "192k", "-shortest", "-movflags", "+faststart", out,
    ]
    subprocess.run(cmd, check=True)
    print("wrote", out)


if __name__ == "__main__":
    if len(sys.argv) >= 2 and sys.argv[1] == "gen":
        gen()
    elif len(sys.argv) == 4 and sys.argv[1] == "mix":
        mix(sys.argv[2], sys.argv[3])
    else:
        sys.exit(__doc__)
