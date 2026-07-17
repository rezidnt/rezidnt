# Handoff — 2026-07-17 (session 4 close, part 3: S4 DONE — golden path completes)

## State of play
**Current slice: S5** (ratatui read-only fleet board) — not started. **S4 CLOSED: the golden path completes.** Full loop on record: `/oracle gate` (37-test board + new rezidnt-gate crate) → warden ratified `gate.passed` v1 / new `diff.merged` v1 / `agent.spawned` governed fields → implementer built the verifier engine to green → **independent vet caught 1 flake the implementer missed** (see below) → re-vet green host + WSL → `/debrief` **PASS**. The S4 exit test (`golden_path_verified_merged_diff_with_replayable_debrief_and_cost`) runs end-to-end: gated open → vet pre-spawn → real worktree change → diff.ready → pre_merge verifiers → gate.passed → REAL git merge (verified in HEAD) → diff.merged → replayable debrief exit 0, cost 0.001, alarms []. Slice pointer advanced S4→S5.

## Session log (part 3)
`1f3e25e` S4 board + ratification (incl. SUBJECTS_V0 diff.merged companion, taxonomy_drift kept green) → `0dbde68` S4 impl (vet+debrief pass). LOCAL — `origin/main` at `bc9fc61` (pushed through S3 close only). **UNPUSHED: fbb7f4b, a3620ce, 66aa5a5, 20610cd, 1f3e25e, 0dbde68 (6 commits) — push on owner order.**

## Process note (important)
The implementer reported "all 37 green" but my independent WSL vet found `exec_contract.rs::nonzero_exit_...` RED once. Root cause: my first WSL run was concurrent with the host gauntlet (two cargo instances saturating the cores); under that fork pressure `cmd.spawn()` of /bin/sh hits EAGAIN and the runner maps it to Inconclusive{MalformedOutput} instead of {NonzeroExit}. Verdict stays Inconclusive on every failure branch — never a false pass (I6 core holds). Re-runs green: 3/3 isolated, 5/5 in-binary, 2/2 full-workspace. Auditor adjudicated: LOW tracked, not a blocker. **Lesson: do not run host vet.sh and WSL workspace concurrently — they fight for cores and induce spawn flakes.**

## Next action
**S5 planning, then `/oracle tui` (or `/slice` first).** S5 = ratatui read-only fleet board consuming ONLY watch channels — the proof that I1 (zero pixels in core) held: the board is a pure client of the socket/watch surface, no daemon change. Primary visibility surface beyond the CLI. NOTE slice-discipline says S5 "may precede Phase 3" and is demand-flexible; confirm with owner whether S5 is next or whether to bank Phase-2 hardening / the deferred /dr + warden queue first (the golden path is DONE, so pressure is off).

## Open /debrief findings (S4, all in-latitude, verdict PASS)
- **LOW:** exec spawn/wait-io failures collapse to `MalformedOutput` reason (the flake). Direction: add a `could_not_run` reason, map spawn/io errors to it — additive (ontology reason vocab is additive strings). `rezidnt-gate/src/lib.rs:621-688`.
- **LOW:** `environment_is_scrubbed` uses process-global `std::env::set_var` (test-isolation smell; env_clear makes the scrub structural so it can't cause a false pass). Oracle: per-test isolated env in a future pass. `exec_contract.rs:196`.
- **LOW (seam debt, I4):** daemon `git_diff_summary` (gates.rs:390) duplicates the S2 git-adapter diff-summary parser — divergence risk if S2's format evolves. Direction: route the run-task summary through the RepoSubstrate seam when wired. `bins/rezidentd/src/gates.rs:390`.
- **LOW:** CLI `agent_spec_toml` (bins/rezidnt/main.rs:660) and daemon copy (gates.rs:44) must stay byte-identical to vet the same preimage; today they agree. De-dup opportunity.
- **BOUNDARY (S0, not S4):** replay tamper-detection is bounded by CAS content-addressing + log-chain integrity; a self-consistent tamper (blob+ref+chain) is the S0 chain-verify guarantee's job, not the gate layer's. Honest boundary.
- **NOTE:** `run_native` cost floor `.max(1)` is not replay-stable across machines, but replay compares verdicts only (not cost) — no alarm risk. Guard if replay ever widens to cost.

## Carried S3 findings (still open)
- T1/T2 CLOSED this session (eviction fix landed). T3 (gate_explain unbadged) → badge bundle below. T4–T7 lows (at-least-once alloc, unbounded HTTP body cap, daemon.warning correlation, lockfile create_new). T8 silent DEFAULTs for scribe.

## /dr and warden queue
- **Divergence-as-log-fact (`/dr`, NEW):** whether debrief replay divergence lands a durable integrity-alarm FACT on the log (daemon.error? new `integrity.alarm`/`gate.diverged`?) or stays CLI-report-only (today: report + exit 3, emits nothing). Additive; touches §14 self-observation. Owner/`/dr` call.
- **Badge bundle (one session):** `badge.issued` emit-or-drop + operator-badge daemon-lifetime scope + `badge_id` on other mutation facts + S3-T3 unbadged `gate.explained`.
- **Carried:** `release_worktree` BINDING extension `/dr` (twice-tracked, now thrice); warden conflict at-least-once wording; capture-chunk `/dr` flag; scribe: hand-rolled-over-rmcp DEFAULT + T8 DEFAULTs + `could_not_run` reason note; RepoSubstrate/GitError seam (I4); S1 hardening list; `daemon.warning` payload ratification; fixture housekeeping; root README; crates.io placeholder (owner `cargo login`); `rezident` fallback doc note; S2-T4 ingest helper + S2 T5 prune verb (pairs with S5/CLI work); S2 T1 → Phase-2 hardening.
- Demo recording (S3) location still not noted in-repo (docs/demo/?).

## Environment
WSL = `wsl.exe -d Ubuntu-24.04`, cargo `~/.cargo/bin`, `CARGO_TARGET_DIR=$HOME/.cache/rezidnt-target`. Vet hook host-side; daemon/gate/exec tests WSL. **Do NOT run host vet.sh + WSL workspace concurrently (spawn-flake under core contention).** Fable 5 hit its weekly credit limit mid-session — all agents now run on Opus 4.8 (owner switched default). Demo daemon may still be running (port 40173, `~/rezidnt-demo`). WSL `claude` = Windows npm shim via interop.
