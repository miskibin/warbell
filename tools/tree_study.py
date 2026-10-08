"""Reproducible, sequential Warbell tree A/B runs. No player saves are touched."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import re
import subprocess
import time

ROOT = Path(__file__).resolve().parents[1]
SCENES = {
    "forest_close": {"FOREST_CAM": "-98,4.5,80,-104,1.8,67"},
    "forest_wide": {"FOREST_CAM": "-82,18,90,-104,2,63"},
    "siege": {"FOREST_CAM": "0,15,30,0,2,-8", "FOREST_WAVE": "7",
              "FOREST_PERF_COUNT": "96", "FOREST_DEFEND": "1",
              "FOREST_TOWN": "full", "FOREST_ARCHERS": "64"},
    "trees": {"FOREST_TREELINE": "-26,18", "FOREST_CAM": "-32,3.7,27,-32,1.8,18"},
    "rts": {"FOREST_RTS": "1", "FOREST_PERF_ARMY": "72", "FOREST_RTS_CAM": "0,0,60"},
}

def run(args):
    out = ROOT / "target/tree-study" / args.label
    out.mkdir(parents=True, exist_ok=False)
    exe = Path(args.exe).resolve()
    env = {k: v for k, v in os.environ.items() if not k.startswith("FOREST_")}
    env.update(BEVY_ASSET_ROOT=str(ROOT), APPDATA=str(out / "userdata"),
               RUST_LOG="info", NO_COLOR="1", FOREST_FREEROAM="1",
               FOREST_RES="1920x1080", FOREST_QUALITY=args.quality,
               FOREST_NOVSYNC="1", FOREST_TIME="0.28", FOREST_DAY="1000000",
               FOREST_IMMORTAL="1")
    env.update(SCENES[args.scene])
    if args.scene == "siege":
        env["FOREST_TIME"] = "0.75"
    if args.blender:
        env["FOREST_BLENDERTREES"] = "1"
    if args.shot:
        env.update(FOREST_SHOT=str(out / "shot.png"), FOREST_NOHUD="1",
                   FOREST_SHOT_WARMUP="35")
    else:
        env["FOREST_PERFTEST"] = str(args.seconds)
        if not args.natural_sky:
            env["FOREST_PERF_SKY"] = "0.75" if args.scene == "siege" else "0.28"
    record = {"label": args.label, "scene": args.scene, "quality": args.quality,
              "blender": args.blender, "screenshot": args.shot,
              "exe": str(exe), "exe_sha256": hashlib.sha256(exe.read_bytes()).hexdigest(),
              "git_head": subprocess.check_output(["git", "rev-parse", "HEAD"], cwd=ROOT, text=True).strip(),
              "tree_asset_sha256": {p.name: hashlib.sha256(p.read_bytes()).hexdigest()
                   for p in sorted((ROOT / "assets/models/blender_trees").glob("*"))
                   if p.suffix in (".json", ".png")},
              "env": {k: v for k, v in env.items() if k.startswith("FOREST_") or k in ("BEVY_ASSET_ROOT", "APPDATA")}}
    (out / "run.json").write_text(json.dumps(record, indent=2), encoding="utf-8")
    si = subprocess.STARTUPINFO()
    si.dwFlags |= subprocess.STARTF_USESHOWWINDOW
    si.wShowWindow = 0
    start = time.monotonic()
    with (out / "stdout.log").open("w", encoding="utf-8") as stdout, (out / "stderr.log").open("w", encoding="utf-8") as stderr:
        proc = subprocess.Popen([str(exe)], cwd=ROOT, env=env, stdout=stdout, stderr=stderr, startupinfo=si)
        print(f"RUN {args.label} pid={proc.pid}", flush=True)
        try:
            code = proc.wait(timeout=args.seconds + 180)
        except subprocess.TimeoutExpired:
            proc.kill()
            proc.wait()
            raise RuntimeError(f"Owned game process {proc.pid} exceeded timeout")
    record.update(exit_code=code, wall_seconds=time.monotonic() - start)
    log = (out / "stdout.log").read_text(encoding="utf-8") + (out / "stderr.log").read_text(encoding="utf-8")
    log = re.sub(r"\x1b\[[0-9;]*m", "", log)
    summary = re.search(r"PERF_SUMMARY mode=(\w+) frames=(\d+) mean_ms=([\d.]+) p50_ms=([\d.]+) p95_ms=([\d.]+) p99_ms=([\d.]+) max_ms=([\d.]+)", log)
    if summary:
        record["performance"] = dict(zip(("mode", "frames", "mean_ms", "p50_ms", "p95_ms", "p99_ms", "max_ms"), summary.groups()))
        record["performance"] = {k: (v if k == "mode" else float(v)) for k, v in record["performance"].items()}
    record["errors"] = [line for line in log.splitlines() if re.search(r"Path not found|panicked|Validation Error|PERF_SUMMARY INVALID|ERROR", line)]
    record["screenshot_saved"] = "Screenshot saved" in log and (out / "shot.png").is_file()
    (out / "run.json").write_text(json.dumps(record, indent=2), encoding="utf-8")
    print(json.dumps({k: record[k] for k in ("label", "exit_code", "wall_seconds", "errors", "screenshot_saved")}) , flush=True)
    if summary:
        print(json.dumps(record["performance"]), flush=True)
    if code or record["errors"] or (args.shot and not record["screenshot_saved"]) or (not args.shot and not summary):
        raise SystemExit(1)

if __name__ == "__main__":
    p = argparse.ArgumentParser()
    p.add_argument("--exe", required=True)
    p.add_argument("--label", required=True)
    p.add_argument("--scene", choices=SCENES, required=True)
    p.add_argument("--quality", choices=("low", "high", "ultra"), default="high")
    p.add_argument("--seconds", type=int, default=60)
    p.add_argument("--blender", action="store_true")
    p.add_argument("--shot", action="store_true")
    p.add_argument("--natural-sky", action="store_true", help="Pristine-main behavioral baseline; not the pinned A/B")
    run(p.parse_args())
