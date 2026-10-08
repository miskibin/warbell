"""One bounded, reproducible forest-slice composition pass after CC0 v5 QA.

Only the approval scene changes. The source snapshot is the validated v5 PNG
capture layout; this script always starts from it, so reruns do not accumulate
new plants or sink trunks twice.
"""
from collections import Counter
from pathlib import Path
import hashlib
import json
import math
import random

ROOT = Path(__file__).resolve().parents[2]
SOURCE = ROOT / "art/forest_preview/layout_v5_before_final_tune.json"
OUTPUT = ROOT / "assets/models/forest_slice/layout.json"
layout = json.loads(SOURCE.read_text(encoding="utf-8"))
rows = layout["instances"]
rng = random.Random(20261008)

layout["camera"] = {
    "eye": [10.5, 1.85, 19.3],
    "target": [-3.5, 1.35, -8.5],
}

# These are the lime legacy crowns directly behind the path. Keep all other
# distant legacy trees, preserving the already-reviewed 48 m composition.
tree_targets = [
    (-8.0, -13.0), (-9.93809, -19.48328), (-11.74739, -21.5768),
    (-4.1, -21.8), (-0.60527, -19.10341), (1.01853, -22.54688),
    (4.16294, -14.34454), (-8.86543, -28.94882),
    (-3.32733, -28.04465), (-15.2, -39.8), (-27.3, -49.3),
    (-19.7, -51.4),
]
used = set()
for tx, tz in tree_targets:
    candidates = [
        (math.hypot(row["position"][0] - tx, row["position"][2] - tz), i)
        for i, row in enumerate(rows)
        if row["model"].startswith("tree:") and i not in used
    ]
    distance, index = min(candidates)
    assert distance < 0.35, (tx, tz, distance)
    used.add(index)
    row = rows[index]
    row["model"] = "gltf:tree_small_02_optimized"
    z = row["position"][2]
    scale = round(max(0.79, min(0.98, 0.95 + 0.004 * (z + 14)
                                      + rng.uniform(-0.025, 0.025))), 5)
    row["scale"] = [scale] * 3
    row["rotation_y"] = round((row["rotation_y"] + rng.uniform(-0.38, 0.38))
                              % (2 * math.pi), 6)

# A flat scanned-soil rim is attached to both CC0 tree trunks. The original
# diffuse PBR bark/root surfaces stay intact; a 14 cm placement sink hides
# just that flat rim below the undulating terrain sheet.
tree_ids = {"gltf:tree_small_02_optimized", "gltf:island_tree_01_optimized"}
sunk = 0
for row in rows:
    if row["model"] in tree_ids:
        row["position"][1] = round(row["position"][1] - 0.14, 5)
        sunk += 1
assert sunk == 41, sunk  # 29 existing + 12 central replacements

path = [(9, 24), (6, 15), (1, 6), (-2, -4), (-1, -14), (-7, -24)]


def path_x(z):
    for (ax, az), (bx, bz) in zip(path, path[1:]):
        if bz <= z <= az:
            return ax + (bx - ax) * (z - az) / (bz - az)
    raise ValueError(z)


def path_distance(x, z):
    best = float("inf")
    for (ax, az), (bx, bz) in zip(path, path[1:]):
        dx, dz = bx - ax, bz - az
        t = max(0.0, min(1.0, ((x - ax) * dx + (z - az) * dz)
                         / (dx * dx + dz * dz)))
        best = min(best, math.hypot(x - ax - t * dx, z - az - t * dz))
    return best


def slice_height(x, z):
    return (0.06 * math.sin(0.22 * x) * math.cos(0.15 * z)
            + 0.03 * math.sin(0.63 * x + 0.40 * z))


def scatter_near(cx, cz, min_verge):
    for _ in range(40):
        x = cx + rng.gauss(0, 0.75)
        z = cz + rng.gauss(0, 0.62)
        if 0 < z < 21 and min_verge < path_distance(x, z) < 5.5:
            return round(x, 5), round(z, 5)
    raise ValueError((cx, cz))


