[← Decision records index](../rezidnt-architecture.md#20-decision-records) · [Architecture plan](../rezidnt-architecture.md) · [DR-026](DR-026-c3bc-egress-credential-brokering.md)

# Decision Record DR-027 — C3b+c split: decide-then-enforce — landing the egress governance + type-safety + CA/TLS scaffolding layer as explicitly ENFORCEMENT-INERT, and sequencing DR-026's ratified full-MITM dataplane into its own next slice

**Date:** 2026-07-21 · **Status:** ACCEPTED (owner) — records the owner's 2026-07-21 re-scope after the implementation pass; this amends a just-ratified DR (DR-026, ACCEPTED 2026-07-21) to **sequence, not revoke** — DR-026's full-MITM scope, threat model, and I7 dep delta all stand. · **Amends:** [DR-026](DR-026-c3bc-egress-credential-brokering.md) (splits its folded c3bc into two sequenced slices — decide + enforce; DR-026 §Acceptance-criteria are **partitioned, none weakened**) and §16 (the C3 phase's folded c3bc becomes two sequenced slices). **No invariant text is rewritten. No new dependency** — this is a slicing DR. · **Builds on:** [DR-026](DR-026-c3bc-egress-credential-brokering.md) (the ratified full-MITM scope this sequences), [DR-025](DR-025-c3a-linux-sandbox.md) (C3a containment), [DR-009](DR-009-match-omnigent-scope.md) (the C3 fence — a design sketch + implementation DR per primitive), the sketch [`docs/design/permit-egress-proxy-c3b.md`](../design/permit-egress-proxy-c3b.md). · **Cites:** [DR-022](DR-022-benchmark-harness-slice.md) (the no-half-measuring-stick honesty discipline this applies).

## Context

DR-026 ratified the folded C3b+c slice: full L7 MITM (TLS termination + body mediation) plus credential brokering. The oracle wrote the suite — host-runnable decision/type tests green, and the real netns+TLS integration as `#[cfg(unix)]` `#[ignore]`'d `unimplemented!()` bodies in `crates/rezidnt-run/tests/egress_mediation_c3bc.rs`.

The implementer built the **decision + policy + crypto-config + type-safety layer** to green (18/18 host tests): the `EgressScope` verifier + registration, `EgressPolicy` private-field no-widening (allowlist + injection-map, folded-only), the redacted `BrokeredSecret` newtype (crit 5 secret-never-in-log as a *type property* — value private, reachable only via `expose`), degrade-CLOSED as a decision + fact, the dep-scan (rustls+rcgen only), and the `RezidntCa` (rcgen CA + leaf) + rustls `terminating_config` **scaffolding** proving the crypto lifecycle compiles and runs.

**The enforcement dataplane is NOT built.** `crates/rezidnt-run/src/egress.rs::inject_and_proxy` makes the mediation decision and builds the CA/leaf, then returns only the `secret_ref` — its own comment records that the one `.expose()` upstream-write call-site "lives in the live upstream-write path (the WSL integration surface), never here." There is no netns→proxy→upstream byte-path, no live TLS termination of real traffic, and no real credential injection. Consequently DR-026 criteria **3 (inescapability)** and **4 (agent-never-sees-token, real capture)** are unproven and DR-026's exit demo is not achievable. The maker flagged the gap; the owner chose to re-scope: land the decision layer honestly, split the dataplane into its own slice.

## Strongest counterargument (dissent, recorded verbatim per house style)

*"Splitting is a face-saving relabel of an incomplete slice. You ratified full MITM, built the easy half, and are now calling the governance layer 'done' while the actual chokepoint — the thing C3 exists for — is vaporware. A decision layer that decides nothing is enforced is worse than not shipping: it invites the exact overclaim DR-026's own threat model warned about. Do not carve a green checkmark out of a slice that hasn't done its one job."*

## Counter to the counter

The decision/type layer is **real, correct, and independently valuable**: deny-by-default `EgressScope`, private-field no-widening on both the allowlist and injection map (the C6/DR-024 property, provable without a live socket), and secret-redaction as a type invariant are each verifiable now. It is landed **explicitly enforcement-inert** — not wired into a live run loop, and neither the code nor any product copy claims egress is mediated or credentials are brokered. That is DR-022's no-half-measuring-stick discipline: the layer announces itself as inert rather than looking authoritative. Splitting is honest sequencing of a genuinely large subsystem (netns proxy, live TLS byte-path, real injection) whose integration cannot be verified in one pass or non-interactively — **not** a relabel: the enforce slice carries DR-026's exit demo and criteria 3/4 unchanged. **The owner chose this knowingly.**

## Decision

- **Split DR-026's folded c3bc into two sequenced slices — decide, then enforce.** DR-026's ratified full-MITM scope is the two-slice target; it is not weakened or revoked.
- **c3bc-decide (LANDING NOW):** the egress decision/governance + type-safety + CA/TLS scaffolding layer, **explicitly ENFORCEMENT-INERT** — it decides and can build certs, but does NOT mediate live traffic or inject real credentials, and is NOT wired into a live run loop. Its done-bar = the host-provable subset of DR-026 (criteria below), passing /vet + /debrief.
- **c3bc-enforce (NEXT slice):** the enforcement dataplane — the `pasta` netns proxy forcing all outbound through rezidnt (inescapability, DR-026 crit 3), the live TLS-termination byte-path, real upstream credential injection (crit 4), and the WSL integration suite (`egress_mediation_c3bc.rs`, currently `#[ignore]`'d). This is where DR-026's exit demo becomes achievable. It gets its own oracle → impl → /vet → /debrief.
- **DR-026 stands.** The full-MITM posture commitment, the threat model (secret-in-log, CA-blast-radius), and the I7 rustls+rcgen linked-dep delta the owner ratified all carry unchanged.
- **Honesty guard (load-bearing):** until c3bc-enforce ships, the `EgressProxy`/`PastaProxy` substrate is decision-and-scaffold only and MUST NOT be wired into a live run loop as if it enforced egress. Product copy and any wiring must not claim egress mediation or credential brokering works until the dataplane lands.

## Consequences

- **§16 delta:** the C3 phase's folded c3bc becomes two sequenced slices — **c3bc-decide** (host-provable governance/type/scaffold layer, landing now) then **c3bc-enforce** (the dataplane, carrying DR-026's exit demo). `/oracle` for the enforce slice rides DR-026's ratified design; nothing is re-designed here.
- **Risk-register (§18) delta — overclaim / inert-layer risk (NEW):** a landed decision layer could be mistaken for working enforcement — the exact overclaim DR-026's threat model warned against. Mitigated by (a) the enforcement-inert labeling in code and docs, (b) the substrate not being wired into a live run loop, and (c) the enforce slice owning criteria 3/4 and the full-MITM exit demo, so "egress works" is not claimable until it ships.
- **No DR-026 criterion is weakened — they are PARTITIONED.** decide takes the host-provable subset (crit 1-decision, 2, 5, 6, 7, 8); enforce takes crit 3, crit 4, and the real live-traffic arms of 1/7. Splitting a slice's criteria across two sequenced slices is not softening a gate — the same bars must all still be cleared, in order.

## Acceptance-criteria — c3bc-decide (what /debrief checks now)

1. **Destination decision** — an allowlisted host decides *reach*, a non-allowlisted host decides *deny*, an undecidable case *escalates* (DR-026 crit 1-decision + crit 2), each a durable logged fact; verdict replays from the log.
2. **Secret-never-in-log as a type property** (DR-026 crit 5) — `BrokeredSecret`'s value is private, redacted in Debug/Display, reachable only via the one sanctioned `expose`; a property scan over the whole emitted fabric asserts only `secret_ref` appears.
3. **No-widening** (DR-026 crit 6) — no run-supplied input can widen the allowlist or the injection map; both are private-field folded authority (the C6/DR-024 guard).
4. **Degrade-CLOSED decision + fact** (DR-026 crit 7, decision arm) — connector/CA absent decides *no network* + a loud `egress.unavailable` fact + no injection; never a silent open.
5. **Dep-scan** (DR-026 crit 8) — rustls + rcgen are the only new linked deps; no other new linked crate.
6. **Enforcement-inert, no overclaim (NEW, decide-specific):** the substrate is NOT wired into a live run loop, and the code + docs label it enforcement-inert — nothing claims live egress mediation or real credential brokering works yet.

## Exit demo — c3bc-decide

One take, host-only, no netns/TLS live path. The host suite green: a decision **denies an off-allowlist host and logs it**; a secret's **value appears nowhere in the emitted fabric** (only the ref); **no run-supplied input widens** the allowlist or injection map; an **absent connector/CA degrades closed** with a loud `egress.unavailable` fact; **rustls + rcgen are the only new linked deps**. The FULL-MITM exit demo — a confined agent clones over the broker with an injected token it never held, an off-allowlist host denied-and-replayed over the live path, absent-CA no-network — **explicitly belongs to c3bc-enforce**.

## What this does NOT decide

- **The enforce dataplane's design** — the netns proxy, live TLS byte-path, and real injection ride DR-026's already-ratified design; not re-opened here.
- **The `egress.*`/`credential.*` warden subject** — still a `/subject` session (DR-026 §What-this-does-NOT-decide), unchanged.
- **macOS / Windows egress backends** — later behind the same trait; no reorder implied.

*Amendments to this record require DR-028.*
