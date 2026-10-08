"""Read-only connected leaf mesh island distribution through Blender MCP."""
import bpy
import numpy as np
from collections import Counter

leaf=next(o for o in bpy.context.scene.objects if o.type=="MESH" and
          o.material_slots and o.material_slots[0].material.name=="tree_small_02_leaves")
mesh=leaf.data
nv=len(mesh.vertices)
edges=np.empty(len(mesh.edges)*2,dtype=np.int32)
mesh.edges.foreach_get("vertices",edges)
edges=edges.reshape(-1,2)
parent=list(range(nv))
size=[1]*nv
def find(i):
    while parent[i]!=i:
        parent[i]=parent[parent[i]]
        i=parent[i]
    return i
for a,b in edges:
    a=find(int(a)); b=find(int(b))
    if a!=b:
        if size[a]<size[b]:a,b=b,a
        parent[b]=a;size[a]+=size[b]
roots={find(i) for i in range(nv)}
sizes=Counter(size[r] for r in roots)
print("LEAF_ISLANDS",len(roots),"TOP_VERTEX_SIZES",sorted((size[r] for r in roots),reverse=True)[:12],
      "COMMON_VERTEX_SIZES",sizes.most_common(12))
