//! SP1 oracle — the `request_permission` MCP tool (design
//! `docs/design/permit-engine.md` §5; I5 MCP-first; DR-008/DR-009). The harness
//! PEP asks the daemon PDP "may this action proceed?" and gets back a
//! three-valued decision (`allow | deny | ask`) — NEVER coerced (I6).
//!
//! RED MODE: two layers.
//!   - **compile-red**: the schema round-trip references
//!     `rezidnt_types::mcp::RequestPermissionArgs`, the schemars arg type the
//!     implementer adds; the crate fails to compile until it lands.
//!   - **assert-red**: `tools/list` does not yet serve `request_permission`,
//!     and `tools/call` does not yet dispatch it, so the surface tests fail
//!     until the tool joins `tools_list()` / `tools_call`.
//!
//! Badge posture (design §5, pinned here): `request_permission` is READ-CLASS
//! on the decision, but its RESULT authorizes a later mutation, so the caller
//! must be identified — the schema REQUIRES `badge` (the caller's identity,
//! carried to `permit.requested.badge_id`). The decision itself is honest
//! three-valued and is never a mutation side effect on the daemon.

mod util;

use serde_json::json;

/// The tool joins the served surface alongside the S3 four (doc §9, I5).
///
/// ASSERT-RED until `request_permission` is added to `tools_list()`.
#[tokio::test]
async fn tools_list_serves_request_permission() {
    let (_dir, core) = util::core();
    let tools = util::list_tools(&core).await;
    util::find_tool(&tools, "request_permission");
    // The S3 surface is unchanged — the tool is ADDITIVE, not a replacement.
    for name in ["open_project", "spawn_agent", "gate_explain", "tail_events"] {
        util::find_tool(&tools, name);
    }
}

/// Doc §9 BINDING no-drift rule: the served `inputSchema` for
/// `request_permission` EQUALS `schemars::schema_for!` of the
/// `rezidnt_types::mcp::RequestPermissionArgs` shape — surface and published
/// type can never drift.
///
/// COMPILE-RED until `RequestPermissionArgs` exists; then ASSERT-RED until the
/// tool is served with that generated schema.
#[tokio::test]
async fn request_permission_schema_is_generated_from_rezidnt_types() {
    let (_dir, core) = util::core();
    let tools = util::list_tools(&core).await;
    let tool = util::find_tool(&tools, "request_permission");
    let expected = serde_json::to_value(schemars::schema_for!(
        rezidnt_types::mcp::RequestPermissionArgs
    ))
    .unwrap();
    assert_eq!(
        tool["inputSchema"], expected,
        "served request_permission inputSchema must EQUAL schemars::schema_for! of the rezidnt-types shape (no drift, doc §9)"
    );
}

/// Badge posture (design §5): the decision RESULT authorizes a mutation, so the
/// caller must be identified — the schema REQUIRES `badge`, plus the action
/// descriptor (`run`, `action`, `tool`).
///
/// COMPILE-RED until the arg type exists; then ASSERT-RED until it is served.
#[tokio::test]
async fn request_permission_schema_requires_badge_and_action_descriptor() {
    let (_dir, core) = util::core();
    let tools = util::list_tools(&core).await;
    let tool = util::find_tool(&tools, "request_permission");
    let required: Vec<String> = tool["inputSchema"]["required"]
        .as_array()
        .unwrap_or_else(|| panic!("request_permission inputSchema.required must exist: {tool:#}"))
        .iter()
        .filter_map(|v| v.as_str().map(String::from))
        .collect();
    for field in ["badge", "run", "action"] {
        assert!(
            required.contains(&field.to_string()),
            "request_permission schema must REQUIRE {field:?} (design §5); got {required:?}"
        );
    }
}

/// A missing badge is refused with the machine-readable `badge.required` code
/// BEFORE any decision is made (design §5 badge posture; §12 door discipline) —
/// the caller of an authorization decision must be identified.
///
/// ASSERT-RED until `request_permission` dispatch enforces the badge door.
#[tokio::test]
async fn request_permission_without_badge_is_refused() {
    let (_dir, core) = util::core();
    let result = util::tool_call(
        &core,
        1,
        "request_permission",
        json!({"run": "01SP1MCPRUN00000000000R001", "action": "tool.invoke", "tool": "Bash"}),
    )
    .await;
    util::assert_tool_refusal(&result, rezidnt_mcp::codes::BADGE_REQUIRED);
}

