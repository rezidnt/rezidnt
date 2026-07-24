//! DR-043 slice `secret-scan-native` ORACLE — CRITERION (a), the INPUT-CONTRACT
//! PIN: at `pre_merge` the daemon exposes the NEW pinned input ref
//! `refs["content"]` (the diff's per-file ADDED CONTENT, `cas.put()`-ed by the
//! git adapter) ALONGSIDE the retained `refs["diff"]` path-status summary, and
//! the NATIVE `secret-scan` verifier scans THAT content — producing a genuine
//! `gate.passed` (clean) or `gate.failed` (a committed secret, NAMING the
//! verifier) fact, end-to-end through the real daemon pre_merge chain.
//!
//! This is the §8 native-input-shape AMENDMENT DR-043 owes a test for (DR-043
//! Consequences (a)). It is the daemon-side sibling of the pure-logic board
//! (`crates/rezidnt-gate/tests/secret_scan_native.rs`) and the replay board
//! (`.../secret_scan_replay_equivalence.rs`): those pin the native's verdict as a
//! pure function of a content ref; THIS pins that the daemon actually PINS that
//! content ref at pre_merge and hands it to the native.
//!
//! SIBLING of `verify_lints_e2e.rs` / `verify_subcommand_e2e.rs` (the DR-041 e2e
//! boards) and `golden_path.rs` (the S4 native pre_merge chain) — same
//! `start_daemon` + socket `tail` harness, same `gate.passed`/`gate.failed`
//! per-verifier shape. Unlike the lint/cargo-test boards this gate is NATIVE (no
//! exec argv), so the fixture names `native = "secret-scan"` directly.
//!
//! Unix-only (the S4 daemon-harness precedent — it drives the daemon over a
//! `UnixStream`). Host `/vet` compiles this to 0 tests
//! ([[vet-is-host-side-wsl-insufficient]]); it runs under WSL.
//!
//! ## RED MODE (RED-when-run, for TWO NAMED right reasons — both real today)
//!
//!  1. `secret-scan` is NOT dispatched by the daemon: `native_by_name` in
//!     `bins/rezidentd/src/gates.rs` (the match at ~L42) does NOT know
//!     `"secret-scan"`, so `resolve_one` SKIPS the spec entry
//!     (`gates.rs::resolve_one` warns + returns None on an unknown native) and
//!     the pre_merge gate runs with ZERO verifiers. It therefore emits a
//!     `gate.passed` with an EMPTY `verifiers` array and NO secret-scan record —
//!     so `find(verifier == "secret-scan")` PANICS on the clean leg, and the
//!     secret leg NEVER produces the `gate.failed` it waits for (the read blocks
//!     to its deadline and PANICS). NAMED reason: secret-scan native not wired
//!     into `native_by_name`.
//!
//!  2. `refs["content"]` is NOT emitted: the pre_merge refs map is built as
//!     `BTreeMap::from([("diff", diff_ref)])` at `bins/rezidentd/src/runs.rs:1576`
//!     and `gates::summarize_worktree` pins ONLY the path-status summary (no
//!     `cas.put()` of per-file content). So even once the native is dispatched,
//!     `inputs.refs["content"]` is ABSENT — the `content_ref_is_present…` and
//!     `pinned_content_matches_added_bytes` assertions FAIL. NAMED reason:
//!     refs["content"] not emitted yet.
//!
//! Once the implementer (a) wires `secret-scan` into `native_by_name` and (b)
//! makes `summarize_worktree`/`run_pre_merge` `cas.put()` the added content and
//! add `refs["content"]` to the pre_merge refs map, these go green on their own
//! merits (mirroring how `verify_subcommand_e2e.rs` went green when cargo-test
//! landed).
#![cfg(unix)]

mod common;

use std::time::Duration;

use common::{
    DaemonGuard, SecretScanFixture, connect, make_secret_scan_gated_project, read_until, run_cli,
    send_line, start_daemon,
};
use rezidnt_cas::Cas;
use rezidnt_types::refs::CasRef;
use serde_json::json;

