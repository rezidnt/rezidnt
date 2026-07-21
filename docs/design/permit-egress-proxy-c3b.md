# Design sketch — C3b+c: mediated egress + credential brokering (full L7 MITM, folded into one slice)

**Status:** PROPOSED (design-first per [DR-002](../decisions/DR-002-prior-art-protocol.md) rule 1) · **Feeds:** the C3b+c implementation DR (owner sign-off — the [DR-009](../decisions/DR-009-match-omnigent-scope.md) fence) · **Builds on:** [DR-025](../decisions/DR-025-c3a-linux-sandbox.md) (C3a sandbox — the containment this rides), [permit-sole-chokepoint-c3](permit-sole-chokepoint-c3.md) §4 (decomposition — **owner has now FOLDED C3b+C3c into one slice**), [permit-engine](permit-engine.md) §3/§6/§10.2, [DR-017](../decisions/DR-017-permit-macaroon-delegation-sp4b.md)/SP4b (the process-lifetime root-of-trust + threat-model template), intel memo [`001-omnigent-permission-governance.md`](../../intel/001-omnigent-permission-governance.md) C3 + #7/#9/#17 (DR-002 rule 3, black-box) · **Owner:** TwofoldTech LLC

> Not BINDING. Committed before any `/oracle` and before the DR, per the DR-009 fence. **Scope decision (owner, 2026-07-20):** rather than ship destination-level mediation first (C3b) and defer TLS-termination/credential-brokering to a later C3c, the owner chose to build the **full L7 chokepoint in one slice** — TLS MITM + body mediation + credential brokering. This sketch reflects that folded scope. Its center of gravity is the **threat model** (§4/§8), because C3b+c makes the daemon a TLS-terminating authority holding a signing CA and live secrets. Nothing built until the DR is ACCEPTED.

## 1. Scope — the full egress chokepoint, credentials brokered, in one slice

C3a seals the network namespace (`--unshare-all`, `sandbox.rs:75-77`): a confined agent has **no external network** today. C3b+c reintroduces egress such that:

1. the **only** route out is a rezidnt-owned proxy (inescapable by construction — §2);
2. the proxy **terminates TLS** with a rezidnt-minted CA the sandbox trusts, so it sees plaintext requests (L7);
3. each request is a `permit` decision (allow/deny/escalate) and a durable, replayable fact (I3);
4. on approved egress, the proxy **injects a brokered credential** (e.g. a GitHub token) that the agent **never sees** — the memo's #7/#17 headline (credential non-exposure).

This matches Omnigent's full egress primitive (memo C3, "high") — the one enforcement gap DR-009 named — at the mediation **and** transform layer. It is the biggest single C3 slice by far; §8 is the honest accounting of what that costs.

## 2. Inescapability — mediated egress into a sealed netns (unchanged from the bounded design)

The value rests on: the confined process reaches the network **only** through the proxy. An `HTTP_PROXY` env var does not deliver that (the agent unsets it); the netns must have *no* default internet route, only a path to the proxy.

**Recommended: a userspace-net connector (`pasta`/passt, or `slirp4netns`) exec'd like `bwrap`** — rootless (no CAP_NET_ADMIN), an external binary (zero new *linked* dep, I7-clean), whose sole outbound target is the rezidnt proxy. Because the netns starts with no route, even an agent that unsets every env var and opens a raw socket reaches nothing but the proxy. The DR evaluates `pasta` vs `slirp4netns` (availability/maturity/rootless, SP4b-style) and **pins the interception model** (§9.1): *all* outbound TCP+DNS must redirect to the mediator (transparent), not only declared/HTTP_PROXY-aware ports — "only proxy-aware clients caught" is a silent hole and is rejected.

## 3. TLS termination + body mediation (the L7 layer)

- **The proxy is a substrate (`EgressProxy`, I4)** — the rezidnt-owned PEP (permit-engine §3). It accepts the netns's outbound connections, reads SNI/Host, makes the `permit` decision, then (if allowed) **terminates TLS** using a per-destination leaf cert signed by the rezidnt CA, opens its own upstream TLS to the real destination, and proxies the plaintext between them.
- **The sandbox trusts the rezidnt CA** — the CA cert is injected read-only into the confined process's trust store (a bind, folded like every other, C3a's mechanism). This is what makes MITM work without cert errors — and is exactly the trust the threat model (§4) accounts for.
- **The decision is an ordinary permit-verifier** (`EgressScope`, native — a `PathConfinement` sibling): `pass→proxy / fail→refuse / inconclusive→escalate` (I6). With plaintext in hand it can key on method/path/host, not just destination — but the decision engine is unchanged (the gate engine *is* the policy engine).
- **Allowlist + injection policy are folded authority, never a self-declared arg** — the C6/DR-024 guard C3a enforces in the type system (`SandboxPolicy.binds` private). The egress allowlist and the *which-secret-for-which-destination* map both come from the folded project-spec/role layer; a run-supplied value can never widen either.

