//! DR-057 debrief finding F1 (SECURITY) — `cas_read`'s `hash` is a
//! caller-controlled PATH COMPONENT on an UNBADGED tool, and its shape is a
//! security boundary.
//!
//! `rezidnt_cas::Cas::path_for` is a bare `root.join(hash)`. `PathBuf::join`
//! REPLACES the root on an absolute component and normalizes no `..`, so before
//! this guard an unbadged caller could aim `cas_read` at any path the daemon can
//! read and harvest two facts from the refusal alone: EXISTENCE (a missing
//! target answers `cas.not_found`, a present one answers `cas.corrupt` or
//! `cas.too_large`) and EXACT BYTE SIZE (`blob is {actual} bytes`). Content
//! stayed safe only incidentally, because `Cas::get` re-hashes. The metadata
//! oracle was real, and it is exactly the local-disk backchannel DR-038
//! §Decision 4 forecloses.
//!
//! What this board pins:
//!
//! 1. every malformed address refuses BYTE-IDENTICALLY (`cas.hash_invalid`,
//!    a message carrying neither the input nor any fact about the store), so a
//!    caller cannot separate a traversal from an absolute path from uppercase
//!    hex from a wrong length from a stray non-hex character;
//! 2. the refusal is IDENTICAL whether or not the traversal target exists, and
//!    identical whichever size it is. That equality IS the oracle's death: the
//!    answer carries zero bits about the filesystem;
//! 3. the guard is not a blanket refusal — a well-formed address still reaches
//!    the store, still serves content, and still answers `cas.not_found` for a
//!    blob this daemon does not hold;
//! 4. the PREMISE above is not folklore — the refusals this guard stands in
//!    FRONT of really are fact-bearing (`cas.too_large` states a byte count,
//!    `cas.corrupt` a content hash), so a tree that quietly made them fact-free
//!    could not retire this board's reason for existing without going red.
//!
//! WHAT THE MUTATION SHOWS NOW (corrected 2026-07-26; the claim this paragraph
//! replaced was true of the pre-DR-058 tree and is FALSE of this one). Deleting
//! the `is_cas_address` gate in `read_bounded` leaves the two behavioral tests
//! below GREEN. Control falls through to the `Cas::path_for` call beneath it,
//! whose `InvalidAddress` arm answers the same `cas.hash_invalid` with the same
//! message, so the tool's answer does not move — §Decision 4's defence in depth
//! doing exactly what it was kept for, and equally the reason these two tests
//! judge the TOOL's answer (whichever layer produced it), never the MCP layer
//! alone. Moving the gate BELOW the first syscall is likewise invisible to
//! them.
//!
//! That leaves layer 1 with no behavioral judge at all, and a check nothing can
//! reach is a check the next reader deletes as dead. Its PRESENCE and POSITION
//! are pinned instead as SOURCE TEXT, by the third test below, scoped to
//! `read_bounded`'s first statement so that no comment and no prose elsewhere
//! can green it. Mutation-proven the other way: delete that `if` block, or move
//! it past the `fs::metadata` call, and the source-text test goes red while
//! these two stay green.
//!
//! RE-CUT under DR-058 §Decision 2 (ACCEPTED, owner, 2026-07-26): `cas_read`
//! moved behind the badge door, so every call below presents an ADMITTED
//! operator badge (the DR-045 re-cut precedent, disclosed by the record).
//! What this board judges therefore SHIFTS in meaning without any assertion
//! moving: the shape gate is no longer the only line an unauthenticated
//! caller meets (the door is — `dr058_cas_read_badge_door.rs`), it is the
//! MCP-layer half of §Decision 4's defence-in-depth, KEPT and still judged
//! byte-identical behind the door. The unbadged-caller history in the prose
//! above describes the pre-DR-058 threat this gate was cut against.

mod util;

use std::sync::Arc;

use rezidnt_cas::Cas;
use rezidnt_fabric::{EventLog, Fabric};
use rezidnt_mcp::{BadgeBook, MAX_CAS_READ_BYTES_DEFAULT, McpCore, codes};
use rezidnt_run::badge::Badge;
use serde_json::{Value, json};

