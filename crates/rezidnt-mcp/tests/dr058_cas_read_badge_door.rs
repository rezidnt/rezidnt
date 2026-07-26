//! DR-058 ORACLE — §Decision 2: `cas_read` moves behind the SAME dual-path
//! badge door every mutating tool uses (`check_badge`: an admitted operator
//! token OR a verified agent macaroon), refused `badge.required`/
//! `badge.invalid` BEFORE the mime check and BEFORE any filesystem call.
//!
//! ## The ordering is judged BEHAVIOURALLY, not by reading the source
//!
//! A door in the wrong place leaks through its refusals: if a badgeless
//! caller can tell an existing blob from an absent one, an over-bound one, a
//! corrupt one, a non-text claim, a malformed address, or a store-less core,
//! then something ran before the door. So the load-bearing judge here is
//! BYTE-IDENTITY of the refusal across ALL of those states — the same
//! equality-is-the-oracle's-death shape `dr057_cas_address_guard.rs` pinned
//! one layer down.
//!
//! ## Both admit paths, non-vacuously (the DR-055 fall-through pattern)
//!
//! An "admitted" assertion alone cannot distinguish a door that verifies from
//! a door that waves everything through, and a served-content assertion alone
//! cannot distinguish "the door admitted" from "there is no door". Each admit
//! leg therefore carries a deny CONTROL on the same core, and the CAS-less
//! fall-through legs prove the admitted call fell PAST the door to the
//! substrate check (`substrate.unavailable`) — door before store, the DR-055
//! substrate-less pattern.
//!
//! ## What this board deliberately does NOT pin
//!
//! - The macaroon VERB `cas_read`'s door derives (DR-058 rules the door, not
//!   the verb). The macaroons here carry NO `Verb` caveat, so they verify
//!   under any derivation; the deny controls are a foreign-root macaroon and
//!   a garbage token, which refuse under every derivation. When a verb is
//!   ruled, a narrowed-verb control tightens here.
//! - The refusal MESSAGE text. Only code + byte-identity across states.
//! - The args SCHEMA — that is `dr057_surface.rs`'s job, RE-CUT under the
//!   owner's in-place DR-058 correction (`81f437c`): the "shape is unchanged"
//!   clause was struck and `CasReadArgs` gains a declared, required `badge`
//!   (house pattern). This board's calls already present the badge at the
//!   TOP LEVEL of `arguments`, exactly where a declared field rides — the
//!   same wire shape `fan_out`/`kill_run` calls use.
//!
//! ## RED MODE (against the tree at cut time)
//!
//! ASSERT-RED: `cas_read` is unbadged today, so every no-badge call is served
//! or refused with a `cas.*` code (the byte-identity matrix fails on its
//! first state), a garbage badge is ignored (the `badge.invalid` legs fail),
//! and the CAS-less no-badge call answers `substrate.unavailable` instead of
//! `badge.required` (the door-before-substrate leg fails).

mod util;

use std::sync::Arc;

use rezidnt_cas::Cas;
use rezidnt_fabric::{EventLog, Fabric};
use rezidnt_mcp::{BadgeBook, MAX_CAS_READ_BYTES_DEFAULT, McpCore, codes};
use rezidnt_run::badge::{Badge, Macaroon, RootKey};
use serde_json::{Value, json};

/// One fixed root key for the macaroon leg; a DIFFERENT key mints the
/// foreign-root control.
fn daemon_root() -> RootKey {
    RootKey::from_bytes([7u8; 32])
}
fn foreign_root() -> RootKey {
    RootKey::from_bytes([8u8; 32])
}

/// A caveat-free agent macaroon: verifies under any verb/workspace/now, so
/// this board does not pin the door's (unruled) verb derivation.
fn agent_macaroon(root: &RootKey) -> Macaroon {
    Macaroon::mint(root, "run-01DR058CASREADDOOR000000", vec![])
}

/// A core with CAS + root key + one admitted operator badge — every door
/// path exists, so nothing about the CORE can excuse a refusal.
fn full_core(badge: &Badge) -> (tempfile::TempDir, Arc<Cas>, Arc<McpCore>) {
    let dir = tempfile::tempdir().expect("tempdir");
    let log = EventLog::open(&dir.path().join("events.db")).expect("open log");
    let fabric = Fabric::new(log, 1024);
    let cas = Arc::new(Cas::open(&dir.path().join("cas")).expect("open cas"));
    let mut book = BadgeBook::new();
    book.admit(badge);
    let core = McpCore::new(fabric, book)
        .with_root_key(daemon_root())
        .with_cas(Arc::clone(&cas));
    (dir, cas, Arc::new(core))
}

