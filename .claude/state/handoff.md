# Handoff — 2026-07-25 (session 29)

## Slice
**`worktree-release-lifecycle` (DR-049) is DONE — 16/16, merged, pushed.** So is **DR-048 Trials slice A**.
Both lanes landed this session; both worktrees removed, both branches deleted. **`HEAD` = `d91a7ea`** (not
`da17699` — the handoff commit followed the merge), and the working tree is **DIRTY, ~32 paths, none of this
session's later work committed**. `current-slice` advanced to **`trials-slice-b`**, which is **entry-gated**
(below).
High autonomy ON. Nothing running.

## This session (23 commits)
Lane 2 (Trials slice A) merged at `554bad5`; lane 1 (DR-049) at `da17699`. Both passed host `/vet` + `/debrief`;
DR-049's four `cfg(unix)` boards verified green under WSL, since host cargo compiles them to nothing.
Also: toolchain **pinned to 1.97.1** (`rust-toolchain.toml`) after the host sat 4 releases behind WSL while
being the gating side; `.vscode/settings.json` added (rust-analyzer → clippy `--all-targets -D warnings`, own
target dir, worktrees excluded); `.claude/worktrees/` gitignored.

## ► NEXT ACTION — slice-B criteria (a) and (c) DONE; **(b) is HALF-MET and owes real work**
State: `HEAD = d91a7ea`, working tree dirty (~32 paths) — **none of this session's work is committed.**
Criteria are DR-050 §Decision 2, with (c) sharpened by DR-051 §Decision 4. Verified by symbol, not by line:

1. **(a) DONE** — `pep_enforced = plan.permit_hook_config().is_some()`, keyed on the plan, not the gates list.
   See [[pep-stamp-decoupled-from-interception]].
