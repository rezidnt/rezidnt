# Design sketch — C3 sole-chokepoint enforcement (sandbox · egress proxy · credential brokering)

**Status:** PROPOSED (design-first per [DR-002](../decisions/DR-002-prior-art-protocol.md) rule 1) · **Feeds:** C3's own implementation DR (owner sign-off — the fence [DR-009](../decisions/DR-009-match-omnigent-scope.md) §Decision mandates) · **Builds on:** [DR-009](../decisions/DR-009-match-omnigent-scope.md) (scoped C3 as "a distinct, later sole-chokepoint enforcement phase … too large to design inline"), [DR-008](../decisions/DR-008-permit-engine-pivot.md) (PDP/PEP split), [permit-engine](permit-engine.md) §3/§10.1 (the sole-chokepoint phase boundary), intel memo [`001-omnigent-permission-governance.md`](../../intel/001-omnigent-permission-governance.md) C3 + honest-counters (DR-002 rule 3, black-box) · **Owner:** TwofoldTech LLC

> Not BINDING. Committed before any `/oracle` and before the implementation DR, exactly as DR-009 requires ("REQUIRES its own design sketch plus its own implementation DR before any build"). Nothing here is built until that DR is ACCEPTED. This sketch decomposes C3 and recommends a first slice; it does not commit an implementation.

## 1. Scope — the one gap rezidnt does not match

The Omnigent read (memo 001, C3, "high") found the single capability rezidnt does **not** cover: Omnigent's enforcement is a genuine execution chokepoint built from three layered primitives —

1. **OS sandbox** — bubblewrap (Linux), seatbelt (macOS); Windows degraded to Job Objects (process containment, *no* filesystem/network isolation). Contains the agent process so it *cannot* escape mediation.
2. **L7 egress proxy** — intercepts/transforms the agent's network requests at the application layer.
3. **Credential brokering** — the proxy injects a secret (e.g. a GitHub token) only on an approved egress, so **the agent never sees the credential**.

Today rezidnt's permit engine is honest about the limit (permit-engine §10.1, DR-008 Consequences): enforcement rides the harness `PreToolUse` hook (PDP/PEP split), so it is **bounded by the hook surface** and can be *out-enforced* by a product that owns the process. C3 is the phase that closes this — it shifts rezidnt's posture from *"the PDP rides the harness hook"* toward *"rezidnt is the execution chokepoint the agent runs inside."* This is the memo's Q8 gap and design §3's explicitly-deferred "later, bigger phase."

## 2. Why this is fenced (and stays fenced)

DR-009's recorded dissent names C3 as the archetypal scope-gravity risk: *"re-implementing boring sandbox/proxy/credential-broker infrastructure that competes for the hours that should build the evidence/audit wedge."* That dissent is **correct as a warning** and this sketch honors it two ways:

- **C3 decomposes into three independently-shippable primitives** (§4). We do not build all three as one monolith; each is its own slice with its own exit demo, and the roadmap can stop after any one of them.
- **The wedge stays the wedge.** C3's value to rezidnt is *not* "match Omnigent's sandbox" — it is **one log, both axes** applied to enforcement: a sandbox/egress/credential decision becomes a durable, replayable fact on the same fabric as `debrief` (I3). We build the chokepoint *so its decisions are auditable*, not for enforcement breadth alone. A sandbox that logs nothing replayable is Omnigent's game on Omnigent's turf; a sandbox whose every denial replays as evidence is ours.

## 3. The load-bearing architectural fit — chokepoint is a substrate (I4), decision stays core

The PDP/PEP split (design §3) does **not** change; C3 *strengthens the PEP*. Today the PEP is the harness hook. C3 adds a **stronger PEP that rezidnt owns**: the process runs inside a rezidnt-controlled sandbox whose network is forced through a rezidnt-controlled proxy. But the decision — allow/deny/escalate on any given action — remains a **permit-verifier** on the existing `permit` gate (design §4). The verdict contract is unchanged (`pass→allow / fail→deny / inconclusive→escalate`, I6, never coerced).

So the new surface area is **enforcement mechanism (substrate), not policy engine**:

- **`SandboxSubstrate` (I4 trait).** `bwrap` on Linux, `sandbox-exec`/seatbelt on macOS, degraded on Windows — behind one trait, selected by platform, exactly like the run/git substrates (DR-001). The sandbox launch wraps the S1 spawn seam (the daemon already owns the PTY and spawns the harness).
- **`EgressProxy` (substrate).** A loopback L7 proxy the sandboxed process is forced to use (`HTTP(S)_PROXY` + blocked direct egress inside the sandbox netns). Each intercepted request is a `permit` decision.
- **The permit-verifiers stay native/exec** (design §6). "May this egress proceed?" and "may this credential be injected?" are ordinary permit-verifiers returning the three-valued verdict — no new decision engine.

This keeps C3 an *extension of shipped primitives* (the design §2 discipline), not a second pillar: gate engine = policy engine; C3 only adds the muscle that makes the policy unavoidable.

## 4. Decomposition — three slices, independently shippable

