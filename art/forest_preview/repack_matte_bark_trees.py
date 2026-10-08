"""PNG-repack the two material-corrected Blender tree exports and update ledger."""
from pathlib import Path
import importlib.util
import json

ROOT = Path(__file__).resolve().parents[2]
spec = importlib.util.spec_from_file_location(
    "warbell_glb_png_repack", ROOT / "tools/transcode_forest_glb_textures.py")
module = importlib.util.module_from_spec(spec)
assert spec.loader is not None
spec.loader.exec_module(module)

report = ROOT / "art/forest_preview/cc0_png_repack.json"
ledger = json.loads(report.read_text(encoding="utf-8"))
models = {row["file"]: row for row in ledger["models"]}
for name in ("tree_small_02_optimized", "tree_small_02_backdrop"):
    path = ROOT / "assets/models/forest_slice/cc0" / (name + ".glb")
    converted = module.repack(path)
    assert converted is not None, f"Expected Blender JPEG sources: {name}"
    assert converted["geometry_unchanged"] and converted["images"]
    models[converted["file"]] = converted
    print(name, len(converted["images"]), converted["output_sha256"])
ledger["models"] = [models[key] for key in sorted(models)]
assert len(ledger["models"]) == 11
report.write_text(json.dumps(ledger, indent=2), encoding="utf-8")
