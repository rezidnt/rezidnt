//! DR-044 ORACLE (§9 write-side schema no-drift, GOLDEN-PIN leg) — guard (a) of
//! the five named in DR-044 §Consequences "Test/criterion honesty". Mirrors the
//! shipped read-side pin `orchestration_schema_no_drift.rs` exactly, one class
//! up: `fan_out` is the first MUTATING tool this arc mints, so the pin also has
//! to hold the §12 badge marker and the §9 idempotency rule in place.
//!
//! FAILING-FIRST: `rezidnt_types::mcp::FanOutArgs` and `FanOutTask` DO NOT EXIST
//! yet, so this file fails to COMPILE (unresolved path). That is the sanctioned
//! oracle red for a type-level pin.
//!
//! ## Why a golden and not just a surface round-trip
//!
//! `rezidnt-mcp`'s `jsonrpc_surface.rs` asserts the SERVED `inputSchema` equals
//! `schema_for!` of the generating type. That catches surface-versus-type
//! divergence only: if BOTH drift together (a field silently added, removed, or
//! retyped) it still passes. This file pins the generated STRUCTURE against a
//! committed golden so the published tool-args schema cannot drift silently
//! (doc §9 BINDING no-drift, DR-044 §Decision 1).
//!
//! The golden pins STRUCTURE, not prose: `description` keys (the verbatim
//! doc-comments) are stripped before comparison, so rewording a comment does not
//! flip the golden red. Only a real shape change does.
//!
//! ## The shape this pins (DR-044 §Decision 1, implementer builds to EXACTLY it)
//!
//! ```ignore
//! /// `fan_out` — one call, N tasks. MUTATING: badge required (§12).
//! #[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
//! pub struct FanOutArgs {
//!     pub badge: String,
//!     pub workspace: String,
//!     pub tasks: Vec<FanOutTask>,
//! }
//!
//! #[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
//! pub struct FanOutTask {
//!     pub agent: String,
//!     pub idempotency_key: String,
//! }
//! ```
//!
//! No `lead_run` field: DR-044 §Decision 1 fixes the arg shape at
//! `{badge, workspace, tasks}`, so the lead is identified by the BADGE the door
//! verified, never self-declared by the caller. No optional fields: every field
//! is required, which is why the golden's two `required` arrays are load-bearing.
//!
//! Ontology posture: this file pins a schemars type. It reads no fixture and
//! emits no event, so it has ZERO dependence on the `worktree.allocated.allocator`
//! value vocabulary the parallel warden `/subject` session is widening.

use serde_json::{Value, json};

/// Recursively drop every `"description"` key so the golden pins STRUCTURE
/// (types, properties, required) and not the embedded doc-comment prose.
/// Identical to the read-side pin's helper — deliberately duplicated rather than
/// shared, so the two goldens cannot drift through a common helper edit.
fn strip_descriptions(mut v: Value) -> Value {
    match &mut v {
        Value::Object(map) => {
            map.remove("description");
            let stripped: serde_json::Map<String, Value> = map
                .iter()
                .map(|(k, val)| (k.clone(), strip_descriptions(val.clone())))
                .collect();
            Value::Object(stripped)
        }
        Value::Array(items) => Value::Array(
            items
                .iter()
                .map(|i| strip_descriptions(i.clone()))
                .collect(),
        ),
        other => other.clone(),
    }
}

fn golden() -> Value {
    let path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../spec/fixtures/dr044_fan_out_args.schema.golden.json"
    );
    let text = std::fs::read_to_string(path)
        .unwrap_or_else(|e| panic!("schema golden {path} must exist: {e}"));
    serde_json::from_str(&text).expect("schema golden is valid JSON")
}

fn generated() -> Value {
    serde_json::to_value(schemars::schema_for!(rezidnt_types::mcp::FanOutArgs))
        .expect("schema serializes")
}

/// CRITERION (a), DR-044 §Consequences — the schemars-generated structure of
/// `FanOutArgs` (with its nested `FanOutTask` `$def`) EQUALS the committed
/// golden. A silent drift flips this red and forces the golden to be updated
/// deliberately in the same diff: the published MCP tool-args schema is doc §9
/// BINDING no-drift.
#[test]
fn fan_out_args_schema_matches_golden() {
    assert_eq!(
        strip_descriptions(generated()),
        golden(),
        "FanOutArgs schema STRUCTURE drifted from the committed golden \
         (spec/fixtures/dr044_fan_out_args.schema.golden.json). If this change is intended, \
         update the golden in the same diff — the published MCP tool-args schema is doc §9 \
         BINDING no-drift (DR-044 §Decision 1). Structure only; prose descriptions are stripped."
    );
}

