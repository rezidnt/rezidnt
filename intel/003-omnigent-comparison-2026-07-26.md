# Intel memo 003 — Omnigent vs rezidnt, refreshed capability comparison
**Date:** 2026-07-26 · **Analyst session** · **DR-002 scoped read**

Scope: a refresh of memos [001](001-omnigent-permission-governance.md) (2026-07-17, permit/policy axis) and
[002](002-omnigent-capability-comparison.md) (2026-07-24, the other five fronts). This is **capability-extraction
for positioning / benchmark / risk-register only** (DR-002 rule 2) — not trait, ontology, or design input.
Design-first is satisfied: every rezidnt front cited below is committed in `docs/rezidnt-architecture.md` and the
DR set through DR-054. Two things changed since memo 002 that this memo exists to capture: (a) Omnigent shipped
four point releases (v0.3.0–v0.6.0) in the interim; (b) rezidnt landed the registry-convergence slice (DR-047),
opened and partly closed the Trials arc (DR-048/050/051), and named the worktree-release lifecycle an **open**
slice (DR-047 Decision 6) — so several verdicts in memo 002 have moved, in both directions. **No competitor source
code was read** — only the public GitHub releases page, Databricks blogs/docs, and search aggregators, the latter
flagged and down-weighted.

> **Correction pass, 2026-07-26 (same day, later session).** Finding F3, verdict-table row 7, and the Benchmark
> and Risk-register bullets in §Implications (all downstream of F3) were amended in place — original text
> struck, not deleted, with a bold **CORRECTED** note attached, per the house pattern DR-049's Status field uses.
> **What was wrong:** F3 asserted rezidnt "has not yet closed" the worktree-release
> lifecycle, citing DR-047 (2026-07-24) Decision 6 as an open slice. That was true when written but was
> superseded the very next day: [DR-049](../docs/decisions/DR-049-worktree-release-lifecycle.md) (ACCEPTED
> 2026-07-25) opened and closed DR-047 §Decision 6 — release-at-merge is live in production, merged
> 2026-07-25 (commit `da17699`, "merge lane 1: DR-049 worktree release lifecycle, 16/16"). **Methodology note
> for future intel reads:** this memo verified its rezidnt-side claim against the highest-numbered DR that
> discussed the topic at the time (DR-047) rather than against the git log or the tree, and so missed a record
> that landed the day before the read. Ground every rezidnt-side claim in an intel memo against `git log` and
> the working tree directly, not only against the most recent DR that discusses the same area — a one-day-old
> record can flip a "rezidnt hasn't closed X" finding into "rezidnt closed X yesterday."

## Questions this read must answer
1. Has Omnigent's architecture approach (meta-harness over coding harnesses; server-held session state) changed since 2026-07-24?
2. Has Omnigent shipped, or moved toward, an MCP *server* surface (the roadmap item memo 002 flagged as perishable)?
3. Any change to Omnigent's gate/verifier or policy-verdict story (RBAC, audit trail, determinism)?
4. Any change to Omnigent's event-log/state-derivation model?
5. Any change to onboarding/installer or packaging?
6. Any change to operator UI / dashboard / collaboration surface?
7. Any change to orchestration / multi-agent fan-out, and — separately — has rezidnt's own fan-out/worktree story moved since memo 002 (it has, per DR-044/046/047/048)?
8. Any other newly-shipped feature area material to comparison (e.g. worktree lifecycle management, mobile, harness breadth)?

## Findings

- **F0 — Release cadence since memo 002** (confidence: high, 2026-07-26): four releases landed on the public
  `omnigent-ai/omnigent` GitHub releases page since Omnigent's June 2026 launch: v0.2.0 → v0.6.0 (this memo saw
  v0.2.0 through v0.6.0; v0.1.0 is the launch tag). **Date caveat (confidence: low on exact dates):** the releases
  page rendered dates that a fetch tool echoed as "2024" — almost certainly a year-rendering artifact, since
  Omnigent did not exist before June 2026 per memo 001/002 and every dated blog source is 2026. Relative sequence
  and content are trustworthy; absolute dates are not and should be re-verified directly against
  https://github.com/omnigent-ai/omnigent/releases before any of them are cited outside this memo.
  Source: https://github.com/omnigent-ai/omnigent/releases.

