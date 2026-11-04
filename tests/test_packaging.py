import json
import subprocess
import sys
import tomllib
from pathlib import Path

import yaml

from agent_qa.install import configuration

ROOT = Path(__file__).resolve().parents[1]


def test_client_configs_parse_and_preserve_workspace(tmp_path):
    for client in ("codex", "claude", "cursor", "antigravity"):
        text = configuration(client, tmp_path, ROOT)
        parsed = tomllib.loads(text) if client == "codex" else json.loads(text)
        entry = parsed["mcp_servers" if client == "codex" else "mcpServers"]["agent-qa"]
        assert entry["command"] == sys.executable
        assert entry["args"][-1] == str(tmp_path.resolve())


def test_skills_have_valid_frontmatter_and_references():
    skills = list((ROOT / "skills").glob("*/SKILL.md"))
    assert len(skills) == 4
    for path in skills:
        frontmatter = yaml.safe_load(path.read_text(encoding="utf-8").split("---", 2)[1])
        assert frontmatter["name"] == path.parent.name
        assert frontmatter["description"]


def test_native_bundles_have_only_their_own_config():
    result = subprocess.run(
        [sys.executable, "scripts/package_plugins.py"], cwd=ROOT, check=True, capture_output=True, text=True
    )
    bundles = json.loads(result.stdout)
    configs = {
        "codex": ".mcp.json",
        "claude": ".mcp.json",
        "cursor": "mcp.json",
        "antigravity": "mcp_config.json",
    }
    for client, directory in bundles.items():
        path = Path(directory)
        assert len(list((path / "skills").glob("*/SKILL.md"))) == 4
        assert (path / configs[client]).is_file()
        for other_config in set(configs.values()) - {configs[client]}:
            assert not (path / other_config).exists()
