//! DR-043 slice `secret-scan-native` ORACLE — the `secret-scan` native
//! verifier's pure VERDICT logic over the pinned added-content ref.
//!
//! DR-041 Decision 2 named `secret-scan` NATIVE (its verdict is a
//! deterministic, in-process, CAS-replayable computation over pinned inputs, not
//! a subprocess-natural tool). DR-043 makes that classification BUILDABLE by
//! amending the §8 native INPUT CONTRACT: the daemon git adapter `cas.put()`s the
//! diff's per-file ADDED CONTENT and exposes it as a NEW pinned input ref
//! `refs["content"]`, sitting NEXT TO the retained `refs["diff"]` path-status
//! summary. This board pins the native's verdict as a PURE FUNCTION of that
//! content ref — the property that keeps it native (I3/I6), NOT the live worktree
//! and NOT the path-only summary (DR-043 Decision 3).
//!
//! This is the SIBLING of `native_verifiers.rs` (the S4 native pack) — same
//! `NativeVerifier` shape, same CAS-ref inputs, same evidence-blob-to-CAS rule
//! (I2), same "same content-hashed inputs => same verdict" determinism (I6). It
//! is CROSS-PLATFORM by design (pure logic over content bytes, no socket, no
//! toolchain) — host `/vet` covers it, exactly like `native_verifiers.rs` and
//! `path_confinement_native_c3a.rs`.
//!
//! ## RED MODE — assert-red (the house `PathConfinement`/S4-skeleton precedent)
//!
//! `rezidnt_gate::SecretScan` EXISTS as an ORACLE STUB whose `verify` is
//! `todo!()` (see `crates/rezidnt-gate/src/lib.rs`), so every test here PANICS
//! ("not yet implemented") until the implementer writes the content scan. A test
//! that PASSED before the scan exists would be testing nothing (testing-oracles:
//! test honesty). The NAMED RED reason for every test below: `SecretScan::verify`
//! is the ORACLE STUB `todo!()` — the secret-scan content scan is not written.
//!
//! ## Fixture hygiene — planted FAKE secrets assembled at runtime
//!
//! The planted-secret tripwires (an `AKIA`-shaped token, a PEM `BEGIN PRIVATE
//! KEY` block) are ASSEMBLED FROM PARTS at runtime, never written as literal
//! credential strings in source — so the repo carries NO credential-shaped
//! literal (the house secret-guard). They are FAKE by construction (a documented
//! example token; a truncated fake PEM body). The SCANNER must still see the
//! reassembled bytes and fire.
//!
//! ## Pinned content-blob format (the oracle decision, stated for the implementer)
//!
//! `refs["content"]` is the per-file ADDED CONTENT of the diff. The format this
//! board pins — the shape the daemon git adapter emits (DR-043 Decision 2) and
//! the shape `SecretScan` reads:
//!
//! ```text
//! >>> <repo-relative-path>\n
//! <that file's added bytes...>\n
//! ```
//!
//! The `>>> <path>` banner is what lets a `fail` NAME the file (interrogability,
//! I6). The implementer owns the exact banner token; if it changes, this header
//! and the fixtures move with it (fixture hygiene).

use std::collections::BTreeMap;

use rezidnt_cas::Cas;
use rezidnt_gate::{NativeVerifier, SecretScan, Verdict, VerifierInput};
use serde_json::{Value, json};

// ---------------------------------------------------------------------------
// Fixtures — built IN-TEST (control the inputs; NO network, no worktree). Each
// is the pinned added-content blob for one diff, banner + bytes. Secret-shaped
// tokens are ASSEMBLED FROM PARTS (no credential literal in source).
// ---------------------------------------------------------------------------

/// Clean added content: an ordinary source change, no secret material.
fn content_clean() -> Vec<u8> {
    b">>> src/checkout/cart.rs\npub fn total(items: &[i64]) -> i64 { items.iter().sum() }\n"
        .to_vec()
}

