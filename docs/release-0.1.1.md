# Agent QA Harness v0.1.1

Installed-client validation and interoperability fixes.

- Real model-driven Codex, Claude Code and Cursor runs completed exploration, defect discovery, separate-run reproduction, checkpointing and JSON/Markdown report export against the same black-box local fixture.
- Three independent confirmed findings and 144 total artifacts; every artifact hash verified. The fixture intentionally contains a persistence defect; successful QA execution does not mean the application passed quality checks.
- Added readonly MCP annotations to list/get/event tools, plus protocol-level regression coverage.
- Added a native Codex repository marketplace entry, a repeatable client acceptance fixture/runner and a separate evidence verifier.
- Documented Cursor's distinction between MCP server trust and tool approval, its required deny array, and exact workspace-scoped permissions.

Antigravity is installed and discovers the Skill/MCP schemas. Its full flow remains incomplete because noninteractive tool permission defaulted to denial and the interactive first-run setup required action approval. No global permission bypass was applied. Full acceptance on all four clients is not claimed.

The deterministic regression suite still passes 20 tests. See docs/client-acceptance-2026-11-04.md for scope and limitations. Raw model transcripts/authentication metadata are not included in the release.
