//! DR-040 oracle (C3) — `get_escalations` end-to-end over the loopback-HTTP MCP
//! transport (I5: every capability is an MCP tool first; doc §9).
//!
//! This is the real read path a DR-038 GUI escalation panel would ride: seed the
//! log from a committed golden fixture, bring up the daemon serving MCP over HTTP
//! (lockfile-announced), call `get_escalations` with the empty (all-runs) arg
//! through the HTTP surface, and assert the payload is an array of escalation
//! rows carrying the seeded OUTSTANDING escalation (a `permit.escalated` never
//! resolved). A read — never `isError`, no badge (doc §12 as amended by DR-005).
//!
//! RED MODE: `get_escalations` is not advertised/dispatched yet, so
//! `mcp_tool_call` returns an error result (or the tool is unknown) until the
//! implementer serves it. That red is "missing tool", not a typo.
#![cfg(unix)]

mod common;

use std::time::Duration;

use common::{mcp_tool_call, start_daemon_with_mcp, tool_payload, wait_for_lockfile};
use serde_json::json;

const LOCK_DEADLINE: Duration = Duration::from_secs(10);

/// The OUTSTANDING escalation in `s5b_board_permit.jsonl`: a `permit.escalated`
/// on run `…RN01`, request `…RQ003`, that is never resolved. The fixture also
/// carries a granted and a denied permit — so a tool that surfaced ALL permit
/// decisions (not just the outstanding escalations) would fail the specificity
/// assertions below. Pinning them makes the e2e assertion specific: the folded
/// ledger actually surfaces THIS outstanding escalation over the wire.
const S5B_RUN: &str = "01S5BB0ARDPERMFXTRE000RN01";
const S5B_ESCALATED_REQ: &str = "01S5BB0ARDPERMFXTRERQ003";

/// `get_escalations` over MCP-HTTP returns the outstanding-escalations
/// projection: an array of rows, with the seeded run id + request_id present and
/// the escalation reason surfaced verbatim. Read-class: no badge, never
/// `isError`.
#[test]
fn get_escalations_reads_outstanding_escalations_over_http() {
    // Pre-seed the log from the committed golden fixture BEFORE the daemon
    // starts — the log is truth (I3), so the escalation comes from there.
    let (_daemon, lock_path) = start_daemon_with_mcp(Some("s5b_board_permit.jsonl"));
    let lock = wait_for_lockfile(&lock_path, LOCK_DEADLINE);
    let url = lock["url"].as_str().expect("lockfile carries url");

    // Read-class tool: empty (all-runs) arg, no badge (doc §12 / DR-005).
    let result = mcp_tool_call(url, 1, "get_escalations", json!({}));
    assert_ne!(
        result["isError"],
        json!(true),
        "get_escalations is a read; it must not error: {result:#}"
    );

    let payload = tool_payload(&result);

    // The projection is a `Vec<EscalationRow>` — an array on the wire.
    let rows = payload
        .as_array()
        .unwrap_or_else(|| panic!("get_escalations payload must be an array: {payload:#}"));

    // The seeded OUTSTANDING escalation is present, by its known request_id on
    // the known run — and NOT drowned by the fixture's granted/denied permits.
    let row = rows
        .iter()
        .find(|r| r["request_id"] == json!(S5B_ESCALATED_REQ))
        .unwrap_or_else(|| {
            panic!("the seeded outstanding escalation {S5B_ESCALATED_REQ} must appear: {payload:#}")
        });
    assert_eq!(
        row["run"],
        json!(S5B_RUN),
        "the escalation row carries its run: {payload:#}"
    );
    assert_eq!(
        row["reason"],
        json!("cumulative spend crossed soft cap"),
        "the escalation reason surfaces verbatim over the wire (I6): {payload:#}"
    );
}
