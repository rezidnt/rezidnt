//! DR-057 ORACLE — `cas_read` (DR-057 §Decision 2/4): the bounded, text-only,
//! refuse-never-chop CAS reader that closes Review.
//!
//! RE-CUT under DR-058 §Decision 2 (ACCEPTED, owner, 2026-07-26): `cas_read`
//! moved behind the badge door, so every call below presents an ADMITTED
//! operator badge — the same re-cut DR-045's `fan_out_door_and_width_cap.rs`
//! needed for a door added after a tool shipped, disclosed by the record
//! itself ("every existing `cas_read` test calling the tool with no badge
//! must be re-cut to present one"). EVERY content/bounds/mime/refusal
//! assertion of the original board survives intact; the door itself is judged
//! in `dr058_cas_read_badge_door.rs`, not here.
//!
//! THE LOAD-BEARING JUDGE lives here: over-bound content is REFUSED, never
//! silently chopped. A client must never be able to mistake a partial diff
//! for a whole one — a silent truncation makes a review surface worse than no
//! review surface, because it manufactures false confidence. The v1 corollary
//! this board pins on EVERY success: the served content is the WHOLE
//! addressed blob (`bytes_returned == ref.bytes == byte-length of content`)
//! and `truncated == false` — v1 has no partial reads at all, so a success
//! that says otherwise is either an inconsistency or a chop.
//!
//! ## API surface this board PINS (implementer builds to exactly this)
//!
//! - Tool `cas_read`, args = the full `CasRef` triple `{hash, bytes, mime}`
//!   at the TOP LEVEL of `arguments` — a client passes `diff_view`'s `diff`
//!   value straight through (plus, post-DR-058, its badge). Returns
//!   `{content, bytes_returned, truncated}`.
//! - `rezidnt_mcp::MAX_CAS_READ_BYTES_DEFAULT: u64` — the DEFAULT read
//!   bound, named after `MAX_FAN_OUT_DEFAULT` (one number, one place; `u64`
//!   because it bounds bytes, `CasRef.bytes`' own type). The check
//!   admits exactly-at-bound and refuses one-byte-over (a strict `>`), the
//!   fan-out cap's own semantics. The VALUE is a DEFAULT (DR-057: 256 KiB,
//!   cheap to revisit) and this board deliberately never asserts it — every
//!   test derives its blob from the const, so a revised DEFAULT moves no
//!   test. Only non-degeneracy is asserted (a zero bound would refuse every
//!   read and make the tool a lie).
//!
//! ## What DR-057 left open, disclosed rather than guessed
//!
//! - The refusal CODES minted here were ratified by DR-058 §Decision 1;
//!   `dr057_settled_gaps.rs` and `dr058_*` pin several exactly. This board
//!   still pins the MECHANISM (isError, a machine-readable non-empty `code`
//!   that is not a badge code, NO content in the refusal payload).
//! - The text-mime boundary is only sampled: `text/*` admits,
//!   `application/octet-stream` refuses. `application/json` is pinned
//!   NOT-text in `dr057_settled_gaps.rs`.
//! - A ref whose claimed `bytes` OVERSTATES a small blob is pinned SERVED in
//!   `dr057_settled_gaps.rs`. The lying UNDER-claim IS pinned (below): it
//!   must never smuggle over-bound content.

mod util;

use std::sync::Arc;

use rezidnt_cas::Cas;
use rezidnt_fabric::{EventLog, Fabric};
use rezidnt_mcp::{BadgeBook, MAX_CAS_READ_BYTES_DEFAULT, McpCore};
use rezidnt_run::badge::Badge;
use serde_json::{Value, json};

/// A core with a WIRED CAS (the daemon's own seam, `McpCore::with_cas`) and
/// ONE ADMITTED OPERATOR BADGE (the DR-058 door's path 1). No substrate, no
/// root key: only the read path exists to answer an admitted call.
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

/// Args = the ref triple at top level, exactly as `diff_view` serves it, plus
/// the admitted badge (DR-058 §Decision 2 re-cut).
fn ref_args(r: &rezidnt_types::refs::CasRef, badge: &Badge) -> Value {
    json!({
        "hash": r.hash,
        "bytes": r.bytes,
        "mime": r.mime,
        "badge": badge.token_hex(),
    })
}

/// The v1 success invariant: whole blob, consistent accounting, no partials.
fn assert_whole_read(payload: &Value, expected_text: &str) {
    assert_eq!(
        payload["content"],
        json!(expected_text),
        "content is the WHOLE addressed blob, byte-for-byte"
    );
    assert_eq!(
        payload["bytes_returned"].as_u64(),
        Some(expected_text.len() as u64),
        "bytes_returned equals the byte length of the served content: {payload:#}"
    );
    assert_eq!(
        payload["truncated"],
        json!(false),
        "v1 serves no partial reads — over-bound is REFUSED, so every success \
         is whole and says so: {payload:#}"
    );
}

