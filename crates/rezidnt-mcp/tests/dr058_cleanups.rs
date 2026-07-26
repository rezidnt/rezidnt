//! DR-058 ORACLE — §Decision 5: the three folded-in cleanups, each judged as
//! close to behaviour as the cleanup allows.
//!
//! (i)   `codes::CAS_HASH_INVALID`'s served MESSAGE must interpolate
//!       `CAS_ADDRESS_HEX_LEN`, not hardcode `64`. Judged as a DRIFT TRIPWIRE:
//!       the served refusal must contain the const's rendered value. GREEN at
//!       cut time BY NECESSITY — with the const at 64 an interpolated message
//!       and a hardcoded one are byte-identical, so no behavioural test can be
//!       red today. The tripwire's teeth are proven by MUTATION (const 64→63:
//!       hardcoded message → red, interpolated message → green); see the board
//!       report. This is the one deliberately-green judge on the red files.
//! (ii)  `codes::WORKTREE_UNKNOWN`'s doc must cover `diff_view` as well as
//!       `release_worktree`. Const doc comments are served NOWHERE (they are
//!       not schema prose), so no behavioural or served-schema judge exists —
//!       this is a SOURCE-TEXT guard, scoped to the contiguous doc block
//!       immediately above the const so prose elsewhere cannot green it
//!       (mutation-proven; see report). RED at cut time.
//! (iii) `CasReadArgs.hash`'s doc is served VERBATIM over `tools/list` to
//!       every MCP client, so this one IS a served-schema judge: the
//!       description must state the RULE (64, lowercase, hex) and must NOT
//!       name `rezidnt_mcp::is_cas_address` — a private symbol no external
//!       reader can resolve. RED at cut time (the description names it).

mod util;

use rezidnt_mcp::{CAS_ADDRESS_HEX_LEN, codes};
use rezidnt_run::badge::Badge;
use serde_json::json;
use std::sync::Arc;

use rezidnt_cas::Cas;
use rezidnt_fabric::{EventLog, Fabric};
use rezidnt_mcp::{BadgeBook, McpCore};

fn badged_cas_core() -> (tempfile::TempDir, Badge, Arc<McpCore>) {
    let dir = tempfile::tempdir().expect("tempdir");
    let log = EventLog::open(&dir.path().join("events.db")).expect("open log");
    let fabric = Fabric::new(log, 1024);
    let cas = Arc::new(Cas::open(&dir.path().join("cas")).expect("open cas"));
    let operator = Badge::mint().expect("mint badge");
    let mut book = BadgeBook::new();
    book.admit(&operator);
    let core = McpCore::new(fabric, book).with_cas(cas);
    (dir, operator, Arc::new(core))
}

/// (i) THE DRIFT TRIPWIRE — the served `cas.hash_invalid` message carries the
/// rendered `CAS_ADDRESS_HEX_LEN`. Deliberately green today (64 == "64");
/// binds message to const so a revised const with a stale hardcoded message
/// goes red. The call presents an admitted badge so this file survives the
/// DR-058 door unchanged.
#[tokio::test]
async fn the_hash_invalid_message_tracks_cas_address_hex_len() {
    let (_dir, operator, core) = badged_cas_core();
    let result = util::tool_call(
        &core,
        1,
        "cas_read",
        json!({
            "hash": "../not-an-address",
            "bytes": 21,
            "mime": "text/x-diff",
            "badge": operator.token_hex(),
        }),
    )
    .await;
    util::assert_tool_refusal(&result, codes::CAS_HASH_INVALID);
    let message = util::tool_payload(&result)["message"]
        .as_str()
        .expect("refusal carries a message")
        .to_string();
    assert!(
        message.contains(&CAS_ADDRESS_HEX_LEN.to_string()),
        "the refusal message must render CAS_ADDRESS_HEX_LEN ({}) — a message \
         that hardcodes the number drifts silently when the const moves \
         (DR-058 §Decision 5(i)). Message: {message:?}",
        CAS_ADDRESS_HEX_LEN
    );
}

/// (ii) SOURCE-TEXT GUARD, tightly scoped — the contiguous `///` block
/// immediately above `pub const WORKTREE_UNKNOWN` must name `diff_view`,
/// which emits the code (DR-057) alongside `release_worktree` (DR-049).
///
/// Scoping is the guard's honesty: only the block that library consumers see
/// on THIS const counts, so the word appearing anywhere else in the file
/// cannot green it (mutation-proven; see the board report).
#[test]
fn the_worktree_unknown_doc_covers_diff_view() {
    let source = std::fs::read_to_string(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src/lib.rs"),
    )
    .expect("read rezidnt-mcp/src/lib.rs");

    let lines: Vec<&str> = source.lines().collect();
    let const_line = lines
        .iter()
        .position(|l| l.trim_start().starts_with("pub const WORKTREE_UNKNOWN"))
        .expect("codes::WORKTREE_UNKNOWN exists");

    // Walk back through the contiguous doc block (/// lines, tolerating the
    // blank-comment separators rustfmt keeps inside one block).
    let mut start = const_line;
    while start > 0 {
        let prev = lines[start - 1].trim_start();
        if prev.starts_with("///") {
            start -= 1;
        } else {
            break;
        }
    }
    let doc_block: String = lines[start..const_line].join("\n");

    assert!(
        doc_block.contains("release_worktree"),
        "guard sanity: the block scoped-to must be the one that today \
         describes release_worktree — if this fires, the scoping walked to \
         the wrong const. Block:\n{doc_block}"
    );
    assert!(
        doc_block.contains("diff_view"),
        "codes::WORKTREE_UNKNOWN's doc must cover diff_view too — DR-057 \
         made diff_view a second emitter and DR-058 §Decision 5(ii) rules the \
         doc widened (the doc, not the code). Block:\n{doc_block}"
    );
}

/// (iii) SERVED-SCHEMA JUDGE — the `cas_read` `hash` description every MCP
/// client receives over `tools/list` states the rule and names no private
/// symbol.
#[tokio::test]
async fn the_served_hash_description_states_the_rule_without_private_symbols() {
    let (_dir, _operator, core) = badged_cas_core();
    let tools = util::list_tools(&core).await;
    let description =
        util::find_tool(&tools, "cas_read")["inputSchema"]["properties"]["hash"]["description"]
            .as_str()
            .expect("hash serves a description (dr057_surface pins presence)")
            .to_string();

    // The RULE, stated: 64, lowercase, hex. Loose contains — prose is free to
    // be reworded — but the three load-bearing tokens must survive.
    for token in ["64", "lowercase", "hex"] {
        assert!(
            description.to_ascii_lowercase().contains(token),
            "the served hash description must state the rule ({token} \
             missing) — an external client has only this prose to go on \
             (DR-058 §Decision 5(iii)). Description: {description:?}"
        );
    }
    assert!(
        !description.contains("is_cas_address"),
        "the served description names `is_cas_address`, a PRIVATE symbol no \
         MCP client can resolve — state the rule, not the implementation \
         (DR-058 §Decision 5(iii)). Description: {description:?}"
    );
    assert!(
        !description.contains("rezidnt_mcp::"),
        "no `rezidnt_mcp::` path belongs in wire prose served to external \
         clients — none of them can resolve it. Description: {description:?}"
    );
}