- **F1 — No MCP-server, RBAC, or audit-log shipment across four releases** (confidence: moderate–high, 2026-07-26):
  none of v0.2.0–v0.6.0's release notes mention an "Omnigent Server MCP," enterprise RBAC, or a durable
  decision/event audit log. Shipped instead: more harnesses (Hermes, Copilot, OpenCode, Goose, Qwen, Kiro, Kimi,
  generic ACP), more sandbox providers (NVIDIA OpenShell, E2B, CoreWeave, Podman, Kubernetes Pods, Cloudflare
  Containers), desktop apps for Windows/Linux/macOS and an iOS app, and UX polish (themes, command palette,
  message queuing). **This corroborates memo 002 F1/F3/F4 rather than changing them** — the three biggest
  structural gaps memo 002 identified (log-is-truth, deterministic verifier layer, orchestration-as-MCP-server)
  are still absent as of this read. Source: https://github.com/omnigent-ai/omnigent/releases (v0.2.0–v0.6.0 notes,
  synthesized by fetch — not independently re-verified line-by-line, hence moderate not high).

- **F2 — Sub-agent orchestration deepened, not just maintained** (confidence: moderate, 2026-07-26): v0.4.0 reports
  the "Polly" orchestrator now fans work across **cursor, hermes, and opencode agents** (previously the marquee
  demo was narrower), and v0.3.0 reports "parallel sub-agents received stability improvements, with serialized
  continuation fixing race conditions in turn-start operations" — i.e. Omnigent is hardening a fan-out mechanism
  it already shipped, not merely announcing one. Source: https://github.com/omnigent-ai/omnigent/releases.
  *rezidnt side (confidence: high, dated 2026-07-24/25 — DR-047/048):* memo 002 said "rezidnt does not ship a
  lead-agent-delegates-to-sub-agents orchestrator on the golden path; Omnigent leads there." **This has partly
  closed**: DR-044/045/046 shipped live lead→sub fan-out with per-sub worktree isolation, and DR-047 landed the
  sole-allocator worktree registry that makes that fan-out safe under conflict (a refused sub-task no longer takes
  its siblings down, DR-044 §Decision 3 finally "reached"). The gap is narrower than memo 002 recorded, but not
  closed: rezidnt's fan-out is lead-only (DR-045 — no nested fan-out), Omnigent's is not described with that
  restriction, and DR-046 still defers nested/cross-workspace fan-out.

- ~~**F3 — Worktree lifecycle: Omnigent shipped cleanup; rezidnt named the same problem and has not yet closed it**
  (confidence: moderate on Omnigent, high on rezidnt, 2026-07-26): v0.5.0 lists "git worktree session creation
  with **automatic cleanup**." rezidnt's own DR-047 (2026-07-24) recorded the mirror-image defect as *open*: "the
  daemon calls `release_worktree` from no production path, so DR-007's ratified allocate→use→release lifecycle
  never completes: every allocation leaks a `notify` watcher plus a debounce task for the process lifetime, trees
  stay on disk, and registry entries are never closed," naming it the next slice (Decision 6) rather than solving
  it. This is a genuine, dated, apples-to-apples gap: Omnigent claims a shipped answer to the same lifecycle
  question rezidnt has explicitly deferred. Confidence is moderate on Omnigent because the claim is a one-line
  release-note bullet, not verified by reading or running the feature (DR-002 rule 6 would permit a black-box run
  to confirm it actually reclaims disk/watchers, not just the worktree). Sources:
  https://github.com/omnigent-ai/omnigent/releases (v0.5.0); DR-047 (../docs/decisions/DR-047-registry-convergence-landed.md) Decision 6.~~

  **CORRECTED 2026-07-26 (same-day correction pass).** The struck finding above is preserved verbatim for audit.
  It was accurate only as of DR-047 (2026-07-24) and went stale the very next day; this memo, written 2026-07-26,
  should have caught the update and did not (see the methodology note at the top of this memo).
  [DR-049](../docs/decisions/DR-049-worktree-release-lifecycle.md) (Status: **ACCEPTED**, 2026-07-25) opened and
  closed DR-047 §Decision 6 in full: a merged worktree is now released at merge, in production —
  `bins/rezidentd/src/runs.rs` ~line 2225 calls `ctx.daemon.release_worktree_at(&ctx.worktree.display().to_string())`
  inside the verified-pass arm, and the adjacent comment states plainly this is "the call the 'OWED' note that
  used to sit at the end of `drive_run` was waiting for." The fold's collapsed `status` field is split into
  `lifecycle`/`outcome` (DR-049 §Decision 2) so a release can no longer clobber a merge fact — the exact defect
  that made DR-047 §Decision 5 decline to do this. The release verb is dispatched on the MCP write surface
  (`crates/rezidnt-mcp/src/lib.rs` ~line 686: `"release_worktree" => self.call_release_worktree(args).await`).
  The lane merged 2026-07-25, commit `da17699` ("merge lane 1: DR-049 worktree release lifecycle, 16/16"), and
  is independently confirmed landed by a later record: [DR-055](../docs/decisions/DR-055-trials-slice-b-matrix-primitive.md)
  §Decision 4 states "DR-049's release lifecycle, which has since landed (16/16, merged)."
  **The lifecycle gap this finding described is CLOSED, not open.**

  What remains is a *named, deliberate divergence*, not a residual deficit. DR-049 §Decision 3 chose
  explicit-only release for FAILED trees — they survive on disk with an open registry claim, for triage, with no
  TTL and no auto-reap — where Omnigent's v0.5.0 note claims automatic cleanup (scope of that automation still
  unverified — still a one-line release note; the DR-002 rule 6 black-box run to confirm it has not been done).
  This is an accepted cost, not an oversight: DR-049's own Status field names Decision 3 as one of "the two
  settlements most likely to warrant re-opening," precisely because it "accepts unbounded accumulation until an
  operator acts" — and DR-049's own §Consequences risk register escalates the point further on its own initiative:
  post-daemon-restart, `Daemon::allocations` is an in-memory `HashMap` only (`bins/rezidentd/src/runs.rs:223`), so
  a retained failed tree becomes unreachable by the only release door that could close it, and accumulation is
  then bounded by *nothing*, not even operator discipline. The accurate comparison is: rezidnt ships a deliberate,
  named, self-escalated-risk policy for the failure path — not "a competitor closed a gap rezidnt named for
  itself first and had not yet addressed." Confidence: high on rezidnt (primary source — DR text and code read
  directly); moderate on Omnigent, unchanged from the original finding (still an unverified release-note bullet).
  Sources: DR-049 (../docs/decisions/DR-049-worktree-release-lifecycle.md) §Decision 1, §Decision 3, Status
  field, §Consequences risk (1); DR-055 (../docs/decisions/DR-055-trials-slice-b-matrix-primitive.md) §Decision 4;
  commit `da17699`; `bins/rezidentd/src/runs.rs` ~line 2225 and `:223`; `crates/rezidnt-mcp/src/lib.rs` ~line 686.

