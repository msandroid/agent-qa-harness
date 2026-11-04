# Agent QA Harness

Evidence-first QA for **Codex, Claude Code, Google Antigravity and Cursor**. Bring the frontier model and application tools you already use. This plugin provides an MCP server and four portable Skills for exploratory testing, adversarial review, reproduction and resumable reporting.

**No separate model API key, proxy or hosted model service.** The host agent makes decisions. This local runtime stores contracts and evidence, enforces its own action budgets, and optionally drives Chromium.

[日本語](README.ja.md) · [Client setup](docs/clients.md) · [Architecture and trust boundaries](docs/architecture.md) · [Validation](docs/validation.md)

## What works in v0.1

- Stdio MCP: 18 tools, a workflow resource and an exploration prompt.
- Immutable job contracts: target build, criteria, scope, exact Web origins and budgets.
- SQLite ledger: action receipts, checkpoints, pause/resume, interrupted-action reconciliation.
- Evidence copies with SHA-256 integrity checks and job/run ownership checks.
- Findings start as candidates; separate reproduction runs produce confirmed/intermittent/not-reproduced states.
- Criteria cannot pass without evidence or while a reproduced defect remains on the same build.
- JSON/Markdown reports include evidence manifest, reproduction counts and coverage gaps.
- Optional Playwright browser: isolated context, origin-restricted HTTP requests, screenshots, visible text and bounded console/error logs.
- External-tool workflow for games, mobile, desktop, API and code tests: use the host's existing tools and import their evidence, explicitly labeled as operator-attested/imported.

This is a **local QA workflow plugin**, not a hosted multi-tenant testing fleet, an engine controller, a real-time gameplay policy or a certification of defect absence. Client manifests are included; see validation status before assuming every client has been tested end to end.

## Install the runtime

Python 3.11+ and [uv](https://docs.astral.sh/uv/) are required for these commands. Run them in a terminal with the same user account as your coding client.

```sh
git clone https://github.com/msandroid/agent-qa-harness.git
cd agent-qa-harness
uv sync --locked --extra browser --extra dev
uv run playwright install chromium
```

For native plugin manifests, install the CLI on your user's PATH:

```sh
uv tool install --from ".[browser]" agent-qa-harness
agent-qa --version
```

Restart the client if it has not picked up the updated PATH. The alternative below launches the project environment by absolute Python path and does not need a global CLI installation.

## Connect a client explicitly

Generate a config snippet for an existing **test workspace**, not your plugin checkout:

```sh
uv run agent-qa config --client codex --workspace /absolute/path/to/test-workspace
uv run agent-qa config --client claude --workspace /absolute/path/to/test-workspace
uv run agent-qa config --client cursor --workspace /absolute/path/to/test-workspace
uv run agent-qa config --client antigravity --workspace /absolute/path/to/test-workspace
```

On Windows, use a quoted path such as `"C:\Projects\MyApp"`. Merge the printed entry into the client's MCP settings. The command **prints only**; it does not overwrite settings. The generated absolute Python path is specific to this installation and must be regenerated if the environment moves.

Add the relevant `skills/<name>` folders through the client's plugin support, or copy them into that client's workspace Skills directory. See [exact client instructions](docs/clients.md), including native plugin bundles.

## Use it

Ask your agent:

> Use qa-explore to inspect this application's save/load flow on build abc123. Spend at most 60 recorded operations and 20 minutes. Preserve evidence, reproduce findings after reset, and report untested areas. Do not modify the application.

The expected sequence is:

1. `qa_create_job`: build, scope, criteria, origins, budgets.
2. Reset target with available tools; `qa_begin_run` records what was reset.
3. Browser adapter or existing app/game tools; record external operations with `qa_record_step`.
4. `qa_attach_artifact`, `qa_add_finding`, `qa_checkpoint`.
5. New reset and reproduction run; `qa_record_reproduction` with fresh evidence.
6. `qa_complete_job` with every criterion explicitly pass/fail/inconclusive/not_tested.
7. `qa_export_report`; close the owned browser.

The ledger defaults to `<workspace>/.qa-harness`. Override with `QA_DATA_DIR` or `serve --data-dir`. Set the workspace explicitly with the generated config; native plugin launchers inherit the client's working directory unless `QA_WORKSPACE` is set. **Confirm the workspace shown in the created job before importing files.**

## Skills

| Skill | Purpose |
|---|---|
| `qa-explore` | Risk-guided exploration and evidence capture |
| `qa-reproduce` | Fresh-run reproduction with separate evidence |
| `qa-adversarial-review` | Executable counterexamples to product guarantees |
| `qa-resume-report` | Restore context, reconcile unknown operations, report gaps |

Skills use your current model and available tools. They do not silently enable background agents, auto-approve commands, add credentials or publish results.

## Test and package

```sh
uv run pytest -q
uv run ruff check src tests scripts
uv run ruff format --check src tests scripts
uv run python scripts/package_plugins.py
```

Browser integration tests need Chromium. The suite uses a local deliberately broken save-flow fixture; it performs real UI input, observes the failure after reload, repeats it in a fresh browser, and verifies the report. It does not invoke a paid model.

`dist/plugins/{codex,claude,cursor,antigravity}` contains client-specific bundles. Runtime installation is still required. Publishing this repository does not automatically list it in any client's marketplace.

## Important boundaries

- The ledger survives restarts; browser/game/device processes and target state do not. Reconcile unknown actions instead of replaying them blindly.
- External reset descriptions, imported artifacts and external observations are attestations. A separate run is an organizational boundary, not independent proof that the target was reset.
- Budgets apply to harness operations. The host can use other tools directly, so Skills must record/check those operations; this server cannot police the whole IDE. Reported cost is not automatic subscription billing.
- Browser origins are exact and intentionally restrictive; required CDNs must be listed. WebSockets and popups are blocked in this minimal driver. This driver is not an OS/network sandbox: use an isolated VM/container/network for untrusted builds or hostile sites.
- Artifacts and reports can contain screenshots, personal data, target text and logs. There is no automatic image/log redaction. Inspect before import/export or sharing. Fill values are omitted from action metadata, but may appear in screenshots or application logs.
- Same-user local storage is not authenticated tenant isolation or tamper-proof audit storage. Use separate processes/data directories and operating-system isolation for different trust domains.
- Game internal APIs, editor/headless tests and rendered player-input tests establish different things. Use existing engine/native tools and report those distinctions.

## License

MIT. No telemetry or cloud account is built into this package. Dependency downloads and the optional browser installation require network access; the optional browser connects to the configured target origins.
