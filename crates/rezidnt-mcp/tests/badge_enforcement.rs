//! S3 oracle — badge enforcement on mutating MCP tools (doc §12).
//!
//! The point is attribution and refusal-before-effect: a mutating call with
//! no valid badge is refused with a machine-readable code and leaves the log
//! UNTOUCHED (I3: if it isn't in the log, it didn't happen — so a refused
//! call must put nothing there).
//!
//! Pending-ratification note (S2 pattern): the `badge_id` attribution
//! assertion in the valid-badge daemon tests rides an ADDITIVE field on
//! ratified payloads; the warden item is flagged in the oracle work order.
//! Here we pin only refusal semantics, which no ratification can change.

mod util;

use rezidnt_run::badge::Badge;
use serde_json::json;

/// No `badge` argument at all → `badge.required`, and NO event lands on the
/// fabric.
#[tokio::test]
async fn open_project_without_badge_is_refused_with_no_side_effect() {
    let (_dir, core) = util::core();
    let result = util::tool_call(
        &core,
        1,
        "open_project",
        json!({"spec_toml": "[project]\nname = \"x\"\nrepo = \".\"\n"}),
    )
    .await;
    util::assert_tool_refusal(&result, rezidnt_mcp::codes::BADGE_REQUIRED);
    assert!(
        util::log_events(&core).is_empty(),
        "a refused mutation must leave the log untouched"
    );
}

/// A well-formed but unknown token → `badge.invalid`, no side effect.
#[tokio::test]
async fn open_project_with_unknown_badge_is_refused_with_no_side_effect() {
    let (_dir, core) = util::core();
    let stranger = Badge::mint().expect("mint"); // never admitted
    let result = util::tool_call(
        &core,
        2,
        "open_project",
        json!({
            "badge": stranger.token_hex(),
            "spec_toml": "[project]\nname = \"x\"\nrepo = \".\"\n"
        }),
    )
    .await;
    util::assert_tool_refusal(&result, rezidnt_mcp::codes::BADGE_INVALID);
    assert!(
        util::log_events(&core).is_empty(),
        "an unknown badge must leave the log untouched"
    );
}

/// `spawn_agent` sits behind the same door.
#[tokio::test]
async fn spawn_agent_without_badge_is_refused_with_no_side_effect() {
    let (_dir, core) = util::core();
    let result = util::tool_call(
        &core,
        3,
        "spawn_agent",
        json!({
            "workspace": "01ARZ3NDEKTSV4RRFFQ69G5FAV",
            "agent": "impl",
            "idempotency_key": "k-1"
        }),
    )
    .await;
    util::assert_tool_refusal(&result, rezidnt_mcp::codes::BADGE_REQUIRED);
    assert!(util::log_events(&core).is_empty());
}

/// Ordering: the badge is checked BEFORE the spec is even parsed. A valid
/// badge plus garbage spec must fail as `spec.invalid` — proof the badge
/// gate passed and refusal-ordering is badge-first.
#[tokio::test]
async fn badge_check_precedes_spec_parsing() {
    let admitted = Badge::mint().expect("mint");
    let (_dir, core) = util::core_with_badges(&[&admitted]);
    let result = util::tool_call(
        &core,
        4,
        "open_project",
        json!({
            "badge": admitted.token_hex(),
            "spec_toml": "this is not toml ["
        }),
    )
    .await;
    util::assert_tool_refusal(&result, rezidnt_mcp::codes::SPEC_INVALID);
    assert!(
        util::log_events(&core).is_empty(),
        "a spec that never parsed must not materialize anything"
    );
}
