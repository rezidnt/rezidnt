> Index: [§20 of the plan](../rezidnt-architecture.md#20-decision-records) · plan §7 (run substrate — the `AgentSubstrate` seam this record finally makes real) · §8 (gates — the adjudicators; I6) · §5/§6 (append-only fabric, pure folds — I3 makes history free) · builds on [DR-022](DR-022-benchmark-harness-slice.md) (the collate fold), [DR-044](DR-044-live-lead-sub-fanout.md)/[DR-046](DR-046-fanout-limits-edge-and-registry-deferral.md) (per-sub worktree isolation), [DR-047](DR-047-registry-convergence-landed.md) §Decision 6 (the release-lifecycle slice this arc must not collide with) · invariants I3, I4, I6

# Decision Record DR-048 — Trials: a verifier-adjudicated trial matrix over harness × model × agent-spec variants

**Date:** 2026-07-25
**Status:** ACCEPTED (ratified under the standing autonomy grant. The owner made the three shaping calls in conversation 2026-07-24/25 — the name, the v1 selection mode, and the v1 comparison scope; the record itself mints no invariant and touches no ontology. Flagged rather than assumed, so the owner can overturn it knowingly.)
**Amends:** §7 — the `AgentSubstrate` trait, prose-only until now (doc comments at `crates/rezidnt-run/src/spec.rs:59` and `bins/rezidentd/src/runs.rs:682`; the concrete `ClaudeCodeAdapter` at `crates/rezidnt-run/src/adapter.rs:63` is the only implementation and `SUPPORTED_HARNESSES` at `bins/rezidentd/src/runs.rs:685` names only `"claude-code"`), becomes a real trait per I4. Adds a `model` field to `AgentSpec` (no such field exists anywhere on `AgentSpec`, `SpawnPlan`, `FanOutTask`, or any event payload today — verified by workspace grep). Mints NO invariant, NO subject (two are QUEUED for a warden `/subject`, Decision 5), NO badge, NO PDP path. Adds a §20 index row.

## Context

Three facts in the tree make a trial matrix cheap, and one fact blocks it. Cheap: (i) fan-out (DR-044/046) already gives every sub-run an isolated worktree through the sole-allocator registry; (ii) `agent.completed` already carries tokens and `duration_ms` on the wire (`crates/rezidnt-run/src/adapter.rs:188-192`) — nobody collates them; (iii) the DR-022 collator (`bench/harness/src/lib.rs:540`) is a pure fold over the recorded log, so historical comparison and a leaderboard are just another fold — I3 makes history free. Blocked: `FanOutTask`'s REQUIRED per-task `idempotency_key` (`crates/rezidnt-types/src/mcp.rs:275-281`) deliberately dedupes a retried task to the SAME run — correct for fan-out, fatal for N-samples-of-one-task. And the substrate axis does not exist: one hardcoded harness, no trait, no model selector.

**Strongest counterargument (recorded, not resolved away):** best-of-N is N× the cost for one merged diff, and the obvious "cheap" fix — an LLM integrator that synthesizes the best parts of N candidates — is both more valuable and more dangerous. It was argued for and is deferred, not rejected: v2 may add integrator-synthesis, but its output goes through the same vet as any candidate, because under I6 an LLM is never judge-of-record. The judge is the verifier set, deterministic and interrogable; anything else is vibes with a leaderboard. Naming dissent: "arena" was rejected for minting lore past the vet/debrief/dossier cap; "bench" for colliding with the DR-022 bench harness. **Trials** — a plain word — was ratified.

## Invariant posture

**I4 — the point of slice A.** The trait exists in prose; extraction makes it real, with claude-code staying green and a second implementation (codex CLI, `codex exec --json`) proving the seam is a seam. **I3 — load-bearing.** Trial scoring and the leaderboard are pure folds over facts already on the log; nothing is a source of record but the log. **I6 — load-bearing.** Candidates are adjudicated by the existing gate verifiers; verdicts are `pass|fail|inconclusive`, never coerced, and the winner is the best PASSING diff — an inconclusive candidate never wins. **I1, I2, I5, I7, I8 — untouched.** No intel memo is cited: no competitor source informed this record.

## Decision

1. **Open the Trials arc:** one task run across N candidate variants (harness × model × agent-spec/skills/capabilities), each in an isolated worktree, adjudicated by verifiers, with historical comparison and a best-of-N selection gate.
2. **v1 selection mode is winner-take-all.** Verifiers score all candidates; the best passing diff merges; the rest are released. Integrator-synthesis is v2, gated by the same vet.
3. **v1 comparison axes are verdicts + wall time only.** Cross-vendor cost/token normalization is deferred — tokens are collated (slice C) but not compared across vendors, because a Claude token and a Codex token are not the same unit and pretending otherwise would fabricate a metric.
4. **Slices:** **A** — extract `AgentSubstrate` (claude-code green throughout) + codex CLI adapter + `model` field on `AgentSpec`. **B** — trial matrix primitive: N samples × variants over one task, scored via existing gate events (this is where the idempotency design must answer mcp.rs:275 — distinct sample identities, no new dedup mechanism weakened). **C** — collator v2: tokens, wall time, verdict tallies + leaderboard fold atop `bench/harness`. **D** — winner-take-all selection gate.
   > **A** closed out by [DR-050](DR-050-trials-slice-a-closeout.md). **B** instantiated by [DR-055](DR-055-trials-slice-b-matrix-primitive.md) (ACCEPTED 2026-07-26), which also discharges this record's §Consequences binding clause on the idempotency design — in its plain words, the dedup rule is unchanged and only its key space is extended.
5. **Ontology untouched here.** `trial.opened` / `trial.scored` are QUEUED for a warden `/subject`; this record names them and mints nothing.
6. **Parallel-build file boundary (binding on the lanes):** slice A phase 1 stays inside `crates/rezidnt-run` behind an unchanged constructor API, so it can run concurrent with the worktree-release-lifecycle slice (DR-047 §Decision 6), which owns `bins/rezidentd/src/runs.rs`. Daemon wiring lands only after that slice merges.

## Consequences

- **Roadmap.** §20 gains this row; the arc queues behind/alongside the release lifecycle per Decision 6. One warden `/subject` owed (Decision 5).
- **Risk register.** ADDS: (1) a second harness adapter doubles the CLI-churn surface `harness_version` pinning exists for; (2) N-sample spawning multiplies the per-allocation watcher/registry leak DR-047 already carries — the release-lifecycle slice becomes load-bearing for Trials at any real N; (3) the sample-identity design in slice B touches spawn dedup — any weakening of the idempotency discipline must be stated in that slice's record, in plain words.
- **Test/criterion honesty.** This record weakens no test and lowers no bar. Slice A's bar is explicit: existing claude-code suites stay green through the extraction, unmodified.
- **Deferred, named:** integrator-synthesis (v2); cross-vendor cost/token normalization; harnesses beyond codex.

Amendments to this record require DR-050 (DR-049 is the worktree-release-lifecycle record, opened the same day).
