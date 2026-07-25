# Handoff — 2026-07-25 (session 28: both lanes built, one blocked on a `/subject`, DR-050 opened and then settled by recording)

## State of play
`current-slice` = **`worktree-release-lifecycle`** (DR-049). Main is clean and pushed at **`cc8c869`**.
High autonomy ON. **Neither lane is merged** — neither has passed the serialized `/vet` gate.
One agent was IN FLIGHT at close (lane 2 implementer, see below) — verify its worktree before trusting this file.

## What landed on main this session (2 commits, pushed)
- **`a0e74a2` — DR-050 (PROPOSED)** `docs/decisions/DR-050-trials-slice-a-closeout.md` + §20 index row.
- **`cc8c869` — gate integrity.** `board_rich.rs:323` `unnecessary_sort_by` fixed (**main's own `/vet` was red
  on `--all-targets` independently of either lane** — the memory note called this a "WSL wart"; it was a host
  gate failure). Plus `rust-toolchain.toml` pinning **1.97.1** (owner-authorized this session).

## Lane 2 — DR-048 Trials slice A (worktree `.claude/worktrees/agent-ab4e17a54fbbdb421`)
Boundary BINDING per DR-048 §D6: `crates/rezidnt-run` only. Held all session (verified by `git diff --stat`).
- `a559623` — **PASSED `/debrief`.** `AgentSubstrate` is a real dyn-safe trait with two live impls; `model`
  threaded through spec/spawner/vet-preimage; 138 crate tests green, clippy `-D warnings` clean.
  Remediated a fail verdict first: a turn-scoped→run-terminal `agent.completed` (I3), a vacuous version gate,
  two false doc claims, and a single-sourcing claim that was true by neither construction nor test.
- `51b515d` — oracle red board for the **`turn.failed` arm** + a REAL new fixture
  `spec/fixtures/transcripts/codex_exec_v0.145.0_turn_failed.jsonl` (recorded this session, codex-cli 0.145.0).
- **IN FLIGHT at close:** the implementer was mapping `turn.failed`. **Check `git log` in that worktree first.**
  If it committed, it owes a re-`/debrief`; if not, re-send the work order (5 red tests, all assertion-red).

## Lane 1 — DR-049 release lifecycle (worktree `.claude/worktrees/agent-a01bfc6c4807a2b3b`)
`a63e580`. **14 of 16 green.** Worktree clean. Both reds are the same assertion: `outcome = failed`.
Everything else in criterion (c) passed on a real daemon under WSL — failed tree on disk, registry claim open,
`lifecycle = allocated`, MCP release honored, tree gone, claim closed, `worktree.released` ordered after
`gate.failed`, `lifecycle = released`. **Only the attribution is missing.**

### ► THE BLOCKER — owner/warden, this is the next action
`outcome = failed` has **no attributing fact**. `gate.failed` v1 carries `{run, gate, verifier, evidence, inputs}`
— no worktree. The implementer stopped rather than guess. **Route A (correlation join) is VERIFIED UNSOUND:**
`runs.rs:687` mints ONE correlation for every `[[agent]]` in a spec, and `mcp.rs:361` does the same for fan-out
("ONE correlation for the whole fan-out"). So one correlation spans N runs and N worktrees, and a join would
attribute one sub's failure to **all its siblings' trees** — reachable today, and exactly the silent-wrong class
this arc produced eleven of. It also needs a correlation→path index resident in the `Graph`, which
`spec/ontology.md:248` already rejected as "the stored-derivation smell".

**Proposed `/subject` (narrower than DR-049 anticipated):** additive optional `worktree?: string` on `gate.failed`,
present **iff** the gate ran against an allocated tree. `gates::run_gate` already receives that path as its `cwd`
(DR-041), so the emitter has the value with zero plumbing. Absent on `vet`/`permit` gates. `v` stays 1; one new
reducer arm. **Two ontology prose lines need the same pass** — `spec/ontology.md:94` and `:349` assert the reducer
folds `status = "merged"`, now false. DR-049 §Decision 6 already queues a `/subject`; these ride it.

### Lane 1 calls that need the auditor's eye at `/debrief`
- **It added a defaulted method to the MCP-internal `McpSubstrate` trait** despite DR-049's banner saying
  "NO trait method". Its argument: that seam isn't one of the four §7 substrate traits I4 makes BINDING, and
  "exposed MCP-first" is unreachable without it. Self-flagged. **Adjudicate rather than wave through.**
- `Daemon::allocations`, a path-keyed handle table, is **process-lifetime** — a tree allocated before a restart
  isn't releasable via MCP. Refuses loudly (`worktree.unknown`), never silently succeeds. Disclosed.
- Release call site is `run_pre_merge`'s `Verdict::Pass` arm — failed trees retained by *control flow*, not a
  conditional. Board door is DUAL (operator **or** lead) per §Decision 3's literal words.
- `registry_convergence_e2e.rs` kills the daemon on `agent.completed`, which now races a merge+release that
  deletes the tree its `exists()` assertions read. Large margin, passed under WSL — but it's a NEW way to lose
  that race. Weigh it.
- `crates/rezidnt-tui/tests/dr049_board_split_render_note.rs` **does not exist** — board 1's header names it;
  nothing was ever written there. Covered in `board_render_golden.rs`'s header instead.

## DR-050 — PROPOSED, and its contested item is now SETTLED BY EVIDENCE
Owner directed "record as failing". Done: `codex exec --json --skip-git-repo-check -m <bogus>` recorded.
**A failing codex turn emits `turn.failed`, NOT `turn.completed`** — so §Decision 3's contested mapping resolves
the *implementer's* way and the auditor's mismapped-failure branch (risk-register item 5) is **refuted for
codex-cli 0.145.x**. The recording also proved the `item.completed` type-`error` items are noise (the failing run
carries two, including the same one the successful probe carries), and that **`turn.failed` carries no `usage`
object at all** — a failed candidate's cost is ABSENT, not zero, which DR-048 slice C must honour or the
leaderboard reads a failed run as free.

**OWED:** once lane 2's `turn.failed` arm is green, amend DR-050 — flip Decision 3 to resolved-by-recording, strike
risk item 5, and correct its "residual" claim, which the oracle showed was half wrong: the daemon fallback does
fire, but it **discards the recorded failure reason**. Then the record can go ACCEPTED.

## NEXT ACTION → in order
1. Check lane 2's worktree `git log` — resolve the in-flight implementer (re-`/debrief` if it committed).
2. **`/subject` with the warden** for `gate.failed.worktree?` + the two stale ontology prose lines. This unblocks
   lane 1's last two tests. It is the critical path for the current slice.
3. Lane 1 implementer finishes to 16/16, then `/gauntlet`.
4. **`/vet` ONE LANE AT A TIME** ([[vet-concurrency-flake]]). Merge order: lane 2 first — it is crates-only and
   merges independently; DR-048 §D6 holds daemon wiring for Trials until lane 1 merges.
5. Amend DR-050 per above, then ratify.

## Open, non-blocking
- DR-050's three `runs.rs` traps are slice-B **entry criteria** now, not backlog. Memory:
  `pep-stamp-decoupled-from-interception`.
- **No spawn in the tree is version-gated** — `version_gate` has zero non-test callers. Dormant while the daemon
  admits only `"claude-code"`; live the day codex is wired.
- `.claude/worktrees/` is **not gitignored** — a `git add -A` on main would swallow both worktrees. Worth a line.
- Owner question still UNANSWERED: "what happens when I'm out of fable?"
- DR-049's two most-contestable settlements stand; DR-046 carry-overs unchanged.