/// The refusal invariant: machine-readable, honest about why, and carrying
/// ZERO bytes — a partial diff dressed as a refusal payload is still a chop.
fn assert_bytes_free_refusal(result: &Value, context: &str) -> String {
    assert_eq!(
        result["isError"],
        json!(true),
        "{context}: must be REFUSED: {result:#}"
    );
    let payload = util::tool_payload(result);
    let code = payload["code"].as_str().unwrap_or_else(|| {
        panic!("{context}: refusal carries a machine-readable code: {payload:#}")
    });
    assert!(!code.is_empty(), "{context}: the code is non-empty");
    assert!(
        code != rezidnt_mcp::codes::BADGE_REQUIRED && code != rezidnt_mcp::codes::BADGE_INVALID,
        "{context}: every call here presents an ADMITTED badge, so a refusal \
         never fails on badges (I6: a refusal never misstates why); got {code:?}"
    );
    assert!(
        payload.get("content").is_none(),
        "{context}: a refusal carries NO content — not even a prefix; partial \
         service inside a refusal is still a silent chop: {payload:#}"
    );
    code.to_string()
}

/// A deterministic ASCII blob of exactly `len` bytes.
fn text_of(len: usize) -> String {
    "0123456789abcdef".repeat(len / 16 + 1)[..len].to_string()
}

/// ROUND-TRIP + ADMIT LEG — a small text diff reads back whole on a
/// substrate-less core: the caller presents the exact ref `put` returned
/// (as `diff_view` would serve it) with its admitted badge and receives the
/// exact bytes back. Real content returning proves the read door was reached
/// and is open (DR-057 §Decision 4 as amended by DR-058 §Decision 2).
#[tokio::test]
async fn a_text_diff_round_trips_whole_and_badged() {
    let (_dir, cas, badge, core) = core_with_cas();
    let text = "--- a/lib.rs\n+++ b/lib.rs\n@@ -1 +1 @@\n-old\n+new\n";
    let r = cas.put(text.as_bytes(), "text/x-diff").expect("put diff");

    let result = util::tool_call(&core, 1, "cas_read", ref_args(&r, &badge)).await;
    assert_ne!(
        result["isError"],
        json!(true),
        "an admitted in-bound text read is ADMITTED: {result:#}"
    );
    assert_whole_read(&util::tool_payload(&result), text);
}

/// AT-BOUND ADMITTED (DR-057 §Decision 2, the adversarial edge) — a blob of
/// EXACTLY `MAX_CAS_READ_BYTES_DEFAULT` bytes is served whole. The bound is
/// an admit-through-here line, not a less-than: an implementation refusing
/// at-bound has narrowed the ruled DEFAULT by one byte.
#[tokio::test]
async fn a_blob_exactly_at_the_bound_is_admitted_whole() {
    // Non-degeneracy: a zero DEFAULT bound would refuse every read and make
    // the tool a lie. Deliberately a const assertion (the VALUE itself is
    // free, only degeneracy is fenced), hence the targeted allow.
    #[allow(clippy::assertions_on_constants)]
    {
        assert!(
            MAX_CAS_READ_BYTES_DEFAULT > 0,
            "a zero DEFAULT bound would refuse every read — degenerate, the \
             tool could never serve any diff"
        );
    }
    let (_dir, cas, badge, core) = core_with_cas();
    let text = text_of(MAX_CAS_READ_BYTES_DEFAULT as usize);
    let r = cas
        .put(text.as_bytes(), "text/x-diff")
        .expect("put at-bound blob");

    let result = util::tool_call(&core, 2, "cas_read", ref_args(&r, &badge)).await;
    assert_ne!(
        result["isError"],
        json!(true),
        "exactly-at-bound is ADMITTED (strict >, the MAX_FAN_OUT_DEFAULT \
         semantics): {result:#}"
    );
    assert_whole_read(&util::tool_payload(&result), &text);
}

/// THE LOAD-BEARING JUDGE — one byte over the bound is REFUSED as a whole:
/// isError, machine-readable code, and NOT ONE BYTE of content anywhere in
/// the reply. Silent truncation — a success carrying the first N bytes — is
/// the defect class that makes a review surface worse than none.
#[tokio::test]
async fn one_byte_over_the_bound_is_refused_never_chopped() {
    let (_dir, cas, badge, core) = core_with_cas();
    let text = text_of(MAX_CAS_READ_BYTES_DEFAULT as usize + 1);
    let r = cas
        .put(text.as_bytes(), "text/x-diff")
        .expect("put over-bound blob");

    let result = util::tool_call(&core, 3, "cas_read", ref_args(&r, &badge)).await;
    assert_bytes_free_refusal(&result, "one-byte-over-bound read");
}