| Slice | Primitive | Rides | Foundation-for | First-slice fit |
|---|---|---|---|---|
| **C3a** | OS sandbox (Linux `bwrap` first) | the S1 spawn seam (daemon owns the process) | C3b/C3c (you must contain the process before you can force its egress) | **Recommended first** — foundation, Linux-native (matches the live WSL/unix surface), exec-based (bwrap is an external tool, I7-clean like git-CLI), and demonstrable alone (fs/path confinement is real value without the proxy). |
| **C3b** | L7 egress proxy | C3a's netns (block direct egress, force the loopback proxy) | C3c | Second — the differentiated primitive, but only trustworthy *given* C3a (without the sandbox the agent bypasses the proxy). |
| **C3c** | Credential brokering | C3b (inject-on-approved-egress) | — | Last — the highest-value scenario (memo #7/#17: secret never reaches the agent) but strictly rides the proxy. |

**Recommended first slice: C3a (Linux OS sandbox behind `SandboxSubstrate`).** Rationale: it is the containment foundation the other two *require* (a proxy the agent can bypass is theater); it rides the existing run-substrate spawn seam rather than opening new surface; it is Linux-first, which is exactly where the daemon runs live today (the Windows sandbox tier is gated behind the deferred native-Windows Platform phase — see §6); and it yields a standalone exit demo (an agent confined to its worktree, a write outside it denied-and-logged) without needing the proxy.

## 5. C3a design detail (the recommended first slice)

- **Substrate.** `trait SandboxSubstrate { fn spawn_confined(&self, plan: &SpawnPlan, policy: &SandboxPolicy) -> Result<Child> }`. Linux impl execs `bwrap` with `--ro-bind`/`--bind` the allowed paths (the worktree + toolchain), `--unshare-all`, `--die-with-parent`; the daemon keeps the PTY (S1 invariant — the daemon owns the process, not the client). No `bwrap` present → the substrate reports **unavailable**, and the run degrades to the current unsandboxed spawn **loudly** (a logged `sandbox.unavailable` fact), never silently (I6, memo scenario #9 "degraded-enforcement honesty").
- **Policy source.** The sandbox policy (allowed binds, unshared namespaces) comes from the project spec `[gates.permit]`/role layer, folded like every other permit input — **never a self-declared spawn arg** (the C6 privilege-escalation lesson: an input that *widens* confinement must come from folded authority, handoff §maker/checker rule).
- **The durable facts (warden `/subject`, gated — not minted here).** Candidate subjects: `sandbox.spawned {run, backend, binds_ref, policy_ref}`, `sandbox.denied {run, attempted_path, policy_ref}` (or ride the existing `permit.denied` axis — the warden's call), `sandbox.unavailable {run, backend, reason}`. Each needs a folding reducer (no consumer-less subjects, DR-006). Whether a sandbox denial is a new subject or a `permit.denied` variant is a `/subject` question, flagged not settled.
- **The verifier.** A `PathConfinement` native permit-verifier already has a sibling in `ForbiddenPath`/`DiffScope` (permit-engine §2 table) — the deterministic decision "is this path inside the confinement?" is pure and interrogable; the sandbox is the *mechanism* that makes the deterministic verdict unbypassable.

## 6. Cross-platform — honest degradation, and the tie to the deferred Platform phase

The sandbox tier is inherently platform-split, and this is where C3 touches the wedge honestly:

| Platform | Backend | Isolation | Status |
|---|---|---|---|
| Linux | `bwrap` | fs + network namespaces | **C3a target** (live surface today) |
| macOS | `sandbox-exec` (seatbelt) | fs + network | C3a follow (second backend behind the same trait) |
| Windows | Job Objects / AppContainer | process containment; **no fs/network isolation** (matches Omnigent's own Windows degradation, memo C3) | **gated behind the native-Windows Platform phase** (Phase 3), which the owner deferred — see the transport/Platform fork |

The Windows sandbox tier **requires** the native-Windows daemon (Job Objects, AppContainer, named-pipe transport) that is the deferred Platform work. So C3 and Platform are coupled at exactly one point: **full Windows enforcement parity waits on Platform.** This is coherent with the deferral — C3a/C3b/C3c ship Linux-first (and macOS), and Windows degrades *loudly* (I6) until the Platform phase lands the native sandbox. Product copy must not claim Windows interception parity before then (echoes memo honest-counter + design §10.1). No reorder of the deferral is implied.

## 7. Invariant fit

| Inv. | Fit |
|---|---|
| **I1** zero pixels | The sandbox/proxy are headless daemon-side mechanisms; the escalate/ask surface is a client, not the daemon. ✓ |
| **I2** plane split | A permit decision carries action metadata (≤32 KiB); request bodies, sandbox bind lists, and egress payloads are CAS refs (`binds_ref`, `args_ref`). ⚠️ egress-proxy throughput is a data-plane concern — §8. |
| **I3** log is truth | Every sandbox launch, denial, egress decision, and credential injection is a durable fact; the enforcement graph is a pure fold; **decisions replay as evidence** — the reason to build C3 at all (§2). ✓ |
| **I4** substrates behind traits | `SandboxSubstrate`/`EgressProxy` are substrate capabilities selected by platform; the decision (PDP) stays core; absent a backend the run degrades explicitly. ✓ |
| **I5** MCP-first | No new client verb required for C3a (sandboxing wraps spawn); C3b/C3c egress-approval escalations route through the existing `request_permission`/escalate surface. ✓ |
| **I6** deterministic + interrogable | The confinement/egress/credential verdicts are `pass/fail/inconclusive`, replayable, and `gate_explain` names the deciding policy; **unavailability is a logged fact, never a silent allow** (memo #9). ✓ |
| **I7** one static binary | `bwrap`/`sandbox-exec` are external OS tools invoked like the git-CLI (exec, not linked) — zero new linked dependency for C3a. The C3b proxy's crate choice (hand-rolled loopback vs. a minimal TLS-terminating proxy crate) is a **C3b DR question**, evaluated against I7 the way DR-016/SP4b evaluated the macaroon crate — not decided here. ✓ for C3a |
| **I8** clean-room | bwrap/seatbelt are OS tooling, not Omnigent code. The intel memo is our own black-box read (DR-002); no AGPL source is read or ported. ✓ |

## 8. Honest risks & tensions

1. **Scope gravity (the DR-009 dissent, restated and load-bearing).** Three sandbox/proxy/credential primitives could consume the roadmap that should build the evidence wedge. Mitigation: the §4 decomposition — each primitive is its own slice with its own DR and exit demo; the roadmap may stop after C3a; and §2 binds C3's justification to *replayable decisions*, not enforcement breadth. **The owner accepts or rejects this trade at the C3a implementation DR.**
2. **Sole-chokepoint is a posture claim that must not overclaim (memo honest-counter, design §10.1).** Even with C3a shipped, enforcement is only sole-chokepoint for the primitives that shipped and the platforms that support them. Product copy states what interception is real per platform/primitive; Windows and the un-built primitives degrade loudly.
3. **The chokepoint must be inescapable to mean anything.** A sandbox with a hole (a writable bind that reaches outside, a leaked env var enabling direct egress) is worse than none — it *looks* enforced. C3a's most-tested surface: an agent's attempt to write outside its binds, or to reach the network with no netns, is **denied and logged**, and the property that confinement cannot be widened by any run-supplied input (the C6 escalation lesson).
4. **Egress-proxy latency & throughput (C3b, deferred).** An L7 proxy on every network call sits on the agent's critical path and pressures the fabric's ≤~10³ events/min budget (design §10.2). A C3b concern; log-all-denials, sample/compact allows — resolved in the C3b DR, not here.
5. **TLS interception for credential brokering (C3c) is a trust decision.** Injecting a token on approved egress means terminating TLS at the proxy (the agent trusts a rezidnt-minted CA). This is a real security-posture commitment (the daemon holds the injected secrets and a signing CA) that C3c's DR must state as a threat-model boundary, mirroring SP4b's root-key threat model.

## 9. Decisions the C3a implementation DR must ratify

1. **Decomposition + first slice (§4) — recommended: C3a (Linux `bwrap` sandbox) first**, C3b/C3c fenced behind their own DRs. The DR ratifies the decomposition and that the roadmap may stop after any primitive.
2. **`SandboxSubstrate` trait shape + the bwrap invocation** (binds from folded policy, `--die-with-parent`, daemon keeps the PTY) — confirmed in the DR.
3. **Degradation contract (§5/§6) — RECOMMENDED: unavailable backend ⇒ a logged `sandbox.unavailable` fact + loud degrade, never a silent unsandboxed spawn** (I6). The DR confirms this is the honest default.
4. **Windows tier gated behind the deferred Platform phase (§6)** — the DR records the coupling (full Windows enforcement waits on native-Windows) and that no reorder of the Platform deferral is implied.
5. **Sandbox facts** — a new `sandbox.*` subject family vs. riding the existing `permit.*` axis: a **warden `/subject`** question, flagged here, minted in that gated session — not designed in the DR.
6. **Threat model** — what C3a defends against (an agent cannot read/write outside its confinement; a confinement policy cannot be widened by a run-supplied arg) and what it does not (a compromised *daemon* holds the sandbox policy — out of scope, same root-of-trust boundary as SP4b).

## 10. What this sketch does NOT decide

- **C3b (egress proxy) and C3c (credential brokering) design** — each is its own design sketch + DR after C3a lands (§4). The proxy crate/TLS-CA choices are theirs.
- **The `sandbox.*` taxonomy** — subjects/reducers are a warden `/subject` session (§9.5).
- **The Windows native sandbox** — gated behind the deferred Platform phase (§6); C3 does not pull it forward.
- **The Omnigent-baseline benchmark run** (rezidnt vs Omnigent on the memo's 10-scenario suite, esp. #7/#9/#17) — a separate black-box activity (DR-002 rule 6), not this slice; it consumes C3, it does not gate it.
- **Any micro-benchmark of proxy/sandbox overhead** — a measured concern for C3b, not a C3a acceptance criterion.
