//! S3 oracle — the JSON-RPC/MCP surface itself (doc §9, I5).
//!
//! Transport-agnostic on purpose: these tests speak JSON-RPC values through
//! `McpCore::handle` and pin the observable messages, never SDK internals —
//! rmcp and a hand-rolled layer are equally admissible implementations.

mod util;

use serde_json::json;

/// MCP handshake: `initialize` answers with a protocol version and server
/// identity, correlated to the request id.
#[tokio::test]
async fn initialize_answers_with_protocol_and_server_info() {
    let (_dir, core) = util::core();
    let result = util::call_ok(
        &core,
        1,
        "initialize",
        json!({
            "protocolVersion": "2025-06-18",
            "capabilities": {},
            "clientInfo": {"name": "oracle", "version": "0"}
        }),
    )
    .await;
    assert!(
        result["protocolVersion"]
            .as_str()
            .is_some_and(|v| !v.is_empty()),
        "initialize result carries a protocolVersion: {result:#}"
    );
    assert!(
        result["serverInfo"]["name"]
            .as_str()
            .is_some_and(|n| n.to_ascii_lowercase().contains("rezidnt")),
        "serverInfo names rezidnt: {result:#}"
    );
}

/// The S3 tool surface: `open_project`, `spawn_agent`, `gate_explain`,
/// `tail_events` are all served (doc §9; vet/debrief arrive with S4's gate
/// engine and are NOT required here).
#[tokio::test]
async fn tools_list_serves_the_s3_surface() {
    let (_dir, core) = util::core();
    let tools = util::list_tools(&core).await;
    for name in ["open_project", "spawn_agent", "gate_explain", "tail_events"] {
        util::find_tool(&tools, name);
    }
}

/// DR-039: the READ-ONLY `board_view` tool is served in `tools/list`. It is in
/// the `tail_events` read class (unbadged, doc §12 as amended by DR-005) and
/// returns the fleet `BoardView` projection. RED until the implementer
/// advertises the tool (`rezidnt-mcp/src/lib.rs` tools_list()).
#[tokio::test]
async fn tools_list_serves_board_view() {
    let (_dir, core) = util::core();
    let tools = util::list_tools(&core).await;
    util::find_tool(&tools, "board_view");
}

/// DR-040: the READ-ONLY `get_escalations` tool is served in `tools/list`. It
/// is in the `tail_events`/`board_view` read class (unbadged, doc §12 as
/// amended by DR-005) and returns the outstanding permit escalations as
/// `Vec<EscalationRow>`. RED until the implementer advertises the tool
/// (`rezidnt-mcp/src/lib.rs` tools_list()).
#[tokio::test]
async fn tools_list_serves_get_escalations() {
    let (_dir, core) = util::core();
    let tools = util::list_tools(&core).await;
    util::find_tool(&tools, "get_escalations");
}

/// Doc §9 BINDING no-drift rule: every served inputSchema EQUALS the schema
/// generated from `rezidnt-types` via schemars — the surface and the
/// published types can never drift.
#[tokio::test]
async fn tool_schemas_are_generated_from_rezidnt_types() {
    let (_dir, core) = util::core();
    let tools = util::list_tools(&core).await;
    let expected = [
        (
            "open_project",
            serde_json::to_value(schemars::schema_for!(rezidnt_types::mcp::OpenProjectArgs))
                .unwrap(),
        ),
        (
            "spawn_agent",
            serde_json::to_value(schemars::schema_for!(rezidnt_types::mcp::SpawnAgentArgs))
                .unwrap(),
        ),
        (
            "gate_explain",
            serde_json::to_value(schemars::schema_for!(rezidnt_types::mcp::GateExplainArgs))
                .unwrap(),
        ),
        (
            "tail_events",
            serde_json::to_value(schemars::schema_for!(rezidnt_types::mcp::TailEventsArgs))
                .unwrap(),
        ),
        // DR-039: the READ-ONLY board_view tool. Its empty `BoardViewArgs {}`
        // snapshot arg mirrors `TailEventsArgs`' arg-struct pattern; the served
        // inputSchema MUST equal the schemars-generated one (no drift, §9
        // BINDING). RED until the implementer adds
        // `rezidnt_types::mcp::BoardViewArgs` and advertises the tool.
        (
            "board_view",
            serde_json::to_value(schemars::schema_for!(rezidnt_types::mcp::BoardViewArgs)).unwrap(),
        ),
        // DR-040: the READ-ONLY get_escalations tool. Its
        // `GetEscalationsArgs { run: Option<String> }` snapshot arg mirrors the
        // arg-struct pattern of `TailEventsArgs`/`BoardViewArgs`; the served
        // inputSchema MUST equal the schemars-generated one (no drift, §9
        // BINDING). RED until the implementer adds
        // `rezidnt_types::mcp::GetEscalationsArgs` and advertises the tool.
        (
            "get_escalations",
            serde_json::to_value(schemars::schema_for!(
                rezidnt_types::mcp::GetEscalationsArgs
            ))
            .unwrap(),
        ),
    ];
    for (name, schema) in expected {
        let tool = util::find_tool(&tools, name);
        assert_eq!(
            tool["inputSchema"], schema,
            "{name}: served inputSchema must EQUAL schemars::schema_for! of the rezidnt-types shape (no drift, doc §9)"
        );
    }
}

/// Mutating tools carry the badge/idempotency contract in their SCHEMAS
/// (doc §12, §9): `badge` required on both; `spawn_agent` (non-idempotent)
/// additionally requires `idempotency_key`. Survives independently of the
/// full-equality test above.
#[tokio::test]
async fn mutating_tool_schemas_require_badge_and_idempotency_key() {
    let (_dir, core) = util::core();
    let tools = util::list_tools(&core).await;
    let required = |name: &str| -> Vec<String> {
        let tool = util::find_tool(&tools, name);
        tool["inputSchema"]["required"]
            .as_array()
            .unwrap_or_else(|| panic!("{name}: inputSchema.required must exist"))
            .iter()
            .filter_map(|v| v.as_str().map(String::from))
            .collect()
    };
    for name in ["open_project", "spawn_agent"] {
        assert!(
            required(name).contains(&"badge".to_string()),
            "{name}: mutating tool schema must REQUIRE badge (doc §12)"
        );
    }
    assert!(
        required("spawn_agent").contains(&"idempotency_key".to_string()),
        "spawn_agent is not idempotent by nature, so its key is required (doc §9)"
    );
}

/// Garbage in, spec error out: an unknown method gets JSON-RPC -32601, not a
/// hang, panic, or disconnect.
#[tokio::test]
async fn unknown_method_gets_method_not_found() {
    let (_dir, core) = util::core();
    let response = core
        .handle(util::rpc(7, "tools/definitely_not_a_method", json!({})))
        .await
        .expect("requests always get a response");
    assert_eq!(response["id"], json!(7));
    assert_eq!(
        response["error"]["code"],
        json!(-32601),
        "unknown method must answer with JSON-RPC method-not-found: {response:#}"
    );
}
