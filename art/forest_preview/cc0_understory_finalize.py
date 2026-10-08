"""Finalize Blender MCP understory exports for the game's PNG-only GLB loader.

Run after ``cc0_understory_build.py`` finishes on isolated Blender port 9878.
The shared transcoder is idempotent and preserves decoded pixels and geometry.
"""

import hashlib
import json
from pathlib import Path
import struct
import subprocess
import sys


ROOT = Path(__file__).resolve().parents[2]
MANIFEST = ROOT / "art/forest_preview/cc0_understory_exports.json"
TRANSCODER = ROOT / "tools/transcode_forest_glb_textures.py"


def glb_images(path: Path) -> list[str]:
    raw = path.read_bytes()
    json_size, kind = struct.unpack_from("<I4s", raw, 12)
    assert kind == b"JSON", path
    document = json.loads(raw[20 : 20 + json_size])
    return [image["mimeType"] for image in document.get("images", [])]


def main() -> None:
    subprocess.run([sys.executable, str(TRANSCODER)], check=True, cwd=ROOT)
    manifest = json.loads(MANIFEST.read_text(encoding="utf-8"))
    for model in manifest["models"]:
        path = ROOT / model["path"]
        images = glb_images(path)
        assert images and all(mime == "image/png" for mime in images), (path, images)
        new_hash = hashlib.sha256(path.read_bytes()).hexdigest()
        if new_hash != model["sha256"]:
            model.setdefault("blender_export_sha256", model["sha256"])
            model["sha256"] = new_hash
        model["bytes"] = path.stat().st_size
        model["embedded_images_png_only"] = True
    manifest["postprocess"] = "tools/transcode_forest_glb_textures.py"
    MANIFEST.write_text(json.dumps(manifest, indent=2) + "\n", encoding="utf-8")
    print(f"Finalized {len(manifest['models'])} PNG-only understory GLBs")


if __name__ == "__main__":
    main()
