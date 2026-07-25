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
- **`4ceccb7` — the `turn.failed` arm is GREEN.** 5/5 oracle pins, no oracle assertion touched, 145 crate tests
  green, clippy `-D warnings` clean, `cargo check --workspace` clean. **Lane 2 is code-complete and OWES ONLY A
  RE-`/debrief`** (last verdict was on `a559623`; `51b515d` + `4ceccb7` are un-audited).
  Two calls it settled, both worth reading before auditing: **`num_turns` counts a failed turn (reads 1)** while
  its tokens stay absent — the distinction is epistemic, absent tokens mean *never measured*, `num_turns: 1`
  means *we watched one turn begin and end*, and one test asserts both on the same payload so the contrast
  can't silently collapse. And **`turn.failed` DOES trip the single-shot guard** — it counts run-*terminal*
  lines, not successes; two terminal lines of different outcomes make the run's verdict ambiguous, not just its
  cost. Field renamed `completed_turns` → `terminal_turns`; guard pinned in BOTH orders so the policy can't
  drift into being outcome-sensitive.
  `Completion` now carries `usage: Option<TokenUsage>` + `error_message: Option<String>`; `into_fact` omits keys
  rather than nulling them. Tokens are modeled as one present-or-absent unit because that is how the wire
  carries it. The single-sourcing guard was re-verified green by the implementer, not assumed.

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

**OWED — the arm is now green (`4ceccb7`), so this is unblocked.** Amend DR-050: flip Decision 3 to
resolved-by-recording, strike risk item 5, and correct its "residual" claim, which the oracle showed was half
wrong — the daemon fallback does fire, but it **discards the recorded failure reason**, and once `turn.failed`
maps in-crate the fallback stops firing on this path entirely. Then the record can go ACCEPTED.

**End-to-end absence is verified, not assumed.** The implementer checked the reducer *before* changing the fact
shape: `rezidnt-state` reads these via `payload["cost"]["input_tokens"].as_u64()`, which yields `None` for a
missing key, so absent tokens fold to `None` rather than `Some(0)`. The absence survives into derived state
intact — which is what makes the "a failed candidate's cost is absent, not free" reasoning hold all the way to
DR-048 slice C's leaderboard, rather than only at the adapter boundary.

## ► ONE `/subject` SESSION NOW OWES FOUR THINGS — batch them, don't drip
The warden pass is the critical path for the current slice AND the cleanup queue for two arcs. In one session:
1. **`gate.failed.worktree?`** — additive optional, present iff the gate ran against an allocated tree
   (**BLOCKING lane 1's last two tests**; rationale and the unsoundness of the alternative are above).
2. **`agent.completed.error.message?`** — lane 2's `turn.failed` arm now emits it. Additive payload evolution
   the fabric rules permit, and the oracle pinned its location, but `spec/ontology.md`'s copy of the subject
   does not describe it. Flagged by the implementer rather than touched (the file is hook-blocked to it).
3. **`spec/ontology.md:94` and `:349`** — both assert the reducer folds `status = "merged"`. False after DR-049.
4. **DR-047 §Decision 4's queued pass** — the `diff.ready` emitter cell + `source` attribution. DR-049 §Decision 6
   already rides it with this slice.

## NEXT ACTION → in order
1. **`/debrief` lane 2** on `4ceccb7` (its `a559623` verdict predates two commits). It is otherwise done.
2. **`/subject`** — all four items above.
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