- **F4 — Gate/verifier engine: rezidnt's edge sharpened, not eroded** (confidence: high on rezidnt side,
  2026-07-25/26): nothing in the Omnigent releases changes memo 002 F3's conclusion (no deterministic interrogable
  verifier layer distinct from the policy engine). Meanwhile rezidnt tightened its own contract: DR-051 recorded
  and resolved a live I6 risk (a codex `turn.completed→"success"` mapping that could have silently coerced a
  mismapped failure to `pass`) by recording a real failing transcript rather than asserting the mapping correct —
  the exact "deterministic, interrogable, never-coerced" discipline memo 002 credited rezidnt with, demonstrated
  in the wild. DR-054 further hardened the contract by giving a contract-violated run its own debrief exit code
  (3) rather than folding it into a generic failure. No comparable "we caught our own policy engine almost lying"
  discipline is described anywhere in the Omnigent sources read. Sources: DR-051
  (../docs/decisions/DR-051-codex-failure-recording-and-fallback-fidelity.md); DR-054
  (../docs/decisions/DR-054-contract-violated-debrief-exit-code.md).

- **F5 — Harness breadth: Omnigent is pulling further ahead** (confidence: high, 2026-07-26): Omnigent now
  natively supports at least eleven harnesses/SDKs (Claude Code, Codex, Cursor, Pi, Hermes, GitHub Copilot,
  OpenCode, Goose, Qwen Code, Kiro, Kimi Code, plus a generic ACP adapter for anything else) versus rezidnt's one
  production harness (`claude-code`) plus one just-landed second adapter (codex CLI, DR-048/050, gated behind
  `TESTED_CODEX_VERSIONS = &[(0,145)]` and not yet wired into the daemon's `SUPPORTED_HARNESSES`). This is a
  volume gap, not a structural one — rezidnt's DR-048 slice A extracted `AgentSubstrate` from a single hardcoded
  impl specifically to make this cheap to close per-harness, and I4 already commits to the seam — but it is a
  real, dated, and widening gap in shipped breadth. Source: https://github.com/omnigent-ai/omnigent/releases
  (v0.3.0/v0.4.0 harness additions); DR-048 (../docs/decisions/DR-048-trials.md), DR-050
  (../docs/decisions/DR-050-trials-slice-a-closeout.md).

- **F6 — Operator UX: mobile parity landed** (confidence: high, 2026-07-26): v0.5.0 shipped an iOS app (Android
  "coming soon"), closing part of the "multi-device presence" gap memo 002 F5 already conceded to Omnigent.
  rezidnt's twelve-month non-goals (arch §1) explicitly exclude mobile clients and real-time multi-device sync —
  this is a stated non-goal, not an oversight, but the gap memo 002 flagged as "do not compete here" is now wider
  in absolute terms. Source: https://github.com/omnigent-ai/omnigent/releases (v0.5.0);
  `docs/rezidnt-architecture.md` §1 (twelve-month non-goals).

- **F7 — Packaging: still not one static binary** (confidence: high, 2026-07-26): v0.3.0/v0.6.0 add more sandbox
  and deployment targets (Kubernetes, Databricks Apps, Cloudflare Containers, AWS Bedrock) — i.e. Omnigent is
  investing further into a service/container deployment model, the opposite direction from a single static
  binary. rezidnt's I7 posture (`curl | sh`, no runtime deps, no telemetry, DR-037's shipped installer) is
  unchanged and, if anything, the two products' packaging philosophies are diverging further rather than
  converging. Source: https://github.com/omnigent-ai/omnigent/releases.

