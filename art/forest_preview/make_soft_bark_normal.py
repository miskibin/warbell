"""Bake the small-tree trunk normal strength into tangent-space RGB.

Bevy 0.19.1's glTF loader ignores normalTexture.scale, so source and runtime
both use a strength-1 normal map whose X/Y slopes were reduced to 60% before
renormalizing the vector. The verified Poly Haven source JPG stays unchanged.
"""
from pathlib import Path
import hashlib
import json
import numpy as np
from PIL import Image

ROOT = Path(__file__).resolve().parents[2]
ART = ROOT / "art/forest_preview"
SOURCE = ROOT / "target/world-preview/cc0-sources/tree_small_02/textures/tree_small_02_nor_gl_1k.jpg"
OUTPUT = ART / "cc0_prepared/tree_small_02_trunk_nor_soft_1k.png"
REPORT = ART / "soft_bark_normal_map.json"
downloads = json.loads((ART / "cc0_tree_small_02_downloads.json").read_text(encoding="utf-8"))
entry = next(row for row in downloads["files"]
             if row["path"].replace("\\", "/").endswith("tree_small_02_nor_gl_1k.jpg"))
assert hashlib.sha256(SOURCE.read_bytes()).hexdigest() == entry["sha256"]
rgb = np.asarray(Image.open(SOURCE).convert("RGB"), dtype=np.uint8)
normal = rgb.astype(np.float32) / 127.5 - 1.0
original_xy = np.linalg.norm(normal[..., :2], axis=2)
normal[..., :2] *= 0.6
length = np.linalg.norm(normal, axis=2, keepdims=True)
normal /= np.maximum(length, 1e-8)
encoded = np.clip(np.rint((normal + 1.0) * 127.5), 0, 255).astype(np.uint8)
Image.fromarray(encoded, mode="RGB").save(OUTPUT, format="PNG", compress_level=9)
report = {
    "schema": "warbell.forest_soft_bark_normal.v1",
    "source": str(SOURCE.relative_to(ROOT)).replace("\\", "/"),
    "source_sha256": entry["sha256"], "source_url": entry["url"],
    "license": "CC0-1.0",
    "output": str(OUTPUT.relative_to(ROOT)).replace("\\", "/"),
    "output_sha256": hashlib.sha256(OUTPUT.read_bytes()).hexdigest(),
    "dimensions": list(Image.open(OUTPUT).size),
    "tangent_xy_scale_before_renormalize": 0.6,
    "median_xy_length_before": round(float(np.median(original_xy)), 5),
    "median_xy_length_after": round(float(np.median(np.linalg.norm(normal[..., :2], axis=2))), 5),
}
REPORT.write_text(json.dumps(report, indent=2), encoding="utf-8")
print(json.dumps(report, sort_keys=True))