/// A core with a WIRED CAS rooted at `<tmp>/cas`, ONE admitted operator badge
/// (DR-058 door, path 1), no substrate and no root key.
fn core_with_cas() -> (tempfile::TempDir, Arc<Cas>, Badge, Arc<McpCore>) {
    let dir = tempfile::tempdir().expect("tempdir");
    let log = EventLog::open(&dir.path().join("events.db")).expect("open log");
    let fabric = Fabric::new(log, 1024);
    let cas = Arc::new(Cas::open(&dir.path().join("cas")).expect("open cas"));
    let badge = Badge::mint().expect("mint badge");
    let mut book = BadgeBook::new();
    book.admit(&badge);
    let core = McpCore::new(fabric, book).with_cas(Arc::clone(&cas));
    (dir, cas, badge, Arc::new(core))
}

/// Args with an arbitrary `hash` and an otherwise VALID text ref (badge
/// admitted), so nothing but the address shape can be what refuses.
fn args(hash: &str, badge: &Badge) -> Value {
    json!({
        "hash": hash,
        "bytes": 21,
        "mime": "text/x-diff",
        "badge": badge.token_hex(),
    })
}

/// A deterministic ASCII blob of exactly `len` bytes.
fn text_of(len: usize) -> String {
    "0123456789abcdef".repeat(len / 16 + 1)[..len].to_string()
}

/// The five malformed shapes, each a distinct class of wrong. `TRAVERSAL` and
/// `ABSOLUTE` are the two that actually escape the CAS root; the other three are
/// strings that address nothing this daemon can hold.
const TRAVERSAL: &str = "../probe.txt";
const UPPERCASE: &str = "AA11BB22CC33DD44EE55FF660718293A4B5C6D7E8F90A1B2C3D4E5F607182930";
const TOO_SHORT: &str = "aa11bb22cc33dd44ee55ff660718293a4b5c6d7e8f90a1b2c3d4e5f60718293";
const NON_HEX: &str = "zz11bb22cc33dd44ee55ff660718293a4b5c6d7e8f90a1b2c3d4e5f607182930";

/// A REAL, well-formed address (64 lowercase hex) that this daemon does not
/// hold — the control for "valid shape, absent blob".
const ABSENT: &str = "aa11bb22cc33dd44ee55ff660718293a4b5c6d7e8f90a1b2c3d4e5f607182930";

/// THE GUARD — every malformed address is refused `cas.hash_invalid`, and every
/// refusal is BYTE-IDENTICAL. The traversal and absolute targets are planted as
/// REAL, OVER-BOUND files, so a tree without the guard answers `cas.too_large`
/// and prints the exact byte count; with it, all five answers are one answer.
#[tokio::test]
async fn every_malformed_address_refuses_identically() {
    let (dir, _cas, badge, core) = core_with_cas();

    // Plant real files at both escape targets, deliberately over the read bound
    // so the pre-guard behavior would have leaked their exact size.
    let leaky = text_of(MAX_CAS_READ_BYTES_DEFAULT as usize + 1);
    std::fs::write(dir.path().join("probe.txt"), &leaky).expect("plant traversal target");
    let absolute = dir.path().join("absolute_probe.txt");
    std::fs::write(&absolute, &leaky).expect("plant absolute target");
    let absolute = absolute.to_string_lossy().to_string();

    let shapes = [
        ("path traversal", TRAVERSAL),
        ("absolute path", absolute.as_str()),
        ("uppercase hex", UPPERCASE),
        ("wrong length", TOO_SHORT),
        ("non-hex character", NON_HEX),
    ];

    let mut payloads: Vec<(&str, Value)> = Vec::new();
    for (label, hash) in shapes {
        let result = util::tool_call(&core, 1, "cas_read", args(hash, &badge)).await;
        assert_eq!(
            result["isError"],
            json!(true),
            "{label}: a hash that is not a CAS address must be REFUSED: {result:#}"
        );
        let payload = util::tool_payload(&result);
        assert_eq!(
            payload["code"],
            json!(codes::CAS_HASH_INVALID),
            "{label}: refused on SHAPE, before any lookup — never a code that \
             claims the daemon looked at the store: {payload:#}"
        );
        assert!(
            payload.get("content").is_none(),
            "{label}: a refusal carries no content: {payload:#}"
        );
        let message = payload["message"].as_str().unwrap_or_default();
        assert!(
            !message.contains(&leaky.len().to_string()),
            "{label}: the refusal must not name the target's size — that byte \
             count IS the metadata oracle: {payload:#}"
        );
        assert!(
            !message.contains(hash),
            "{label}: the refusal echoes no part of the argument, so the five \
             shapes cannot be told apart by their messages: {payload:#}"
        );
        payloads.push((label, payload));
    }

    let (first_label, first) = &payloads[0];
    for (label, payload) in &payloads[1..] {
        assert_eq!(
            payload, first,
            "{label} must be indistinguishable from {first_label}: every \
             rejected shape answers with ONE refusal, so a caller learns \
             nothing it did not already know from its own argument"
        );
    }
}

