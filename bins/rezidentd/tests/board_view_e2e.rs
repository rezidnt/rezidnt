//! DR-039 oracle (C3) — `board_view` end-to-end over the loopback-HTTP MCP
//! transport (I5: every capability is an MCP tool first; doc §9).
//!
//! This is the real read path a DR-038 GUI panel would ride: seed the log from a
//! committed golden fixture, bring up the daemon serving MCP over HTTP
//! (lockfile-announced), call `board_view` with the empty snapshot arg through
//! the HTTP surface, and assert the payload is the fleet `BoardView` shape with
//! the seeded run + worktree present. A read — never `isError`, no badge (doc
//! §12 as amended by DR-005).
//!
//! RED MODE: `board_view` is not advertised/dispatched yet, so `mcp_tool_call`
//! returns an error result (or the tool is unknown) until the implementer serves
//! it. That red is "missing tool", not a typo.
#![cfg(unix)]

mod common;

use std::time::Duration;

use common::{mcp_tool_call, start_daemon_with_mcp, tool_payload, wait_for_lockfile};
use serde_json::json;

const LOCK_DEADLINE: Duration = Duration::from_secs(10);

/// The known entities in `s4_verified_run.jsonl` (a verified run + an
/// allocated/merged worktree). Pinning them makes the e2e assertion specific:
/// the folded fleet actually surfaces THIS run and THIS worktree over the wire.
const S4_RUN: &str = "01S4VER1F1ED00000000000R01";
const S4_WORKTREE: &str = "/tmp/rezidnt-s4/impl";

/// `board_view` over MCP-HTTP returns the fleet projection: a u64
/// `events_folded` heartbeat, array `runs` / `worktrees` / `counts_by_subject`,
/// and the seeded run id + worktree path both present. Read-class: no badge,
/// never `isError`.
#[test]
fn board_view_reads_fleet_projection_over_http() {
    // Pre-seed the log from the committed golden fixture BEFORE the daemon
    // starts — the log is truth (I3), so the fleet state comes from there.
    let (_daemon, lock_path) = start_daemon_with_mcp(Some("s4_verified_run.jsonl"));
    let lock = wait_for_lockfile(&lock_path, LOCK_DEADLINE);
    let url = lock["url"].as_str().expect("lockfile carries url");

    // Read-class tool: empty snapshot arg, no badge (doc §12 / DR-005).
    let result = mcp_tool_call(url, 1, "board_view", json!({}));
    assert_ne!(
        result["isError"],
        json!(true),
        "board_view is a read; it must not error: {result:#}"
    );

    let payload = tool_payload(&result);

    // Fleet heartbeat: events_folded is a u64 (the whole-log fold count).
    assert!(
        payload["events_folded"].is_u64(),
        "events_folded must be a u64 heartbeat: {payload:#}"
    );

    // The three fleet collections are arrays.
    let runs = payload["runs"]
        .as_array()
        .unwrap_or_else(|| panic!("runs must be an array: {payload:#}"));
    let worktrees = payload["worktrees"]
        .as_array()
        .unwrap_or_else(|| panic!("worktrees must be an array: {payload:#}"));
    assert!(
        payload["counts_by_subject"].is_array(),
        "counts_by_subject must be an array: {payload:#}"
    );

    // The seeded run is present, by its known id.
    assert!(
        runs.iter().any(|r| r["run"] == json!(S4_RUN)),
        "the seeded run {S4_RUN} must appear in the board's runs: {payload:#}"
    );

    // The seeded worktree is present, by its known path.
    assert!(
        worktrees.iter().any(|w| w["path"] == json!(S4_WORKTREE)),
        "the seeded worktree {S4_WORKTREE} must appear in the board's worktrees: {payload:#}"
    );
}