/// Added content with a planted AWS-access-key-shaped FAKE token (an `AKIA...`
/// string, the canonical committed-secret tripwire), assembled at runtime so no
/// credential literal sits in source. The banner names the file.
fn content_aws_key() -> Vec<u8> {
    // "AKIA" + a 16-char uppercase/-digit body = the AWS access-key ID shape.
    let token = format!("{}{}", "AKIA", "IOSFODNN7EXAMPLE");
    format!(">>> src/config/aws.rs\nconst KEY: &str = \"{token}\";\n").into_bytes()
}

/// Added content with a planted PEM private-key block — the second canonical
/// committed-secret shape (a `BEGIN PRIVATE KEY` header). Header/footer assembled
/// at runtime; the body is a short FAKE base64 run.
fn content_private_key() -> Vec<u8> {
    let begin = format!("-----{} PRIVATE KEY-----", "BEGIN");
    let end = format!("-----{} PRIVATE KEY-----", "END");
    format!(">>> deploy/id_rsa\n{begin}\nMIIEvQIBADANBgFAKEbodyAAAA\n{end}\n").into_bytes()
}

/// A BINARY blob the scanner cannot faithfully read as text (NUL bytes + a
/// non-UTF-8 sequence). DR-043 Decision 4: unscannable => inconclusive, NEVER a
/// silent pass. NOTE the `AKIA` bytes are PRESENT after a NUL — a scanner that
/// naively UTF-8-lossy'd this and matched anyway would WRONGLY report a verdict;
/// the honest verdict is inconclusive (it cannot faithfully read the content).
fn content_binary() -> Vec<u8> {
    let mut v = b">>> assets/blob.bin\n".to_vec();
    v.extend_from_slice(&[0x00, 0x01, 0x02, 0xff, 0xfe, 0x00]);
    v.extend_from_slice(b"AKIA");
    v.extend_from_slice(&[0x00, 0x80, 0x81, b'\n']);
    v
}

/// Build a §8 input carrying ONLY `refs["content"]` (the DR-043 pinned added
/// content). `params` is passed through (the implementer may key a scan bound or
/// a pattern set off params; the tests do not require any).
fn content_input(ref_str: &str, params: Value) -> VerifierInput {
    VerifierInput {
        gate: "pre_merge".to_string(),
        workspace: None,
        refs: BTreeMap::from([("content".to_string(), ref_str.to_string())]),
        params,
        timeout_ms: 120_000,
    }
}

/// Temp CAS with `bytes` planted under `mime`; returns the cas + its
/// `cas:blake3:<hex>` ref string.
fn cas_with(bytes: &[u8], mime: &str) -> (tempfile::TempDir, Cas, String) {
    let dir = tempfile::tempdir().expect("tempdir");
    let cas = Cas::open(dir.path()).expect("open cas");
    let cas_ref = cas.put(bytes, mime).expect("put content blob");
    let ref_str = format!("cas:blake3:{}", cas_ref.hash);
    (dir, cas, ref_str)
}

/// Flatten a verifier output's evidence into one searchable string.
fn evidence_text(out: &rezidnt_gate::VerifierOutput) -> String {
    out.evidence
        .iter()
        .map(|e| format!("{} {}", e.kind, e.msg))
        .collect::<Vec<_>>()
        .join(" | ")
}

// ===========================================================================
// DR-041 secret-scan criterion — CLEAN CONTENT -> PASS
// ===========================================================================

/// Clean added content (no secret material) -> `pass`. Never coerced to fail.
/// RED reason: `SecretScan::verify` is the ORACLE STUB `todo!()`.
#[test]
fn clean_content_maps_to_pass() {
    let (_dir, cas, content) = cas_with(&content_clean(), "text/plain");
    let out = SecretScan
        .verify(&content_input(&content, json!({})), &cas)
        .expect("cannot-run is a verdict, not an engine error");
    assert_eq!(
        out.verdict,
        Verdict::Pass,
        "clean added content is a pass (DR-043 Decision 3)"
    );
}

