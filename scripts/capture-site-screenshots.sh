#!/usr/bin/env bash
# Capture polished marketing screenshots into site/screenshots/.
# Requires: cargo build, xvfb, mesa-vulkan-drivers. Run from repo root.
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"
BIN="$ROOT/target/debug/tileworld_bevy_forest"
OUT="$ROOT/site/screenshots"
LOG_DIR="$ROOT/target/screenshot-logs"
mkdir -p "$OUT" "$LOG_DIR"

if [[ ! -x "$BIN" ]]; then
  echo "Missing binary — run: cargo build"
  exit 1
fi

shot() {
  local name="$1"
  shift
  local log="$LOG_DIR/${name%.png}.log"
  echo "=== Capturing $name ==="
  # Extra args are env assignments (FOREST_CAM=..., FOREST_WAVE=1, …) — must precede the
  # binary, not trail it as argv (the game only reads these from the environment).
  # shellcheck disable=SC2068
  env BEVY_ASSET_ROOT="$ROOT" \
    FOREST_NOHUD=1 \
    FOREST_NOBLUR=1 \
    FOREST_QUALITY=ultra \
    FOREST_SHOT="$OUT/$name" \
    "$@" \
    timeout 570 xvfb-run -a -s "-screen 0 1920x1080x24" \
    "$BIN" >"$log" 2>&1 || {
      echo "FAILED: $name (see $log)"
      tail -30 "$log"
      return 1
    }
  if ! grep -q "Screenshot saved" "$log"; then
    echo "FAILED: no Screenshot saved line for $name"
    tail -30 "$log"
    return 1
  fi
  if grep -q "Path not found" "$log"; then
    echo "WARN: asset 404s in $name — check BEVY_ASSET_ROOT"
    grep "Path not found" "$log" | head -5
  fi
  echo "OK: $name"
}

# Hero banner — fortified keep at golden hour
shot castle-after.png \
  FOREST_DEFEND=1 FOREST_TOWN=full \
  FOREST_TIME=0.42 \
  FOREST_CAM="0,46,58,0,5,-6"

# Siege action — fortified keep under assault (courtyard framing from batch2)
shot fight-orks.png \
  FOREST_WAVE=1 FOREST_DEFEND=1 FOREST_TOWN=1 \
  FOREST_TIME=0.35 \
  FOREST_EQUIP="sword_gold,gold_armor" \
  FOREST_CAM="6,11,18,0,1.6,6"

# Night assault — wave boot forces midnight
shot night-combat.png \
  FOREST_WAVE=1 FOREST_DEFEND=1 \
  FOREST_EQUIP="sword_gold,gold_armor" \
  FOREST_CAM="-5,10,15,0,1.5,5"

# Biomes — hero staged in each region
shot desert-biome.png \
  FOREST_HERO="60,-39" FOREST_TIME=0.30 \
  FOREST_CAM="48,12,-48,60,3,-39"

shot snow-biome.png \
  FOREST_HERO="-69,-45" FOREST_TIME=0.42 \
  FOREST_CAM="-82,14,-38,-69,2.5,-45"

shot forest-biome.png \
  FOREST_HERO="-60,39" FOREST_TIME=0.28 \
  FOREST_CAM="-72,10,48,-60,2,-39"

shot rock-biome.png \
  FOREST_HERO="66,4" FOREST_TIME=0.32 \
  FOREST_CAM="78,16,-6,66,4,4"

# Blight / ork fortress — approach from the causeway
shot ork-fortress.png \
  FOREST_HERO="12,102" FOREST_TIME=0.50 \
  FOREST_CAM="12,18,98,12,10,118"

shot swamp-fortress.png \
  FOREST_HERO="0,90" FOREST_TIME=0.50 \
  FOREST_CAM="12,18,98,12,10,118"

# Character / world vignettes
shot hero-knight.png \
  FOREST_EQUIP="sword_gold,gold_armor" \
  FOREST_TIME=0.35 \
  FOREST_CAM="0.7,0.9,-13.5,0,0.5,-15"

shot ork-warrior.png \
  FOREST_ORKLINE="0,-6" FOREST_TIME=0.35 \
  FOREST_CAM="0,3.2,5,0,1.5,-6"

shot golden-hour.png \
  FOREST_TIME=0.72 \
  FOREST_CAM="0,62,112,0,0,-20"

shot town-dusk.png \
  FOREST_TOWN=full FOREST_DEFEND=1 FOREST_TIME=0.70 \
  FOREST_CAM="-22,14,28,-4,3,8"

# Start screen keeps the menu UI (no FOREST_NOHUD override for this one)
echo "=== Capturing start-screen.png ==="
BEVY_ASSET_ROOT="$ROOT" \
  FOREST_NOBLUR=1 \
  FOREST_QUALITY=ultra \
  FOREST_MENU=1 \
  FOREST_SHOT="$OUT/start-screen.png" \
  timeout 570 xvfb-run -a -s "-screen 0 1920x1080x24" \
  "$BIN" >"$LOG_DIR/start-screen.log" 2>&1
grep -q "Screenshot saved" "$LOG_DIR/start-screen.log" && echo "OK: start-screen.png"

echo ""
echo "All screenshots written to $OUT"
