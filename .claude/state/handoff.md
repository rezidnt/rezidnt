# Handoff — 2026-07-17 (session 3 close, S2 done)

## State of play
**Current slice: S3** (MCP + attach) — not started. S2 closed this session, full loop on record: planning triage → `/oracle git` (17-test board, 2 fixture pairs) → warden ratification of the four S2 payload schemas (observed/conflict/released/diff.ready v1, additive over oracle pins) → impl to green → first debrief **FAILED** (exactly-once conflict was process-lifetime only → spurious `worktree.conflict` after restart; `observe` had no production caller + false coverage comment) → remediation oracle-first (5 restart/discovery tests in `restart_and_discovery.rs`, verified red) → registry-persisted marks + `reconcile_on_open` + `startup_facts` seam → re-debrief **PASS**. Boards at close: vet pass host (two independent runs, `{"verdict":"pass","evidence":[]}`); WSL adapter 5+4+3+3 and state suites green; fixtures green. `diff.ready` measured ~272–316 ms against the 1 s bound.

## Session log
Pushed prior session's 2 stragglers (`628a845`, `04f6d65`) to origin on owner order. This session: `4f1ba7f` S2 board+ratification (fixtures, ontology S2 payload set) → `ae75ee7` S2 impl+remediation → close commit (this file + slice pointer). **Close commits NOT pushed** — owner authorized commit+close only; push was a separate one-off authorization last time.

## Next action
**S3 planning: triage the tracked list below into S3 scope vs deferred, then `/oracle mcp`.** S3 criteria: Claude Code, via MCP only, opens a project, spawns an agent, reads its dossier, and receives a `gate_explain` for a forced failure; `attach` byte-proxy demonstrated over the socket. Phase-1 exit = golden-path-minus-gates demo, one take, recorded. NOTE: the two proto items below (request-scoped open ack; attach unknown-run error frame) were parked "S3" — they are now in-scope candidates, triage first.

## Open threads (non-slice)
- **SCRIBE/DR OWED (auditor, twice-tracked):** `release_worktree` extended the BINDING `RepoSubstrate` sketch (doc §7) with no DR or doc note — constitution says BINDING changes via `/dr`. One-paragraph `/dr` or §7 refresh settles it.
- **Warden one-liner:** ontology says conflict emission is "exactly one … forever"; under crash-between-emit-and-persist it is at-least-once (re-debrief T2). Annotate crash-free or ratify at-least-once.
- **/dr REQUIRED before Phase 2** (carried): exit-code table collision (local-input exit 2 vs §9 gate-fail=2; daemon-refusal exit 3 semantics).
- Carried: product-shaped root README not written; crates.io placeholder publish of `rezidnt-types` 0.0.1 (needs owner `cargo login`); fallback string `rezident` compromised (npm+GitHub taken) — doc still names it, note for scribe.

## Tracked items (auditor — S2 debriefs, triage at S3 planning)
Re-debrief (T1–T5): T1 crash between `allocated` emit and registry persist → next open testifies `observed allocator:"human"` against rezidnt's own tree (moderate; suspicion heuristic on pass-2 discovery). T2 conflict exactly-once is at-least-once under crash (minor; warden wording, see open threads). T3 branch discriminator false pos/neg — occupant HEAD switch reads as takeover (spurious conflict + unreleasable orphan), human remove+re-add at registered branch is invisible; **strengthen identity (marker file / HEAD oid) before Phase 2 leans on it** (major-but-in-latitude). T4 `worktree_conflict.rs:8-9` comment still imprecise: scan shares dedup marks but never calls `observe`, and reads branch via porcelain vs gix in `observe` — drift surface; extract shared ingest helper. T5 registered-but-missing rezidnt entries retained forever; conflicted entries block re-allocation with no recovery/prune verb.
First S2 debrief, still open: I4 seam hygiene — `RepoSubstrate` lives in the git crate and hard-binds `GitError` (embeds notify/git-CLI baggage); associated Error type wanted. Near-dead alloc conflict branch (CLI fails on existing path before registry check; branch untested). Fixture README preimage doesn't match live summary-v1 format (deliberate independence — don't "fix" the fixture against live output). Unasserted DEFAULTs: per-open correlation ULID and causation chain on `diff.ready`/`released` untested.
Carried S1 hardening (unchanged): open watch-loop deadline; strengthen `open_refuses_unknown_harness`; version_gate wiring; reaper wiring; harness stderr capture; denylist widening; `agent.message` >8 KiB swap test; streaming tail backlog perf.
Warden (carried): `daemon.warning` payload ratification; `badge.issued` emit-or-drop; capture-chunk subject (flagged for /dr in `artifact.captured` baseline).
Fixtures (carried): re-record tool_use transcript (PROVISIONAL); regenerate s0_rebuild_equality line 3.

## Environment
WSL = `wsl.exe -d Ubuntu-24.04`, cargo at `~/.cargo/bin`, `CARGO_TARGET_DIR=$HOME/.cache/rezidnt-target`. Vet hooks run host-side Windows cargo; unix tests need WSL. Guardrails: firewall, ontology-gate, fmt. Transient note: the permission classifier flaked once this session (temporarily unavailable) — retry works.