// ===========================================================================
// DR-041 secret-scan criterion — COMMITTED SECRET -> FAIL, NAMING file + pattern
// ===========================================================================

/// A committed AWS-key-shaped secret in the added content -> `fail`, and the
/// evidence NAMES both the FILE (`src/config/aws.rs`) and the SECRET PATTERN that
/// fired (an aws-access-key class token) — a bare boolean is not enough
/// (interrogability, I6; DR-043 Decision 3). RED reason: ORACLE STUB.
#[test]
fn committed_aws_key_maps_to_fail_and_names_file_and_pattern() {
    let (_dir, cas, content) = cas_with(&content_aws_key(), "text/plain");
    let out = SecretScan
        .verify(&content_input(&content, json!({})), &cas)
        .expect("engine ok");

    assert_eq!(
        out.verdict,
        Verdict::Fail,
        "a committed secret in the added content is a fail (DR-043 Decision 3)"
    );
    assert!(
        !out.evidence.is_empty(),
        "a fail carries evidence (interrogability, I6)"
    );
    let joined = evidence_text(&out);
    assert!(
        joined.contains("src/config/aws.rs"),
        "the fail must NAME the offending file (interrogability, I6); evidence={:?}",
        out.evidence
    );
    // The SECRET PATTERN that fired must be named (WHICH shape tripped), not
    // merely "a secret". The implementer owns the label; we accept the class token.
    let names_pattern = joined.to_ascii_lowercase().contains("aws")
        || joined.contains("AKIA")
        || joined.to_ascii_lowercase().contains("access key")
        || joined.to_ascii_lowercase().contains("access-key");
    assert!(
        names_pattern,
        "the fail must NAME the secret pattern that fired — a bare boolean is not \
         enough (I6); evidence={:?}",
        out.evidence
    );
}

/// A committed PEM private-key block -> `fail`, evidence names the file and the
/// private-key pattern. Second canonical secret shape. RED reason: ORACLE STUB.
#[test]
fn committed_private_key_block_maps_to_fail_and_names_pattern() {
    let (_dir, cas, content) = cas_with(&content_private_key(), "text/plain");
    let out = SecretScan
        .verify(&content_input(&content, json!({})), &cas)
        .expect("engine ok");

    assert_eq!(
        out.verdict,
        Verdict::Fail,
        "a committed PEM private key is a fail"
    );
    let joined = evidence_text(&out);
    assert!(
        joined.contains("deploy/id_rsa"),
        "the fail names the offending file; evidence={:?}",
        out.evidence
    );
    let names_pattern = joined.to_ascii_lowercase().contains("private key")
        || joined.to_ascii_lowercase().contains("private-key")
        || joined.to_ascii_lowercase().contains("pem");
    assert!(
        names_pattern,
        "the fail names the private-key pattern that fired (I6); evidence={:?}",
        out.evidence
    );
}

/// The fail evidence blob goes to the CAS and rides as a ref (I2 — refs, never
/// bytes on the fact), and that blob itself names the offender. Mirrors
/// `native_verifiers.rs::diff_scope_fails_out_of_scope_and_evidence_names_the_path`.
/// RED reason: ORACLE STUB.
#[test]
fn fail_evidence_blob_lands_in_cas_and_names_the_secret() {
    let (_dir, cas, content) = cas_with(&content_aws_key(), "text/plain");
    let out = SecretScan
        .verify(&content_input(&content, json!({})), &cas)
        .expect("engine ok");
    assert_eq!(out.verdict, Verdict::Fail);

    let ev_ref = out
        .evidence
        .iter()
        .find_map(|e| e.cas_ref.as_deref())
        .expect("a fail's evidence blob lands in the CAS (I2)");
    let hash = ev_ref.strip_prefix("cas:blake3:").expect("cas ref shape");
    let blob = cas
        .get(&rezidnt_types::refs::CasRef {
            hash: hash.to_string(),
            bytes: 0,
            mime: "text/plain".to_string(),
        })
        .expect("evidence ref resolves");
    assert!(
        String::from_utf8_lossy(&blob).contains("src/config/aws.rs"),
        "the evidence blob itself names the offending file (I2 + interrogability)"
    );
}

