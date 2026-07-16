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
| `artifact.captured` | 1 | any capturing component (run capture, gate evidence, git adapter) | Bytes were persisted to the CAS; the fabric carries the ref only (I2). | `{ref: CasRef, provenance}` — ratified below; the v0 sketch's top-level `mime`/`bytes` ride inside the ref |

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

## Payload schemas — v1 baselines

Ratified per-subject payload shapes. A field marked `?` is optional and may be absent; readers tolerate unknown fields (additive evolution, doc §5). Types map JSON onto the rezidnt-types Rust shapes; `CasRef` is `{hash: blake3 hex string, bytes: u64, mime: string}`. Subjects not listed here have no ratified payload schema yet — their shape is proposal-stage until a warden session ratifies it. These baselines define v = 1; a breaking change to any of them mints v+1 per the change discipline above.

### S1 set (ratified 2026-07-16)

**`workspace.opened` v1** — the envelope `workspace` id is the entity key (reducers fold on it); the payload carries human-facing identity.
- `name: string` — workspace name from the project spec.
- `root: string` — canonicalized absolute path of the workspace root.

**`workspace.spec.applied` v1**
- `spec_ref?: CasRef` — the applied spec file as persisted to the CAS; optional because an open may apply an inline/default spec with no persisted blob.
- `agents: [string]` — spec agent names configured by the applied spec.

**`worktree.allocated` v1**
- `path: string` — canonicalized worktree path; exists on disk at emission time.
- `branch?: string` — branch checked out in the worktree, when one was requested.
- `allocator: "rezidnt"` — sole-allocator model (DR-001). The value `"human"` is reserved for out-of-band observation and is never emitted by rezidnt on this subject.

**`agent.spawned` v1**
- `run: string` — RunId ULID; the key every `agent.*` fact carries.
- `agent: string` — spec agent name.
- `harness: string` — harness identifier (e.g. `claude-code`).
- `harness_version?: string` — as probed at spawn; version-gated per adapter.
- `pid?: u32` — OS process id when known at emission.
- `badge_id: string` — loggable badge identifier. NEVER the badge token (doc §12); the token exists only in the spawned environment.

**`agent.status.changed` v1** — state delta.
- `run: string`
- `from: string`, `to: string` — run-status vocabulary: `spawning | running | completed | failed | signaled`.

**`agent.completed` v1** — dossier accounting (DR-001).
- `run: string`
- `status: "success" | "error"` — the harness result outcome. This is a distinct vocabulary from the run-status values of `agent.status.changed` (`completed`/`failed`); reducers must not conflate the two.
- `cost: {total_usd: f64, input_tokens: u64, output_tokens: u64}`
- `num_turns: u64`
- `duration_ms: u64`
- `session_id?: string` — harness session id, captured for run checkpointing (`--resume`, DR-001).

**`agent.signaled` v1**
- `run: string`
- `signal: string` — the delivered signal name (e.g. `SIGTERM`, `SIGKILL`).
- `escalation?: "term" | "kill"` — present when the signal came from the reaper's TERM→KILL escalation path, recording the stage; absent for out-of-band signals.

**`agent.tool.invoked` v1** — harness telemetry (DR-001).
- `run: string`
- `tool: string` — tool name as reported by the harness.
- `input_summary?: string` — truncated human-readable summary of the tool input; bulk input goes to the CAS and rides `artifact.captured`, never inline (I2).

**`agent.message` v1** — harness telemetry (DR-001). Carries exactly one of `text` / `ref`.
- `run: string`
- `role: "assistant"` — fixed in v1; other roles arrive additively.
- `text?: string` — inline only when ≤ 8 KiB (DEFAULT cap; keeps envelope headroom under the 32 KiB I2 hard cap).
- `ref?: CasRef` — bulk message body persisted to the CAS.

**`artifact.captured` v1**
- `ref: CasRef` — carries `hash`, `bytes`, `mime`; the v0 sketch's top-level `mime`/`bytes` live inside the ref (no duplicated fields).
- `provenance: {run?: string, kind: string, chunk?: u64}` — `run` when the artifact belongs to an agent run; `kind` names the capture class (e.g. `capture-chunk`, `diff`, `gate-evidence`); `chunk` is the 0-based ordinal within the run's capture stream, present iff `kind = "capture-chunk"`. DR-001 chunked run output rides this subject via ref-only manifest facts (`rezidnt-run` `ManifestEntry {run, chunk, ref}`); whether high-rate capture chunks deserve a dedicated subject is an open question flagged for `/dr` — not minted here.

## Changelog

- 2026-07-16 · warden · bootstrap: taxonomy v0 transcribed from architecture doc v0.2 Appendix B; DR-001 additions `agent.tool.invoked` and `agent.message` (native harness telemetry); DR-001 scope note on `worktree.observed`/`worktree.conflict` (out-of-band guard only, rezidnt sole allocator); all subjects minted at `v = 1`.
- 2026-07-16 · warden · S1 payload ratification: v1 payload baselines recorded for `workspace.opened`, `workspace.spec.applied`, `worktree.allocated`, `agent.spawned`, `agent.status.changed`, `agent.completed`, `agent.signaled`, `agent.tool.invoked`, `agent.message`, `artifact.captured` — additive documentation of shape, every subject stays `v = 1`; `artifact.captured` sketch normalized (top-level `mime`/`bytes` subsumed into `ref: CasRef`); capture chunks ride `artifact.captured` via `provenance.kind = "capture-chunk"` + `provenance.chunk`, dedicated capture subject deferred and flagged for `/dr`.
