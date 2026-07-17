# Handoff — 2026-07-17 (session 4 close, part 2: pre-S4 batch done)

## State of play
**Current slice: S4** (verifier engine v1, Phase 2) — pre-work COMPLETE, engine not started. Earlier today: S3 closed + Phase 1 exited (demo recorded, see prior handoff content in git history at `bc9fc61`). Then the owner ordered the full pre-S4 batch; all three items landed:
1. **DR-004 RATIFIED (owner, option C)** — exit-code table widened: 0 ok · 1 unexpected · 2 local input/usage (clap-aligned) · 3 substrate-fault incl. refusals · 4 daemon-unreachable · **5 gate-fail** (`inconclusive` is 3, never coerced, I6). Zero migration; collision flags retired. **S4 oracle board MUST pin exit 5 for gate-fail and 3 for inconclusive.**
2. **S3-T1/T2 fixed** — eager `rebuild_workspaces` (pure log+CAS fold) before any transport; `idempotency_key` additive on `agent.spawned` (warden-ratified, dedup scope = envelope workspace + key); keyed retry survives SIGKILL; ghost window closed by evict-on-failure.
3. **S2-T3 fixed** — WorktreeId marker in the private gitdir, written before the `allocated` fact; scan compares marker vs registry id. Branch switch ≠ takeover; foreign re-add detected exactly once.
Loop on record for 2+3: oracle 5-test red board → warden ratification → impl → independent vet (host `{"verdict":"pass"}` + WSL workspace green) → debrief **PASS**.

## Session log (this part)
`fbb7f4b` DR-004 → `a3620ce` remediation board + ratification → `66aa5a5` remediation impl (vet+debrief pass) → this handoff. LOCAL — `origin/main` at `bc9fc61`; push on owner order.

## Next action
**`/oracle gate` — the S4 board.** S4 = verifier engine v1: native pack (diff-scope, test-suite, forbidden-path, secret-leak, build-passes) + exec contract (§8 JSON stdin/stdout, CAS-pinned inputs, no-network default, 120s timeout, malformed/nonzero → inconclusive); `vet` enforces bare-mode/pinned-version/allowedTools pre-spawn; `pre_merge` + `debrief` on the golden path. Exit: agent spawned under gates produces a VERIFIED merged diff with replayable `debrief` and recorded cost. Board obligations: exit-code pins per DR-004; `gate.passed` v1 needs warden ratification (deferred until an emitter exists — that's now); replay-divergence integrity alarm (§8).

## Open /debrief findings (tracked)
- **NEW medium (this debrief, wants an oracle pin early in S4):** T2 eviction over-reaches — fires on ANY materialization failure, but `workspace.opened` publishes at step 1; a post-fact failure (e.g. agent 2 of 2 launch) evicts what the log opened and restart resurrects it. Refusal divergence, not false facts. Direction: evict only when `opened_id` was never published. `runs.rs:374-457`.
- **NEW low:** legacy id-less registry entries keep branch-as-identity (self-extinguishing migration fallback); marker-write vs registry-persist crash window joins the T4 provenance family; boot-time full-log scan is a flagged DEFAULT (wants index/snapshot when logs grow).
- **Carried S3 lows:** T4 at-least-once `worktree.allocated` on retry + daemon-wide spawn lock (liveness); unbounded HTTP body (cap it); `daemon.warning` open-failed fresh correlation; lockfile tmp `create(true)`→`create_new`; T8 silent DEFAULTs for scribe (protocol version, tail limit, 202, dedicated runtime, schemars runtime dep).

## /dr and warden queue
- **Badge bundle (one session):** `badge.issued` emit-or-drop + operator-badge daemon-lifetime scope + `badge_id` on other mutation facts + S3-T3 unbadged `gate.explained`. `gate.passed` v1 → fold into S4 oracle/warden pass.
- **Carried:** `release_worktree` BINDING extension `/dr` (twice-tracked); warden conflict at-least-once wording; capture-chunk `/dr` flag; scribe note: hand-rolled-over-rmcp as formal DEFAULT; RepoSubstrate/GitError seam (I4); S1 hardening list; `daemon.warning` payload ratification; fixture housekeeping; root README; crates.io placeholder (owner `cargo login`); `rezident` fallback doc note; S2-T4 ingest helper → next git-adapter touch; S2 T1/T5 → Phase-2 hardening (T5 prune verb pairs naturally with S4 CLI work).
- Demo recording location not yet noted in-repo (docs/demo/?) — ask owner.

## Environment
WSL = `wsl.exe -d Ubuntu-24.04`, cargo `~/.cargo/bin`, `CARGO_TARGET_DIR=$HOME/.cache/rezidnt-target`. Vet hook host-side; daemon tests WSL. Demo daemon may still be running (port 40173, `~/rezidnt-demo`). WSL `claude` resolves to the Windows npm shim via interop — native install advised if S4 spawns real harnesses in tests.
