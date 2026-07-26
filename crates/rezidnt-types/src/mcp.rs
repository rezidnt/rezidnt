//! MCP tool-argument shapes (doc §9, BINDING no-drift rule).
//!
//! Every MCP tool's input JSON Schema is GENERATED from these types via
//! `schemars` — the served surface and the published types can never drift.
//! The S3 oracle pins this with a round-trip assertion in
//! `rezidnt-mcp/tests/jsonrpc_surface.rs`: the `inputSchema` served by
//! `tools/list` must equal `schemars::schema_for!` of the matching type here.
//!
//! Badge rule (doc §12): every MUTATING tool carries a required `badge`
//! field — the capability token, checked before anything else happens.
//! Idempotency rule (doc §9): every tool is idempotent or carries an
//! idempotency key; `spawn_agent` (non-idempotent by nature) REQUIRES one.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// `open_project` — materialize a workspace from a §13 project spec.
/// Mutating: requires a badge. Idempotency: an optional key; two calls with
/// the same key must not materialize twice.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct OpenProjectArgs {
    /// Capability badge token (hex), doc §12. Checked before the spec is
    /// even parsed.
    pub badge: String,
    /// The §13 project spec, TOML text.
    pub spec_toml: String,
    /// Optional idempotency key: same key, same materialization.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub idempotency_key: Option<String>,
}

/// `spawn_agent` — spawn one spec agent in an open workspace.
/// Mutating: requires a badge AND an idempotency key (spawning twice is an
/// observable difference, so the key is not optional).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct SpawnAgentArgs {
    /// Capability badge token (hex), doc §12.
    pub badge: String,
    /// Workspace ULID (canonical 26-char text form).
    pub workspace: String,
    /// Spec agent name (the `[[agent]]` entry to spawn).
    pub agent: String,
    /// Required idempotency key: a retried call with the same key returns
    /// the SAME run and spawns nothing new.
    pub idempotency_key: String,
}

/// `gate_explain` — interrogability (I6, doc §8): the failing verifier, its
/// evidence refs, and the exact inputs. Read-only, idempotent, no badge.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct GateExplainArgs {
    /// Run ULID (canonical 26-char text form) to explain.
    pub run: String,
}

/// `kill_run` — DR-032 §Decision 1: the OPERATOR-ONLY mutating tool that
/// terminates a run. Mutating: requires an operator badge (doc §12), checked
/// before any side effect; the agent-macaroon path is rejected on policy.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct KillRunArgs {
    /// Operator badge token (hex), doc §12 / DR-032 §1. The operator identity
    /// checked before the run is touched; never logged (the verified id is,
    /// not the token, §12/I2).
    pub badge: String,
    /// Run ULID (canonical 26-char text form) to terminate.
    pub run: String,
    /// Optional operator-supplied reason: rides the emitted `agent.signaled`
    /// fact when present (I6 interrogability), omitted when the caller gave
    /// none — never synthesized.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
}

/// `release_worktree` — DR-049 §Decision 3: the EXPLICIT release that closes a
/// retained worktree. A failed run's tree survives for triage until someone
/// acts (v1 is explicit-only: no TTL, no timer, no auto-reap), and this is the
/// door they act through — MCP-first (I5) on the write-capable operator
/// surface, never on the read-only board (DR-031).
///
/// Mutating, so a badge is required (doc §12), checked before any side effect.
/// The door is the DUAL path (`check_badge`), not `kill_run`'s operator-only
/// one, because DR-049 §Decision 3 says "operator **or** lead": a lead run that
/// allocated trees is entitled to close one, and narrowing the record to
/// operator-only would be a decision this slice has no authority to make.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct ReleaseWorktreeArgs {
    /// Operator badge token or agent macaroon (doc §12). Never logged — the
    /// verified id is loggable, the token never (§12/I2).
    pub badge: String,
    /// The worktree's path, spelled EXACTLY as the allocation minted it — the
    /// canonicalized string that is already the identity every consumer keys
    /// on: the fold's `worktrees` map key, the sole-allocator registry line,
    /// and the `worktree.released` v1 payload. A caller reads it off
    /// `worktree.allocated` / `board_view`, never types it.
    pub path: String,
}

