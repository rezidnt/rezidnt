//! DR-043 slice `secret-scan-native` ORACLE — the CAS-REPLAY-EQUIVALENCE board
//! (DR-043 Consequences (b), the I3 property the whole record turns on).
//!
//! DR-043 keeps `secret-scan` NATIVE precisely because its verdict is a PURE
//! FUNCTION of a CAS-pinned content ref — replay-equivalent by construction. The
//! rejected exec-with-cwd alternative would have made the verdict depend on
//! mutable worktree state, forfeiting exactly this. This board PINS the property
//! two ways:
//!
//!  1. RE-FOLD (the compliance path): a recorded `secret-scan` verdict, folded
//!     from a committed event-log fixture, RE-EXECUTES to the SAME verdict from
//!     `log + CAS` alone (via `rezidnt_gate::replay`) — no integrity alarm. This
//!     is the debrief/replay sentence applied to secret-scan: the verdict is
//!     reconstructable from the pinned content ref, not from a live tree.
//!
//!  2. DOUBLE-COMPUTE (the same-ref path): feeding the SAME `refs["content"]`
//!     ref to the native twice yields byte-identical outputs — the direct
//!     "same content ref twice -> same verdict" half.
//!
//! SIBLING of `replay.rs` (the S4 replay board) — same fixture-fold + seeded-CAS
//! shape. CROSS-PLATFORM (pure fold over log + CAS, no socket/toolchain).
//!
//! ## RED MODE — assert-red
//!
//! `secret-scan` IS a registered native (an ORACLE STUB), so `replay` FINDS it
//! and RE-EXECUTES it — calling `SecretScan::verify`, which is `todo!()`. So
//! `replay(...)` PANICS ("not yet implemented") inside the native re-execution,
//! and the double-compute test PANICS the same way. NAMED RED reason: the
//! `SecretScan::verify` content scan is the ORACLE STUB `todo!()` — replay
//! re-executes it and hits the stub. Once the scan lands, the recorded verdicts
//! re-execute to equality on their own merits (mirroring how `replay.rs` went
//! green when the natives landed).
//!
//! ## Fixture CAS preimages (hashes computed via the crate's own CAS put, the
//! ## independent-of-scan reference — see the temp-hash note in the session log)
//!
//! - CLEAN content (83 B) ->
//!   fcfed777f565e32baa4ce3eb861fa7bdfec0c14a59ff2bce9938de05d0534543
//!   preimage: `>>> src/checkout/cart.rs\npub fn total(...) { ... }\n`
//! - COMMITTED-SECRET content (64 B) ->
//!   0b2c4b8a1aa4632dbc247d7983f238bc03444465ed121b2b29038e3ee475bd49
//!   preimage: `>>> src/config/aws.rs\nconst KEY = "AKIA...";\n` (FAKE token,
//!   assembled at runtime so no credential literal sits in source).

use std::collections::BTreeMap;
use std::path::PathBuf;

use rezidnt_cas::Cas;
use rezidnt_gate::{NativeVerifier, SecretScan, Verdict, VerifierInput, replay};
use rezidnt_types::Event;
use serde_json::json;

const CLEAN_HASH: &str = "fcfed777f565e32baa4ce3eb861fa7bdfec0c14a59ff2bce9938de05d0534543";
const SECRET_HASH: &str = "0b2c4b8a1aa4632dbc247d7983f238bc03444465ed121b2b29038e3ee475bd49";

/// The CLEAN content preimage the clean-replay fixture pins.
fn clean_preimage() -> Vec<u8> {
    b">>> src/checkout/cart.rs\npub fn total(items: &[i64]) -> i64 { items.iter().sum() }\n"
        .to_vec()
}

/// The COMMITTED-SECRET content preimage the committed-replay fixture pins. The
/// `AKIA...` token is assembled from parts (no credential literal in source).
fn secret_preimage() -> Vec<u8> {
    let token = format!("{}{}", "AKIA", "IOSFODNN7EXAMPLE");
    format!(">>> src/config/aws.rs\nconst KEY: &str = \"{token}\";\n").into_bytes()
}

fn fixture_events(name: &str) -> Vec<Event> {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../spec/fixtures")
        .join(name);
    std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("fixture {} must exist: {e}", path.display()))
        .lines()
        .filter(|l| !l.trim().is_empty())
        .map(|l| Event::from_json_line(l).unwrap_or_else(|e| panic!("{name}: bad line ({e}): {l}")))
        .collect()
}

