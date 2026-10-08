"""Optimize the fuller CC0 island oak in Blender MCP, retaining all 1K PBR maps."""
from pathlib import Path
from mathutils import Vector
import bpy,json

ROOT=Path(__file__).resolve().parents[2]
SRC=ROOT/"target/world-preview/cc0-sources/island_tree_01/island_tree_01_1k.gltf"
ALPHA=ROOT/"art/forest_preview/cc0_prepared/island_tree_01_leaves_rgba_1k.png"
OUT=ROOT/"assets/models/forest_slice/cc0/island_tree_01_optimized.glb"
BLEND=ROOT/"art/forest_preview/island_tree_01_optimized.blend"
OUT.parent.mkdir(parents=True,exist_ok=True)

source=[o for o in bpy.context.scene.objects if o.type=="MESH" and
        any(s.material and s.material.name=="island_tree_01_leaves" for s in o.material_slots)]
if len(source)!=1:
    bpy.ops.object.select_all(action="SELECT")
    bpy.ops.object.delete(use_global=False)
    bpy.data.orphans_purge(do_recursive=True)
    bpy.ops.import_scene.gltf(filepath=str(SRC))
    source=[o for o in bpy.context.scene.objects if o.type=="MESH" and
            any(s.material and s.material.name=="island_tree_01_leaves" for s in o.material_slots)]
assert len(source)==1
tree=source[0]

leaf_mat=bpy.data.materials["island_tree_01_leaves"]
nodes=leaf_mat.node_tree.nodes;links=leaf_mat.node_tree.links
shader=next(n for n in nodes if n.type=="BSDF_PRINCIPLED")
base=next(l.from_node for l in shader.inputs["Base Color"].links if l.from_node.type=="TEX_IMAGE")
base.image=bpy.data.images.load(str(ALPHA),check_existing=True);base.image.pack()
for link in list(shader.inputs["Alpha"].links):links.remove(link)
clip=nodes.new("ShaderNodeMath");clip.name="Official leaf alpha clip 0.45"
clip.operation="GREATER_THAN";clip.inputs[1].default_value=.45
links.new(base.outputs["Alpha"],clip.inputs[0]);links.new(clip.outputs[0],shader.inputs["Alpha"])
leaf_mat.surface_render_method="DITHERED";leaf_mat.alpha_threshold=.45
leaf_mat.use_backface_culling=False

bpy.ops.object.select_all(action="DESELECT")
tree.select_set(True);bpy.context.view_layer.objects.active=tree
bpy.ops.object.mode_set(mode="EDIT")
bpy.ops.mesh.select_all(action="SELECT")
bpy.ops.mesh.separate(type="MATERIAL")
bpy.ops.object.mode_set(mode="OBJECT")
objects={s.material.name:o for o in bpy.context.scene.objects if o.type=="MESH"
         for s in o.material_slots if s.material and s.material.name.startswith("island_tree_01")}
assert set(objects)=={"island_tree_01","island_tree_01_leaves","island_tree_01_branches"},objects
ratios={"island_tree_01":.85,"island_tree_01_leaves":.22,"island_tree_01_branches":.35}
before={name:len(o.data.polygons) for name,o in objects.items()}
for name in ("island_tree_01_leaves","island_tree_01_branches","island_tree_01"):
    obj=objects[name]
    bpy.ops.object.select_all(action="DESELECT")
    obj.select_set(True);bpy.context.view_layer.objects.active=obj
    mod=obj.modifiers.new("natural source decimation",type="DECIMATE")
    mod.ratio=ratios[name]
    bpy.ops.object.modifier_apply(modifier=mod.name)
    print("ISLAND_DECIMATED",name,before[name],len(obj.data.polygons),flush=True)

for obj in objects.values():
    bpy.ops.object.select_all(action="DESELECT")
    obj.select_set(True);bpy.context.view_layer.objects.active=obj
    if obj.parent:bpy.ops.object.parent_clear(type="CLEAR_KEEP_TRANSFORM")
    bpy.ops.object.transform_apply(location=True,rotation=True,scale=True)
pts=[obj.matrix_world@Vector(c) for obj in objects.values() for c in obj.bound_box]
zmin=min(v.z for v in pts);zmax=max(v.z for v in pts)
factor=8.7/(zmax-zmin)
for obj in objects.values():
    obj.scale=(factor,)*3;obj.location.z=-zmin*factor
    bpy.ops.object.select_all(action="DESELECT")
    obj.select_set(True);bpy.context.view_layer.objects.active=obj
    bpy.ops.object.transform_apply(location=True,rotation=True,scale=True)
    obj.name=obj.material_slots[0].material.name

bpy.ops.object.select_all(action="DESELECT")
for obj in objects.values():obj.select_set(True)
bpy.context.view_layer.objects.active=objects["island_tree_01"]
bpy.ops.export_scene.gltf(filepath=str(OUT),export_format="GLB",use_selection=True,
                          export_yup=True,export_apply=True,export_tangents=True,
                          export_normals=True,export_texcoords=True)
bpy.context.preferences.filepaths.save_version=0
bpy.ops.wm.save_as_mainfile(filepath=str(BLEND))
pts=[obj.matrix_world@Vector(c) for obj in objects.values() for c in obj.bound_box]
report={"id":"island_tree_01_optimized","original_triangles":sum(before.values()),
        "optimized_triangles":sum(len(o.data.polygons) for o in objects.values()),
        "per_material_triangles":{name:len(o.data.polygons) for name,o in objects.items()},
        "bounds_blender_min":[min(v[i] for v in pts) for i in range(3)],
        "bounds_blender_max":[max(v[i] for v in pts) for i in range(3)],
        "glb_bytes":OUT.stat().st_size,"blend_bytes":BLEND.stat().st_size,
        "leaf_alpha_clip":.45}
print("WARBELL_CC0_ISLAND_EXPORT",json.dumps(report),flush=True)
