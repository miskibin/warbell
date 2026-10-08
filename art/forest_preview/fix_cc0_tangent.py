"""Repair one near-collinear trunk UV triangle, then re-export in Blender MCP."""
from pathlib import Path
import bpy

ROOT=Path(__file__).resolve().parents[2]
OUT=ROOT/"assets/models/forest_slice/cc0/tree_small_02_optimized.glb"
BLEND=ROOT/"art/forest_preview/tree_small_02_optimized.blend"
trunk=next(o for o in bpy.context.scene.objects if o.type=="MESH" and
           o.material_slots and o.material_slots[0].material.name=="tree_small_02_trunk")
mesh=trunk.data
target=(0.3490058481693268,-1.92645263671875,4.943565368652344)
uvtarget=(0.543842077255249,1.0-0.4340325593948364)
matches=[]
for polygon in mesh.polygons:
    for loop_index in polygon.loop_indices:
        v=mesh.vertices[mesh.loops[loop_index].vertex_index].co
        uv=mesh.uv_layers.active.data[loop_index].uv
        if (sum((v[i]-target[i])**2 for i in range(3))<1e-8 and
                sum((uv[i]-uvtarget[i])**2 for i in range(2))<1e-8):
            matches.append((polygon.index,loop_index,list(v),list(uv)))
if not matches:raise ValueError("Degenerate tangent corner not found")
def uv_area(poly_index):
    p=mesh.polygons[poly_index]
    tex=[mesh.uv_layers.active.data[i].uv for i in p.loop_indices]
    return abs((tex[1].x-tex[0].x)*(tex[2].y-tex[0].y)-
               (tex[1].y-tex[0].y)*(tex[2].x-tex[0].x))
polygon_index,loop_index,_,_=min(matches,key=lambda row:uv_area(row[0]))
if uv_area(polygon_index)>1e-6:
    raise ValueError(f"Matched UV face is not degenerate: {uv_area(polygon_index)}")
mesh.uv_layers.active.data[loop_index].uv.x+=0.003
print("TANGENT_UV_REPAIRED",polygon_index,loop_index,
      list(mesh.uv_layers.active.data[loop_index].uv))
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
print("WARBELL_CC0_TREE_TANGENT_FIXED",OUT.stat().st_size,BLEND.stat().st_size)
