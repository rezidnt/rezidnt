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

/// `request_permission` — the harness PEP asks the daemon PDP "may this action
/// proceed?" and gets back a three-valued decision (`allow | deny | ask`),
/// NEVER coerced (I6, design §5).
///
/// Badge posture (design §5): read-class on the DECISION, but the result
/// authorizes a later mutation, so the caller must be identified — `badge` is
/// REQUIRED (the caller identity, carried to `permit.requested.badge_id`).
/// The bulk action context (argv, file bytes) is a `context_ref` CAS-ref
/// string (`cas:blake3:<hex>`), never inline bytes (I2).
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
    /// Optional bulk-context CAS ref (`cas:blake3:<hex>`) — the ref only, never
    /// inline bytes (I2).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub context_ref: Option<String>,
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
