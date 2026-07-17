[← Decision records index](../rezidnt-architecture.md#20-decision-records) · [Architecture plan](../rezidnt-architecture.md)

# Decision Record DR-007 — RepoSubstrate BINDING extension: ratify `release_worktree` as-built

**Date:** 2026-07-17 · **Status:** ACCEPTED (owner) · **Amends:** §7 RepoSubstrate trait sketch (BINDING shape). Retroactive: blesses code that shipped and passed S2 debrief. I4 is the axis.

## Context

`release_worktree` extended the BINDING §7 RepoSubstrate sketch with no DR (flagged three times). The sketch listed only `alloc_worktree` and `diff_summary`; the trait as built (`crates/rezidnt-adapters/git/src/lib.rs:165-177`) adds `release_worktree` (the allocate→use→release lifecycle the S2 slice pins) and hard-binds `Result<_, GitError>` into every method. Adding a *method* touches the BINDING *shape*, not just a DEFAULT signature — hence the flags.

## Decision (Option A)

Ratify the three-method trait exactly as built as the §7 RepoSubstrate seam, superseding the two-method sketch:

```rust
pub trait RepoSubstrate: Send + Sync {
    async fn alloc_worktree(&self, req: WorktreeReq) -> Result<Worktree, GitError>;
    async fn diff_summary(&self, wt: &WorktreeId) -> Result<CasRef, GitError>;
    async fn release_worktree(&self, wt: &WorktreeId) -> Result<(), GitError>;
}
```

`release_worktree` is now BINDING; signatures remain DEFAULT ("shape BINDING, signatures DEFAULT" per §7), so the method *set* is fixed but signature tweaks stay note-only. **Deferred (tracked I4 debt):** the trait hard-binds the concrete `GitError`; a future non-git impl would be forced to speak it. The clean shape is an associated `type Error` / shared `RepoError`. With exactly one impl today the abstraction pressure is theoretical, so the fix is deferred to when a second RepoSubstrate impl (or the Phase-3 substrate seam) lands — recorded so it is not dropped a fourth time.

## Consequences

- §7's RepoSubstrate block is replaced with the three-method form above. **No test or behavior changes** — this ratifies passing code.
- Risk-register / I4 delta: one tracked line — "RepoSubstrate hard-binds `GitError`; introduce associated `type Error`/`RepoError` at the second impl." Retires the thrice-carried `release_worktree` `/dr` item.

*Amendments to this record require DR-008.*