## 4. Credential brokering — the daemon holds the secrets, the agent never does

The headline capability and the heaviest commitment.

- **A folded secret store, daemon-side, process-lifetime** — brokered secrets (tokens, keys) live in the daemon, minted/loaded at startup, **never on the fabric** (like the SP4b root key + operator badge, DR-017). The agent's environment carries **none** of them.
- **Inject-on-approved-egress:** when a request to an allowlisted destination is approved and the folded injection policy maps that destination to a secret, the proxy adds the credential to the *upstream* request (e.g. `Authorization: token …` on the real connection to `github.com`) — on the plaintext the agent never sees, after termination. The agent's own request carried no token; it cannot exfiltrate what it never held.
- **The injection is a durable fact — BY REFERENCE, never the value (I2/I3 honesty):** `credential.injected {run, dest, secret_ref, policy_ref}` records *that* a secret was injected and *which* (a ref/label), **never the secret bytes**. The secret is control-plane-forbidden the way payloads are (I2). This is the one place logging discipline is security-critical: a leaked secret in the log defeats the whole primitive.

## 5. The facts (warden `/subject`, deferred — NOT minted here)

Candidate: `egress.requested {run, dest, dest_kind}` · `egress.allowed | .denied | .escalated {…, policy_ref, evidence_ref}` · `credential.injected {run, dest, secret_ref}` (never the value). A new `egress.*`/`credential.*` family vs. riding `permit.*` is a **warden `/subject`** question — flagged, not settled. Each needs a folding reducer (DR-006). Hot-path volume (permit-engine §10.2): log-all deny/escalate/inject; allows may compact/sample — safe default log-all, optimize only if measured (mirrors DR-021's disclosed lag).

## 6. Degradation — fails CLOSED (the asymmetry with C3a, sharpened by secrets)

C3a degrades **open** (no `bwrap` ⇒ loud fact + unsandboxed spawn — "can't run" is worse). **C3b+c degrades CLOSED:** if the connector, proxy, or CA is unavailable, the sandbox keeps C3a's sealed netns — **no network** — never unmediated egress, and **never injects a credential without the mediation path intact**. A missing mediator must never mean open egress *or* a leaked secret. The unavailability is a loud logged fact (`egress.unavailable`), but the degrade direction is deny. This is the correct inverse of C3a and doubly so once secrets are in play.

## 7. Invariant fit

| Inv. | Fit |
|---|---|
| **I1** zero pixels | proxy is headless daemon-side; escalate/ask is a client. ✓ |
| **I2** plane split | egress decisions carry destination metadata (≤32 KiB); request bodies and **secrets** are never inlined on the control plane — bodies are CAS refs if captured, secrets are ref-only in facts (§4). ✓ |
| **I3** log is truth | every decision + injection is a durable, replayable fact — **but the secret value is never logged** (ref only). ⚠️ allow-volume §5. ✓ |
| **I4** substrates behind traits | `EgressProxy` + netns connector are platform substrates; decision (PDP) stays core; absent backend degrades closed. ✓ |
| **I5** MCP-first | escalations via existing `request_permission`/escalate; no new client verb on the happy path. ✓ |
| **I6** deterministic + interrogable | `EgressScope` verdict is `pass/fail/inconclusive`, replayable; `gate_explain` names the policy + destination; unavailability degrades closed, never a silent open. ✓ |
| **I7** one static binary | connector + `pasta` exec'd like `bwrap`; the proxy is tokio + a **TLS crate** (`rustls` — already the ecosystem default, verify if vendored) + a cert-gen crate (`rcgen`) for the CA/leaf certs. **This slice likely adds linked deps** — evaluate `rustls`/`rcgen` against I7 in the DR the way SP4b evaluated the macaroon crate; MITM cannot be hand-rolled safely (unlike SP4b's caveat envelope over vetted blake3). ⚠️ **honest dep delta — the DR must own it.** |
| **I8** clean-room | `pasta`/`slirp4netns`/`rustls`/`rcgen` are OS/ecosystem tooling, not Omnigent code; memo is our own black-box read; nothing ported. ✓ |

## 8. Honest risks & tensions (heavier than any prior C3 slice — read this before ratifying)

