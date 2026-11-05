# Rust v0.2.0 validation and migration

Local validation on Windows, Rust/Cargo 1.97.1, 2026-11-05 (JST):

| Check | Observed result |
|---|---|
| `cargo fmt --check` | Passed |
| `cargo clippy --locked --all-targets -- -D warnings` | Passed |
| `cargo test --locked` | 13 passed; browser integration intentionally ignored in this invocation |
| `cargo test --locked --test browser -- --ignored --nocapture` | 1 passed, real Chrome |
| `cargo build --release --locked` | Passed, Windows x86_64 native executable |
| Native MCP stdio | Protocol 2025-11-25 negotiated; 18 tools, workflow resource, prompt, parallel discovery requests and job creation |
| Four-client packaging | Native manifests/configs/Skills and explicit config generation checked |
| Legacy ledger migration | Exported a copied Python Codex acceptance ledger: 2 runs, 1 confirmed finding, 45 artifacts, 87 events; all stored artifact hashes verified by exporter |

The browser fixture deliberately reports a successful save without persisting the name. The test fills/clicks/reloads, records a candidate, closes the original context, repeats in a fresh context, records a confirmed finding, completes the criterion as **fail**, and exports reports. It also checks password-value redaction in observations, whitespace-preserving input, cross-context state isolation, missing-selector uncertainty and rejected external navigation. Two requests with a 700 ms server delay overlap: observed ~0.95 s for both navigation actions, maximum simultaneous requests 2. This is a concurrency proof for the fixture, not a general throughput benchmark.

Core tests cover atomic concurrent budget reservations, cross-job/run evidence rejection, artifact tampering, workspace confinement, separate reproduction and duplicate-run rejection, pending-action reconciliation, explicit outcomes, pause/cost/time limits, restart persistence and repeated export. A cancellation test holds a SQLite write lock, cancels the caller, proves capacity remains occupied, releases the lock, and verifies the operation commits before shutdown.

## Memory observation

A release process was initialized over real MCP stdio and enumerated all 18 tools, with no browser launched. Windows process counters showed **9.06 MiB working set**, **2.54 MiB private memory**, **9.06 MiB peak working set** at that point. Executable size: **7,595,520 bytes**. [Raw measurement](evidence/rust-memory-windows.json).

This is one idle initialized process measurement, not peak memory under load, a Python comparison, or a guarantee for other platforms. Chromium and page processes are excluded. The implementation bounds operation concurrency and browser contexts, streams evidence in 64 KiB chunks, limits artifacts to 25 MiB, limits snapshots/events/logs, and streams report rows. SQLite's configured per-connection page cache is 512 KiB. Large individual records, parsing and target pages still allocate memory; these limits do not enforce a whole-process memory quota.

## Compatible migration and rollback

1. Stop this runtime's active operations and close its owned browser contexts.
2. Back up the **entire** `.qa-harness` directory, including artifacts and any SQLite WAL files, after the old process exits. Keep a separate copy when switching versions.
3. Install v0.2.0 with Cargo, regenerate explicit MCP config paths, and restart the client connection. Native plugin launchers resolve `agent-qa` on PATH; ensure it selects the intended binary.
4. Export an existing job and inspect the output before new exploration.

The six SQLite tables and existing record JSON are retained; initialization only adds indexes and WAL/cache settings. No destructive schema conversion is performed. The actual old acceptance data was copied to a separate temporary validation root; the original ledger was not opened for writing by the Rust validation. The Rust exporter checked its 45 artifacts while regenerating JSON/Markdown. A separate regression verifies that export/reopening does not rewrite job records.

For rollback, disconnect the Rust process, restore your complete pre-upgrade backup, and use the Python runtime/source or packages from [v0.1.1](https://github.com/msandroid/agent-qa-harness/releases/tag/v0.1.1). Run historical Python tests/scripts from that tag. They were removed from the Rust default branch rather than retained as a second runtime. New snapshots are bounded and browser selector support differs; tool name/schema compatibility does not promise identical unbounded responses or full Playwright behavior.

## Client and platform boundaries

The [2026-11-04 installed-client acceptance](client-acceptance-2026-11-04.md) belongs to the previous Python runtime. Three model-driven clients completed QA then; Antigravity remained incomplete. Rust protocol/browser tests do **not** establish new real-model end-to-end acceptance in all four clients. Existing user-installed runtimes were not silently replaced during this source migration.

Windows was tested locally. Linux CI is configured but not locally executed. A workflow definition is not proof of a completed hosted run. Earlier GitHub Actions runs were blocked before jobs by an account billing lock; no billing changes or paid-feature workarounds are part of this release.
