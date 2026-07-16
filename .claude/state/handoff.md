# Handoff — 2026-07-16 (session 2 close)

## State of play
**Current slice: S2** (git adapter) — not started. S1 closed this session: first debrief FAILED (no `rezidnt open`/`attach` CLI verbs — daemon mechanics existed but weren't human-typeable; plus I4 harness-field-ignored), remediated oracle-first (4 CLI-surface pins at affeb89, verbs + atomic harness refusal at 15a34d8), re-debrief PASS. Boards at close: Windows vet pass; WSL 41 suites / 77 tests / 0 failed; fixtures green.

## What changed this session (since 029781b)
- `b75ea03` S1 oracle board (36 red; REAL recorded claude 2.1.191 transcript fixture) → `06f3edc` warden payload ratification (10 subjects, zero drift) → `b300a27` fixture provenance fix (oracle recorded 2 runs, installed the wrong one; restored pinned run verbatim) → `1ac0cff` S1 implementation (rezidnt-cas, rezidnt-run, proto requests, agent-run reducers, daemon open/attach) → `affeb89` CLI pins → `15a34d8` remediation.
- Agent-path note: a sustained 529 outage forced the S1 oracle round to run INLINE in the main session (fixture mixup above was the cost); agents recovered from the warden round on.

## Next action
**S2 planning: triage the tracked list below into S2 scope vs deferred, then `/oracle git`** (S2 criteria: `diff.ready` ≤1 s post-debounce; out-of-band worktree collision → exactly one `worktree.conflict`).

## Tracked items (auditor's list, re-debrief-updated — triage at S2 planning)
- **/dr REQUIRED before Phase 2**: exit-code table collision (local-input exit 2 vs §9 gate-fail=2; daemon-refusal exit 3 semantics).
- S2-adjacent (natural scope): committed-repo detach worktree test; `worktree.released` + `.rezidnt/worktrees` registry; `worktree.allocated` source-id → git adapter; streaming tail backlog (S0 carryover, O(log) String per client).
- Hardening: `open` watch-loop deadline (same-millisecond marker skip → hang); strengthen `open_refuses_unknown_harness` (assert zero workspace/worktree facts; multi-agent case); version_gate wiring (bite: init line's `claude_code_version`); reaper wiring (pidfiles at spawn, startup reconcile, emit `agent.signaled`); harness stderr capture; denylist widening; `agent.message` >8 KiB swap-path test.
- Proto/S3: request-scoped open ack (deletes name-match heuristic); attach unknown-run error frame (currently reads as silent success).
- Warden: `daemon.warning` payload ratification (bounded error); `badge.issued` emit-or-drop decision; capture-chunk dedicated-subject question.
- Fixtures: re-record tool_use transcript from a real run (PROVISIONAL); regenerate s0_rebuild_equality line 3 (pre-ratification payload).

## Standing gates (owner-only, unchanged)
employer IP memo (push blocked until `.claude/state/ip-memo-cleared`); name registry checks (fallback `rezident`).

## Environment (unchanged)
WSL = `wsl.exe -d Ubuntu-24.04`, cargo at `~/.cargo/bin`, `CARGO_TARGET_DIR=$HOME/.cache/rezidnt-target`. Vet hooks run host-side Windows cargo; unix tests need WSL.