/// The SAME configuration minus the CAS — the fall-through core. Identical
/// otherwise, so a door refusal on this core must be byte-identical to one on
/// `full_core`.
fn casless_core(badge: &Badge) -> (tempfile::TempDir, Arc<McpCore>) {
    let dir = tempfile::tempdir().expect("tempdir");
    let log = EventLog::open(&dir.path().join("events.db")).expect("open log");
    let fabric = Fabric::new(log, 1024);
    let mut book = BadgeBook::new();
    book.admit(badge);
    let core = McpCore::new(fabric, book).with_root_key(daemon_root());
    (dir, Arc::new(core))
}

/// A deterministic ASCII blob of exactly `len` bytes.
fn text_of(len: usize) -> String {
    "0123456789abcdef".repeat(len / 16 + 1)[..len].to_string()
}

/// Well-formed address no store holds.
const ABSENT: &str = "aa11bb22cc33dd44ee55ff660718293a4b5c6d7e8f90a1b2c3d4e5f607182930";

/// The blob-state matrix: every distinguishable thing the store/argument can
/// be, each as ready-to-send args (no badge). Prepared on `cas`'s root, with
/// the traversal target planted beside it.
fn blob_states(dir: &tempfile::TempDir, cas: &Cas) -> Vec<(&'static str, Value)> {
    let present = cas
        .put(
            b"--- a/x.rs\n+++ b/x.rs\n@@ -1 +1 @@\n-a\n+b\n",
            "text/x-diff",
        )
        .expect("put present blob");
    let over = cas
        .put(
            text_of(MAX_CAS_READ_BYTES_DEFAULT as usize + 1).as_bytes(),
            "text/x-diff",
        )
        .expect("put over-bound blob");
    let corrupt = cas
        .put(b"the content this address promises", "text/plain")
        .expect("put to-be-corrupted blob");
    std::fs::write(cas.root().join(&corrupt.hash), b"entirely different bytes")
        .expect("plant corruption");
    let binary = cas
        .put(
            &[0x89, 0x50, 0x4e, 0x47, 0x00, 0xff],
            "application/octet-stream",
        )
        .expect("put binary blob");
    std::fs::write(
        dir.path().join("probe.txt"),
        text_of(MAX_CAS_READ_BYTES_DEFAULT as usize + 1),
    )
    .expect("plant traversal target");

    vec![
        (
            "existing blob",
            json!({"hash": present.hash, "bytes": present.bytes, "mime": present.mime}),
        ),
        (
            "absent blob",
            json!({"hash": ABSENT, "bytes": 21, "mime": "text/x-diff"}),
        ),
        (
            "over-bound blob",
            json!({"hash": over.hash, "bytes": over.bytes, "mime": over.mime}),
        ),
        (
            "corrupt blob",
            json!({"hash": corrupt.hash, "bytes": corrupt.bytes, "mime": corrupt.mime}),
        ),
        (
            "non-text mime claim",
            json!({"hash": binary.hash, "bytes": binary.bytes, "mime": binary.mime}),
        ),
        (
            "malformed (traversal) address",
            json!({"hash": "../probe.txt", "bytes": 21, "mime": "text/x-diff"}),
        ),
    ]
}

/// Merge a badge argument (or none) into ref args.
fn with_badge(mut args: Value, badge: Option<&str>) -> Value {
    if let Some(token) = badge {
        args["badge"] = json!(token);
    }
    args
}

/// Collect the FULL serialized tool result for one call — byte-identity is
/// compared on the wire shape, not on a projection of it.
async fn refusal_bytes(core: &McpCore, args: Value) -> (Value, String) {
    let result = util::tool_call(core, 1, "cas_read", args).await;
    let bytes = serde_json::to_string(&result).expect("result serializes");
    (result, bytes)
}

