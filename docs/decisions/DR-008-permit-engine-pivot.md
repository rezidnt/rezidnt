[← Decision records index](../rezidnt-architecture.md#20-decision-records) · [Architecture plan](../rezidnt-architecture.md)

# Decision Record DR-008 — Permit-engine pivot (rezidnt owns both axes)

**Date:** 2026-07-17 · **Status:** ACCEPTED (owner) · **Amends:** §1 (non-goals — drops the compose-only framing), §8 (positioning/lifecycle), §16 (roadmap — new phase). Closes a §19 open-decisions item; promotes DR-005's PROVISIONAL macaroons. No invariant text is rewritten; I3, I4, I6 are the axes and each is preserved (see Consequences). Design basis: [`docs/design/permit-engine.md`](../design/permit-engine.md) (design-first per DR-002 rule 1).

## Context

Today rezidnt is positioned as the *complement* to Omnigent: their policies gate permissions **before** the act; rezidnt gates evidence **after** it — "the models compose" (§8; echoed in §1). This record reverses that. rezidnt takes the **pre-hoc governance axis natively** via a central in-daemon permit engine, and Omnigent becomes a benchmark baseline / optional adapter, not a required companion.

The seam already exists, so this is an extension, not a new pillar: a **fourth gate lifecycle point** `permit` joins `vet`/`pre_merge`/`post_run`. A permit-verifier is just a verifier on that gate, so **the gate engine *is* the policy engine** — no second engine, no bespoke DSL. The verdict contract maps to authorization with zero new vocabulary and I6 intact: `pass → allow`, `fail → deny`, `inconclusive → escalate-to-a-human` (routed to a client, never coerced). Architecture is the standard **PDP/PEP split**: rezidnt is the headless in-daemon Policy Decision Point; the harness's `PreToolUse` hook is the Policy Enforcement Point. Enforcement is a **substrate capability (I4)** — true interception where a harness exposes a hook, graceful degradation to `vet` + post-hoc evidence where it does not.

**Strongest counterargument (dissent, recorded verbatim per house style):** this abandons rezidnt's sharp, defensible wedge — §18 names the differentiation as evidence-gates, "none of which their model rewards." Replacing Omnigent means fighting Databricks/Omnigent head-on on their turf, where they have a head start, and risks diluting the one thing nobody else does: post-hoc replayable evidence. **Counter to the counter:** the permit+verify unification on a **single append-only log** is itself novel — one log carries both permission decisions and evidence, so a permission decision is replayable *as* evidence — and the marginal build cost is bounded because the seam (vet gate + badges + two verifier kinds) already exists.

## Decision (ratify the scope/positioning pivot — not yet implementation)

- **rezidnt owns the "may" axis natively.** Add the `permit` gate lifecycle point; permit-verifiers are native (tool-allowlist, path-scope, etc.) or exec (OPA/Rego or Cedar via the existing §8 argv+JSON contract — "use a mature DSL," don't build one).
- **PDP/PEP split** as above; enforcement degrades gracefully and we do not overclaim interception.
- **`request_permission` MCP tool (I5)**, also reachable over the socket for the harness hook; action context travels as a CAS ref, never inline (I2).
- **Delegation via macaroon-attenuated badges** — this is the real delegation use case DR-005 said would promote them from **PROVISIONAL**; a lead agent attenuates a sub-agent's badge, offline-verifiable, no central lookup. Closes the §19 open-decisions item.
- **New event subjects** `permit.requested` / `permit.granted` / `permit.denied` / `permit.escalated`, minted through a warden `/subject` follow-on, each with a folding reducer (no consumer-less subjects — DR-006 precedent).

## Consequences

- **§16 roadmap delta:** adds a new phase between gates (Phase 2) and terminal fidelity (Phase 3), per the design sketch's proposed **SP0–SP5 sequence** (plus an Omnigent benchmark run under DR-002 rule 6) — see `docs/design/permit-engine.md` §11; not re-specified here.
- **Process follow-ons (all gated):** a scoped `/intel` read of Omnigent's policy/enforcement model to gap-check coverage (DR-002 rules 1–3; any change it motivates gets its own DR); a warden `/subject` pass for `permit.*`; then oracle-first slices.
- **Risk-register (§18) deltas:**
  - *Enforcement is only as strong as the PEP.* rezidnt cannot intercept an action a harness won't route through its hook; "replace Omnigent" is bounded by the harness hook surface until a later, larger phase makes rezidnt the sole execution chokepoint. Product copy must not overclaim interception.
  - *Hot-path latency vs I3.* Per-action permit checks sit on the agent's critical path against a fabric designed for ≤~10³ events/min (§5). Mitigation is a decision fast-path cache; the decision-logging policy must not quietly violate I3 — **safe default: log all decisions**, optimize (compact/sample allows) only if measured. Resolve any relaxation in a future DR, not silently.
- **No test or acceptance criterion is weakened** by this record; it is scope/positioning only. New acceptance criteria arrive with the SP0–SP5 slices via the oracle.

*Amendments to this record require DR-009.*
