> Index: [§20 of the plan](../rezidnt-architecture.md#20-decision-records) · plan §16 (permit engine), §19 (first graphical client) · invariants I1, I2, I5

# Decision Record DR-031 — Board read-only / operator-client split

**Date:** 2026-07-21
**Status:** ACCEPTED
**Amends:** §16 (S5 board scope), §19 (first graphical client register); reaffirms I1, I2, I5 as they bind `rezidnt-tui`.

## Context

The S5 `rezidnt-tui` fleet board is a pure downstream reader of derived state. Its read-only-ness is not a convention — it is a machine-checked proof of I1: `crate_has_no_writer_dependency` (`crates/rezidnt-tui/tests/read_only.rs:30-64`) parses the crate's own runtime `[dependencies]` and asserts it links none of `rezidnt-fabric`, `rezidnt-proto`, `rezidnt-run`, `rezidnt-mcp`, `rezidnt-gate`, `rusqlite`, `blake3`. A sibling test (`read_only.rs:89-115`) pins that the board rides the existing `Request::Tail { subject: None }` op and mints no board-specific proto op. An actionable board deletes both proofs by construction.

The control plane for the two operator actions in view — resolving an escalated permit and killing a run — already exists and is MCP-first (I5). The proto carries `Request::RequestPermission` (`crates/rezidnt-proto/src/lib.rs:98`) → `Reply::PermitDecision` (`:199`); `permit.escalated` is defined in `spec/ontology.md:146` as "escalate to a human (routed to a client, never auto-resolved)", carrying `policy_ref` + `evidence_ref`. State-mutating MCP calls already carry the operator badge / macaroon and are VERIFY-checked (DR-017). Bolting write actions onto the board stands up a SECOND control channel that re-implements auth, policy dispatch, and interrogability the MCP surface already provides — I2 (control/data plane never mix) and I5 both cut against it. Interactive fidelity is Phase 3 and explicitly demand-gated, not scheduled (§19; sequencing law: fabric → gates → terminal last).

**Strongest counterargument (recorded, not just outcome):** a single actionable board is better operator UX — see-a-problem, fix-it-in-one-surface, no context switch — and `permit.escalated` being "routed to a client" can be read as an invitation for the VIEW client to be the resolver. We reject collapsing the planes for that convenience: the I1 structural proof and I2 plane-separation are load-bearing (a board that can emit is a board that can be made to lie about the log it renders), whereas the UX gap is closable without them. Mitigation: the separate operator client may be launched from / alongside the board, giving near-parity UX (one screen, two processes) without one crate holding both a reader and a writer.

## Decision

1. The `rezidnt-tui` fleet board is **read-only permanently**. No writer / socket-write / emit dependency may ever be added to it; `crate_has_no_writer_dependency` is the BINDING guard and must stay green.
2. Operator write actions — resolving an escalated permit, killing a run — live on a **separate, explicitly-authorized MCP write client** (its own crate / bin mode), architecturally distinct from the board, carrying the operator badge and routing through the existing PDP. This is the "operator-client seam." Building it is DR-first when actually undertaken.
3. "Build out the board" may only mean **richer read-only render** (I1 preserved), which amends the S5 render golden and runs the normal /oracle → implementer → /vet → /debrief loop.

## Consequences

- **Roadmap:** the operator write client is filed under Phase 3 / demand-gated (§19), not scheduled now. No S5 criterion is weakened — S5 stays read-only render + watch-channel consumption; the two structural guards in `read_only.rs` are reaffirmed, not relaxed.
- **Risk register:** removes the "actionable board silently becomes a second control plane" drift risk by naming the sanctioned home for writes up front.
- **Test/criterion honesty:** this record weakens no test and lowers no bar. It tightens one: it makes `crate_has_no_writer_dependency` a permanent invariant rather than an S5-local guard. Any future richer-render work still owes the S5 render golden its update.
- Cross-references: DR-006 (single-writer append / integrity — the board must not be a second writer), DR-008/DR-009 (permit PDP that the operator client calls), DR-014 (PEP enforcement of the decision), DR-017 (operator badge / macaroon the write client carries).

Amendments to this record require DR-032.
