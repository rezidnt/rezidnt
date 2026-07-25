# Handoff — 2026-07-24 (session 25: PHASE 3 OPENED · DR-044 live lead→sub fan-out COMPLETE)

## State of play
Owner opened **Phase 3** for the orchestrator arc (Phase 2's exit criterion is met — `bench/harness/tests/real_driver.rs`
drives a real `DaemonDriver` end-to-end). The **DR-044 live lead→sub fan-out slice is DONE**: final `/debrief` **PASS**,
host `/vet` **green at `459ef6d`**, WSL `fan_out_live_e2e` **7/7**. 19 commits, **pushed and synced to `origin/main`**
(`910e6b6`), tree clean except the pre-existing untracked `.playwright-mcp/` and `docs/site/` (leave them). Nothing is
mid-flight and no agent is running. High autonomy ON ([[autonomy-high-trust]]).
`current-slice` = `live-lead-sub-fanout` (**done**).

**Shipped:** the governed `fan_out` MCP tool (badge-doored + lead-only, per-task idempotency through the EXISTING
`spawn_keys` map, honest partial failure with no fake rollback, width cap 8 refused whole-call before any effect), the
daemon lead→sub path, and the `orchestration_graph` read side now folding a real edge.

## Records (3 accepted, 2 errata)
- **DR-044** — the slice; opens DR-042's Phase-3 gate. Decisions 2b/6 later amended, Decision 3 posture amended.
- **DR-045** — `fan_out` is **lead-only**; an operator badge is refused on POLICY (`fan_out.lead_only`), kept distinct
  from `badge.invalid` because an operator badge is *valid*, just the wrong kind (I6). Mirror of DR-032's `kill_run`.
