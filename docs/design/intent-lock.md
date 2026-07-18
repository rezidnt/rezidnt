# Design sketch — intent-lock (C7: intent-based authorization)

**Status:** PROPOSED (design-first per [DR-002](../decisions/DR-002-prior-art-protocol.md) rule 1) · **Feeds:** a `/dr` (DR-010) ratifying SP-intent scope + criteria, then a warden `/subject` pass · **Owner:** TwofoldTech LLC · **Cites:** intel memo [`intel/001-omnigent-permission-governance.md`](../../intel/001-omnigent-permission-governance.md) C7 (DR-002 rule 3 — memo-motivated).

> Not BINDING. This freezes the *rezidnt* design for C7 before the ratifying DR, so DR-010 has something concrete to sign off. Nothing here is built until the DR is ACCEPTED and the subjects are minted through the warden.

## 1. Thesis — bind tools to the run's initiating intent
C7 (memo 001, "high", **uncovered gap**): lock an agent's usable tools to the task it was spawned for, so an **off-task instruction — a prompt injection — cannot reach tools the original intent never needed** (least-privilege in time). DR-009 committed this as the **SP-intent** slice and named it a positioning differentiator. Omnigent ships intent-based authorization imperatively; rezidnt expresses it as *one more native permit-verifier reading per-run state folded from the log* (I3) — the same shape as C1 spend/rate (SP1).

## 2. The seam already exists (what SP-intent adds)
SP1 shipped the `permit` gate, native permit-verifiers reading content-pinned `inputs.params`/`refs`, and per-run/per-session accumulators folded from `permit.*`. SP-intent adds exactly two things:
1. a per-run **intent state** (the task + its allowed-tool set) folded from a new fact, and
2. an **`intent-lock`** native permit-verifier that checks a `permit.requested` tool against that state.
The novelty is not the verifier plumbing — it is *what "intent" is* and *how the allowlist is formed without breaking determinism.*

## 3. The load-bearing decision — intent is DECLARED and PINNED, never inferred inside the verifier
The determinism BINDING (gate-authoring: same content-hashed inputs → same verdict) **forbids a live LLM call inside the verifier** — an intent→allowlist inference re-run at decision time would be non-deterministic and non-replayable, breaking I6 and `debrief` replay. So the run's intent→allowed-tool-set is **captured once as a durable fact and content-pinned**; the verifier reads the pinned intent state, never re-derives it. Two ways to *form* the allowlist — the fork DR-010 must ratify:

- **(a) Explicit intent manifest (RECOMMENDED for SP-intent).** The spawn/task declares an explicit tool allowlist tied to its intent (project spec `[gates.permit]` / agent spec). Fully deterministic, on existing rails, ships now. The "intent" text is recorded for interrogation; the *enforced* set is the declared list.
- **(b) Recorded derivation (later extension).** An out-of-band step (harness, or a dedicated exec-verifier) derives an allowlist from the initiating prompt **once**, records it as the intent fact; the verifier reads the recorded set. Non-determinism lives at the *edge* (recorded once, replayable thereafter), never in the verifier. Deferred behind its own note — (a) is the SP-intent deliverable.

This split keeps SP-intent honest and shippable while leaving room for the smarter derivation without a determinism violation.

## 4. The `intent-lock` verifier (a native permit-verifier on the `permit` gate)
On a `permit.requested`, compare the requested `target.tool` (and action) against the run's pinned intent allowlist:
- **in-set → `Pass`** (→ `permit.granted`).
- **off-task → the honesty question.** Off-task tool use may be a prompt injection or a benign scope-expansion — rezidnt does not know. So the **default is `Inconclusive` → `permit.escalated`** (route to a human; never coerced, I6), with a policy knob (`on_off_task = escalate | deny`, default `escalate`) to harden to `Fail`/`permit.denied` for high-assurance runs. This matches the product's "ask-a-human is the honest default" and the `on_inconclusive` knob SP1 already models.
- **intent state absent → `Inconclusive`** (cannot-run; never a synthesized pass — same discipline as `SpendCap` with missing caps).
Evidence names the off-task tool and the declared intent (CAS-ref carried, I2), so `gate_explain` explains the block (I6).

## 5. New subject (warden `/subject`, gated) — recording intent
The per-run intent needs a durable fact with a folding reducer (no consumer-less subjects, DR-006 precedent):
```
run.intent.declared v1  { run, intent_ref: CasRef, allowed_tools: [string] }
```
- `intent_ref: CasRef` — the initiating task/prompt text, in the CAS (never inline; I2).
- `allowed_tools: [string]` — the intent-scoped tool set the `intent-lock` verifier enforces.
Folds into a per-run **intent state** (`AgentRunState.intent`: the allowed set + the intent ref), which the verifier reads as a pinned input. **Distinct from `agent.spawned.allowed_tools?`** (SP1/S4), which records the *composed harness allowlist* (what the agent was configured with) — a governance-composition fact, not an intent-derived least-privilege set. The house pattern is a distinct subject for a distinct axis (DR-006 precedent: `integrity.*` not folded into `gate.*`). The warden session decides final shape; this is the proposal.

## 6. Invariant fit
| Inv. | Fit |
|---|---|
| **I2** | intent text is a `CasRef`; the allowlist is a short string list — payload is small scalars + one ref. ✓ |
| **I3** | intent is a durable fact; the enforced set is a pure fold; the verdict replays from log+CAS. ✓ |
| **I6** | off-task → escalate (never coerced to allow); deterministic verdict from pinned intent; `gate_explain` returns the deciding intent + off-task tool. ✓ |
| **determinism** | the verifier reads the pinned intent state from `inputs`, never re-infers — no live model call. ✓ (the whole point of §3) |
| **I4/I5** | intent-lock is a native verifier behind the same trait; enforced at the `permit` gate the MCP/PEP surface already drives. ✓ |

## 7. Honest risks
1. **(a) is only as good as the declared manifest.** An over-broad declared allowlist defeats the point; the value is in narrow, task-scoped declarations. (b) is where automatic least-privilege lives — flagged, not built.
2. **Off-task ≠ malicious.** Escalate-by-default trades some friction for not silently denying legitimate scope growth; the `deny` knob exists for runs that prefer hard-fail. Stated so product copy does not overclaim "blocks prompt injection" — it *surfaces* off-task tool use for a human, deterministically.
3. **Scope gravity (DR-009 dissent echo).** Keep SP-intent to (a) + the verifier + the subject; resist pulling the derivation model (b) into this slice.

## 8. Acceptance criteria (proposed — ratified only by DR-010)
- `intent-lock` native verifier exists and is registered in `builtin_natives()`.
- `run.intent.declared` is minted (warden) and folds to a rebuild-stable per-run intent state.
- A `request_permission` for an **in-intent** tool → allow; an **off-task** tool → escalate (deny under the hardened knob), with interrogable evidence naming the off-task tool + the intent; intent absent → escalate, never a pass.
- `gate_explain` surfaces the intent-lock escalation/denial (reason + policy_ref + evidence_ref).
- **Accept demo (memo 001 scenario 5):** a run declares an on-task intent; an injected off-task tool request is **blocked (escalated)** while an on-task request passes — one take, replayable.

## 9. Process gates before code
1. **DR-010** — ratify SP-intent scope + the §3 declared-vs-inferred fork (recommend (a)), citing memo 001 (DR-002 rule 3). Owner sign-off.
2. **warden `/subject`** — mint `run.intent.declared` + reducer (per-run intent state).
3. Then, and only then, oracle-first: `/oracle` → implementer → `/vet` → `/debrief`.
