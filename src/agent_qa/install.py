"""Explicit generated configs use absolute paths; no host-variable guessing."""

import json
import sys
from pathlib import Path


def configuration(client: str, workspace: Path, project: Path) -> str:
    root = str(workspace.resolve(strict=True))
    args = ["-m", "agent_qa.cli", "serve", "--workspace", root]
    if client == "codex":
        return (
            "[mcp_servers.agent-qa]\ncommand = "
            + json.dumps(sys.executable)
            + "\nargs = "
            + json.dumps(args)
            + "\n"
        )
    return json.dumps({"mcpServers": {"agent-qa": {"command": sys.executable, "args": args}}}, indent=2)