/// `resolve_permit` — DR-033 §Decision 1 (slice 2): the OPERATOR-ONLY mutating
/// tool by which a human resolves a previously-escalated permit. Mutating:
/// requires an operator badge (doc §12 / DR-033 §Design), checked before any
/// side effect; the agent-macaroon path is rejected on policy (resolving is an
/// operator action, not agent self-action — mirrors `kill_run`, DR-032 §1). On
/// admit the daemon emits ONE `permit.resolved` fact the PDP later APPLIES on the
/// agent's next ask for the same action `(run, tool, action/target)`.
///
/// The operator supplies NO `action` and NO `target` — the DAEMON DERIVES them
/// from the log by `request_id` (DR-033 §Design, /debrief FAIL close): a
/// hardcoded operator `target` broke the PDP action-identity match. The trimmed
/// shape is `{ badge, run, request_id, decision, reason? }`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct ResolvePermitArgs {
    /// Operator badge token (hex), doc §12 / DR-033 §Design. The operator
    /// identity checked before any fact is emitted; never logged (the verified id
    /// rides `permit.resolved.operator_badge_id`, not the token, §12/I2).
    pub badge: String,
    /// Run ULID (canonical 26-char text form) the escalated permit belongs to —
    /// half the `(run, tool, action/target)` match key the PDP applies on. The
    /// run the daemon folds to DERIVE `(action, target)` by `request_id`.
    pub run: String,
    /// The ESCALATED ask's `request_id` — the audit correlation (which escalation
    /// this resolution answers) AND the lookup key the daemon derives
    /// `(action, target)` from. Rides the fact and, once applied, the outcome's
    /// `resolved_from` (NOT the match key: `request_id` is re-minted per ask,
    /// DR-033 §Context).
    pub request_id: String,
    /// The human's binding choice, the override the PDP applies: the INPUT VERB
    /// `"allow"` | `"deny"` (never `granted`/`denied` — that is the PDP outcome
    /// subject, DR-033 §Decision; the CLI edge enforces the closed two-value set).
    pub decision: String,
    /// Optional operator-supplied reason: rides the emitted `permit.resolved` fact
    /// when present (I6 interrogability), omitted when the caller gave none —
    /// never synthesized.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    /// DR-035 §Decision 1 — optional TTL, a millisecond DURATION relative to the
    /// resolution's OWN envelope-ULID timestamp. When present, the PDP applies this
    /// resolution only while an incoming request's envelope timestamp is at or
    /// before `resolution_envelope_ms + ttl_ms`; past that the request re-escalates
    /// (log-derived expiry, no decision-time wall-clock — I3). ABSENT = permanent
    /// (DR-033 §Decision 2, today's behavior). Additive-optional so `schema_for!`
    /// stays doc §9 no-drift: absent = OMITTED, never null.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ttl_ms: Option<u64>,
    /// DR-035 §Decision 2 — optional grant-all scope: a single-axis wildcard that
    /// widens the match from the exact `(run, tool, action/target)` to a class.
    /// The only value in v1 is `"run_tool"` = "any action on this `(run, tool)`".
    /// ABSENT = today's DR-033 exact request-scoped match. A closed named-axis
    /// enum, NOT a boolean and NOT an expression string (DR-035 §Decision 2
    /// rejected an unrestricted predicate language): the value token IS the
    /// predicate, so `gate why`/`debrief` render it verbatim (I6). COUPLING
    /// (DR-035 §Decision 3): when `scope` is present, `ttl_ms` MUST also be
    /// present (broad OR permanent, never both) — enforced at the `resolve_permit`
    /// tool boundary before any fact is emitted, NOT in this schema. Additive-
    /// optional so `schema_for!` stays doc §9 no-drift: absent = OMITTED, never null.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scope: Option<String>,
}

