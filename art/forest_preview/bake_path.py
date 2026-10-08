"""Bake slice-only path PBR tiles from the Poly Haven stony dirt CC0 scan.

Run through the official Blender MCP bridge. The campaign path atlas is not
modified. Each source channel remains aligned through the same resize and
periodic border repair; only albedo receives a neutral exposure adjustment.
"""

from pathlib import Path
import hashlib
import json

import bpy
import numpy as np


ROOT = Path(__file__).resolve().parents[2]
SOURCE_DIR = ROOT / "art/blender_environment/polyhaven/stony_dirt_path"
OUT = ROOT / "assets/models/forest_slice"
OUT.mkdir(parents=True, exist_ok=True)
SIZE = 512
SOURCE_FILES = {
    "albedo": "stony_dirt_path_diff_1k.png",
    "normal": "stony_dirt_path_nor_gl_1k.png",
    "roughness": "stony_dirt_path_rough_1k.png",
}


def read_raw_rgb(name):
    source = SOURCE_DIR / name
    image = bpy.data.images.load(str(source), check_existing=False)
    # Read PNG bytes as authored: Blender's display transform is not baked in.
    image.colorspace_settings.name = "Non-Color"
    image.scale(SIZE, SIZE)
    values = np.empty(SIZE * SIZE * 4, dtype=np.float32)
    image.pixels.foreach_get(values)
    bpy.data.images.remove(image)
    return np.flipud(values.reshape(SIZE, SIZE, 4))[..., :3].copy()


def make_periodic(rgb):
    # The original scan is tileable in appearance; blend only the outer 12 px
    # so bilinear and mip sampling cannot expose a one-texel seam.
    for axis in (1, 0):
        for k in range(12):
            weight = 0.5 * (1.0 - k / 12.0)
            first_idx = [slice(None)] * 3
            last_idx = [slice(None)] * 3
            first_idx[axis] = k
            last_idx[axis] = -1 - k
            first = rgb[tuple(first_idx)].copy()
            last = rgb[tuple(last_idx)].copy()
            rgb[tuple(first_idx)] = first + (last - first) * weight
            rgb[tuple(last_idx)] = last + (first - last) * weight
    return rgb


def save(name, rgb, colorspace):
    rgba = np.ones((SIZE, SIZE, 4), dtype=np.float32)
    rgba[..., :3] = np.clip(rgb, 0.0, 1.0)
    image = bpy.data.images.new(name, width=SIZE, height=SIZE, alpha=True)
    image.colorspace_settings.name = colorspace
    image.pixels.foreach_set(np.flipud(rgba).ravel())
    image.update()
    image.filepath_raw = str(OUT / name)
    image.file_format = "PNG"
    image.save()
    bpy.data.images.remove(image)


albedo = make_periodic(read_raw_rgb(SOURCE_FILES["albedo"]))


def smoothstep(value, lo, hi):
    t = np.clip((value - lo) / (hi - lo), 0.0, 1.0)
    return t * t * (3.0 - 2.0 * t)


# The scan contains a few saturated rust-orange mineral flakes. At ground-level
# game scale those read as painted orange chips. Reduce chroma only on the warm,
# bright flakes; preserve the original value detail and the rest of the scan.
red, green, blue = np.moveaxis(albedo, 2, 0)
rust = (
    smoothstep(red - 1.25 * green, 0.02, 0.11)
    * smoothstep(green - 1.20 * blue, 0.008, 0.068)
    * smoothstep(red, 0.21, 0.375)
)
luma = 0.2126 * red + 0.7152 * green + 0.0722 * blue
neutral_mineral = np.stack((1.10 * luma, luma, 0.92 * luma), axis=2)
albedo = albedo * (1.0 - 0.88 * rust[..., None]) + neutral_mineral * (0.88 * rust[..., None])
# A neutral lift keeps the brown soil readable in afternoon light without
# reintroducing an ochre hue cast.
albedo = np.clip(albedo * 1.18, 0.0, 1.0)
save("ground_path_albedo.png", albedo, "sRGB")

normal = make_periodic(read_raw_rgb(SOURCE_FILES["normal"])) * 2.0 - 1.0
normal /= np.maximum(np.linalg.norm(normal, axis=2, keepdims=True), 1e-6)
save("ground_path_normal.png", normal * 0.5 + 0.5, "Non-Color")

roughness = make_periodic(read_raw_rgb(SOURCE_FILES["roughness"]))
save("ground_path_roughness.png", roughness, "Non-Color")

output_hashes = {
    path.name: hashlib.sha256(path.read_bytes()).hexdigest()
    for path in sorted(OUT.glob("ground_path_*.png"))
}
report = {
    "schema": "warbell.forest_slice_path.v2",
    "source_page": "https://polyhaven.com/a/stony_dirt_path",
    "license": "CC0-1.0",
    "source_files": {
        channel: {
            "path": str((SOURCE_DIR / name).relative_to(ROOT)).replace("\\", "/"),
            "sha256": hashlib.sha256((SOURCE_DIR / name).read_bytes()).hexdigest(),
        }
        for channel, name in SOURCE_FILES.items()
    },
    "method": "Blender raw-channel import, 1k to 512 resize, 12px periodic seam, local rust-fleck chroma reduction (red-green excess mask; 0.88 strength), neutral 1.18 albedo exposure, source OpenGL normal and roughness",
    "rust_fleck_mask_fraction_over_0_2": float(np.mean(rust > 0.2)),
    "mean_rgb_255": [float(value) for value in albedo.mean((0, 1)) * 255.0],
    "file_sha256": output_hashes,
}
(ROOT / "art/forest_preview/path_report.json").write_text(
    json.dumps(report, indent=2) + "\n", encoding="utf-8"
)
print("WARBELL_FOREST_PATH", json.dumps(report), flush=True)
