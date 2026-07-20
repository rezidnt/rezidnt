# Design sketch — SP4b macaroon-attenuated delegation (badges become attenuable)

**Status:** PROPOSED (design-first per [DR-002](../decisions/DR-002-prior-art-protocol.md) rule 1) · **Feeds:** SP4b's own DR (owner sign-off — the crypto + dep decision DR-016 §Dec 3 deferred here) · **Builds on:** [DR-016](../decisions/DR-016-permit-roles-sp4-slicing.md) §Decision 3 (SP4b direction), [DR-005](../decisions/DR-005-badge-consolidation.md) (badge bundle — promotes its PROVISIONAL macaroon item), [permit-engine](permit-engine.md) §7 (RBAC & delegation), SP4a (roles) · **Owner:** TwofoldTech LLC

> Not BINDING. Committed before any `/oracle`. The crate evaluation (§3) is the gating step DR-016 §Dec 3 required *before* the crypto choice — it is done and drives the recommendation. Nothing built until the DR is ACCEPTED.

## 1. Scope — a badge you can narrow

SP4b promotes DR-005's PROVISIONAL macaroon item and closes plan §19 ("macaroon-attenuated badges — needs a real delegation use case"): **sub-agent spawning is that use case.** A lead agent, holding its badge, mints a **narrowed** badge for a sub-agent — offline, no daemon round-trip, no central mint. The daemon verifies the narrowed badge and honors only the reduced capability. Today's badge (opaque 256-bit random, id-equality check, `badge.rs`) cannot express this: it has no structure to narrow.

## 2. What a macaroon buys (vs the opaque token)

A macaroon is a token whose capability is a **root identifier + a chain of caveats** (attenuating predicates), bound by a keyed-MAC chain so that: (a) the holder can **append a caveat** to narrow it without the root key; (b) anyone with the root key can **verify** the chain and evaluate the caveats; (c) tampering or widening breaks the MAC. That is exactly §7's delegation primitive — offline, monotonic-narrowing, no central lookup.

