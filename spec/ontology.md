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
| `diff.merged` | 1 | git adapter (RepoSubstrate, git CLI mutations) | A verified diff was merged into the target branch and the worktree lifecycle closed. Emitted only after the `pre_merge` gate `gate.passed` verdict (golden-path exit). Distinct from `merge.completed`: `diff.merged` is the worktree-lifecycle fact keyed on `{run, worktree, diff}` that the S4 reducer folds to `status = "merged"`; `merge.completed` is the v0 merge-mutation fact. |

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

**Operator badge (S3 note):** doc §12 badges are per-`AgentRun` capability tokens; S3 additionally mints one daemon-lifetime **operator badge** for local human clients, announced via the 0600 MCP lockfile (`{pid, port, url, badge}`) — possession of the file is possession of the capability. This is a §12/DEFAULT security-layer reading, pinned by the S3 board; the lockfile shape itself is discovery metadata, **not fabric surface**, and is documented at doc level (scribe scope). The concept is blessed here only insofar as `badge.*` semantics must eventually accommodate a badge scoped to the daemon lifetime rather than a run — folded into the open `badge.issued` emit-or-drop question below.

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
- `idempotency_key?: string` — caller-supplied spawn idempotency key, recorded on the fact so the key→run map is log-derivable (I3; S3-T1 remediation, pre-S4). Present iff the spawning call supplied one: MCP `spawn_agent` requires a key (`SpawnAgentArgs.idempotency_key`), other spawn paths carry none — optional is the honest shape, never synthesized for keyless spawns. Constraints: non-empty, ≤ 256 bytes UTF-8 (DEFAULT cap; a key is a short opaque token, trivially inside I2). Opaque to the daemon beyond byte equality. Dedup scope is per workspace — the envelope `workspace` id paired with the key: a spawn request bearing a key already recorded on an `agent.spawned` fact for that workspace answers with that fact's `run` and emits nothing new, including across daemon restart. Emitter obligation: a keyed spawn fact MUST set the envelope `workspace` field, or the rebuilt map has no scope. **Added 2026-07-17 (additive; v stays 1).**
- `bare?: bool` — whether the spawn was governed as a *bare* agent (no interactive/permission-prompt affordances): the enforcement decision the `vet` gate's `bare-mode` verifier checked, recorded on the fact so the governance posture is log-derivable (DR-001: enforcement decisions recorded in events; I3). Optional/additive: present on governed spawns that ran through `vet`; absent on legacy or ungoverned spawn paths (never synthesized — absence is honest, not `false`). **Added 2026-07-17 (S4; additive; v stays 1).**
- `allowed_tools?: [string]` — the composed allow-list of tool names the agent was spawned with, as enforced pre-spawn — the permission composition the `vet` gate's `allowed-tools` verifier checked (DR-001: allowedTools recorded in events; I3). Records what was granted, for log-derivable attribution of what the agent *could* do vs. `agent.tool.invoked` facts for what it *did*. Optional/additive: present on governed spawns; absent when no tool policy was composed. A short list of short strings — trivially inside I2; a pathological policy stays a CAS concern only if it ever exceeded the envelope cap (it does not for tool-name lists). **Added 2026-07-17 (S4; additive; v stays 1).**
  - Note on `harness_version`: already ratified above as `harness_version?: string` (S1 baseline). The `vet` gate's `pinned-version` verifier reads/enforces it; no new field is minted for version pinning — the existing optional field carries the recorded decision. Cross-referenced here so the governed-spawn triple (`bare`, `harness_version`, `allowed_tools`) reads as one set.
- Interaction check (recorded 2026-07-17): the three governed-spawn fields are mutually independent optionals and do not collide with `idempotency_key?` (dedup key) or `badge_id` (attribution). The `badge.issued` emit-or-drop and `badge_id`-on-mutation-facts deferrals are untouched and not foreclosed — no field added here occupies a name or scope reserved for those decisions.

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

### S2 set (ratified 2026-07-17)

**`worktree.observed` v1** — out-of-band guard fact (DR-001); registers the tree so re-observation stays silent.
- `path: string` — canonicalized path of the observed tree; the registry key it occupies.
- `allocator: "human"` — fixed in v1: observation is by definition out-of-band. The value `"rezidnt"` is never emitted on this subject (exact mirror of the `worktree.allocated` reservation).
- `branch?: string` — branch checked out in the observed tree when the watcher can read one; absent for detached human trees. Additive; family coherence with `worktree.allocated`.

