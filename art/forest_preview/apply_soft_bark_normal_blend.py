"""Pack baked 60% trunk normal into both editable tree Blender sources."""
from pathlib import Path
import bpy
import hashlib
import json

ROOT = Path(__file__).resolve().parents[2]
ART = ROOT / "art/forest_preview"
NORMAL = ART / "cc0_prepared/tree_small_02_trunk_nor_soft_1k.png"
REPORT = ART / "matte_bark_blends.json"
file = Path(bpy.data.filepath).resolve()
assert file.parent == ART.resolve() and file.stem in {
    "tree_small_02_optimized", "tree_small_02_backdrop"}
mat = bpy.data.materials["tree_small_02_trunk"]
nodes = mat.node_tree.nodes
normal_image_node = nodes["Image Texture.002"]
assert normal_image_node.type == "TEX_IMAGE"
normal_map = next(n for n in nodes if n.type == "NORMAL_MAP")
assert abs(normal_map.inputs["Strength"].default_value - .6) < 1e-5
before_meshes = {o.name: len(o.data.polygons) for o in bpy.context.scene.objects
                 if o.type == "MESH"}
image = bpy.data.images.load(str(NORMAL), check_existing=True)
image.colorspace_settings.name = "Non-Color"
image.pack()
normal_image_node.image = image
normal_map.inputs["Strength"].default_value = 1.0
bpy.ops.file.make_paths_relative()
bpy.context.preferences.filepaths.save_version = 0
bpy.ops.wm.save_as_mainfile(filepath=str(file), compress=True)
assert before_meshes == {o.name: len(o.data.polygons) for o in bpy.context.scene.objects
                         if o.type == "MESH"}
report = json.loads(REPORT.read_text(encoding="utf-8"))
entry = next(row for row in report["models"] if row["model"] == file.stem)
entry.update({"after_sha256": hashlib.sha256(file.read_bytes()).hexdigest(),
              "normal_map_strength": 1.0,
              "baked_tangent_xy_scale": 0.6,
              "trunk_normal": str(NORMAL.relative_to(ROOT)).replace("\\", "/"),
              "trunk_normal_sha256": hashlib.sha256(NORMAL.read_bytes()).hexdigest(),
              "normal_packed": bool(image.packed_file)})
REPORT.write_text(json.dumps(report, indent=2), encoding="utf-8")
print("WARBELL_SOFT_BARK_BLEND", json.dumps(entry, sort_keys=True))
