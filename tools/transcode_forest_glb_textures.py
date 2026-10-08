"""Repack forest GLBs for Warbell's PNG-only Bevy image feature set.

JPEG pixels are decoded once and saved losslessly as PNG. Mesh bytes, UVs,
material parameters, alpha maps and image resolution remain unchanged.
Blender 4.5's AUTO exporter can retain JPEGs, so this is a reproducible export step.
"""
from pathlib import Path
import hashlib
import io
import json
import struct

import numpy as np
from PIL import Image

ROOT = Path(__file__).resolve().parents[1]
REPORT = ROOT / "art/forest_preview/cc0_png_repack.json"


def repack(path):
    raw = path.read_bytes()
    size, kind = struct.unpack_from("<I4s", raw, 12)
    assert kind == b"JSON"
    doc = json.loads(raw[20:20 + size])
    binary = raw[28 + size:]
    assert raw[24 + size:28 + size] == b"BIN\0"
    views = [dict(view) for view in doc["bufferViews"]]
    images = {image["bufferView"]: image for image in doc.get("images", [])}
    if not any(image["mimeType"] == "image/jpeg" for image in images.values()):
        return None
    output = bytearray()
    before_geometry = hashlib.sha256()
    after_geometry = hashlib.sha256()
    conversions = []
    for index, view in enumerate(views):
        start = view.get("byteOffset", 0)
        data = binary[start:start + view["byteLength"]]
        if index not in images:
            before_geometry.update(data)
        elif images[index]["mimeType"] == "image/jpeg":
            decoded = Image.open(io.BytesIO(data)).convert("RGB")
            encoded = io.BytesIO()
            decoded.save(encoded, format="PNG")
            data = encoded.getvalue()
            check = Image.open(io.BytesIO(data))
            assert np.array_equal(np.asarray(decoded), np.asarray(check)), "pixel change during transcode"
            images[index]["mimeType"] = "image/png"
            conversions.append({"image": images[index].get("name"), "dimensions": list(decoded.size),
                                "decoded_pixel_max_delta": 0})
        doc["bufferViews"][index]["byteOffset"] = len(output)
        doc["bufferViews"][index]["byteLength"] = len(data)
        output.extend(data)
        output.extend(b"\0" * ((-len(output)) % 4))
        if index not in images:
            after_geometry.update(data)
    assert before_geometry.digest() == after_geometry.digest(), "geometry changed"
    doc["buffers"][0]["byteLength"] = len(output)
    encoded_doc = json.dumps(doc, separators=(",", ":")).encode()
    encoded_doc += b" " * ((-len(encoded_doc)) % 4)
    new_raw = (struct.pack("<4sII", b"glTF", 2, 28 + len(encoded_doc) + len(output))
               + struct.pack("<I4s", len(encoded_doc), b"JSON") + encoded_doc
               + struct.pack("<I4s", len(output), b"BIN\0") + output)
    temp = path.with_suffix(".glb.pngtmp")
    temp.write_bytes(new_raw)
    temp.replace(path)
    return {"file": str(path.relative_to(ROOT)).replace("\\", "/"),
            "input_sha256": hashlib.sha256(raw).hexdigest(),
            "output_sha256": hashlib.sha256(new_raw).hexdigest(),
            "input_bytes": len(raw), "output_bytes": len(new_raw),
            "non_image_buffer_views_sha256": before_geometry.hexdigest(),
            "geometry_unchanged": True, "images": conversions}


def main():
    reports = []
    for path in sorted((ROOT / "assets/models/forest_slice/cc0").glob("*.glb")):
        result = repack(path)
        if result:
            reports.append(result)
            print(path.name, len(result["images"]), "JPEGs -> PNG; geometry unchanged", flush=True)
    if reports:
        REPORT.write_text(json.dumps({"schema": "warbell.forest_cc0_png_repack.v1", "models": reports}, indent=2))
    print("Converted", len(reports), "models; no source image pixels resampled or recolored.")


if __name__ == "__main__":
    main()
