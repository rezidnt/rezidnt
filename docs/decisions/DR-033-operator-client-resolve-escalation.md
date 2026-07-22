> Index: [§20 of the plan](../rezidnt-architecture.md#20-decision-records) · plan §16 (permit engine), §19 (operator write client) · invariants I2, I3, I5, I6, I7 · slice 2 of DR-032 · paired warden /subject for `permit.resolved`

# Decision Record DR-033 — Operator client, slice 2: resolve-escalation

**Date:** 2026-07-22
**Status:** ACCEPTED
**Amends:** §16 (permit PDP decision path — adds a pre-verifier ledger-check applying a prior human resolution); §19 (operator write client register gains `resolve-permit`); fulfils DR-032's slice-2 sequencing; extends DR-008/DR-009 (permit PDP) and DR-031 (operator-client seam). Reaffirms I1's board proof untouched.

## Context

DR-032 shipped the operator-client seam and slice 1 (kill-run), sequencing resolve-escalation into slice 2 as "a genuine gap": `permit.escalated` is routed to a client but never auto-resolved, and the request sits `pending` with no TTL or recovery. This record designs that resolution.

The mechanics constrain the design and were read, not assumed:
- **The PEP is one-shot.** `permit_hook` sends one `Request::RequestPermission`, reads one `Reply::PermitDecision`, exits (`bins/rezidnt/src/permit_hook.rs:139-173`); on "ask", the agent blocks and *that* tool call is dead — no channel stays open (`:270-275`). *(Erratum 2026-07-22, via DR-034: this path was first written `crates/rezidnt-mcp/src/permit_hook.rs`; the PEP binary lives under `bins/rezidnt/`. Line ranges and the reasoning are unaffected.)* The daemon returns the decision and closes the connection (`crates/rezidnt-mcp/src/lib.rs:477-499`). `Reply::PermitDecision { request_id, decision, reason }` has no "waiting"/"update" variant (`crates/rezidnt-proto/src/lib.rs:178-204`).
- **No TTL exists.** `PermitLedgerEntry` carries `action`, `decision`, `policy_ref?`, `reason?` — no created_at/expires_at (`crates/rezidnt-state/src/lib.rs:192-207`); the reaper times out processes, not permits. An escalation sits `pending` indefinitely.
- **The PDP already replays the log.** `decide_permit` folds run state on every call via `fold_run_state` (`crates/rezidnt-mcp/src/lib.rs:858, 1101-1114`) — so a recorded resolution needs NO live seam; the next request reads it.
- **request_id is not stable across asks.** `request_permission` mints a fresh `request_id` when the caller omits one (`crates/rezidnt-mcp/src/lib.rs:858`); a re-ask for the same action carries a *different* id than the escalated one, so "apply on next ask" must key on ACTION identity `(run, tool, action/target)`, not request_id.

Slice 1's pattern is the template: an operator-only badged MCP tool (`check_operator_badge`, opaque-badge-only, macaroon refused), advertised in `tools_list` with a `schema_for!` inputSchema (§9 no-drift), dispatch → substrate method → one attributed fact via the single writer.

**Strongest counterargument (recorded, not just the outcome):** "honored on next ask" does NOT unblock the currently-stalled agent. Only the *rejected* live-unblock option — a new `Reply::PermitUpdate` plus a PEP that holds its connection open and long-polls for a resolution — achieves true auto-resume. The operator who resolves sees no effect on the dead call; the agent must ask again (retry / re-prompt) for the resolution to bite. We reject live-unblock for slice 2 because it inverts the one-shot PEP contract (a held-open connection reintroduces the exact control/data-plane liveness coupling I2 and the event-sourcing discipline push out), balloons scope across proto + PEP + daemon socket lifecycle, and trades log-truth simplicity for a live mechanism the PDP-replays-the-log design makes unnecessary for correctness. "Honored on next ask" is complete and interrogable with a recorded fact and zero PEP change; it is paired with slice-1 `kill-run` so an operator who wants to *stop* the stalled agent rather than *retry* it already has that lever. Live-unblock is noted as a POSSIBLE future slice IF measured operator friction demands it — demand-gated, not scheduled.

## Decision

1. **Resolve semantics = "honored on next ask" (the load-bearing lifecycle call).** `resolve_permit` records a durable `permit.resolved` fact carrying the human's decision. It does NOT resume the dead blocked call and adds NO live-unblock / long-poll to the PEP — the one-shot PEP contract is unchanged. Instead, `decide_permit`, BEFORE running verifiers, consults the folded ledger: if the incoming request matches a prior `permit.resolved` for the same action on the same run, the PDP APPLIES that human decision — emitting the corresponding `permit.granted` / `permit.denied` and citing the resolution (its `request_id` + `operator_badge_id`) as the deciding authority — instead of re-escalating. **Limit, stated plainly:** an operator resolving sees no effect until the agent next asks; pair with slice-1 `kill-run` to stop instead of retry.

2. **No TTL (default).** A `permit.resolved` stands until overridden by a NEW `permit.resolved` with a different decision (append-only discipline, no clock dependency). A mistaken deny is corrected by emitting a new resolve, not by waiting one out. A future slice MAY add expiry if measured demand shows it.

3. **Request-scoped, action-matched — not a broad predicate (default).** A resolution answers a specific escalated `request_id` (recorded for the audit chain) AND carries the action identity so the PDP matches a SUBSEQUENT request on the same `(run, tool, action/target)`. It does NOT grant unrelated tools/actions on the run, and it is NOT a role/workspace predicate. A "grant-all-matching" variant is explicitly deferred.

## Design

- **Tool:** `resolve_permit { run, request_id, decision: "allow"|"deny", reason? }` behind the operator-only door — reuse/extend `check_operator_badge`; agent macaroons refused, badge verified BEFORE any side effect (mirrors DR-032 slice 1). Advertised in `tools_list` with a `schema_for!(rezidnt_types::mcp::ResolvePermitArgs)` inputSchema (§9 no-drift). Dispatch → `call_resolve_permit` emits ONE `permit.resolved` fact directly via the single writer (I3, DR-006). Attribution: `operator_badge_id` = the verified loggable id, NEVER the token. *(Erratum 2026-07-22: this draft first named a `McpSubstrate::resolve_permit` method; the implemented design needs none — a resolution is a pure fact emit with no side effect to perform, unlike `kill_run`'s reaper seam. The decision is unchanged.)* The daemon DERIVES the escalation's `action`/`target` from the folded log by `request_id` (the operator supplies neither), so a resolution always carries a matchable descriptor.
- **Subject:** NEW `permit.resolved` v1 — the exact payload is a warden `/subject` decision paired with this DR; flagged here, NOT minted here. Intent: `run`, `request_id` (the escalation it answers), the action identity needed for next-ask matching, `decision` (allow|deny), `reason?`, `operator_badge_id?`. Consumers exist: the reducer, the PDP ledger-check, and debrief/gate_explain.
- **PDP path:** a ledger-check in `decide_permit` (`crates/rezidnt-mcp/src/lib.rs:858`) BEFORE verifier dispatch that applies a matching prior resolution; the applied decision is itself logged (`permit.granted`/`permit.denied`) so `gate why` / `debrief` shows "escalated → human-resolved(allow) by operator badge_id → granted" (I6).
- **Reducer:** a `permit.resolved` arm folding onto the `PermitLedgerEntry` (`crates/rezidnt-state/src/lib.rs:192-207`: decision → resolved) + AgentRunState, keyed to correlate the escalation by action identity.

## Invariants

- **I2** — control-plane; evidence rides by-ref, never inline. No liveness coupling: the PEP connection is not held open.
- **I3** — the resolution is a durable, replayable fact via the single writer; the PDP re-derives the applied decision from the log, never from hidden state.
- **I5** — MCP tool, advertised in `tools_list`.
- **I6** — the resolution and its deciding authority are interrogable; an escalated permit is never SILENTLY coerced — a human grant is a RECORDED override, not a coercion.
- **I7** — no heavy dep: reuses the badge door, the existing PDP fold, and the single writer.
- **I1** — the operator client links no board (`rezidnt-tui`) code, so the board's `crate_has_no_writer_dependency` proof stays untouched.

## Consequences

- **Roadmap:** slice 2 (resolve-escalation) enters the loop on ratify; current-slice advances to the slice-2 line. §19's operator-write-client register gains the `rezidnt operator resolve-permit` entry. A paired warden `/subject` mints `permit.resolved` v1 before/with implementation.
- **Risk register:** closes DR-032's carry-forward risk that `permit.escalated` remained un-resolvable — an escalation now has a resolver. Adds one carry-forward risk in plain words: a resolved escalation does NOT auto-resume the stalled agent; the agent must ask again for the resolution to take effect, so a resolve without a re-ask leaves the run stalled (kill-run is the stop lever).
- **Test/criterion honesty:** this record weakens NO test and lowers NO bar. It does add one honest limitation by design, not by dilution: "honored on next ask" cannot unblock the currently-blocked tool call; only a future demand-gated live-unblock slice could, and it is explicitly not scheduled.
- Cross-references: DR-032 (parent — this is its slice 2), DR-031 (the operator-client seam), DR-017 (operator badge / macaroon-refused door), DR-008/DR-009 (the permit PDP this extends), DR-006 (single-writer append — client emits via the daemon, never directly).

Amendments to this record require DR-034.
