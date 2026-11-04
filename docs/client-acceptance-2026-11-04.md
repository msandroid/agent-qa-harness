# Installed-client acceptance — 2026-11-04

These are actual model-driven runs against a disposable local application, not replays of the deterministic unit test. Client transcripts/authentication metadata remain private. Only the results below are published.

## Method

Installed the runtime into the user's tool environment. Installed Codex and Claude via their native local-marketplace managers, Antigravity via `agy plugin install`, and Cursor under its documented user-local plugin directory. The model was asked to load the installed `qa-explore` skill and use the installed MCP server.

Each client received an independently served copy of Harbor Profile, build `harbor-profile-fixture-v1`, with the same visible save/cancel/persistence requirements. The prompt did not disclose the seeded persistence defect. Reading application source, shell HTTP access, product edits and replacement QA implementations were excluded. Client workspaces, ledgers and browser contexts were separate.

Acceptance requires actual browser actions, a checkpoint, an evidence-backed finding, a separate reproduction run with fresh artifacts, finalized criteria and both exported report formats. Stored evidence hashes were independently checked after the run. The fixture has one intentionally seeded persistence defect; these results do not measure general defect recall or certify games/mobile clients.

## Results for v0.1.0

| Client | Native installation | Model-driven end to end | Recorded operations | Artifacts | Elapsed |
|---|---|---|---:|---:|---:|
| Codex CLI 0.166.0-alpha.6 | Passed | Passed | 17 | 45 | 271 s |
| Claude Code 2.1.247 | Passed | Passed | 25 | 69 | 416 s |
| Cursor Agent 2026.10.22-27dcf5f | Passed | Passed after workspace-scoped tool approval | 12 | 30 | 136 s |
| Antigravity CLI | Passed; 4 Skills and 1 MCP server registered | Incomplete: per-action approval / first-run UI blocked | 0 | 0 | — |

All three completed clients found the save-persistence defect from the UI, independently reproduced it in a fresh isolated browser, finalized save-persistence as fail and cancel-edit as pass within their stated coverage, exported JSON and Markdown, and closed the browser. Every copied artifact verified against its stored SHA-256. This is successful QA workflow execution; the tested application intentionally fails one criterion.

Job/finding IDs:

- Codex: job `506706724f604abaacd2253ba275fadb`, confirmed finding `907484a06957446887f3d7432a8523dc`.
- Claude: job `b7725be072e14b8ba1f7e1c8cfa8dc34`, confirmed finding `770de6370d6b48e18114888f5fa5d570`.
- Cursor: job `9a29fcefb88a4df38a5a4d90757526fd`, confirmed finding `7a93940d4b1c4b179744d6a1662ff2d2`.

## Issues found and addressed

1. **Noninteractive permission rejection is not a product failure.** Cursor's `--approve-mcps` trusts server startup but does not approve every tool execution. Initial calls were rejected. A disposable test workspace `.cursor/cli.json` with exact `Mcp(plugin-agent-qa-agent-qa:qa_...)` tool entries allowed the run to complete. This installed version also requires `permissions.deny` to be an array, even when empty. No global Cursor permission policy was changed.
2. **Readonly metadata was missing.** The MCP list/get/event tools lacked readonly hints, so clients could classify a list operation as mutating. Added protocol annotations for just these three tools and a real stdio schema test. Mutation tools retain conservative defaults.
3. **Codex CLI options conflicted.** The acceptance runner initially combined `--sandbox` with `--approve-for-me`. Removed the redundant mutually exclusive argument; native plugin discovery and the full flow then succeeded.
4. **Repeatable client verification was absent.** Added a black-box UI fixture, a client launcher with bounded runtime, and a separate ledger/artifact verifier. Child exit codes now propagate; no helper manufactures a finding or completes a job on behalf of the client.
5. **Codex repository marketplace instructions were incomplete.** Added an explicit `.agents/plugins/marketplace.json` so the repository can be added through the native manager.
6. **Windows runtime/plugin updates can fail with active file locks.** Both a force tool reinstall and Antigravity's remove-then-copy update stopped on in-use paths. The runtime was repaired by installing the package into the same environment and the plugin files were restored in place, without stopping unrelated apps or changing permission policies. The installed CLI and all four plugin manifests were then verified at v0.1.1. Prefer closing the owned plugin processes before future remove/recreate updates; do not stop unknown workloads.

The end-to-end results above were collected on v0.1.0. The v0.1.1 changes preserve the workflow and add readonly annotations/documentation/evaluation tooling; the 20-test regression suite and a real stdio probe of the installed v0.1.1 runtime passed. This distinction is retained instead of relabeling old runs as new-version acceptance.

## Antigravity boundary

The installed Skill and MCP tool schemas were discovered by the model. In noninteractive mode, an unconfigured `qa_create_job` permission was denied; auto mode was unavailable on the account. A proposed global server-wide allow rule was rejected by the automatic approval reviewer as broader and more durable than necessary, and was **not applied**. An interactive attempt reached the first-run theme/setup screen, but automated Enter input was also rejected because the reviewer could not independently establish the current screen state. No permission bypass was used. End-to-end acceptance is not claimed for Antigravity until the scoped interactive setup/approval is authorized and completed.

## Reproduce the evaluation

After installing the native client plugin and runtime:

```sh
uv run python scripts/run_client_acceptance.py codex --workspace /absolute/disposable/codex
uv run python scripts/run_client_acceptance.py claude --workspace /absolute/disposable/claude
uv run python scripts/run_client_acceptance.py cursor --workspace /absolute/disposable/cursor
uv run python scripts/summarize_client_acceptance.py /absolute/disposable/codex
```

This uses the selected client's authenticated model and may consume its normal usage allowance. Prepare tool-specific permissions in the disposable workspace or use interactive per-action approval. Do not disable all approval/sandbox controls to make the test pass. Raw outputs may include private client metadata and should not be committed or uploaded without review.