2. **(b) HALF-MET — DO NOT TREAT AS DONE.** The *exclusion* arm is built:
   `matches!(e, AdapterError::ContractViolated { .. })` is out of the tolerant garbage-line warn arm. But
   criterion (b) requires the refusal **surface as a fact-worthy failure**, and today it surfaces to `tracing`
   and nothing else — `contract_violation` is a write-only variable. Still owed:
   - an **emitter** for `run.contract.violated` — the subject is warden-ratified and minted in
     `crates/rezidnt-types/src/taxonomy.rs` + `spec/ontology.md`, and exists **nowhere else in the workspace**.
     The ontology marks the emitter "implementer scope, work-ordered, NOT wired this session."
   - the **fold** — `AgentRunState.contract_violated: Option<ContractViolationRecord>` with a
     `"run.contract.violated"` match arm, plus its three named consumers (fold, `rezidnt debrief` dossier,
     and the DR-048 slice-C collator, which MUST treat a violated run's accounting as untrusted).
   - **removal** of the non-conforming arm: `contract_violation.or(last_line)` routes rezidnt-authored refusal
     text into `agent.completed.error.message`, which the ratified authorship boundary forbids (that field
     carries harness-authored text ONLY). Its removal is a named work order, not a clause widening.

   **READ THIS BEFORE PLANNING (b) — it determines HOW it can be built.** `drive_run` constructs
   `ClaudeCodeAdapter` **concretely** (`ClaudeCodeAdapter::new(ctx.run)`), and `AdapterError::ContractViolated`
   has exactly **one** construction site in the workspace: `CodexAdapter::map_run_completed`
   (`crates/rezidnt-run/src/adapter.rs`). So the exclusion arm **cannot fire on any daemon path at all** — a
   stronger unreachability than the `completed_id` one, and it does not depend on the fallback's guard.
   Consequence: **"build (b) oracle-first" is NOT achievable as an e2e behavioral oracle until substrate
   selection lands** — there is no way to drive a `ContractViolated` through the daemon while the adapter is
   hardcoded. The seam the oracle must test at is **adapter selection in `drive_run`** (the `AgentSubstrate`
   trait boundary DR-048 slice A extracted); until that seam is live, (b)'s judge can only be structural.
   Both `crates/rezidnt-run/src/adapter.rs`'s module header and
   `bins/rezidentd/tests/dr050_contract_violated_surfacing.rs` already say this — that test explicitly
   discloses itself as "a containment backstop — naming the variant is necessary, not sufficient; the
   behavioral judge lands with the daemon-side codex wiring." Do not mistake it for the behavioral oracle.
3. **(c) DONE — but DONE-UNDER-WSL, not under the host gauntlet.** The fallback `agent.completed` literal
   emits `"cost": {"total_usd": 0.0}` only (token keys OMITTED, not zeroed) and carries the harness's
   `error.message`; the cross-crate FAILURE-shape pin is
   `bins/rezidentd/tests/dr051_fallback_completion_fidelity_e2e.rs`. **That file is `#![cfg(unix)]`, so the
   host gauntlet compiles it to nothing** — "commit + `/gauntlet`" on Windows does NOT exercise this pin.
   Run it WSL-side before treating (c) as proven. (Same class as the DR-049 watcher-drop leg;
   see [[watcher-behavior-wsl-only]] and [[vet-is-host-side-wsl-insufficient]].)
   **Stale doc in that file, flag not fixed:** its module header still declares "All three tests are
   ASSERT-RED today … hardcodes zeroed token keys and carries no `error` key at all", which the tree no
   longer matches — a reader following this handoff's own citation reads the opposite of the claim. Retire
   that header when the arm is next touched; it was left alone deliberately this session rather than edited
   by a docs pass with no test to re-run.
   `bins/rezidnt/src/main.rs`'s `debrief` cost block also builds per-key under `if let Some(...)`
   (DR-051 §Decision 5's null-leak) — **LIVE at `d91a7ea`, fixed by the commit that lands this handoff.**

**Next action: commit + `/gauntlet` what exists, then build (b)'s surfacing arm oracle-first.** The gauntlet was
legitimately red mid-remediation; do not treat any earlier green as evidence about this work.

**Anchor discipline (warden-ratified 2026-07-24, `spec/ontology.md`):** cite by SYMBOL, not line — the prior
version of this block cited five line numbers and NONE of them resolved to the construct named. A line number
is admissible only bolted to a commit hash.

## Owner decisions owed
- **DR-051 and DR-052 are both ACCEPTED (owner, 2026-07-25) — this item is CLOSED.** DR-052's ledger is
  **five** undisclosed mints, not four: the fifth (`gate.failed.worktree?`, an additive-optional FIELD on a
  ratified v1 subject) was found by `/debrief` on the ratification pass itself. Back-pointers are applied into
  DR-049 and DR-039; DR-049's false "Mints NO trait method" banner is struck in place at all three sites it
  stood at (§Amends, §Status, its §20 row), original wording preserved for audit.
- **DR-053 is DRAFTED, PROPOSED, and owed a decision.** It discloses a SIXTH undisclosed mint —
  `agent.completed.error?`, minted on the DR-048 lane for the `turn.failed` arm DR-051 ratifies, disclosed in
  no record in `docs/` — and corrects DR-050 §Deferred's falsified "no ontology change is expected" prediction.
  **Its back-pointers into DR-050 and DR-051 are deliberately NOT applied while it is PROPOSED**, the same
  discipline DR-052 applied to itself. Apply them on acceptance.
- **`docs/site/`** — untracked, still undecided: ignore, commit, or delete?

## Named, not closed by this record
- **`rezidnt debrief`'s cost block** (in `bins/rezidnt/src/main.rs`) serialized an unfolded `Option` as JSON
  `null` — **LIVE at `d91a7ea`, fixed by the commit that lands this handoff**, which builds a
  `serde_json::Map` per key under `if let Some(...)` so an absent field is omitted. Predates this arc;
  DR-051 §Decision 5 also enters it as a **slice-C** criterion (the collator reads the same fields and would
  inherit it at a higher-stakes surface), so slice C still owes the collator half.
- **Ordering exposure:** the `gate.failed` fold is gate-name-agnostic, so a future post-merge gate carrying a
  worktree could overwrite `merged` with `failed`. No guard — a `gate == "pre_merge"` filter would narrow the
  ontology from the reducer. Recorded at the `gate.failed` arm in `crates/rezidnt-state/src/lib.rs` + DR-052 §Decision 4.
- **Post-restart:** `Daemon::allocations` is process-lifetime, so a retained failed tree is unreachable by the
  only release door after a restart. Refuses loudly (`worktree.unknown`), never silently.

## Also
- **One unexplained `/vet` flake**: a single `test` failure on lane 1 that did not reproduce in two subsequent
  full runs; the failing test was never captured. Matches [[vet-concurrency-flake]] but is NOT diagnosed.
- rust-analyzer needs a **restart** to pick up the pinned toolchain — the running instance predates it.
- `.claude/worktrees/agent-ab4e17a54fbbdb421/` is an empty dir a Windows handle refused to delete. Gitignored,
  harmless, deregistered from git.
- Ontology follow-through: the warden left `gate.failed.worktree?` (~:330) saying wiring/folding "are DR-049
  lane-1 implementer scope, not written here" — historical, not false, but stale now that the fold exists.
