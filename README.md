# Agent QA Harness

Evidence-first QA for **Codex, Claude Code, Antigravity and Cursor**, implemented in **Rust + Cargo**. Use your coding client's frontier model and existing application/game tools. The native stdio MCP server provides 18 tools and four portable Skills for exploration, adversarial review, reproduction and reporting. No separate model API key is required.

[日本語](README.ja.md) · [Client setup](docs/clients.md) · [Architecture](docs/architecture.md) · [Rust validation](docs/validation.md)

## Install

Build with Rust/Cargo (Rust 1.88 minimum declared; tested with 1.97.1):

```sh
cargo install --git https://github.com/msandroid/agent-qa-harness --tag v0.2.0 --locked
```

Or build this checkout:

```sh
cargo build --release --locked
cargo install --path . --locked
agent-qa --version
```

The runtime needs neither Python nor Node. The optional browser driver needs an installed Chrome/Chromium executable; set `QA_CHROME` to its absolute path if autodetection fails. Browser memory is additional to the harness process.

Generate an explicit configuration for a test workspace:

```sh
agent-qa config --client codex --workspace /absolute/test/workspace
agent-qa config --client claude --workspace /absolute/test/workspace
agent-qa config --client cursor --workspace /absolute/test/workspace
agent-qa config --client antigravity --workspace /absolute/test/workspace
```

On Windows use a quoted path, for example `"C:\Projects\MyApp"`. Merge the printed server entry into your client's MCP settings. It uses this executable's absolute path and only prints configuration. Native plugin manifests use `agent-qa serve` on PATH. See [client instructions](docs/clients.md) for Skills and plugin bundles.

## Bounded memory and parallel execution

```sh
agent-qa serve --workspace /absolute/test/workspace --max-concurrency 4 --max-browsers 2
```

- Tokio runs two async worker threads. Blocking ledger/file operations use a bounded blocking pool.
- At most four tool operations run by default; excess requests receive a busy error. Different jobs can progress concurrently; operations on the same job serialize.
- One shared Chromium process hosts at most two isolated job contexts by default. Browser processes have their own memory use; target pages can consume substantial memory.
- Artifact copies and SHA-256 checks use 64 KiB chunks, with a 25 MiB artifact limit. JSON/Markdown exports stream SQLite rows instead of loading an entire ledger.
- SQLite connections have a 512 KiB cache setting. Job snapshots cap child records at 500 per table and disclose truncation. Events are paginated at 100. Browser logs retain at most 200 entries; observations retain bounded text/controls.

These are operation/data limits, not an OS-enforced process memory quota or a sandbox for hostile content.

## QA workflow

Ask your agent to use `qa-explore` on a specific build, with explicit criteria, scope, allowed origins and a budget. Then:

1. `qa_create_job` fixes the contract; reset the target and `qa_begin_run`.
2. Explore through the browser adapter or existing tools; record external steps and import evidence.
3. `qa_add_finding` records candidates. Checkpoint hypotheses and untested areas.
4. Reset again, begin a separate reproduction run, and record fresh evidence with `qa_record_reproduction`.
5. `qa_complete_job` records every criterion as pass/fail/inconclusive/not_tested; `qa_export_report` writes JSON and Markdown.
6. Close owned browser contexts. Reconcile unknown actions before retrying or completing.

Ledger and evidence default to `<workspace>/.qa-harness`, override with `QA_DATA_DIR` or `--data-dir`. Artifact hashes and job/run ownership are checked. A completed harness job can correctly report a failed product criterion; it is not proof of defect absence. Imported observations and resets remain operator-attested.

| Skill | Purpose |
|---|---|
| `qa-explore` | Risk-guided exploration |
| `qa-reproduce` | Fresh-run reproduction |
| `qa-adversarial-review` | Executable counterexamples |
| `qa-resume-report` | Resume, reconcile and report gaps |

## Browser capabilities

Native Chrome DevTools Protocol supports navigation, observation, click, fill, press and viewport screenshots. Selectors support CSS, `text=...`, and simple `:has-text('...')`; this is not full Playwright selector compatibility. Keys support common editing/navigation keys, ASCII and modifiers. The driver restricts routed HTTP(S) requests to exact contract origins, blocks WebSockets, bypasses service workers and denies downloads. It exposes no arbitrary JavaScript tool. Request interception is not a network security boundary; use OS/network isolation for untrusted targets.

Games, desktop and mobile use the host's existing tools and imported evidence. Hosted tenant isolation, device farms and customer billing are extension points, not shipped services.

## Validate and package

```sh
cargo fmt --check
cargo clippy --locked --all-targets -- -D warnings
cargo test --locked
cargo test --locked --test browser -- --ignored --nocapture
cargo run --locked -- package
```

The ignored browser test requires Chrome. It drives an intentionally broken local save flow, verifies isolated parallel jobs and independent reproduction, and exports a failed QA report. No paid model is invoked. The packager writes four client directories under `dist/plugins`; runtime installation is separate.

## Migration and acceptance status

v0.2.0 replaces the Python runtime with Rust while retaining the 18 tool names/input schemas and SQLite tables. Existing ledgers remain readable; back up the whole data directory before switching runtimes. [Migration evidence and rollback](docs/validation.md).

The [2026-11-04 client acceptance](docs/client-acceptance-2026-11-04.md) documents the previous Python runtime: Codex, Claude and Cursor completed real-model QA, while Antigravity remained incomplete. Those results do not establish four-client model acceptance of this Rust rewrite. Rust stdio protocol, ledger migration and real browser integration are validated separately.

MIT license. Publishing the repository does not automatically list it in client marketplaces.
