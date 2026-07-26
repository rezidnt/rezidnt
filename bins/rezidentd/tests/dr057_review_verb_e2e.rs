//! DR-057 ORACLE — the Review verb end-to-end over the daemon's loopback-HTTP
//! MCP transport (I5: every capability is an MCP tool first; doc §9).
//!
//! This is the real read path the operator cockpit's diff panel rides:
//! `diff_view` hands over the full `CasRef`, `cas_read` resolves it to bytes
//! — against the LIVE daemon, not a bare test core. The load-bearing wiring
//! claim is the second test: the daemon must serve `cas_read` from ITS OWN
//! CAS root (`cas/` next to the event db, `McpCore::with_cas` at
//! `bins/rezidentd/src/main.rs`), because an implementation green on the
//! bare-core boards could still read the EPHEMERAL fallback CAS in the daemon
//! and answer NotFound for every real diff — a record-says-wired,
//! tree-isn't-wired defect of exactly the fan-out silent-wrong class. This
//! test is behavioral, not a source-text guard: it fails against the
//! ephemeral-CAS dodge because the planted blob is only in the daemon's root.
//!
//! RED MODE: `diff_view`/`cas_read` are unknown tools, so `mcp_tool_call`'s
//! "must not be a protocol error" assertion fires. Red for the right reason:
//! the tools do not exist.
#![cfg(unix)]

mod common;

use std::time::Duration;

use common::{mcp_tool_call, start_daemon_with_mcp, tool_payload, wait_for_lockfile};
use rezidnt_cas::Cas;
use serde_json::json;

const LOCK_DEADLINE: Duration = Duration::from_secs(10);

/// The known entities of `s4_verified_run.jsonl` (the board_view_e2e pins).
const S4_WORKTREE: &str = "/tmp/rezidnt-s4/impl";
const S4_HASH: &str = "1d50030ca17af09eb6fad0eadfb3492275bfc76635d0965260cde6bc685d785e";

/// `diff_view` over MCP-HTTP serves the merged tree's row with the FULL
/// `CasRef` — hash, bytes, mime — verbatim from the seeded log. Read-class:
/// no badge argument.
#[test]
fn diff_view_serves_the_full_ref_over_http() {
    let (_daemon, lock_path) = start_daemon_with_mcp(Some("s4_verified_run.jsonl"));
    let lock = wait_for_lockfile(&lock_path, LOCK_DEADLINE);
    let url = lock["url"].as_str().expect("lockfile carries url");

    let result = mcp_tool_call(url, 1, "diff_view", json!({"worktree": S4_WORKTREE}));
    assert_ne!(
        result["isError"],
        json!(true),
        "diff_view is a read; it must not error: {result:#}"
    );
    let payload = tool_payload(&result);
    assert_eq!(payload["worktree"], json!(S4_WORKTREE));
    assert_eq!(payload["lifecycle"], json!("allocated"));
    assert_eq!(payload["outcome"], json!("merged"));
    assert_eq!(
        payload["diff"],
        json!({"hash": S4_HASH, "bytes": 23, "mime": "text/plain"}),
        "the full CasRef rides the wire — the exact triple the fixture's \
         diff.merged carries, so cas_read can be called with it verbatim: {payload:#}"
    );
}

/// `cas_read` over MCP-HTTP reads the DAEMON'S OWN CAS root. The blob is
/// planted only in `cas/` next to the daemon's event db (the write-once
/// store is safe to share); the exact bytes coming back over HTTP proves
/// `with_cas` wiring reaches the tool — an ephemeral-fallback implementation
/// answers NotFound here and fails.
#[test]
fn cas_read_serves_the_daemons_own_cas_over_http() {
    let (daemon, lock_path) = start_daemon_with_mcp(None);
    let lock = wait_for_lockfile(&lock_path, LOCK_DEADLINE);
    let url = lock["url"].as_str().expect("lockfile carries url");

    let cas_root = daemon.db.parent().expect("db has a parent dir").join("cas");
    let cas = Cas::open(&cas_root).expect("open the daemon's CAS root");
    let text = "--- a/review.rs\n+++ b/review.rs\n@@ -1 +1 @@\n-before\n+after\n";
    let planted = cas
        .put(text.as_bytes(), "text/x-diff")
        .expect("plant diff blob");

    let result = mcp_tool_call(
        url,
        2,
        "cas_read",
        json!({"hash": planted.hash, "bytes": planted.bytes, "mime": planted.mime}),
    );
    assert_ne!(
        result["isError"],
        json!(true),
        "an in-bound text read of a blob in the daemon's CAS is ADMITTED: {result:#}"
    );
    let payload = tool_payload(&result);
    assert_eq!(
        payload["content"],
        json!(text),
        "the exact planted bytes come back — the daemon serves ITS cas/, not \
         an ephemeral fallback: {payload:#}"
    );
    assert_eq!(payload["bytes_returned"].as_u64(), Some(text.len() as u64));
    assert_eq!(
        payload["truncated"],
        json!(false),
        "a whole read says so — v1 serves no partials"
    );
}
