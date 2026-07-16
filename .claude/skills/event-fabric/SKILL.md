---
name: event-fabric
description: >-
  The rezidnt event envelope, subject taxonomy grammar, log/CQRS model, and delivery
  semantics. This skill should be used when implementing or reviewing the fabric, reducers,
  materialized state, the event log, or anything that emits or consumes events — and when
  adding event subjects. Load for work on rezidnt-types, rezidnt-fabric, or rezidnt-state.
user-invocable: false
version: 0.2.0
---

# Event fabric

Full design: architecture doc §5–§6 and Appendix B. This is the working reference.

## Envelope (shape BINDING; additive evolution only)
Fields: `id: Ulid` (time-ordered), `ts` (UTC daemon clock), `v: u16` (payload schema version per subject), `source: SourceId`, `workspace: Option<WorkspaceId>`, `subject: Subject`, `correlation: Ulid` (causal chain), `causation: Option<Ulid>` (direct trigger), `payload: serde_json::Value` (≤ 32 KiB — I2). JSON Lines on the wire, JSON in the log column. `rezidnt-types` owns all serde derives so a binary re-encoding is a later drop-in.

## Subject grammar (warden-enforced)
`noun.verb[.qualifier]`. Past tense for facts (`worktree.allocated`, `gate.passed`); present for state deltas (`agent.status.changed`). BINDING rules: subjects are NEVER renamed (deprecate only); payloads evolve additively; a breaking payload change mints `v+1` and every live reducer must handle all versions. Taxonomy v0 is Appendix B; the canonical copy is `spec/ontology.md`, edited only via `/subject`.

## Log (DEFAULT: SQLite, WAL)
Append is the commit point; ULID uniqueness gives exactly-once. Columns include a `chain BLOB` = `blake3(prev.chain || id || payload)` for tamper-evidence (near-zero cost, on by default). Indices on `(subject, seq)`, `(workspace, seq)`, `(correlation)`. The log is retained forever by default — it is the compliance artifact and the eval corpus; compaction is PROVISIONAL and disk-pressure-gated.

## Delivery semantics (BINDING client rule)
In-process fan-out is `tokio::sync::broadcast` (at-least-once to live subscribers). A subscriber that overflows its buffer receives `Lagged(n)` and MUST resync from the log by last-seen ULID — never pretend continuity. This single rule keeps slow clients from back-pressuring the daemon.

## Materialized state (CQRS-lite)
Pure reducers `fn apply(&mut Graph, &Event)` in rezidnt-state fold the log into the entity graph (Project → Workspace → Worktree/Session/AgentRun → Dossier; plus GateDef/VerifierDef, Artifact, AdapterHealth). Each entity class exposes a `watch` channel; clients subscribe to state, not the firehose, unless they ask. Snapshots every 5,000 events or 15 min (DEFAULT). `rezidnt rebuild` refolds from seq 0; rebuild-diverges-from-live is a reducer bug and a release blocker (property-tested).
