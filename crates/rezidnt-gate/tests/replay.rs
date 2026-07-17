//! S4 oracle — debrief replay, the compliance sentence (doc §8, BINDING):
//! recorded verdicts re-execute from log + CAS; divergence between recorded
//! and replayed verdict raises an INTEGRITY ALARM (verifier nondeterminism
//! or log tampering). This property is what makes the audit trail evidence
//! rather than assertion.
//!
//! RED MODE: assert-red. `rezidnt_gate::replay` is `todo!()`-stubbed; every
//! test panics until it exists.
//!
//! v1 replay policy pinned here (oracle decision, stated in the work order):
//! - NATIVE verifiers are re-executed from the recorded `inputs` + CAS.
//! - EXEC verifiers are reported from the record (`replayed: None`) — their
//!   argv is not recorded on the v1 verdict payload, so re-execution would
//!   be a guess, and a guess is not evidence.
//! - Recorded `inconclusive` is honest can't-decide: never re-executed,
//!   never an alarm.
//!
//! Fixture CAS preimages (hashes computed with the reference blake3 crate,
//! independent of any rezidnt code — see spec/fixtures/README.md):
//! - diff `M\tsrc/checkout/cart.rs\n` (23 B) →
//!   1d50030ca17af09eb6fad0eadfb3492275bfc76635d0965260cde6bc685d785e

use std::path::PathBuf;

use rezidnt_cas::Cas;
use rezidnt_gate::{Verdict, replay};
use rezidnt_types::Event;

const DIFF_PREIMAGE: &[u8] = b"M\tsrc/checkout/cart.rs\n";
const DIFF_HASH: &str = "1d50030ca17af09eb6fad0eadfb3492275bfc76635d0965260cde6bc685d785e";

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

/// A temp CAS seeded with the diff blob the fixtures' recorded inputs pin.
fn seeded_cas() -> (tempfile::TempDir, Cas) {
    let dir = tempfile::tempdir().expect("tempdir");
    let cas = Cas::open(dir.path()).expect("open cas");
    let put = cas
        .put(DIFF_PREIMAGE, "text/plain")
        .expect("seed diff blob");
    assert_eq!(
        put.hash, DIFF_HASH,
        "oracle hash bug: preimage/hash mismatch"
    );
    (dir, cas)
}

/// The golden case: a recorded `pass` (diff-scope over the CAS-pinned diff)
/// re-executes to `pass`. Equality, no alarm — replayable debrief.
#[test]
fn recorded_verdicts_replay_to_equality() {
    let events = fixture_events("s4_replay_verified.jsonl");
    let (_dir, cas) = seeded_cas();

    let report = replay(&events, &cas).expect("replay runs");
    assert_eq!(report.alarms, vec![], "an honest log replays clean");
    assert_eq!(report.verdicts.len(), 1);
    let v = &report.verdicts[0];
    assert_eq!(v.verifier, "diff-scope");
    assert_eq!(v.recorded, Verdict::Pass);
    assert_eq!(
        v.replayed,
        Some(Verdict::Pass),
        "native verifiers are RE-EXECUTED, not echoed"
    );
}

/// The tampered case: the committed fixture records `fail` for diff-scope,
/// but re-execution over the committed CAS preimage yields `pass`. That
/// divergence is an INTEGRITY ALARM naming the verifier and both verdicts —
/// never silently reconciled in either direction.
#[test]
fn divergence_raises_integrity_alarm_naming_verifier_and_both_verdicts() {
    let events = fixture_events("s4_replay_divergence_alarm.jsonl");
    let (_dir, cas) = seeded_cas();

    let report = replay(&events, &cas).expect("replay runs");
    assert_eq!(
        report.alarms.len(),
        1,
        "recorded fail vs replayed pass MUST alarm (verifier bug or altered log)"
    );
    let alarm = &report.alarms[0];
    assert_eq!(alarm.verifier, "diff-scope");
    assert_eq!(alarm.recorded, Verdict::Fail);
    assert_eq!(alarm.replayed, Verdict::Pass);
    assert_eq!(alarm.run, "01S4D1VERGE000000000000R01");
    assert_eq!(alarm.gate, "pre_merge");
}

/// Recorded `inconclusive` (the S3 timeout fixture) is reported verbatim:
/// not re-executed, not coerced, and NOT an alarm (I6 — inconclusive routed
/// to a human is honest).
#[test]
fn inconclusive_records_are_reported_not_reexecuted() {
    let events = fixture_events("s3_gate_inconclusive.jsonl");
    let dir = tempfile::tempdir().expect("tempdir");
    let cas = Cas::open(dir.path()).expect("open cas");

    let report = replay(&events, &cas).expect("replay runs");
    assert_eq!(report.alarms, vec![], "inconclusive never alarms");
    assert_eq!(report.verdicts.len(), 1);
    let v = &report.verdicts[0];
    assert_eq!(v.verifier, "tests-pass");
    assert_eq!(v.recorded, Verdict::Inconclusive);
    assert_eq!(
        v.replayed, None,
        "nothing deterministic to reproduce — never re-executed"
    );
}
