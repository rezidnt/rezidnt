> Index: [§20 of the plan](../rezidnt-architecture.md#20-decision-records) · plan §16 (permit engine), §19 (first graphical client) · invariants I2, I3, I5, I6, I7 · fulfils DR-031

# Decision Record DR-032 — Operator client, slice 1: kill-run

**Date:** 2026-07-22
**Status:** ACCEPTED
**Amends:** §19 (operator write client register); refines DR-031 §Decision 2 ("routing through the existing PDP") for the non-decision kill action; reaffirms I1's board proof as untouched.

## Context

DR-031 named a separate, explicitly-authorized MCP write client as the sanctioned home for the two operator write actions — kill a run, resolve an escalated permit — and forbade the read-only board from holding either. This record designs that seam and BOTH actions, then partitions criteria so slice 1 ships only **kill-run**; resolve-escalation is sequenced into slice 2 (the DR-027 split-and-sequence precedent — partition, do not weaken).

The two actions are asymmetric in readiness. **Kill-run machinery exists but is unexposed:** `reaper::stop_with_escalation(pid, grace)` (`crates/rezidnt-run/src/reaper.rs:104-116`) does SIGTERM → grace → SIGKILL and emits a terminal fact, but no client-facing op or tool drives it (`tools_call` has no kill case, `crates/rezidnt-mcp/src/lib.rs:492-498`; the proto `Request` enum has no terminate variant, `crates/rezidnt-proto/src/lib.rs:76-116`). **Resolve-escalation is a genuine gap:** `permit.escalated` is "routed to a client, never auto-resolved" (`spec/ontology.md:146`, `:155`) but has no tool/op/subject to resolve it; the request sits `pending` with no TTL or recovery. Slice 1 exposes the ready action; slice 2 owns the gap.

Operator auth is live (DR-017): an opaque operator badge is minted at serve (`crates/rezidnt-mcp/src/lib.rs:1368-1370`), carried in the 0600 lockfile, and verified at the §12 `check_badge` door (`:517-601`, dual-path — opaque token first, agent macaroon second) BEFORE any side effect. The MCP write pattern is `call_<tool> → check_badge → substrate call → tool_ok/tool_refused` (`:483-500`, `:603-608`). Kill reuses `agent.signaled` (v1: `run`, `signal`, `escalation?` — `spec/ontology.md:243-246`), which the reaper already emits.

**Strongest counterargument (recorded, not just outcome):** kill-run over the existing socket — a new `Request::TerminateRun` served by `rezidnt-client` — is far less plumbing: no MCP-HTTP client, no badge dance, since the 0600 UDS already gates local access (DR-013). We reject it for slice 1 because DR-031's whole point is that operator write actions carry **deliberate, loggable authorization**, not **ambient local access**. UDS identity proves "a local process"; the operator badge proves "the authorized operator, and here is the loggable badge id on the resulting fact." Collapsing to the socket would re-open the exact "any local caller can mutate without a recorded authorization" surface DR-031 closed, and would set kill on a different auth footing than every other mutating tool. `op.not_served` (`crates/rezidnt-proto/src/lib.rs:158`) already pins MCP as the ratified write/decision path. A socket kill op MAY be offered later as a convenience alias once its auth story is designed — noted, not decided here.

## Decision (slice 1 — kill-run)

1. **Tool.** Add a badged MCP tool `kill_run { run: <ulid> }` behind the §12 operator-badge door. The operator badge is REQUIRED; **agent macaroons are NOT admitted for kill** — terminating a run is an operator action, not an agent self-action (the `check_badge` verb-derivation stays, but the kill door rejects the macaroon path). It calls a new substrate method that drives the EXISTING `reaper::stop_with_escalation` and emits `agent.signaled` through the single writer. **No new PDP path** — kill is a lifecycle action, not a permit decision. This REFINES DR-031 §Decision 2's "routing through the existing PDP," which was written with escalation-resolution in mind; the PDP route binds slice 2, not kill.
2. **Transport.** The operator client speaks MCP over the loopback HTTP surface carrying the operator badge read from the lockfile — NOT the bare socket. The extra plumbing (a minimal loopback HTTP POST of `tools/call`) is justified by DR-031's "explicitly-authorized" requirement; the socket's UDS-identity would bypass exactly that explicit authorization.
3. **Client shape.** A new `rezidnt operator kill-run <run>` subcommand in `bins/rezidnt` that reads the lockfile (port + operator badge) and POSTs a `tools/call`. It is distinct from the read-only board (`rezidnt-tui`) and links no board code, so the board's I1 `crate_has_no_writer_dependency` proof (`crates/rezidnt-tui/tests/read_only.rs:30-64`) is untouched. ALTERNATIVE (owner's call at ratify): a fully separate bin/crate per DR-031's "own crate/bin mode" phrasing.
4. **Ontology.** REUSE `agent.signaled` (`spec/ontology.md:243-246`). Operator attribution (who/why) is added as an ADDITIVE field only if the subject lacks it — the exact field is a warden `/subject` decision paired with this DR. Flagged here, not minted here. No new subject in slice 1.
5. **Interrogability (I6).** The `agent.signaled` fact carries operator attribution + reason so `debrief` / `gate why` can show a human-initiated stop distinct from a daemon-timeout stop (`escalation: "term"|"kill"`).

## Invariants

- **I2** — kill is control-plane, correctly on the MCP write surface; any evidence rides by-ref, never inline.
- **I3** — the kill emits a durable `agent.signaled` fact through the daemon's single writer (DR-006); the client never writes the log directly.
- **I5** — MCP-first write path; no bare-socket write op minted.
- **I6** — interrogable operator attribution on the fact.
- **I7** — no new heavy dep: a minimal loopback HTTP POST reusing existing reaper + badge machinery.
- **I1** — the read-only board's writer-free proof is untouched; the operator client links no `rezidnt-tui` code.

## Consequences

- **Roadmap:** slice 1 (kill-run) enters the loop now; resolve-escalation is sequenced to slice 2 with its own criteria. §19's operator-write-client register gains the `rezidnt operator kill-run` entry.
- **Risk register:** removes the "kill machinery exists but is unreachable / only reachable without recorded authorization" gap. Adds slice 2's carry-forward risk: `permit.escalated` remains un-resolvable and TTL-less until slice 2 lands — an escalated permit still sits `pending` indefinitely (unchanged from today, but now explicitly owned).
- **Test/criterion honesty:** this record weakens NO test and lowers NO bar. It defers resolve-escalation rather than diluting it — slice 2 owes the full escalation-resolution criteria. It admits one honest limit: refusing the macaroon path for kill means an attenuated agent badge cannot self-terminate via this tool by design.
- Cross-references: DR-031 (the seam this fulfils; refined for kill), DR-017 (operator badge / macaroon), DR-013 (socket UDS identity — rejected as the kill auth basis), DR-006 (single-writer append — client emits via the daemon, never directly), DR-004 (exit-code classes for the new subcommand), DR-008/DR-009 (PDP — slice-2 context for escalation-resolution).

## Open questions (slice 2 / deferred)

- Escalation-resolution lifecycle: does a resolved-granted escalation auto-retry the stalled agent, or require re-invocation? TTL on unresolved escalations? The new `permit.resolved` subject + its reducer.
- Whether kill-run also gets a socket op later (convenience alias; auth story TBD).
- Operator-badge non-delegability (does the kill authorization attenuate/delegate, or is it operator-only by construction?).

Amendments to this record require DR-033.
