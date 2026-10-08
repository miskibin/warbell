"""Audited execute_blender_code bridge to this task's private Blender on 9878."""

import asyncio
from datetime import timedelta
import hashlib
import json
import os
from pathlib import Path
import shutil
import sys

from mcp import ClientSession, StdioServerParameters
from mcp.client.stdio import stdio_client


ROOT = Path(__file__).resolve().parents[2]
SOURCE = Path(sys.argv[1]).resolve()
LOG = ROOT / "target/world-preview/cc0-understory/mcp_calls.jsonl"
candidate = Path(sys.executable).with_name("mcp-for-blender.exe" if os.name == "nt" else "mcp-for-blender")
SERVER = os.environ.get("BLENDER_MCP_COMMAND") or (
    str(candidate) if candidate.is_file() else shutil.which("mcp-for-blender")
)
if not SERVER:
    raise SystemExit("mcp-for-blender is required; set BLENDER_MCP_COMMAND")


async def main() -> None:
    params = StdioServerParameters(
        command=SERVER,
        args=[],
        env={
            **os.environ,
            "BLENDER_HOST": "127.0.0.1",
            "BLENDER_PORT": "9878",
            "BLENDER_MCP_DISABLE_TELEMETRY": "1",
        },
    )
    async with stdio_client(params) as (read, write):
        async with ClientSession(
            read, write, read_timeout_seconds=timedelta(minutes=40)
        ) as session:
            await session.initialize()
            code = f"__file__={str(SOURCE)!r}\n" + SOURCE.read_text(encoding="utf-8")
            result = await session.call_tool(
                "execute_blender_code",
                {
                    "code": code,
                    "user_prompt": "Adapt audited Poly Haven CC0 forest understory sources into optimized Warbell GLBs in isolated Blender.",
                },
                read_timeout_seconds=timedelta(minutes=40),
            )
            LOG.parent.mkdir(parents=True, exist_ok=True)
            record = {
                "transport": "official mcp-for-blender -> Blender add-on 127.0.0.1:9878",
                "tool": "execute_blender_code",
                "source": str(SOURCE.relative_to(ROOT)),
                "source_sha256": hashlib.sha256(SOURCE.read_bytes()).hexdigest(),
                "result": result.model_dump(mode="json"),
            }
            with LOG.open("a", encoding="utf-8") as handle:
                handle.write(json.dumps(record, ensure_ascii=False) + "\n")
            for content in result.content:
                if hasattr(content, "text"):
                    print(content.text)
            if result.isError or any(
                "Error executing code:" in getattr(content, "text", "")
                for content in result.content
            ):
                raise SystemExit(1)


asyncio.run(main())