/// `request_permission` — the harness PEP asks the daemon PDP "may this action
/// proceed?" and gets back a three-valued decision (`allow | deny | ask`),
/// NEVER coerced (I6, design §5).
///
/// Badge posture (design §5): read-class on the DECISION, but the result
/// authorizes a later mutation, so the caller must be identified — `badge` is
/// REQUIRED (the caller identity, carried to `permit.requested.badge_id`).
/// The bulk action context (argv, file bytes) is a `context_ref` CAS-ref
/// string (`cas:blake3:<hex>`), never inline bytes (I2).
///
/// The adapter also reads an optional `request_id` (the PEP's correlation
/// token; MCP mints one when absent, DR-013) and an optional `paths` axis (the
/// input the `path-scope` verifiers read). Both are OPTIONAL and declared here
/// so the served inputSchema matches exactly what `call_request_permission`
/// consumes — no doc §9 drift.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct RequestPermissionArgs {
    /// Capability badge token (hex), design §5. The caller identity checked
    /// before any decision is made.
    pub badge: String,
    /// Run ULID (canonical 26-char text form) the action belongs to.
    pub run: String,
    /// The action verb (e.g. `tool.invoke`).
    pub action: String,
    /// The small inline action descriptor (the tool name).
    pub tool: String,
    /// Optional caller-supplied correlation token (the PEP's request id).
    /// Absent = the daemon MINTS one (DR-013 decision 1); when present it is
    /// echoed onto the on-log decision fact so the caller's ask and the fact
    /// share one id.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub request_id: Option<String>,
    /// Optional path axis the `path-scope` verifiers read over MCP (parity with
    /// the socket transport). Absent = no path constraint is evaluated.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub paths: Option<serde_json::Value>,
    /// Optional bulk-context CAS ref (`cas:blake3:<hex>`) — the ref only, never
    /// inline bytes (I2).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub context_ref: Option<String>,
}

/// `board_view` — DR-039: read the derived fleet `BoardView` projection (the
/// whole-log fold, projected). Read-only, idempotent, no badge — in the
/// `tail_events` read class (doc §12 as amended by DR-005). The empty snapshot
/// arg (full fold) mirrors `TailEventsArgs`' arg-struct pattern; the served
/// `inputSchema` MUST equal `schema_for!` of this shape (doc §9 no-drift).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct BoardViewArgs {}

/// `get_escalations` — DR-040: read the outstanding permit escalations as
/// `Vec<EscalationRow>` (the drill-down detail behind `board_view`'s
/// `permit_escalated` count). Read-only, idempotent, no badge — in the
/// `tail_events`/`board_view` read class (doc §12 as amended by DR-005). The
/// optional `run` filters to one run (all runs when absent), mirroring the
/// optional-arg pattern of `TailEventsArgs`; the served `inputSchema` MUST
/// equal `schema_for!` of this shape (doc §9 no-drift).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct GetEscalationsArgs {
    /// Filter to one run's escalations (canonical 26-char ULID text form).
    /// Absent = all outstanding escalations across the fleet. Additive-optional
    /// so `schema_for!` stays doc §9 no-drift: absent = OMITTED, never null.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub run: Option<String>,
}

/// `orchestration_graph` — DR-042: read the fleet's lead → parallel sub-runs
/// orchestration graph as an `OrchestrationView` (each lead's DERIVED fan-out
/// over its delegated subs). Read-only, idempotent, no badge — in the
/// `board_view`/`get_escalations` read class (doc §12 as amended by DR-005).
/// The optional `run` filters to one lead (whole fleet when absent), mirroring
/// `GetEscalationsArgs`' optional-run-filter shape; the served `inputSchema`
/// MUST equal `schema_for!` of this shape (doc §9 no-drift).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct OrchestrationViewArgs {
    /// Filter to one lead's fan-out (canonical 26-char ULID text form).
    /// Absent = the whole fleet's orchestration graph. Additive-optional so
    /// `schema_for!` stays doc §9 no-drift: absent = OMITTED, never null.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub run: Option<String>,
}

