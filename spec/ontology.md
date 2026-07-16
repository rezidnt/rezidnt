# rezidnt — subject taxonomy

**Taxonomy version:** v0 · **This file is the canonical copy.** `docs/rezidnt-architecture.md` Appendix B is the excerpt; when they diverge, this file wins and the appendix is stale.
**Provenance:** bootstrapped 2026-07-16 from architecture doc v0.2 — Appendix B, plus the DR-001 amendments (native harness telemetry subjects; sole-allocator worktree model).
**Custody:** this file is edited only through `/subject` inside a warden-gated ontology session; direct edits are blocked by the ontology-gate hook. Every change appends one line to the changelog at the bottom. Any change that amends a BINDING item routes through `/dr` first.

## Grammar and change discipline (BINDING)

- Subjects are dot-namespaced: `noun.verb[.qualifier]`.
- Past tense for facts (`worktree.allocated`, `gate.passed`); present tense for state deltas (`agent.status.changed`).
- Subjects are **never renamed** — deprecation only. A deprecated subject's name stays reserved forever.
- Payloads evolve **additively**. A breaking payload change mints `v+1` for that subject, and every live reducer must handle all live versions.
- Payloads ride the Event envelope (doc §5): `v: u16` is the per-subject payload schema version; payloads are JSON, hard-capped at 32 KiB (I2). Larger content becomes a CAS ref (`CasRef { hash, bytes, mime }`), never inline bytes.
- Taxonomy v0 starts every subject at `v = 1`.

**Grammar note (grandfathered):** six v0 subjects keep their Appendix B forms verbatim even though they do not strictly parse as `noun.verb`: `worktree.conflict`, `gate.inconclusive`, `diff.ready`, `daemon.warning`, `daemon.error`, `agent.message`. They are canonical and will never be renamed; strict `noun.verb[.qualifier]` applies to all new subjects from v0 forward.

## Subjects

### workspace

| Subject | v | Emitter | Semantics |
|---|---|---|---|
| `workspace.opened` | 1 | daemon (`rezidnt open` materialization) | A workspace was materialized from a project spec and is live. |
| `workspace.closed` | 1 | daemon | A workspace was shut down and is no longer live. |
| `workspace.spec.applied` | 1 | daemon | A project spec (`rezidnt.toml` shape, doc §13) was applied to a workspace — layout intent, agents, and gate bindings configured. |

### worktree

rezidnt is the **sole allocator** of worktrees (DR-001). `worktree.observed` and `worktree.conflict` are retained only to guard against out-of-band human git activity; the two-allocator reconciliation problem is deleted, not solved (DR-001, trait changes).

| Subject | v | Emitter | Semantics |
|---|---|---|---|
| `worktree.allocated` | 1 | git adapter (RepoSubstrate) | rezidnt allocated a worktree; registered under its canonicalized path with branch and allocator recorded. |
| `worktree.observed` | 1 | git adapter (FS watcher) | A worktree not allocated by rezidnt was observed on disk — out-of-band human git activity guard (DR-001). |
| `worktree.conflict` | 1 | git adapter (worktree registry) | A second claim landed on an already-registered canonicalized path; emitted instead of silently double-tracking — out-of-band human git activity guard (DR-001). |
| `worktree.released` | 1 | git adapter (RepoSubstrate) | A worktree was released and its registry entry closed. |

### session

Terminal-substrate lifecycle. DR-001 removed TerminalSubstrate from Phases 1–2 and reserves it as the Phase 3 seam; these subjects are retained (not deprecated) and their emitter arrives with Phase 3.

| Subject | v | Emitter | Semantics |
|---|---|---|---|
| `session.created` | 1 | terminal substrate (Phase 3 seam) | A terminal session came into existence. |
| `session.attached` | 1 | terminal substrate (Phase 3 seam) | A client attached to a session. The byte stream itself is out-of-band (I2); this is the lifecycle fact. |

### pane

| Subject | v | Emitter | Semantics |
|---|---|---|---|
| `pane.spawned` | 1 | terminal substrate (Phase 3 seam) | A pane was spawned within a session. |
| `pane.exited` | 1 | terminal substrate (Phase 3 seam) | A pane's process exited. |

### agent

Lifecycle facts come from `rezidnt-run` (the ProcessSubstrate, DR-001). Harness telemetry subjects (`agent.tool.invoked`, `agent.message`) were added by DR-001 for the native claude-code adapter: stream-json events map onto the fabric as typed telemetry instead of terminal-scraping heuristics.