/// A LYING UNDER-CLAIM cannot smuggle over-bound content: the blob is
/// over-bound in the store, the presented ref claims a tiny `bytes`. Any
/// success would either serve over-bound content (bound violated), chop
/// (forbidden), or fabricate — so the only honest answer is a refusal. The
/// bound governs CONTENT, not the caller's claim (DR-057 §Decision 2:
/// "over-bound content is REFUSED").
#[tokio::test]
async fn a_lying_under_bound_ref_cannot_smuggle_over_bound_content() {
    let (_dir, cas, badge, core) = core_with_cas();
    let text = text_of(MAX_CAS_READ_BYTES_DEFAULT as usize + 1);
    let real = cas
        .put(text.as_bytes(), "text/x-diff")
        .expect("put over-bound blob");

    let result = util::tool_call(
        &core,
        4,
        "cas_read",
        json!({
            "hash": real.hash,
            "bytes": 10,
            "mime": "text/x-diff",
            "badge": badge.token_hex(),
        }),
    )
    .await;
    assert_bytes_free_refusal(&result, "under-claimed over-bound read");
}

/// TEXT MIMES ONLY (DR-057 §Decision 2, v1) — a non-text mime is refused
/// with a plain code and zero bytes, never mangled into lossy UTF-8.
#[tokio::test]
async fn a_non_text_mime_is_refused_plainly() {
    let (_dir, cas, badge, core) = core_with_cas();
    let bytes: Vec<u8> = vec![0x89, 0x50, 0x4e, 0x47, 0x00, 0xff, 0xfe];
    let r = cas
        .put(&bytes, "application/octet-stream")
        .expect("put binary blob");

    let result = util::tool_call(&core, 5, "cas_read", ref_args(&r, &badge)).await;
    assert_bytes_free_refusal(&result, "non-text mime read");
}

/// A TEXT-CLAIMED blob whose bytes are NOT valid UTF-8 must never be served
/// via `from_utf8_lossy`: a JSON string cannot carry those bytes faithfully,
/// so any success IS a mangling (U+FFFD substitution — content that hashes to
/// something other than the addressed blob). Refusal is the only answer that
/// does not lie about the content.
#[tokio::test]
async fn claimed_text_with_invalid_utf8_is_never_served_lossy() {
    let (_dir, cas, badge, core) = core_with_cas();
    let bytes: Vec<u8> = vec![0xf0, 0x28, 0x8c, 0x28, 0x0a];
    let r = cas
        .put(&bytes, "text/plain")
        .expect("put invalid-utf8 blob");

    let result = util::tool_call(&core, 6, "cas_read", ref_args(&r, &badge)).await;
    assert_bytes_free_refusal(&result, "invalid-utf8 text-claimed read");
}

/// A MISSING blob is an honest refusal, never an empty success. An empty
/// `{content: "", truncated: false}` for an absent blob would fabricate a
/// zero-byte diff the log never pinned — the null-honesty leg's sibling.
#[tokio::test]
async fn a_missing_blob_is_refused_never_an_empty_success() {
    let (_dir, _cas, badge, core) = core_with_cas();
    let result = util::tool_call(
        &core,
        7,
        "cas_read",
        json!({
            "hash": "aa11bb22cc33dd44ee55ff660718293a4b5c6d7e8f90a1b2c3d4e5f607182930",
            "bytes": 21,
            "mime": "text/x-diff",
            "badge": badge.token_hex(),
        }),
    )
    .await;
    assert_bytes_free_refusal(&result, "missing-blob read");
}

/// A CORRUPT blob — bytes at the addressed path that do not hash to the
/// address — is refused, never served ("echoes and VERIFIES the caller's own
/// ref", DR-057 §Decision 2; `Cas::get` already refuses corruption, and the
/// tool must not route around it).
#[tokio::test]
async fn a_corrupt_blob_is_refused_never_served() {
    let (_dir, cas, badge, core) = core_with_cas();
    // Derive a REAL address via put, then overwrite the stored file with
    // different bytes: the path now holds content that does not hash to it.
    // (`root().join`, not `path_for`: the DR-058 §Decision 4 guard makes
    // `path_for` fallible, and this plant must not depend on either signature.)
    let promised = cas
        .put(b"the content this hash promises", "text/plain")
        .expect("put promised blob");
    std::fs::write(cas.root().join(&promised.hash), b"entirely different bytes")
        .expect("plant corrupt blob");

    // The caller presents its own HONEST ref, verbatim — the STORE is what
    // lies here, and the refusal must be about that.
    let result = util::tool_call(&core, 8, "cas_read", ref_args(&promised, &badge)).await;
    assert_bytes_free_refusal(&result, "corrupt-blob read");
}