/// A temp CAS seeded with a content preimage; asserts the returned hash matches
/// the fixture-pinned hash (an oracle-hash-bug tripwire, mirrors `replay.rs`).
fn cas_seeded_with(preimage: &[u8], expect_hash: &str) -> (tempfile::TempDir, Cas) {
    let dir = tempfile::tempdir().expect("tempdir");
    let cas = Cas::open(dir.path()).expect("open cas");
    let put = cas.put(preimage, "text/plain").expect("seed content blob");
    assert_eq!(
        put.hash, expect_hash,
        "oracle hash bug: content preimage/hash mismatch (fixture drift)"
    );
    (dir, cas)
}

// ===========================================================================
// (1) RE-FOLD — recorded verdict re-executes to equality from log + CAS
// ===========================================================================

/// A recorded `secret-scan` PASS over the pinned CLEAN content ref re-executes
/// (re-folds) to `pass` — equality, no integrity alarm. This is the I3
/// replay-equivalence property DR-043 turns on: the verdict is reconstructable
/// from the pinned content ref alone, not from a live worktree. RED reason:
/// `replay` re-executes the native and hits `SecretScan::verify`'s `todo!()`.
#[test]
fn recorded_clean_pass_refolds_to_equality_no_alarm() {
    let events = fixture_events("dr043_secret_scan_clean_replay.jsonl");
    let (_dir, cas) = cas_seeded_with(&clean_preimage(), CLEAN_HASH);

    let report = replay(&events, &cas).expect("replay runs");
    assert_eq!(
        report.alarms,
        vec![],
        "a clean secret-scan pass re-folds to equality — no divergence (I3)"
    );
    let v = report
        .verdicts
        .iter()
        .find(|v| v.verifier == "secret-scan")
        .expect("the secret-scan record is replayed");
    assert_eq!(v.recorded, Verdict::Pass);
    assert_eq!(
        v.replayed,
        Some(Verdict::Pass),
        "secret-scan is NATIVE -> RE-EXECUTED from log + CAS, not echoed (the \
         property that keeps it native, DR-043 Decision 1)"
    );
}

/// A recorded `secret-scan` FAIL over the pinned COMMITTED-SECRET content ref
/// re-executes to `fail` — equality, no alarm. The fail leg of the same I3
/// property: the same content bytes deterministically reproduce the fail. RED
/// reason: replay re-executes the native and hits the `todo!()` stub.
#[test]
fn recorded_committed_fail_refolds_to_equality_no_alarm() {
    let events = fixture_events("dr043_secret_scan_committed_replay.jsonl");
    let (_dir, cas) = cas_seeded_with(&secret_preimage(), SECRET_HASH);

    let report = replay(&events, &cas).expect("replay runs");
    assert_eq!(
        report.alarms,
        vec![],
        "a committed-secret fail re-folds to the SAME fail — no divergence (I3)"
    );
    let v = report
        .verdicts
        .iter()
        .find(|v| v.verifier == "secret-scan")
        .expect("the secret-scan record is replayed");
    assert_eq!(v.recorded, Verdict::Fail);
    assert_eq!(
        v.replayed,
        Some(Verdict::Fail),
        "the committed secret re-executes to fail from the pinned content ref alone"
    );
}

// ===========================================================================
// (2) DOUBLE-COMPUTE — same content ref twice -> byte-identical output
// ===========================================================================

/// Feeding the SAME `refs["content"]` ref to the native TWICE yields IDENTICAL
/// output (verdict + evidence). The direct "same content ref twice -> same
/// verdict" half of DR-043(b), independent of the log fold. RED reason: ORACLE
/// STUB `todo!()`.
#[test]
fn same_content_ref_recomputes_identically() {
    let (_dir, cas) = cas_seeded_with(&secret_preimage(), SECRET_HASH);
    let input = VerifierInput {
        gate: "pre_merge".to_string(),
        workspace: None,
        refs: BTreeMap::from([("content".to_string(), format!("cas:blake3:{SECRET_HASH}"))]),
        params: json!({}),
        timeout_ms: 120_000,
    };

    let first = SecretScan.verify(&input, &cas).expect("engine ok");
    let second = SecretScan.verify(&input, &cas).expect("engine ok");
    assert_eq!(
        first.verdict, second.verdict,
        "same content ref -> same verdict (I3/I6)"
    );
    assert_eq!(
        first.evidence, second.evidence,
        "same content ref -> byte-identical evidence, refs included (I3/I6)"
    );
}
