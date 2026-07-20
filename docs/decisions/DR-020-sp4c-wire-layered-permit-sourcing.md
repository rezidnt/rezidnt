[← Decision records index](../rezidnt-architecture.md#20-decision-records) · [Architecture plan](../rezidnt-architecture.md)

# Decision Record DR-020 — SP4c-wire: three-source layered permit wiring (admin outside the workspace spec)

**Date:** 2026-07-20 · **Status:** ACCEPTED (owner) · **Amends:** §9 (MCP surface — the permit-config resolution seam): `permit_config_for` sources **three** layers (admin/dev/session) and returns `PermitConfig::from_specs(compose_layers(admin, dev, session))`; a new `McpCore::with_layered_permit_config(admin, dev, session)` test builder mirrors `with_permit_config`; the emitted decision fact pins `outcome.deciding_layer` in the CAS policy blob. **Additive** — no envelope/wire-schema break, no invariant text rewritten, aggregate + verdict→decision table untouched. **Cites:** [DR-019](DR-019-c8-layered-precedence-sp4c.md) (completes its explicitly-deferred wiring), [DR-011](DR-011-permit-pdp-config-seam.md) (the `permit_config_for` seam this extends). DR-019 inherits C8's intel motivation ([`intel/001-omnigent-permission-governance.md`](../../intel/001-omnigent-permission-governance.md)) transitively via DR-009; this record inherits it through DR-019 (DR-002 rule 3). **Builds on:** [DR-019](DR-019-c8-layered-precedence-sp4c.md) §Decision 2 (layer-sourcing DIRECTION) + §"What this does NOT decide" (fences the wiring to this /dr), [DR-011](DR-011-permit-pdp-config-seam.md), [DR-016](DR-016-permit-roles-sp4-slicing.md) §Decision 4.

## Context

**Scope correction first — the prevailing framing of this seam is stale.** The SP4b handoff, DR-019's "future seam /dr" pointer, and the `crates/rezidnt-mcp/tests/permit_wire_dispatch.rs` file header (line 27 prose) all describe a "live config-dispatch seam" as unbuilt / `#[ignore]`-gated. **At HEAD that is false, and the record must be corrected.** The generic live dispatch seam is DONE and green:

- `McpCore::with_permit_config(config)` already exists (`crates/rezidnt-mcp/src/lib.rs:392`, SP2/DR-013).
- `decide_permit` (`lib.rs:755-858`) already dispatches the **configured** `[gates.permit]` set — static config or per-run `substrate.permit_config_for(run)` — folds run-state (intent/role/spend), aggregates via `aggregate_async`, and emits ONE decision fact with the deciding verifier's pinned policy. It is NOT a hardcoded single ToolAllowlist.
- All five tests in `permit_wire_dispatch.rs` are live `#[tokio::test]` (lines 110/166/207/275/334) and pass in `/vet` today. The word `#[ignore]` appears only in the header prose, never on a test.

So the prior framing conflated the **already-built generic seam** with the **still-unbuilt layered wiring**. DR-020 is therefore NARROWER than the handoff implied: it ratifies ONLY the C8 three-source LAYERED wiring — the exact piece DR-019 §"What this does NOT decide" fenced to "a future /dr." The DR-019 ratified core (`PermitLayer`, `compose_layers`, `deciding_layer` on `PermitOutcome`) is built and shipped (commit 4b97d4f); `PermitOutcome.deciding_layer` exists at `crates/rezidnt-gate/src/permit.rs:359` and is populated by `aggregate_async` (`:521/:549/:566`).

Four pieces remain genuinely unbuilt — the daemon/MCP wiring to feed three real layers through the shipped core:

1. **Layer sourcing (the load-bearing fork).** Today there is exactly ONE `[gates.permit]` block per workspace, folded from `workspace.spec.applied` (`bins/rezidentd/src/mcp.rs:140-178`). C8 needs three (admin/dev/session). WHERE do they come from?
2. **`permit_config_for` merge** — source three blocks, return `PermitConfig::from_specs(compose_layers(admin, dev, session))`. The flat consumer path (aggregate) is UNCHANGED; only resolution merges three sourced layers with provenance instead of reading one. Additive.
3. **`McpCore::with_layered_permit_config(admin, dev, session)` test builder** — mirrors `with_permit_config` (`lib.rs:392`); injects three resolved layers for the live oracle (the builder the removed `permit_layered_live.rs` referenced).
4. **`deciding_layer` on the emitted decision fact.** The emit path pins only `outcome.deciding_verifier` (`lib.rs:844-848`); add `outcome.deciding_layer` so `gate_explain` / the `permit.denied` fact surface the deciding LAYER. This makes DR-019 criterion 2's interrogability LIVE — today it is latent in the type. (SP4c auditor's explicit forward note.)

