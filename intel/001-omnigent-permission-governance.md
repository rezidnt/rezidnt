# Intel memo 001 — Omnigent permission-governance capabilities
**Date:** 2026-07-17 · **Analyst session** · **DR-002 scoped read**

Scope: sizes the DR-008 permit-engine replacement ([`docs/decisions/DR-008-permit-engine-pivot.md`](../docs/decisions/DR-008-permit-engine-pivot.md),
design [`docs/design/permit-engine.md`](../docs/design/permit-engine.md)). Design-first is satisfied, so this read is permitted.
This is **capability-extraction for positioning/benchmark/risk-register only** (DR-002 rule 2) — not trait or ontology input.
SP0–SP5 below refer to the permit-engine design sketch §11.

**Target identification (high confidence):** "Omnigent" is Databricks' open-source **meta-harness** for AI agents,
open-sourced ~June 2026 under Apache-2.0 (`omnigent-ai/omnigent`, also offered managed on Databricks). It sits *above*
coding harnesses (Claude Code, Codex, Cursor, Pi, OpenCode, agent SDKs) and drives them through a uniform API. No other
product by this name surfaced in the read. This is the same Omnigent already named across the plan and DR-008.

## Questions this read must answer
1. What does Omnigent govern — policy/permission model; which agent actions/resources?
2. Enforcement model — pre-hoc only or runtime per-action? How does it intercept (hooks, proxy, sole chokepoint)?
3. Policy expression — DSL, declarative config, or code?
4. RBAC / roles / delegation / attenuation?
5. Human-in-the-loop approval & escalation?
6. Integration/enforcement surface into harnesses?
7. Observability/audit — what it records about decisions?
8. Coverage gaps / weaknesses our single-log permit+verify unification structurally beats.

## Findings