/// The decision is three-valued and NEVER coerced (I6): a `request_permission`
/// result carries a `decision` field that is exactly one of `allow | deny |
/// ask`. This pins the honest verdict vocabulary at the MCP surface —
/// `inconclusive` surfaces as `ask` (route-to-a-human), never a synthesized
/// allow.
///
/// ASSERT-RED until the tool returns a decision result.
#[tokio::test]
async fn request_permission_decision_is_three_valued_never_coerced() {
    let badge = rezidnt_run::badge::Badge::mint().expect("mint badge");
    let (_dir, core) = util::core_with_badges(&[&badge]);
    let result = util::tool_call(
        &core,
        2,
        "request_permission",
        json!({
            "badge": badge.token_hex(),
            "run": "01SP1MCPRUN00000000000R002",
            "action": "tool.invoke",
            "tool": "Bash"
        }),
    )
    .await;
    let payload = util::tool_payload(&result);
    let decision = payload["decision"]
        .as_str()
        .unwrap_or_else(|| panic!("request_permission result carries a decision: {payload:#}"));
    assert!(
        matches!(decision, "allow" | "deny" | "ask"),
        "decision must be exactly one of allow|deny|ask, never coerced (I6); got {decision:?}"
    );
}

/// The request lands a `permit.requested` fact on the log (I3: the log is
/// truth; the permission stream is first-class in `tail`), and the decision
/// lands one of `permit.granted`/`permit.denied`/`permit.escalated`. The MCP
/// tool is a producer of the ratified permit subjects.
///
/// ASSERT-RED until the tool publishes the permit facts.
#[tokio::test]
async fn request_permission_logs_the_permit_request_and_decision_facts() {
    let badge = rezidnt_run::badge::Badge::mint().expect("mint badge");
    let (_dir, core) = util::core_with_badges(&[&badge]);
    let _ = util::tool_call(
        &core,
        3,
        "request_permission",
        json!({
            "badge": badge.token_hex(),
            "run": "01SP1MCPRUN00000000000R003",
            "action": "tool.invoke",
            "tool": "Bash"
        }),
    )
    .await;
    let subjects: Vec<String> = util::log_events(&core)
        .iter()
        .map(|e| e.subject.as_str().to_string())
        .collect();
    assert!(
        subjects.iter().any(|s| s == "permit.requested"),
        "the request lands a permit.requested fact on the log (I3); got {subjects:?}"
    );
    assert!(
        subjects.iter().any(|s| {
            matches!(
                s.as_str(),
                "permit.granted" | "permit.denied" | "permit.escalated"
            )
        }),
        "the decision lands one of the three permit decision facts (I3); got {subjects:?}"
    );
}

/// SP1 ACCEPT DEMO (the `gate why` leg, I6) — after an agent is DENIED on a
/// forced policy breach (the committed `permit_deny_demo` golden fixture),
/// `gate_explain` on that run resolves the DECIDING policy and evidence: the
/// deny `reason`, the `policy_ref`, and the `evidence_ref`. A blocked agent can
/// always read WHY (design §5; ontology `permit.denied` lines 334-337).
///
/// ASSERT-RED until `gate_explain` surfaces the permit decision (today it only
/// resolves `gate.passed`/`gate.failed`/`gate.inconclusive` facts, so a
/// permit-only run answers `gate.no_verdict` — the SP1 work is to make the
/// interrogation see the permit deny).
#[tokio::test]
async fn gate_explain_resolves_a_permit_deny_policy_and_evidence() {
    let (_dir, core) = util::core();
    util::seed_fixture(&core, "permit_deny_demo.jsonl");

    let result = util::tool_call(
        &core,
        4,
        "gate_explain",
        json!({"run": "01SP1DENYDEM0FXX00000RN01"}),
    )
    .await;
    assert_ne!(
        result["isError"],
        json!(true),
        "a denied run is interrogable — gate_explain must not answer gate.no_verdict: {result:#}"
    );
    let payload = util::tool_payload(&result);
    assert_eq!(
        payload["verdict"],
        json!("deny"),
        "the permit deny surfaces as a deny verdict (I6), never coerced"
    );
    assert_eq!(
        payload["reason"],
        json!("tool Bash not in allowlist"),
        "the deny reason is surfaced so the blocked agent reads WHY (I6)"
    );
    assert_eq!(
        payload["policy_ref"]["hash"],
        json!("70014110w1157000000000000000000000000000000000000000d3ny70011s"),
        "the deciding policy_ref is resolvable (I6)"
    );
    assert_eq!(
        payload["evidence_ref"]["hash"],
        json!("ev1dence00000000000000000000000000000000000000000000000d3nyev1"),
        "the decision evidence ref is resolvable (I6, I2 — ref not inline)"
    );
}
