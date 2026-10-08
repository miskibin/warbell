"""Derive a cheaper organic background tree from the packed CC0 small tree.

Load tree_small_02_optimized.blend, execute through Blender MCP, and save only
the editable LOD source here. Runtime GLB export is a separate frozen step.
"""
from pathlib import Path
import bpy
import hashlib
import json

ROOT = Path(__file__).resolve().parents[2]
ART = ROOT / "art/forest_preview"
SOURCE = ART / "tree_small_02_optimized.blend"
OUTPUT = ART / "tree_small_02_backdrop.blend"
assert Path(bpy.data.filepath).resolve() == SOURCE.resolve(), bpy.data.filepath

ratios = {
    "tree_small_02_leaves": 0.38,
    "tree_small_02_branches": 0.52,
    "tree_small_02_trunk": 0.62,
}
objects = {o.name: o for o in bpy.context.scene.objects if o.type == "MESH"
           and o.name in ratios}
assert set(objects) == set(ratios), sorted(objects)
before = {name: len(obj.data.polygons) for name, obj in objects.items()}

for name, ratio in ratios.items():
    obj = objects[name]
    bpy.ops.object.select_all(action="DESELECT")
    obj.select_set(True)
    bpy.context.view_layer.objects.active = obj
    modifier = obj.modifiers.new("distance LOD decimation", type="DECIMATE")
    modifier.ratio = ratio
    bpy.ops.object.modifier_apply(modifier=modifier.name)
    obj.data.update()

after = {name: len(obj.data.polygons) for name, obj in objects.items()}
total = sum(after.values())
assert 50000 <= total <= 78000, after
bpy.ops.file.pack_all()
bpy.ops.file.make_paths_relative()
bpy.context.preferences.filepaths.save_version = 0
bpy.ops.wm.save_as_mainfile(filepath=str(OUTPUT), compress=True)
report = {
    "schema": "warbell.forest_cc0_backdrop_lod_author.v1",
    "source_blend": str(SOURCE.relative_to(ROOT)).replace("\\", "/"),
    "source_blend_sha256": hashlib.sha256(SOURCE.read_bytes()).hexdigest(),
    "source_triangles_by_material": before,
    "lod_triangles_by_material": after,
    "lod_total_triangles": total,
    "ratios": ratios,
    "lod_blend": str(OUTPUT.relative_to(ROOT)).replace("\\", "/"),
    "lod_blend_bytes": OUTPUT.stat().st_size,
    "lod_blend_sha256": hashlib.sha256(OUTPUT.read_bytes()).hexdigest(),
}
(ART / "cc0_backdrop_author.json").write_text(json.dumps(report, indent=2),
                                                encoding="utf-8")
print("WARBELL_CC0_BACKDROP_LOD_AUTHOR", json.dumps(report, sort_keys=True))
