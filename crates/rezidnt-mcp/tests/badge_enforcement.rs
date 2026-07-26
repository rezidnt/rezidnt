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

/// The door's MESSAGE, not just its code. Since DR-058 §Decision 2 TWO tool
/// classes come through `check_badge` — every mutating tool, and `cas_read`, a
/// READ tool — and that message is served over the wire to the caller it
/// refuses. The code alone cannot judge this: both classes refuse with the same
/// code BY DESIGN, so a message that asserts the refused call was mutating is
/// false to half the callers who receive it and nothing goes red.
///
/// Two assertions, both load-bearing: ONE door answers ONE message (branching
/// it per tool would hand a caller a way to tell the classes apart from a
/// refusal), and that message claims no property only one class has.
#[tokio::test]
async fn the_badge_required_message_is_true_of_a_read_tool_and_a_mutating_one() {
    let (_dir, core) = util::core();

    let mutating = util::tool_call(
        &core,
        5,
        "open_project",
        json!({"spec_toml": "[project]\nname = \"x\"\nrepo = \".\"\n"}),
    )
    .await;
    util::assert_tool_refusal(&mutating, rezidnt_mcp::codes::BADGE_REQUIRED);

    // A READ tool behind the same door (DR-058 §Decision 2). The door runs
    // before the substrate check, so a core with no CAS still answers it.
    let read = util::tool_call(
        &core,
        6,
        "cas_read",
        json!({
            "hash": "aa11bb22cc33dd44ee55ff660718293a4b5c6d7e8f90a1b2c3d4e5f607182930",
            "bytes": 21,
            "mime": "text/x-diff",
        }),
    )
    .await;
    util::assert_tool_refusal(&read, rezidnt_mcp::codes::BADGE_REQUIRED);

    let message = |result: &serde_json::Value| {
        util::tool_payload(result)["message"]
            .as_str()
            .expect("a refusal carries a message")
            .to_string()
    };
    let (mutating, read) = (message(&mutating), message(&read));
    assert_eq!(
        mutating, read,
        "one §12 door, one badge.required message: a message that varies by \
         tool is a fact about the call leaking out of a refusal that is \
         supposed to carry none"
    );
    assert!(
        !read.to_ascii_lowercase().contains("mutating"),
        "the badge.required message is served to a refused `cas_read` caller, \
         and `cas_read` is a READ tool — a message calling it mutating tells \
         that caller something false about the call it just made (DR-058 \
         §Decision 2). Message: {read:?}"
    );
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
