"""Capture the actual campaign or the isolated forest art slice.

Each run uses private settings/save data, a fresh output directory and a fixed camera.
Optional uncapped timings are a separate world experiment, never tree-study data.
"""
import argparse
import hashlib
import json
import math
import os
from pathlib import Path
import re
import subprocess
import time

ROOT = Path(__file__).resolve().parents[1]
SCENES = {
    "forest-slice": {"FOREST_FORESTSLICE": "1"},
    "forest-detail": {"FOREST_FORESTSLICE": "1"},
    "castle": {"FOREST_CAM": "24,9,28,0,2.5,0", "FOREST_TOWN": "full", "FOREST_DEFEND": "1"},
    "village": {"FOREST_CAM": "-29,4.5,24,-10,1.4,10", "FOREST_TOWN": "full", "FOREST_DEFEND": "1"},
    "forest": {"FOREST_CAM": "-98,4.5,80,-104,1.8,67"},
    "path": {"FOREST_CAM": "-98,1.6,81,-103,0.1,75"},
    "trees": {"FOREST_TREELINE": "-26,18", "FOREST_CAM": "-32,3.7,27,-32,1.8,18"},
    "swamp": {"FOREST_CAM": "12,5,57,0.5,2.5,43.2"},
    "snow": {"FOREST_CAM": "-103,6,9,-118,4,-6.1"},
    "desert": {"FOREST_CAM": "59,7,-47,43.6,3,-62.7"},
    "rocky": {"FOREST_CAM": "49,6,43,35.9,2.6,28.7"},
    "fort": {"FOREST_CAM": "-13,13,140,12,5,161"},
    "gameplay": {"FOREST_TPS": "1", "FOREST_HERO": "-18,24", "FOREST_TOWN": "full", "FOREST_DEFEND": "1"},
}


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--exe", type=Path, default=ROOT / "target/world-preview/bin/warbell-world.exe")
    parser.add_argument("--label", required=True)
    parser.add_argument("--scene", choices=SCENES, required=True)
    parser.add_argument("--mode", choices=("native", "trees", "world", "slice"), default="world")
    parser.add_argument("--quality", choices=("low", "high", "ultra"), default="high")
    parser.add_argument("--warmup", type=int, default=45)
    parser.add_argument("--profile", type=int, metavar="SECONDS", help="Time an uncapped run instead of taking a screenshot")
    parser.add_argument("--set", action="append", default=[], metavar="FOREST_KEY=VALUE")
    args = parser.parse_args()
    out = ROOT / "target/world-preview" / args.label
    out.mkdir(parents=True, exist_ok=False)
    exe = args.exe.resolve()
    env = {k: v for k, v in os.environ.items() if not k.startswith("FOREST_")}
    env.update(BEVY_ASSET_ROOT=str(ROOT), APPDATA=str(out / "userdata"), RUST_LOG="info", NO_COLOR="1",
               FOREST_SHOT=str(out / "shot.png"), FOREST_SHOT_WARMUP=str(args.warmup),
               FOREST_RES="1600x900", FOREST_QUALITY=args.quality, FOREST_NOHUD="1",
               FOREST_FREEROAM="1", FOREST_IMMORTAL="1", FOREST_TIME="0.28", FOREST_DAY="1000000", FOREST_MUTE="1")
    env.update(SCENES[args.scene])
    if args.scene == "forest-detail":
        layout = json.loads((ROOT / "assets/models/forest_slice/layout.json").read_text(encoding="utf-8"))
        pose = layout["closeup_camera"]
        world_pose = [a + b for point in (pose["eye"], pose["target"])
                      for a, b in zip(point, layout["origin"])]
        env["FOREST_CAM"] = ",".join(str(v) for v in world_pose)
        env["FOREST_FOCAL"] = str(math.dist(pose["eye"], pose["target"]))
    if args.profile:
        env.pop("FOREST_SHOT")
        env.pop("FOREST_SHOT_WARMUP")
        env.update(FOREST_PERFTEST=str(args.profile), FOREST_NOVSYNC="1", FOREST_PERF_SKY="0.28")
    if args.mode in ("trees", "world"):
        env["FOREST_BLENDERTREES"] = "1"
    if args.mode == "world":
        env["FOREST_BLENDERWORLD"] = "1"
    if args.mode == "slice":
        env["FOREST_FORESTSLICE"] = "1"
    for item in args.set:
        key, value = item.split("=", 1)
        if not key.startswith("FOREST_"):
            parser.error("--set only accepts FOREST_ options")
        env[key] = value
    record = {"scene": args.scene, "mode": args.mode, "quality": args.quality,
              "exe_sha256": hashlib.sha256(exe.read_bytes()).hexdigest(),
              "git_head": subprocess.check_output(["git", "rev-parse", "HEAD"], cwd=ROOT, text=True).strip(),
              "profile_seconds": args.profile,
              "source_sha256": hashlib.sha256(b"".join(
                   str(p.relative_to(ROOT)).encode() + b"\0" + p.read_bytes()
                   for p in sorted([*ROOT.glob("src/**/*.rs"), *ROOT.glob("assets/shaders/*.wgsl")])
              )).hexdigest(),
              "asset_sha256": {str(p.relative_to(ROOT)): hashlib.sha256(p.read_bytes()).hexdigest()
                   for folder in ("blender_trees", "blender_environment", "forest_slice")
                   for p in sorted((ROOT / "assets/models" / folder).rglob("*"))
                   if p.is_file() and p.suffix.lower() in (".json", ".png", ".jpg", ".jpeg", ".bin", ".glb", ".gltf")},
              "env": {k: v for k, v in env.items() if k.startswith("FOREST_") or k in ("APPDATA", "BEVY_ASSET_ROOT")}}
    (out / "run.json").write_text(json.dumps(record, indent=2), encoding="utf-8")
    startup = subprocess.STARTUPINFO()
    startup.dwFlags |= subprocess.STARTF_USESHOWWINDOW
    startup.wShowWindow = 0
    start = time.monotonic()
    with (out / "game.log").open("w", encoding="utf-8") as log:
        child = subprocess.Popen([str(exe)], cwd=ROOT, env=env, stdout=log, stderr=subprocess.STDOUT, startupinfo=startup)
        print(f"CAPTURE {args.label} pid={child.pid}", flush=True)
        try:
            code = child.wait(timeout=(args.profile or args.warmup) + 240)
        except subprocess.TimeoutExpired:
            child.kill()
            child.wait()
            raise RuntimeError(f"Capture process {child.pid} exceeded timeout")
    log = re.sub(r"\x1b\[[0-9;]*m", "", (out / "game.log").read_text(encoding="utf-8"))
    errors = [line for line in log.splitlines() if re.search(r"Path not found|panicked|Validation Error|PERF_SUMMARY INVALID|ERROR", line)]
    saved = "Screenshot saved" in log and (out / "shot.png").is_file()
    summary = re.search(r"PERF_SUMMARY mode=(\w+) frames=(\d+) mean_ms=([\d.]+) p50_ms=([\d.]+) p95_ms=([\d.]+) p99_ms=([\d.]+) max_ms=([\d.]+)", log)
    if summary:
        record["performance"] = {k: (v if k == "mode" else float(v)) for k, v in zip(
            ("mode", "frames", "mean_ms", "p50_ms", "p95_ms", "p99_ms", "max_ms"), summary.groups())}
    if saved:
        import struct
        record["screenshot_resolution"] = list(struct.unpack(">II", (out / "shot.png").read_bytes()[16:24]))
    record.update(exit_code=code, errors=errors, screenshot_saved=saved,
                  audio_playback_disabled="HARNESS_AUDIO silent: campaign playback disabled" in log,
                  wall_seconds=time.monotonic()-start)
    (out / "run.json").write_text(json.dumps(record, indent=2), encoding="utf-8")
    print(json.dumps({"label": args.label, "exit_code": code, "errors": errors, "screenshot_saved": saved}), flush=True)
    if summary:
        print(json.dumps(record["performance"]), flush=True)
    if code or errors or (not args.profile and not saved) or (args.profile and not summary):
        raise SystemExit(1)


if __name__ == "__main__":
    main()
