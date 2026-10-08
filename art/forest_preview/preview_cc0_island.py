"""Inspect the fuller CC0 island tree with its official alpha, in Blender MCP."""
from pathlib import Path
from mathutils import Vector
import bpy,json

ROOT=Path(__file__).resolve().parents[2]
SRC=ROOT/"target/world-preview/cc0-sources/island_tree_01/island_tree_01_1k.gltf"
ALPHA=ROOT/"art/forest_preview/cc0_prepared/island_tree_01_leaves_rgba_1k.png"
OUT=ROOT/"art/forest_preview/cc0_island_tree_source_preview.png"
bpy.ops.object.select_all(action="SELECT")
bpy.ops.object.delete(use_global=False)
bpy.ops.import_scene.gltf(filepath=str(SRC))
leaf_mat=next(m for m in bpy.data.materials if m.name=="island_tree_01_leaves")
nodes=leaf_mat.node_tree.nodes;links=leaf_mat.node_tree.links
base=next(n for n in nodes if n.type=="TEX_IMAGE" and "leaves_diff" in n.image.name)
base.image=bpy.data.images.load(str(ALPHA),check_existing=True)
base.image.pack()
shader=next(n for n in nodes if n.type=="BSDF_PRINCIPLED")
for link in list(shader.inputs["Alpha"].links):links.remove(link)
clip=nodes.new("ShaderNodeMath");clip.operation="GREATER_THAN";clip.inputs[1].default_value=.45
links.new(base.outputs["Alpha"],clip.inputs[0]);links.new(clip.outputs[0],shader.inputs["Alpha"])
leaf_mat.surface_render_method="DITHERED";leaf_mat.alpha_threshold=.45
leaf_mat.use_backface_culling=False
meshes=[o for o in bpy.context.scene.objects if o.type=="MESH"]
pts=[o.matrix_world@Vector(c) for o in meshes for c in o.bound_box]
print("ISLAND_SOURCE",json.dumps({"objects":[(o.name,len(o.data.vertices),len(o.data.polygons),
  [s.material.name for s in o.material_slots]) for o in meshes],
  "min":[min(v[i] for v in pts) for i in range(3)],
  "max":[max(v[i] for v in pts) for i in range(3)]}),flush=True)
cam_data=bpy.data.cameras.new("Island source QA camera")
cam=bpy.data.objects.new("Island source QA camera",cam_data)
bpy.context.collection.objects.link(cam)
cam.location=(9.5,-12.5,6.6)
cam.rotation_euler=(Vector((0,-.4,2.65))-cam.location).to_track_quat("-Z","Y").to_euler()
cam_data.lens=43
bpy.context.scene.camera=cam
sun_data=bpy.data.lights.new("Island QA sun","SUN")
sun=bpy.data.objects.new("Island QA sun",sun_data)
bpy.context.collection.objects.link(sun)
sun.rotation_euler=(.42,-.55,-.3);sun_data.energy=2.4
world=bpy.context.scene.world;world.use_nodes=True
world.node_tree.nodes.get("Background").inputs["Color"].default_value=(.55,.65,.75,1)
world.node_tree.nodes.get("Background").inputs["Strength"].default_value=1.1
scene=bpy.context.scene;scene.render.engine="BLENDER_EEVEE_NEXT"
scene.render.resolution_x=1050;scene.render.resolution_y=900
scene.render.resolution_percentage=100
scene.render.image_settings.file_format="PNG";scene.render.filepath=str(OUT)
bpy.ops.render.render(write_still=True)
print("WARBELL_ISLAND_SOURCE_PREVIEW",str(OUT),OUT.stat().st_size)