**`worktree.conflict` v1** — emitted instead of double-tracking; exactly one per collision (adapter obligation), each logged fact folded once (reducer obligation, I3).
- `path: string` — the contested **canonicalized registry key** (the path as registered, not the colliding spelling).
- `claimed_path?: string` — the colliding claim as observed, pre-canonicalization, present when it differs from `path`. Triage evidence: names the spelling that collided without a registry read.
- `holder?: "rezidnt" | "human"` — allocator recorded on the standing registry entry at collision time, making the fact self-contained for triage. (Named `holder`, not `allocator`: on this subject "allocator" would be ambiguous between the entry's holder and the out-of-band claimant.)

**`worktree.released` v1**
- `path: string` — canonicalized registry key; byte-identical to the spelling the allocation minted (both are canonicalized, so the strings agree — the release fact closes exactly the entry the allocated fact opened).
- `branch?: string` — branch the worktree carried, when it had one. Additive; lets a consumer of the release fact alone know what went away, family coherence with `worktree.allocated`.

**`diff.ready` v1** — I2 contract: the diff summary is a CAS ref, never inline diff bytes.
- `worktree: string` — canonicalized path of the worktree the diff concerns (the same registry key `worktree.allocated` minted).
- `diff: CasRef` — the diff summary persisted to the CAS; resolvable at emission time. `mime` is `text/x-diff` (DEFAULT, not load-bearing); a real tree change yields a non-empty summary (`bytes > 0`).

### S3 set (ratified 2026-07-17)

Strict supersets of the S3 oracle pins (`rezidnt-mcp/tests/gate_explain.rs`, `bins/rezidentd/tests/mcp_http.rs`, `spec/fixtures/s3_gate_forced_failure.jsonl`, `s3_gate_inconclusive.jsonl`). The verdict is carried by the **subject itself** (`gate.failed` → `fail`) — never a bare boolean field, and `inconclusive` is never coerced (I6). The full verifier engine is S4; these baselines cover exactly what S3's forced-failure / `gate_explain` slice emits and interrogates, and S4 extends them additively (e.g. network-opt-in recording, `cost_ms`). `gate.passed` has no S3 emitter or pin and its baseline is deliberately **not** ratified here — S4 scope.

**`gate.entered` v1** — the envelope `correlation` groups the gate run (doc §5); `causation` on subsequent verdict facts points back at this fact.
- `run: string` — RunId ULID; the key every gate fact carries.
- `gate: string` — the named policy point (`vet`, `pre_merge`, `post_run`); a string, not a closed enum — gate defs are named policy points (doc §8).

**`gate.failed` v1**
- `run: string`, `gate: string` — as `gate.entered`.
- `verifier: string` — the **failing** verifier's name (doc §8; exactly what `gate_explain` must return).
- `evidence: [CasRef]` — evidence blobs live in the CAS; the fact carries refs only (I2, doc §8 BINDING).
- `inputs: object` — the exact verifier input document, recorded **verbatim** (doc §8 stdin contract: `{gate, refs, params, timeout_ms}`; `refs` values are `cas:blake3:<hex>` strings — inputs pinned by content hash, per the determinism BINDING). Deliberately an opaque-but-recorded object, not a decomposed schema: `gate_explain` returns it byte-for-byte from the log (I6 interrogability), and the S4 engine widens the document additively without a payload break here.

**`gate.inconclusive` v1** — same shape as `gate.failed` plus `reason`; routed to a human, never coerced to `pass` (I6).
- `run`, `gate`, `verifier`, `evidence: [CasRef]`, `inputs` — exactly as `gate.failed`.
- `reason: string` — v1 vocabulary from the §8 causes: `timeout | nonzero_exit | malformed_output`; new causes arrive additively as strings.

**`gate.explained` v1** — interrogations are facts too. The explanation content (verifier, evidence, inputs) is **derived** from the verdict fact already on the log (I3) and is not duplicated into this payload.
- `run: string` — the interrogated run; the pinned minimum is this field alone.
- `gate?: string`, `verdict?: "pass" | "fail" | "inconclusive"` — optional self-contained triage context; when present, `verdict` is the recorded verdict verbatim (never coerced, I6).

### S4 set (ratified 2026-07-17)

Strict supersets of the S4 oracle pins: `crates/rezidnt-gate/tests/`, `crates/rezidnt-state/tests/s4_gates.rs`, `bins/rezidentd/tests/{vet_gate.rs,golden_path.rs}`, and the S4 fixtures `spec/fixtures/s4_{verified_run,vet_refusal,replay_verified,replay_divergence_alarm}*`. Additive-only; every subject stays `v = 1`. The `inputs` document is recorded **verbatim** per the §8 stdin contract and is measured at ~182 bytes for the largest S4 pin (a `pre_merge` diff-scope record with a glob `allow` list); a full 3-verifier `gate.passed` payload is ~838 bytes — three orders of magnitude under the 32 KiB I2 hard cap. Tool-name lists and glob params are short strings; larger content is already a CAS ref by the §8 contract, so no S4 payload approaches the cap.

**`gate.passed` v1** — the S3 baseline deliberately left this unratified ("until an emitter exists"); the S4 engine is that emitter. Now ratified.
- `run: string`, `gate: string` — as `gate.entered`.
- `verifiers: [VerifierRecord]` — one record **per verifier** that ran on the gate, in execution order. Replay re-executes each verifier against its own recorded inputs, and the slice exit requires per-verifier recorded cost — so the evidence is carried per verifier, not flattened on the event. Each `VerifierRecord` is:
  - `verifier: string` — the verifier's name (§8).
  - `cost_ms: u64` — wall-clock cost of this verifier's execution (the §8 stdout `cost_ms`); the "recorded cost" the golden-path exit asserts.
  - `evidence: [CasRef]` — this verifier's evidence blobs as CAS refs (I2); empty when the verifier emitted none. The reducer flattens all records' evidence hashes in order into `GateState.evidence`.
  - `inputs: object` — the exact per-verifier §8 stdin document, recorded verbatim (`{gate, refs, params, timeout_ms}`; `refs` values are `cas:blake3:<hex>` — inputs pinned by content hash, determinism BINDING). Same opaque-but-recorded discipline as `gate.failed.inputs`; widened additively by the engine without a payload break.
- **Asymmetry with `gate.failed` v1 (rationale, ratified):** `gate.failed` carries exactly ONE verifier record (flat: `verifier`, `evidence`, `inputs` at payload top level) — the *first* verifier to fail short-circuits the gate, so there is a single failing verifier and no further verifiers ran. `gate.passed` carries ALL verifier records because a pass means every verifier ran to completion and each contributes replayable evidence + recorded cost. The shapes are deliberately different (single-failing vs. full-pass-evidence), NOT a nested-vs-flat accident: the pins fix both (`vet_gate.rs` asserts a `verifiers` array on passed and a top-level `verifier` on failed; `s4_gates.rs` folds both). This asymmetry is intentional and load-bearing; it is not a candidate for later "harmonization" without a `v+1` on both subjects.

**`diff.merged` v1** — NEW subject (golden-path merge fact); closes the worktree lifecycle after a verified `pre_merge` pass.
- `run: string` — RunId ULID; ties the merge to the agent run (the golden-path exit asserts `diff.merged.payload.run == run_id`).
- `worktree: string` — canonicalized worktree path; the same registry key `worktree.allocated`/`diff.ready` minted. The reducer folds `status = "merged"` here, inserting the entry even if never allocated (the log is truth, I3 — `s4_gates.rs::diff_merged_marks_the_worktree`).
- `diff: CasRef` — the merged diff summary as a CAS ref (I2); the reducer pins `last_diff = Some(diff.hash)`.
- **New-subject checklist (recorded):** *grammar* — `diff.merged` parses `noun.verb`, past tense for a fact (conformant; unlike grandfathered `diff.ready`). *family coherence* — sits in the `diff` section with `diff.ready`; shares the `worktree` + `diff: CasRef` shape of `diff.ready` and adds `run` (the merge is attributable to a run; a diff-summary is not). *emitter* — git adapter (RepoSubstrate), the S4 golden-path emitter. *reducer consumer exists* — yes: `rezidnt-state` S4 reducer scaffold folds it (`s4_gates.rs` test `diff_merged_marks_the_worktree`; `s4_verified_run.expected.json` counts it). *not a duplicate of `merge.completed`* — `merge.completed` (v0, `merge` section) is the raw merge-mutation fact; `diff.merged` is the worktree-lifecycle-closing fact keyed on the run and folded by the S4 reducer. They coexist; `diff.merged` is not a rename or synonym (distinct key set, distinct consumer).
- **COLLISION with the taxonomy drift guard (reported, not silently resolved):** adding `diff.merged` as a new subject table row makes `spec/ontology.md` declare 34 subjects where `rezidnt_types::taxonomy::SUBJECTS_V0` declares 33. The committed test `crates/rezidnt-types/tests/taxonomy_drift.rs` asserts the two match exactly (same set, same table order) and runs under `/vet` (`cargo test --workspace`). Ratifying `diff.merged` therefore REQUIRES a one-line companion edit — insert `"diff.merged",` into `SUBJECTS_V0` (in the `// diff` group, immediately after `"diff.ready"`, to preserve ontology-table order). Per the warden protocol step 4 this const lives in `rezidnt-types` and is normally edited in the same session; this session's work order scoped edits to `spec/ontology.md` ONLY, so the companion edit is **flagged for the implementer as a same-commit obligation** rather than applied here. Until it lands, `/vet` will be red on `taxonomy_drift.rs`. This is a scope collision between the session constraint and the drift coupling, not an ontology-vs-pin content collision — the pin (the reducer/fixtures) and the ontology agree on `diff.merged`.

**`agent.spawned` v1 — governed-spawn additive fields** (`bare?`, `allowed_tools?`; `harness_version?` pre-existing) are documented inline in the `agent.spawned` S1 baseline above (kept with the rest of that subject's fields). Additive; v stays 1.

### S4 open questions — decided or tracked

- **(a) exec-verifier network opt-in — where recorded (DECIDED, no new field):** §8 says "no network by default … unless the gate def opts in, recorded in the event." The opt-in is a property of the *verifier's inputs*, so it rides the already-verbatim `inputs` object on `gate.passed`/`gate.failed`/`gate.inconclusive` — i.e. `inputs.params` (the §8 `params` the verifier receives) carries the opt-in flag when the gate def sets it. No new top-level payload field is minted: the recorded-verbatim `inputs` document already satisfies "recorded in the event," and adding a parallel top-level field would duplicate state the §8 stdin contract owns. When the S4 engine begins emitting the flag, it appears inside `inputs.params` additively (the `inputs` object is explicitly "widened additively by the engine without a payload break"). The S4 pins carry `params: {}` / `params: {allow: […]}` with no network opt-in exercised, so this decision foreclosed nothing the pins fixed. If a future gate needs the flag surfaced OUT of the opaque `inputs` for indexing/query, that is a `v+1` proposal with a consuming reducer — tracked, not minted.
- **(b) debrief replay divergence — integrity alarm as a log FACT? (TRACKED, not ratified):** the S4 pins (`golden_path.rs::cli_debrief_divergence_raises_integrity_alarm`) fix the alarm as a **CLI-surface report** (`report.alarms[]` on `debrief --json`, exit 3) — a derived read over log + CAS, emitting nothing. Whether a divergence should ALSO land a durable fact on the log (candidate subjects: reuse `daemon.error`, or a new `integrity.alarm` / `gate.diverged`) is additive and has no pin, no emitter, and no folding reducer today. Minting it now would ratify a subject with no consumer (dead-letter risk) and touches the daemon's self-observation posture (§14, `daemon.*` mirroring) — an owner/`/dr` call, not a warden ratification. **Tracked for `/dr`:** "should replay-divergence integrity alarms be durable log facts, and under which subject." Not foreclosed — the CLI-report-only behavior the pins fix remains correct and additive to either outcome.

## Changelog

- 2026-07-16 · warden · bootstrap: taxonomy v0 transcribed from architecture doc v0.2 Appendix B; DR-001 additions `agent.tool.invoked` and `agent.message` (native harness telemetry); DR-001 scope note on `worktree.observed`/`worktree.conflict` (out-of-band guard only, rezidnt sole allocator); all subjects minted at `v = 1`.
- 2026-07-16 · warden · S1 payload ratification: v1 payload baselines recorded for `workspace.opened`, `workspace.spec.applied`, `worktree.allocated`, `agent.spawned`, `agent.status.changed`, `agent.completed`, `agent.signaled`, `agent.tool.invoked`, `agent.message`, `artifact.captured` — additive documentation of shape, every subject stays `v = 1`; `artifact.captured` sketch normalized (top-level `mime`/`bytes` subsumed into `ref: CasRef`); capture chunks ride `artifact.captured` via `provenance.kind = "capture-chunk"` + `provenance.chunk`, dedicated capture subject deferred and flagged for `/dr`.
- 2026-07-17 · warden · S2 payload ratification: v1 payload baselines recorded for `worktree.observed`, `worktree.conflict`, `worktree.released`, `diff.ready` — strict supersets of the S2 oracle-pinned minimums (adapter tests, `s2_worktrees.rs` reducer tests, `spec/fixtures/s2_*.jsonl`); additive-only, every subject stays `v = 1`, no reducer/fixture changes required. Additive fields beyond the pinned minimum: `branch?` on observed/released, `claimed_path?` + `holder?` on conflict. Open items left tracked, not expanded: `daemon.warning` payload ratification, `badge.issued` emit-or-drop, capture-chunk subject (still flagged for `/dr`).
- 2026-07-17 · warden · S3 payload ratification: v1 payload baselines recorded for `gate.entered`, `gate.failed`, `gate.inconclusive`, `gate.explained` — strict supersets of the S3 oracle pins (`gate_explain.rs`, `mcp_http.rs`, `spec/fixtures/s3_gate_*.jsonl`); additive-only, every subject stays `v = 1`, no reducer/fixture/type changes required; verdict carried by the subject, `inputs` recorded verbatim per the §8 stdin contract (I6). `gate.passed` baseline deliberately not ratified (no S3 emitter/pin — S4 scope). Operator-badge concept noted in the badge section as a §12/DEFAULT doc-level matter, not fabric surface. Deferred with reasons: `badge_id` as an additive field on mutation facts (S3 board ties calls to the log via the acked `correlation`; no pin, no consumer — ratifying a cross-subject attribution convention without a folding reducer risks dead-letter fields); `badge.issued` emit-or-drop (no S3 pin or emitter, and the operator badge adds a daemon-lifetime-scope question that should be settled in the same session as the `badge_id` decision). Capture-chunk subject remains tracked and flagged for `/dr` — not resolved here.
- 2026-07-17 · warden · S4 payload ratification: (1) `gate.passed` v1 ratified — now has an emitter (S4 gate engine), so the S3-deferred baseline is minted: `{run, gate, verifiers: [{verifier, cost_ms, evidence: [CasRef], inputs}]}`, per-verifier records for replay + recorded cost; documented the intentional single-failing-verifier (`gate.failed`) vs. all-verifiers (`gate.passed`) asymmetry (first-failure short-circuit vs. full-pass evidence). (2) NEW subject `diff.merged` v1 `{run, worktree, diff: CasRef}` — golden-path merge/worktree-lifecycle-close fact; full new-subject checklist passed (grammar, `diff` family coherence, git-adapter emitter, S4 reducer consumer exists), distinct from v0 `merge.completed`. (3) `agent.spawned` governed-spawn additive fields `bare?: bool`, `allowed_tools?: [string]` (and cross-reference to the pre-existing `harness_version?`) — DR-001 enforcement-decisions-recorded-in-events; additive, independent of `idempotency_key?`/`badge_id`, badge deferrals not foreclosed. All subjects stay `v = 1`; strict supersets of the S4 pins; no oracle test/fixture edits. I2 verified: inputs doc ~182 B, 3-verifier `gate.passed` ~838 B — far under 32 KiB. COLLISION reported: the new `diff.merged` table row requires a same-commit one-line addition of `"diff.merged"` to `rezidnt_types::taxonomy::SUBJECTS_V0` (after `"diff.ready"`) or `taxonomy_drift.rs` fails `/vet`; flagged for the implementer since this session was scoped to `spec/ontology.md` only (scope collision, not a pin↔ontology content collision). Open questions: (a) exec-verifier network opt-in recorded inside the verbatim `inputs.params` — DECIDED, no new field; (b) replay-divergence integrity alarm as a durable log fact — TRACKED for `/dr` (CLI-report-only per the pins; a log fact would be additive, no consumer today).
- 2026-07-17 · warden · pre-S4 remediation (S3-T1, I3): additive optional field `idempotency_key?: string` on `agent.spawned` (v stays 1) — records the caller-supplied spawn idempotency key so the key→run map rebuilds from the log after restart (behavior pinned by `mcp_workspace_recovery.rs::spawn_key_idempotency_survives_daemon_restart`; mechanism ratified here, not in the test). Named to match `SpawnAgentArgs.idempotency_key` (cross-subject convention: `OpenWorkspaceArgs` carries the same name). Optional because non-MCP spawn paths are keyless; non-empty, ≤ 256 bytes UTF-8 (DEFAULT cap); dedup scoped to (envelope `workspace`, key). Existing fixtures unchanged (field absent = valid); no reducer or type edits required. The open `badge_id`-on-mutation-facts and `badge.issued` deferrals are untouched and not foreclosed.
