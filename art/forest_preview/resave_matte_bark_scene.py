"""Resave the approval scene after its packed linked tree libraries changed."""
from pathlib import Path
import bpy
import hashlib
import json

ROOT=Path(__file__).resolve().parents[2]
SCENE=ROOT/"art/forest_preview/forest_scene.blend"
assert Path(bpy.data.filepath).resolve()==SCENE.resolve()
linked={Path(lib.filepath).name:lib for lib in bpy.data.libraries}
assert all(name in linked for name in (
    "tree_small_02_optimized.blend","tree_small_02_backdrop.blend",
    "island_tree_01_optimized.blend"))
assert all(lib.filepath.startswith("//") for lib in linked.values())
bpy.ops.file.make_paths_relative()
bpy.context.preferences.filepaths.save_version=0
bpy.ops.wm.save_as_mainfile(filepath=str(SCENE),compress=True)
print("WARBELL_MATTE_BARK_SCENE",json.dumps({
    "sha256":hashlib.sha256(SCENE.read_bytes()).hexdigest(),
    "bytes":SCENE.stat().st_size,
    "linked_libraries":sorted(linked),
},sort_keys=True))