rezidnt needs only **first-party caveats** (predicates the daemon evaluates: workspace, verb, expiry, role) — **no third-party discharge macaroons** (those coordinate independent services; rezidnt's daemon is the sole authority). This shrinks the construction dramatically.

## 3. Crate evaluation (DR-016 §Dec 3 gating step — DONE)

DR-016 ratified "evaluate a permissive macaroon crate first; hand-roll only as fallback if none clears the license/dep/audit bar." Evaluated 2026-07-19:

| Candidate | License | Maintenance | Fit | Verdict |
|---|---|---|---|---|
| `macaroon` (macaroon-rs) | MIT ✓ | **Stale — last release Oct 2022, pre-1.0 (0.3.0), ~46K downloads** | true macaroon (HMAC, first-party caveats) | **Fails the audit/maintenance bar** — a stale, low-adoption, pre-1.0 crate is unacceptable for security-critical delegation crypto. |
| `rusty-macaroon` (deislabs) | — | **Alpha, "NOT READY FOR PRODUCTION"** | v2 spec | Rejected (alpha). |
| `libmacaroon-rs` | — | **Deprecated** | — | Rejected (deprecated). |
| `biscuit-auth` | Apache-2.0 ✓ | **Healthy — v6.0.0 Jul 2025, 10.3M downloads, Eclipse-hosted** | **Mismatch:** public-key crypto + a Datalog authz language | **Over-fit** — see below. |

**Why `biscuit-auth` is the wrong fit despite being well-maintained:**
1. **Public-key crypto rezidnt doesn't need.** Biscuit's headline is *decentralized* verification (any holder of the root *public* key verifies). rezidnt's daemon is the **sole minter and verifier** — a shared-secret keyed-MAC is the leaner, correct model; asymmetric crypto is dead weight here.
2. **Datalog competes with SP3.** Biscuit carries a Datalog authorization language. rezidnt **already has** a policy-DSL story — the SP3 exec-verifier (`§8` argv, OPA/Cedar/any). Pulling Biscuit's Datalog in for delegation muddies "where does policy live" and duplicates the permit-verifier model.
3. **I7 dependency surface.** Biscuit pulls Ed25519 + a Datalog engine + protobuf — a large surface for one static binary, versus the delegation primitive we actually need.

**The clinching finding — hand-roll is ZERO new dependencies.** rezidnt **already vendors `blake3`**, and `blake3::keyed_hash(key: &[u8;32], data)` is a **secure keyed MAC** (BLAKE3's specified keyed mode — a PRF, not a hand-rolled primitive). A first-party-caveat macaroon over blake3-keyed is an ~80-line envelope:

```
sig₀      = blake3::keyed_hash(root_key, identifier)
sigᵢ₊₁    = blake3::keyed_hash(sigᵢ.as_bytes(), caveatᵢ)      // each caveat re-keys the running sig
macaroon  = { identifier, caveats: [...], sig: sig_last }
verify(m) = recompute the chain from root_key; constant-time compare m.sig; then evaluate every caveat
```

**Recommendation: hand-roll over blake3-keyed** — the DR-016 fallback, and it is the *better* option, not a compromise: the crypto **primitive** stays a vetted crate (blake3), only the thin caveat-chain envelope is ours (property-testable), and it adds **no dependency** (I7-pure), decisively beating both the stale macaroon crate and the over-heavy biscuit. `rand` (already vendored) mints the root key + identifier.

## 4. The construction (first-party caveats only)

- **Root key.** Daemon process-lifetime 256-bit key (like the operator-badge secret), `rand`-minted at startup, never on the fabric.
- **A caveat is a small structured predicate**, serialized deterministically: `workspace = <ulid>` · `verb ∈ {…}` · `expiry < <ts>` · `role = <s>`. (The DR-005 `{workspace, verb set, expiry}` badge shape, now first-class + attenuable, plus the SP4a role.)
- **Mint** (daemon, at spawn): identifier binds the run; base caveats = the run's scope. Serialize + inject under `REZIDNT_BADGE` (replacing the opaque token; same env seam).
- **Attenuate** (a lead agent, offline): append a narrowing caveat, re-key the running sig with blake3-keyed. No root key needed — that's the delegation property.
- **Verify** (`check_badge` extension): recompute the sig chain from the root key, **constant-time** compare, then evaluate every caveat against the request context (this workspace, this verb, now, this role). Any unsatisfied caveat → refuse. Offline: root key + presented macaroon, no lookup.

## 5. Monotonicity — the load-bearing security invariant (I6)

The whole security property: **a caveat can only narrow; attenuation never widens.** A widening bug is privilege escalation. This is the most-tested thing in the slice:
- **Property test:** for any macaroon `M` and caveat `c`, `capability(verify(M+c)) ⊆ capability(verify(M))` (attenuation is monotone-decreasing).
- **Forgery/tamper tests:** a macaroon with a caveat removed, reordered, or edited — or a forged sig — **fails verify** (the keyed-MAC chain breaks).
- **Constant-time sig comparison** (no timing oracle on the MAC).
- **Expiry is a caveat**, evaluated at verify time against a deterministic clock input (I6 — no ambient `now()` inside the verifier; the request context carries the timestamp, replayable).

## 6. Badge migration (DR-005 boundary respected)

- **Agent badges become macaroons.** The `Badge` type gains a macaroon representation; `REZIDNT_BADGE` carries the serialized macaroon; `badge_id` stays `blake3(identifier)[..8]` (loggable, unchanged shape on `agent.spawned`).
- **The operator badge stays the DR-005 opaque daemon-lifetime class** — possession of the 0600 lockfile = capability; it is not per-run and needs no attenuation. SP4b does not touch it.
- **`check_badge` verification** flips from id-equality to macaroon-verify + caveat-eval — the one behavioral change on the mutating-call door (§12).

## 7. The delegation fact (I3 — the capability chain is auditable)

A delegation must be a **durable fact** so the capability chain replays (I3): when a lead agent's badge is attenuated for a sub-spawn, the daemon records it. **Open (warden `/subject`):** a new `permit.delegated {parent_badge_id, child_badge_id, added_caveats, run}` subject + reducer, vs a field on `agent.spawned`. Recommend a **subject** here (unlike roles/pep): delegation IS a new event in time (a capability was narrowed and handed off), on the `permit.*` axis — it earns a row (contrast the SP4a role, which is a spawn-time property). Decided in the `/subject` session.

## 8. Invariant fit

| Inv. | Fit |
|---|---|
| **I7** one static binary | **Zero new dependency** — blake3 (vendored) keyed-hash + rand (vendored). The primitive is audited; only the envelope is ours. ✓ |
| **I3** log is truth | delegation is a durable fact (`permit.delegated`); the capability chain replays. ✓ |
| **I6** deterministic/interrogable | verify is pure; expiry evaluated against a passed-in timestamp (no ambient clock); `gate_explain` surfaces which caveat refused. ✓ |
| **I2** plane split | a macaroon (identifier + a few caveats) is small — inline under `REZIDNT_BADGE`, never CAS. ✓ |
| **I1/I4/I5** | unchanged — verification is behind `check_badge` (substrate); decision stays core/headless. ✓ |

## 9. Honest risks

- **Crypto we own (the envelope).** Even over a vetted primitive, the caveat-chain logic is security-critical. Mitigation: keep it minimal + first-party-only; property-test monotonicity and forgery exhaustively (§5); a clear threat model in the DR.
- **Widening = escalation** (§5) — the single most important test surface.
- **Root-key lifetime.** A process-lifetime key means badges don't survive a daemon restart (a restart re-mints). State this: agent badges are run-scoped and short-lived anyway (DR-005 expiry); a restart invalidating in-flight badges is acceptable and honest (matches the operator-badge daemon-lifetime model).
- **Scope (permit-engine §10.3, restated).** SP4b is the crypto slice; it's bounded because delegation rides the existing badge seam (`check_badge`, `SpawnPlan` injection) and adds no dependency.

## 10. Open decisions the DR must ratify (owner sign-off)

1. **Macaroon impl (§3) — RECOMMENDED: hand-roll a first-party-caveat macaroon over blake3-keyed (zero new dep)**, having evaluated and rejected the stale `macaroon` crate (audit/maintenance) and the over-fit `biscuit-auth` (public-key + Datalog + I7 surface). The DR ratifies this + records the evaluation. *(This is the load-bearing decision — it touches the approved-dep set + I7.)*
2. **Badge migration (§6)** — agent badges become macaroons; operator badge stays the DR-005 opaque class. Confirm.
3. **Delegation fact (§7)** — a `permit.delegated` subject (recommended) vs an `agent.spawned` field — a warden `/subject`, gated.
4. **Root-key lifetime (§9)** — process-lifetime (recommended; badges are run-scoped/short-lived) vs persisted.
5. **Threat model** — the DR should state what SP4b defends against (a compromised sub-agent cannot widen its badge; a tampered macaroon fails verify) and what it does NOT (a compromised *daemon* holds the root key — out of scope, same as any root-of-trust).