1. **The daemon becomes a TLS-terminating authority holding a CA private key + live secrets.** Blast radius: a compromised daemon can mint trusted certs for any host the sandbox reaches AND holds the brokered tokens. This is a strictly bigger root-of-trust than C3a (policy only) or SP4b (a MAC root key). Mitigation + honest boundary (state in the DR, mirror SP4b §threat-model): the CA is process-lifetime, sandbox-scoped (injected only into confined trust stores, never the host's), and never on the fabric; "a compromised daemon" is explicitly out of scope, same root-of-trust boundary as SP4b — but the *consequence* is larger and must be said plainly.
2. **Inescapability is still the whole thing (§2).** A bypass (non-proxy-aware client, raw socket the userspace stack forwards directly, DNS/IPv6/UDP leak) makes it theater. Most-tested surface: direct-egress attempts reach nothing but the proxy; DNS resolves *through* the mediator or is denied.
3. **Cert-pinning clients break — an honest functional limit.** An agent tool that pins certs (won't trust the injected CA) fails to connect through the MITM. This is inherent to TLS interception (Omnigent has it too). State it: such traffic is denied-visibly, not silently mangled; product copy notes pinned endpoints don't traverse the broker.
4. **Scope gravity, now largest (the DR-009 dissent, loudest here).** Folding C3b+c is the biggest single enforcement slice — TLS, CA lifecycle, secret store, injection policy, body proxying. It is the most roadmap-hours away from the evidence wedge. Counter (same as C3a, weightier): the justification is *replayable egress+injection decisions as evidence* (one log, both axes) and *credential non-exposure* — a differentiator Omnigent has but nobody pairs with a replayable audit log. The owner chose this scope knowingly; the DR records the trade.
5. **Secret-in-log is a catastrophic failure mode (§4).** The single most important test surface after inescapability: assert the secret value **never** appears in any fact, evidence blob, CAS ref, or trace — only the ref. A property test over the whole emitted log.
6. **Cross-platform + `pasta` availability (C3a pattern).** Linux-first; macOS/Windows egress backends later behind the same trait; Windows gated behind the deferred Platform phase. Missing connector/CA ⇒ degrade closed (§6).

## 9. Decisions the C3b+c implementation DR must ratify

1. **Netns connector — recommended `pasta`, exec'd, sole outbound = proxy; PIN transparent interception** (all outbound TCP+DNS redirected, not just proxy-aware ports). `pasta` vs `slirp4netns` evaluated.
2. **`EgressProxy` (I4) + `EgressScope` verifier**, allowlist + injection-policy from folded authority (mirror C3a's private-field no-widening).
3. **TLS MITM: RATIFY the CA model** — process-lifetime rezidnt CA, sandbox-scoped trust injection (bind), private key daemon-only, never on the fabric. Leaf certs per-destination via `rcgen`; termination via `rustls`.
4. **Credential brokering: RATIFY inject-on-approved-egress**, secrets daemon-side/process-lifetime/never-on-fabric, injected into the *upstream* request only, `credential.injected` logged **by ref never value**.
5. **Degrade CLOSED (§6)** — connector/proxy/CA unavailable ⇒ sealed netns + loud fact, no network, no injection.
6. **I7 dep delta — OWN IT:** `rustls` + `rcgen` (+ connector exec'd) are the honest cost of not hand-rolling TLS/PKI; evaluate against the one-static-binary posture explicitly (MITM is not safely hand-rollable — the opposite of SP4b's envelope-over-blake3 finding). The DR states the added linked deps and why they clear the bar.
7. **Threat model (§8) — the DR's center:** what C3b+c defends (a confined agent cannot reach a non-allowlisted host, cannot bypass the proxy, cannot read or exfiltrate a brokered secret it never holds) and what it does NOT (a compromised daemon holds the CA + secrets — out of scope, bigger blast radius than SP4b, stated plainly; cert-pinning endpoints don't traverse the broker).

## 10. What this sketch does NOT decide

- **The `egress.*`/`credential.*` taxonomy** — a warden `/subject` session (§5).
- **macOS/Windows egress backends** — later behind the same trait; Windows gated behind the deferred Platform phase. No reorder implied.
- **The decision fast-path cache** — a measured optimization (permit-engine §10.2), not an acceptance criterion; safe default log-all/no-cache until latency is measured.
- **The Omnigent-baseline egress benchmark** (memo #7/#9/#17, black-box, DR-002 rule 6) — consumes C3, does not gate it.
- **Secret provenance** — where brokered secrets come from (env, a secrets file, an external broker) beyond "daemon-side, folded, never on the fabric" — the DR names the v1 source; richer provenance is later.
