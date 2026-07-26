//! DR-058 ORACLE — §Decision 2's OTHER leg: the rest of the read family is
//! UNCHANGED. `diff_view`, `board_view`, `tail_events` and `get_escalations`
//! stay unbadged and must still be ADMITTED with no badge — they return
//! structural facts, never raw blob bytes, and DR-058 redraws the tier on the
//! structure/content line, not the read/write line.
//!
//! ## Why this fence exists, and why it is GREEN at cut time
//!
//! This is the leg a careless "add badge checks to the read family" edit
//! breaks — nothing else in the slice would fail if `tail_events` quietly
//! grew a door, because the DR-057/DR-039/DR-040 boards mostly assert on
//! bare cores where a wrongly-added door and a missing one can look alike.
//! These tests run on a badge-CAPABLE core (root key wired, an operator badge
//! admitted, a CAS wired) so a door added "for the family" cannot hide behind
//! a core that lacks the machinery to enforce one.
//!
//! GREEN-BY-DESIGN, disclosed per the oracle's own rule: these judges pin
//! behaviour that must SURVIVE the slice, not behaviour the slice creates.
//! They are the sanctioned exception on this board — every other dr058 file
//! is red at cut time — and they earn their place by being the only explicit
//! judge of §Decision 2's "UNCHANGED" clause on a badge-capable core.
//!
//! Non-vacuity: every admission asserts real folded content coming back
//! (fixture-pinned rows/events), never a bare `isError == false`.

mod util;

use std::sync::Arc;

use rezidnt_cas::Cas;
use rezidnt_fabric::{EventLog, Fabric};
use rezidnt_mcp::{BadgeBook, McpCore};
use rezidnt_run::badge::{Badge, RootKey};
use serde_json::json;

/// The known entities of `s4_verified_run.jsonl` (the DR-057 boards' pins).
const S4_WORKTREE: &str = "/tmp/rezidnt-s4/impl";
const S4_HASH: &str = "1d50030ca17af09eb6fad0eadfb3492275bfc76635d0965260cde6bc685d785e";
/// The outstanding escalation of `s5b_board_permit.jsonl` (the DR-040 pins).
const S5B_RUN: &str = "01S5BB0ARDPERMFXTRE000RN01";
const S5B_ESCALATED_REQ: &str = "01S5BB0ARDPERMFXTRERQ003";

/// A badge-CAPABLE core: root key, one admitted operator badge, a wired CAS.
/// Every door mechanism exists here, so an unbadged admission below proves
/// the tool HAS no door — not that the core couldn't have enforced one.
fn badge_capable_core() -> (tempfile::TempDir, Arc<McpCore>) {
    let dir = tempfile::tempdir().expect("tempdir");
    let log = EventLog::open(&dir.path().join("events.db")).expect("open log");
    let fabric = Fabric::new(log, 1024);
    let cas = Arc::new(Cas::open(&dir.path().join("cas")).expect("open cas"));
    let operator = Badge::mint().expect("mint badge");
    let mut book = BadgeBook::new();
    book.admit(&operator);
    let core = McpCore::new(fabric, book)
        .with_root_key(RootKey::from_bytes([7u8; 32]))
        .with_cas(cas);
    (dir, Arc::new(core))
}

/// `diff_view` with NO badge still serves the merged tree's full row —
/// lifecycle, outcome, and the complete `CasRef` — on a core that could have
/// enforced a door if one existed.
#[tokio::test]
async fn diff_view_stays_unbadged_and_serves_the_full_row() {
    let (_dir, core) = badge_capable_core();
    util::seed_fixture(&core, "s4_verified_run.jsonl");

    let result = util::tool_call(&core, 1, "diff_view", json!({"worktree": S4_WORKTREE})).await;
    assert_ne!(
        result["isError"],
        json!(true),
        "diff_view is UNCHANGED by DR-058 §Decision 2 — unbadged, admitted: {result:#}"
    );
    let payload = util::tool_payload(&result);
    assert_eq!(payload["worktree"], json!(S4_WORKTREE));
    assert_eq!(payload["outcome"], json!("merged"));
    assert_eq!(
        payload["diff"],
        json!({"hash": S4_HASH, "bytes": 23, "mime": "text/plain"}),
        "the full ref still rides — structural fact, not blob content: {payload:#}"
    );
}

/// `board_view` with NO badge still serves the whole folded fleet.
#[tokio::test]
async fn board_view_stays_unbadged_and_serves_the_folded_fleet() {
    let (_dir, core) = badge_capable_core();
    let seeded = util::seed_fixture(&core, "s4_verified_run.jsonl");

    let result = util::tool_call(&core, 1, "board_view", json!({})).await;
    assert_ne!(
        result["isError"],
        json!(true),
        "board_view is UNCHANGED by DR-058 §Decision 2 — unbadged, admitted: {result:#}"
    );
    let payload = util::tool_payload(&result);
    let worktrees = payload["worktrees"]
        .as_array()
        .unwrap_or_else(|| panic!("board_view serves worktrees: {payload:#}"));
    assert_eq!(worktrees.len(), 1, "the s4 fixture folds to one worktree");
    assert_eq!(
        payload["events_folded"].as_u64(),
        Some(seeded.len() as u64),
        "the whole seeded log folded — real content, not a vacuous admit: {payload:#}"
    );
}

/// `tail_events` with NO badge still serves the verbatim envelopes.
#[tokio::test]
async fn tail_events_stays_unbadged_and_serves_verbatim_envelopes() {
    let (_dir, core) = badge_capable_core();
    let seeded = util::seed_fixture(&core, "s4_verified_run.jsonl");

    let result = util::tool_call(&core, 1, "tail_events", json!({})).await;
    assert_ne!(
        result["isError"],
        json!(true),
        "tail_events is UNCHANGED by DR-058 §Decision 2 — unbadged, admitted: {result:#}"
    );
    let payload = util::tool_payload(&result);
    let events = payload["events"]
        .as_array()
        .unwrap_or_else(|| panic!("tail_events serves events: {payload:#}"));
    assert_eq!(
        events.len(),
        seeded.len(),
        "every seeded envelope comes back — the tool that made every CasRef \
         discoverable stays open by RULING, not by accident: {payload:#}"
    );
    assert_eq!(
        events[0],
        serde_json::to_value(&seeded[0]).expect("event serializes"),
        "envelopes are verbatim from the log: {payload:#}"
    );
}

/// `get_escalations` with NO badge still serves the outstanding escalation.
#[tokio::test]
async fn get_escalations_stays_unbadged_and_serves_the_outstanding_row() {
    let (_dir, core) = badge_capable_core();
    util::seed_fixture(&core, "s5b_board_permit.jsonl");

    let result = util::tool_call(&core, 1, "get_escalations", json!({"run": S5B_RUN})).await;
    assert_ne!(
        result["isError"],
        json!(true),
        "get_escalations is UNCHANGED by DR-058 §Decision 2 — unbadged, admitted: {result:#}"
    );
    let payload = util::tool_payload(&result);
    let rows = payload
        .as_array()
        .unwrap_or_else(|| panic!("get_escalations serves rows: {payload:#}"));
    assert!(
        rows.iter()
            .any(|r| r["request_id"] == json!(S5B_ESCALATED_REQ)),
        "the fixture's outstanding escalation is served — real content, not a \
         vacuous admit: {payload:#}"
    );
}
