"""Separate the imported Poly Haven tree by original glTF material in Blender MCP."""
import bpy

tree=next(o for o in bpy.context.scene.objects if o.type=="MESH" and o.name.startswith("tree_small_02"))
bpy.ops.object.select_all(action="DESELECT")
tree.select_set(True)
bpy.context.view_layer.objects.active=tree
print("BEFORE",tree.name,len(tree.data.vertices),len(tree.data.polygons),
      [s.material.name if s.material else None for s in tree.material_slots])
bpy.ops.object.mode_set(mode="EDIT")
bpy.ops.mesh.select_all(action="SELECT")
bpy.ops.mesh.separate(type="MATERIAL")
bpy.ops.object.mode_set(mode="OBJECT")
for obj in sorted((o for o in bpy.context.scene.objects if o.type=="MESH"),key=lambda o:o.name):
    print("SEPARATED",obj.name,len(obj.data.vertices),len(obj.data.polygons),
          [s.material.name if s.material else None for s in obj.material_slots],
          [(uv.name,len(uv.data)) for uv in obj.data.uv_layers])
