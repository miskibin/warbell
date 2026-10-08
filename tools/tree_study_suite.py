"""Run this only after art freeze, with no concurrent compiler or Blender renderer."""
from pathlib import Path
import subprocess
import sys
import json
import hashlib

ROOT = Path(__file__).resolve().parents[1]
BIN = ROOT / "target/tree-study/bin"
EXE_SHA = hashlib.sha256((BIN / "trees.exe").read_bytes()).hexdigest()
ASSET_SHA = {p.name: hashlib.sha256(p.read_bytes()).hexdigest()
             for p in sorted((ROOT / "assets/models/blender_trees").glob("*"))
             if p.suffix in (".json", ".png")}

def run(exe, label, scene, *extra):
    previous = ROOT / "target/tree-study" / label / "run.json"
    if previous.is_file():
        result = json.loads(previous.read_text(encoding="utf-8"))
        expected_quality = extra[extra.index("--quality") + 1] if "--quality" in extra else "high"
        expected_shot = "--shot" in extra
        matches = (result.get("exe_sha256") == EXE_SHA and result.get("tree_asset_sha256") == ASSET_SHA
                   and result.get("scene") == scene and result.get("quality") == expected_quality
                   and result.get("blender") == ("--blender" in extra)
                   and result.get("screenshot") == expected_shot)
        complete = result.get("screenshot_saved") if expected_shot else "performance" in result
        if matches and result.get("exit_code") == 0 and complete and not result.get("errors"):
            print(f"SKIP completed {label}", flush=True)
            return
        raise RuntimeError(f"Existing run {label} is incomplete or has different inputs; archive it explicitly before retry")
    subprocess.run([sys.executable, str(ROOT / "tools/tree_study.py"),
                    "--exe", str(BIN / exe), "--label", label, "--scene", scene,
                    "--seconds", "60", *extra], cwd=ROOT, check=True)

for scene in ("forest_close", "forest_wide", "siege"):
    for rep in (1, 2, 3):
        for candidate in ((False, True) if rep != 2 else (True, False)):
            label = "new" if candidate else "base"
            run("trees.exe", f"uncapped-{scene}-{label}-{rep}", scene,
                *(["--blender"] if candidate else []))

for candidate in (False, True):
    label = "new" if candidate else "base"
    run("trees.exe", f"uncapped-low-{label}-1", "forest_close", "--quality", "low",
        *(["--blender"] if candidate else []))

subprocess.run([sys.executable, str(ROOT / "tools/tree_study_summary.py")], cwd=ROOT, check=True)

# Captures happen only after timed runs so PNG encoding never contaminates the samples.
for scene in ("trees", "forest_close", "forest_wide"):
    for candidate in (False, True):
        label = "new" if candidate else "base"
        run("trees.exe", f"report-{scene}-{label}", scene, "--shot",
            *(["--blender"] if candidate else []))
