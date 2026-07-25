# Handoff — 2026-07-25 (session 28)

## Slice
`current-slice` = **`worktree-release-lifecycle`** (DR-049). **14 of 16 criteria green.** Both reds are the same
assertion — `outcome = failed` — blocked on one ontology decision (below). Main clean+pushed at **`67547f5`**.
High autonomy ON. Nothing running. Both worktrees clean.

## Main this session
`a0e74a2` DR-050 (PROPOSED) + §20 row · `cc8c869` toolchain pinned to 1.97.1 (owner-authorized) + `board_rich.rs:323`
clippy fix — **main's own `/vet` was red on `--all-targets`**, independent of either lane; memory had it filed as a
WSL wart, it was a host gate failure · `4d44c3e`/`67547f5` handoffs.

## Lane 2 — DR-048 Trials slice A · `agent-ab4e17a54fbbdb421` · `4ceccb7`
**CODE-COMPLETE. Owes only a re-`/debrief`** (last verdict was `a559623`; `51b515d`+`4ceccb7` un-audited).
`AgentSubstrate` is a real dyn-safe trait, two live impls, `model` threaded through; the `turn.failed` arm maps.
145 crate tests green, clippy `-D warnings` clean, `check --workspace` clean. Boundary (`crates/rezidnt-run` only,
DR-048 §D6) held all session — verified by `git diff --stat`, not by report.

## Lane 1 — DR-049 release lifecycle · `agent-a01bfc6c4807a2b3b` · `a63e580`
Released-at-merge, the `lifecycle`/`outcome` split, and the explicit MCP release all work on a real daemon under
WSL. **Only failure attribution is missing.** The implementer stopped rather than guess, correctly:
`gate.failed` carries no worktree, and the correlation join is **verified unsound** — `runs.rs:687` and
`mcp.rs:361` each mint ONE correlation per *spec*, so it spans N runs and N trees; joining on it would attribute
one sub's failure to all its siblings. Reachable today.

## ► NEXT ACTION — one `/subject` session, four items, batched
1. **`gate.failed.worktree?`** — additive optional, present iff the gate ran against an allocated tree.
   `run_gate` already has the value as its `cwd` (DR-041). **This unblocks lane 1's last two tests.**
2. **`agent.completed.error.message?`** — lane 2's new arm emits it; the ontology doesn't describe it.
3. **`spec/ontology.md:94` and `:349`** — both still assert the reducer folds `status = "merged"`. False post-DR-049.
4. **DR-047 §D4's queued pass** — `diff.ready` emitter cell + `source` attribution.

Then: `/debrief` lane 2 on `4ceccb7` → lane 1 to 16/16 → `/gauntlet` → **`/vet` ONE LANE AT A TIME**
([[vet-concurrency-flake]]) → merge lane 2 first (crates-only, independent; §D6 holds Trials daemon wiring until
lane 1 lands).

## Open `/debrief` findings
- Lane 1 self-flagged: it **added a defaulted method to the MCP-internal `McpSubstrate` trait** though DR-049's
  banner says "NO trait method" (argues that seam isn't one of the four §7 traits I4 binds). **Adjudicate.**
- `Daemon::allocations` is process-lifetime — a tree allocated pre-restart isn't MCP-releasable. Refuses loudly
  (`worktree.unknown`), never silently. Disclosed.
- `registry_convergence_e2e.rs` kills the daemon on `agent.completed`, now racing a merge+release that deletes the
  tree its `exists()` assertions read. Wide margin, WSL-green, but a NEW way to lose that race.
- `crates/rezidnt-tui/tests/dr049_board_split_render_note.rs` **does not exist** despite being cited in a board header.

## Needs a `/dr`
- **DR-050 is PROPOSED and now amendable.** The owner's "record as failing" call settled its contested item by
  evidence: a failing codex turn emits **`turn.failed`, never `turn.completed`** — the mapping is sound and the
  auditor's mismapped-failure branch is refuted for 0.145.x. Amend: flip Decision 3 to resolved-by-recording,
  strike risk item 5, correct the "residual" claim (the daemon fallback fires but **discards the failure reason**).
  Then ACCEPT. Bonus the recording forced: `turn.failed` carries **no `usage`** — a failed candidate's cost is
  *absent*, not zero, and it folds to `None` end-to-end (verified in the reducer), or slice C scores a failed run
  as free.
- DR-050's three `runs.rs` traps are slice-B **entry criteria**, not backlog (memory: `pep-stamp-decoupled-from-interception`).

## Also open
No spawn in the tree is version-gated — `version_gate` has zero non-test callers; dormant until codex is wired ·
`.claude/worktrees/` is **not gitignored**; a `git add -A` on main would swallow both worktrees · owner question
unanswered: "what happens when I'm out of fable?"
