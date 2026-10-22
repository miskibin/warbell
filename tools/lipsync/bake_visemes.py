"""Mouth shapes for every recorded voice line -> assets/audio/vo/visemes.txt (read by src/lipsync.rs).

    python -I tools/lipsync/bake_visemes.py <rhubarb.exe> [--only <substring>]

Runs Rhubarb Lip Sync (github.com/DanielSWolf/rhubarb-lip-sync, offline, as a tool: the game only reads its
output) on each .ogg under assets/audio/vo/{campaign,life,npc,rival,ork}, handing it the line's transcript when
the code has one (the `line(..)` / `life(..)` / `script(..)` tables in src/audio/lines.rs, and the `"...ogg", ... // "text"`
VO tables of the cutscenes), which makes its phoneme timing much better than recognition alone. Hero lines are
skipped: the knight's face is never shown talking.

Output, one clip per line:  <path under assets/>\\t<ms>:<shape> <ms>:<shape> ...
Shapes are Rhubarb's: A (M B P, lips shut) B (most consonants, teeth together) C (EH AE, open) D (AA, wide) E (AO
ER, rounded) F (UW OW W, puckered) G (F V, lip under teeth) H (L, tongue up) X (rest). Re-run after adding or
re-recording a clip; existing entries are kept unless their .ogg is newer than the table.
"""
import os, re, subprocess, sys, tempfile
from concurrent.futures import ThreadPoolExecutor

ROOT = os.path.dirname(os.path.dirname(os.path.dirname(os.path.abspath(__file__))))
ASSETS = os.path.join(ROOT, "assets")
OUT = os.path.join(ASSETS, "audio", "vo", "visemes.txt")
DIRS = ["campaign", "life", "npc", "rival", "ork"]

rhubarb = sys.argv[1]
only = sys.argv[sys.argv.index("--only") + 1] if "--only" in sys.argv else None

# transcripts
text = {}
call = re.compile(r'\b(?:line|life|script|script_planned)\(\s*"([A-Za-z0-9_]+)"\s*,[^"\n]*?"((?:[^"\\]|\\.)*)"\s*\)')
vo_tab = re.compile(r'"(audio/vo/[^"]+\.ogg)"[^\n]*?//\s*"(.*)"\s*$')
for dp, _, fs in os.walk(os.path.join(ROOT, "src")):
    for f in fs:
        if not f.endswith(".rs"):
            continue
        for ln in open(os.path.join(dp, f), encoding="utf-8"):
            for m in call.finditer(ln):
                text.setdefault(m.group(1), m.group(2))
            m = vo_tab.search(ln)
            if m:
                text[m.group(1)] = m.group(2)

clips = []
for d in DIRS:
    for dp, _, fs in os.walk(os.path.join(ASSETS, "audio", "vo", d)):
        for f in sorted(fs):
            if f.endswith(".ogg"):
                rel = os.path.relpath(os.path.join(dp, f), ASSETS).replace("\\", "/")
                if only is None or only in rel:
                    clips.append(rel)

old = {}
if os.path.exists(OUT):
    for ln in open(OUT, encoding="utf-8"):
        if "\t" in ln:
            k, v = ln.rstrip("\n").split("\t", 1)
            old[k] = v
stamp = os.path.getmtime(OUT) if os.path.exists(OUT) else 0.0


def clean(t):
    t = re.sub(r"\[[^\]]*\]", " ", t)          # ElevenLabs stage tags
    t = t.replace("…", "...").replace("—", ", ").replace("’", "'")
    return re.sub(r"\s+", " ", t).strip()


def run(rel):
    path = os.path.join(ASSETS, rel)
    if rel in old and os.path.getmtime(path) <= stamp and not os.environ.get('REDO'):
        return rel, old[rel], "kept"
    stem = os.path.splitext(os.path.basename(rel))[0]
    t = text.get(rel) or text.get(stem)
    with tempfile.TemporaryDirectory() as td:
        args = [rhubarb, "-q", "-f", "tsv", "--extendedShapes", "GHX", "-o", os.path.join(td, "out.tsv")]
        if t:
            open(os.path.join(td, "d.txt"), "w", encoding="utf-8").write(clean(t))
            args += ["-d", os.path.join(td, "d.txt")]
        r = subprocess.run(args + [path], capture_output=True, text=True)
        if r.returncode != 0:
            return rel, None, r.stderr.strip()[-200:]
        keys = []
        for ln in open(os.path.join(td, "out.tsv"), encoding="utf-8"):
            p = ln.split()
            if len(p) == 2:
                keys.append(f"{round(float(p[0]) * 1000)}:{p[1]}")
    return rel, " ".join(keys), "text" if t else "no text"


res = dict(old)
with ThreadPoolExecutor(max_workers=6) as ex:
    for i, (rel, v, how) in enumerate(ex.map(run, clips)):
        if v is None:
            print("FAIL", rel, how, flush=True)
            continue
        res[rel] = v
        if how != "kept":
            print(f"[{i + 1}/{len(clips)}] {rel} ({how})", flush=True)
with open(OUT, "w", encoding="utf-8", newline="\n") as f:
    f.write("# Mouth shapes per voice clip (tools/lipsync/bake_visemes.py, Rhubarb Lip Sync): <clip>\\t<ms>:<shape> ...\n")
    for k in sorted(res):
        f.write(f"{k}\t{res[k]}\n")
print("wrote", OUT, len(res), "clips")
