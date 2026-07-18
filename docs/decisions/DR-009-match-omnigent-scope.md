[← Decision records index](../rezidnt-architecture.md#20-decision-records) · [Architecture plan](../rezidnt-architecture.md)

# Decision Record DR-009 — Match-Omnigent scope (four memo-surfaced permit capabilities)

**Date:** 2026-07-17 · **Status:** ACCEPTED (owner) · **Amends:** §16 (roadmap — extends the DR-008 permit phase with C1/C7/C8 and adds a distinct later "sole-chokepoint enforcement" phase for C3). No invariant text is rewritten. **Cites:** intel memo [`intel/001-omnigent-permission-governance.md`](../../intel/001-omnigent-permission-governance.md) (DR-002 rule 3 — this is a memo-motivated change). **Builds on:** [DR-008](DR-008-permit-engine-pivot.md); design basis [`docs/design/permit-engine.md`](../design/permit-engine.md) (SP0–SP5).

## Context

The scoped Omnigent read (memo 001) confirmed the DR-008 permit-engine design is sound — the PDP/PEP split, the verifier-as-policy contract, and the single-log evidence wedge all hold (matrix C2/C4/C5/C6/C11). The read also surfaced four capability gaps the owner has chosen to fold into the "match Omnigent now" scope. This record ratifies that scope; it is not a design and specifies no implementation.

**Strongest counterargument (dissent, recorded verbatim per house style):** committing all four gaps — C3 especially — is exactly the "scope gravity" §18 warns about ("building layer N+1 before N has users"). C3 in particular pulls rezidnt into re-implementing boring sandbox/proxy/credential-broker infrastructure that competes for the hours that should build the evidence/audit wedge, which §18 names as the actual differentiator ("differentiation is evidence-gates … none of which their model rewards"). Chasing enforcement breadth risks blurring the one thing nobody else ships. **Counter to the counter:** matching enforcement breadth is table-stakes for displacing Omnigent at all — the memo's honest counter is that rezidnt can be *out-enforced* until a sole-chokepoint phase (memo Q8 / C3; design §10.1). C1/C7/C8 are bounded and mostly ride existing rails (native permit-verifiers, roles, SP4), and C3 is explicitly fenced behind its own design sketch + implementation DR so it cannot silently consume the roadmap. **The owner has accepted this trade knowingly.**

## Decision (ratify scope — not implementation)

Fold four memo-surfaced capabilities into the permit-engine roadmap (§16, already amended by DR-008), each traceable to memo 001:

- **C1 — spend / rate-limit permit-verifiers.** Native permit-verifiers for cumulative-spend caps (soft → ASK/escalate, hard → DENY) and rate limits. On existing rails; **folds into SP1** alongside tool-allowlist and path-scope.
- **C7 — intent-based authorization ("intent-lock").** A native permit-verifier that binds an agent's tool allowlist to the run's initiating intent and blocks off-task tool use (anti-prompt-injection). New work and a positioning differentiator. Added as a **new roadmap slice (SP-intent)**, placed after SP1's native verifiers land; a roadmap note only, not a spec.
- **C8 — layered policy precedence.** admin / dev / session scope with stricter-layer-wins precedence, extending the project-spec `[gates.permit]` + roles model. **Folds into SP4** (roles).
- **C3 — sandbox / egress / credential brokering.** OS sandbox (bwrap/seatbelt), L7 egress proxy, credential brokering. Omnigent's strongest enforcement primitive (memo C3, "high"). Committing it shifts rezidnt's posture from "PDP/PEP rides the harness hook" toward "be the sole execution chokepoint" (design §3, §10.1). **Committed to the roadmap as a distinct, later "sole-chokepoint enforcement" phase — but it is too large to design inline and REQUIRES its own design sketch plus its own implementation DR before any build.** Recorded here as a scoped roadmap commitment, not a design.

## Consequences

- **§16 roadmap delta:** the DR-008 permit phase gains C1 (into SP1), SP-intent (C7), and C8 (into SP4); a separate later sole-chokepoint enforcement phase is added for C3, fenced behind its own design + DR.
- **Risk-register (§18) deltas:**
  - *Scope-gravity risk from the four-gap expansion.* Mitigated by C3's mandatory design/DR fence (it cannot enter build without its own record) and by oracle-first slicing of C1/C7/C8.
  - *Reaffirms the DR-008 "enforcement bounded by the PEP" delta.* That delta now has a concrete closer — C3, the sole-chokepoint phase — but only in the later phase; product copy must still not overclaim interception breadth until it ships (echoes memo honest counters + design §10.1).
- **No test or acceptance criterion is weakened by this record; it is scope only.** New acceptance criteria arrive with the SP1/SP-intent/SP4 and the future C3 slices via the oracle.

*Amendments to this record require DR-010.*
