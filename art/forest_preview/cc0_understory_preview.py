"""CPU render of the isolated CC0 GLB outputs for silhouette/alpha QA."""

from pathlib import Path
import math

import bpy
from mathutils import Vector


root = Path(__file__).resolve().parents[2]
source = root / "assets/models/forest_slice/cc0"
target = root / "target/world-preview/cc0-understory/preview.png"
bpy.ops.object.select_all(action="SELECT")
bpy.ops.object.delete(use_global=False)

models = [
    "cc0_grass_patch_a",
    "cc0_grass_patch_b",
    "cc0_shrub_patch_a",
    "cc0_dandelion_a",
    "cc0_heliophila_a",
    "cc0_stump_a",
    "cc0_moss_rock_a",
    "cc0_moss_rock_b",
]
for index, name in enumerate(models):
    before = set(bpy.data.objects)
    bpy.ops.import_scene.gltf(filepath=str(source / f"{name}.glb"))
    imported = [obj for obj in bpy.data.objects if obj not in before and obj.type == "MESH"]
    x = (index % 4 - 1.5) * 1.9
    y = (0.65 if index < 4 else -0.95)
    for obj in imported:
        obj.location.x += x
        obj.location.y += y

bpy.ops.mesh.primitive_plane_add(size=16)
ground = bpy.context.object
ground.name = "neutral preview ground"
ground.location.z = -0.02
material = bpy.data.materials.new("matte earthen stage")
material.diffuse_color = (0.24, 0.30, 0.22, 1)
material.use_nodes = True
material.node_tree.nodes.get("Principled BSDF").inputs["Base Color"].default_value = (
    0.24, 0.30, 0.22, 1
)
ground.data.materials.append(material)

bpy.ops.object.light_add(type="AREA", location=(-3.5, -4.5, 7))
light = bpy.context.object
light.data.energy = 1400
light.data.shape = "DISK"
light.data.size = 5

bpy.ops.object.camera_add(location=(0, -8.3, 4.7))
camera = bpy.context.object
direction = Vector((0, 0, 0.15)) - camera.location
camera.rotation_euler = direction.to_track_quat("-Z", "Y").to_euler()
camera.data.type = "ORTHO"
camera.data.ortho_scale = 9.2

scene = bpy.context.scene
scene.camera = camera
scene.render.engine = "CYCLES"
scene.cycles.device = "CPU"
scene.cycles.samples = 12
scene.render.resolution_x = 1440
scene.render.resolution_y = 900
scene.render.resolution_percentage = 100
scene.render.image_settings.file_format = "PNG"
scene.render.filepath = str(target)
scene.world.color = (0.42, 0.42, 0.42)
scene.view_settings.view_transform = "AgX"
bpy.ops.render.render(write_still=True)
print("WARBELL_CC0_UNDERSTORY_PREVIEW", str(target), flush=True)
