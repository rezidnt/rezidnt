---
name: rust-conventions
description: >-
  Rust style, error-handling, async, and dependency conventions for the rezidnt codebase.
  This skill should be used whenever writing or reviewing Rust in this project — it fixes
  the error-handling split, async rules, forbidden patterns, and the approved dependency
  set so code is consistent across agents. Load for any work under crates/ or bins/.
user-invocable: false
version: 0.1.0
---

# Rust conventions (rezidnt)

Rust edition 2024. These are enforced by `/vet` (clippy -D warnings) and the auditor.

## Errors
- Libraries (`crates/*`): `thiserror`-derived enums, one per crate domain. No `anyhow` in libs.
- Binaries (`bins/*`): `anyhow` at the edges, `?` throughout.
- No `unwrap`/`expect` outside `#[cfg(test)]`. A genuine invariant-panic uses `unreachable!` with a message stating the invariant.

## Async
- `tokio` multi-threaded runtime. No blocking calls in async contexts: filesystem-heavy or `git2`-style blocking work goes in `spawn_blocking`; prefer `tokio::process` and `tokio::fs`.
- Every adapter task carries a `tracing` span (`info_span!("adapter", kind=...)`). Supervision (backoff, breaker) lives in rezidnt-supervise, not hand-rolled per adapter.
- Channels: `mpsc` for commands into a task, `broadcast` for the fabric, `watch` for materialized state. Bound every `mpsc`; document the bound.

## Types and API
- `newtype` every id (`WorkspaceId(Ulid)`), never raw strings/UUIDs across boundaries.
- Public payload/config structs derive `Serialize, Deserialize, JsonSchema` (schemars) so the MCP surface and npm-published types cannot drift.
- No `pub` field leakage of invariants — if a value must stay ≤32KiB, enforce it in a constructor, not by convention.

## Dependencies (approved set; additions via a note in the PR)
tokio, serde/serde_json, rusqlite (WAL), ulid, schemars, gix (reads), notify, blake3, tracing/tracing-subscriber, clap, thiserror/anyhow. Phase-gated: portable-pty (run substrate), rmcp (MCP — verify at S3; fallback hand-rolled JSON-RPC), ratatui (S5 board). Prefer std and these over novel crates; every new dependency is attack surface against I7.

## Forbidden
- `localStorage`/browser-storage assumptions (N/A here but flagged for any bundled artifact HTML).
- Panicking error handling in library code. Silent `let _ =` on a Result that carries a real failure.
- Reaching for a crate to avoid writing 20 lines — but equally, NIH-ing a syscall wrapper (I8 component clause).
