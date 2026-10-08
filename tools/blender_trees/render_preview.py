"""CPU-only neutral studio line-up of the five MCP-authored trees."""
from pathlib import Path
import bpy, math
from mathutils import Vector

ROOT=Path(__file__).resolve().parents[2]
OUT=ROOT/"art/blender_trees/studio_preview.png"

floor_mat=bpy.data.materials.new("warm neutral floor")
floor_mat.diffuse_color=(.41,.43,.36,1)
floor_mat.use_nodes=True
floor_mat.node_tree.nodes.get("Principled BSDF").inputs["Base Color"].default_value=(.41,.43,.36,1)
bpy.ops.mesh.primitive_plane_add(size=200, location=(0,0,-.015))
floor=bpy.context.object
floor.name="preview floor"
floor.data.materials.append(floor_mat)

bpy.ops.object.camera_add(location=(5,-11,4.8))
cam=bpy.context.object
cam.name="preview camera"
target=Vector((0,0,1.08))
cam.rotation_euler=(target-cam.location).to_track_quat('-Z','Y').to_euler()
cam.data.type='ORTHO'; cam.data.ortho_scale=12.0
bpy.context.scene.camera=cam

def area(name,loc,power,size):
    bpy.ops.object.light_add(type='AREA',location=loc)
    l=bpy.context.object;l.name=name;l.data.energy=power;l.data.shape='DISK';l.data.size=size
    l.rotation_euler=(target-l.location).to_track_quat('-Z','Y').to_euler()
area("large soft key",(-4,-3,8),1600,7)
area("soft sky fill",(4,1,6),850,8)
area("subtle rim",(0,5,6),900,6)

world=bpy.context.scene.world
world.use_nodes=True
world.node_tree.nodes.get("Background").inputs["Color"].default_value=(.50,.57,.63,1)
world.node_tree.nodes.get("Background").inputs["Strength"].default_value=.6
s=bpy.context.scene
s.render.engine='CYCLES'
s.cycles.device='CPU'
s.cycles.samples=24
s.render.resolution_x=1440
s.render.resolution_y=720
s.render.resolution_percentage=100
s.render.image_settings.file_format='PNG'
s.render.filepath=str(OUT)
s.view_settings.view_transform='AgX'
s.view_settings.look='AgX - Medium High Contrast'
s.render.film_transparent=False
bpy.ops.render.render(write_still=True)
print("WARBELL_STUDIO_PREVIEW",OUT,OUT.stat().st_size)