| Subject | v | Emitter | Semantics |
|---|---|---|---|
| `agent.spawned` | 1 | rezidnt-run (AgentSubstrate impl) | An agent run was spawned — environment scrubbed, badge injected at spawn (DR-001). |
| `agent.status.changed` | 1 | rezidnt-run / harness adapter | State delta: the run's status transitioned. For claude-code, mapped from stream-json telemetry (DR-001). |
| `agent.blocked` | 1 | rezidnt-run + gate engine | The run became blocked at a gate; `gate why` / `gate_explain` answers with the failing verifier and evidence (I6). |
| `agent.completed` | 1 | rezidnt-run (reaper) | The run finished; exit status recorded. |
| `agent.signaled` | 1 | rezidnt-run (reaper) | A signal was delivered to the run (TERM→KILL escalation with grace). |
| `agent.tool.invoked` | 1 | native harness adapters (DR-001) | Harness telemetry: the agent invoked a tool. Mapped from claude-code stream-json tool-call events. **Added by DR-001.** |
| `agent.message` | 1 | native harness adapters (DR-001) | Harness telemetry: an assistant message emitted by the agent. Mapped from claude-code stream-json message events. **Added by DR-001.** |

### gate

Verdicts are `pass | fail | inconclusive` — never a bare boolean; `inconclusive` is never coerced to `pass` (I6, doc §8). Evidence blobs go to the CAS; gate events carry refs only.

| Subject | v | Emitter | Semantics |
|---|---|---|---|
| `gate.entered` | 1 | gate engine (rezidnt-gate) | A run entered a named gate (`vet`, `pre_merge`, `post_run`/debrief). |
| `gate.passed` | 1 | gate engine | Every verifier on the gate passed. Payload carries evidence CAS refs. |
| `gate.failed` | 1 | gate engine | A verifier failed. Payload carries the failing verifier and its evidence CAS refs. |
| `gate.inconclusive` | 1 | gate engine | A verifier was inconclusive (timeout, nonzero exit, malformed output). Routed to a human; never coerced to pass. |
| `gate.explained` | 1 | gate engine | An interrogation (`gate why` / `gate_explain`) was answered: failing verifier, evidence refs, exact inputs. |

### artifact

| Subject | v | Emitter | Semantics | Payload sketch |
|---|---|---|---|---|
| `artifact.captured` | 1 | any capturing component (run capture, gate evidence, git adapter) | Bytes were persisted to the CAS; the fabric carries the ref only (I2). | `{ref, mime, bytes, provenance}` |

### diff

| Subject | v | Emitter | Semantics |
|---|---|---|---|
| `diff.ready` | 1 | git adapter (notify watcher, debounced 250 ms) | A diff summary for a worktree is ready as a CAS ref. S2 acceptance: within 1 s of write, post-debounce. |

### merge

| Subject | v | Emitter | Semantics |
|---|---|---|---|
| `merge.completed` | 1 | git adapter (git CLI mutations) | A diff was merged. |
| `merge.rejected` | 1 | git adapter | A merge attempt was rejected. |

### adapter

| Subject | v | Emitter | Semantics |
|---|---|---|---|
| `adapter.health.changed` | 1 | supervisor (rezidnt-supervise) | State delta: an adapter's health transitioned (`Starting → Healthy → Degraded → Faulted`, doc §7). The crash-loop breaker parks an adapter in `Faulted` visibly — never a silent retry storm. |

### daemon

WARN and above from `tracing` are mirrored onto the fabric as `daemon.*` events so the system's own misbehavior is queryable with the same tools as everything else (doc §14).

| Subject | v | Emitter | Semantics |
|---|---|---|---|
| `daemon.started` | 1 | daemon core | The daemon started. |
| `daemon.warning` | 1 | daemon core | A WARN-level condition, mirrored from tracing onto the fabric. |
| `daemon.error` | 1 | daemon core | An ERROR-level condition, mirrored from tracing onto the fabric. |

### badge

| Subject | v | Emitter | Semantics |
|---|---|---|---|
| `badge.issued` | 1 | daemon (security layer) | A per-run capability badge was minted: 256-bit token scoped to `{workspace, verb set, expiry}` (doc §12), injected at spawn (DR-001). Makes an agent's writes attributable in the log. |
| `badge.revoked` | 1 | daemon (security layer) | A badge was revoked; mutating calls bearing it are refused thereafter. |

## Changelog

- 2026-07-16 · warden · bootstrap: taxonomy v0 transcribed from architecture doc v0.2 Appendix B; DR-001 additions `agent.tool.invoked` and `agent.message` (native harness telemetry); DR-001 scope note on `worktree.observed`/`worktree.conflict` (out-of-band guard only, rezidnt sole allocator); all subjects minted at `v = 1`.
