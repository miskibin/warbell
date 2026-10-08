"""Load existing editable forest scene in isolated Blender MCP for CC0 update."""
from pathlib import Path
import bpy

ROOT=Path(__file__).resolve().parents[2]
SCENE=ROOT/"art/forest_preview/forest_scene.blend"
bpy.ops.wm.open_mainfile(filepath=str(SCENE))
print("FOREST_SCENE_LOADED",len(bpy.data.objects),
      sum(o.type=="MESH" for o in bpy.data.objects),
      len(bpy.data.meshes),len(bpy.data.images))
