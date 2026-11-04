# Client setup

All four clients use the same local stdio MCP server and the same SKILL.md folders. Model choice and subscription stay with the client. No model endpoint is configured here.

## Reliable explicit setup

Run `uv sync --locked --extra browser --extra dev` from the checkout, then:

```sh
uv run agent-qa config --client codex --workspace /absolute/test/workspace
```

Change the client flag as needed. The generated snippet uses this environment's absolute Python executable and an explicit workspace. Merge only the named server entry; preserve existing settings. Never commit a real client's credentials or private workspace configuration to this public repository.

| Client | MCP configuration | Workspace Skills fallback |
|---|---|---|
| Codex | Merge the printed TOML server entry into the selected Codex config | `.agents/skills/<skill-name>/SKILL.md` |
| Claude Code | Merge JSON `mcpServers.agent-qa` into project `.mcp.json` or selected user config | `.claude/skills/<skill-name>/SKILL.md` |
| Cursor | Merge JSON server entry into project `.cursor/mcp.json` or selected user config | `.cursor/skills/<skill-name>/SKILL.md` |
| Antigravity | Merge JSON server entry using the MCP configuration UI for the installed surface/version | `.agents/skills/<skill-name>/SKILL.md` |

Copy each whole skill folder, including references. Do not copy it twice when native plugin discovery already loads it.

After connecting, list tools and read `qa://workflow`. Create a small job and check its `workspace` path before importing artifacts. If the client cannot find Python/CLI, regenerate the explicit config instead of guessing PATH behavior.

## Native plugin bundles

Install the runtime on PATH first:

```sh
uv tool install --from ".[browser]" agent-qa-harness
uv run python scripts/package_plugins.py
```

The packager creates one directory per client. This avoids making another client's validator interpret unrelated manifests/configs. Native launchers run `agent-qa serve`; their workspace comes from `QA_WORKSPACE` or the inherited working directory. Use the explicit setup above when the client launches in a different directory.

- **Claude Code**: the repository includes a marketplace manifest. Add the repository as a marketplace and install `agent-qa@agent-qa-marketplace` through Claude's plugin manager. For local testing, use `claude --plugin-dir /absolute/path/to/dist/plugins/claude`. Validate the plugin with `claude plugin validate /absolute/path/to/dist/plugins/claude`.
- **Codex**: the `dist/plugins/codex` bundle has `.codex-plugin/plugin.json`, `.mcp.json` and Skills. Use a local/plugin source mechanism supported by your installed Codex surface; the explicit MCP+Skills setup remains available. This repository is not automatically an approved marketplace listing.
- **Cursor**: the `dist/plugins/cursor` bundle has `.cursor-plugin/plugin.json`, `mcp.json` and Skills. Load it through the local plugin mechanism supported by your installed version, or use the explicit MCP+Skills setup. Marketplace publication is a separate review.
- **Antigravity**: load `dist/plugins/antigravity` as a local plugin, or place it under the workspace `.agents/plugins/agent-qa/` if supported by your IDE surface. It contains `.antigravity-plugin/plugin.json`, `mcp_config.json` and Skills. Validate with `agy plugin validate --strict /absolute/path/to/dist/plugins/antigravity`.

Client APIs and plugin discovery can change. Included manifests and schema validation are not proof that every model/client combination autonomously completes QA. See [validation status](validation.md).

## Sources for packaging

- [Claude plugin reference](https://code.claude.com/docs/en/plugins-reference)
- [Cursor plugin reference](https://cursor.com/docs/reference/plugins)
- [Antigravity build plugins](https://antigravity.google/docs/build-plugins)
- Codex manifest fields were also checked against installed first-party/curated plugin manifests on the development host.
