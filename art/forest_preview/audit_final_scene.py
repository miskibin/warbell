"""Verify the saved Blender source matches every forest-slice runtime row.

Execute through the private Blender MCP bridge after opening forest_scene.blend.
The check inspects actual in-memory objects, then hashes the saved .blend and
records the layout/source relationship without touching the runtime assets.
"""
from pathlib import Path
import bpy
import hashlib
import json

ROOT = Path(__file__).resolve().parents[2]
ART = ROOT / "art/forest_preview"
LAYOUT = ROOT / "assets/models/forest_slice/layout.json"
SCENE = ART / "forest_scene.blend"
rows = json.loads(LAYOUT.read_text(encoding="utf-8"))["instances"]
by_prefix = {}
for obj in bpy.data.objects:
    if len(obj.name) > 5 and obj.name[:4].isdigit() and obj.name[4] == " ":
        by_prefix.setdefault(int(obj.name[:4]), []).append(obj)

issues = []
for index, row in enumerate(rows):
    objs = by_prefix.get(index, [])
    if not objs:
        issues.append(f"missing index {index}: {row['model']}")
        continue
    x, y, z = row["position"]
    sx, sy, sz = row["scale"]
    expected_loc = (x, -z, y)  # source .blend is Z-up
    expected_scale = (sx, sz, sy)
    for obj in objs:
        if max(abs(obj.location[n] - expected_loc[n]) for n in range(3)) > 1e-4:
            issues.append(f"location {index}: {obj.name}")
        if max(abs(obj.scale[n] - expected_scale[n]) for n in range(3)) > 1e-4:
            issues.append(f"scale {index}: {obj.name}")
        if abs(obj.rotation_euler.z - row["rotation_y"]) > 1e-4:
            issues.append(f"rotation {index}: {obj.name}")
        if row["model"].startswith("gltf:") and row["model"] not in obj.name:
            issues.append(f"model label {index}: {obj.name}")

extra = sorted(set(by_prefix) - set(range(len(rows))))
if extra:
    issues.append(f"extra indexed objects {extra[:12]}")
tree_names = {"tree_small_02_optimized", "island_tree_01_optimized",
              "tree_small_02_backdrop"}
linked = sorted({mesh.library.filepath for mesh in bpy.data.meshes
                 if mesh.library and any(name in mesh.library.filepath
                                         for name in tree_names)})
if len(linked) != 3 or any(not p.startswith("//") for p in linked):
    issues.append(f"tree library paths {linked}")
report = {
    "schema": "warbell.forest_slice_blender_source_audit.v1",
    "pass": not issues,
    "instances": len(rows),
    "indexed_scene_objects": sum(map(len, by_prefix.values())),
    "linked_tree_libraries": linked,
    "layout_sha256": hashlib.sha256(LAYOUT.read_bytes()).hexdigest(),
    "scene_sha256": hashlib.sha256(SCENE.read_bytes()).hexdigest(),
    "scene_bytes": SCENE.stat().st_size,
    "issues": issues,
}
(ART / "final_scene_audit.json").write_text(json.dumps(report, indent=2),
                                             encoding="utf-8")
print("WARBELL_FINAL_SCENE_AUDIT", json.dumps(report, sort_keys=True))
if issues:
    raise ValueError(issues[:12])