## Verdict table (rezidnt vs Omnigent, as of 2026-07-26)

| Area | Verdict | Reasoning (brief) |
|---|---|---|
| 1. Architecture approach (daemon/agent model, core philosophy) | **Not comparable** (stable since memo 002) | Meta-harness-over-harnesses with server-held session state vs. rezidnt's log-is-truth resident daemon. Different category of product solving overlapping problems; "ahead/behind" doesn't apply cleanly. rezidnt's structural claim (append-only fabric, pure-fold state) is unchanged and unmatched (memo 002 F1). |
| 2. MCP surface / tool integration | **rezidnt ahead** (unchanged, perishable) | rezidnt is an MCP client *and* server-of-the-fleet today (`board_view`, `get_escalations`, `gate_explain`, etc., schema-generated no-drift). Omnigent remains MCP-client-only across four releases; its "Server MCP" stays roadmap (F1). Edge holds but is explicitly perishable per memo 002 — watch every release. |
| 3. Gate/verifier / quality-gate engine | **rezidnt ahead, and the edge is now demonstrated, not just structural** | No Omnigent release adds a deterministic pass/fail/inconclusive verifier layer distinct from its policy engine. rezidnt caught and closed its own near-I6-violation this week (F4, DR-051) — evidence the discipline is load-bearing, not marketing. |
| 4. Event fabric / log model / state derivation | **rezidnt ahead** (unchanged) | Omnigent still shows no append-only event log or replay story across four releases (F1); Postgres/SQLite session-state-of-record persists as the model (memo 002 F1). rezidnt's hash-chained fabric + `rebuild` is unmatched. |
| 5. Onboarding / installer experience | **rezidnt ahead on friction, Omnigent ahead on reach** | rezidnt: one static musl binary, `curl \| sh` (DR-037). Omnigent: desktop apps now on Windows/Linux/macOS plus iOS (F6), a broader reach story, but still a Python/FastAPI service + datastore underneath, not a single binary (F7). Different axis of "easy" — install friction vs. platform reach. |
| 6. Operator UI / dashboard | **Mixed, gap widened on Omnigent's side** | Omnigent shipped mobile (iOS) and richer desktop UX (themes, command palette, PDF/notebook preview) since memo 002 — a real, shipped expansion of the "presence/collaboration" lead memo 002 already conceded (F6). rezidnt's read-only board + separate write client is architecturally cleaner (memo 002 F5) but rezidnt does not compete on device/surface breadth and its non-goals say it won't. |
| 7. Orchestration / multi-agent fan-out | **Gap narrowed, not closed** | Memo 002: "Omnigent leads there, rezidnt has no golden-path orchestrator." Now: rezidnt shipped live lead→sub fan-out with a conflict-safe worktree registry (DR-044/046/047, F2) — a real, dated capability that did not exist at memo 002's writing. Omnigent simultaneously widened its own fan-out (Polly now spans cursor/hermes/opencode, F2) and ~~shipped worktree auto-cleanup that rezidnt has explicitly not yet built (F3, DR-047 Decision 6 still open)~~ **CORRECTED 2026-07-26: stale the day after it was written.** [DR-049](../docs/decisions/DR-049-worktree-release-lifecycle.md) (ACCEPTED 2026-07-25) closed DR-047 Decision 6; release-at-merge is live in production and merged (commit `da17699`; `bins/rezidentd/src/runs.rs` ~line 2225). What remains is DR-049 §Decision 3's deliberate explicit-only release for FAILED trees — a named, accepted, self-escalated-risk cost (see corrected F3), not an unclosed gap. Net: rezidnt closed part of the gap and Omnigent kept moving; **still behind** on fan-out/harness breadth generally, but the worktree-lifecycle component of that gap is now closed — less far behind than 2026-07-24 on both counts, more so on the lifecycle point specifically. |
| 8. Harness/adapter breadth | **rezidnt behind, gap widening** | Omnigent: ~11 harnesses + a generic ACP adapter (F5). rezidnt: one production harness plus one just-landed, not-yet-daemon-wired second adapter (codex, gated to a single tested version pair, DR-050). Structural seam (I4, `AgentSubstrate`) is sound and this slice (DR-048 A) proved it extracts cleanly, but shipped breadth is a real, growing gap. |
| 9. Packaging / telemetry posture | **rezidnt ahead** (unchanged, diverging further) | Static binary + no-telemetry stance (I7, DR-037) vs. Omnigent's deepening investment in containerized/hosted deployment targets (F7). The two roadmaps are pointed in opposite directions on this axis, not converging. |

