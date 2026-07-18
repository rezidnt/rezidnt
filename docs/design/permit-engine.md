# Design sketch — native permit engine (the pre-hoc governance pillar)

**Status:** PROPOSED (design-first per [DR-002](../decisions/DR-002-prior-art-protocol.md) rule 1) · **Feeds:** DR-008 (scope pivot) + a warden `/subject` pass · **Owner:** TwofoldTech LLC

> This sketch is not BINDING. It exists so the *rezidnt* design is committed in writing **before** any scoped `/intel` read of Omnigent, and so DR-008 has something concrete to ratify. Nothing here is built until the DR is ACCEPTED.

## 1. Thesis — rezidnt owns both axes

Today rezidnt is positioned as the *complement* to Omnigent: Omnigent gates what an agent **may** do (pre-hoc permissions); rezidnt proves what it **did** (post-hoc evidence). See plan §8: *"the models compose."* This sketch reverses that: rezidnt takes the **may** axis natively and Omnigent becomes a benchmark baseline / optional adapter, not a required companion.

The unifying claim — and the reason this is *not* just cloning Omnigent — is **one log, one replay, both axes.** A permission decision and its later evidence live in the same append-only fabric, so you can replay *"would this policy have allowed this action?"* against history exactly the way `debrief` replays a verdict. Nobody ships that.

## 2. The good news — the seam already exists

This is an extension of shipped primitives, not a new pillar from zero:

| Primitive | Where | What it already does |
|---|---|---|
| `vet` gate (pre-spawn) | plan §8; `crates/rezidnt-gate` | policy decision on the agent spec before spawn |
| Native verifiers | `crates/rezidnt-gate/src/lib.rs` (`BareMode`, `DiffScope`, `ForbiddenPath`) | deterministic policy checks |
| Exec-verifier contract | plan §8 | polyglot policy via any argv program speaking JSON |
| Badges | plan §12 + [DR-005](../decisions/DR-005-badge-consolidation.md) | per-`AgentRun` capability tokens `{workspace, verb set, expiry}` |
| Verdict contract | I6 | `pass \| fail \| inconclusive`, replayable, interrogable |

The gap is only the **middle of the lifecycle**: today enforcement is at the *edges* (pre-spawn `vet`, post-hoc `debrief`) and per-action enforcement is delegated to the harness's own `--allowedTools`. The permit engine fills the middle.

## 3. The core architecture — PDP / PEP split

The load-bearing decision. Standard authorization split:

- **rezidnt is the Policy Decision Point (PDP).** It holds policy, decides, and records. This is core, headless, in-daemon.
- **The harness is the Policy Enforcement Point (PEP).** claude-code's `PreToolUse` hook (and equivalents) intercept the action and *ask* rezidnt before proceeding. Enforcement is a **substrate capability** (I4): where a harness exposes a hook, we get true interception; where it does not, enforcement degrades gracefully to pre-spawn `vet` + post-hoc evidence, and we say so.

```
agent about to act
  └─ harness PreToolUse hook (PEP)  ──request_permission──▶  rezidentd (PDP)
                                                              │  permit gate:
                                                              │   native + exec
                                                              │   permit-verifiers
     ◀── allow | deny | ask ─────────────────────────────────┘  → logged fact
```

This keeps rezidnt honest: it can only enforce as strongly as the PEP allows. A later, stronger phase can make rezidnt the sole execution chokepoint (all actions via rezidnt-mediated MCP tools), but that is not required for v1 and is called out as a phase boundary, not assumed.

## 4. The `permit` gate lifecycle point

Add a fourth lifecycle point to the gate model (§8: `vet` / `pre_merge` / `post_run`):

- **`permit`** — per-action, mid-run. A **permit-verifier** is just a verifier attached to this gate, so **the gate engine *is* the policy engine** — no second engine to build.

The verdict contract maps to authorization with zero new vocabulary, honoring I6 (never coerced):

| Verdict | Permit meaning |
|---|---|
| `pass` | **allow** |
| `fail` | **deny** |
| `inconclusive` | **escalate to a human** (routed to a client, never auto-resolved) |

`inconclusive → ask-a-human` is the honest default the whole product is built on, applied to permissions.

## 5. The MCP tool + wire shape

`request_permission` joins the MCP surface (I5 — MCP-first), alongside `open_project`, `spawn_agent`, `gate_explain`, `tail_events` (`crates/rezidnt-mcp/src/lib.rs`). Also reachable over the socket for the harness hook.

```json
// request  (control-plane fact; large context is a CAS ref, never inline — I2)
{ "gate": "permit", "badge": "…", "action": "tool.invoke",
  "target": { "tool": "Bash", "args_ref": "cas:blake3:…" },
  "workspace": "…", "run": "…" }
// reply
{ "decision": "deny", "reason": "path outside allowed scope",
  "policy_ref": "cas:blake3:…", "evidence": [ { "kind": "finding", "msg": "…" } ] }
```

Mutating actions already require a badge (§12 + DR-005); `request_permission` is **read-class** on the decision but its *result* authorizes a mutation, so the badge is the caller's identity here.

## 6. Policy model — bring-your-own DSL, don't build one

Policies are verifiers, so we inherit the two-kind model instead of inventing a language:

- **Native permit-verifiers** (Rust) for the common cases: tool allowlist/denylist, path scope, secret-access gate, rate/− budget limits, network egress.
- **Exec permit-verifiers** for everything else — this is where an **OPA/Rego or Cedar** policy plugs in as an argv program speaking the §8 JSON contract. The DSL story is "use a mature one," not "write ours."

