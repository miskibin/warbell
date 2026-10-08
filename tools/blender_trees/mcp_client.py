"""Auditable official MCP execute_blender_code client for this tree experiment."""
import asyncio, json, os, sys, shutil
from pathlib import Path
from datetime import timedelta
from mcp import ClientSession, StdioServerParameters
from mcp.client.stdio import stdio_client

HERE=Path(__file__).resolve().parent
server_candidate = Path(sys.executable).with_name("mcp-for-blender.exe" if os.name == "nt" else "mcp-for-blender")
SERVER = os.environ.get("BLENDER_MCP_COMMAND") or (
    str(server_candidate) if server_candidate.is_file() else shutil.which("mcp-for-blender"))
if not SERVER:
    raise SystemExit("Install mcp-for-blender in this Python environment or set BLENDER_MCP_COMMAND.")
source=Path(sys.argv[1]).resolve()

async def main():
    params=StdioServerParameters(
        command=SERVER,args=[],
        env={**os.environ,"BLENDER_HOST":"127.0.0.1","BLENDER_PORT":"9877","BLENDER_MCP_DISABLE_TELEMETRY":"1"})
    async with stdio_client(params) as (read,write):
        async with ClientSession(read,write,read_timeout_seconds=timedelta(minutes=30)) as session:
            await session.initialize()
            code=f"__file__={str(source)!r}\n"+source.read_text(encoding="utf-8")
            result=await session.call_tool("execute_blender_code",{"code":code,"user_prompt":"Author natural textured trees for Warbell in isolated Blender via MCP."},read_timeout_seconds=timedelta(minutes=30))
            record={"transport":"official mcp-for-blender -> Blender add-on 127.0.0.1:9877","tool":"execute_blender_code","source":str(source),"result":result.model_dump(mode="json")}
            with (HERE/"mcp_calls.jsonl").open("a",encoding="utf-8") as f: f.write(json.dumps(record,ensure_ascii=False)+"\n")
            for c in result.content:
                if hasattr(c,"text"): print(c.text)
            if result.isError or any("Error executing code:" in getattr(c,"text","") for c in result.content): raise SystemExit(1)

asyncio.run(main())