/// THE DOOR-PLACEMENT JUDGE — with NO badge, every blob state refuses
/// `badge.required`, and every refusal is BYTE-IDENTICAL, including on a core
/// with no CAS at all. If any state (existence, size, corruption, mime,
/// address shape, store presence) moves the answer, something ran before the
/// door and the door is in the wrong place.
#[tokio::test]
async fn an_unbadged_call_refuses_byte_identically_across_every_blob_state() {
    let operator = Badge::mint().expect("mint badge");
    let (dir, cas, core) = full_core(&operator);
    let (_dir2, casless) = casless_core(&operator);

    let mut answers: Vec<(&'static str, Value, String)> = Vec::new();
    for (label, args) in blob_states(&dir, &cas) {
        let (result, bytes) = refusal_bytes(&core, with_badge(args, None)).await;
        util::assert_tool_refusal(&result, codes::BADGE_REQUIRED);
        answers.push((label, result, bytes));
    }
    // The store-less core: the door must refuse before the substrate check,
    // so this refusal is the same bytes as all the others.
    let (result, bytes) = refusal_bytes(
        &casless,
        json!({"hash": ABSENT, "bytes": 21, "mime": "text/x-diff"}),
    )
    .await;
    util::assert_tool_refusal(&result, codes::BADGE_REQUIRED);
    answers.push(("store-less core", result, bytes));

    let (first_label, _, first_bytes) = &answers[0];
    for (label, result, bytes) in &answers[1..] {
        assert_eq!(
            bytes, first_bytes,
            "{label} must refuse byte-identically to {first_label} — a \
             badgeless caller who can distinguish blob states has a door \
             standing AFTER a lookup: {result:#}"
        );
    }
}

/// The same matrix under a BAD badge: `badge.invalid`, byte-identical across
/// every state. A garbage token is neither an admitted operator token nor a
/// parseable macaroon on this core.
#[tokio::test]
async fn a_bad_badge_refuses_badge_invalid_byte_identically_across_every_blob_state() {
    let operator = Badge::mint().expect("mint badge");
    let (dir, cas, core) = full_core(&operator);

    let mut answers: Vec<(&'static str, Value, String)> = Vec::new();
    for (label, args) in blob_states(&dir, &cas) {
        let (result, bytes) = refusal_bytes(&core, with_badge(args, Some("not-a-badge"))).await;
        util::assert_tool_refusal(&result, codes::BADGE_INVALID);
        answers.push((label, result, bytes));
    }
    let (first_label, _, first_bytes) = &answers[0];
    for (label, result, bytes) in &answers[1..] {
        assert_eq!(
            bytes, first_bytes,
            "{label} must refuse byte-identically to {first_label} under a bad \
             badge — `badge.invalid` may not carry blob facts either: {result:#}"
        );
    }
}

/// ADMIT PATH 1, non-vacuous — an admitted OPERATOR TOKEN reaches the store:
/// the exact planted bytes come back. The deny control on the SAME core (same
/// args, no badge) is what makes the admission mean something.
#[tokio::test]
async fn an_admitted_operator_token_reaches_the_store_and_reads() {
    let operator = Badge::mint().expect("mint badge");
    let (_dir, cas, core) = full_core(&operator);
    let text = "--- a/door.rs\n+++ b/door.rs\n@@ -1 +1 @@\n-shut\n+open\n";
    let r = cas.put(text.as_bytes(), "text/x-diff").expect("put diff");
    let args = json!({"hash": r.hash, "bytes": r.bytes, "mime": r.mime});

    let denied = util::tool_call(&core, 1, "cas_read", args.clone()).await;
    util::assert_tool_refusal(&denied, codes::BADGE_REQUIRED);

    let served = util::tool_call(
        &core,
        2,
        "cas_read",
        with_badge(args, Some(&operator.token_hex())),
    )
    .await;
    assert_ne!(
        served["isError"],
        json!(true),
        "an admitted operator token is ADMITTED — path 1 of the dual door \
         (DR-058 §Decision 2): {served:#}"
    );
    assert_eq!(
        util::tool_payload(&served)["content"],
        json!(text),
        "and the admitted call REACHED the store: the planted bytes come back \
         whole, so the admit path is not vacuous"
    );
}

/// ADMIT PATH 2, non-vacuous — a VERIFIED AGENT MACAROON (minted under this
/// core's root key) reaches the store; a FOREIGN-ROOT macaroon is refused
/// `badge.invalid` and reads nothing. No operator badge exists on this core,
/// so path 1 cannot be what admitted.
#[tokio::test]
async fn a_verified_agent_macaroon_reaches_the_store_and_a_foreign_root_does_not() {
    let dir = tempfile::tempdir().expect("tempdir");
    let log = EventLog::open(&dir.path().join("events.db")).expect("open log");
    let fabric = Fabric::new(log, 1024);
    let cas = Arc::new(Cas::open(&dir.path().join("cas")).expect("open cas"));
    // EMPTY badge book, deliberately: only the macaroon leg can admit.
    let core = Arc::new(
        McpCore::new(fabric, BadgeBook::new())
            .with_root_key(daemon_root())
            .with_cas(Arc::clone(&cas)),
    );
    let text = "--- a/lead.rs\n+++ b/lead.rs\n@@ -1 +1 @@\n-x\n+y\n";
    let r = cas.put(text.as_bytes(), "text/x-diff").expect("put diff");
    let args = json!({"hash": r.hash, "bytes": r.bytes, "mime": r.mime});

    let foreign = agent_macaroon(&foreign_root());
    let denied = util::tool_call(
        &core,
        1,
        "cas_read",
        with_badge(args.clone(), Some(&foreign.to_wire())),
    )
    .await;
    util::assert_tool_refusal(&denied, codes::BADGE_INVALID);

    let verified = agent_macaroon(&daemon_root());
    let served = util::tool_call(
        &core,
        2,
        "cas_read",
        with_badge(args, Some(&verified.to_wire())),
    )
    .await;
    assert_ne!(
        served["isError"],
        json!(true),
        "a macaroon verified against the daemon root key is ADMITTED — path 2 \
         of the dual door (DR-058 §Decision 2): {served:#}"
    );
    assert_eq!(
        util::tool_payload(&served)["content"],
        json!(text),
        "and the admitted macaroon call REACHED the store"
    );
}

/// THE FALL-THROUGH (DR-055 pattern) — on a store-less core, the door still
/// runs FIRST: no badge is `badge.required`, and BOTH admit paths fall past
/// the door to the honest `substrate.unavailable`. That code arriving only on
/// a badged call is the proof the door admitted and stands BEFORE the
/// substrate check — never after it.
#[tokio::test]
async fn both_admit_paths_fall_through_a_storeless_core_to_substrate_unavailable() {
    let operator = Badge::mint().expect("mint badge");
    let (_dir, core) = casless_core(&operator);
    let args = json!({"hash": ABSENT, "bytes": 21, "mime": "text/x-diff"});

    let unbadged = util::tool_call(&core, 1, "cas_read", args.clone()).await;
    util::assert_tool_refusal(&unbadged, codes::BADGE_REQUIRED);

    let operator_admitted = util::tool_call(
        &core,
        2,
        "cas_read",
        with_badge(args.clone(), Some(&operator.token_hex())),
    )
    .await;
    util::assert_tool_refusal(&operator_admitted, codes::SUBSTRATE_UNAVAILABLE);

    let macaroon = agent_macaroon(&daemon_root());
    let macaroon_admitted = util::tool_call(
        &core,
        3,
        "cas_read",
        with_badge(args, Some(&macaroon.to_wire())),
    )
    .await;
    util::assert_tool_refusal(&macaroon_admitted, codes::SUBSTRATE_UNAVAILABLE);
}

/// DEFENCE-IN-DEPTH ORDERING (§Decision 4: the door's `is_cas_address` is
/// KEPT) — behind an admitted badge, the mime gate and the address-shape gate
/// still refuse exactly as `dr057_cas_read.rs`/`dr057_cas_address_guard.rs`
/// pin; without a badge, neither is reachable. The badged legs keep the crate
/// guard's arrival from becoming the reason the door boards pass vacuously:
/// the MCP-layer refusals stay observable THROUGH the door.
#[tokio::test]
async fn the_mime_and_shape_gates_stand_behind_the_door_not_before_it() {
    let operator = Badge::mint().expect("mint badge");
    let (_dir, cas, core) = full_core(&operator);
    let binary = cas
        .put(&[0x00, 0x01, 0xff], "application/octet-stream")
        .expect("put binary blob");

    // Unbadged: the door answers, never the inner gates.
    let unbadged_mime = util::tool_call(
        &core,
        1,
        "cas_read",
        json!({"hash": binary.hash, "bytes": binary.bytes, "mime": binary.mime}),
    )
    .await;
    util::assert_tool_refusal(&unbadged_mime, codes::BADGE_REQUIRED);
    let unbadged_shape = util::tool_call(
        &core,
        2,
        "cas_read",
        json!({"hash": "../probe.txt", "bytes": 21, "mime": "text/x-diff"}),
    )
    .await;
    util::assert_tool_refusal(&unbadged_shape, codes::BADGE_REQUIRED);

    // Badged: the inner gates still answer for themselves.
    let badged_mime = util::tool_call(
        &core,
        3,
        "cas_read",
        with_badge(
            json!({"hash": binary.hash, "bytes": binary.bytes, "mime": binary.mime}),
            Some(&operator.token_hex()),
        ),
    )
    .await;
    util::assert_tool_refusal(&badged_mime, codes::CAS_NOT_TEXT);
    let badged_shape = util::tool_call(
        &core,
        4,
        "cas_read",
        with_badge(
            json!({"hash": "../probe.txt", "bytes": 21, "mime": "text/x-diff"}),
            Some(&operator.token_hex()),
        ),
    )
    .await;
    util::assert_tool_refusal(&badged_shape, codes::CAS_HASH_INVALID);
}