/// `fan_out` — DR-044 §Decision 1: ONE call, N tasks. The first MUTATING tool of
/// the orchestrator arc: a lead delegates N sub-runs in a single unit, so it
/// requires a badge (doc §12), checked before any allocation or spawn. N separate
/// `delegate` calls are REJECTED by the record — a per-call shape can neither
/// enforce the width cap atomically nor report a fan-out as one unit.
///
/// The caller does NOT name the lead: there is no `lead_run` field, because the
/// lead is the identity the §12 door verified from `badge` (DR-044 §Decision
/// 1/2b) — a caller-declared parentage would let a run claim authority it does
/// not hold. Authorization reuses the EXISTING `"spawn"` verb; no new verb and no
/// new badge kind.
///
/// Idempotency (doc §9) composes PER TASK, not per call: each [`FanOutTask`]
/// carries a required key resolving through the existing per-workspace
/// `spawn_keys` map. Partial failure is normal and honest — the response is a
/// per-task outcome vector and a retry with the same keys re-returns the same
/// runs, spawning nothing new. There is no all-or-nothing rollback; spawns are
/// not transactional.
///
/// I2: this shape carries run/task identifiers only. No sub diff, dossier, or
/// transcript ever rides a fan-out call or its response — sub work folds back
/// through the existing per-run CAS-ref paths (DR-044 §Decision 5).
///
/// Field order is load-bearing for the doc §9 BINDING no-drift pin: it fixes the
/// generated `required` array against `spec/fixtures/dr044_fan_out_args.schema.golden.json`
/// (`crates/rezidnt-types/tests/fanout_schema_no_drift.rs`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct FanOutArgs {
    /// Capability badge token (hex), doc §12. The LEAD's own badge — it both
    /// authorizes the call (existing `"spawn"` verb) and IDENTIFIES the lead the
    /// emitted edges are keyed on. Checked before any worktree is allocated or
    /// any agent spawned; never logged (the verified id is, not the token, I2).
    pub badge: String,
    /// Workspace ULID (canonical 26-char text form). One workspace per call —
    /// cross-workspace fan-out is deferred by name (DR-044 §Consequences (d)).
    pub workspace: String,
    /// The tasks to fan out, one sub-run each. BOTH ends of the length are
    /// refused at the tool boundary, after the badge door and before any effect
    /// — the schema cannot express either bound, so the door enforces them:
    /// an EMPTY list is refused `ARGS_INVALID` (a zero-task call is a caller bug
    /// and is answered loudly, never with an empty outcome vector that a broken
    /// caller could read as a fan-out that happened), and a list wider than the
    /// `[orchestrator] max_fan_out` DEFAULT is refused WHOLE with
    /// `FAN_OUT_TOO_WIDE` (DR-044 §Decision 4 — the cap is this slice's only
    /// backpressure, `rezidnt-supervise` does not exist).
    pub tasks: Vec<FanOutTask>,
}

/// `fan_out` — DR-044 §Decision 1: one delegated sub-run's task. Carries exactly
/// the two spawn axes and nothing else: NO per-task badge (the call's `badge` is
/// the sole authority) and NO worktree or allocator hint — isolation rides the
/// existing §7 sole-allocator registry, which the daemon drives (DR-044
/// §Decision 3). A worktree-conflicted task mints NO run and is reported REFUSED,
/// never as a passed, failed, or inconclusive sub (I6).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct FanOutTask {
    /// Spec agent name (the `[[agent]]` entry to spawn as this sub).
    pub agent: String,
    /// REQUIRED idempotency key, per task (doc §9, DR-044 §Decision 1). Spawning
    /// is non-idempotent by nature, so this is not optional — the same discipline
    /// [`SpawnAgentArgs`] already carries. Resolves through the EXISTING
    /// per-workspace `spawn_keys` map (log-derived from
    /// `agent.spawned.idempotency_key`, I3): a retried task with the same key
    /// returns the SAME run and spawns nothing new. No new dedup mechanism.
    pub idempotency_key: String,
}

/// `diff_view` — DR-057 §Decision 1/3: read one worktree's Review row
/// (`{worktree, lifecycle, outcome, diff: CasRef | null}`). Read-only,
/// idempotent, no badge — in the `board_view`/`get_escalations` read class (doc
/// §12 as amended by DR-005).
///
/// KEYED BY WORKTREE, and only by worktree (DR-057 §Decision 3): there is
/// deliberately NO `run` field. `RunRow` carries no worktree reference and the
/// ordinary allocator is the bare string `"rezidnt"`, so a run key has nothing
/// sound to join on — and DR-049 already ruled the obvious alternative (a
/// correlation join) UNSOUND, one correlation spanning N runs and N trees.
/// Adding a `run` property here would put that unsound join back on the surface.
///
/// The value is the graph's own worktree key — the canonicalized path string
/// `board_view`'s `WorktreeRow.path` serves and `gate.failed.worktree?` attributes
/// against. A caller passes back the key it was served; the tool re-canonicalizes
/// nothing (I3).
///
/// SCHEMA PROSE rides the wire, like every other tool on this surface. An
/// earlier revision carried `#[schemars(description = "")]` here to keep these
/// doc-comments OUT of the served schema, so that a DR-057 golden comparing
/// `schema_for!` VERBATIM could not be reddened by a reword. That inverted the
/// order: the test's convenience dictated the product's wire format, and clients
/// lost the field guidance every other tool serves. The house fix — already
/// shipped twice, in `tests/fanout_schema_no_drift.rs` and
/// `tests/orchestration_schema_no_drift.rs` — normalizes the TEST: the golden
/// leg strips `description` keys before comparing, so it pins STRUCTURE and a
/// reword moves nothing. Prose is a product feature here, not a test liability.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct DiffViewArgs {
    /// The worktree path key (graph `worktrees` key / `WorktreeRow.path`).
    pub worktree: String,
}

