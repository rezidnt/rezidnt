# Handoff — 2026-07-25 (session 31)

## Slice
**`trials-slice-b` — the DR-050 ENTRY GATE IS NOW FULLY MET (a), (b), (c). The slice is UNBLOCKED but NOT
STARTED.** Criterion (b) was HALF-MET at pickup and is closed by this session. `HEAD = 18dedff`, pushed,
**working tree CLEAN**, `origin/main` in sync. High autonomy ON. Nothing running.

Next session's first move is `/slice` — read slice B's ACTUAL criteria (this session only cleared the gate to
enter it). Do not mistake the entry gate for the slice.

## This session (2 commits)
`6a094e9` oracle (red judges) → `18dedff` the arc. Loop ran clean: `/oracle` → implementer → `/gauntlet` ×4.
**The auditor failed this arc THREE times before passing.** That is the story of the session and the reason to
read the next block before touching these files.

## ► NEXT ACTION — `/slice`, then build slice B proper
Nothing is owed on criterion (b). What follows is what a slice-B (or slice-C) author will hit.

### The three qualifications criterion (b) is met UNDER (auditor's words, carry them forward)
1. **The emitter is WIRED-BUT-UNREACHABLE.** `drive_run` constructs `ClaudeCodeAdapter` concretely,
   `AdapterError::ContractViolated`'s sole construction site is `CodexAdapter::map_run_completed`, and
   `SUPPORTED_HARNESSES` is `["claude-code"]` alone. **No daemon path can fire the arm.** Every emitter guard
   is source-text; a daemon publishing the subject with the wrong envelope or timing passes all of them.
   **The slice that lands substrate selection at the `AgentSubstrate` seam owes the first behavioral judge.**
   `spec/ontology.md` now records this as a third emitter status, distinct from both "not wired" and "live".
2. **`the_fallback_reason_is_the_last_line_alone_not_the_refusal` is a SPELLING CHECK** —
   `!contains("contract_violation.or(last_line)")`. A rename routing the same refusal text slips past, and it
   pins nothing about what *does* ride `error.message`. Treat the authorship boundary as UNJUDGED.
3. **Nothing in the tree REFUSES a premise-broken run**, and the sharp form is stronger than "debrief exits 0":
   `contract_refused` is read in exactly ONE place, the stream loop's short-circuit, and never again. A
   contract-violated run still reaps, still chunks its capture, still runs `pre_merge`, **and can still MERGE.**
   Whether a withdrawn stream premise should block a merge is **UNRULED** — no DR or ontology bullet says.
   Consumer (3), the DR-048 slice-C collator, is correctly unbuilt. A slice-C author hits this first.

### READ BEFORE EDITING `bins/rezidentd/src/runs.rs`
- **`last_line = Some(...)` must stay ABOVE the `if contract_refused { continue; }` short-circuit.** It was
  moved below this session and REVERTED: the ontology's `run.contract.violated` Timing bullet rules that a
  pre-completion refusal leaves the DR-051 fallback carrying *the last stream line*, and the move narrowed
  that ratified semantics. It is now pinned by an assertion that cites the bullet and demands a `/dr` rather
  than an edit. The pin holds against a MOVE, **not against an ADDITION** (first-occurrence `str::find`).
- The emitter guards match over a **comment-stripped** substrate. `strip_comments` is **not a lexer**; both of
  its holes fail toward GREEN (re-admitting prose), so its precondition over `runs.rs` is ASSERTED, for two of
  four spellings. **If that assertion goes red, the cheapest correct fix is to use `//` comments in `runs.rs`
  — do not patch the stripper and do not relax the assertion.**