Policies live in the project spec (§13), extending the existing `[[agent]]` / `[gates.*]` blocks:

```toml
[[agent]]
name = "impl"
role = "contributor"          # NEW: RBAC seam

[gates.permit]                 # NEW lifecycle point
verifiers = [
  { native = "tool-allowlist", params = { allow = ["Read", "Edit", "Bash"] } },
  { native = "path-scope",     params = { allow = ["src/checkout/**"] } },
  { exec   = "policy/cedar",   params = { policy = "policies/agent.cedar" } },
]
on_inconclusive = "escalate"   # allow | deny | escalate  (default escalate)
```

## 7. RBAC & delegation — where the PROVISIONAL badge work lands

- **Roles** attach to the AgentSpec (`role` above); policies key on role + workspace + action.
- **Delegation** is the *real delegation use case* [DR-005](../decisions/DR-005-badge-consolidation.md) said would justify promoting **macaroon-attenuated badges** from PROVISIONAL. A lead agent delegates a narrowed capability to a sub-agent by attenuating its badge — cryptographically, offline-verifiable, no central lookup. This also closes the §19 open-decisions item ("macaroon-attenuated badges — needs a real delegation use case").

## 8. New event subjects (warden `/subject`, gated)

The permit lifecycle needs taxonomy, minted through the warden — never edited directly:

```
permit.requested            # {run, action, target_ref, badge}
permit.granted | permit.denied | permit.escalated   # {…, policy_ref, evidence_ref}
```

Each subject needs a folding reducer (no consumer-less subjects — the DR-006 precedent). This makes the permission stream first-class in `tail`, the fleet board, and `rebuild`.

## 9. Invariant fit

| Inv. | Fit |
|---|---|
| **I1** zero pixels | PDP is headless; the `escalate`/ask-a-human surface is a *client*, not the daemon. ✓ |
| **I2** plane split | permit requests carry action metadata (≤32 KiB); args/diffs/context are CAS refs. ⚠️ see §10 (throughput). |
| **I3** log is truth | every deny/escalate is a durable fact; the permission graph is a pure fold; **decisions are replayable** — the core novelty. ✓ (allow-logging volume: §10) |
| **I4** substrates behind traits | enforcement (PEP) is a substrate capability; decision (PDP) is core; harnesses without a hook degrade explicitly. ✓ |
| **I5** MCP-first | `request_permission` ships as an MCP tool before any keybinding. ✓ |
| **I6** deterministic + interrogable | permit verdicts are `pass/fail/inconclusive`, replayable, and `gate why`/`gate_explain` returns the deciding policy + evidence. ✓ |
| **I7** one binary, no telemetry | native verifiers in-binary; exec verifiers are local argv. ✓ |

## 10. Honest risks & tensions (must survive DR-008)

1. **Enforcement is only as strong as the PEP.** rezidnt cannot intercept an action a harness won't route through its hook. "Replace Omnigent" is bounded by the harness's hook surface until/unless rezidnt becomes the sole execution chokepoint (a later, bigger phase). State this in the product copy — do not overclaim interception.
2. **Hot-path latency & fabric throughput.** Per-action permit checks sit on the agent's critical path, and the fabric is designed for ≤~10³ events/min (§5). Chatty agents could blow that. Mitigation: a decision **fast-path cache** keyed by `(policy hash, request shape)`; **always log deny/escalate**, and log allows in a compacted/sampled form. This slightly pressures I3 — resolve it explicitly in the DR (the safe default: log all decisions; optimize only if measured).
3. **Strategic dilution (the strongest counterargument).** rezidnt's defensible wedge (§18) is *evidence-gates — "none of which their model rewards."* Entering the permissions arena fights Omnigent head-on where they have a head start, and risks blurring the one thing nobody else does. **Counter:** the permit+verify unification on a single replayable log is itself novel, and the marginal build cost is bounded because the seam (vet + badges + verifier kinds) already exists. This dissent belongs verbatim in DR-008.

## 11. Roadmap (proposed — ratified only by DR-008)

A new phase between gates (Phase 2) and the terminal fidelity layer (Phase 3):

- **SP0** — `permit` lifecycle point + verdict→decision mapping (reuse gate engine). *Accept:* a permit-verifier returns allow/deny/inconclusive, logged.
- **SP1** — `request_permission` MCP tool + socket path; native permit-verifiers (tool-allowlist, path-scope). *Accept:* an agent asks and is denied on a forced policy breach; `gate why` explains it.
- **SP2** — harness PEP integration (claude-code `PreToolUse` hook → permit endpoint). *Accept:* a real mid-run tool call is blocked by policy, one take.
- **SP3** — policy-as-exec-verifier (OPA/Rego or Cedar). *Accept:* an external policy file decides a permit.
- **SP4** — roles + macaroon-attenuated delegation (promotes DR-005 PROVISIONAL).
- **SP5** — warden `/subject` for `permit.*` + folding reducers.
- **Benchmark** — rezidnt vs Omnigent on a permission-policy suite (DR-002 rule 6 permits black-box runs).

## 12. Process gates before code

1. **DR-008** — ratify the scope pivot: amends §1 (non-goals — drop "compose only"), §8 (positioning), §16/roadmap; records the §10.3 dissent. Owner sign-off required.
2. **`/intel`** — *after* this sketch freezes, a scoped read of Omnigent's policy/enforcement model to gap-check coverage (DR-002 rules 1–3). Any change it motivates gets its own DR.
3. **`/subject`** — warden mints `permit.*` with reducers.
4. Then, and only then, the oracle-first slices above.