/// WRITE-class guard — `fan_out` is MUTATING, so its args schema MUST declare a
/// required `badge` property (doc §12: the capability token, checked before
/// anything else happens). Pinned independently of the golden so the intent is
/// legible in the failure output: a `badge` quietly going optional would be a
/// silent demotion to an unbadged tool.
#[test]
fn fan_out_args_schema_is_write_class() {
    let schema = generated();

    assert_eq!(
        schema["properties"]["badge"]["type"],
        json!("string"),
        "fan_out is MUTATING — its args schema must declare a `badge` string property \
         (doc §12): got {:#}",
        schema["properties"]
    );

    let required = schema["required"]
        .as_array()
        .unwrap_or_else(|| panic!("fan_out args must declare a `required` array: {schema:#}"));
    for field in ["badge", "workspace", "tasks"] {
        assert!(
            required.iter().any(|r| r == field),
            "`{field}` is REQUIRED on fan_out (DR-044 §Decision 1 fixes the shape at \
             {{badge, workspace, tasks}}): got {required:#?}"
        );
    }

    // DR-044 §Decision 1: one call, N tasks. The width cap and the single-unit
    // report are only enforceable because the tasks ride ONE call — a scalar
    // `agent`/`idempotency_key` pair here would be N separate delegates by
    // another name, the shape the record explicitly REJECTS.
    assert_eq!(
        schema["properties"]["tasks"]["type"],
        json!("array"),
        "`tasks` is an ARRAY — one call, N tasks. A per-call scalar shape cannot enforce a \
         width cap atomically and cannot report a fan-out as one unit (DR-044 §Decision 1, \
         which REJECTS N separate delegate calls): got {:#}",
        schema["properties"]["tasks"]
    );

    // The caller does not name the lead: the lead is the identity the §12 door
    // verified from the badge. A `lead_run` arg would let a caller claim a
    // parentage it does not hold.
    assert!(
        schema["properties"].get("lead_run").is_none(),
        "fan_out must NOT take a caller-declared `lead_run` — the lead is derived from the \
         badge the door verified (DR-044 §Decision 1/2b): got {:#}",
        schema["properties"]
    );
}

/// §9 IDEMPOTENCY rule, per task — DR-044 §Decision 1: "idempotency composes per
/// task, not per call". Every `FanOutTask` carries a REQUIRED `idempotency_key`,
/// resolving through the existing per-workspace `spawn_keys` map
/// (`bins/rezidentd/src/runs.rs:149`, `:229`, `:287`). Spawning is
/// non-idempotent by nature, so the key is not optional — the same discipline
/// `SpawnAgentArgs` already carries. An optional key here would silently make a
/// retry spawn a second time.
#[test]
fn every_fan_out_task_carries_a_required_idempotency_key() {
    let schema = generated();
    let task = &schema["$defs"]["FanOutTask"];

    assert!(
        !task.is_null(),
        "FanOutArgs.tasks must reference a `FanOutTask` $def: got {schema:#}"
    );

    let required = task["required"]
        .as_array()
        .unwrap_or_else(|| panic!("FanOutTask must declare a `required` array: {task:#}"));
    for field in ["agent", "idempotency_key"] {
        assert!(
            required.iter().any(|r| r == field),
            "`{field}` is REQUIRED on every FanOutTask — idempotency composes PER TASK, and \
             spawning is non-idempotent by nature (doc §9, DR-044 §Decision 1): got {required:#?}"
        );
    }

    // A task carries the spawn axes and nothing else: no badge (the call's badge
    // is the authority), and no per-task worktree/allocator hint (isolation rides
    // the existing §7 registry, DR-044 §Decision 3).
    let props = task["properties"]
        .as_object()
        .unwrap_or_else(|| panic!("FanOutTask must declare properties: {task:#}"));
    let mut names: Vec<&str> = props.keys().map(String::as_str).collect();
    names.sort_unstable();
    assert_eq!(
        names,
        vec!["agent", "idempotency_key"],
        "a FanOutTask carries exactly {{agent, idempotency_key}} — no per-task badge (the \
         call's badge is the authority, DR-044 §Decision 1) and no worktree/allocator hint \
         (isolation rides the existing §7 registry, §Decision 3): got {task:#}"
    );
}
