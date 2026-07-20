[← Decision records index](../rezidnt-architecture.md#20-decision-records) · [Architecture plan](../rezidnt-architecture.md)

# Decision Record DR-025 — C3a: the Linux OS-sandbox slice — a `bwrap`-backed `SandboxSubstrate` that makes the permit verdict unbypassable, degrading loudly when absent

**Date:** 2026-07-20 · **Status:** ACCEPTED (owner) — ratified 2026-07-20; this is a sole-chokepoint posture commitment the owner signed off on, discharging the DR-009 fence · **Amends:** §16 (the C3 "sole-chokepoint enforcement" phase, committed by [DR-009](DR-009-match-omnigent-scope.md) but never sliced, gains its first slice C3a + acceptance criteria + exit demo) and §18 risk-register (half-enforcement/overclaim + scope-gravity, carried). No invariant text is rewritten. **May imply a downstream warden `/subject`** (a `sandbox.*` subject family vs. riding the existing `permit.*` axis) — flagged here, NOT designed here. · **Builds on:** [DR-009](DR-009-match-omnigent-scope.md) (fenced C3 as "a distinct, later sole-chokepoint enforcement phase … REQUIRES its own design sketch plus its own implementation DR before any build"), [DR-008](DR-008-permit-engine-pivot.md) (PDP/PEP split — this slice strengthens the PEP, not the policy engine), the design sketch [`docs/design/permit-sole-chokepoint-c3.md`](../design/permit-sole-chokepoint-c3.md) §4/§5/§9 (the design basis this DR ratifies), [permit-engine](../design/permit-engine.md) §3/§10.1 (the sole-chokepoint phase boundary + the disclosed lagging-enforcement limit), and the C6 privilege-escalation lesson (DR-024 — an input that widens authority must come from folded authority, never a self-declared arg). · **Cites:** intel memo [`intel/001-omnigent-permission-governance.md`](../../intel/001-omnigent-permission-governance.md) C3 + honest-counters (DR-002 rule 3 — our own black-box read; nothing ported).

## Context

DR-009 folded C3 (sandbox / egress proxy / credential brokering) into the roadmap as Omnigent's one enforcement primitive rezidnt does not match — the genuine execution chokepoint that can *out-enforce* a PDP riding only the harness hook (permit-engine §10.1). It fenced C3 hard: no build without its own design sketch and its own implementation DR. The sketch is now written (`permit-sole-chokepoint-c3.md`); it decomposes C3 into three independently-shippable primitives — C3a OS sandbox, C3b egress proxy, C3c credential brokering — and recommends C3a (Linux `bwrap`) first, because containment is the foundation the other two *require* (a proxy the agent can bypass is theater, sketch §4).

This record ratifies the **C3a scope** for owner sign-off. It is not a line-by-line implementation and mints no ontology. C3a is an *extension of the shipped spawn seam*, not a second pillar: the PDP/PEP split (DR-008) is unchanged. The decision "is this path inside confinement?" stays a native permit-verifier (a `PathConfinement` sibling of `ForbiddenPath`/`DiffScope`) returning the three-valued verdict (I6, never coerced); the sandbox is only the **mechanism that makes that deterministic verdict unbypassable** by confining the process the daemon already spawns and PTY-owns (S1).

## Strongest counterargument (dissent, recorded verbatim per house style)

*"C3a is scope gravity wearing a foundation costume. It is boring OS-sandbox infrastructure — `bwrap` binds, namespaces, degradation plumbing — and every hour spent on it is an hour not spent on the evidence/audit wedge that §18 names as the actual differentiator, the one thing nobody else ships. Enforcement breadth is Omnigent's turf; they have a mature three-layer chokepoint and a head start on it. Matching their sandbox is a footrace we start from behind, on ground chosen by them. And a half-built chokepoint is worse than none: shipping C3a alone, with C3b egress and C3c credential brokering fenced out, produces a sandbox that contains the filesystem but leaves the network wide open — which invites exactly the overclaim DR-009 warned against, product copy implying 'sole chokepoint' when only one of three primitives and only one of three platforms exists. Better to leave C3 a roadmap commitment and keep building the wedge until enforcement breadth is actually the thing blocking a user."*

## Counter to the counter

C3a's justification is **not** enforcement breadth — it is *replayable decisions as evidence* (sketch §2). The wedge stays the wedge: a sandbox that logs nothing replayable is Omnigent's game on Omnigent's turf, but a sandbox whose every denial folds onto the same log as `debrief` (I3, one log both axes) is ours — we build the chokepoint *so its decisions are auditable*, not for breadth alone. The marginal cost is bounded: C3a **rides the existing S1 spawn seam** (the daemon already spawns and PTY-owns the harness) rather than opening new surface, and `bwrap` is exec'd like the git-CLI (zero new linked dependency, I7). The overclaim risk is real and is disarmed by the honest-enforcement defaults below, not deferred: unavailability is a *loud logged fact*, never a silent allow (I6), and the roadmap may **stop after C3a** — C3b/C3c are each fenced behind their own design sketch and DR, so this slice cannot silently consume the roadmap. **The owner accepts or rejects this trade at ratification.**

## Decision (ratify scope — not a line-by-line impl)

- **Ratify the §4 decomposition and C3a-first.** C3 is three independently-shippable primitives; C3a (Linux `bwrap` OS sandbox behind a `SandboxSubstrate`) ships first as the containment foundation. C3b (egress proxy) and C3c (credential brokering) are fenced OUT, each behind its own design sketch + DR. **The roadmap may stop after any primitive** — C3a is not a down-payment obligating C3b/C3c.
- **`SandboxSubstrate` (I4 trait), Linux `bwrap` impl.** Wraps the S1 spawn seam: execs `bwrap` with `--ro-bind`/`--bind` the allowed paths (worktree + toolchain), `--unshare-all`, `--die-with-parent`. **The daemon keeps the PTY** (S1 — the daemon owns the process, not the client).
- **Confinement policy comes from FOLDED authority, never a self-declared spawn arg.** The allowed binds and unshared namespaces are folded from the project spec `[gates.permit]`/role layer like every other permit input. This is the C6 privilege-escalation lesson (DR-024): an input that *widens* confinement must come from folded authority, or the sandbox is escapable-by-argument.
- **Degradation contract (load-bearing, I6):** no `bwrap` present ⇒ a logged `sandbox.unavailable` fact **plus a LOUD degrade** to the current unsandboxed spawn — **never a silent unsandboxed spawn.** This is the honest-enforcement default (memo scenario #9, degraded-enforcement honesty).
- **The verdict stays a native permit-verifier.** A `PathConfinement`-style verifier makes the deterministic "inside confinement?" decision (`pass/fail/inconclusive`, `gate_explain` names it); the sandbox makes that verdict unbypassable. No new decision engine.
- **Windows tier stays gated behind the deferred native-Windows Platform phase (Phase 3).** Full Windows enforcement parity waits on the native-Windows daemon (Job Objects/AppContainer/named-pipe); until then Windows degrades loudly. **No reorder of the Platform deferral is implied.**

## Invariant-fit

| Inv | Fit |
|---|---|
| **I1** zero pixels | The sandbox is a headless daemon-side mechanism; any escalate/ask surface is a client, not the daemon. ✓ |
| **I2** plane split | The permit decision carries action metadata (≤32 KiB); bind lists / policy are CAS refs (`binds_ref`, `policy_ref`), not inline payload. C3a adds no data-plane path (egress throughput is a C3b concern). ✓ |
| **I3** log is truth | Every sandbox launch, denial, and `sandbox.unavailable` degrade is a durable fact; the enforcement graph folds pure; **decisions replay as evidence** — the reason to build C3a at all. ✓ |
| **I4** substrates behind traits | `SandboxSubstrate` is a substrate capability selected by platform, exactly like the run/git substrates (DR-001); the decision (PDP) stays core; absent a backend the run degrades explicitly. ✓ |
| **I5** MCP-first | No new client verb required for C3a — sandboxing wraps the existing spawn. Escalations (C3b/C3c) route through the existing `request_permission`/escalate surface, later. ✓ |
| **I6** deterministic + interrogable | The confinement verdict is `pass/fail/inconclusive`, replayable, and `gate_explain` names the deciding policy; **unavailability is a logged fact, never coerced to a silent allow** (memo #9). ✓ |
| **I7** one static binary | `bwrap` is an external OS tool invoked like the git-CLI (exec, not linked) — zero new linked dependency for C3a. ✓ |
| **I8** clean-room | `bwrap` is OS tooling, not Omnigent code; the intel memo is our own black-box read (DR-002); no AGPL source read, nothing ported. ✓ |

## Consequences

- **§16 delta:** the C3 sole-chokepoint phase (DR-009, previously criteria-less prose) gains its first slice C3a + the acceptance criteria and exit demo below, so `/oracle` has something concrete to encode. C3b/C3c/macOS/Windows remain fenced, each behind its own DR.
- **Risk-register (§18) deltas:** *half-enforcement / overclaim risk (carried, sharpened):* C3a contains the filesystem but not the network (that is C3b) and only on Linux — a reader could mistake it for full sole-chokepoint. Mitigated by the loud-degrade contract (I6), by the exit demo requiring the `sandbox.unavailable` fact to be visible, and by product copy stating what interception is real per platform/primitive (echoes memo honest-counter + permit-engine §10.1). *scope-gravity risk (carried from DR-009):* three sandbox/proxy/credential primitives could consume the wedge's hours; mitigated by the §4 decomposition (each its own slice + DR), by the roadmap being free to stop after C3a, and by binding C3a's justification to *replayable decisions*, not breadth.
- **No shipped test or acceptance criterion is weakened.** This slice ADDS a substrate, a verifier, and criteria; it softens no existing gate, verifier, or golden. The loud-degrade path is not a relaxed criterion — it is the honest, logged report of a missing enforcement backend (I6), announcing itself rather than passing silently.
- **Warden `/subject` (deferred):** whether a sandbox denial is a new `sandbox.*` subject family (`sandbox.spawned`/`sandbox.denied`/`sandbox.unavailable`) or a `permit.denied` variant is the warden's call in a gated `/subject` session (each needs a folding reducer — no consumer-less subjects, DR-006). Flagged here, not designed.

## Acceptance-criteria sketch (what `/oracle` encodes once C3a starts)

1. An agent spawned under the sandbox is **confined to its worktree binds** — it runs, and reads/writes inside the allowed binds succeed.
2. A **write or read outside the binds is DENIED and logged** — a durable denial fact lands on the log, not a silent success and not a crash.
3. The **confinement policy cannot be widened by any run-supplied input** (the C6 escalation property): a spawn arg / agent-supplied value that attempts to add a bind or unshare-exception does not widen confinement — the binds come only from folded authority.
4. **`bwrap` absent ⇒ a logged `sandbox.unavailable` fact + loud degrade to unsandboxed spawn — never a silent allow** (I6); the degrade is visible on the log, not swallowed.
5. **No new linked dependency** — `bwrap` is exec'd like the git-CLI; the workspace gains no new linked crate for the sandbox mechanism (I7).

## Exit demo (made concrete)

One take, headless, socket/CLI only. On a host **with** `bwrap`: an agent runs **confined** to its worktree binds; an **out-of-bounds filesystem access is denied-and-logged**, and that denial **replays from the log** (same recorded facts → same verdict). On a host **without** `bwrap`: the same run **degrades loudly** — a `sandbox.unavailable` fact is logged and the run proceeds unsandboxed, never silently allowed. This is the C3a exit: the Linux sandbox makes the permit verdict unbypassable, and its absence announces itself on the log.

## What this does NOT decide

- **C3b (egress proxy) and C3c (credential brokering)** — each is its own design sketch + DR after C3a lands (sketch §4/§10); the proxy crate and TLS-CA threat model are theirs.
- **macOS (`sandbox-exec`) and Windows (Job Objects/AppContainer) backends** — macOS is a second backend behind the same trait, later; Windows is gated behind the deferred native-Windows Platform phase (sketch §6). No reorder of that deferral is implied.
- **The `sandbox.*` taxonomy** — subjects/reducers are a warden `/subject` session (sketch §9.5), not this DR.
- **The Omnigent-baseline benchmark run** (rezidnt vs Omnigent on the memo's scenario suite, esp. #7/#9/#17) — a separate black-box activity (DR-002 rule 6) that *consumes* C3, it does not gate it.

*Amendments to this record require DR-026.*