/// THE ORACLE'S DEATH — the same traversal hash gets the SAME answer whether
/// the target file is absent, small, or over-bound. A refusal that does not move
/// when the filesystem moves carries zero bits about the filesystem.
///
/// Pre-guard this test is red three ways: absent answers `cas.not_found`, small
/// answers `cas.corrupt` (naming the target's real blake3), and over-bound
/// answers `cas.too_large` (naming its exact length).
#[tokio::test]
async fn the_refusal_does_not_move_when_the_filesystem_does() {
    let (dir, _cas, badge, core) = core_with_cas();
    let target = dir.path().join("probe.txt");

    let absent =
        util::tool_payload(&util::tool_call(&core, 1, "cas_read", args(TRAVERSAL, &badge)).await);

    std::fs::write(&target, b"a small secret").expect("plant small target");
    let small =
        util::tool_payload(&util::tool_call(&core, 2, "cas_read", args(TRAVERSAL, &badge)).await);

    std::fs::write(&target, text_of(MAX_CAS_READ_BYTES_DEFAULT as usize + 1))
        .expect("plant over-bound target");
    let large =
        util::tool_payload(&util::tool_call(&core, 3, "cas_read", args(TRAVERSAL, &badge)).await);

    assert_eq!(
        absent, small,
        "existence must not be observable: an absent target and a present one \
         answer identically"
    );
    assert_eq!(
        small, large,
        "size must not be observable: a 14-byte target and an over-bound one \
         answer identically"
    );
    assert_eq!(
        absent["code"],
        json!(codes::CAS_HASH_INVALID),
        "all three are the one shape refusal: {absent:#}"
    );
}

/// NON-VACUITY — the guard admits what it should. A well-formed address still
/// reaches the store and serves its blob whole, and a well-formed address the
/// daemon does not hold still answers `cas.not_found`, distinct from the shape
/// refusal. Without this, a guard that refused every read would pass the two
/// tests above.
#[tokio::test]
async fn a_well_formed_address_still_reaches_the_store() {
    let (_dir, cas, badge, core) = core_with_cas();
    let text = "--- a/lib.rs\n+++ b/lib.rs\n@@ -1 +1 @@\n-old\n+new\n";
    let stored = cas.put(text.as_bytes(), "text/x-diff").expect("put diff");

    let served = util::tool_call(
        &core,
        1,
        "cas_read",
        json!({
            "hash": stored.hash,
            "bytes": stored.bytes,
            "mime": stored.mime,
            "badge": badge.token_hex(),
        }),
    )
    .await;
    assert_ne!(
        served["isError"],
        json!(true),
        "a 64-lowercase-hex address is a CAS address and reads normally: {served:#}"
    );
    assert_eq!(util::tool_payload(&served)["content"], json!(text));

    let missing = util::tool_call(&core, 2, "cas_read", args(ABSENT, &badge)).await;
    util::assert_tool_refusal(&missing, codes::CAS_NOT_FOUND);
}

