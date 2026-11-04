"""Probe the installed CLI through real MCP stdio, without a model call."""

import argparse
import asyncio
import json
import shutil
import subprocess
from pathlib import Path

from mcp import ClientSession, StdioServerParameters
from mcp.client.stdio import stdio_client


async def probe(workspace: Path):
    workspace = workspace.resolve()
    workspace.mkdir(parents=True, exist_ok=True)
    executable = shutil.which("agent-qa")
    if not executable:
        raise RuntimeError("agent-qa is not installed on PATH")
    version = subprocess.check_output([executable, "--version"], text=True).strip()
    params = StdioServerParameters(command=executable, args=["serve", "--workspace", str(workspace)])
    async with stdio_client(params) as (reader, writer):
        async with ClientSession(reader, writer) as session:
            await session.initialize()
            tools = await session.list_tools()
            names = {tool.name: tool for tool in tools.tools}
            readonly = {
                name: names[name].annotations.readOnlyHint
                for name in ("qa_list_jobs", "qa_get_job", "qa_get_events")
            }
            if not all(readonly.values()):
                raise RuntimeError("Installed runtime is missing readonly annotations")
            result = await session.call_tool("qa_list_jobs", {})
            if result.isError:
                raise RuntimeError("Installed runtime failed its read-only probe")
            return {
                "installed_version": version,
                "tool_count": len(names),
                "readonly": readonly,
                "stdio_probe": "passed",
            }


if __name__ == "__main__":
    parser = argparse.ArgumentParser()
    parser.add_argument("--workspace", type=Path, required=True)
    args = parser.parse_args()
    print(json.dumps(asyncio.run(probe(args.workspace)), indent=2))
