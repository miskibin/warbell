"""Export the packed distance tree source through official Blender MCP."""
from pathlib import Path
import bpy
import hashlib
import json

ROOT = Path(__file__).resolve().parents[2]
ART = ROOT / "art/forest_preview"
SOURCE = ART / "tree_small_02_backdrop.blend"
OUTPUT = ROOT / "assets/models/forest_slice/cc0/tree_small_02_backdrop.glb"
assert Path(bpy.data.filepath).resolve() == SOURCE.resolve(), bpy.data.filepath
objects = [o for o in bpy.context.scene.objects if o.type == "MESH"]
assert len(objects) == 3, [o.name for o in objects]
assert sum(len(o.data.polygons) for o in objects) == 71283
assert all(o.material_slots and o.material_slots[0].material for o in objects)
bpy.ops.object.select_all(action="DESELECT")
for obj in objects:
    obj.select_set(True)
bpy.context.view_layer.objects.active = objects[0]
bpy.ops.export_scene.gltf(filepath=str(OUTPUT), export_format="GLB",
                          use_selection=True, export_yup=True,
                          export_apply=True, export_tangents=True,
                          export_normals=True, export_texcoords=True)
print("WARBELL_CC0_BACKDROP_LOD_EXPORT", json.dumps({
    "file": str(OUTPUT.relative_to(ROOT)).replace("\\", "/"),
    "triangles": sum(len(o.data.polygons) for o in objects),
    "bytes": OUTPUT.stat().st_size,
    "sha256": hashlib.sha256(OUTPUT.read_bytes()).hexdigest(),
}, sort_keys=True))
