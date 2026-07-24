//! DR-042 ORACLE (§9 schema no-drift, GOLDEN-PIN leg) — the owed test named in
//! DR-042 §Consequences: "the eventual slice owes the §9 no-drift schema pin for
//! the new MCP tool". The MCP-surface oracle
//! (`rezidnt-mcp/tests/orchestration_graph_surface.rs`) already asserts the
//! SERVED `inputSchema` EQUALS `schema_for!(OrchestrationViewArgs)` — but that
//! catches only a surface-vs-type divergence; if BOTH the served schema and the
//! generating TYPE drift together (a field silently added/removed/retyped, or a
//! `required`/`badge` sneaking in), that test still passes. THIS file closes that
//! gap: it pins the schemars-generated STRUCTURE of `OrchestrationViewArgs`
//! against a committed golden, so the published tool-args schema cannot drift
//! silently (doc §9 BINDING no-drift, DR-042 §Decision 4).
//!
//! The golden pins STRUCTURE, not prose: `description` fields (the verbatim
//! doc-comments) are stripped before comparison, so a harmless comment reword
//! does NOT flip the golden red — only a real shape change does (a field, a type,
//! a `required`, a `badge`). That is the honest no-drift contract: the wire shape
//! is frozen, the documentation is free.
//!
//! Read-side / design-legal (DR-042 Decision 5): this pins a schema, it wires no
//! fan-out.

use serde_json::{Value, json};

/// Recursively drop every `"description"` key so the golden pins STRUCTURE
/// (types, properties, required) and not the embedded doc-comment prose. A
/// comment reword must not flip a structural no-drift pin red.
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
        "/../../spec/fixtures/dr042_orchestration_view_args.schema.golden.json"
    );
    let text = std::fs::read_to_string(path)
        .unwrap_or_else(|e| panic!("schema golden {path} must exist: {e}"));
    serde_json::from_str(&text).expect("schema golden is valid JSON")
}

/// CRITERION (DR-042 §Decision 4, doc §9 no-drift) — the schemars-generated
/// structure of `OrchestrationViewArgs` EQUALS the committed golden. A silent
/// drift (a new field, a retyped `run`, a stray `required`) flips this red and
/// forces the golden to be updated deliberately alongside the change.
#[test]
fn orchestration_view_args_schema_matches_golden() {
    let generated = serde_json::to_value(schemars::schema_for!(
        rezidnt_types::mcp::OrchestrationViewArgs
    ))
    .expect("schema serializes");
    let structural = strip_descriptions(generated);

    assert_eq!(
        structural,
        golden(),
        "OrchestrationViewArgs schema STRUCTURE drifted from the committed golden \
         (spec/fixtures/dr042_orchestration_view_args.schema.golden.json). If this change is \
         intended, update the golden in the same diff — the published MCP tool-args schema is \
         doc §9 BINDING no-drift (DR-042 §Decision 4). Structure only; prose descriptions are stripped."
    );
}

/// Read-class no-drift guard — the schema requires NOTHING (no `required` array)
/// and specifically declares NO `badge` property. `orchestration_graph` is a
/// read like `board_view`/`get_escalations`; a `required`/`badge` appearing would
/// be a silent promotion to a mutating tool. Pinned independently of the golden
/// so the intent is legible in the failure output (DR-042 §Invariant I5).
#[test]
fn orchestration_view_args_schema_is_read_class() {
    let generated = serde_json::to_value(schemars::schema_for!(
        rezidnt_types::mcp::OrchestrationViewArgs
    ))
    .expect("schema serializes");

    assert!(
        generated.get("required").is_none() || generated["required"] == json!([]),
        "orchestration_graph is READ-class — its args schema must require nothing \
         (no `required`): got {:#}",
        generated["required"]
    );
    assert!(
        generated["properties"].get("badge").is_none(),
        "orchestration_graph is READ-class (board_view/get_escalations) — its args schema must \
         NOT declare a `badge` property (that is a mutating-tool marker, doc §12): got {:#}",
        generated["properties"]
    );
    // The single field is exactly the optional `run` filter (nullable string) —
    // the shape it MUST keep to mirror GetEscalationsArgs (DR-042 §Decision 4).
    assert_eq!(
        generated["properties"]["run"]["type"],
        json!(["string", "null"]),
        "the sole arg is the optional `run` filter (nullable string): got {:#}",
        generated["properties"]["run"]
    );
}
