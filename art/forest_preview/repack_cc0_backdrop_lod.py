"""Convert the one new Blender GLB's embedded JPEGs to PNG losslessly.

Keeps the existing ten-model conversion ledger intact. Warbell's Bevy build
decodes PNG GLB images, while Blender's glTF exporter retains source JPEGs.
"""
from pathlib import Path
import importlib.util
import json

ROOT = Path(__file__).resolve().parents[2]
TOOL = ROOT / "tools/transcode_forest_glb_textures.py"
spec = importlib.util.spec_from_file_location("warbell_glb_png_repack", TOOL)
module = importlib.util.module_from_spec(spec)
assert spec.loader is not None
spec.loader.exec_module(module)

file = ROOT / "assets/models/forest_slice/cc0/tree_small_02_backdrop.glb"
report = ROOT / "art/forest_preview/cc0_png_repack.json"
existing = json.loads(report.read_text(encoding="utf-8"))
converted = module.repack(file)
if converted is None:
    raise ValueError("Expected Blender-exported source JPEGs before PNG repack")
models = [row for row in existing["models"] if row["file"] != converted["file"]]
models.append(converted)
existing["models"] = sorted(models, key=lambda row: row["file"])
report.write_text(json.dumps(existing, indent=2), encoding="utf-8")
print(json.dumps({"file": converted["file"],
                  "images_converted": len(converted["images"]),
                  "geometry_unchanged": converted["geometry_unchanged"],
                  "ledger_models": len(existing["models"])}))
