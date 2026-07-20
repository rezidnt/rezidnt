[← Decision records index](../rezidnt-architecture.md#20-decision-records) · [Architecture plan](../rezidnt-architecture.md)

# Decision Record DR-021 — Live spend-cap enforcement (C1): the spend_delta source and producer seam

**Date:** 2026-07-20 · **Status:** ACCEPTED (owner) · **Amends:** §8/§9 (permit engine — makes the C1 spend-cap enforcement path LIVE, not the current inert wiring) and, under the ratified B2, the reducer **fold source** (`spend_delta_usd` moves OFF the pre-action permit decision fact onto a post-action metering fact). No invariant text is rewritten. **Implies a downstream warden `/subject`** (a new `action.metered` subject) — flagged here, NOT designed here. · **Cites:** intel memo [`intel/001-omnigent-permission-governance.md`](../../intel/001-omnigent-permission-governance.md) (C1 = "spend/rate-limit … currently uncovered," "each needs its own DR" — DR-002 rule 3, memo-motivated); [DR-009](DR-009-match-omnigent-scope.md) (owner-accepted C1 into SP1 scope as table-stakes). · **Builds on:** [DR-008](DR-008-permit-engine-pivot.md) (PDP/PEP pivot, soft→ASK/hard→DENY verdict shape), [DR-011](DR-011-permit-pdp-config-seam.md) (the PDP config seam that injects permit params), SP5 accumulators/reducers (`PermitAccumulators`, the `spend_delta_usd`→`cumulative_spend_usd` fold, `rezidnt-state/src/lib.rs:725-729`).

## Context

The `SpendCap` native verifier already exists and is correct (`rezidnt-gate/src/lib.rs:680-746`, registered in `builtin_natives`): projected `= cumulative + cost`; under soft → Pass; soft ≤ projected < hard → Inconclusive/escalate (never coerced, I6, DR-008 §4); ≥ hard → Fail; window count ≥ rate limit → Fail; caps missing → cannot-run. **It is inert.** The PDP injects only `cumulative_spend_usd` into permit params (`rezidnt-mcp/src/lib.rs:831-834`), not `soft_cap_usd`/`hard_cap_usd`/`action_cost_usd`, so the verifier returns cannot-run every time. And the emit site hard-codes `DecisionDeltas::default()` (`:896-904`), so `spend_delta_usd` is never produced — the reducer folds ZERO forever and `cumulative_spend_usd` never moves. DR-009 folded C1 into SP1, but SP1 shipped tool + path only. This record decides how to make C1 live and honest. Three coupled questions:

**A — where "this action's spend" comes from at PRE-ACTION decision time.** A permit decision fires *before* the action runs, so the actual $ cost is unknown. (1) Per-request estimate from the harness/PEP (agent-reported → trust/honesty concern); (2) fixed per-tool cost table in policy config (deterministic/replayable, coarse); (3) deferred/post-action attribution (decision folds nothing; actuals ride a later fact); (4) estimate + reconcile.

**B — the I3 honesty question (the CRUX).** Under I3 the log is truth; a `spend_delta_usd` on a `permit.granted` fact is a durable, replayable CLAIM the reducer sums and the *next* decision reads as if it were incurred cost. If it's a pre-action estimate, the fact asserts "this action cost $X" before the action ran — or when it was DENIED and cost nothing. Two honest resolutions: **B1** — keep the delta on the permit fact but relabel it an *authorization estimate* and redocument `cumulative_spend_usd` as "cumulative authorized/estimated spend"; internally consistent but a dishonest-if-misread accumulator. **B2** — move spend attribution OFF the pre-action decision entirely; measured actuals ride a POST-action fact; C1 enforces on a lagging-but-truthful cumulative. B2 means `spend_delta_usd` may not belong on the permit fact at all — reshaping the reducer's fold source.

**C — the producer-seam shape (depends on B).** For a verifier-produced delta (A=1/2 ∧ B1), the delta must flow verifier→fact, but `VerifierOutput` (`:95-100`) has no spend field and `PermitOutcome` (`permit.rs:342`) has no delta field, so the emit site cannot pass one. (i) add `spend_delta_usd` to `VerifierOutput` — a §8 exec-verifier STDOUT CONTRACT change touching every exec golden + replay; (ii) add a delta field to `PermitOutcome`, populated by the aggregator from the deciding verifier (natives-only, no §8 break); (iii) if B2 wins, the seam is not verifier-produced at all — it is PEP-reported on a post-action fact, and `VerifierOutput`/`PermitOutcome` stay UNCHANGED.

**Strongest counterargument (dissent, recorded verbatim per house style):** *"Live spend-caps are premature enforcement breadth (§18 scope gravity) for a delta whose honest source (B2) requires a whole new metering fact plus a warden `/subject` — a lot of machinery before any user has asked to cap spend. The flat tool-allowlist and path-scope gates already cover the common case; the SpendCap verifier can sit inert behind its config keys until a real spend-cap user appears, at which point the honest source can be chosen against a concrete need instead of guessed at now."* **Counter to the counter:** DR-009's owner accepted C1 as table-stakes for displacing Omnigent (memo C1/C3), knowingly; the SpendCap verdict logic already exists and is inert only for want of wiring; and B2's post-action fact can seed off `agent.completed`'s already-shipped `cost.total_usd`/`input_tokens`/`output_tokens` (`spec/ontology.md:207`), so the "whole new machinery" is smaller than it looks — the token/$ data is already on the log, only the per-action attribution grain is new. The scope-gravity concern is honored by fencing the metering `/subject` behind the warden and shipping nothing until oracle criteria exist. **The owner has accepted this trade knowingly.**

## Decision

- **A → option 3 (deferred / post-action attribution).** The permit decision folds NO spend; measured actuals attribute after the action ran. Estimates (1/4) are rejected as the *fold* source because a summed estimate becomes indistinguishable from actuals downstream; a fixed cost table (2) may still be layered later as a projection input to the verifier's `action_cost_usd` for the *soft-band forecast* without ever being folded — that is a follow-on, not decided here.
- **B → B2 (spend attribution moves off the pre-action permit decision).** Most I3-honest: spend is measured, not guessed; a lagging cumulative is a defensible spend-cap model (you stop *after* the action that crossed, exactly as a real budget behaves). `spend_delta_usd` on `permit.granted`/`permit.denied` is retired as the C1 fold source. B1 is recorded as the runner-up (simpler, one fewer subject) and explicitly rejected on the dishonest-if-misread-accumulator ground.
- **C → option (iii).** Because B2 wins, the seam is NOT verifier-produced. `VerifierOutput` (`:95-100`) and `PermitOutcome` (`permit.rs:342`) stay UNCHANGED — no §8 STDOUT contract change, no exec-golden/replay churn. The delta rides a post-action metering fact the PEP reports; the reducer's fold arm moves from `permit.*` to that new fact. This is a point in B2's favor, not a coincidence.
- **The metering fact and its subject are NOT designed here.** Introducing `action.metered` (or attributing per-action off `agent.completed`) is a **warden `/subject` session** — a new subject with its own `v1` payload and reducer arm. This record commits the *direction* (post-action, measured, off the permit fact); the subject shape is that session's to settle.

## Invariant-fit

| Inv | Fit |
|---|---|
| **I3 (the crux)** | B2 keeps the log honest: folded `cumulative_spend_usd` = *measured incurred* spend, never a pre-action guess or a phantom charge on a denied action. B1 would fold estimates and require relabeling the accumulator so nobody reads it as incurred — internally consistent but fragile. B2 chosen for fidelity. |
| **I6** | Unchanged. SpendCap's verdict logic is already deterministic + interrogable (`:686-746`); it stays cannot-run until real caps are injected (garbage never coerces to pass). A lagging cumulative is still a content-hashed, replayable input. |
| **I2** | Caps (`soft_cap_usd`/`hard_cap_usd`/`rate_limit`) are small inline scalar params, well under 32 KiB; the metering fact carries scalar $ + token counts, no bulk payload. No CAS pressure. |
| **I7** | No new dependency; pure Rust wiring + one fold-arm move. |
| **I1/I4/I5/I8** | Untouched. |

## Consequences

- **§8/§9 delta:** the SpendCap enforcement path becomes live: the PDP config seam (DR-011) must inject `soft_cap_usd`/`hard_cap_usd`/`rate_limit`/`window_action_count` from `[gates.permit]` config (mirroring the `role` injection at `:828-829`), so the verifier can run instead of cannot-run. The `DecisionDeltas::default()` at `rezidnt-mcp/src/lib.rs:903` STAYS default for spend — spend no longer flows through the permit fact.
- **Reducer fold-source delta (§reducer):** `rezidnt-state/src/lib.rs:725-726` (the `payload["spend_delta_usd"]` → `cumulative_spend_usd` arm) moves from the `permit.*` reducer to the new post-action metering fact's reducer. `risk_delta` (C6) is untouched and stays on the permit path.
- **Ontology delta (deferred to `/subject`):** `spend_delta_usd?` retires from the `permit.granted`/`permit.denied` payload (`spec/ontology.md:364,370`) as the C1 fold source; the new metering subject gains it. `cost_ms?` is unaffected. This is a warden gate, not this DR's to write.
- **Risk-register (§18):** *scope-gravity risk* — carried, mitigated by the mandatory oracle-first slice + the `/subject` fence (no build until criteria exist). *lagging-enforcement risk (new)* — a spend cap under B2 is enforced one action LATE (the crossing action completes, then the next is blocked). This is stated plainly and is acceptable for a budget model; product copy must not claim pre-emptive spend blocking. This weakens nothing that shipped — it names the honest limit of the chosen model.
- **No shipped test or acceptance criterion is weakened.** Concretely: B2 does not soften any existing gate; it declines to fold *estimates*, which were never folded (the path was inert). The exec-verifier STDOUT contract and every exec golden are UNCHANGED (C=iii). New criteria arrive with the C1 slice via the oracle.

## Acceptance-criteria sketch (what `/oracle` encodes once the C1 build slice starts)

1. Config `[gates.permit]` carrying `soft_cap_usd`/`hard_cap_usd` causes the PDP to inject them; SpendCap runs (not cannot-run) and returns Pass under soft.
2. Cumulative measured spend driven past soft → Inconclusive → Escalate/ASK; past hard → Fail → Deny (memo benchmark scenario 3, `:113`).
3. `window_action_count ≥ rate_limit` → Deny independent of spend.
4. A post-action metering fact folds its measured delta into `cumulative_spend_usd`; a `permit.*` fact folds NO spend (the fold-source move is asserted, not just the total).
5. A DENIED action contributes ZERO to `cumulative_spend_usd` (no phantom charge — the B2 honesty property, replayable from the log).

## What this does NOT decide

- The metering subject shape (`action.metered` vs off-`agent.completed`) — a warden `/subject` session.
- **C6 risk-delta / running risk score** — stays fenced; its own later DR may reuse this post-action seam.
- **`cost_ms`** — a separate no-DR slice shipping in parallel; recorded-only, no accumulator (`spec/ontology.md:362`), untouched here.
- **C3** (sandbox/egress/credential) — remains fenced behind DR-009's own-DR requirement.
- The fixed per-tool cost table as a soft-band *projection* input (A option 2, non-folded) — a possible follow-on, not decided.

*Amendments to this record require DR-022.*
