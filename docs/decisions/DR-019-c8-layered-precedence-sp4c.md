[← Decision records index](../rezidnt-architecture.md#20-decision-records) · [Architecture plan](../rezidnt-architecture.md)

# Decision Record DR-019 — SP4c: C8 layered policy precedence via monotone concat

**Date:** 2026-07-20 · **Status:** ACCEPTED (owner) · **Amends:** §16 (roadmap — pins SP4c acceptance as the **final** SP4 sub-slice, closing the DR-016 three-slice split and realizing the DR-009 C8 line) / §8–§9 (permit-config seam — `permit_config_for` merges **three sourced layers** admin/dev/session instead of reading one `gates["permit"]` block; additive, no wire change); no invariant text is rewritten. **Cites:** [DR-009](DR-009-match-omnigent-scope.md) C8 (which cites intel memo [`intel/001-omnigent-permission-governance.md`](../../intel/001-omnigent-permission-governance.md) — this record inherits C8's memo motivation transitively; DR-002 rule 3). **Builds on:** [DR-016](DR-016-permit-roles-sp4-slicing.md) §Decision 4 (pins the SP4c direction — "admin/dev/session layered precedence, stricter-wins, merged in the `permit_config_for` resolution seam"), [DR-011](DR-011-permit-pdp-config-seam.md) (the `permit_config_for` seam this extends), [DR-008](DR-008-permit-engine-pivot.md)/[DR-009](DR-009-match-omnigent-scope.md) (permit engine + C8 scope); design sketch [`docs/design/permit-roles-delegation-sp4.md`](../design/permit-roles-delegation-sp4.md) §5 (which explicitly **defers** the composition question — "concatenate-ordered vs a lattice … a design detail for the C8 slice" — settled here); aggregate contract [`docs/design/permit-pep-sp2.md`](../design/permit-pep-sp2.md) step 5 + [DR-015](DR-015-permit-exec-verifier.md).

## Context

SP4a (roles, DR-016) and SP4b (macaroon delegation, DR-017/DR-018) are DONE (shipped at commit e8301af). SP4c is the last SP4 sub-slice: it realizes **C8 — layered policy precedence** (DR-009). Three policy layers — **admin / dev / session** — must compose with **stricter-layer-wins**: an admin-layer `[gates.permit]` deny cannot be overridden by a dev- or session-layer allow. This is policy-resolution logic, not crypto; it extends the DR-011 config-resolution seam `permit_config_for` (`bins/rezidentd/src/mcp.rs:131-179`), which today reads **one** `gates["permit"]` block from the opened-workspace registry, folds its specs, and returns a flat ordered `PermitConfig { verifiers: Vec<PermitVerifierSpec> }` (`crates/rezidnt-mcp/src/lib.rs:154-190`). The sketch §5 explicitly deferred the *composition* question to this slice.

Two compositions were considered:

- **(a) Concatenate-ordered (RECOMMENDED).** Merge the three layers into the existing flat `Vec<PermitVerifierSpec>` in **admin → dev → session** order. Load-bearing insight: the aggregate (`rezidnt_gate::permit::aggregate_async`) has **no allow-override primitive** — a Grant is merely *the absence of a veto* (first-`Fail`→Deny short-circuit, any-`Inconclusive`→Escalate, else Grant; DR-015, permit-pep-sp2 step 5). Adding a layer's verifiers can therefore only make the verdict **stricter** (more chances to Fail→Deny or Inconclusive→Escalate), never more permissive. Stricter-wins falls out of the **existing monotone aggregate for free**; an admin deny is non-overridable purely because a later layer cannot un-Fail an earlier Fail. Minimal change: `permit_config_for` merges three sourced blocks instead of one; the aggregate and the verdict→decision table are untouched. The remaining real work is (i) **where the layers are sourced** (admin from daemon/host config, dev from the workspace applied spec, session from the run/agent) and (ii) **layer provenance** carried on each `PermitVerifierSpec` so `gate_explain` / the emitted decision fact names the deciding **layer**, not just the deciding verifier (I6).
- **(b) Explicit lattice / precedence structure.** A richer type where layers are distinct and a resolver enforces precedence explicitly. More machinery, justified only if a future layer needs allow-override or non-monotone composition — which the permit model deliberately does **not** have. Rejected as premature (scope gravity, §18).

**Strongest counterargument (dissent, recorded verbatim per house style):** *"Three config-source layers with provenance is added surface area for a feature no user has yet asked to differentiate by — session-vs-dev is a distinction nobody has requested. A single applied `[gates.permit]` block already composes via ordering; the monotone aggregate already makes an earlier Fail un-overridable. So C8-as-three-layers risks being ceremony over the flat concat we already ship — we could get 'stricter-wins' by just telling operators to author admin rules first."* **Counter to the counter:** DR-009 already owner-accepted C8 as table-stakes enforcement breadth versus Omnigent (memo 001), not a speculative add. And "admin rules first by config-authorship convention" is exactly the fragility C8 removes: with a single flat block, an admin deny is non-overridable only *by luck of who wrote the file last* and *in what order* — a dev editing the workspace spec can reorder or drop it silently, with no audit trail of which authority the rule belonged to. Distinct sourced layers with provenance make an admin deny **auditably** non-overridable (I6 interrogability): the decision fact names the deciding layer, so "why blocked" answers *"admin layer"*, not merely *"verifier #1"*. That is the difference between a policy guarantee and a coincidence. **The owner has accepted this trade knowingly.**

## Decision

1. **Ratify composition (a): concatenate-ordered, admin → dev → session.** `permit_config_for` sources three `[gates.permit]` blocks (admin, dev, session) and merges their specs into the existing flat `Vec<PermitVerifierSpec>` in that fixed order. `PermitConfig`, `aggregate_async`, and the verdict→decision table are **unchanged**. Stricter-wins is inherited from the existing monotone aggregate (no allow-override primitive exists to override a Fail). **Reject (b)** as premature machinery — no non-monotone or allow-override requirement exists to justify a lattice; it is fenced, not lost, if such a layer ever appears.

2. **Layer sourcing.** admin from daemon/host config; dev from the workspace applied spec (`workspace.spec.applied`, the current single source, I3); session from the run/agent. Each layer resolves from folded state / applied spec — no new authority reaches the core; the daemon-side fold discipline of DR-011 §1 is preserved (the seam relocates the merge to the daemon, not the transport-agnostic core).

3. **Layer provenance on each spec.** Each `PermitVerifierSpec` carries the layer it was sourced from so `gate_explain` and the emitted decision fact name the **deciding layer** (I6). Whether provenance is a new field on `PermitVerifierSpec` (touching `rezidnt_gate::permit`) or a parallel tag is an implementation detail for `/oracle`; the *contract* — the decision fact names the layer — is ratified here.

4. **Honest degradation is preserved.** An absent or empty layer contributes zero verifiers; an all-empty resolution remains **honest-undecidable → escalate**, never a synthesized allow (DR-011 §3, `PermitConfig` doc `lib.rs:161-163`). No layer's absence manufactures a permission.

## Invariant fit

| Inv. | Fit |
|---|---|
| **I3** log is truth | Layers resolve from folded state / the applied spec (dev = `workspace.spec.applied`); the decision fact carries the deciding layer, so the composed verdict replays from log + config. ✓ |
| **I6** determinism / interrogable | Concat is a deterministic ordered merge; `gate_explain` names the deciding **layer** (not just the verifier) — an admin deny is *auditably* non-overridable. Empty/absent layer → escalate, never coerced to pass. ✓ |
| **I2** plane split | Layer specs are small ordered config entries — inline params, not CAS. Unchanged. ✓ |
| **I1 / I4 / I5 / I7** | Unchanged — decision stays core/headless; config resolution stays a substrate capability (I4) behind `permit_config_for`; no new dependency, no crypto (I7). ✓ |

## Consequences

- **§16 roadmap delta:** SP4c is pinned as the **final** SP4 sub-slice — acceptance = C8 layered precedence live (admin deny non-overridable by a session allow, deciding layer surfaced). Accepting this closes the DR-016 three-slice split and fully realizes the DR-009 C8 line and permit-engine §7.
- **§8–§9 seam delta:** `permit_config_for` merges three sourced `[gates.permit]` layers instead of reading one; `PermitVerifierSpec` gains layer provenance. Additive; no wire change; aggregate and verdict→decision table untouched.
- **Risk-register (§18) delta:** *Scope-gravity / strategic dilution (permit-engine §10.3, carried from DR-016).* Mitigation holds: (a) is the *minimal* composition — it reuses the existing monotone aggregate and touches only the resolution seam, deliberately declining the lattice (b). *New: layer-provenance surface* — mitigated by keeping provenance a tag read only by `gate_explain`/the decision fact, changing no verdict logic.
- **No test or acceptance criterion is weakened.** In plain words: adding layers can only make a permit **stricter** (more veto opportunities), never relax an existing gate; empty/absent layers still degrade to escalate, not allow. This is a tightening (admin denies become *auditably* non-overridable) and a new capability (layer-keyed decisions), not a softening. New criteria arrive via `/oracle` (below).

**Acceptance-criteria sketch (what `/oracle` encodes for SP4c):**
1. A layered fixture where an **admin-layer deny is NOT overridable by a session-layer allow** (the composed verdict is Deny).
2. **Layer provenance surfaced** in the decision fact / `gate_explain` — the deciding layer is named (e.g. "admin"), not merely the verifier.
3. **Concat order verified** — admin → dev → session; a later-layer verifier cannot un-Fail an earlier-layer Fail.
4. **Empty / absent layer degrades to escalate, never allow** — an all-empty resolution escalates (DR-011 §3 discipline preserved per layer).

## What this does NOT decide

- **No crypto.** SP4c is policy-resolution logic only; the macaroon badge/delegation surface (SP4b, DR-017/DR-018) is untouched.
- **No allow-override primitive** is added — deliberately. A Grant stays the absence of a veto; the lattice (b) is fenced for a hypothetical future non-monotone layer and is not built.
- **C3 sole-chokepoint enforcement stays fenced** under DR-009 (its own design sketch + implementation DR before any build); this record does not touch it.
- The exact **provenance carrier** (field vs tag) and the daemon-side wiring of the three sources are left to `/oracle` + the implementer; only the contract (layer named on the decision) is ratified.

*Amendments to this record require DR-020.*
