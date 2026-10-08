"""Audit source materials after import; read-only Blender MCP."""
import bpy

for mat in bpy.data.materials:
    if not mat.name.startswith("tree_small_02"): continue
    print("MATERIAL",mat.name,"surface_render_method",getattr(mat,"surface_render_method",None),
          "alpha_threshold",getattr(mat,"alpha_threshold",None),"diffuse",list(mat.diffuse_color))
    for node in mat.node_tree.nodes if mat.use_nodes else []:
        if node.type=="TEX_IMAGE":
            print("IMAGE",node.name,node.image.name if node.image else None,
                  node.image.filepath if node.image else None)
        elif node.type=="BSDF_PRINCIPLED":
            print("PRINCIPLED",node.name,
                  [(i.name,i.default_value if isinstance(i.default_value,(float,int)) else "vector")
                   for i in node.inputs if i.name in {"Alpha","Roughness","Metallic"}],
                  [(i.name,[f"{l.from_node.name}.{l.from_socket.name}" for l in i.links])
                   for i in node.inputs if i.name in {"Base Color","Alpha","Normal","Roughness","Metallic"}])
