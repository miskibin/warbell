"""Read-only list of tree mesh IDs available for linked composition source."""
from pathlib import Path
import bpy

ROOT=Path(__file__).resolve().parents[2]
for name in ("tree_small_02_optimized","island_tree_01_optimized"):
    path=ROOT/f"art/forest_preview/{name}.blend"
    with bpy.data.libraries.load(str(path),link=True) as (source,target):
        print("CC0_LIBRARY",name,"meshes",source.meshes,"objects",source.objects,
              "collections",source.collections)
