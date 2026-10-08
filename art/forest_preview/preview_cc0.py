"""Fast textured Eevee silhouette QA of the raw Poly Haven source in Blender."""
from pathlib import Path
import bpy
from mathutils import Vector

ROOT=Path(__file__).resolve().parents[2]
SRC=ROOT/"target/world-preview/cc0-sources/tree_small_02/tree_small_02_1k.gltf"
OUT=ROOT/"art/forest_preview/cc0_tree_source_preview.png"
bpy.ops.object.select_all(action="SELECT")
bpy.ops.object.delete(use_global=False)
bpy.ops.import_scene.gltf(filepath=str(SRC))
tree=next(o for o in bpy.context.scene.objects if o.type=="MESH")
tree.select_set(False)

cam_data=bpy.data.cameras.new("Source QA camera")
cam=bpy.data.objects.new("Source QA camera",cam_data)
bpy.context.collection.objects.link(cam)
cam.location=(7,-9,5.1)
target=Vector((0,-.6,2.35))
cam.rotation_euler=(target-cam.location).to_track_quat("-Z","Y").to_euler()
cam_data.lens=43
bpy.context.scene.camera=cam
sun_data=bpy.data.lights.new("Source QA sun","SUN")
sun=bpy.data.objects.new("Source QA sun",sun_data)
bpy.context.collection.objects.link(sun)
sun.rotation_euler=(.42,-.55,-.3)
sun_data.energy=2.1
bpy.context.scene.world.color=(.60,.67,.74)
scene=bpy.context.scene
scene.render.resolution_x=960
scene.render.resolution_y=900
scene.render.resolution_percentage=100
scene.render.image_settings.file_format="PNG"
scene.render.filepath=str(OUT)
scene.render.film_transparent=False
bpy.ops.render.render(write_still=True)
print("WARBELL_CC0_SOURCE_PREVIEW",str(OUT),OUT.stat().st_size)