/// `cas_read` — DR-057 §Decision 2: resolve ONE `CasRef` to its text content,
/// bounded. Read-only and idempotent, but BADGED (DR-058 §Decision 2): this is
/// the one tool in the read family that hands back blob CONTENT rather than
/// structural facts, and content is what the doc §12 door gates everywhere else
/// on this surface. `diff_view`/`board_view`/`tail_events`/`get_escalations`
/// are UNCHANGED and stay unbadged.
///
/// The badge is a DECLARED field, not door-level folklore, because the §9
/// schema is the contract (I5): a door invisible to the schema is one a
/// schema-only client discovers only by being refused.
///
/// The args ARE the ref triple, all three required: the caller presents its own
/// ref (the value `diff_view` served, verbatim) and the daemon echoes and
/// VERIFIES it. Not a bare hash, because the CAS at rest is content-only — mime
/// lives on the event payload, never in the store — so a bare-hash tool would
/// force the daemon to invent metadata it does not have (DR-057 §Decision 2).
///
/// What the daemon actually trusts, stated because the shape does not say it:
/// the HASH addresses and verifies the content, so it is authoritative. The
/// `bytes` and `mime` fields are the CALLER'S CLAIM. `mime` gates admission
/// (v1 serves text only) and `bytes` is not trusted at all — the read bound is
/// enforced against the ACTUAL blob, and the response's `bytes_returned` reports
/// what was actually served. See `rezidnt_mcp::MAX_CAS_READ_BYTES_DEFAULT`.
///
/// Field ORDER is load-bearing for the doc §9 no-drift pin: it fixes the
/// generated `required` array against
/// `spec/fixtures/dr057_cas_read_args.schema.golden.json`. Schema prose rides
/// the wire for the reason given on [`DiffViewArgs`].
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct CasReadArgs {
    /// Operator badge token (hex) or agent macaroon, doc §12 / DR-058
    /// §Decision 2. Checked BEFORE the mime check and before any filesystem
    /// call, so a refused caller learns nothing about the store. Never logged —
    /// the verified id is loggable, the token never (§12/I2).
    pub badge: String,
    /// Lowercase blake3 hex, exactly 64 characters — the blob's address, and the
    /// only field the daemon can verify content against. ENFORCED, not merely
    /// documented: anything that is not 64 lowercase hex characters is refused
    /// `cas.hash_invalid` on shape alone, before any lookup, so the refusal
    /// reports nothing about this daemon's store.
    pub hash: String,
    /// The caller's claimed blob length. Advisory: never used to authorize or
    /// to bound the read (a claim that could widen the bound would be a
    /// smuggling channel; one that could narrow it would deny an in-bound read
    /// over the caller's own bad metadata). An OVER-claim is served the actual
    /// blob, with `bytes_returned` reporting what was really served.
    pub bytes: u64,
    /// The caller's claimed media type, from the event payload the ref rode on.
    /// v1 admits `text/*` only — `application/json` is NOT text here — and a
    /// non-text claim is refused rather than mangled. Mime PARAMETERS are
    /// ignored, so `text/plain; charset=utf-8` is text.
    pub mime: String,
}

/// `tail_events` — read a range of event envelopes from the log.
/// Read-only, idempotent, no badge.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct TailEventsArgs {
    /// Exclusive lower bound: return events with id strictly after this
    /// ULID. Absent = from the start of the log.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub since: Option<String>,
    /// Maximum number of envelopes to return. Absent = server default.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub limit: Option<u64>,
}
