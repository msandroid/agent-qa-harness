---
name: qa-explore
description: Explore running applications, games or software for unknown quality issues using the Agent QA MCP evidence ledger and available application tools.
---

# Explore product quality

Use the host's current model. No additional model API account is required. The Agent QA MCP server manages state and evidence; existing browser, mobile, engine or computer-use tools supply capabilities it does not implement. If Agent QA tools are absent, give the setup steps in the repository README; do not imply that a ledger exists.

Fix the target build, scope, criteria and budgets with `qa_create_job`. Read `qa://workflow` if available. Browser jobs need exact allowed HTTP(S) origins, including required resource origins. Choose criteria that describe user-visible behavior, not just element presence. Reuse a suitable existing job only when its build and scope match.

Reset the target with available tools, then call `qa_begin_run` with the actual fixture description. This records an attestation, not proof of reset. Use `qa_browser_start` and `qa_browser_action` for supported Web operations, or existing host tools and `qa_record_step` for other applications. Browser contexts reset cookies/storage, not databases or game servers. External tools are outside the harness's budget enforcement: check remaining budget before using them and record each operation promptly.

Track explored functions/states, open hypotheses and next actions with `qa_checkpoint`, especially before context loss. Prefer unvisited, high-impact states over repeating the same successful flow. At the deadline or budget limit, preserve evidence and unresolved areas, then finalize them as inconclusive/not_tested as appropriate.

For an anomaly, preserve screenshots/logs/relevant state in the workspace, inspect them for private data, and use `qa_attach_artifact`. `qa_add_finding` requires run-local evidence and separates expected from actual behavior. Label cause explanations as hypotheses until established. See the adjacent `qa-reproduce` skill to verify a candidate.

Finalize every criterion with `qa_complete_job`, then `qa_export_report`. Completion is workflow completion, not quality approval. Evidence-backed pass/fail is an operator assessment; missing evidence or blocked execution is inconclusive/not_tested. Close owned browsers after work. Do not modify product code or weaken checks merely to make QA pass.

On resume, inspect `qa_get_job` and paginate `qa_get_events`. Reconcile unfinished actions after observing the target; never blindly replay an uncertain submit/save. Browser processes do not survive server restart; start a new run/context. For game input/rendered acceptance, read [references/game-testing.md](references/game-testing.md).