## Implications for rezidnt (positioning / benchmark / risk-register only — NOT directives)
- **Positioning:** memo 002's thesis — lead on replayable evidence, deterministic verdicts, and local-first
  single-binary; do not lead on collaboration richness, device reach, or harness/orchestration breadth — still
  holds and is now reinforced by a concrete, dated example (F4, DR-051) rather than only an architectural
  argument.
- **Benchmark:** add one scenario to the memo-001/002 suite — **worktree reclamation** (F3): drive both products
  through N agent runs and measure disk/watcher/registry state after completion. ~~This is now a fair, apples-to-
  apples black-box test (DR-002 rule 6) because both products name the same lifecycle problem, one claiming a
  shipped answer, one with it as an open slice.~~ **CORRECTED 2026-07-26: the "open slice" premise is stale (see
  corrected F3) — rezidnt's merged-path release is now shipped, not open.** The scenario is still worth running,
  narrowed: rezidnt's merged-tree reclamation is now a claim to *verify* black-box (DR-002 rule 6) rather than a
  gap to note, and the genuinely open comparison is the **failed-tree** path specifically — rezidnt's DR-049
  §Decision 3 explicit-only/no-TTL policy vs. whatever Omnigent's v0.5.0 "automatic cleanup" actually does on a
  failed/abandoned session (unverified either way; a black-box run is the only way to know).
- **Risk-register:** two live signals, both trending toward "watch, don't ignore" — (1) Omnigent's harness-breadth
  and fan-out investment (F2, F5) is accelerating, not idling, which shortens the runway on "rezidnt has the
  structurally sound seam" as a standalone claim if it isn't populated; (2) ~~Omnigent shipping worktree
  auto-cleanup (F3) while rezidnt's own worktree release lifecycle sits as an explicitly *undecided* open slice
  (DR-047 Decision 6, not merely unbuilt) is a concrete, dated instance of a competitor closing a gap rezidnt
  named for itself first.~~ **CORRECTED 2026-07-26: false as of the day after DR-047 was cited —
  [DR-049](../docs/decisions/DR-049-worktree-release-lifecycle.md) (ACCEPTED 2026-07-25) closed the lifecycle
  slice; release-at-merge is live and merged (`da17699`). The actual live risk is narrower, rezidnt-authored,
  and already on rezidnt's own ledger, not a competitor catching an unnoticed gap: DR-049 §Decision 3
  deliberately leaves FAILED-tree release explicit-only (no TTL), and DR-049's own §Consequences risk register
  escalates this further on its own initiative — post-daemon-restart, `Daemon::allocations` is in-memory only
  (`bins/rezidentd/src/runs.rs:223`), so accumulation is then bounded by *nothing*, not even operator discipline.
  DR-049's Status field itself flags Decision 3 as one of the two settlements most likely to be reopened. Watch
  this as a self-identified, self-escalated risk that may warrant a future DR — not as evidence of a competitor
  winning a race rezidnt didn't know it was running.**

## Coverage gaps (taxonomy diff)
- None minted here. This is a positioning read, not a `/subject` pass.

---
Design changes motivated by this memo require a DR citing it (DR-002 rule 3). No competitor code structure is reproduced above.
