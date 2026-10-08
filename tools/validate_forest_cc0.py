"""Audit the standalone Blender-exported GLBs used by the forest approval slice.

No renderer or Blender state is trusted: buffer bounds, indices, attributes,
embedded images, material references, scene selection, and runtime instances
are checked directly against the exported files.
"""
from pathlib import Path
import hashlib
import io
import json
import struct
import numpy as np
from PIL import Image

ROOT = Path(__file__).resolve().parents[1]
ASSETS = ROOT / "assets/models/forest_slice"
OUT = ROOT / "art/forest_preview/cc0_validation.json"


def validate(path):
    raw = path.read_bytes()
    magic, version, size = struct.unpack_from("<4sII", raw)
    assert (magic, version, size) == (b"glTF", 2, len(raw)), "invalid GLB header"
    chunks = {}
    offset = 12
    while offset < size:
        length, kind = struct.unpack_from("<I4s", raw, offset)
        assert offset + 8 + length <= size, "truncated chunk"
        chunks[kind] = raw[offset + 8:offset + 8 + length]
        offset += 8 + length
    doc = json.loads(chunks[b"JSON"])
    binary = chunks[b"BIN\0"]
    assert len(doc["buffers"]) == 1 and "uri" not in doc["buffers"][0]
    assert len(binary) >= doc["buffers"][0]["byteLength"]
    scenes = doc.get("scenes", [])
    assert scenes and scenes[0].get("nodes"), "runtime loads Scene(0)"
    for view in doc["bufferViews"]:
        assert view.get("buffer", 0) == 0
        assert view.get("byteOffset", 0) + view["byteLength"] <= len(binary)

    def accessor(index):
        entry = doc["accessors"][index]
        assert "sparse" not in entry, "unexpected sparse accessor"
        view = doc["bufferViews"][entry["bufferView"]]
        dtype = np.dtype({5120: "i1", 5121: "u1", 5122: "<i2", 5123: "<u2",
                          5125: "<u4", 5126: "<f4"}[entry["componentType"]])
        width = {"SCALAR": 1, "VEC2": 2, "VEC3": 3, "VEC4": 4, "MAT4": 16}[entry["type"]]
        stride = view.get("byteStride", width * dtype.itemsize)
        start = view.get("byteOffset", 0) + entry.get("byteOffset", 0)
        count = entry["count"]
        needed = entry.get("byteOffset", 0) + max(count - 1, 0) * stride + width * dtype.itemsize
        assert needed <= view["byteLength"], "accessor beyond buffer view"
        array = np.ndarray((count, width), dtype=dtype, buffer=binary,
                           offset=start, strides=(stride, dtype.itemsize))
        assert np.isfinite(array).all(), "non-finite attribute"
        return array

    triangles = 0
    mesh_reports = []
    materials = doc.get("materials", [])
    for mesh in doc.get("meshes", []):
        for primitive in mesh["primitives"]:
            assert primitive.get("mode", 4) == 4, "triangle export required"
            attrs = primitive["attributes"]
            position = accessor(attrs["POSITION"])
            normal = accessor(attrs["NORMAL"])
            assert len(position) == len(normal)
            assert np.max(np.abs(np.linalg.norm(normal, axis=1) - 1)) < 0.025
            if "TEXCOORD_0" in attrs:
                assert len(accessor(attrs["TEXCOORD_0"])) == len(position)
            if "TANGENT" in attrs:
                tangent = accessor(attrs["TANGENT"])
                assert len(tangent) == len(position)
                assert np.max(np.abs(np.linalg.norm(tangent[:, :3], axis=1) - 1)) < 0.025
                assert np.max(np.abs(np.abs(tangent[:, 3]) - 1)) < 0.001
            indices = accessor(primitive["indices"]).reshape(-1)
            assert len(indices) % 3 == 0 and indices.min() >= 0 and indices.max() < len(position)
            material = materials[primitive["material"]]
            if "normalTexture" in material:
                assert "TANGENT" in attrs and "TEXCOORD_0" in attrs, "normal map requires tangents and UV"
            triangles += len(indices) // 3
            mesh_reports.append({"name": mesh.get("name"), "vertices": len(position),
                                 "triangles": len(indices) // 3,
                                 "min": position.min(axis=0).tolist(),
                                 "max": position.max(axis=0).tolist(),
                                 "material": primitive["material"]})
    assert triangles > 0
    for texture in doc.get("textures", []):
        assert 0 <= texture["source"] < len(doc.get("images", []))
    image_reports = []
    for img in doc.get("images", []):
        assert "bufferView" in img and "uri" not in img, "standalone GLB requires embedded images"
        view = doc["bufferViews"][img["bufferView"]]
        start = view.get("byteOffset", 0)
        data = binary[start:start + view["byteLength"]]
        assert img["mimeType"] == "image/png", "Warbell's current Bevy build supports PNG-only GLB images"
        assert data.startswith(b"\x89PNG")
        decoded = Image.open(io.BytesIO(data))
        alpha = np.asarray(decoded.convert("RGBA"))[..., 3]
        image_reports.append({"name": img.get("name"), "bytes": len(data),
                              "sha256": hashlib.sha256(data).hexdigest(),
                              "dimensions": list(decoded.size),
                              "alpha_min": int(alpha.min()), "alpha_max": int(alpha.max()),
                              "alpha_below_half_fraction": float((alpha < 128).mean())})
    for material in materials:
        mode = material.get("alphaMode", "OPAQUE")
        assert mode in ("OPAQUE", "MASK"), "forest foliage must use depth-tested MASK, not alpha BLEND"
        base = material.get("pbrMetallicRoughness", {}).get("baseColorTexture")
        if base is not None:
            source = doc["textures"][base["index"]]["source"]
            if image_reports[source]["alpha_min"] < 128:
                assert mode == "MASK", "cutout base color must use MASK"
        if material.get("alphaMode") == "MASK":
            assert base is not None, "masked foliage requires textured alpha"
            source = doc["textures"][base["index"]]["source"]
            assert image_reports[source]["alpha_min"] < 128, "MASK material has opaque image alpha"
    return {"file": str(path.relative_to(ROOT)).replace("\\", "/"),
            "sha256": hashlib.sha256(raw).hexdigest(), "bytes": len(raw),
            "triangles": triangles, "meshes": mesh_reports, "images": image_reports,
            "materials": [{"name": m.get("name"), "alpha_mode": m.get("alphaMode", "OPAQUE"),
                           "double_sided": m.get("doubleSided", False),
                           "normal_map": "normalTexture" in m,
                           "base_texture": "baseColorTexture" in m.get("pbrMetallicRoughness", {})}
                          for m in materials]}


def main():
    layout = json.loads((ASSETS / "layout.json").read_text())
    ids = sorted({row["model"][5:] for row in layout["instances"] if row["model"].startswith("gltf:")})
    assert ids, "no imported models in runtime layout"
    results, issues = [], []
    for name in ids:
        try:
            assert name and all(c.isalnum() or c in "_-" for c in name), "invalid model ID"
            results.append(validate(ASSETS / "cc0" / (name + ".glb")))
        except Exception as error:
            issues.append({"model": name, "error": repr(error)})
    counts = {name: sum(row["model"] == "gltf:" + name for row in layout["instances"]) for name in ids}
    report = {"schema": "warbell.forest_slice_cc0_validation.v1", "pass": not issues,
              "layout_sha256": hashlib.sha256((ASSETS / "layout.json").read_bytes()).hexdigest(),
              "instance_counts": counts, "models": results, "issues": issues}
    OUT.write_text(json.dumps(report, indent=2), encoding="utf-8")
    print(json.dumps({"pass": report["pass"], "models": len(results), "instances": counts,
                      "source_triangles": sum(row["triangles"] for row in results), "issues": issues}))
    raise SystemExit(0 if report["pass"] else 1)


if __name__ == "__main__":
    main()