// ===========================================================================
// DR-043(c) — INCONCLUSIVE-ON-UNSCANNABLE-CONTENT (never coerced to pass)
// ===========================================================================

/// Binary/non-text content the scanner cannot faithfully read -> `inconclusive`,
/// PROVABLY not coerced to pass (and not to fail). The blob even CONTAINS `AKIA`
/// bytes after a NUL, so a naive lossy scan could wrongly decide; the honest
/// verdict is inconclusive — it cannot faithfully read the content (DR-043
/// Decision 4, I6). RED reason: ORACLE STUB.
#[test]
fn binary_content_maps_to_inconclusive_not_coerced() {
    let (_dir, cas, content) = cas_with(&content_binary(), "application/octet-stream");
    let out = SecretScan
        .verify(&content_input(&content, json!({})), &cas)
        .expect("cannot-run is a verdict, not an engine error");

    assert_eq!(
        out.verdict,
        Verdict::Inconclusive,
        "content the scanner cannot faithfully read is inconclusive (DR-043 Decision 4)"
    );
    assert_ne!(
        out.verdict,
        Verdict::Pass,
        "unscannable content is NEVER coerced to a silent pass (I6) — the whole point"
    );
    assert_ne!(
        out.verdict,
        Verdict::Fail,
        "unscannable content is not a fail either — the scanner could not decide"
    );
}

/// A content blob EXCEEDING the scan bound -> `inconclusive` (not a silent pass).
/// DR-043 Decision 4 names "a blob exceeding a scan bound" alongside binary as the
/// unscannable case. The bound is the implementer's; this pins the BEHAVIOR:
/// over-bound content is inconclusive, never coerced. Built as a large CLEAN text
/// blob, so a pass would be the tempting-but-dishonest verdict (the un-scanned
/// tail is unproven). RED reason: ORACLE STUB.
#[test]
fn oversized_content_maps_to_inconclusive_not_pass() {
    // ~2 MiB of clean text, comfortably past any reasonable text scan bound. If
    // the scanner refuses to scan past a bound, it must say inconclusive — not
    // wave it through as "no secret found" (unproven for the un-scanned tail).
    let mut big = b">>> src/generated/data.rs\n".to_vec();
    big.extend(std::iter::repeat_n(b'x', 2 * 1024 * 1024));
    big.push(b'\n');
    let (_dir, cas, content) = cas_with(&big, "text/plain");

    let out = SecretScan
        .verify(&content_input(&content, json!({})), &cas)
        .expect("engine ok");
    assert_eq!(
        out.verdict,
        Verdict::Inconclusive,
        "content exceeding the scan bound is inconclusive, never a silent pass \
         (DR-043 Decision 4, I6)"
    );
    assert_ne!(
        out.verdict,
        Verdict::Pass,
        "over-bound content is not a proven pass"
    );
}

/// The `refs["content"]` blob ABSENT from the CAS is cannot-run -> `inconclusive`,
/// never pass/fail/error (the S4 native honesty rule, mirrors
/// `native_verifiers.rs::missing_cas_blob_is_inconclusive_not_pass`). RED reason:
/// ORACLE STUB.
#[test]
fn missing_content_blob_is_inconclusive_not_pass() {
    let dir = tempfile::tempdir().expect("tempdir");
    let cas = Cas::open(dir.path()).expect("open cas");
    let absent = "cas:blake3:0000000000000000000000000000000000000000000000000000000000000000";
    let out = SecretScan
        .verify(&content_input(absent, json!({})), &cas)
        .expect("cannot-run is a verdict, not an error");
    assert_eq!(out.verdict, Verdict::Inconclusive);
    assert_ne!(
        out.verdict,
        Verdict::Pass,
        "an absent content blob is never a pass"
    );
}

