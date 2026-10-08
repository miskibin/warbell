"""Replace only the forest approval slice's remaining pastel study trees.

Camera, path, ground and every non-tree placement stay byte-equivalent at the
JSON value level. Reruns derive from the tracked v6 snapshot, never from the
previous output, so scales and base offsets cannot accumulate.
"""
from pathlib import Path
from collections import Counter
import hashlib
import json

ROOT = Path(__file__).resolve().parents[2]
ART = ROOT / "art/forest_preview"
SOURCE = ART / "layout_v6_before_backdrop.json"
OUTPUT = ROOT / "assets/models/forest_slice/layout.json"
TREE_ROOT = ROOT / "assets/models/blender_trees"
layout = json.loads(SOURCE.read_text(encoding="utf-8"))
rows = layout["instances"]
old_models = sorted({row["model"] for row in rows if row["model"].startswith("tree:")})
expected = {"tree:oak_a", "tree:oak_b", "tree:birch_a", "tree:birch_b", "tree:pine_a"}
assert set(old_models) == expected, old_models
old_dims = {}
for model in old_models:
    doc = json.loads((TREE_ROOT / (model[5:] + ".json")).read_text(encoding="utf-8"))
    bounds = doc["bounds"]
    old_dims[model] = max(bounds["max"][0] - bounds["min"][0],
                          bounds["max"][2] - bounds["min"][2])

original_camera = json.loads(json.dumps(layout["camera"]))
original_path = json.loads(json.dumps(layout["path_centerline"]))
counts = Counter()
scales = []
for row in rows:
    model = row["model"]
    if not model.startswith("tree:"):
        continue
    # The 8 m scanned source has a broad, asymmetric crown. Match the old
    # variant's horizontal visual width, with caps for varied tree ages.
    old_scale = row["scale"][0]
    s = round(max(0.52, min(1.03, old_scale * old_dims[model] / 5.8)), 5)
    row["model"] = "gltf:tree_small_02_backdrop"
    row["scale"] = [s, s, s]
    row["position"][1] = round(row["position"][1] - 0.14, 5)
    counts[model] += 1
    scales.append(s)

assert sum(counts.values()) == 88, counts
assert layout["camera"] == original_camera
assert layout["path_centerline"] == original_path
OUTPUT.write_text(json.dumps(layout, indent=2), encoding="utf-8")
print(json.dumps({
    "layout_sha256": hashlib.sha256(OUTPUT.read_bytes()).hexdigest(),
    "replaced": dict(sorted(counts.items())),
    "backdrop_scale_min": min(scales),
    "backdrop_scale_max": max(scales),
    "instances": len(rows),
    "camera": layout["camera"],
}, sort_keys=True))
