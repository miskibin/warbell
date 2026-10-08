"""Small official-MCP import probe before altering the isolated Blender scene."""

from pathlib import Path

import bpy


root = Path(__file__).resolve().parents[2]
source = root / "target/world-preview/cc0-sources/grass_medium_01/grass_medium_01_1k.gltf"
bpy.ops.object.select_all(action="SELECT")
bpy.ops.object.delete(use_global=False)
bpy.ops.import_scene.gltf(filepath=str(source))
meshes = [obj for obj in bpy.context.scene.objects if obj.type == "MESH"]
materials = list({mat.name for obj in meshes for mat in obj.data.materials if mat})
print(
    "WARBELL_CC0_MCP_PROBE",
    {
        "source": str(source),
        "mesh_objects": len(meshes),
        "sample_names": [obj.name for obj in meshes[:6]],
        "sample_transforms": [
            {
                "name": obj.name,
                "location": list(obj.location),
                "rotation": list(obj.rotation_euler),
                "dimensions": list(obj.dimensions),
            }
            for obj in meshes[:6]
        ],
        "materials": materials,
        "material_alpha_modes": [bpy.data.materials[name].surface_render_method for name in materials],
        "nodes": [
            (node.type, node.name, node.image.name if node.type == "TEX_IMAGE" and node.image else None)
            for node in bpy.data.materials[materials[0]].node_tree.nodes
        ],
    },
    flush=True,
)
