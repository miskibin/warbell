"""Author matte small-tree trunk PBR in both editable Blender libraries.

Load either tree_small_02_optimized.blend or tree_small_02_backdrop.blend and
execute through the private official Blender MCP bridge. Only the trunk ARM
image binding and its normal-map strength are edited; meshes remain untouched.
"""
from pathlib import Path
import bpy
import hashlib
import json

ROOT = Path(__file__).resolve().parents[2]
ART = ROOT / "art/forest_preview"
ARM = ART / "cc0_prepared/tree_small_02_trunk_arm_matte_1k.png"
REPORT = ART / "matte_bark_blends.json"
file = Path(bpy.data.filepath).resolve()
assert file.parent == ART.resolve() and file.stem in {
    "tree_small_02_optimized", "tree_small_02_backdrop"}, file
before_sha = hashlib.sha256(file.read_bytes()).hexdigest()

mat = bpy.data.materials["tree_small_02_trunk"]
assert mat.use_nodes
nodes = mat.node_tree.nodes
rough = nodes["Image Texture.001"]
assert rough.type == "TEX_IMAGE" and "rough" in rough.image.name
assert rough.image.colorspace_settings.name == "Non-Color"
normal = next(n for n in nodes if n.type == "NORMAL_MAP")
assert not normal.inputs["Strength"].is_linked
assert abs(normal.inputs["Strength"].default_value - 1.0) < 1e-6
before_meshes = {o.name: len(o.data.polygons) for o in bpy.context.scene.objects
                 if o.type == "MESH"}
assert len(before_meshes) == 3

matte = bpy.data.images.load(str(ARM), check_existing=True)
matte.colorspace_settings.name = "Non-Color"
matte.pack()
rough.image = matte
normal.inputs["Strength"].default_value = 0.6
assert rough.image.packed_file is not None
bpy.ops.file.make_paths_relative()
bpy.context.preferences.filepaths.save_version = 0
bpy.ops.wm.save_as_mainfile(filepath=str(file), compress=True)
after_meshes = {o.name: len(o.data.polygons) for o in bpy.context.scene.objects
                if o.type == "MESH"}
assert after_meshes == before_meshes

rows = json.loads(REPORT.read_text(encoding="utf-8")) if REPORT.exists() else {
    "schema": "warbell.forest_matte_bark_blends.v1", "models": []}
entry = {"model": file.stem,
         "blend": str(file.relative_to(ROOT)).replace("\\", "/"),
         "before_sha256": before_sha,
         "after_sha256": hashlib.sha256(file.read_bytes()).hexdigest(),
         "triangles_by_mesh": after_meshes,
         "trunk_arm": str(ARM.relative_to(ROOT)).replace("\\", "/"),
         "trunk_arm_sha256": hashlib.sha256(ARM.read_bytes()).hexdigest(),
         "normal_map_strength": normal.inputs["Strength"].default_value,
         "packed": bool(rough.image.packed_file)}
rows["models"] = sorted([r for r in rows["models"] if r["model"] != file.stem]
                        + [entry], key=lambda row: row["model"])
REPORT.write_text(json.dumps(rows, indent=2), encoding="utf-8")
print("WARBELL_MATTE_BARK_BLEND", json.dumps(entry, sort_keys=True))
