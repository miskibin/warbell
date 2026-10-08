"""Curated Blender Eevee visual QA of the exported campaign kit."""
from pathlib import Path
import bpy
import math
from mathutils import Vector

ROOT=Path(__file__).resolve().parents[2]
ART=ROOT/"art/blender_environment"
bpy.ops.wm.open_mainfile(filepath=str(ART/"warbell_environment.blend"))
scene=bpy.context.scene
objects={o.name:o for o in bpy.data.objects if o.type=="MESH"}
for o in objects.values(): o.hide_render=True
placements=[
    ("keep_core",(-4.4,3.1,0),0),("keep_tier1",(-4.4,3.1,0),0),
    ("keep_tier2",(-4.4,3.1,0),0),
    ("wall_10",(-4.4,-1.8,0),0),
    ("tower",(-9.3,-1.8,0),0),("gate",(-.2,-1.8,0),0),
    ("cottage",(3.0,.9,0),0),("longhouse",(6.5,3.6,0),.1),
    ("well",(3.9,-2.0,0),0),("cart",(6.2,-1.0,0),0),
    ("boulder_a",(6.9,-3.4,0),0),("snow_pine_a",(-9.9,3.1,0),0),
]
for name,position,rot in placements:
    o=objects[name];o.hide_render=False;o.location=position;o.rotation_euler.z=rot
for name,x,y,scale in (
    ("grass_a",.8,-3.0,1),("grass_b",2.4,-3.4,1),("fern_a",7.2,-2.6,1),
    ("shrub_a",7.8,.3,1.15),("grass_c",-.8,-3.3,1),("rock_a",.2,1.0,1),
):
    src=objects[name]
    dup=src.copy();dup.data=src.data;scene.collection.objects.link(dup)
    dup.hide_render=False;dup.location=(x,y,0);dup.scale=(scale,)*3
ground=bpy.data.meshes.new("preview-ground")
ground.from_pydata([(-18,-12,-.07),(18,-12,-.07),(18,14,-.07),(-18,14,-.07)],
                   [],[(0,1,2),(0,2,3)])
obj=bpy.data.objects.new("preview-ground",ground);scene.collection.objects.link(obj)
mat=bpy.data.materials.new("preview-ground-mat")
mat.diffuse_color=(.27,.30,.24,1);obj.data.materials.append(mat)
world=bpy.data.worlds.new("preview sky") if not scene.world else scene.world
scene.world=world;world.use_nodes=True
world.node_tree.nodes["Background"].inputs["Color"].default_value=(.44,.53,.65,1)
world.node_tree.nodes["Background"].inputs["Strength"].default_value=.55
sun=bpy.data.lights.new("preview sun","SUN");sun.energy=2.25;sun.angle=math.radians(6)
sobj=bpy.data.objects.new("preview sun",sun);scene.collection.objects.link(sobj)
sobj.rotation_euler=(math.radians(30),math.radians(-25),math.radians(28))
cam_data=bpy.data.cameras.new("preview camera")
cam=bpy.data.objects.new("preview camera",cam_data);scene.collection.objects.link(cam)
cam.location=(13,-19,12)
target=Vector((-1.3,0,1.4))
cam.rotation_euler=(target-Vector(cam.location)).to_track_quat("-Z","Y").to_euler()
cam_data.type="ORTHO";cam_data.ortho_scale=20.5
scene.camera=cam
scene.render.engine="BLENDER_EEVEE_NEXT"
scene.render.resolution_x=1600;scene.render.resolution_y=900;scene.render.resolution_percentage=100
scene.render.image_settings.file_format="PNG"
scene.render.filepath=str(ART/"studio_preview.png")
scene.view_settings.view_transform="AgX"
bpy.ops.render.render(write_still=True)
print("WARBELL_ENV_PREVIEW_DONE",str(ART/"studio_preview.png"))
