[← Decision records index](../rezidnt-architecture.md#20-decision-records) · [Architecture plan](../rezidnt-architecture.md)

# Decision Record DR-010 — SP-intent scope + criteria (C7 intent-lock)

**Date:** 2026-07-18 · **Status:** ACCEPTED (owner) · **Amends:** §16 (roadmap — gives the DR-009 SP-intent slice concrete acceptance criteria; no invariant text is rewritten). **Cites:** intel memo [`intel/001-omnigent-permission-governance.md`](../../intel/001-omnigent-permission-governance.md) C7 (DR-002 rule 3 — memo-motivated). **Builds on:** [DR-008](DR-008-permit-engine-pivot.md), [DR-009](DR-009-match-omnigent-scope.md); design basis [`docs/design/intent-lock.md`](../design/intent-lock.md).

## Context

DR-009 committed C7 (intent-lock) to the roadmap as the **SP-intent** slice, but as a *note only, with no acceptance criteria*. The design sketch [`docs/design/intent-lock.md`](../design/intent-lock.md) now freezes the rezidnt design (design-first, DR-002 rule 1) so this DR has something concrete to ratify. This record makes SP-intent buildable: it fixes scope, ratifies the one load-bearing fork, and adopts the §8 criteria. It is scope + criteria, not an implementation.

The load-bearing fork (design §3): the run's intent→allowed-tool-set must be **DECLARED once and content-pinned**, never inferred live inside the verifier. A live LLM derivation at decision time would be non-deterministic and non-replayable — it breaks the determinism BINDING and I6 `debrief` replay. Two ways to *form* the allowlist: **(a) an explicit intent manifest** (deterministic, on existing SP1 rails, ships now) and **(b) a recorded out-of-band derivation** (non-determinism captured once at the edge, replayable thereafter).

**Strongest counterargument (dissent, recorded verbatim per house style):** two-part. First, the DR-009 §18 scope-gravity argument applies again — "differentiation is evidence-gates … none of which their model rewards"; every hour on permission *breadth* is an hour off the audit/evidence wedge that is the actual differentiator. Second, and more damning, **(a)'s value depends entirely on a narrow declared manifest — an over-broad allowlist defeats the whole point (design §7.1), and the actually-novel part, automatic least-privilege, lives in (b), which is deferred.** So SP-intent *as scoped here* is table-stakes least-privilege-in-time, **not** the injection-proof headline; product copy must not claim "blocks prompt injection" — it *surfaces off-task tool use for a human, deterministically* (design §7.2). **Counter to the counter:** the slice is deliberately bounded (one native verifier + one subject + a pinned fold, all on SP1 rails); (b) is *fenced, not lost* — it has a reserved note and can arrive without a determinism violation because non-determinism is recorded at the edge; and the escalate-default (below) is honest about what (a) does and does not prove. **The owner accepts this trade knowingly.**

## Decision

1. **Ratify SP-intent scope + acceptance criteria.** Adopt the §8 criteria of [`docs/design/intent-lock.md`](../design/intent-lock.md) as SP-intent's definition of done: an `intent-lock` native permit-verifier registered in `builtin_natives()`; `run.intent.declared` minted and folding to a rebuild-stable per-run intent state; in-intent tool → allow, off-task → escalate (deny under the hardened knob) with interrogable evidence naming the off-task tool + the intent, intent-absent → escalate never a pass; `gate_explain` surfaces the escalation/denial (reason + policy_ref + evidence_ref); and the **accept demo** (memo 001 scenario 5) — a declared on-task intent, an injected off-task request blocked (escalated), an on-task request passing, one take, replayable. This extends DR-009's roadmap note; no invariant text changes.

2. **Ratify the §3 fork.** The intent→allowed-tool-set is **DECLARED and content-pinned**, read by the verifier, never re-derived at decision time (protects the determinism BINDING + I6). **(a) explicit intent manifest is the SP-intent deliverable** — RECOMMENDED and in-scope. **(b) recorded out-of-band derivation is DEFERRED** behind its own later note; it is not built in SP-intent and must not be pulled in.

3. **Off-task verdict default.** Off-task tool use → `Inconclusive` / `permit.escalated` — routed to a human, never coerced to a pass (I6). Policy knob **`on_off_task = escalate | deny`, default `escalate`**; `deny` hardens to `Fail`/`permit.denied` for high-assurance runs. This follows the SP1 `on_inconclusive` precedent (same discipline as `SpendCap` with missing caps).

4. **Process gates before code.** A warden `/subject` pass must mint `run.intent.declared v1 { run, intent_ref: CasRef, allowed_tools: [string] }` + its folding reducer (per-run intent state) **before** oracle-first slicing. This subject is **distinct from `agent.spawned.allowed_tools?`**: that records the *composed harness allowlist* (what the agent was configured with); `run.intent.declared` records the *intent-derived least-privilege set* the verifier enforces — a distinct axis warrants a distinct subject (DR-006 precedent). Sequence: DR-010 sign-off → warden `/subject` → `/oracle` → implementer → `/vet` → `/debrief`.

## Consequences

- **§16 roadmap delta:** the DR-009 SP-intent note gains concrete acceptance criteria (§8) and a ratified (a)-vs-(b) fork; (b) is added as a fenced later note, not a slice.
- **Risk-register (§18) deltas:**
  - *Overclaim risk.* SP-intent as scoped is least-privilege-in-time, not injection-proofing; product copy must say "surfaces off-task tool use for a human," never "blocks prompt injection" (design §7.2). Mitigated by the escalate-default and this recorded limit.
  - *Manifest-breadth risk.* (a) is only as strong as a narrow declared allowlist (design §7.1); an over-broad manifest silently defeats the control. Flagged; the auto-least-privilege answer is (b), deferred.
  - *Scope-gravity (DR-009 echo).* Mitigated by keeping SP-intent to (a) + the verifier + the subject and fencing (b).
- **No test or acceptance criterion is weakened by this record.** It *adds* criteria to a previously criteria-free slice. The one honest softening to record in plain words: the escalate-**default** means an off-task request is not hard-blocked unless the operator sets `on_off_task = deny` — the default surfaces-and-escalates rather than denies, by design (I6, never coerce). SP-intent's criteria are met by the escalate path; hard-deny is exercised only under the knob.

*Amendments to this record require DR-011.*
