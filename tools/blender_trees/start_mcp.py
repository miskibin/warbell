"""Private Warbell Blender MCP add-on on 127.0.0.1:9877."""
import sys, os
from pathlib import Path
import bpy

addon_dir = os.environ.get("BLENDER_MCP_ADDON_DIR")
if not addon_dir:
    raise RuntimeError("Set BLENDER_MCP_ADDON_DIR to the folder containing blender_mcp_addon.py.")
sys.path.append(addon_dir)
# Append external dependencies so Blender keeps its own ABI-compatible NumPy first.
if packages := os.environ.get("BLENDER_MCP_SITE_PACKAGES"):
    sys.path.append(packages)
import blender_mcp_addon as addon

addon.register()
bpy.context.scene.blendermcp_port = 9877
bpy.context.scene.blendermcp_auto_start_server = False
server = addon.BlenderMCPServer(host="127.0.0.1", port=9877)
bpy.types.blendermcp_server = server
server.start()
print("WARBELL_BLENDER_MCP_READY_9877",flush=True)
