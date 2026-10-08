"""Audited official execute_blender_code bridge to this task's isolated Blender."""
import asyncio
import json
import os
from pathlib import Path
import shutil
import sys
from datetime import timedelta
from mcp import ClientSession, StdioServerParameters
from mcp.client.stdio import stdio_client

HERE=Path(__file__).resolve().parent
candidate=Path(sys.executable).with_name("mcp-for-blender.exe" if os.name=="nt" else "mcp-for-blender")
SERVER=os.environ.get("BLENDER_MCP_COMMAND") or (
    str(candidate) if candidate.is_file() else shutil.which("mcp-for-blender"))
if not SERVER:
    raise SystemExit("mcp-for-blender is required; set BLENDER_MCP_COMMAND")
SOURCE=Path(sys.argv[1]).resolve()

async def main():
    params=StdioServerParameters(command=SERVER,args=[],env={
        **os.environ,"BLENDER_HOST":"127.0.0.1","BLENDER_PORT":"9877",
        "BLENDER_MCP_DISABLE_TELEMETRY":"1"})
    async with stdio_client(params) as (read,write):
        async with ClientSession(read,write,
                                 read_timeout_seconds=timedelta(minutes=40)) as session:
            await session.initialize()
            code=f"__file__={str(SOURCE)!r}\n"+SOURCE.read_text(encoding="utf-8")
            result=await session.call_tool(
                "execute_blender_code",
                {"code":code,"user_prompt":"Author original textured medieval campaign environment assets for Warbell inside isolated Blender."},
                read_timeout_seconds=timedelta(minutes=40))
            record={"transport":"official mcp-for-blender -> Blender add-on 127.0.0.1:9877",
                    "tool":"execute_blender_code","source":str(SOURCE),
                    "source_sha256":__import__("hashlib").sha256(SOURCE.read_bytes()).hexdigest(),
                    "result":result.model_dump(mode="json")}
            with (HERE/"mcp_calls.jsonl").open("a",encoding="utf-8") as f:
                f.write(json.dumps(record,ensure_ascii=False)+"\n")
            for c in result.content:
                if hasattr(c,"text"): print(c.text)
            if result.isError or any("Error executing code:" in getattr(c,"text","") for c in result.content):
                raise SystemExit(1)

asyncio.run(main())
