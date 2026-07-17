# Handoff — 2026-07-17 (session 2 close, post-publication)

## State of play
**Current slice: S2** (git adapter) — not started. S1 closed: first debrief FAILED (no `rezidnt open`/`attach` CLI verbs; I4 harness-field-ignored), remediated oracle-first (CLI pins `594f8d1`, remediation `e7e3132`), re-debrief PASS. Boards at close: Windows vet pass; WSL 41 suites / 77 tests / 0 failed; fixtures green.

**PUBLISHED**: repo is public at github.com/rezidnt/rezidnt (owner's org, created 2026-07-04), first push 2026-07-16, `main` tracking `origin/main`. NOTE: history was REWRITTEN before publication (employer-entity references scrubbed by owner order, old objects purged, DR-003); the current SHAs are the only lineage that exists anywhere.

## Session log (SHAs are post-rewrite)
`95c7fc1` bootstrap (harness+doc+ontology) → `242df09`/`b53b29e` S0 board+impl → `ce982c2` S1 advance → `fb22379` S1 board (REAL claude 2.1.191 transcript fixture) → `282f16f` payload ratification → `f2fd3b0` fixture provenance fix → `0544be5` S1 impl → `594f8d1`+`e7e3132` remediation → `c3a4e35` S1 close → `610cdd0` DR-003 pushgate retirement → `99ec768` §17 publication files (LICENSE, LICENSE-MIT, TRADEMARKS, SECURITY, CONTRIBUTING; NOTICE deliberately absent — nothing ported per DR-001).

## Next action
**S2 planning: triage the tracked list below into S2 scope vs deferred, then `/oracle git`** (S2 criteria: `diff.ready` ≤1 s post-debounce; out-of-band worktree collision → exactly one `worktree.conflict`).

## Open threads (non-slice)
- Product-shaped root README offered, not written (landing page currently shows the harness README).
- crates.io placeholder publish of `rezidnt-types` 0.0.1 to lock the name (needs owner `cargo login`). `rezidnt` free on crates.io+npm; GitHub org is the owner's. FALLBACK STRING COMPROMISED: `rezident` taken on npm and GitHub — doc still names it as fallback; note for scribe.
- **/dr REQUIRED before Phase 2**: exit-code table collision (local-input exit 2 vs §9 gate-fail=2; daemon-refusal exit 3 semantics).

## Tracked items (auditor's list — triage at S2 planning)
- S2-adjacent: committed-repo detach worktree test; `worktree.released` + `.rezidnt/worktrees` registry; `worktree.allocated` source-id → git adapter; streaming tail backlog (O(log) String per client).
- Hardening: `open` watch-loop deadline (same-ms marker skip → hang); strengthen `open_refuses_unknown_harness` (assert zero workspace/worktree facts; multi-agent case); version_gate wiring (bite: init line's `claude_code_version`); reaper wiring (pidfiles at spawn, startup reconcile, emit `agent.signaled`); harness stderr capture; denylist widening; `agent.message` >8 KiB swap-path test.
- Proto/S3: request-scoped open ack (deletes name-match heuristic); attach unknown-run error frame (reads as silent success).
- Warden: `daemon.warning` payload ratification (bounded error); `badge.issued` emit-or-drop; capture-chunk subject question.
- Fixtures: re-record tool_use transcript (PROVISIONAL); regenerate s0_rebuild_equality line 3 (pre-ratification payload).

## Environment
WSL = `wsl.exe -d Ubuntu-24.04`, cargo at `~/.cargo/bin`, `CARGO_TARGET_DIR=$HOME/.cache/rezidnt-target`. Vet hooks run host-side Windows cargo; unix tests need WSL. Guardrails now three hooks: firewall, ontology-gate, fmt (pushgate retired by DR-003).
