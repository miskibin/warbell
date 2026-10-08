"""Read-only nearest Blender corners for the one zero GLB tangent."""
import bpy

trunk=next(o for o in bpy.context.scene.objects if o.type=="MESH" and
           o.material_slots and o.material_slots[0].material.name=="tree_small_02_trunk")
mesh=trunk.data
target=(0.3490058481693268,-1.92645263671875,4.943565368652344)
near=sorted(((sum((v.co[i]-target[i])**2 for i in range(3)),v.index,tuple(v.co))
             for v in mesh.vertices))[:8]
print("NEAREST",near,"UVLAYERS",[l.name for l in mesh.uv_layers])
for distance,index,_ in near[:3]:
    for poly in mesh.polygons:
        if index not in poly.vertices:continue
        print("CORNER",distance,poly.index,
              [(loop_index,mesh.loops[loop_index].vertex_index,
                [tuple(l.data[loop_index].uv) for l in mesh.uv_layers])
               for loop_index in poly.loop_indices])
        break