- **Q1 — What it governs** (confidence: high, 2026-07-17): Policies govern agent actions across **shell commands, file
  edits, and token/model spend**, plus resource-level rules (e.g. "write only to docs the agent created this session").
  Governance is at the **meta-harness layer, not via prompts**. Source: Databricks blog "Introducing Omnigent"
  (https://www.databricks.com/blog/introducing-omnigent-meta-harness-combine-control-and-share-your-agents); Help Net
  Security (https://www.helpnetsecurity.com/2026/07/06/omnigent-open-source-ai-agent-framework/).

- **Q2 — Enforcement model = runtime, per-action** (confidence: high): Not pre-hoc-only. Omnigent **intercepts tool calls
  at the meta-harness layer** and evaluates a policy on **every action** before it proceeds. Interception is layered:
  (a) tool-call interposition via the wrapping runner; (b) an **OS sandbox** (bubblewrap on Linux, seatbelt on macOS;
  Windows is degraded — Job Objects for process containment, **no filesystem/network isolation**); (c) an **L7 egress
  proxy** that can intercept/transform network requests and broker credentials (e.g. inject a GitHub token only on
  approved egress so the agent never sees it). Sources: Contextual-Policies blog
  (https://www.databricks.com/blog/contextual-policies-omnigent-using-session-state-better-govern-ai-agents); repo README
  (https://github.com/omnigent-ai/omnigent); Help Net Security (as above).
  - *Interception surface (moderate confidence):* the runner "wraps any agent in a sandboxed session with a uniform API"
    and intercepts tool calls at that wrapper; the public docs do **not** state that it uses each harness's own native
    hook (e.g. Claude Code `PreToolUse`). The chokepoint appears to be Omnigent's own wrapper + sandbox + proxy, i.e.
    Omnigent aims to *be* the execution chokepoint rather than ride a harness hook. Marked moderate because the exact
    interposition mechanism per harness is not spelled out in the sources read.

- **Q3 — Policy expression = code (Python functions)** (confidence: high): A contextual policy is **a Python function**
  that takes the prior session state plus the event the agent is attempting, and returns state updates plus a decision.
  Config references handlers as `type: function` with a `handler` pointing at a Python callable (built-ins like
  `omnigent.policies.builtins.safety.ask_on_os_tools`, `...cost.cost_budget`); built-ins are then parameterized in
  server config / agent YAML. So: **policy = imperative Python + declarative YAML wiring**, not a purpose-built DSL.
  Note: the **managed Databricks** offering supports built-in contextual policies but **disallows custom arbitrary-code
  policy functions** — a hosting constraint worth flagging. Sources: Contextual-Policies blog; repo README; Databricks
  docs (https://docs.databricks.com/aws/en/omnigent/).

- **Q4 — RBAC / roles / delegation** (confidence: moderate, and genuinely ambiguous — see below): Policy **scoping**
  exists at three levels — **server-wide (admin), per-agent (developer), per-session (user)** — with stricter
  session-level rules checked first. That is scope precedence, **not** classic role-based access control. On true RBAC the
  sources conflict: one search summary states **"enterprise RBAC and audit are planned"** / "await before regulated
  workloads"; Databricks docs mention **integrating with the workspace identity provider** (SSO-ish) and the OSS auth
  supports Google/GitHub/Okta/Microsoft sign-in (invite-only). No source describes **capability delegation or
  attenuation** (a lead agent narrowing a sub-agent's grant). **Confidence: moderate that a formal enterprise RBAC model
  is not yet GA; low on the precise roadmap wording** (only one aggregator asserted "planned"; primary docs neither
  confirm nor deny). Sources: repo README; Databricks docs; search aggregate incl.
  Help Net Security. **Credential brokering** (hide-and-inject via the egress proxy) is the closest thing to delegation
  and is present today (high confidence).

- **Q5 — Human-in-the-loop = first-class** (confidence: high): Policy decisions are three-valued —
  **ALLOW / DENY / ASK**. ASK **pauses execution and requests user sign-off** before proceeding. Approval is
  **context-aware**: the same action (e.g. send email / share file) can be auto-allowed early in a session and require
  approval once accumulated risk or spend crosses a threshold. Example given: require human approval to `git push` after
  the agent installed a new npm package; pause after every \$100 spent. Sources: Contextual-Policies blog;
  Introducing-Omnigent blog; repo README.

- **Q6 — Integration / enforcement surface** (confidence: high on the abstraction, moderate on mechanism): Omnigent is a
  **meta-harness** presenting a **common interface above** terminal coding agents (Claude Code, Codex, Cursor, Pi,
  OpenCode, Hermes) and SDK agents (OpenAI Agents, Claude Agent SDK). An agent is declared in **a short YAML file**;
  swapping harness/model is "one line." Enforcement rides the wrapper + sandbox + egress proxy rather than (per the
  sources) each harness's own hook API. Sources: Introducing-Omnigent blog; repo README; Databricks docs.

- **Q7 — Observability / audit** (confidence: low-to-moderate): Policies are explicitly **stateful** — they accumulate
  per-session state (documents read, cumulative spend, running risk score, original user intent) and decide the next
  action from that context; cost tracking is called out concretely. Sessions + working directories are the collaboration
  surface (view, comment, replay files together). **But no primary source read here describes a durable, decision-level
  audit log, its record format, retention, or replayability of past decisions.** "Hardened audit trails" appear in the
  same "await before regulated workloads" bucket as RBAC in the one aggregator summary. **This is the least-documented
  area and the biggest verified gap** (see Q8). Sources: Contextual-Policies blog; Introducing-Omnigent blog; search
  aggregate.

- **Q8 — Weaknesses / where we can do better** (confidence: mixed, per item below in "Do it better").

## Capability matrix (Omnigent → rezidnt permit-engine answer / gap)

| # | Omnigent capability (cite, confidence) | rezidnt answer (map to SP0–SP5 / gap) |
|---|---|---|
| C1 | Governs shell / file-edit / token-spend + resource rules (blog, high) | **Matched.** Native permit-verifiers: tool-allowlist, path-scope, plus budget/rate limits. **SP1** covers tool+path; **spend/rate-limit verifiers = currently uncovered**, add as native permit-verifiers (new). |
| C2 | Runtime per-action interception on every tool call (blog+README, high) | **Matched in intent, weaker at first.** `permit` gate + `request_permission` = **SP0/SP1**; real mid-run block via harness PEP = **SP2**. rezidnt rides the harness hook (PDP/PEP split), so early enforcement is bounded by hook coverage — Omnigent's own sandbox/proxy chokepoint is broader on day one. **Partial / phased gap.** |
| C3 | OS sandbox (bwrap/seatbelt) + L7 egress proxy + credential brokering (README/HNS, high) | **Gap (uncovered).** rezidnt v1 has no OS sandbox and no egress proxy; credential brokering is not in the sketch. This is Omnigent's strongest enforcement primitive and rezidnt does **not** match it. Flag for risk-register; a later "sole chokepoint" phase (design §3, §10.1) would be the earliest place to close it. **New / uncovered.** |
| C4 | Policy = Python function (state, event) → (state', decision) (blog/README, high) | **Matched via a different, stronger contract.** Policy = a verifier on the `permit` gate (native Rust or exec argv+JSON), decision `pass/fail/inconclusive`. **SP0/SP3.** We deliberately do **not** build a policy language — exec permit-verifier hosts a mature DSL (Cedar/Rego). |
| C5 | Three-valued ALLOW/DENY/ASK with pause-for-approval (blog/README, high) | **Matched natively.** I6 verdict maps 1:1 — `pass→allow / fail→deny / inconclusive→escalate-to-human`, `inconclusive` never coerced. **SP0.** |
| C6 | Stateful/contextual policy (running risk score, cumulative spend, session intent) (blog, high) | **Matched by architecture, and arguably better.** Session state that Omnigent holds imperatively = a **pure fold over the log** for rezidnt (I3); risk/spend are reducers. Requires `permit.*` subjects + reducers = **SP5**. Contextual/stateful permit-verifiers read materialized state = new work but on existing rails. |
| C7 | Intent-based authorization — lock tools to the initial user prompt (least-privilege vs prompt injection) (blog, high) | **Gap (uncovered).** rezidnt has no notion of "derive an allowlist from the initiating prompt." Interesting and injection-relevant; **new**, would be a native permit-verifier keyed on run intent. Positioning candidate. |
| C8 | Three-level policy scope: admin / developer / session, stricter-first (README, moderate) | **Partial.** rezidnt policy lives in the project spec (`[gates.permit]`) + roles on `[[agent]]`; precedence across org/dev/session layers is **not specified** in the sketch. **Gap in the scoping model** — SP4 (roles) is the natural home; layered precedence is new. |
| C9 | RBAC / SSO / enterprise identity — "planned"/IdP-integrated (docs/aggregate, moderate–low) | **Comparable maturity, different shape.** Omnigent's formal RBAC appears not-yet-GA; rezidnt roles = `role` on AgentSpec + policy keying (**SP4**). Neither is a mature enterprise IdP story. Not a differentiator either way today. |
| C10 | Capability delegation / attenuation | **rezidnt is ahead (hypothesis).** No source shows Omnigent doing cryptographic capability attenuation; its analogue is credential brokering. rezidnt's **macaroon-attenuated badges** (offline-verifiable, no central lookup) = **SP4** and appear to have **no Omnigent equivalent**. Positioning candidate (moderate confidence, absence-of-evidence). |
| C11 | Audit / durable decision record (aggregate, low) | **rezidnt is structurally ahead — the core wedge.** No primary source shows Omnigent recording every decision as a durable, replayable fact. rezidnt logs every permit decision to the **single append-only fabric**, so a permission decision is replayable *as evidence* (design §1, I3). **SP0 onward.** This is the "one log, both axes" claim and the thing to lead with. |
| C12 | Managed offering disallows custom-code policies (docs, high) | **rezidnt sidesteps this.** Exec permit-verifiers are local argv programs in a one-static-binary product (I7) — no managed-hosting arbitrary-code restriction applies. Minor positioning point. |

## Benchmark-suite seed (rezidnt vs Omnigent — DR-002 rule 6 permits black-box runs)

Concrete permission scenarios a benchmark harness should score (black-box, install-and-run both):
1. **Tool-allowlist deny** — agent attempts a tool outside its allowlist; expect DENY. Score: blocked? decision logged? explanation retrievable?
2. **Path-scope breach** — write outside the allowed path glob; expect DENY.
3. **Spend cap** — drive cumulative model spend past a soft cap (→ pause/ASK) then a hard cap (→ DENY).
4. **Risk-accumulation escalation** — a sequence of individually-benign sensitive actions crosses a threshold and flips a later action from ALLOW to ASK. Tests *stateful* policy.
5. **Prompt-injection / intent lock** — inject an off-task instruction; test whether tools unrelated to the original prompt are blocked (Omnigent's intent-based authorization vs a rezidnt intent verifier if built).
6. **Human-approval round-trip** — an ASK/escalate pauses execution and resumes correctly on approve/deny.
7. **Credential non-exposure** — verify a secret never reaches the agent while an approved egress call still succeeds (Omnigent egress proxy; rezidnt likely fails this today — records the C3 gap honestly).
8. **Decision replay / audit** — after a run, can you reconstruct *every* permit decision and *why*? (rezidnt's expected win; measures C11.)
9. **Degraded-enforcement honesty** — on a harness/platform with no hook or no sandbox (e.g. Windows), does the tool block, or silently allow? Both products must be scored on what they claim vs do.
10. **Latency overhead** — per-action permit-check added latency on a chatty agent (rezidnt risk per design §10.2).

## Do it better (positioning / hypotheses — NOT design mandates)

- **One log, both axes (lead with this).** Omnigent holds session/policy state imperatively and shows no durable
  decision-audit story; rezidnt puts every permit decision on the same append-only log as evidence, so
  *"would this policy have allowed this action?"* replays against history. Strongest structural advantage (C11, high on
  the rezidnt side, low on Omnigent's audit maturity → this is where a benchmark, not assertion, should settle it).
- **Deterministic + interrogable decisions.** Omnigent policies are arbitrary Python (nondeterministic-capable);
  rezidnt permit-verifiers inherit I6 — same content-hashed inputs → same verdict, replayable, and `gate why` returns the
  deciding policy + evidence. Positioning: *auditable authorization*, not just enforced authorization.
- **Bring-your-own mature DSL vs bespoke Python.** Omnigent's policy-as-Python is flexible but ad hoc and (in managed
  form) restricted; rezidnt hosts Cedar/Rego as an exec verifier — a real policy language with its own tooling.
- **Delegation as cryptographic attenuation.** rezidnt's macaroon-attenuated badges (offline-verifiable) have no observed
  Omnigent equivalent beyond credential brokering (C10). Hypothesis, absence-of-evidence — verify before it becomes copy.

**Honest counters (do NOT overclaim):**
- Omnigent's **OS sandbox + L7 egress proxy + credential brokering** (C3) is a genuinely broader enforcement chokepoint
  than rezidnt's PDP/PEP-via-harness-hook design for v1. rezidnt can be *out-enforced* until a later sole-chokepoint
  phase. Product copy must not claim parity on interception breadth (echoes DR-008 Consequences + design §10.1).
- Omnigent's **stateful/contextual policies and intent-based authorization** (C6, C7) are shipping features; rezidnt
  matches C6 by architecture but must actually build the reducers, and C7 is uncovered new work.

## Implications for rezidnt (positioning / benchmark / risk-register only — not directives)
- **Positioning:** lead on *auditable, replayable, deterministic* authorization unified with evidence on one log; do not
  lead on enforcement breadth.
- **Benchmark:** the 10-scenario seed above becomes the rezidnt-vs-Omnigent suite (the design sketch §11 "Benchmark" line).
- **Risk-register:** confirm two deltas already in DR-008 — (1) enforcement bounded by PEP/hook surface (Omnigent's
  sandbox/proxy is broader, C3); (2) the missing OS-sandbox/egress-proxy/credential-broker capability is a real coverage
  gap, not just a phase deferral.
- **Coverage gaps to feed a future DR (each needs its own DR to become design):** spend/rate-limit permit-verifiers (C1),
  intent-based authorization verifier (C7), layered admin/dev/session precedence (C8), and the sandbox/egress/credential
  primitives (C3).

## Coverage gaps (taxonomy diff — for the warden `permit.*` pass, SP5)
- Omnigent's stateful policies imply per-session accumulators (risk score, cumulative spend, tool-use-vs-intent) that a
  rezidnt reducer would need to materialize from `permit.*` events — noted so the `/subject` pass sizes the payloads for
  contextual (not just stateless) permit decisions. Not a subject definition; a sizing note only.

---
Design changes motivated by this memo require a DR citing it (DR-002 rule 3). No competitor code structure is reproduced above.
