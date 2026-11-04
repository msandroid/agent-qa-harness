"""Produce client-specific local plugin bundles; never modifies installed clients."""

import json
import shutil
import tomllib
import zipfile
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
VERSION = tomllib.loads((ROOT / "pyproject.toml").read_text(encoding="utf-8"))["project"]["version"]
MANIFESTS = {
    "codex": ".codex-plugin",
    "claude": ".claude-plugin",
    "cursor": ".cursor-plugin",
    "antigravity": ".antigravity-plugin",
}
CONFIGS = {
    "codex": ".mcp.json",
    "claude": ".mcp.json",
    "cursor": "mcp.json",
    "antigravity": "mcp_config.json",
}


def package(output: Path):
    output = output.resolve()
    if not output.is_relative_to(ROOT / "dist"):
        raise ValueError("Generated bundles must stay inside this checkout's dist directory")
    for client, manifest_dir in MANIFESTS.items():
        dest = output / client
        dest.mkdir(parents=True, exist_ok=True)
        for folder in ("skills", "assets"):
            shutil.copytree(ROOT / folder, dest / folder, dirs_exist_ok=True)
        (dest / manifest_dir).mkdir(exist_ok=True)
        shutil.copy2(ROOT / manifest_dir / "plugin.json", dest / manifest_dir / "plugin.json")
        for name in (CONFIGS[client], "README.md", "LICENSE"):
            shutil.copy2(ROOT / name, dest / name)
        # Include documentation used by the README links.
        shutil.copytree(ROOT / "docs", dest / "docs", dirs_exist_ok=True)
        shutil.copy2(ROOT / "README.ja.md", dest / "README.ja.md")
        archive = output.parent / f"agent-qa-{client}-{VERSION}.zip"
        with zipfile.ZipFile(archive, "w", compression=zipfile.ZIP_DEFLATED) as z:
            for file in sorted(dest.rglob("*")):
                if file.is_file():
                    z.write(file, (Path("agent-qa") / file.relative_to(dest)).as_posix())
    return {client: str(output / client) for client in MANIFESTS}


if __name__ == "__main__":
    print(json.dumps(package(ROOT / "dist" / "plugins"), indent=2))
