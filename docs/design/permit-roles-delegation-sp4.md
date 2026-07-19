# Design sketch — SP4 roles + macaroon-attenuated delegation (+ C8 layered precedence)

**Status:** PROPOSED (design-first per [DR-002](../decisions/DR-002-prior-art-protocol.md) rule 1) · **Feeds:** a DR (owner sign-off); promotes [DR-005](../decisions/DR-005-badge-consolidation.md) PROVISIONAL macaroon item · **Builds on:** [permit-engine](permit-engine.md) §7, [DR-005](../decisions/DR-005-badge-consolidation.md) (badge bundle), [DR-008](../decisions/DR-008-permit-engine-pivot.md)/[DR-009](../decisions/DR-009-match-omnigent-scope.md) (C8 scope), SP1–SP3 (native + exec permit verifiers, `decide_permit`) · **Owner:** TwofoldTech LLC

> Not BINDING. Committed before any `/oracle`. Nothing built until the DR is ACCEPTED. SP4 is **larger than one slice** — this sketch proposes a slicing (§8) so each piece is oracle-first and independently shippable.

## 1. Scope — three coupled capabilities

permit-engine §7 names two; DR-009 adds a third:

1. **Roles (RBAC seam).** A `role` on `AgentSpec` (§6 shows `role = "contributor"` as NEW); permit policies key on **role + workspace + action**. The native/exec permit verifiers gain role as an input axis.
2. **Macaroon-attenuated delegation.** A lead agent delegates a **narrowed** capability to a sub-agent by attenuating its badge — cryptographically, offline-verifiable, no central lookup. This promotes DR-005's **PROVISIONAL** macaroon item and closes the plan §19 open-decision ("macaroon-attenuated badges — needs a real delegation use case"): sub-agent spawning IS that use case.
3. **C8 — layered precedence.** admin / dev / session policy layers with **stricter-layer-wins** precedence (DR-009 C8), extending the `[gates.permit]` + roles model.

## 2. The badge today — why a macaroon is the change

`crates/rezidnt-run/src/badge.rs`: a badge is a **256-bit random opaque token**; `id = blake3(token)[..8]`; the token is the secret, injected at spawn under `REZIDNT_BADGE`, never on the fabric. Verification is **id-equality** against what the daemon minted (`check_badge`). It carries **no structure** — no scope, no caveats, no way for a holder to *narrow* it. Delegation with an opaque token means either sharing the parent's full token (no attenuation) or a central mint+lookup for every sub-capability (a central dependency DR-005 wants to avoid).

A **macaroon** replaces the opaque token with a structured, HMAC-chained token: a root identifier + a chain of **caveats** (each an attenuating predicate), signed so that anyone holding the root key can verify, and anyone holding the macaroon can **append a caveat** to narrow it (without the root key). That is exactly the delegation primitive §7 wants — offline-verifiable, no central lookup, monotonic narrowing.

## 3. Macaroon design (the load-bearing crypto)

- **Root key.** The daemon holds a process-lifetime root key (like the operator-badge secret). Agent badges are macaroons minted under it.
- **Caveats = the badge's `{workspace, verb set, expiry}` (DR-005) expressed as first-class predicates**, plus delegation narrowing:
  - `workspace = <ulid>` · `verb ∈ {…}` · `expiry < <ts>` (the existing badge shape, now structured).
  - `role = <role>` (SP4 roles as a caveat the permit verifiers read).
  - delegation: a sub-agent's macaroon = parent's macaroon + additional caveats that can only **narrow** (a caveat can shrink the workspace/verb/role set, never widen it — monotonicity is the security property).
- **Verification (`check_badge` extension).** The daemon verifies the HMAC chain against the root key and **evaluates every caveat** against the request context (this workspace, this verb, now, this role). Any unsatisfied caveat → refuse. Offline: no lookup, just the root key + the presented macaroon.
- **Delegation flow.** A lead agent (holding its macaroon) mints a sub-agent's by appending caveats before the sub-spawn; `SpawnPlan` injects the attenuated macaroon under `REZIDNT_BADGE`. The delegation is a **fact on the log** (I3 — a `permit.delegated` or a field on `agent.spawned`; a warden `/subject` question) so the capability chain is auditable/replayable.

**Library vs hand-roll (open decision, §9).** Macaroons are a small, well-specified HMAC construction. Options: a permissive-licensed Rust macaroon crate (must clear the approved-dependency set, rust-conventions; clean-room OK — permissive read is fine) vs a hand-rolled HMAC-chain (no new dep, fully in-binary I7, but crypto-we-maintain). Recommend evaluating a vetted crate first; fall back to a minimal hand-roll if none clears the dep bar. **This is a DR decision (touches the approved-dep set + I7).**

## 4. Roles (the contained, high-value piece)

