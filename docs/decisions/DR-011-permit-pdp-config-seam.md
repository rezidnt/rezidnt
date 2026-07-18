[← Decision records index](../rezidnt-architecture.md#20-decision-records) · [Architecture plan](../rezidnt-architecture.md)

# Decision Record DR-011 — permit PDP config-resolution seam (SP-wire)

**Date:** 2026-07-18 · **Status:** ACCEPTED (owner) · **Amends:** §9 (MCP surface) / §8 — records the `McpSubstrate` PDP config seam; no invariant text is rewritten. **Cites:** — · **Builds on:** [DR-008](DR-008-permit-engine-pivot.md) (permit engine), [DR-009](DR-009-match-omnigent-scope.md), [DR-010](DR-010-intent-lock-scope.md); design basis [`docs/design/permit-engine.md`](../design/permit-engine.md) §5/§6; enables the **SP-wire** slice.

## Context

SP-wire closes the SP-intent `/debrief` residual: the live `request_permission` PDP hardcodes a single `ToolAllowlist`, so `PathScope`, `SpendCap`, and `IntentLock` never run live. To fix this the PDP must dispatch the **configured** verifier set for the run — the applied `[gates.permit]` block.

The ratified design ([`docs/design/permit-engine.md`](../design/permit-engine.md) §5/§6, [DR-008](DR-008-permit-engine-pivot.md)) pins the `[gates.permit]` TOML shape and establishes that `request_permission` *is* the PDP, but is **silent on how the applied config + folded per-run state reach the transport/substrate-agnostic core**. `McpCore` holds only fabric / badges / substrate / cas; the run→workspace map is not on the core. The opened-workspace registry lives on the daemon (`bins/rezidentd/src/runs.rs`), where permit config is folded from `workspace.spec.applied`, keyed by workspace.

The oracle built the fork-free aggregation board (`permit::aggregate`, `rezidnt-gate`) but **STOPPED at this seam** and flagged it for `/dr`. Three ways to route config + state to the core were considered:

- **(a) an `McpSubstrate` trait method** — the daemon resolves the applied `[gates.permit]` verifier set for a run and returns it to the core via a new method (e.g. `permit_config_for(run)`). `McpCore` folds the per-run state it needs (`AgentRunState.permit_accumulators`, `AgentRunState.intent`) from the **fabric it already holds** — exactly as `resources_read` already folds via `rezidnt_state::fold` — and injects that state + the resolved verifier params as content-pinned `inputs.params` (determinism BINDING). RECOMMENDED.
- **(b) an `McpCore::with_permit_config(...)` builder** fed a spec/state source keyed by run/workspace — this plumbs the run→workspace map into the core, a weaker (a) that pushes registry knowledge onto the transport-agnostic core.
- **(c) fold-config-from-log** — run→workspace from `agent.spawned`, then `workspace.spec.applied.spec_ref` → parse the spec from CAS. The most I3-pure, but heavier per request and couples the core to `rezidnt-run` spec parsing.

**Strongest counterargument (dissent, recorded verbatim per house style):** the honest dissent is that **(c) is the more invariant-pure choice**: seam (a) makes the *config selection* reach the core through a trait CALL rather than a log fold, a small I3 concession at the core boundary (the core no longer derives the deciding config purely from the log it can see — it trusts the substrate's answer). **Counter to the counter:** the config remains log-DERIVED — the daemon folds it from `workspace.spec.applied` (I3 upheld daemon-side); the trait method only relocates that fold to where the workspace registry already lives (I4: config resolution is a substrate capability, like `open_project`/`spawn_agent`); and the decision remains fully replayable/interrogable because the emitted decision fact still records `policy_ref` (the deciding verifier's params, pinned to CAS) — so `gate_explain` and `debrief` replay are unaffected. (c) can still be adopted later as a pure-fold optimization without breaking the trait contract. **The owner accepts seam (a) knowingly.**

## Decision

1. **Ratify seam (a): a new `McpSubstrate` trait method.** The daemon resolves the applied `[gates.permit]` verifier set for a run from its opened-workspace registry (folded from `workspace.spec.applied`, keyed by workspace) and returns it to `McpCore` via a new method (e.g. `permit_config_for(run)`). This is additive to the core seam; no invariant text is rewritten.

2. **The core folds its own per-run state.** `McpCore` folds `AgentRunState.permit_accumulators` and `AgentRunState.intent` from the **fabric it already holds** via `rezidnt_state::fold` — the same discipline `resources_read` already uses — and injects that state + the resolved verifier params as content-pinned `inputs.params` (determinism BINDING).

3. **Bare cores degrade honestly.** A bare `McpCore` with **no substrate wired resolves to NO config → escalate/deny, never a synthesized allow** (I6) — the same honest-degradation discipline as the other mutating tools. Config resolution is a substrate capability; its absence produces no permission.

4. **Reject (b) and (c) for SP-wire.** (b) is a weaker (a) that plumbs the run→workspace map onto the transport-agnostic core. (c) is fenced, not lost: it can be adopted later as a pure-fold I3 optimization without breaking the (a) trait contract.

## Consequences

- **§9 MCP-surface delta:** `McpSubstrate` gains one method (`permit_config_for(run)` or equivalent) — additive to the core seam; bare cores degrade to escalate/deny.
- **SP-wire injection tests un-ignore.** `crates/rezidnt-mcp/tests/permit_wire_dispatch.rs` (currently `#[ignore]`-gated pending this DR) unblock against this seam and assert the configured `PathScope`/`SpendCap`/`IntentLock` dispatch live.
- **No ontology / `/subject` change.** This is a core-seam wiring decision; the `permit.*` taxonomy (SP5) is untouched.
- **`permit::aggregate` unaffected and built first.** The fork-free aggregation board (`rezidnt-gate`) the oracle already landed sits below this seam; it is the input this decision routes config into.
- **No test or acceptance criterion is weakened by this record.** In plain words: the bare-core path *degrades to escalate/deny rather than allow* — this is a tightening, not a softening; a core with no config resolver grants no permission.
- **Docket note:** the previously-floated "empty-vs-absent intent semantic" DR candidate (raised in the SP-intent `/debrief`) is **unrelated to this seam** and shifts to a later record number; it is not resolved here.

*Amendments to this record require DR-012.*
