# v0.2.0 — native Rust/Cargo runtime

Replace the Python/Playwright runtime with a native Rust MCP server and Chrome DevTools driver. Preserve the 18 tool input schemas, evidence integrity checks, independent reproduction rules, SQLite records and four portable Skills.

- Bounded Tokio execution; different jobs progress in parallel and same-job actions serialize.
- Shared Chromium with bounded isolated job contexts.
- 64 KiB artifact copy/hash buffers and streamed JSON/Markdown exports.
- Explicit busy responses, snapshot/event/log limits, and shutdown draining of started work after caller cancellation.
- Cargo build/install/test/package commands; no Python/Node runtime requirement.

Windows local validation: 13 normal tests plus one real-browser integration test passed, format and strict Clippy passed, release build succeeded. An initialized release MCP process measured 9.06 MiB working set without Chrome. A copied historical acceptance ledger exported with 45 verified artifacts. See [full evidence and migration/rollback](validation.md).

The historical installed-client model acceptance belongs to v0.1.x. New Rust four-client model acceptance and Linux runtime acceptance remain unverified. Browser support uses a documented subset of selectors/keys; review the README before migration.
