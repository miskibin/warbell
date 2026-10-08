"""Derive a matte trunk ARM map from the verified Poly Haven source scan.

Only roughness (glTF ORM G) changes: G' = min(255, round(1.25 G)). AO (R),
metallic (B), dimensions, and the original source file remain unchanged.
"""
from pathlib import Path
import hashlib
import json
import numpy as np
from PIL import Image

ROOT = Path(__file__).resolve().parents[2]
ART = ROOT / "art/forest_preview"
SOURCE = ROOT / "target/world-preview/cc0-sources/tree_small_02/textures/tree_small_02_arm_1k.jpg"
OUTPUT = ART / "cc0_prepared/tree_small_02_trunk_arm_matte_1k.png"
REPORT = ART / "matte_bark_map.json"
DOWNLOADS = json.loads((ART / "cc0_tree_small_02_downloads.json").read_text(encoding="utf-8"))
entry = next(row for row in DOWNLOADS["files"]
             if row["path"].replace("\\", "/").endswith("tree_small_02_arm_1k.jpg"))
assert hashlib.sha256(SOURCE.read_bytes()).hexdigest() == entry["sha256"]
src = np.asarray(Image.open(SOURCE).convert("RGB"), dtype=np.uint8)
out = src.copy()
out[..., 1] = np.minimum(255, np.floor(src[..., 1].astype(np.float32) * 1.25 + 0.5)).astype(np.uint8)
assert np.array_equal(src[..., 0], out[..., 0])
assert np.array_equal(src[..., 2], out[..., 2])
OUTPUT.parent.mkdir(parents=True, exist_ok=True)
Image.fromarray(out, mode="RGB").save(OUTPUT, format="PNG", compress_level=9)
report = {
    "schema": "warbell.forest_matte_bark_arm.v1",
    "source": str(SOURCE.relative_to(ROOT)).replace("\\", "/"),
    "source_sha256": entry["sha256"],
    "source_url": entry["url"],
    "license": "CC0-1.0",
    "output": str(OUTPUT.relative_to(ROOT)).replace("\\", "/"),
    "output_sha256": hashlib.sha256(OUTPUT.read_bytes()).hexdigest(),
    "dimensions": list(Image.open(OUTPUT).size),
    "roughness_multiplier": 1.25,
    "roughness_median_before": round(float(np.median(src[..., 1]) / 255), 5),
    "roughness_median_after": round(float(np.median(out[..., 1]) / 255), 5),
    "ao_red_unchanged": True,
    "metallic_blue_unchanged": True,
}
REPORT.write_text(json.dumps(report, indent=2), encoding="utf-8")
print(json.dumps(report, sort_keys=True))
