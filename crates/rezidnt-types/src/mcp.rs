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