**The load-bearing fork on piece 1:**

- **(a) RECOMMENDED — three distinct sources by authority.** admin from a **daemon/host-level config surface that lives OUTSIDE the workspace spec** (a workspace/dev author physically cannot edit or reorder it); dev from the existing `workspace.spec.applied` `[gates.permit]` (the current single source); session from the run/agent scope. The argument: an admin deny is only *auditably* non-overridable (the DR-019 I6 guarantee) if the admin layer is sourced from OUTSIDE the dev-editable surface. Otherwise "admin" is just a label a dev can rewrite, stricter-wins becomes a naming convention rather than an authority boundary, and the decision fact's `deciding_layer == "admin"` would be a claim the audit trail cannot back. This is the SP4c auditor's point.
- **(b) REJECT — per-entry `layer` tags in one workspace `[gates.permit]` block.** Keeps all three layers inside the dev-editable workspace spec, so a dev can edit the "admin" entries → defeats C8's entire purpose (a real deny a lower layer cannot override). Reject.

**Strongest counterargument (dissent, recorded verbatim per house style):** *"Introducing a NEW daemon/host-level admin config surface is real new surface area and a new place for config to drift, for a session-vs-dev distinction no user has yet requested. The flat `with_permit_config` plus admin-first ordering already gives de-facto stricter-wins — the monotone aggregate makes an earlier Fail un-overridable regardless of which surface it came from. So the whole layered-sourcing apparatus may be ceremony over the concat we already ship."*

**Counter to the counter:** without an out-of-workspace admin source the DR-019 I6 guarantee is hollow — a dev edits the "admin" rule in the workspace spec and the audit trail then *lies* (the fact says `deciding_layer: admin` for a rule the dev authored). "Admin rules first by config-authorship convention" is precisely the fragility C8 removes: non-overridability by *luck of who wrote the file last and in what order*, silently reorderable, with no authority provenance. DR-009 already owner-accepted C8 as table-stakes enforcement breadth versus Omnigent (memo 001), not a speculative add. And the surface is **bounded**: one host config path + one builder + one pinned field; no new verifier kinds, no aggregate change, no dependency. **The owner has accepted this trade knowingly.**

## Decision

1. **Ratify (a): source admin OUTSIDE the workspace spec.** `permit_config_for` sources three `[gates.permit]` layers — admin from a daemon/host-level config surface (outside `workspace.spec.applied`, so dev-unreachable), dev from `workspace.spec.applied` (the current single source, I3), session from the run/agent scope — and returns `PermitConfig::from_specs(compose_layers(admin, dev, session))`. **Reject (b).** The **sourcing boundary — admin outside the dev-editable workspace spec — is ratified here, not deferred.** The exact host-config file FORMAT is an impl/oracle detail.
2. **`permit_config_for` merges three sourced layers with provenance;** the flat consumer path (`aggregate_async`, verdict→decision table) is UNCHANGED (frozen by DR-019 Decision 1). Additive.
3. **Add `McpCore::with_layered_permit_config(admin, dev, session)`** mirroring `with_permit_config` (`lib.rs:392`) — the live-oracle injection point for three resolved layers.
4. **Pin `outcome.deciding_layer` in the emitted decision policy blob** (`lib.rs:844-848`), alongside `deciding_verifier`. The field already exists on `PermitOutcome` (`permit.rs:359`); this makes DR-019 criterion 2 LIVE on the wire.

