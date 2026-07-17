[← Decision records index](../rezidnt-architecture.md#20-decision-records) · [Architecture plan](../rezidnt-architecture.md)

# Decision Record DR-005 — Badge model consolidation (the badge bundle)

**Date:** 2026-07-17 · **Status:** ACCEPTED (owner) · **Amends:** §12 (badges: scope, mutating-call rule) and the `spec/ontology.md` badge section (`badge.issued` semantics). No invariant text touched; I3/I6 unaffected. The one BINDING clarification: the §12 "badge on every mutating MCP call" rule is narrowed to "every *state-mutating* call."

## Context

Four coupled threads, verified in code. (a) `badge.issued`/`badge.revoked` are reserved subjects with **no emitter**; issuance is already attributable via `agent.spawned.badge_id` (`bins/rezidentd/src/runs.rs:593,611`). (b) The daemon mints one **daemon-lifetime operator badge** for local human clients, announced in the 0600 lockfile (`bins/rezidentd/src/main.rs:192`) — it has no run, workspace scope, or short expiry, so it does not fit §12's per-`AgentRun` `{workspace, verb set, expiry}` shape. (c) `badge_id` rides `agent.spawned` only; `check_badge` computes but discards a loggable id for `open_project`/`spawn_agent` (`crates/rezidnt-mcp/src/lib.rs:284,313`). (d) `gate_explain` appends `gate.explained` **unbadged** (lib.rs:346), as does `tail_events` — while §12 says "badge on every *mutating* call." Dissent recorded: a strict reading of §12 treats any log-append as a mutation, so leaving interrogation unbadged is a de-facto narrowing.

## Decision (Option A)

- **§12 rule clarified (BINDING):** a badge is required on every **state-mutating** call — spawn, open, merge — i.e. calls that change fleet/workspace/repo state. Interrogation (`gate_explain`, `gate why`) and `tail_events` are **read-class**: they leave audit breadcrumbs but are not badged. This preserves the I6 property that a blocked agent can always read *why* it was blocked, badge or not.
- **`badge.issued`: drop the emitter (DEFAULT).** `agent.spawned.badge_id` is the issuance record. The subject stays reserved (never renamed), annotated "no emitter; attribution rides `agent.spawned`." Cheap to revisit if a run-independent delegation use case appears.
- **Operator badge: blessed as a distinct daemon-lifetime badge class (DEFAULT).** §12's per-`AgentRun` shape is the *agent* badge; the operator badge is the *local-human* badge (possession of the 0600 lockfile = capability). Macaroon attenuation stays PROVISIONAL.
- **`badge_id` on other mutation facts: deferred, not foreclosed (DEFAULT)** — no folding reducer consumes it today.

## Consequences

- **Zero code change**; matches the system as built and debriefed. The change narrows the §12 sentence for `gate_explain`/`tail_events` — a real (intended-all-along) relaxation, recorded here so it is explicit.
- Retires the S3-T3 carried finding and the badge-bundle `/dr` item. Follow-on: a warden `/subject` pass updates `spec/ontology.md` `badge.issued` to "reserved, no emitter" (ontology edit, gated).

*Amendments to this record require DR-008.*
