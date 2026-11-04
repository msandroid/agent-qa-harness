# Validation

The tests exercise behavior rather than merely matching documentation:

- Real MCP stdio initialization, tool discovery, resource reading and job workflow.
- Real local Chromium input, a deliberately broken save/reload flow, screenshots, independent-run reproduction and report export.
- Cross-job/run evidence rejection and stored-artifact integrity checks.
- Concurrent step reservations, budget limits, pauses, cost checks and checkpoint persistence.
- Interrupted-action recovery without replay, explicit coverage gaps and evidence-required outcomes.
- Plugin packaging and configuration parsing.

Run with `uv run pytest -q`; the browser test requires `uv run playwright install chromium`.

CI runs the suite on Windows and Linux with Python 3.11 and 3.13. A workflow definition is not evidence of a completed remote CI run; inspect the repository's Actions results.

Native client validation and local acceptance results are recorded in the release notes after execution. Codex/Cursor/Claude/Antigravity model-driven end-to-end acceptance is a separate check from a successful MCP SDK client test or manifest validation.

## Local acceptance — 2026-11-04

- Windows, Python 3.14.6, MCP Python SDK 1.30.0, Playwright 1.64.1.
- `pytest -q`: **20 passed**, including real Chromium and a separate MCP stdio process.
- The same **20 tests passed on Windows/Python 3.11 and 3.13** in separate isolated environments.
- Ruff lint and format checks passed.
- `claude plugin validate dist/plugins/claude`: passed.
- `agy plugin validate --strict dist/plugins/antigravity`: passed, with the installed CLI placed on the validator's PATH.
- Codex and Cursor configs/manifests are included and packaging/config parsing is tested. Native client installation and model-driven QA sessions in those clients were not executed during this acceptance run.
- No paid-model API calls were made. A deterministic browser scenario validates the runtime; it is not a measurement of autonomous bug-finding ability.

Subsequently, real authenticated client/model runs were performed. Codex, Claude Code and Cursor completed the full installed-plugin QA flow; Antigravity installation succeeded but its interactive permission/setup boundary remains unresolved. See [the detailed acceptance record](client-acceptance-2026-11-04.md). The earlier no-model statement applies only to the deterministic unit/integration suite above.

## Remote CI limitation

The first GitHub Actions run was blocked before any test step executed. GitHub reported: "The job was not started because your account is locked due to a billing issue." This is not a test failure observed in the code; remote Windows/Linux acceptance remains unverified until the account issue is resolved. [Initial run](https://github.com/msandroid/agent-qa-harness/actions/runs/41796297785).