- **`role: Option<String>` on `AgentSpec`** (additive, like `bare`/`harness_version`). Recorded on `agent.spawned` (a warden `/subject` additive field, mirroring the `pep`/`bare` precedent) so the role is log-derivable (I3).
- **Permit verifiers gain role as an input axis.** `decide_permit` already injects folded per-run state as content-pinned params (DR-011 §2); the role rides the same way. Native verifiers (and exec policies via the §8 stdin) read `role` to key decisions. No new verifier machinery — a new input field.
- Roles are **independent of macaroons** and much lower-risk: no crypto, reuses the SP1–SP3 dispatch. This is why §8 slices roles first.

## 5. C8 — layered precedence (admin / dev / session)

- Three policy layers compose with **stricter-wins**: an admin-layer `[gates.permit]` deny cannot be overridden by a dev- or session-layer allow. Extends the config-resolution seam (`permit_config_for`, DR-011) to merge layers rather than read one.
- **Precedence rule:** the aggregate already does first-`Fail`→Deny; layering means the admin layer's verifiers run first (or its denies are non-overridable). The exact composition (concatenate-ordered vs a lattice) is a design detail for the C8 slice.
- C8 is **policy-resolution logic**, not crypto — it folds into the precedence-resolution slice, distinct from delegation.

## 6. Invariant fit

| Inv. | Fit |
|---|---|
| **I3** log is truth | roles on `agent.spawned`; delegation is a durable fact — the capability chain replays. ✓ |
| **I6** determinism/interrogable | caveat evaluation is deterministic; `gate_explain` surfaces which caveat/role/layer decided. ✓ |
| **I7** one static binary | a macaroon crate must vendor into the static binary + clear the approved-dep set, else hand-roll. ⚠️ §3/§9. |
| **I2** plane split | macaroons are small (caveat list) — inline, not CAS. ✓ |
| **I1/I4/I5** | unchanged — decision is core/headless; badge verification is a substrate concern behind `check_badge`. ✓ |

## 7. Honest risks

- **Crypto we own (if hand-rolled).** A hand-rolled macaroon HMAC chain is security-critical code with no upstream audit. Mitigation: prefer a vetted crate; if hand-rolled, keep it minimal, property-test the monotonicity (a caveat can never widen), and adversarially test forged/tampered chains.
- **Delegation widening bug = privilege escalation.** The whole security property is that attenuation only narrows. This must be the most-tested invariant in the slice (property test: for any macaroon M and caveat c, `verify(M+c) ⊆ verify(M)`).
- **Scope gravity (permit-engine §10.3, restated).** SP4 is three capabilities; doing all inline risks the §18 scope-gravity warning. Mitigation: the §8 slicing — ship roles first (bounded), then delegation (crypto, gated on the dep decision), then C8.

## 8. Proposed slicing (SP4 is too big for one slice)

Recommend three oracle-first sub-slices, each its own criteria (the DR can ratify the split or fold):

- **SP4a — roles.** `role` on `AgentSpec` + `agent.spawned` (`/subject`) + role as a permit input axis; a role-keyed policy decides. *Lowest risk, high value, no crypto.* **Recommended first.**
- **SP4b — macaroon delegation.** Badge → macaroon; caveat verification in `check_badge`; a lead agent attenuates a sub-agent's badge; delegation logged; monotonicity property-tested. *The crypto slice; gated on the §9 dep decision.*
- **SP4c — C8 layered precedence.** admin/dev/session layers, stricter-wins, in the config-resolution seam. *Policy logic; can follow SP4a/b or fold into SP4a's resolution.*

## 9. Decisions the DR ratifies (three owner-ratified 2026-07-19)

1. **Slicing (§8) — RATIFIED: three sub-slices, SP4a roles → SP4b delegation → SP4c C8.** Each oracle-first + independently shippable. **SP4a (roles) is the immediate slice**; SP4b/SP4c are committed to the roadmap but detailed later (SP4b's concrete crate choice lands in its own follow-on DR once evaluated — see §9.2).
2. **Macaroon: crate vs hand-roll (§3) — RATIFIED: evaluate a permissive Rust macaroon crate first**, against the approved-dep set (rust-conventions); it vendors into the static binary (I7 OK). Fall back to a minimal in-binary hand-roll only if none clears the license/dep/audit bar. **The concrete crate (or the hand-roll fallback) is ratified in SP4b's own DR** — it touches the approved-dep set + I7 and is only decidable after evaluation; SP4a does not depend on it.
3. **Badge migration (§2/§3) — direction:** agent badges *become* macaroons (recommended), operator badge stays the DR-005 opaque daemon-lifetime class. Confirmed concretely in SP4b (with the crate choice).
4. **Delegation fact (§3)** — new `permit.delegated` subject vs a field on `agent.spawned` (warden `/subject`, gated) — decided in SP4b's warden session.
5. **`/intel` — RATIFIED: skip for now.** Memo 001 already sized C7/C8 at DR-009's scope; the macaroon design is DR-005-driven, not a competitor gap. Revisit per-question if SP4b needs it (its own DR then, DR-002).
