//! DR-057 ORACLE — the `diff_view`/`cas_read` §9 surface: served, no-drift,
//! and shaped as the record ruled (DR-057 §Decision 1/2/3/4).
//!
//! Three-way no-drift, the `jsonrpc_surface.rs`/DR-039 pattern hardened with
//! committed goldens (the DR-042/DR-044 `.schema.golden.json` pattern):
//! served `inputSchema` == `schemars::schema_for!` of the `rezidnt-types`
//! shape == the committed golden. The golden leg matters here because
//! generated==served alone is self-referential — it passes whatever shape the
//! implementer types; the committed golden pins the shape the RECORD ruled
//! (worktree-keyed args; a FULL `CasRef`, never a bare hash).
//!
//! Shape pins carried by the schemas themselves:
//!
//! - `diff_view` args require `worktree` and carry NO `run` property — the
//!   tool is keyed by worktree, never by run (DR-057 §Decision 3: `RunRow`
//!   holds no worktree reference and the ordinary allocator is the bare
//!   string `"rezidnt"`, so a run key has nothing sound to join on; DR-049
//!   ruled the correlation join UNSOUND).
//! - `cas_read` args are the full `{hash, bytes, mime}` triple, ALL required
//!   — the caller's own ref, echoed and verified; never a bare hash the
//!   daemon would have to invent metadata for (mime lives only in event
//!   payloads; the CAS at rest is content-only, `crates/rezidnt-cas`).
//! - `diff_view` carries NO `badge` property: read-class, unbadged (DR-057
//!   §Decision 4 as amended — the leg DR-058 leaves UNCHANGED).
//! - `cas_read` REQUIRES a `badge` property, declared FIRST like every other
//!   badged tool on this surface. RE-CUT under the owner's in-place DR-058
//!   correction (`81f437c`, 2026-07-26): the record's "shape is unchanged"
//!   clause was struck — a door invisible to the schema is one clients learn
//!   about by being refused (I5), so `CasReadArgs` gains `pub badge: String`
//!   on the house pattern (`OpenProjectArgs`..`FanOutArgs`). The golden
//!   states this RULED target, so the golden leg is RED until the field
//!   lands — oracle-first, the DR-045 re-cut precedent.
//!
//! ## AUDITOR-DIRECTED CORRECTION (DR-057 debrief, finding F2)
//!
//! The golden leg of this board was originally compared VERBATIM, which made a
//! doc-comment reword flip a structural golden red. The implementer resolved
//! that by stripping the prose out of the PRODUCT
//! (`#[schemars(description = "")]` on `DiffViewArgs`/`CasReadArgs`) — the wrong
//! direction: the golden ended up dictating the wire format, and `diff_view`/
//! `cas_read` served field schemas barer than every other tool's.
//!
//! Corrected here the way the house already solved it twice
//! (`rezidnt-types/tests/fanout_schema_no_drift.rs:57`,
//! `orchestration_schema_no_drift.rs:27`): the TEST normalizes. The golden leg
//! strips `description` keys, so it pins STRUCTURE — types, properties,
//! `required` — and a reword moves nothing. NOTHING IS SOFTENED: all three legs
//! still hold (served == `schema_for!` == golden), just modulo prose, and the
//! served leg below is compared VERBATIM because both its sides come from the
//! same `schema_for!` and prose cannot skew it. A fourth assertion, which did
//! not exist before, now pins that the served schema CARRIES the prose — so
//! re-suppressing descriptions goes red instead of passing quietly.
//!
//! ## RED MODE (against the tree at cut time — post-`1094f40`)
//!
//! COMPILE-RED: `rezidnt_types::mcp::DiffViewArgs` and
//! `rezidnt_types::mcp::CasReadArgs` do not exist (verified by grep this
//! session). Once they exist, ASSERT-RED until `tools_list()` advertises both
//! tools with the generated schemas. The goldens were generated with the
//! workspace-locked schemars 1.2.1 emission (probed this session), so the
//! generated==golden leg goes green the moment the types match the ruled
//! shape — it is the served leg that stays red until the surface serves them.

mod util;

use serde_json::{Value, json};

fn golden(name: &str) -> Value {
    let path = util::fixtures_dir().join(name);
    let text = std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("golden {} must exist: {e}", path.display()));
    serde_json::from_str(&text).unwrap_or_else(|e| panic!("{name}: golden must parse: {e}"))
}

/// Recursively drop every `"description"` key so the golden pins STRUCTURE
/// (types, properties, required) and not the embedded doc-comment prose.
/// Identical to `fanout_schema_no_drift.rs`'s and
/// `orchestration_schema_no_drift.rs`'s helpers — deliberately duplicated rather
/// than shared, so the goldens cannot drift together through one helper edit.
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

/// Both DR-057 read tools are served in `tools/list`.
#[tokio::test]
async fn tools_list_serves_diff_view_and_cas_read() {
    let (_dir, core) = util::core();
    let tools = util::list_tools(&core).await;
    util::find_tool(&tools, "diff_view");
    util::find_tool(&tools, "cas_read");
}

