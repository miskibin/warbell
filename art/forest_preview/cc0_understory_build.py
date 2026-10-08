"""Blender MCP adaptation of six audited Poly Haven 1K non-tree sources.

Run only against this task's isolated official Blender MCP server on port 9878.
Every exported GLB keeps the imported UVs, diffuse, normal and ORM material;
foliage uses the source's separate alpha map merged with its opaque JPG diffuse.
After export, run ``cc0_understory_finalize.py`` to convert embedded JPEGs to
PNG for Warbell's Bevy image features and refresh the runtime asset hashes.
"""

from hashlib import sha256
import json
import math
from pathlib import Path
import random

import bpy


ROOT = Path(__file__).resolve().parents[2]
SOURCES = ROOT / "target/world-preview/cc0-sources"
OUTPUT = ROOT / "assets/models/forest_slice/cc0"
REPORT = ROOT / "art/forest_preview/cc0_understory_exports.json"
OUTPUT.mkdir(parents=True, exist_ok=True)
records = []


def clear_scene():
    bpy.ops.object.select_all(action="SELECT")
    bpy.ops.object.delete(use_global=False)
    bpy.data.orphans_purge(do_recursive=True)


def import_source(asset):
    clear_scene()
    source = SOURCES / asset / f"{asset}_1k.gltf"
    bpy.ops.import_scene.gltf(filepath=str(source))
    objects = [obj for obj in bpy.context.scene.objects if obj.type == "MESH"]
    if not objects:
        raise RuntimeError(f"No meshes imported from {source}")
    if asset in ("shrub_03", "grass_medium_01", "dandelion_01", "flower_heliophila"):
        rgba = SOURCES / asset / "textures" / f"{asset}_diff_alpha_1k.png"
        image = bpy.data.images.load(str(rgba), check_existing=True)
        image.colorspace_settings.name = "sRGB"
        for material in {mat for obj in objects for mat in obj.data.materials if mat}:
            material.use_nodes = True
            nodes = material.node_tree.nodes
            links = material.node_tree.links
            bsdf = next(node for node in nodes if node.type == "BSDF_PRINCIPLED")
            diffuse = next(
                link.from_node
                for link in links
                if link.to_node == bsdf
                and link.to_socket.name == "Base Color"
                and link.from_node.type == "TEX_IMAGE"
            )
            diffuse.image = image
            links.new(diffuse.outputs["Color"], bsdf.inputs["Base Color"])
            # Blender 4.5's glTF exporter derives alphaMode from the node tree.
            # An explicit compare turns the RGBA source into glTF MASK rather
            # than translucent BLEND while preserving the embedded alpha map.
            clip = nodes.new("ShaderNodeMath")
            clip.name = "CC0 alpha clip 0.38"
            clip.operation = "GREATER_THAN"
            clip.inputs[1].default_value = 0.38
            links.new(diffuse.outputs["Alpha"], clip.inputs[0])
            links.new(clip.outputs[0], bsdf.inputs["Alpha"])
            material.surface_render_method = "DITHERED"
            material.alpha_threshold = 0.38
            material.use_backface_culling = False
    return objects


def source_object(objects, needle):
    matches = [obj for obj in objects if needle in obj.name]
    if len(matches) != 1:
        raise ValueError(f"Expected one source object for {needle}, got {[obj.name for obj in matches]}")
    return matches[0]


def clone(source, x, y, angle=0.0, scale=1.0):
    obj = source.copy()
    obj.data = source.data.copy()
    bpy.context.collection.objects.link(obj)
    obj.location = (x, y, 0.0)
    obj.rotation_euler = (0.0, 0.0, angle)
    obj.scale = (scale, scale, scale)
    return obj


def export_group(name, source_asset, objects, decimate=None):
    if not objects:
        raise ValueError(f"{name}: empty geometry")
    bpy.ops.object.select_all(action="DESELECT")
    for obj in objects:
        obj.select_set(True)
    bpy.context.view_layer.objects.active = objects[0]
    bpy.ops.object.join()
    joined = bpy.context.view_layer.objects.active
    bpy.ops.object.transform_apply(location=True, rotation=True, scale=True)
    low_z = min(vertex.co.z for vertex in joined.data.vertices)
    for vertex in joined.data.vertices:
        vertex.co.z -= low_z
    joined.data.update()
    joined.name = name
    joined.data.name = name
    if decimate is not None:
        modifier = joined.modifiers.new("Conservative geometric LOD", "DECIMATE")
        modifier.ratio = decimate
        bpy.ops.object.modifier_apply(modifier=modifier.name)
    triangles = sum(len(poly.vertices) - 2 for poly in joined.data.polygons)
    bounds = [joined.matrix_world @ vertex.co for vertex in joined.data.vertices]
    minimum = [min(point[i] for point in bounds) for i in range(3)]
    maximum = [max(point[i] for point in bounds) for i in range(3)]
    bpy.ops.object.select_all(action="DESELECT")
    joined.select_set(True)
    bpy.context.view_layer.objects.active = joined
    target = OUTPUT / f"{name}.glb"
    bpy.ops.export_scene.gltf(
        filepath=str(target),
        export_format="GLB",
        use_selection=True,
        export_yup=True,
        export_tangents=True,
        # Blender 4.5 has no force-PNG export enum. Its AUTO mode embeds JPEG
        # for opaque maps; the audited post-export transcode converts those
        # images to PNG without changing decoded pixels or mesh/material data.
        export_image_format="AUTO",
        export_texcoords=True,
        export_normals=True,
        export_materials="EXPORT",
        export_cameras=False,
        export_lights=False,
    )
    records.append(
        {
            "id": name,
            "source": source_asset,
            "path": target.relative_to(ROOT).as_posix(),
            "sha256": sha256(target.read_bytes()).hexdigest(),
            "triangles": triangles,
            "bounds_min_blender_xyz_m": [round(value, 5) for value in minimum],
            "bounds_max_blender_xyz_m": [round(value, 5) for value in maximum],
            "extent_blender_xyz_m": [round(maximum[i] - minimum[i], 5) for i in range(3)],
            "base_z_m": 0.0,
        }
    )
    bpy.data.objects.remove(joined, do_unlink=True)


