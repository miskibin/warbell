"""Author the single shared GLB kit and per-model Bevy mesh JSON through Blender MCP."""
from pathlib import Path
import sys
import bpy
import importlib

HERE=Path(__file__).resolve().parent
if str(HERE) not in sys.path: sys.path.append(str(HERE))
import envkit
import architecture
import nature
import props
import world_structures
import biome_extras
importlib.reload(envkit)
importlib.reload(architecture)
importlib.reload(nature)
importlib.reload(props)
importlib.reload(world_structures)
importlib.reload(biome_extras)
from envkit import export
from architecture import fort_variants, town_structures
from nature import nature_models
from props import prop_models
from world_structures import world_models
from biome_extras import biome_extra_models

bpy.ops.object.select_all(action="SELECT")
bpy.ops.object.delete(use_global=False)
models=fort_variants()+town_structures()+nature_models()+prop_models()+world_models()+biome_extra_models()
seen=set()
for model in models:
    if model.name in seen: raise ValueError("Duplicate environment model: "+model.name)
    seen.add(model.name)
export(models)
