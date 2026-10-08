"""Read-only mesh/material inspection inside isolated Blender MCP."""
from pathlib import Path
import bpy
import json
from collections import Counter
from mathutils import Vector

ROOT=Path(__file__).resolve().parents[2]
SRC=ROOT/"target/world-preview/cc0-sources/tree_small_02/tree_small_02_1k.gltf"
before=set(bpy.data.objects)
bpy.ops.import_scene.gltf(filepath=str(SRC))
objects=[o for o in bpy.data.objects if o not in before]
report=[]
for o in objects:
    if o.type!="MESH":continue
    mesh=o.data
    bounds=[o.matrix_world@Vector(corner) for corner in o.bound_box]
    report.append({"name":o.name,"vertices":len(mesh.vertices),
                   "polygons":len(mesh.polygons),
                   "materials":[m.name if m else None for m in mesh.materials],
                   "material_faces":dict(Counter(mesh.materials[p.material_index].name
                                                 for p in mesh.polygons)),
                   "bounds_min":[min(v[i] for v in bounds) for i in range(3)],
                   "bounds_max":[max(v[i] for v in bounds) for i in range(3)]})
print("WARBELL_CC0_INSPECT",json.dumps(report))
