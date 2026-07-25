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

## ► NEXT ACTION — slice B is a DESIGN-FIRST slice: it owes a DR and a `/subject` BEFORE `/oracle`
Nothing is owed on criterion (b). Slice B proper is scoped by **DR-048 §Decision 4**:

> **B** — trial matrix primitive: N samples × variants over one task, scored via existing gate events (this is
> where the idempotency design must answer `mcp.rs:275` — distinct sample identities, no new dedup mechanism
> weakened).

**Its blocker, named in DR-048 §Context:** `FanOutTask`'s **REQUIRED** per-task `idempotency_key`
(`crates/rezidnt-types/src/mcp.rs`) deliberately dedupes a retried task to the SAME run — correct for fan-out,
**fatal for N-samples-of-one-task**. The live mechanism is a per-workspace `spawn_keys: key → RunId` map,
log-derived from `agent.spawned.idempotency_key` (`bins/rezidentd/src/mcp.rs`), so a repeated key returns the
existing run rather than spawning. Slice B must give N samples of one task **distinct** identities without
weakening that. DR-048 §Consequences binds the record: *"any weakening of the idempotency discipline must be
stated in that slice's record, in plain words."*

**Two things do not exist yet and must land before implementation:**
1. **A slice-B DR** — the sample-identity design (a deterministic per-sample key derivation looks like the
   shape that weakens nothing, but that is a proposal, not a ruling), plus the trial primitive's MCP surface.
2. **A warden `/subject`** — `trial.*` subjects are **NOT minted**. `spec/ontology.md` has no `trial.` entry
   and `SUBJECTS_V0` stands at 49. `trial.opened`/`trial.scored` have been carried as QUEUED for several
   sessions; scoring "via existing gate events" may reduce what must be minted, so scope the mint against the
   gate facts that already exist rather than assuming both.

So: `/dr` → `/subject` → `/oracle` → build → `/gauntlet`. What follows is what a slice-B (or slice-C) author
will hit in the code that just landed.

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

## Owner decisions — three RULED this session, two still owed
- **✔ Exit class — RULED, and LANDED as [DR-054](../../docs/decisions/DR-054-contract-violated-debrief-exit-code.md)
  (ACCEPTED, `cf44f01`).** `rezidnt debrief` exits **3** for a run carrying a folded `run.contract.violated`
  record. It joins the EXISTING exit-3 disjunction and **dominates exit 5** — a run that is both
  contract-violated and gate-failing exits 3, because cannot-certify outranks one-gate-failed, mirroring how
  `inconclusive` already dominates `fail`. Amends DR-004; back-pointers applied at §20 and in DR-004 itself.
  `dr050_contract_violated_debrief.rs`'s runner now takes an expected exit class per caller.
- **✔ Projection widening — DEFERRED to slice-C scoping** (owner, 2026-07-25). `RunRow`/`SubRow` still render
  a violated run's turn-1 totals **unmarked**. Recorded in DR-054 §Context 6 and §Deferred so it cannot be
  inherited silently. **The constraint a slice-C author must not miss:** consumer (3) is *required* to treat a
  violated run's accounting as untrusted, and a collator built on `SubRow` as it stands **cannot comply** —
  so slice C either widens `SubRow` (via `/subject`) or reads `AgentRunState` directly and says so.
- **✔ CI lane — RULED (option a) and fixed.** See the section above.
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

## ✔ CI's ubuntu lane: FIXED (`77076b2`, verified green)
Owner ruled option (a). The workflow now grants the capability on the Linux lane only
(`kernel.apparmor_restrict_unprivileged_userns=0`) and **proves the grant took** with an unprivileged
`unshare -Ur` rather than trusting it. First run after the fix: **`completed/success`** — the first green
ubuntu lane this repo has had. Option (b) (degrade a missing sandbox to `inconclusive` per I6's
`could_not_run`) was deliberately NOT taken and the reason is recorded in the workflow: on this runner the
capability is available for the asking, and an inconclusive would forfeit a proof we can actually have. It
remains the right fix for a genuinely capability-less environment. Diagnosis retained below for the next
time it bites.

<details><summary>Original diagnosis (kept — the failure mode will recur on any runner that restricts userns)</summary>
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
</details>

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
