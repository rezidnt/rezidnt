//! DR-042 ORACLE (§9 schema no-drift + read-class leg) — the READ-ONLY
//! `orchestration_graph` MCP tool is served, its `inputSchema` EQUALS
//! `schemars::schema_for!(OrchestrationViewArgs)` (doc §9 BINDING no-drift), and
//! it is in the `board_view` / `get_escalations` READ class (unbadged).
//!
//! FAILING-FIRST — intended reds, all "missing type/tool", not typos:
//! - `rezidnt_types::mcp::OrchestrationViewArgs` does NOT exist yet: the
//!   implementer adds it (mirroring `GetEscalationsArgs { run: Option<String> }`,
//!   the optional-run-filter shape). Until it lands this file fails to COMPILE
//!   (unresolved path). That red is the args-type work order.
//! - `orchestration_graph` is not advertised in `tools_list()` nor dispatched in
//!   `tools_call()` yet (`rezidnt-mcp/src/lib.rs`), so the surface assertions go
//!   red until the read-class tool is served.
//!
//! ## API SURFACE this board PINS (implementer builds to EXACTLY this)
//! In `crates/rezidnt-types/src/mcp.rs`, mirroring `GetEscalationsArgs`:
//! ```ignore
//! #[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
//! pub struct OrchestrationViewArgs {
//!     /// Filter to one lead's fan-out (canonical 26-char ULID text form).
//!     /// Absent = the whole fleet's orchestration graph. Additive-optional so
//!     /// `schema_for!` stays doc §9 no-drift: absent = OMITTED, never null.
//!     #[serde(default, skip_serializing_if = "Option::is_none")]
//!     pub run: Option<String>,
//! }
//! ```
//! and the read-class tool in `rezidnt-mcp/src/lib.rs`: advertised in
//! `tools_list()` with `inputSchema = schema_for!(OrchestrationViewArgs)`,
//! dispatched in `tools_call()` to a `call_orchestration_graph` handler that
//! returns `rezidnt_state::orchestration_graph(&graph)` (filtered by `run` when
//! present) — NO badge, same read-class as `board_view` / `get_escalations`.

mod util;

use serde_json::json;

/// DR-042: the READ-ONLY `orchestration_graph` tool is served in `tools/list`.
/// RED until the implementer advertises the tool (`rezidnt-mcp/src/lib.rs`
/// `tools_list()`).
#[tokio::test]
async fn tools_list_serves_orchestration_graph() {
    let (_dir, core) = util::core();
    let tools = util::list_tools(&core).await;
    util::find_tool(&tools, "orchestration_graph");
}

/// Doc §9 BINDING no-drift rule: the `orchestration_graph` `inputSchema` served
/// by `tools/list` EQUALS `schemars::schema_for!(OrchestrationViewArgs)` — the
/// surface and the published types can never drift. RED until the implementer
/// adds `rezidnt_types::mcp::OrchestrationViewArgs` and serves the tool.
#[tokio::test]
async fn orchestration_graph_schema_is_generated_from_rezidnt_types() {
    let (_dir, core) = util::core();
    let tools = util::list_tools(&core).await;
    let expected = serde_json::to_value(schemars::schema_for!(
        rezidnt_types::mcp::OrchestrationViewArgs
    ))
    .unwrap();
    let tool = util::find_tool(&tools, "orchestration_graph");
    assert_eq!(
        tool["inputSchema"], expected,
        "orchestration_graph: served inputSchema must EQUAL schemars::schema_for! of \
         OrchestrationViewArgs (no drift, doc §9)"
    );
}

/// Read-class (unbadged) shape: `orchestration_graph` is a read like `board_view`
/// / `get_escalations` — its schema does NOT require a `badge` (contrast the
/// mutating `spawn_agent` / `open_project`, which DO). The optional `run` filter
/// is the only field; the schema requires nothing.
#[tokio::test]
async fn orchestration_graph_is_read_class_no_badge() {
    let (_dir, core) = util::core();
    let tools = util::list_tools(&core).await;
    let tool = util::find_tool(&tools, "orchestration_graph");
    // A read-class tool with a single optional field has NO `required` array (or
    // an empty one) — and specifically never requires `badge`.
    let required: Vec<String> = tool["inputSchema"]["required"]
        .as_array()
        .map(|a| {
            a.iter()
                .filter_map(|v| v.as_str().map(String::from))
                .collect()
        })
        .unwrap_or_default();
    assert!(
        !required.contains(&"badge".to_string()),
        "orchestration_graph is a READ (board_view/get_escalations class) — it must NOT \
         require a badge: got required = {required:?}"
    );
}

/// The served result IS the pure projection of the folded log — no
/// re-interpretation (DR-042 §Decision 2/4, I3). Seed the committed fan-out
/// fixture, fold + project independently, and assert the served payload
/// deserializes to the identical `OrchestrationView`. RED until the projection
/// (`rezidnt_state::orchestration_graph`) and the tool both exist.
#[tokio::test]
async fn orchestration_graph_equals_pure_projection_of_fold() {
    let (_dir, core) = util::core();
    let seeded = util::seed_fixture(&core, "dr042_orchestration_fanout.jsonl");

    // The oracle's ground truth: the pure fold-then-project, computed
    // independently of the tool so equality means "the tool re-derives nothing".
    let expected = rezidnt_state::orchestration_graph(&rezidnt_state::fold(seeded.iter()));

    // Empty args = the whole fleet's orchestration graph (full fold), mirroring
    // board_view / get_escalations.
    let result = util::tool_call(&core, 1, "orchestration_graph", json!({})).await;
    assert_ne!(
        result["isError"],
        json!(true),
        "orchestration_graph is a read; it must not error: {result:#}"
    );

    let payload = util::tool_payload(&result);
    let served: rezidnt_state::OrchestrationView = serde_json::from_value(payload.clone())
        .unwrap_or_else(|e| {
            panic!("orchestration_graph payload must deserialize to an OrchestrationView ({e}): {payload:#}")
        });

    assert_eq!(
        served, expected,
        "orchestration_graph result MUST EQUAL rezidnt_state::orchestration_graph(&fold(&events)) — \
         the tool is exactly the pure projection, it re-interprets nothing (I3)"
    );

    // Non-vacuity + the run filter: the fixture folds one lead, two-wide fan-out.
    assert_eq!(served.leads.len(), 1, "the fixture folds exactly one lead");
    assert_eq!(served.leads[0].fan_out, 2, "the lead fans out to two subs");

    // The `run` filter scopes to one lead, equal to the projection filtered the
    // same way (the tool's filter IS the projection's own filter, I3). An absent
    // lead run yields an empty leads vec.
    let absent = util::tool_call(
        &core,
        2,
        "orchestration_graph",
        json!({"run": "01ABSENTLEAD0000000000RX99"}),
    )
    .await;
    assert_ne!(
        absent["isError"],
        json!(true),
        "orchestration_graph must not error for an absent-run filter: {absent:#}"
    );
    let absent_view: rezidnt_state::OrchestrationView =
        serde_json::from_value(util::tool_payload(&absent)).expect("OrchestrationView");
    assert!(
        absent_view.leads.is_empty(),
        "a run filter naming no lead returns an empty leads vec: {absent_view:#?}"
    );
}
