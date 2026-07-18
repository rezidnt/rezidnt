[← Decision records index](../rezidnt-architecture.md#20-decision-records) · [Architecture plan](../rezidnt-architecture.md)

# Decision Record DR-012 — declared-empty vs absent intent allowlist (intent-lock)

**Date:** 2026-07-18 · **Status:** ACCEPTED (owner) · **Decision:** option **B** (distinguish). **Amends:** the intent-lock design ([`docs/design/intent-lock.md`](../design/intent-lock.md) §4 off-task verdict) + [DR-010](DR-010-intent-lock-scope.md) §8 crit-2 "intent-absent" clause — clarifies declared-empty vs absent; no invariant text rewritten. **Cites:** — · **Builds on:** [DR-010](DR-010-intent-lock-scope.md) (intent-lock scope), [DR-011](DR-011-permit-pdp-config-seam.md); design basis [`docs/design/intent-lock.md`](../design/intent-lock.md).

## Context

`IntentLock` (the C7 intent-lock native, DR-010) today treats two distinct situations identically, both → `cannot_run` → Inconclusive → escalate:

- **intent ABSENT** — no `run.intent.declared` fact on the log for the run (`AgentRunState.intent == None`); and
- **intent DECLARED-empty** — a `run.intent.declared` fact WITH `allowed_tools: []` (`AgentRunState.intent == Some(IntentState { allowed_tools: [] })`).

At `crates/rezidnt-gate/src/lib.rs:784-789`, `string_list(p, "allowed_tools")` yields `[]` for **both** a missing key and an explicit `[]`, and `if allowed.is_empty() { cannot_run(...) }` collapses them. The SP-wire params injection collapses them the same way (injects an empty vec for both `None` and `Some([])`). This was flagged in the SP-intent `/debrief` and carried again as a docket note in the SP-wire `/debrief` (DR-011 Consequences).

The choice was between:

- **Option A (status quo — keep the collapse):** declared-empty == absent == cannot-run → escalate. Simplest; consistent with DR-010 as originally written ("no allowlist pinned / empty declared set" → cannot-run). Cost: a deliberate lockdown (`allowed_tools: []` meaning "this run may use NO tools") cannot be expressed as deny-everything even under `on_off_task = deny` — it only ever escalates as cannot-run.
- **Option B (RATIFIED — distinguish):** a DECLARED-empty allowlist means **"every tool is off-task"** and routes through the normal off-task path — escalate by default, deny under `on_off_task = deny` (DR-010's knob) — with interrogable evidence (e.g. "intent permits no tools"). Intent ABSENT stays cannot-run → escalate (we genuinely do not know the intent). This makes an explicit empty declaration a real least-privilege LOCKDOWN control, and stops treating a positive operator declaration as mere absence.

**Why B is cheap and honest:** the discriminator ALREADY EXISTS in derived state — `AgentRunState.intent` is `Option<IntentState>`, `None` = absent, `Some(IntentState{allowed_tools: []})` = declared-empty. No ontology / `/subject` change is needed (the `run.intent.declared` subject and reducer are untouched). Only two places erase the distinction and must faithfully propagate it: (1) the SP-wire params injection must signal declared-ness — only inject the `allowed_tools` param key when `intent` is `Some`, leaving the key ABSENT when `None`; and (2) `IntentLock` must distinguish key-absent (→ cannot-run, absent) from key-present-but-empty (→ every tool off-task, declared lockdown). Both are small, local changes; no shared type (`Evidence`/`VerifierOutput`) broadened. Conflating "operator declared no tools" with "no declaration on record" is a quiet semantic dishonesty — the same class of thing DR-011's fidelity fix addressed; distinguishing them is faithful to what the log actually says.

**Strongest counterargument (dissent, recorded verbatim per house style):** the honest dissent is YAGNI / scope-gravity: is a "declared-empty allowlist" a real use case, or speculative complexity? An operator wanting lockdown can already set `on_off_task = deny` with a deliberately narrow (non-empty) allowlist, or simply not spawn the agent; adding a third semantic to a native that already has three verdict forks is complexity for a marginal case, and every distinction the permit natives carry is surface that SP2's PEP integration must keep straight. **Counter to the counter:** the distinction is nearly free — the `Option` discriminator already exists in state, so B is *faithful propagation* of information the log already carries, not new machinery; and "declared no tools → deny everything under the deny knob" is exactly the maximally-restrictive least-privilege posture C7/intent-lock exists to enable (design [`docs/design/intent-lock.md`](../design/intent-lock.md) §1), which A silently cannot express. **The owner has weighed this and ratified B knowingly.**

## Decision

**Ratify option B — distinguish declared-empty from absent.** `IntentLock` treats key-PRESENT-but-empty `allowed_tools` as **every tool off-task** — routed through the existing off-task path, honoring `on_off_task` (escalate default, deny under the knob), with interrogable evidence naming that the intent permits no tools. Key-ABSENT remains cannot-run → escalate (genuinely absent). The SP-wire params injection propagates declared-ness: the `allowed_tools` key is injected only when `AgentRunState.intent` is `Some`, and OMITTED when `None`. No ontology / `/subject` change; the `run.intent.declared` subject and reducer are untouched.

This is scope + semantics, not an implementation. The build is a follow-on oracle-first slice, not delivered by this record.

## Consequences

- A small `IntentLock` change: key-present-empty routes to the off-task path (honoring `on_off_task`); key-absent stays cannot-run.
- The SP-wire params injection propagates declared-ness — the `allowed_tools` key is omitted when `intent == None`, present (possibly `[]`) when `intent == Some`.
- A new/updated `intent_lock_native.rs` test distinguishing the two cases (declared-empty → off-task/deny under the knob; absent → cannot-run).
- **No ontology / `/subject` change.** The `run.intent.declared` subject and reducer are untouched.
- **DR-010 §8 crit-2d "intent-absent" wording is clarified** to mean genuinely-absent (`Option::None`), not declared-empty. In plain words: this does not weaken crit-2 — it narrows the "absent → escalate never a pass" clause to true absence, and it *tightens* the declared-empty case from an escalate-only cannot-run into a real deny-capable lockdown under `on_off_task = deny`. No test or acceptance criterion is loosened.
- This is a follow-on implementation slice (oracle-first: `/oracle` → implementer → `/vet` → `/debrief`) after acceptance, not built by this DR.

*Amendments to this record require DR-013.*