# Lift a selected front strip of source-scan tufts to knee-level and bring
# modest existing flowers forward. The center of the 2.6 m dirt path stays
# clear; the stronger greenery grows in staggered patches at its verge.
front_grass = 0
front_heliophila = 0
front_hero_flowers = 0
for row in rows:
    x, _, z = row["position"]
    d = path_distance(x, z)
    if 0 < z < 20 and 1.8 < d < 5.0:
        if row["model"] in {"gltf:cc0_grass_patch_a", "gltf:cc0_grass_patch_b"}:
            row["scale"][0] = round(max(row["scale"][0], rng.uniform(1.18, 1.32)), 5)
            row["scale"][1] = round(max(row["scale"][1], rng.uniform(1.20, 1.40)), 5)
            row["scale"][2] = round(max(row["scale"][2], rng.uniform(1.18, 1.32)), 5)
            front_grass += 1
        elif row["model"] == "gltf:cc0_heliophila_a":
            scale = round(max(row["scale"][0], rng.uniform(1.16, 1.33)), 5)
            row["scale"] = [scale] * 3
            front_heliophila += 1
        elif row["model"].startswith("slice:hero_flower_"):
            scale = round(max(row["scale"][0], rng.uniform(0.90, 1.08)), 5)
            row["scale"] = [scale] * 3
            front_hero_flowers += 1

cluster_z = [18.3, 16.0, 13.2, 10.3, 7.5, 4.5]
centers = []
for z in cluster_z:
    for side in (-1, 1):
        centers.append((path_x(z) + side * rng.uniform(2.8, 3.65), z))

new_flowers = 0
new_grass = 0
for cluster_index, (cx, cz) in enumerate(centers):
    # Five white, two yellow and one purple head per cluster. The scanned
    # dandelion remains low filler; these authored multicolour patches provide
    # the visible reference accents without large paper-white blobs.
    flowers = (["slice:hero_flower_white"] * 5
               + ["slice:hero_flower_yellow"] * 2
               + ["slice:hero_flower_purple"])
    rng.shuffle(flowers)
    for model in flowers:
        x, z = scatter_near(cx, cz, 2.05)
        scale = round(rng.uniform(0.90, 1.10), 5)
        rows.append({"model": model,
                     "position": [x, round(slice_height(x, z), 5), z],
                     "rotation_y": round(rng.uniform(0, 2 * math.pi), 6),
                     "scale": [scale] * 3})
        new_flowers += 1
    for j in range(9):
        x, z = scatter_near(cx, cz, 1.62)
        horizontal = round(rng.uniform(1.16, 1.34), 5)
        vertical = round(rng.uniform(1.20, 1.40), 5)
        rows.append({"model": ("gltf:cc0_grass_patch_a" if (j + cluster_index) % 2
                               else "gltf:cc0_grass_patch_b"),
                     "position": [x, round(slice_height(x, z), 5), z],
                     "rotation_y": round(rng.uniform(0, 2 * math.pi), 6),
                     "scale": [horizontal, vertical, horizontal]})
        new_grass += 1

assert new_flowers == 96 and new_grass == 108
assert all(path_distance(r["position"][0], r["position"][2]) > 1.6
           for r in rows[-(new_flowers + new_grass):])
OUTPUT.write_text(json.dumps(layout, indent=2), encoding="utf-8")
print(json.dumps({
    "layout_sha256": hashlib.sha256(OUTPUT.read_bytes()).hexdigest(),
    "instances": len(rows), "central_legacy_replaced": len(used),
    "cc0_tree_bases_sunk": sunk, "front_grass_enlarged": front_grass,
    "front_heliophila_enlarged": front_heliophila,
    "front_hero_flowers_enlarged": front_hero_flowers,
    "new_flowers": new_flowers, "new_grass": new_grass,
    "flower_models": dict(sorted(Counter(r["model"] for r in rows
                                        if "flower" in r["model"]).items())),
}, sort_keys=True))