- **DR-046** — three things the build surfaced: (1) `fan_out` is **structurally same-process-only** (DR-045 × DR-017 §6:
  the root key is per-process, so a lead's badge dies with the daemon); (2) the lead→sub edge was **mis-modelled** —
  emitting `permit.delegated` for a fan-out asserted an attenuation that never happened, contaminating
  `BoardRow.delegated` (chain DEPTH); (3) registry wiring **deferred**. Two errata: the "subject" it asked for was
  correctly minted as a **field**, and its owed-guard ledger was stale on (f).

## Ontology (warden, 2 sessions)
1. `worktree.allocated.allocator` widened to admit the scheme-tagged delegating principal `run:<ULID>`; ordinary
   allocations still emit `"rezidnt"` verbatim. Lockstep fix to `worktree.conflict.holder?`.
2. **REFUSED** an orchestration subject and minted the **field `agent.spawned.lead_run?`** (bare ULID, `lead_run != run`)
   on the house discriminator: a property fixed at spawn, 1:1 with the spawn fact, keyed on the sub's own RunId, earns a
   field. Keying run-to-run discharged **two hazards by construction** — DR-044's silent `fan_out: 0` and the cross-run
   badge-collision residual. Also corrected `ontology:216`, which asserted a `RepoSubstrate::allocate` path the daemon
   does not take.

## Open findings (all non-blocking; `/debrief` PASSED with these named)
- **Source guard is a tripwire, not proof.** `bins/rezidentd/tests/permit_delegated_is_attenuation_only.rs` is the only
  `rezidentd` test that runs on Windows. It **narrows** the host gap to the known emit site — it does NOT close it (an
  emit in another file, or a non-literal `Subject::new(CONST)`, evades it). Say "narrowed", never "closed".
- **Orphan-lead silence:** a sub naming a lead the log never spawned surfaces **no row and no alarm**
  (`crates/rezidnt-state/src/lib.rs:1755-1763`). Ruled the right call (the alternative invents an entity the log never
  minted) but the property lives only in a code comment. DR-046 §Decision 8's slice is where someone meets it next.
- **Line-cite fragility, 3rd occurrence this arc.** `runs.rs:1075-1095` is hard-coded in two files; a 9-line comment
  insertion invalidated the previous cite. See [[fanout-silent-wrong-pattern]].
- **Fixture envelope drift:** `spec/fixtures/dr042_orchestration_fanout.jsonl` `source`/causation still differ from the
  shipped path. No reducer reads either; the edge is faithful, the envelope approximate.
- **`crates/rezidnt-tui/examples/board_rich.rs:323`** fails WSL clippy (`unnecessary_sort_by`), pre-existing, invisible
  to host clippy (newer WSL toolchain). Same class as [[golden-txt-crlf-host-vet]].
- **OWED (registry-convergence, 2026-07-24): `release_worktree` has no production caller.** DR-007's ratified
  allocate→use→release lifecycle never completes. Every finished run leaks a `notify` watcher plus its detached debounce
  task (daemon-lifetime), leaves the tree on disk, and leaves the sole-allocator registry entry open — live claims only
  grow. Not a one-liner: releasing emits `worktree.released`, which the S4 reducer folds to `status = "released"` OVER
  the `"merged"` `diff.merged` just set, so wiring it in without ruling on what a merged-then-released worktree reads as
  trades one derived-state regression for another. Also needs the `WorktreeId` threaded into `RunTaskContext` (it carries
  only the path) and an answer for whether a FAILED run's tree survives for triage. Recorded at the site: end of
  `drive_run`, `bins/rezidentd/src/runs.rs`. The watch outliving the run is not academic — it is what made the
  post-merge `diff.ready` clobber reachable.
- **OWED: stale caveat in `crates/rezidnt-mcp/tests/gate_explain.rs:10-15`.** It says the ontology "ratifies no v1
  payload baseline" for `gate.entered` / `gate.failed` / `gate.inconclusive` / `gate.explained` and that warden
  ratification is required before a richer shape is frozen. **Confirmed false** — all four are ratified in
  `spec/ontology.md`, "S3 set (ratified 2026-07-17)", one section each. The correction to write is **"all FIVE
  `gate.*` subjects are ratified — four in the S3 set, `gate.passed` in the S4 set"**: the S3 note does defer
  `gate.passed` ("no S3 emitter or pin … S4 scope"), but the S4 set then ratifies it outright ("the S4 engine is
  that emitter. Now ratified."). Reading only the S3 deferral and stopping there is how an earlier draft of this
  very bullet seeded a fresh false caveat inside the correction of a false caveat — the third-order instance of
  this arc's own defect class. Same stale-caveat class the warden already ruled on for
  `worktree_conflict.rs` and `diff_ready.rs`. Left uncorrected only because the file is outside the
  registry-convergence slice's diff; it is a comment fix, no code or assertion changes.

## Decisions still needing a /dr
None outstanding. DR-046 §Decision 8 already **fixes the brief** for the next slice, so it needs a slice, not a record.

## What's next (owner's steer — nothing forced)
**DR-046 §Decision 8 — the registry-convergence slice**, whose brief is fully specified in that record: converge BOTH
allocation paths (fan-out AND ordinary), because a registry seeing only fan-out cannot guard a fan-out racing an ordinary
spawn. Known landmines, all verified: `rezidentd` has **no dependency on `rezidnt-adapter-git` at all**; `WorktreeReq`
has **no principal field** and `alloc_worktree` hardcodes `"rezidnt"`; the adapter's `emit` **broadcasts on a channel
with a `None` workspace envelope instead of appending to the fabric** (naive repoint ⇒ the allocation fact drops off the
log, I3) and emits its own `worktree.allocated` (⇒ double-emit); the two path layouts differ (on-disk change). It also
owes the **injectable allocation seam** — DR-044 §Consequences (e)'s I6 conflict test cannot be written black-box without
one (paths are ULID-derived, so no test can pre-claim a path) — and a **distinct conflict refusal code** (everything
collapses to `spawn.failed`). Canaries: `golden_path.rs`, `open_flow.rs:63`, `s2_worktrees.rs`, the adapter's 5 suites.
Alternative arc: the **owned terminal substrate**, the other Phase-3 line and the actual herdr-removal endgame.

## Environment (essentials)
Host `/vet` = `bash .claude/hooks/vet.sh` (definition-of-done; green at `459ef6d`). Pure-logic/projection/schema tests are
host-lintable; `*_e2e.rs` is `#[cfg(unix)]` → WSL ([[wsl-dev-environment]], [[vet-is-host-side-wsl-insufficient]]).
Host+WSL **sequential** ([[vet-concurrency-flake]]). **Read a record's premises against the tree before building to it**
— this arc produced four defects of one class, none test-catchable ([[fanout-silent-wrong-pattern]]).

---
**NEXT ACTION → DR-044 slice COMPLETE and closed (Phase 3 opened; DR-044/045/046 accepted; `fan_out` shipped; the
lead→sub edge is the field `agent.spawned.lead_run?`, NOT the badge-matching derivation DR-042 assumed). Final /debrief
PASS + host /vet green at `459ef6d`. NO forced next — owner's steer; strongest candidate is the DR-046 §Decision 8
registry-convergence slice, whose brief is already fixed in that record. High autonomy ON.**
