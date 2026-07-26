//! DR-057 debrief finding F8 — JUDGES for the four gaps this arc settled in
//! doc comments alone.
//!
//! DR-057 left four questions open; the implementer answered all four in
//! rustdoc and pinned none of them. That is the defect class DR-056 §Decision 2
//! exists to end — prose carrying a mechanism no judge holds — appearing in the
//! very slice that voluntarily adopted DR-056. One assertion each, so the
//! settled reading is falsifiable rather than merely written down:
//!
//! (a) an OVER-CLAIMING `bytes` is served the actual blob, not refused
//!     (`crates/rezidnt-mcp/src/lib.rs`, `call_cas_read` gap 1: the bound
//!     governs CONTENT, and `bytes_returned` reports what was really served, so
//!     a caller detects its own bad metadata itself);
//! (b) `application/json` is NOT text in v1 (gap 2: `text/*` is exactly what the
//!     diff path produces, and admitting structured `application/*` would widen
//!     this into the generic evidence reader DR-057 declines to bless);
//! (c) mime PARAMETERS are ignored, so `text/plain; charset=utf-8` is text
//!     (gap 3) — and that is not a hypothetical: `bins/rezidentd/src/runs.rs`
//!     writes exactly that mime when an oversized `agent.message` overflows to
//!     the CAS;
//! (d) `diff_view`'s unknown tree is refused `worktree.unknown` — the EXISTING
//!     code for "a worktree path this daemon's log does not know", reused
//!     rather than duplicated. The oracle board pinned this only negatively
//!     (`dr057_diff_view.rs`: "not a badge code"); here it is pinned exactly.

mod util;

use std::sync::Arc;

use rezidnt_cas::Cas;
use rezidnt_fabric::{EventLog, Fabric};
use rezidnt_mcp::{BadgeBook, MAX_CAS_READ_BYTES_DEFAULT, McpCore, codes};
use serde_json::json;

/// A core with a WIRED CAS, an empty badge book and no substrate — every call
/// below is unbadged read-class.
fn core_with_cas() -> (tempfile::TempDir, Arc<Cas>, Arc<McpCore>) {
    let dir = tempfile::tempdir().expect("tempdir");
    let log = EventLog::open(&dir.path().join("events.db")).expect("open log");
    let fabric = Fabric::new(log, 1024);
    let cas = Arc::new(Cas::open(&dir.path().join("cas")).expect("open cas"));
    let core = McpCore::new(fabric, BadgeBook::new()).with_cas(Arc::clone(&cas));
    (dir, cas, Arc::new(core))
}

/// (a) An OVER-CLAIMING ref is SERVED, not refused — and `bytes_returned` tells
/// the truth about what came back.
///
/// The claimed `bytes` here is over the read bound while the stored blob is
/// tiny. Refusing on the claim would deny a perfectly in-bound read over the
/// caller's own bad metadata; serving it and reporting the real length lets the
/// caller detect the disagreement itself, which is why no mismatch code is
/// minted. The oracle board pinned the opposite direction (a lying UNDER-claim
/// must not smuggle over-bound content); this is the leg it left open.
#[tokio::test]
async fn an_over_claiming_bytes_is_served_the_actual_blob() {
    let (_dir, cas, core) = core_with_cas();
    let text = "--- a/small.rs\n+++ b/small.rs\n@@ -1 +1 @@\n-a\n+b\n";
    let stored = cas.put(text.as_bytes(), "text/x-diff").expect("put diff");

    let result = util::tool_call(
        &core,
        1,
        "cas_read",
        json!({
            "hash": stored.hash,
            "bytes": MAX_CAS_READ_BYTES_DEFAULT + 1,
            "mime": "text/x-diff",
        }),
    )
    .await;
    assert_ne!(
        result["isError"],
        json!(true),
        "an over-claimed `bytes` never authorizes or denies anything — the \
         bound is judged against the CONTENT, which is in bound here: {result:#}"
    );

    let payload = util::tool_payload(&result);
    assert_eq!(
        payload["content"],
        json!(text),
        "the actual blob is served whole: {payload:#}"
    );
    assert_eq!(
        payload["bytes_returned"].as_u64(),
        Some(text.len() as u64),
        "bytes_returned reports what was ACTUALLY served, not the claim — that \
         is how a caller detects its own stale ref without a mismatch code: {payload:#}"
    );
}

/// (b) `application/json` is NOT text in v1. Structured `application/*` would
/// widen `cas_read` into the generic evidence reader DR-057 §Risk register says
/// should get its own record. Widening later is additive; narrowing later would
/// break callers, so v1 refuses.
#[tokio::test]
async fn application_json_is_not_text_in_v1() {
    let (_dir, cas, core) = core_with_cas();
    let body = r#"{"verdict":"pass"}"#;
    let stored = cas
        .put(body.as_bytes(), "application/json")
        .expect("put json blob");

    let result = util::tool_call(
        &core,
        1,
        "cas_read",
        json!({"hash": stored.hash, "bytes": stored.bytes, "mime": "application/json"}),
    )
    .await;
    util::assert_tool_refusal(&result, codes::CAS_NOT_TEXT);
    assert!(
        util::tool_payload(&result).get("content").is_none(),
        "the refusal carries no content: {result:#}"
    );
}

/// (c) Mime PARAMETERS are ignored: `text/plain; charset=utf-8` is text, and it
/// is a mime this tree really writes (`bins/rezidentd/src/runs.rs` puts an
/// overflowed `agent.message` body under exactly that type). A v1 that matched
/// the whole header string would refuse the daemon's own blobs. Case is ignored
/// too, because media types are case-insensitive.
#[tokio::test]
async fn a_parameterised_text_mime_is_text() {
    let (_dir, cas, core) = core_with_cas();
    let text = "a bulk agent message that overflowed the inline cap\n";
    let stored = cas
        .put(text.as_bytes(), "text/plain; charset=utf-8")
        .expect("put message blob");

    for mime in ["text/plain; charset=utf-8", "TEXT/Plain; charset=UTF-8"] {
        let result = util::tool_call(
            &core,
            1,
            "cas_read",
            json!({"hash": stored.hash, "bytes": stored.bytes, "mime": mime}),
        )
        .await;
        assert_ne!(
            result["isError"],
            json!(true),
            "{mime:?} is a text mime — the type/subtype carries the text/binary \
             distinction, parameters do not: {result:#}"
        );
        assert_eq!(util::tool_payload(&result)["content"], json!(text));
    }
}

/// (d) `diff_view` on a tree the log has never folded refuses with the EXISTING
/// `worktree.unknown` — the code already minted for "a worktree path this
/// daemon holds nothing for" (DR-049 §Decision 3), not a second name for one
/// condition. It refuses rather than answering a miss BODY because it is a
/// TOOL: the miss-body precedent is the dossier RESOURCE, which has no
/// `isError` channel to refuse through.
#[tokio::test]
async fn diff_view_refuses_an_unknown_tree_with_worktree_unknown() {
    let (_dir, core) = util::core();
    let result = util::tool_call(&core, 1, "diff_view", json!({"worktree": "/no/such/tree"})).await;
    util::assert_tool_refusal(&result, codes::WORKTREE_UNKNOWN);
}