## Invariant fit

| Inv. | Fit |
|---|---|
| **I3** log is truth | dev resolves from `workspace.spec.applied` (folded state); admin/session from their own sources; the decision fact carries the deciding layer, so the composed verdict replays from log + config. ✓ |
| **I6** determinism / interrogable | admin deny is *auditably* non-overridable because it is sourced OUTSIDE dev's reach AND `deciding_layer` is live on the wire — "why blocked" answers "admin layer", a claim the audit trail backs. Empty/absent layer → escalate, never coerced to pass. ✓ |
| **I2** plane split | the pinned policy blob (now incl. `deciding_layer`) is CAS evidence, not an envelope field; layer specs are small inline config entries, not CAS payload. ✓ |
| **I4** substrates behind traits | config resolution stays a substrate capability behind `permit_config_for`; the core folds run-state itself (DR-011). ✓ |
| **I7** one binary | no new dependency, no crypto. ✓ |

**Warden posture flag:** the pinned-policy JSON gains a `layer` key. This blob is **CAS-pinned decision evidence, not an envelope/subject field**, so it is *likely NOT* a `/subject` concern — but flagged here for the warden to confirm before build, since it is the one shape change on the wire.

## Consequences

- **§9 seam delta:** `permit_config_for` sources three layers instead of one; `with_layered_permit_config` builder added; the decision policy blob pins `deciding_layer`. Additive; no envelope/wire-schema break.
- **§16 roadmap delta:** completes the DR-019 deferred wiring — SP4c is LIVE end-to-end (admin deny non-overridable through `request_permission`; deciding layer surfaced) rather than ratified-core-only. New surface: one host-level admin config path.
- **Risk-register (§18) delta:** *New — host admin config surface / drift.* Mitigated by keeping it the single out-of-workspace source whose sole purpose is the authority boundary; format left to oracle/impl, bounded to one path. *Scope-gravity* (carried from DR-016/DR-019): mitigation holds — the aggregate and verdict→decision table are untouched; only resolution and the emit blob change.
- **No test or acceptance criterion is weakened.** In plain words: this only makes admin denies *harder* to override (sourced beyond dev's edit reach) and makes the deciding layer *visible* on the fact. It relaxes no gate. It is a tightening plus a new capability, not a softening.

**Acceptance-criteria sketch (what `/oracle` encodes — the work order already sits in `crates/rezidnt-gate/tests/permit_layered_precedence.rs:62-69`):**
1. A **live builder** injecting three resolved layers into `McpCore` (`with_layered_permit_config`).
2. `live_admin_deny_not_overridable_by_session_allow` — same `Edit` request, admin denies, session would allow → live `deny` through `request_permission`.
3. The emitted `permit.denied` fact / `gate_explain` surface the deciding **`layer == "admin"`** (two identically-named `tool-allowlist` verifiers disambiguated by authority, I6).
4. All-empty three layers → live `ask`, never `allow`.

## What this does NOT decide

- **No change to the aggregate or the verdict→decision table** (frozen by DR-019 Decision 1). Composition stays concatenate-ordered admin→dev→session; no allow-override primitive.
- **No new verifier kinds.**
- **C3 sole-chokepoint enforcement stays fenced** under DR-009.
- The exact **host-config file FORMAT** for the admin layer is left to `/oracle` + the implementer. The **sourcing BOUNDARY** (admin outside the workspace spec) is ratified, not deferred.

*Amendments to this record require DR-021.*