/// §9 BINDING no-drift, three ways: generated == committed golden (the shape
/// the record ruled) and served == generated (the surface cannot drift from
/// the published types).
///
/// The GOLDEN leg compares modulo `description` prose (see the module header):
/// the golden pins STRUCTURE, so rewording a doc-comment is free and only a
/// real shape change flips it. The SERVED leg is compared VERBATIM — both its
/// sides come from `schema_for!`, so prose cannot skew it and a verbatim
/// comparison there is strictly the stronger claim.
#[tokio::test]
async fn dr057_schemas_match_types_and_goldens() {
    let (_dir, core) = util::core();
    let tools = util::list_tools(&core).await;

    let cases = [
        (
            "diff_view",
            serde_json::to_value(schemars::schema_for!(rezidnt_types::mcp::DiffViewArgs))
                .expect("DiffViewArgs schema serializes"),
            golden("dr057_diff_view_args.schema.golden.json"),
        ),
        (
            "cas_read",
            serde_json::to_value(schemars::schema_for!(rezidnt_types::mcp::CasReadArgs))
                .expect("CasReadArgs schema serializes"),
            golden("dr057_cas_read_args.schema.golden.json"),
        ),
    ];

    for (name, generated, committed) in cases {
        assert_eq!(
            strip_descriptions(generated.clone()),
            strip_descriptions(committed),
            "{name}: the STRUCTURE schemars::schema_for! generates from the \
             rezidnt-types shape must EQUAL the committed golden — the golden \
             pins the shape DR-057 ruled, not whatever shape got typed. \
             Structure only; description prose is stripped from BOTH sides so \
             the golden can never dictate the wire format's prose"
        );
        let tool = util::find_tool(&tools, name);
        assert_eq!(
            tool["inputSchema"], generated,
            "{name}: served inputSchema must EQUAL the generated schema \
             VERBATIM, prose included (no drift, doc §9 BINDING)"
        );
    }
}

/// PRODUCT LEG (auditor finding F2) — the served arg schemas CARRY field
/// descriptions, like every other tool on this surface. Without this, the
/// `#[schemars(description = "")]` suppression that stripped prose to keep a
/// verbatim golden quiet would pass every assertion above; with it, a
/// re-suppression is red. Prose CONTENT is deliberately unpinned — only its
/// presence — so a reword stays free.
#[tokio::test]
async fn dr057_served_schemas_carry_field_descriptions() {
    let (_dir, core) = util::core();
    let tools = util::list_tools(&core).await;

    for (name, fields) in [
        ("diff_view", &["worktree"][..]),
        // `badge` per the DR-058 in-place correction (`81f437c`): a served
        // field carries prose like every other badged tool's badge does.
        ("cas_read", &["badge", "hash", "bytes", "mime"][..]),
    ] {
        let schema = &util::find_tool(&tools, name)["inputSchema"];
        for field in fields {
            let described = schema["properties"][field]["description"]
                .as_str()
                .unwrap_or_default();
            assert!(
                !described.trim().is_empty(),
                "{name}.{field} must serve a non-empty description — a client \
                 reading the schema gets the same field guidance every other \
                 tool offers. Suppressing prose to keep a golden verbatim \
                 inverts test and product (F2): {schema:#}"
            );
        }
    }
}

/// `diff_view` is worktree-keyed and ONLY worktree-keyed: `worktree` required,
/// no `run` property, no `badge` property (read-class). A run-keyed alias
/// property appearing here is the drift DR-057 §Decision 3 forbids.
#[tokio::test]
async fn diff_view_schema_is_worktree_keyed_and_unbadged() {
    let (_dir, core) = util::core();
    let tools = util::list_tools(&core).await;
    let schema = &util::find_tool(&tools, "diff_view")["inputSchema"];

    assert_eq!(
        schema["required"],
        json!(["worktree"]),
        "diff_view requires exactly the worktree key: {schema:#}"
    );
    let props = schema["properties"]
        .as_object()
        .unwrap_or_else(|| panic!("diff_view schema has properties: {schema:#}"));
    assert!(
        !props.contains_key("run"),
        "diff_view must NOT offer a run key — a run has nothing sound to join \
         on (DR-057 §Decision 3, DR-049 precedent): {schema:#}"
    );
    assert!(
        !props.contains_key("badge"),
        "diff_view is read-class, unbadged (DR-057 §Decision 4): {schema:#}"
    );
}

/// `cas_read` takes the FULL ref — hash AND bytes AND mime, all required —
/// AND a required `badge`, declared first (the house pattern every badged
/// tool follows). A bare-hash schema would force the daemon to invent
/// metadata the CAS never persists at rest (DR-057 §Decision 2); a badge the
/// schema does not declare would be a door clients only discover by being
/// refused (I5 — the DR-058 in-place correction `81f437c` that re-cut this
/// test's original no-badge pin).
#[tokio::test]
async fn cas_read_schema_requires_the_full_ref_and_its_badge() {
    let (_dir, core) = util::core();
    let tools = util::list_tools(&core).await;
    let schema = &util::find_tool(&tools, "cas_read")["inputSchema"];

    let required: Vec<&str> = schema["required"]
        .as_array()
        .unwrap_or_else(|| panic!("cas_read schema has required: {schema:#}"))
        .iter()
        .filter_map(Value::as_str)
        .collect();
    for field in ["badge", "hash", "bytes", "mime"] {
        assert!(
            required.contains(&field),
            "cas_read must REQUIRE {field} — the full ref (DR-057 §Decision 2) \
             plus the badge the DR-058 door demands, DECLARED so a schema-only \
             client can discover it (`81f437c`): {schema:#}"
        );
    }
    let props = schema["properties"]
        .as_object()
        .unwrap_or_else(|| panic!("cas_read schema has properties: {schema:#}"));
    assert!(
        props.contains_key("badge"),
        "cas_read's badge is a DECLARED property, not door-level folklore — \
         the house pattern every badged args struct follows: {schema:#}"
    );
}
