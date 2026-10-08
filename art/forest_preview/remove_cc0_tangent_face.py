"""Delete one degenerate decimated trunk sliver causing a zero tangent."""
from pathlib import Path
import bpy,bmesh

ROOT=Path(__file__).resolve().parents[2]
OUT=ROOT/"assets/models/forest_slice/cc0/tree_small_02_optimized.glb"
BLEND=ROOT/"art/forest_preview/tree_small_02_optimized.blend"
trunk=next(o for o in bpy.context.scene.objects if o.type=="MESH" and
           o.material_slots and o.material_slots[0].material.name=="tree_small_02_trunk")
mesh=trunk.data
index=17006
face=mesh.polygons[index]
points=[tuple(mesh.vertices[v].co) for v in face.vertices]
target=(0.3490058481693268,-1.92645263671875,4.943565368652344)
if not any(sum((p[i]-target[i])**2 for i in range(3))<1e-8 for p in points):
    raise ValueError(f"Unexpected sliver face at index {index}: {points}")
bm=bmesh.new();bm.from_mesh(mesh);bm.faces.ensure_lookup_table()
bmesh.ops.delete(bm,geom=[bm.faces[index]],context="FACES_ONLY")
bm.to_mesh(mesh);bm.free();mesh.update()
print("DEGENERATE_TRUNK_FACE_REMOVED",index,len(mesh.polygons))
bpy.ops.object.select_all(action="DESELECT")
for obj in bpy.context.scene.objects:
    if obj.type=="MESH" and obj.material_slots and obj.material_slots[0].material.name.startswith("tree_small_02_"):
        obj.select_set(True)
bpy.context.view_layer.objects.active=trunk
bpy.ops.export_scene.gltf(filepath=str(OUT),export_format="GLB",use_selection=True,
                          export_yup=True,export_apply=True,export_tangents=True,
                          export_normals=True,export_texcoords=True)
bpy.context.preferences.filepaths.save_version=0
bpy.ops.wm.save_as_mainfile(filepath=str(BLEND))
print("WARBELL_CC0_TREE_TANGENT_FACE_FIXED",OUT.stat().st_size,BLEND.stat().st_size)
