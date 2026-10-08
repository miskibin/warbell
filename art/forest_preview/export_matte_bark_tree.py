"""Re-export only material-corrected small-tree GLBs via Blender MCP."""
from pathlib import Path
import bpy
import hashlib
import json

ROOT = Path(__file__).resolve().parents[2]
ART = ROOT / "art/forest_preview"
name = Path(bpy.data.filepath).stem
expected = {"tree_small_02_optimized": 164608,
            "tree_small_02_backdrop": 71283}
assert name in expected, name
objects = [o for o in bpy.context.scene.objects if o.type == "MESH"]
assert len(objects) == 3
assert sum(len(o.data.polygons) for o in objects) == expected[name]
mat = bpy.data.materials["tree_small_02_trunk"]
normal = next(n for n in mat.node_tree.nodes if n.type == "NORMAL_MAP")
assert abs(normal.inputs["Strength"].default_value - 1.0) < 1e-5
assert mat.node_tree.nodes["Image Texture.001"].image.packed_file
assert mat.node_tree.nodes["Image Texture.002"].image.packed_file
assert mat.node_tree.nodes["Image Texture.002"].image.name == "tree_small_02_trunk_nor_soft_1k.png"
output = ROOT / "assets/models/forest_slice/cc0" / (name + ".glb")
bpy.ops.object.select_all(action="DESELECT")
for obj in objects:
    obj.select_set(True)
bpy.context.view_layer.objects.active = objects[0]
bpy.ops.export_scene.gltf(filepath=str(output), export_format="GLB",
                          use_selection=True, export_yup=True,
                          export_apply=True, export_tangents=True,
                          export_normals=True, export_texcoords=True)
print("WARBELL_MATTE_BARK_EXPORT", json.dumps({
    "model": name, "triangles": expected[name],
    "bytes": output.stat().st_size,
    "sha256": hashlib.sha256(output.read_bytes()).hexdigest(),
}, sort_keys=True))
