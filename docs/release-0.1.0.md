# Agent QA Harness v0.1.0

Initial public release: a local-first QA plugin for existing frontier-model coding clients.

- 18 stdio MCP tools, a workflow resource and an exploration prompt.
- Four portable Skills: explore, reproduce, adversarial review, resume/report.
- Client-specific bundles for Codex, Claude Code, Cursor and Antigravity.
- SQLite contracts/checkpoints/action receipts; atomic recorded-operation budgets.
- Hashed evidence with job/run isolation checks, reproduction tracking and JSON/Markdown reports.
- Optional Chromium adapter with isolated contexts and evidence capture.
- External evidence workflow for the host's existing game/mobile/desktop/software tools.

Validation: 20 local tests passed on each of Windows/Python 3.11, 3.13 and 3.14, including real Chromium and MCP stdio. Claude plugin validation and Antigravity strict validation passed. GitHub Actions was blocked before execution by an account billing lock; Linux and remote CI acceptance are not verified. Native model-driven end-to-end acceptance across all four clients is not claimed.

Install the runtime before loading the plugin bundle; see README. No additional model API key is required. This release does not include a hosted tenant service, device/GPU fleet or engine-specific gameplay controller.