/// Open a secret-scan-gated project spec and drive the pre_merge chain to its
/// terminal verdict fact, returning the live `DaemonGuard` (the caller MUST keep
/// it in scope across any `cas_bytes` read — its `TempDir` owns the CAS dir and
/// `remove_dir_all`s it on drop), the `tail` frames up to (and including) that
/// fact, AND the daemon's CAS root (the `REZIDNT_DB`-relative `dir/cas` default)
/// so a test can resolve the pinned `refs["content"]` blob.
fn run_to_pre_merge_verdict(
    spec: &str,
    stop_subject: &str,
) -> (DaemonGuard, Vec<serde_json::Value>, std::path::PathBuf) {
    let daemon = start_daemon();
    // The daemon's CAS default is a `cas` dir beside its db (the testkit's
    // documented REZIDNT_DB-relative default). Resolved before we move `daemon`.
    let cas_root = daemon.db.parent().expect("db has a parent dir").join("cas");

    let dir = tempfile::tempdir().expect("spec dir");
    let spec_path = dir.path().join("rezidnt.toml");
    std::fs::write(&spec_path, spec).expect("write spec");

    let out = run_cli(&daemon, &["open", spec_path.to_str().expect("utf8")]);
    assert_eq!(
        out.status.code(),
        Some(0),
        "gated open must succeed; stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );

    let mut tail = connect(&daemon.socket);
    send_line(&mut tail, r#"{"op":"tail"}"#);
    let frames = read_until(&mut tail, Duration::from_secs(60), move |v| {
        v["subject"] == stop_subject && v["payload"]["gate"] == json!("pre_merge")
    });
    (daemon, frames, cas_root)
}

/// Fetch a `cas:blake3:<hex>` ref's bytes from the daemon's CAS root.
fn cas_bytes(cas_root: &std::path::Path, ref_str: &str) -> Vec<u8> {
    let hash = ref_str
        .strip_prefix("cas:blake3:")
        .unwrap_or_else(|| panic!("content ref must be a cas:blake3: string; got {ref_str:?}"));
    let cas = Cas::open(cas_root).expect("open daemon cas");
    cas.get(&CasRef {
        hash: hash.to_string(),
        bytes: 0,
        mime: String::new(),
    })
    .unwrap_or_else(|e| panic!("pinned content ref {ref_str} must resolve in the daemon CAS: {e}"))
}

// ===========================================================================
// (a) INPUT-CONTRACT PIN — clean leg: content ref PRESENT + content-correct
// ===========================================================================

/// A CLEAN added change, gated on the native `secret-scan`, produces a genuine
/// `gate.passed` for `pre_merge` carrying a `secret-scan` record whose
/// `inputs.refs` has BOTH the retained `diff` summary ref AND the NEW `content`
/// ref (DR-043 Decision 2) — the two sitting side by side, distinct refs.
#[test]
fn e2e_clean_content_ref_is_present_alongside_diff() {
    let (_dir, spec) = make_secret_scan_gated_project(SecretScanFixture::Clean, 100);
    let (_daemon, lines, _cas_root) = run_to_pre_merge_verdict(&spec, "gate.passed");

    let passed = lines
        .iter()
        .rfind(|v| v["subject"] == "gate.passed" && v["payload"]["gate"] == json!("pre_merge"))
        .expect("a clean change lands a pre_merge gate.passed");
    let verifiers = passed["payload"]["verifiers"]
        .as_array()
        .expect("per-verifier records on gate.passed");
    let record = verifiers
        .iter()
        .find(|v| v["verifier"] == json!("secret-scan"))
        .unwrap_or_else(|| panic!("secret-scan record missing from {verifiers:#?}"));

    let refs = &record["inputs"]["refs"];
    assert!(
        refs["diff"]
            .as_str()
            .is_some_and(|r| r.starts_with("cas:blake3:")),
        "the retained diff path-status ref is still pinned (§8 BINDING); got {record:#?}"
    );
    let content = refs["content"].as_str();
    assert!(
        content.is_some_and(|r| r.starts_with("cas:blake3:")),
        "DR-043 Decision 2: refs[\"content\"] (a content CasRef) must be present \
         ALONGSIDE refs[\"diff\"]; got {record:#?}"
    );
    assert_ne!(
        refs["content"], refs["diff"],
        "the content ref is the ADDED CONTENT, a DISTINCT blob from the path-only \
         summary — not an alias of refs[\"diff\"]"
    );
}

/// The pinned `refs["content"]` blob is CONTENT-CORRECT: its bytes are the diff's
/// actual ADDED CONTENT — they contain the clean line the harness added
/// (`us-east-1`) and do NOT contain the seed base body. This is the "matches the
/// diff's actual added bytes" half of DR-043(a): the ref is content-addressed, so
/// resolving it and finding the added bytes proves the daemon pinned the real
/// content, not an empty/placeholder blob.
#[test]
fn e2e_pinned_content_matches_added_bytes_clean() {
    let (_dir, spec) = make_secret_scan_gated_project(SecretScanFixture::Clean, 100);
    let (_daemon, lines, cas_root) = run_to_pre_merge_verdict(&spec, "gate.passed");

    let passed = lines
        .iter()
        .rfind(|v| v["subject"] == "gate.passed" && v["payload"]["gate"] == json!("pre_merge"))
        .expect("a clean change lands a pre_merge gate.passed");
    let record = passed["payload"]["verifiers"]
        .as_array()
        .and_then(|vs| vs.iter().find(|v| v["verifier"] == json!("secret-scan")))
        .unwrap_or_else(|| panic!("secret-scan record missing from {passed:#?}"));
    let content_ref = record["inputs"]["refs"]["content"]
        .as_str()
        .unwrap_or_else(|| panic!("refs[\"content\"] must be present; got {record:#?}"));

    let bytes = cas_bytes(&cas_root, content_ref);
    let text = String::from_utf8_lossy(&bytes);
    assert!(
        text.contains("us-east-1"),
        "the pinned content must be the diff's ADDED bytes (the harness wrote \
         `us-east-1`); pinned content was {text:?}"
    );
}

// ===========================================================================
// secret leg: the native scans the pinned content, fails, and blocks the merge
// ===========================================================================

/// A change that COMMITS A SECRET, gated on the native `secret-scan`, produces a
/// genuine `gate.failed` for `pre_merge` naming `verifier = "secret-scan"` — a
/// REAL committed-secret detection over the PINNED CONTENT, not the path summary.
/// The merge does NOT happen (no `diff.merged`): the merge follows only a verified
/// pass. The pinned `refs["content"]` the native scanned contains the committed
/// secret token (content-correct on the fail leg too).
#[test]
fn e2e_committed_secret_fails_scanning_pinned_content_and_blocks_merge() {
    let (_dir, spec) = make_secret_scan_gated_project(SecretScanFixture::CommitsSecret, 100);
    let (_daemon, lines, cas_root) = run_to_pre_merge_verdict(&spec, "gate.failed");

    let failed = lines
        .iter()
        .rfind(|v| v["subject"] == "gate.failed" && v["payload"]["gate"] == json!("pre_merge"))
        .expect("a committed secret lands a pre_merge gate.failed");
    assert_eq!(
        failed["payload"]["verifier"], "secret-scan",
        "the failing verifier is named on gate.failed (§8 interrogability)"
    );
    assert!(
        !lines.iter().any(|v| v["subject"] == "diff.merged"),
        "a failing pre_merge blocks the merge — no diff.merged on the log"
    );

    // The content the native scanned is the pinned added CONTENT and carries the
    // committed secret token (DR-043 Decision 3 — the fail is over refs["content"],
    // not the path summary). The token is assembled from parts here too so no
    // credential literal sits in this test's source.
    let content_ref = failed["payload"]["inputs"]["refs"]["content"]
        .as_str()
        .unwrap_or_else(|| {
            panic!("refs[\"content\"] must be present on the fail; got {failed:#?}")
        });
    let bytes = cas_bytes(&cas_root, content_ref);
    let token = format!("{}{}", "AKIA", "IOSFODNN7EXAMPLE");
    assert!(
        String::from_utf8_lossy(&bytes).contains(&token),
        "the pinned content the native scanned must contain the committed secret \
         (proving the fail was over the CONTENT ref, not the path-only summary)"
    );
}

// ===========================================================================
// (c) NON-UTF-8-NUL-FREE CONTENT -> INCONCLUSIVE (the auditor's I6 gap: the
// production pinning path, not the pure-logic native, must carry RAW bytes)
// ===========================================================================

/// A non-UTF-8, NUL-FREE added file, gated on the native `secret-scan`, drives
/// the REAL daemon pre_merge chain to a `gate.inconclusive` for `pre_merge`
/// naming `verifier = "secret-scan"` — the honest "could-not-run" over content
/// the scanner cannot faithfully read (DR-043 Decision 4, I6) — and the merge
/// does NOT happen (no `diff.merged`), and it is NOT coerced to `gate.passed`.
///
/// This is the DAEMON-SIDE sibling of the pure-logic board's
/// `binary_content_maps_to_inconclusive_not_coerced`. That pure-logic test feeds
/// RAW non-UTF-8 bytes STRAIGHT to the native, so it passes even against a lossy
/// daemon — it never exercises the production PINNING path. The auditor's gap
/// (I6): the daemon pins added content, and if it does so with
/// `String::from_utf8_lossy` before `cas.put()`, a non-UTF-8 file with NO NUL
/// byte reaches the native as clean, NUL-free text — so the native's
/// "invalid-UTF-8 OR NUL -> inconclusive" guard can NEVER fire on the production
/// path, and genuinely non-text content is silently PASSED (or, if the lossy
/// U+FFFD text happens to trip the secret pattern, FAILED). Either coercion is
/// the exact behavior DR-043 Decision 4 forbids.
///
/// The `BinaryNoNul` fixture's bytes are chosen to DEFEAT a NUL-only binary
/// check: a lone `0xFF` (never a valid UTF-8 byte) plus stray continuation bytes
/// (`0x80..0x82`), wrapped in ASCII, with NO `0x00` anywhere. `from_utf8` rejects
/// them; a NUL scan does not.
///
/// ## RED MODE
///
/// RED against a LOSSY daemon (one that pins added content via
/// `String::from_utf8_lossy`): the pinned `refs["content"]` blob is then valid
/// UTF-8 (the `0xFF` became U+FFFD), so the native reads it as clean text and the
/// gate lands `gate.passed` (or `gate.failed` if the lossy text tripped the
/// pattern) — NEVER the `gate.inconclusive` this test waits for, so
/// `run_to_pre_merge_verdict` blocks to its 60s deadline and PANICS on the
/// stop-condition-never-met message. GREEN the moment the daemon pins RAW bytes
/// (the implementer's `git_added_content` fix): the native then sees the true
/// non-UTF-8 content and returns the honest inconclusive.
#[test]
fn e2e_binary_no_nul_content_maps_to_inconclusive() {
    let (_dir, spec) = make_secret_scan_gated_project(SecretScanFixture::BinaryNoNul, 100);
    // Bind the guard live: the read must not race the daemon's teardown, and no
    // CAS is read here so only `.socket`/`.db` are used (via the helper).
    let (_daemon, lines, _cas_root) = run_to_pre_merge_verdict(&spec, "gate.inconclusive");

    let inconclusive = lines
        .iter()
        .rfind(|v| {
            v["subject"] == "gate.inconclusive" && v["payload"]["gate"] == json!("pre_merge")
        })
        .expect(
            "non-UTF-8 (NUL-free) added content lands a pre_merge gate.inconclusive — the honest \
             could-not-run over unreadable content, NEVER a silent pass",
        );
    assert_eq!(
        inconclusive["payload"]["verifier"], "secret-scan",
        "the inconclusive names the secret-scan verifier (§8 interrogability, I6)"
    );

    // The reason is a genuine inconclusive class, NOT a coerced pass. A native's
    // cannot-run currently surfaces as `malformed_output` (the daemon's native
    // reason mapping); this asserts the honesty class, not the exact label, so
    // the test targets the coercion gap — not a reason-string detail the
    // implementer owns.
    let reason = inconclusive["payload"]["reason"].as_str().unwrap_or("");
    assert!(
        matches!(reason, "could_not_run" | "malformed_output"),
        "the inconclusive reason must be an honest could-not-run/unreadable class, \
         not a coercion; got {reason:?} in {inconclusive:#?}"
    );

    // The two coercions DR-043 Decision 4 forbids, asserted end-to-end: unreadable
    // content is NEVER a pass, and NEVER merges.
    assert!(
        !lines
            .iter()
            .any(|v| v["subject"] == "gate.passed" && v["payload"]["gate"] == json!("pre_merge")),
        "unreadable content is NEVER coerced to a pre_merge gate.passed (I6) — the whole point"
    );
    assert!(
        !lines.iter().any(|v| v["subject"] == "diff.merged"),
        "an inconclusive pre_merge blocks the merge — no diff.merged on the log (the merge \
         follows only a verified PASS)"
    );
}
