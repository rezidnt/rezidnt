# Handoff — 2026-07-20 (session 11: SP4c core + SP4c-wire both shipped; SP4 COMPLETE & LIVE)

## State of play
Pointer = **SP4**. **SP4 is COMPLETE and now LIVE end-to-end** — SP4a (roles) + SP4b (delegation) +
SP4c C8 layered precedence (**core DR-019 + live wiring DR-020**) all DONE. This session ran two full
loops: **DR-019** (`/dr`→`/oracle`→impl→`/vet` pass→`/debrief` PASS) then **DR-020** (`/dr`→`/subject`
warden→`/oracle`×2→impl→`/vet` pass→`/debrief` PASS, residuals closed). **All committed + pushed to
origin/main through `a6b2c44`.** The `current-slice` file still reads SP4 — **advancing it is an explicit
owner action** (SP4 has no more sub-slices; next slice is owner's call — candidates below).

## What shipped this session (pushed)
- **`4b97d4f` SP4c core (DR-019):** `PermitLayer{Admin,Dev,Session}`, `compose_layers` (admin→dev→session
  concat), per-spec `layer` provenance, `deciding_layer` on `PermitOutcome`. Stricter-wins INHERITED from
  the unchanged monotone aggregate (no allow-override primitive). Aggregate + verdict→decision table FROZEN.
- **`2bbaddb` DR-020 ratify + `a6b2c44` SP4c-wire:** the live three-source wiring DR-019 fenced.
  `permit_config_for` (`bins/rezidentd/src/mcp.rs`) now composes **admin/dev/session** via `compose_layers`;
  **admin is sourced from host env `REZIDNT_ADMIN_PERMIT`** (a TOML `[gates.permit]` OUTSIDE the
  dev-editable workspace spec → dev cannot forge/override an admin deny, the DR-020 §Dec 1 authority
  boundary, auditor-confirmed real); dev = `workspace.spec.applied` re-stamped `Dev`; session = empty
  (future). `McpCore::with_layered_permit_config` builder; `deciding_layer` pinned in the emitted
  decision-fact policy blob (`lib.rs`) so `gate_explain` surfaces the deciding AUTHORITY (I6). Malformed/
  missing admin file **aborts daemon startup**, never serves silently-empty. `toml` parse confined to
  `rezidnt-run` (`permit_gate_from_host_toml`) — **no new daemon dep** (I7). Warden ruled the blob `layer`
  key OUT-OF-SCOPE for the ontology (parity with `deciding_verifier`, already un-modeled in that blob).
- **Housekeeping:** corrected stale `#[ignore]`/"unbuilt seam" prose in `permit_wire_dispatch.rs` +
  `permit_layered_precedence.rs` headers (the generic dispatch seam was already built/green pre-session).
  Added `/target-wsl` to `.gitignore` (uncommitted — see below).

## Next action — SP4 done; owner picks the next slice (candidates)
1. **SP5** — warden `/subject` completion for `permit.*` + folding reducers (`docs/design/permit-engine.md:143`).
2. **Deferred `/dr` items** (each its own record): **holder-offline attenuation** (DR-018 §b) · **decision
   fast-path cache** (permit §10.2, pressures I3 — DR must resolve log-all-vs-sample) · **concrete OPA/Cedar
   adapter** (behind the DR-015 exec axis) · **C3 sole-chokepoint enforcement** (DR-009 fenced; own sketch+DR).
3. Advance `current-slice` when a new numbered slice starts.

## Open /debrief residuals — NONE from this session
Both DR-020 auditor residuals were CLOSED before commit (mis-named negative control split into honest
`unset_admin_env_preserves_single_source_dev_allow` + `permissive_admin_source_does_not_deny`; added
`malformed_admin_permit_aborts_startup` + `missing_admin_permit_file_aborts_startup`). DR-020 daemon suite
is 6/6 WSL, MCP live 3/3 WSL, host `/vet` pass. Nothing carried.

## Uncommitted at session close
- **`.gitignore`** has one staged-in-working-tree change (`+/target-wsl`) not yet committed — a build-dir
  papercut fix. Commit it with the next change or on its own.

## Decisions still needing a /dr
- Holder-offline attenuation (DR-018 §b) · decision fast-path cache · OPA/Cedar adapter · C3 sole-chokepoint.
- Carried pre-permit debt: DR-007 GitError→associated-type; `badge.issued` emitter / `badge_id` on other
  mutations; release items (root README, crates.io `cargo login`); Phase 3 demand-gated.

## Environment
WSL = `wsl.exe -d Ubuntu-24.04`, cargo `~/.cargo/bin`, `CARGO_TARGET_DIR=$HOME/.cache/rezidnt-target`.
**Quote the WSL PATH export** (`export PATH="$HOME/.cargo/bin:$PATH"`) — the interop PATH contains
`Program Files (x86)`; unquoted it breaks on the parens. Vet hook host-side (`bash .claude/hooks/vet.sh`).
**Host vet.sh + WSL workspace SEQUENTIAL, never concurrent** ([[vet-concurrency-flake]]). **`/vet` is
host-side; WSL-green is NOT sufficient** ([[vet-is-host-side-wsl-insufficient]]) — AND the converse this
session: `#![cfg(unix)]` daemon suites (UnixStream) DON'T run on host `/vet`, so verify them on WSL too.
**`clippy::doc_lazy_continuation`** bit test-file `//!` headers twice this session (a line-initial `+`/`-`
in prose is parsed as a markdown bullet; indent list continuations) — host `-D warnings` fails on it.
Auto-push to `main` classifier-gated — ask first (owner approved this session's pushes).

---
**NEXT ACTION → SP4 is COMPLETE and LIVE end-to-end; DR-019 + DR-020 shipped and pushed (`a6b2c44`). No
unblocked SP4 work remains. Owner picks the next slice: SP5 (`permit.*` /subject + reducers) or one of the
deferred `/dr` items (holder-offline · fast-path cache · OPA/Cedar · C3). Advancing `current-slice` past
SP4 is an explicit owner action. One tiny loose end: commit the staged `.gitignore` `/target-wsl` line.**
