"""Small Eevee visual QA of the optimized source-PBR tree; no asset mutation."""
from pathlib import Path
from mathutils import Vector
import bpy

ROOT=Path(__file__).resolve().parents[2]
OUT=ROOT/"art/forest_preview/cc0_tree_optimized_preview.png"
cam=bpy.context.scene.camera
cam.location=(11.5,-16.0,7.3)
cam.rotation_euler=(Vector((0,-0.6,4.1))-cam.location).to_track_quat("-Z","Y").to_euler()
cam.data.lens=44
world=bpy.context.scene.world
world.use_nodes=True
world.node_tree.nodes.get("Background").inputs["Color"].default_value=(0.56,0.64,0.75,1)
world.node_tree.nodes.get("Background").inputs["Strength"].default_value=1.2
sun=next(o for o in bpy.context.scene.objects if o.type=="LIGHT")
sun.data.energy=2.8
sun.rotation_euler=(0.45,-0.6,-0.35)
scene=bpy.context.scene
scene.render.engine="BLENDER_EEVEE_NEXT"
scene.render.resolution_x=1050
scene.render.resolution_y=900
scene.render.resolution_percentage=100
scene.render.image_settings.file_format="PNG"
scene.render.filepath=str(OUT)
scene.view_settings.view_transform="AgX"
bpy.ops.render.render(write_still=True)
print("WARBELL_CC0_OPTIMIZED_PREVIEW",str(OUT),OUT.stat().st_size)
