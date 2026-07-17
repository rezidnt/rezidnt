//! Oracle (S4-debrief LOW, the exec-flake root cause): a verifier that COULD
//! NOT RUN is not the same inconclusive as a verifier that ran and produced
//! MALFORMED output. Today `ExecVerifier::run_inner` maps a `spawn()` failure
//! (and a wait-io failure) onto `InconclusiveReason::MalformedOutput` — but
//! nothing ran, so "malformed output" is untruthful. I6 says a verifier that
//! cannot decide yields `inconclusive`, never `pass` — that stays; this pin
//! only sharpens the REASON so an operator can tell "your argv is wrong" from
//! "your program printed garbage."
//!
//! WARDEN ITEM (flagged in the oracle report): this needs
//! - the `InconclusiveReason` enum to gain a `CouldNotRun` variant, AND
//! - the ontology `gate.inconclusive` v1 `reason` vocabulary to gain the
//!   additive string `could_not_run` (spec/ontology.md line 226 ratified the
//!   reason vocab as "new causes arrive additively as strings", so this is an
//!   ADDITIVE value under the existing rule, not a breaking change — but the
//!   string still lands via a /subject session, never a direct edit).
//!
//! RED MODE: compile-red today. `InconclusiveReason::CouldNotRun` does not
//! exist yet, so this test file does not compile against the current enum —
//! that IS the failing state (the implementer adds the variant + the ontology
//! string, then this goes green). The two positive controls (spawn failure vs.
//! genuine malformed stdout) prove the two reasons are DISTINGUISHED, not that
//! could_not_run swallowed malformed_output.
#![cfg(unix)]

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use rezidnt_gate::{DEFAULT_TIMEOUT_MS, ExecVerifier, InconclusiveReason, Verdict, VerifierInput};
use serde_json::json;

/// Write an executable sh script into `dir` (mirrors exec_contract.rs).
fn script(dir: &Path, name: &str, body: &str) -> PathBuf {
    use std::os::unix::fs::PermissionsExt;
    let path = dir.join(name);
    std::fs::write(&path, format!("#!/bin/sh\n{body}")).expect("write script");
    let mut perms = std::fs::metadata(&path).expect("stat").permissions();
    perms.set_mode(0o755);
    std::fs::set_permissions(&path, perms).expect("chmod");
    path
}

fn input(timeout_ms: u64) -> VerifierInput {
    VerifierInput {
        gate: "pre_merge".to_string(),
        workspace: None,
        refs: BTreeMap::from([(
            "diff".to_string(),
            "cas:blake3:1d50030ca17af09eb6fad0eadfb3492275bfc76635d0965260cde6bc685d785e"
                .to_string(),
        )]),
        params: json!({}),
        timeout_ms,
    }
}

/// THE PIN: a spawn failure (argv[0] points at a program that does not exist)
/// yields `inconclusive { could_not_run }` — NOT `malformed_output`. Nothing
/// ran, so there was no output to be malformed; the reason must say so.
#[tokio::test]
async fn spawn_failure_is_inconclusive_could_not_run_not_malformed_output() {
    let verifier = ExecVerifier {
        name: "ghost-binary".to_string(),
        // A path that cannot exist — the exec spawn itself fails (ENOENT).
        argv: vec!["/nonexistent/rezidnt-oracle-could-not-run".to_string()],
    };
    let record = verifier.run(&input(DEFAULT_TIMEOUT_MS)).await;

    // I6 holds: a verifier that cannot run is never coerced to pass.
    assert_eq!(
        record.verdict,
        Verdict::Inconclusive,
        "a verifier that could not run is inconclusive, never pass (I6)"
    );
    assert_eq!(
        record.reason,
        Some(InconclusiveReason::CouldNotRun),
        "nothing ran, so the reason is could_not_run — not the untruthful \
         malformed_output the current runner mints"
    );
    assert_ne!(
        record.reason,
        Some(InconclusiveReason::MalformedOutput),
        "a spawn failure is NOT malformed output — that conflation was the flake"
    );
}

/// POSITIVE CONTROL: a program that DID run but printed garbage still yields
/// `malformed_output`. The two reasons are now distinguished; could_not_run
/// must not swallow the genuine malformed-stdout case.
#[tokio::test]
async fn malformed_stdout_from_a_program_that_ran_is_still_malformed_output() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = script(dir.path(), "garbage.sh", "printf 'LGTM ship it\\n'");
    let verifier = ExecVerifier {
        name: "ran-but-garbage".to_string(),
        argv: vec![path.display().to_string()],
    };
    let record = verifier.run(&input(DEFAULT_TIMEOUT_MS)).await;

    assert_eq!(record.verdict, Verdict::Inconclusive);
    assert_eq!(
        record.reason,
        Some(InconclusiveReason::MalformedOutput),
        "the program RAN and produced unparseable stdout — that is genuinely \
         malformed_output, distinct from could_not_run"
    );
    assert_ne!(
        record.reason,
        Some(InconclusiveReason::CouldNotRun),
        "a program that ran did not fail to run — the reasons must not collapse"
    );
}

/// The `could_not_run` reason serializes to the additive ontology string
/// `could_not_run` (the value the warden adds to the gate.inconclusive reason
/// vocab). Pins the wire form the log records, so the enum and the ontology
/// string cannot drift.
#[test]
fn could_not_run_serializes_to_the_additive_ontology_string() {
    let json = serde_json::to_value(InconclusiveReason::CouldNotRun).expect("serialize reason");
    assert_eq!(
        json,
        json!("could_not_run"),
        "the reason rides the fact as the additive snake_case string the \
         ontology gate.inconclusive vocab gains"
    );
}
