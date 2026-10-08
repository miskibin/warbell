"""Read-only node/image inventory for the packed small-tree trunk material."""
import bpy
import json

mat = bpy.data.materials["tree_small_02_trunk"]
nodes = mat.node_tree.nodes
links = mat.node_tree.links
report = {
    "blend": bpy.data.filepath,
    "material": mat.name,
    "images": [{"node": n.name, "image": n.image.name,
                "filepath": n.image.filepath,
                "colorspace": n.image.colorspace_settings.name,
                "packed": bool(n.image.packed_file)}
               for n in nodes if n.type == "TEX_IMAGE" and n.image],
    "normal_maps": [{"name": n.name,
                     "strength": n.inputs["Strength"].default_value,
                     "strength_linked": bool(n.inputs["Strength"].links)}
                    for n in nodes if n.type == "NORMAL_MAP"],
    "roughness_links": [{"from_node": l.from_node.name,
                         "from_socket": l.from_socket.name,
                         "to_node": l.to_node.name,
                         "to_socket": l.to_socket.name}
                        for l in links if l.to_socket.name == "Roughness"],
}
print("WARBELL_CC0_BARK_SOURCE", json.dumps(report, sort_keys=True))