/// The native reads `refs["content"]`, NOT `refs["diff"]`. A §8 input carrying
/// ONLY the path-status summary under `refs["diff"]` (no `refs["content"]`) is
/// cannot-run -> inconclusive: the summary has NO file bytes, so a scanner cannot
/// see a secret (the exact gap DR-043 exists to close — a native reading only the
/// summary is the dishonest state). This forbids the lazy "read refs[\"diff\"]"
/// copy from the policy natives. RED reason: ORACLE STUB.
#[test]
fn diff_summary_only_input_is_inconclusive_never_pass() {
    // The path-status summary format the OTHER natives read — deliberately fed
    // under "diff", with NO "content" ref present.
    let (_dir, cas, summary) = cas_with(b"A\tsrc/config/aws.rs\n", "text/x-diff-summary");
    let input = VerifierInput {
        gate: "pre_merge".to_string(),
        workspace: None,
        refs: BTreeMap::from([("diff".to_string(), summary)]),
        params: json!({}),
        timeout_ms: 120_000,
    };
    let out = SecretScan
        .verify(&input, &cas)
        .expect("cannot-run is a verdict, not an error");
    assert_eq!(
        out.verdict,
        Verdict::Inconclusive,
        "the path-only summary has no bytes to scan — inconclusive, NEVER a silent \
         pass (DR-043: the native scans refs[\"content\"], not the summary)"
    );
    assert_ne!(
        out.verdict,
        Verdict::Pass,
        "a scanner that 'passes' on a summary it cannot read is the dishonest state \
         DR-043 closes"
    );
}

// ===========================================================================
// DR-041 secret-scan criterion — DETERMINISM (same content ref -> same verdict)
// ===========================================================================

/// Same content-hashed input -> same verdict AND same evidence (cost excluded).
/// Mirrors `native_verifiers.rs::same_refs_same_verdict_and_same_evidence`. This
/// is the same-inputs-same-verdict half of the I3 replay property (the re-fold
/// half lives in `secret_scan_replay_equivalence.rs`). RED reason: ORACLE STUB.
#[test]
fn same_content_ref_same_verdict_and_evidence() {
    let (_dir, cas, content) = cas_with(&content_aws_key(), "text/plain");
    let inp = content_input(&content, json!({}));
    let first = SecretScan.verify(&inp, &cas).expect("engine ok");
    let second = SecretScan.verify(&inp, &cas).expect("engine ok");
    assert_eq!(
        first.verdict, second.verdict,
        "same content ref -> same verdict (I6)"
    );
    assert_eq!(
        first.evidence, second.evidence,
        "evidence is deterministic, refs included (I6)"
    );
    assert_eq!(
        first.verdict,
        Verdict::Fail,
        "the planted secret is a stable fail"
    );
}

/// NATIVE classification (DR-041 Decision 2 / DR-043 Decision 1): `secret-scan`
/// is in the built-in NATIVE registry — it is NOT an exec verifier. This is the
/// "native — NOT exec" criterion pinned where the other natives are tested.
/// PASSES today (the ORACLE STUB is registered) — the flipped-positive guard
/// against a future reclassification to exec (which DR-043 explicitly rejected as
/// an I3/I6 regression). Kept load-bearing after the slice ships.
#[test]
fn secret_scan_is_a_registered_native_not_exec() {
    let present = rezidnt_gate::builtin_natives()
        .iter()
        .any(|n| n.name() == "secret-scan");
    assert!(
        present,
        "secret-scan must be a registered NATIVE (DR-041 Decision 2, DR-043 Decision 1) \
         so replay re-executes it (I3) — never reclassified to exec"
    );
}
