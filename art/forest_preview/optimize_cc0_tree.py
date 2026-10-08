"""Optimize the imported CC0 Poly Haven tree in Blender and export its PBR GLB.

Run through tools/blender_environment/mcp_client.py after preview_cc0.py and
separate_cc0.py. Original glTF dependencies and the separate leaf alpha are
downloaded/verified by download_cc0.py; prepare_cc0_leaf_alpha.py merges that
alpha with the unaltered 1K diffuse RGB.
"""
from pathlib import Path
from mathutils import Vector
import bpy
import bmesh
import json

ROOT=Path(__file__).resolve().parents[2]
OUT=ROOT/"assets/models/forest_slice/cc0/tree_small_02_optimized.glb"
BLEND=ROOT/"art/forest_preview/tree_small_02_optimized.blend"
LEAF_RGBA=ROOT/"art/forest_preview/cc0_prepared/tree_small_02_leaves_rgba_1k.png"
OUT.parent.mkdir(parents=True,exist_ok=True)

objects={s.material.name:o for o in bpy.context.scene.objects if o.type=="MESH"
         for s in o.material_slots if s.material and s.material.name.startswith("tree_small_02_")}
assert set(objects)=={"tree_small_02_leaves","tree_small_02_branches","tree_small_02_trunk"},list(objects)

# Blender's imported 1K glTF references an opaque JPEG leaf diffuse despite
# ALPHA_MODE=BLEND. Replace it with the same RGB plus the official source mask.
leaf_mat=bpy.data.materials["tree_small_02_leaves"]
nodes=leaf_mat.node_tree.nodes
links=leaf_mat.node_tree.links
base=next(n for n in nodes if n.type=="TEX_IMAGE" and "leaves_diff" in n.image.name)
base.image=bpy.data.images.load(str(LEAF_RGBA),check_existing=True)
base.image.pack()
shader=next(n for n in nodes if n.type=="BSDF_PRINCIPLED")
for link in list(shader.inputs["Alpha"].links):links.remove(link)
clip=nodes.new("ShaderNodeMath")
clip.name="Official leaf alpha clip 0.45"
clip.operation="GREATER_THAN"
clip.inputs[1].default_value=0.45
clip.location=(shader.location.x-230,shader.location.y-420)
links.new(base.outputs["Alpha"],clip.inputs[0])
links.new(clip.outputs[0],shader.inputs["Alpha"])
leaf_mat.surface_render_method="DITHERED"
leaf_mat.alpha_threshold=0.45
leaf_mat.use_backface_culling=False

# On the source mesh, foliage contributes ~1.94M of 2.06M triangles. Collapse
# fine triangles while retaining all sprig islands and their silhouette; keep
# separate source bark/branch materials and all source UVs/maps.
ratios={"tree_small_02_leaves":0.06,
        "tree_small_02_branches":0.30,
        "tree_small_02_trunk":0.70}
before={name:len(o.data.polygons) for name,o in objects.items()}
for name in ("tree_small_02_leaves","tree_small_02_branches","tree_small_02_trunk"):
    obj=objects[name]
    bpy.ops.object.select_all(action="DESELECT")
    obj.select_set(True)
    bpy.context.view_layer.objects.active=obj
    mod=obj.modifiers.new("preserve PBR silhouette ratio",type="DECIMATE")
    mod.ratio=ratios[name]
    bpy.ops.object.modifier_apply(modifier=mod.name)
    print("DECIMATED",name,before[name],len(obj.data.polygons),flush=True)

# Unparent while keeping world transforms, then make the tree intrinsically
# 8.0m tall with local Y=0 at its trunk base in exported glTF coordinates.
for obj in objects.values():
    bpy.ops.object.select_all(action="DESELECT")
    obj.select_set(True)
    bpy.context.view_layer.objects.active=obj
    if obj.parent:bpy.ops.object.parent_clear(type="CLEAR_KEEP_TRANSFORM")
    bpy.ops.object.transform_apply(location=True,rotation=True,scale=True)
world_bounds=[obj.matrix_world@Vector(corner) for obj in objects.values() for corner in obj.bound_box]
zmin=min(v.z for v in world_bounds)
zmax=max(v.z for v in world_bounds)
height=8.0
factor=height/(zmax-zmin)
for obj in objects.values():
    obj.scale=(factor,)*3
    obj.location.z=-zmin*factor
    bpy.ops.object.select_all(action="DESELECT")
    obj.select_set(True)
    bpy.context.view_layer.objects.active=obj
    bpy.ops.object.transform_apply(location=True,rotation=True,scale=True)
    obj.name=obj.material_slots[0].material.name

# The collapse modifier leaves one sliver at this deterministic trunk face.
# Its zero tangent fails normal mapping despite valid UVs; remove only that
# triangle (1/164,609), preserving all surrounding bark geometry.
trunk=objects["tree_small_02_trunk"]
sliver=17006
corners=[tuple(trunk.data.vertices[v].co) for v in trunk.data.polygons[sliver].vertices]
target=(0.3490058481693268,-1.92645263671875,4.943565368652344)
if not any(sum((p[i]-target[i])**2 for i in range(3))<1e-8 for p in corners):
    raise ValueError(f"Expected trunk sliver changed: {corners}")
bm=bmesh.new();bm.from_mesh(trunk.data);bm.faces.ensure_lookup_table()
bmesh.ops.delete(bm,geom=[bm.faces[sliver]],context="FACES_ONLY")
bm.to_mesh(trunk.data);bm.free();trunk.data.update()

for obj in list(bpy.context.scene.objects):
    if obj.type=="MESH" and obj not in objects.values():
        bpy.data.objects.remove(obj,do_unlink=True)

bpy.ops.object.select_all(action="DESELECT")
for obj in objects.values():obj.select_set(True)
bpy.context.view_layer.objects.active=objects["tree_small_02_trunk"]
bpy.ops.export_scene.gltf(filepath=str(OUT),export_format="GLB",use_selection=True,
                          export_yup=True,export_apply=True,export_tangents=True,
                          export_normals=True,export_texcoords=True)

# Save only optimized geometry in the editable Blender source, not the 2M-tri
# import. Original source remains separately downloadable and hash-verified.
bpy.ops.wm.save_as_mainfile(filepath=str(BLEND))
bounds=[obj.matrix_world@Vector(corner) for obj in objects.values() for corner in obj.bound_box]
report={"id":"tree_small_02_optimized","original_triangles":sum(before.values()),
        "optimized_triangles":sum(len(o.data.polygons) for o in objects.values()),
        "per_material_triangles":{name:len(o.data.polygons) for name,o in objects.items()},
        "bounds_blender_min":[min(v[i] for v in bounds) for i in range(3)],
        "bounds_blender_max":[max(v[i] for v in bounds) for i in range(3)],
        "glb_bytes":OUT.stat().st_size,"blend_bytes":BLEND.stat().st_size,
        "leaf_alpha_clip":0.45}
print("WARBELL_CC0_TREE_EXPORT",json.dumps(report),flush=True)