/// THE PREMISE, MADE NON-VACUOUS — the refusals this guard stands in FRONT of
/// really do carry the facts the threat model says they carry.
///
/// The whole case for checking shape before the first syscall is that what lies
/// downstream is fact-BEARING: `cas.too_large` states a byte count and
/// `cas.corrupt` states a content hash. Those are legitimate answers to a badged
/// caller about a blob inside the CAS — nothing here asks for them to change —
/// but they were, until this test, an ASSUMPTION stated only in prose (this
/// board's header, and `is_cas_address`'s doc). A tree that quietly made them
/// fact-free would leave both documents asserting a leak that no longer exists,
/// with nothing red. Now the assertion has a judge.
#[tokio::test]
async fn the_refusals_behind_the_guard_are_fact_bearing() {
    let (_dir, cas, badge, core) = core_with_cas();

    // SIZE. A well-formed in-CAS address over the read bound.
    let over = text_of(MAX_CAS_READ_BYTES_DEFAULT as usize + 1);
    let stored = cas
        .put(over.as_bytes(), "text/plain")
        .expect("put oversize");
    let refused = util::tool_call(
        &core,
        1,
        "cas_read",
        json!({
            "hash": stored.hash,
            "bytes": stored.bytes,
            "mime": stored.mime,
            "badge": badge.token_hex(),
        }),
    )
    .await;
    util::assert_tool_refusal(&refused, codes::CAS_TOO_LARGE);
    let message = util::tool_payload(&refused)["message"]
        .as_str()
        .expect("a refusal carries a message")
        .to_string();
    assert!(
        message.contains(&over.len().to_string()),
        "cas.too_large states the blob's EXACT size ({}) — that is the byte \
         count the shape guard exists to keep off an out-of-CAS target, and a \
         message that stopped stating it would silently retire half this \
         board's premise. Message: {message:?}",
        over.len()
    );

    // CONTENT HASH. Plant B's bytes at A's address; the refusal must name B.
    let addressed = cas
        .put(b"the content this address promises", "text/plain")
        .expect("put a");
    let planted = cas
        .put(b"entirely different bytes", "text/plain")
        .expect("put b");
    std::fs::write(
        cas.root().join(&addressed.hash),
        b"entirely different bytes",
    )
    .expect("plant corruption");
    let refused = util::tool_call(
        &core,
        2,
        "cas_read",
        json!({
            "hash": addressed.hash,
            "bytes": addressed.bytes,
            "mime": addressed.mime,
            "badge": badge.token_hex(),
        }),
    )
    .await;
    util::assert_tool_refusal(&refused, codes::CAS_CORRUPT);
    let message = util::tool_payload(&refused)["message"]
        .as_str()
        .expect("a refusal carries a message")
        .to_string();
    assert!(
        message.contains(&planted.hash),
        "cas.corrupt states the blake3 of the bytes actually found ({}) — the \
         leg that degrades a metadata oracle into a CONTENT oracle, and the \
         reason the shape guard cannot be judged as mere hygiene. Message: \
         {message:?}",
        planted.hash
    );
}

/// LAYER 1's ONLY JUDGE — a SOURCE-TEXT guard, scoped to the FIRST STATEMENT of
/// `read_bounded`.
///
/// Everything above judges the TOOL's answer, and the tool answers identically
/// with this check present or absent (see the header). POSITION is the whole
/// content of the claim: "before any syscall" stops being true the moment the
/// check moves below the first `fs::metadata`, and no served refusal would
/// change if it did. So the first line of the body that is neither blank nor a
/// comment is what this asserts. Comment lines are SKIPPED rather than
/// searched, which is the scoping that keeps the guard honest: prose naming
/// `is_cas_address` — including this doc comment — can never green it.
#[test]
fn the_address_check_is_read_bounded_s_first_statement() {
    let source = std::fs::read_to_string(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src/lib.rs"),
    )
    .expect("read rezidnt-mcp/src/lib.rs");

    let lines: Vec<&str> = source.lines().collect();
    let signature = lines
        .iter()
        .position(|l| l.trim_start().starts_with("fn read_bounded("))
        .expect("guard sanity: rezidnt-mcp defines `fn read_bounded`");
    let body = &lines[signature + 1..];

    let start = body
        .iter()
        .position(|l| {
            let trimmed = l.trim_start();
            !trimmed.is_empty() && !trimmed.starts_with("//")
        })
        .expect("guard sanity: read_bounded has a body");
    let first_statement = body[start].trim();

    assert!(
        first_statement.contains("is_cas_address(&addressed.hash)"),
        "read_bounded's FIRST statement must be the address-shape check — \
         DR-058 §Decision 4 KEEPS it as defence in depth, and deleting it \
         reddens nothing else on this board (the store answers one layer down \
         with the same code and the same message), so this assertion is the \
         only thing standing between that ruling and a tidy-up. First \
         statement found: {first_statement:?}"
    );
    assert!(
        first_statement.ends_with('{'),
        "guard sanity: the check is expected to open a block, so the refusal \
         below can be scoped to it. Got: {first_statement:?}"
    );

    // The check's OWN block, up to its closing brace — not the rest of the
    // function, so an unrelated `CAS_HASH_INVALID` further down cannot green
    // the refusal assertion.
    let end = start
        + body[start..]
            .iter()
            .position(|l| l.trim() == "}")
            .expect("guard sanity: the first statement's block closes")
        + 1;
    let guard_block = body[start..end].join("\n");
    assert!(
        guard_block.contains("codes::CAS_HASH_INVALID"),
        "the shape check must refuse `cas.hash_invalid` — a check that fell \
         through to a lookup, or answered a code implying one, would rebuild \
         the metadata oracle this board exists to kill. Block:\n{guard_block}"
    );
}
