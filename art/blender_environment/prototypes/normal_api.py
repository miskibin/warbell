import bpy
mesh=bpy.data.meshes.new("normal-api-test")
print("CUSTOM_NORMAL_APIS",[s for s in dir(mesh) if "normal" in s.lower()],
      [s for s in dir(bpy.types.GeometryNodeSetMeshNormal) if "mode" in s.lower()])
bpy.data.meshes.remove(mesh)
