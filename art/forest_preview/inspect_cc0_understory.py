"""Inspect downloaded Poly Haven glTF scene geometry from its JSON and BIN data."""

from __future__ import annotations

from json import dumps, loads
from pathlib import Path

import numpy as np


ROOT = Path(__file__).resolve().parents[2]
SOURCES = ROOT / "target/world-preview/cc0-sources"
REPORT = ROOT / "art/forest_preview/cc0_understory_geometry.json"
ASSETS = (
    "shrub_03",
    "tree_stump_01",
    "grass_medium_01",
    "rock_moss_set_01",
    "dandelion_01",
    "flower_heliophila",
)


def node_matrix(node: dict) -> np.ndarray:
    if "matrix" in node:
        return np.asarray(node["matrix"], dtype=np.float64).reshape((4, 4)).T
    x, y, z, w = node.get("rotation", [0, 0, 0, 1])
    rotation = np.array(
        [
            [1 - 2 * (y * y + z * z), 2 * (x * y - z * w), 2 * (x * z + y * w)],
            [2 * (x * y + z * w), 1 - 2 * (x * x + z * z), 2 * (y * z - x * w)],
            [2 * (x * z - y * w), 2 * (y * z + x * w), 1 - 2 * (x * x + y * y)],
        ],
        dtype=np.float64,
    )
    matrix = np.eye(4)
    matrix[:3, :3] = rotation @ np.diag(node.get("scale", [1, 1, 1]))
    matrix[:3, 3] = node.get("translation", [0, 0, 0])
    return matrix


def inspect(asset: str) -> dict:
    folder = SOURCES / asset
    gltf = loads((folder / f"{asset}_1k.gltf").read_text(encoding="utf-8"))
    buffers = []
    for entry in gltf["buffers"]:
        raw = (folder / entry["uri"]).read_bytes()
        if len(raw) != entry["byteLength"]:
            raise ValueError(f"{asset}: buffer size mismatch")
        buffers.append(raw)

    def accessor_array(index: int) -> np.ndarray:
        accessor = gltf["accessors"][index]
        view = gltf["bufferViews"][accessor["bufferView"]]
        if accessor.get("sparse"):
            raise ValueError(f"{asset}: sparse accessor unsupported in audit")
        components = {"SCALAR": 1, "VEC2": 2, "VEC3": 3, "VEC4": 4}[accessor["type"]]
        dtype = {5121: np.uint8, 5123: np.uint16, 5125: np.uint32, 5126: np.float32}[
            accessor["componentType"]
        ]
        item_size = np.dtype(dtype).itemsize
        offset = view.get("byteOffset", 0) + accessor.get("byteOffset", 0)
        stride = view.get("byteStride", item_size * components)
        return np.ndarray(
            shape=(accessor["count"], components),
            dtype=dtype,
            buffer=buffers[view["buffer"]],
            offset=offset,
            strides=(stride, item_size),
        )

    position_cache = {}
    unique_triangles = 0
    primitive_count = 0
    for mesh in gltf.get("meshes", []):
        for primitive in mesh["primitives"]:
            if primitive.get("mode", 4) != 4:
                raise ValueError(f"{asset}: non-triangle primitive")
            position_index = primitive["attributes"]["POSITION"]
            position_cache[position_index] = accessor_array(position_index).astype(np.float64)
            count = (
                gltf["accessors"][primitive["indices"]]["count"]
                if "indices" in primitive
                else len(position_cache[position_index])
            )
            unique_triangles += count // 3
            primitive_count += 1

    lo = np.full(3, np.inf)
    hi = np.full(3, -np.inf)
    scene_triangles = 0
    mesh_instances = 0
    objects = []

    def visit(index: int, parent: np.ndarray) -> None:
        nonlocal lo, hi, scene_triangles, mesh_instances
        node = gltf["nodes"][index]
        world = parent @ node_matrix(node)
        if "mesh" in node:
            mesh_instances += 1
            object_lo = np.full(3, np.inf)
            object_hi = np.full(3, -np.inf)
            object_triangles = 0
            for primitive in gltf["meshes"][node["mesh"]]["primitives"]:
                points = position_cache[primitive["attributes"]["POSITION"]]
                transformed = points @ world[:3, :3].T + world[:3, 3]
                object_lo = np.minimum(object_lo, transformed.min(axis=0))
                object_hi = np.maximum(object_hi, transformed.max(axis=0))
                count = (
                    gltf["accessors"][primitive["indices"]]["count"]
                    if "indices" in primitive
                    else len(points)
                )
                object_triangles += count // 3
            lo = np.minimum(lo, object_lo)
            hi = np.maximum(hi, object_hi)
            scene_triangles += object_triangles
            objects.append(
                {
                    "node": node.get("name", str(index)),
                    "mesh": gltf["meshes"][node["mesh"]].get("name", str(node["mesh"])),
                    "triangles": object_triangles,
                    "bounds_min_xyz_m": object_lo.round(5).tolist(),
                    "bounds_max_xyz_m": object_hi.round(5).tolist(),
                    "extent_xyz_m": (object_hi - object_lo).round(5).tolist(),
                }
            )
        for child in node.get("children", []):
            visit(child, world)

    scene = gltf["scenes"][gltf.get("scene", 0)]
    for root in scene["nodes"]:
        visit(root, np.eye(4))
    if not np.isfinite(lo).all():
        raise ValueError(f"{asset}: scene has no positioned vertices")
    materials = []
    for material in gltf.get("materials", []):
        pbr = material.get("pbrMetallicRoughness", {})
        materials.append(
            {
                "name": material.get("name", ""),
                "alpha_mode": material.get("alphaMode", "OPAQUE"),
                "alpha_cutoff": material.get("alphaCutoff"),
                "double_sided": material.get("doubleSided", False),
                "base_color_texture": pbr.get("baseColorTexture", {}).get("index"),
                "orm_texture": pbr.get("metallicRoughnessTexture", {}).get("index"),
                "normal_texture": material.get("normalTexture", {}).get("index"),
            }
        )
    return {
        "asset": asset,
        "meshes": len(gltf.get("meshes", [])),
        "mesh_instances": mesh_instances,
        "primitives": primitive_count,
        "unique_triangles": unique_triangles,
        "scene_triangles": scene_triangles,
        "materials": materials,
        "objects": objects,
        "image_uris": [image.get("uri") for image in gltf.get("images", [])],
        "bounds_min_xyz_m": lo.round(5).tolist(),
        "bounds_max_xyz_m": hi.round(5).tolist(),
        "extent_xyz_m": (hi - lo).round(5).tolist(),
        "footprint_xz_m": (hi - lo)[[0, 2]].round(5).tolist(),
    }


def main() -> None:
    assets = [inspect(asset) for asset in ASSETS]
    REPORT.write_text(
        dumps({"schema": "warbell.forest_cc0_understory_geometry.v1", "assets": assets}, indent=2)
        + "\n",
        encoding="utf-8",
    )
    for asset in assets:
        print(
            f"{asset['asset']}: {asset['meshes']} meshes, {asset['scene_triangles']} tris, "
            f"{asset['footprint_xz_m']}m footprint, {asset['extent_xyz_m'][1]}m high, "
            f"{len(asset['materials'])} materials"
        )


if __name__ == "__main__":
    main()