def patch_from_tufts(objects, name, seed):
    rng = random.Random(seed)
    tall = [source_object(objects, f"tall_{letter}_LOD0") for letter in "abc"]
    tiny = [source_object(objects, f"tiny_{letter}_LOD0") for letter in "abcdef"]
    pieces = []
    for index in range(18):
        radius = math.sqrt(rng.random()) * 0.43
        theta = rng.random() * math.tau
        template = tall[index % len(tall)]
        pieces.append(
            clone(
                template,
                radius * math.cos(theta),
                radius * math.sin(theta),
                rng.random() * math.tau,
                rng.uniform(0.90, 1.24),
            )
        )
    for index in range(9):
        radius = math.sqrt(rng.random()) * 0.45
        theta = rng.random() * math.tau
        pieces.append(
            clone(
                tiny[index % len(tiny)],
                radius * math.cos(theta),
                radius * math.sin(theta),
                rng.random() * math.tau,
                rng.uniform(0.96, 1.45),
            )
        )
    export_group(name, "grass_medium_01", pieces)


grass = import_source("grass_medium_01")
patch_from_tufts(grass, "cc0_grass_patch_a", 1432)
patch_from_tufts(grass, "cc0_grass_patch_b", 7729)

shrub = import_source("shrub_03")
shrub_pieces = []
for index, needle in enumerate(("shrub_03_a", "shrub_03_b", "shrub_03_c", "shrub_03_d")):
    angle = index * math.tau / 4
    shrub_pieces.append(
        clone(
            source_object(shrub, needle),
            0.16 * math.cos(angle),
            0.16 * math.sin(angle),
            angle + 0.35,
            (1.45, 1.65, 1.75, 1.75)[index],
        )
    )
export_group("cc0_shrub_patch_a", "shrub_03", shrub_pieces)

dandelion = import_source("dandelion_01")
dan_pieces = [
    clone(source_object(dandelion, "dandelion_01_c_LOD0"), -0.17, 0.06, 0.35, 1.22),
    clone(source_object(dandelion, "dandelion_01_d_LOD0"), 0.08, -0.14, 1.90, 1.35),
    clone(source_object(dandelion, "dandelion_01_e_LOD0"), 0.19, 0.13, 3.20, 1.31),
]
export_group("cc0_dandelion_a", "dandelion_01", dan_pieces)

heliophila = import_source("flower_heliophila")
export_group(
    "cc0_heliophila_a",
    "flower_heliophila",
    [clone(source_object(heliophila, "flower_heliophila_medium"), 0, 0, 0.44, 1.06)],
    decimate=0.48,
)

stump = import_source("tree_stump_01")
export_group(
    "cc0_stump_a",
    "tree_stump_01",
    [clone(source_object(stump, "tree_stump_01"), 0, 0, 0.0, 0.82)],
    decimate=0.42,
)

rocks = import_source("rock_moss_set_01")
export_group(
    "cc0_moss_rock_a",
    "rock_moss_set_01",
    [clone(source_object(rocks, "rock03"), 0, 0, 0.55, 0.38)],
    decimate=0.82,
)
export_group(
    "cc0_moss_rock_b",
    "rock_moss_set_01",
    [clone(source_object(rocks, "rock06"), 0, 0, 1.27, 0.34)],
    decimate=0.62,
)

REPORT.write_text(
    json.dumps(
        {
            "schema": "warbell.forest_cc0_understory_exports.v1",
            "method": "Blender 4.5.9 through official mcp-for-blender execute_blender_code on isolated port 9878",
            "source_manifest": "art/forest_preview/cc0_understory_downloads.json",
            "rgba_manifest": "art/forest_preview/cc0_understory_rgba.json",
            "models": records,
        },
        indent=2,
    )
    + "\n",
    encoding="utf-8",
)
print("WARBELL_CC0_UNDERSTORY_DONE", json.dumps(records), flush=True)
