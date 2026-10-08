"""Isolated official Blender MCP add-on for CC0 non-tree adaptation (port 9878)."""

import os
import sys

import bpy


addon_dir = os.environ.get("BLENDER_MCP_ADDON_DIR")
if not addon_dir:
    raise RuntimeError("Set BLENDER_MCP_ADDON_DIR to the official add-on directory")
sys.path.append(addon_dir)
if packages := os.environ.get("BLENDER_MCP_SITE_PACKAGES"):
    sys.path.append(packages)
import blender_mcp_addon as addon


addon.register()
bpy.context.scene.blendermcp_port = 9878
bpy.context.scene.blendermcp_auto_start_server = False
server = addon.BlenderMCPServer(host="127.0.0.1", port=9878)
bpy.types.blendermcp_server = server
server.start()
print("WARBELL_CC0_UNDERSTORY_MCP_READY_9878", flush=True)