## Owner decisions owed
- **Exit class (NEW, from this arc).** A contract-violated run with clean gates **exits 0**. Should it be
  **exit 3**? The `integrity.alarm` analogy is strong — that earns 3 precisely because a recorded verdict can
  no longer be trusted, and this is the same species one axis over (the run's *accounting*). A CI caller
  gating on exit code sees a violated run as clean today. Deliberately left alone; `dr050_contract_violated_debrief.rs`
  pins exit 0 and **discloses that it pins the status quo, not a ruling** — it goes red the day a DR rules.
- **Projection widening (NEW).** No projection carries the flag, so `RunRow` (board) and `SubRow`
  (orchestration graph) render a contract-violated run's **turn-1 totals unmarked**. `SubRow` is the sharper
  one: it is the closest thing to a scoring view, and consumer (3) is *required* to treat that accounting as
  untrusted — a collator built on `SubRow` as it stands **cannot comply**. Either `SubRow` carries the flag
  before slice-C scoring, or slice C reads `AgentRunState` directly and the projection says so. `/subject` or `/dr`.
- **DR-053 is DRAFTED, PROPOSED, and still owed a decision** (carried from session 30, untouched). Its
  back-pointers into DR-050 and DR-051 are deliberately NOT applied while PROPOSED. Apply on acceptance.
- **`docs/site/`** — untracked, still undecided: ignore, commit, or delete? (Carried, untouched.)

## The vet flake is DIAGNOSED and FIXED — stop carrying it as unexplained
Three handoffs carried "one unexplained `/vet` flake that never reproduces". It was
`proxy_errors_and_keeps_serving_on_midsession_daemon_loss`, and the defect was in the **harness**:
`fake_mcp_server_dropping` dropped the socket immediately after `write_all`, so Winsock answered the client's
read with an **RST (`os error 10054`)** and discarded the in-flight response — the client then saw a dead
daemon where the fake one had in fact replied. It now half-closes and drains to EOF. Assertions untouched.

**Measured: 3/40 running the WHOLE binary at default parallelism (the `/vet` condition); 0/30 filtered to the
single test; 0/60 after the fix.** The method matters more than the fix: **a filtered re-run cannot reproduce
an in-process-parallelism race.** Re-running one test and seeing green is not evidence — loop the whole test
binary. One residual, recorded so it is not rediscovered: drain-to-EOF turns a proxy that never closes its
socket from a silent pass into a **hang**, and `run_proxy` has no timeout, so that class would wedge `/vet`
rather than return a verdict.

## ⚠ CI's ubuntu lane has been RED since CI landed — diagnosed, NOT fixed (needs a ruling)
`gauntlet (windows-latest)` **passes** on this arc. `gauntlet (ubuntu-latest)` **fails**, and it also failed on
`bcd0db9` before this session touched anything — **pre-existing, not caused by this arc** (host vet pass, WSL
`cargo test --workspace` exit 0). A gate that is always red teaches everyone to ignore it, so this needs
closing early.

**Cause, from the runner log:** the four `crates/rezidnt-run/tests/egress_mediation_c3bc.rs` tests
(`allowlisted_host_is_reached_through_the_proxy`, `credential_injected_upstream_agent_never_sees_it`,
`direct_egress_attempts_reach_nothing_but_the_proxy`, `egress_backend_present_reports_available_and_mediates`)
die in netns/pasta setup with `Couldn't write to /proc/self/uid_map: Operation not permitted` and
`clone: Operation not permitted`. That is **GitHub's ubuntu-24.04 runner restricting unprivileged user
namespaces** (`kernel.apparmor_restrict_unprivileged_userns=1`), not a defect in the code. WSL permits it,
which is why the same tests are green locally.

**The ruling owed — this is an I6 question, which is why I did not just pick one:**
- **(a) Grant the capability in CI**: one step, `sudo sysctl -w kernel.apparmor_restrict_unprivileged_userns=0`
  before the gauntlet. Keeps the tests real and the lane meaningful. Cheapest, and my recommendation.
- **(b) Make a missing sandbox capability INCONCLUSIVE, not FAIL.** The tests currently `expect()` on a
  capability the environment may not provide, so an absent sandbox renders as a failed assertion. The ontology
  already ratifies `could_not_run` as an *inconclusive* reason for exactly this shape — "the verifier could not
  be executed, nothing ran" — and I6 says never coerce. There is a real argument that these tests should
  detect the capability and report inconclusive rather than fail.
- (a) and (b) are not exclusive; (b) is the more principled fix and (a) is the one that restores signal today.

## Named, not closed by this record
- **`spec/ontology.md` was truth-passed via `/subject`** — the emitter's "NOT wired this session" prose was
  false of the tree. Prose only; nothing minted, renamed, or re-versioned; `SUBJECTS_V0` stays 49. The
  `agent.completed.error?` clause's "removal is a named work-order item" sentence was stale too and is corrected
  — that work order is DISCHARGED.
- **The recurring defect class this arc kept producing** ([[fanout-silent-wrong-pattern]]): every one of the
  three FAILs was *prose asserting a mechanism the code lacked* — guards satisfiable by comments, headers
  claiming ASSERT-RED after landing, a fix comment claiming a property the code did not have. **Two are only
  ever caught by mutation-testing your own guard**: delete the code, keep the prose, confirm red, restore.
  Every structural guard in `dr050_contract_violated_surfacing.rs` has now been proven that way. Do that for
  the next one before reporting it.
- **`loopback_post`'s three error contexts said `kill_run`** on the GENERIC helper for every proxied MCP call
  — a failed `gate_explain` named a tool the operator never invoked (I6). Fixed. No sibling site.
- **The malformed-fact residual in the fold**, disclosed at the arm: when the run entry already exists from its
  spawn, a violation fact missing `harness`/`detail` leaves `contract_violated` at `None`, so consumers (2)
  and (3) see a CLEAN run — fail-OPEN on the axis the subject exists to close. Deliberate, matches
  `integrity.alarm`; the log and `counts_by_subject` remain the record.

## Also
- rust-analyzer still reports phantom errors on `ContractViolationRecord` / `contract_violated`; the running
  instance predates the pinned toolchain. **Restart it** — `cargo test` is green.
- `.claude/worktrees/agent-ab4e17a54fbbdb421/` is still an empty dir a Windows handle refused to delete.
  Gitignored, deregistered, harmless. (Carried.)

**Anchor discipline (warden-ratified 2026-07-24):** cite by SYMBOL, not line. A line number is admissible only
bolted to a commit hash.
