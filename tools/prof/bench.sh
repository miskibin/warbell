#!/usr/bin/env bash
# Repeatable perf baseline: runs the release build through fixed scenes x presets and prints one
# summary block per run (frame ms, main/render CPU ms, GPU pass Σ + top passes, draw counts).
#   tools/prof/bench.sh <out_dir> [preset ...]        (default presets: low high)
# Scenes: village (14-pop town at work), forest (deep forest biome), siege (night wave on the walls).
# Env passed through, so e.g. `FOREST_CASCADES=2 tools/prof/bench.sh out low` A/Bs one knob.
set -u
out=${1:-target/bench}; shift || true
presets=${*:-low high}
exe=${BENCH_EXE:-target/release/tileworld_bevy_forest.exe}
secs=${BENCH_SECS:-45}
mkdir -p "$out"
export BEVY_ASSET_ROOT="$PWD" FOREST_PERFTEST=$secs FOREST_NOVSYNC=1 FOREST_PERFFOCUS=1 FOREST_IMMORTAL=1 FOREST_TPS=1
declare -A SCENE=(
  [village]="FOREST_DEMO=work"
  [forest]="FOREST_HERO=-150,95 FOREST_TPS_AZ=0.8"
  [siege]="FOREST_WAVE=3 FOREST_DEFEND=1 FOREST_TOWN=1 FOREST_HERO=0,-14"
)
for q in $presets; do
  for s in ${BENCH_SCENES:-village forest siege}; do
    log="$out/${s}_${q}.log"
    env FOREST_QUALITY=$q ${SCENE[$s]} "$exe" > "$log" 2>&1
    clean=$(sed 's/\x1b\[[0-9;]*m//g' "$log")
    echo "== $s / $q"
    echo "$clean" | grep -oE "PERF_SUMMARY.*" | tail -1
    echo "$clean" | grep -oE "PERF_CPU.*" | tail -1
    echo "$clean" | grep -oE "GPU Σ=.*" | tail -1 | cut -c1-400
    echo "$clean" | grep -oE "PERF draws.*" | tail -1 | cut -c1-300
    echo "$clean" | grep -E "panic|Validation" | head -2
  done
done
