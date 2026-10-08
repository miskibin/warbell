"""Combine Poly Haven's 1K JPG diffuse with its separate 16-bit alpha mask.

The official 1K glTF names MASK/BLEND but references an opaque JPG diffuse, so
raw imports would produce rectangular cards. RGB stays the decoded source JPG;
only the PNG's 16-bit alpha is quantized to 8-bit for glTF baseColor RGBA.
"""

from hashlib import sha256
from json import dumps
from pathlib import Path

import numpy as np
from PIL import Image


ROOT = Path(__file__).resolve().parents[2]
SOURCES = ROOT / "target/world-preview/cc0-sources"
REPORT = ROOT / "art/forest_preview/cc0_understory_rgba.json"
ASSETS = ("shrub_03", "grass_medium_01", "dandelion_01", "flower_heliophila")


def main() -> None:
    items = []
    for asset in ASSETS:
        folder = SOURCES / asset / "textures"
        diffuse = folder / f"{asset}_diff_1k.jpg"
        alpha = folder / f"{asset}_alpha_1k.png"
        output = folder / f"{asset}_diff_alpha_1k.png"
        rgb = np.asarray(Image.open(diffuse).convert("RGB"), dtype=np.uint8)
        mask16 = np.asarray(Image.open(alpha), dtype=np.uint16)
        if rgb.shape[:2] != mask16.shape:
            raise ValueError(f"{asset}: RGB/alpha dimensions differ")
        mask = (mask16 // 257).astype(np.uint8)
        rgba = np.dstack((rgb, mask))
        Image.fromarray(rgba, "RGBA").save(output, optimize=True)
        if not np.array_equal(np.asarray(Image.open(output))[:, :, :3], rgb):
            raise ValueError(f"{asset}: RGB changed in composite")
        items.append(
            {
                "asset": asset,
                "diffuse_source": diffuse.relative_to(ROOT).as_posix(),
                "alpha_source": alpha.relative_to(ROOT).as_posix(),
                "rgba_output": output.relative_to(ROOT).as_posix(),
                "rgba_sha256": sha256(output.read_bytes()).hexdigest(),
                "alpha_min_max": [int(mask.min()), int(mask.max())],
                "alpha_transparent_fraction": round(float(np.mean(mask < 128)), 6),
            }
        )
    REPORT.write_text(
        dumps({"schema": "warbell.forest_cc0_rgba_derivatives.v1", "items": items}, indent=2)
        + "\n",
        encoding="utf-8",
    )
    print(dumps({item["asset"]: item["alpha_transparent_fraction"] for item in items}))


if __name__ == "__main__":
    main()
