//! DR-041 slice `verify-subcommand` ORACLE — CRITERION 4: the `cargo-test` exec
//! verifier, invoked as `rezidnt verify cargo-test`, resolved through
//! `resolve_one` (`VerifierKind::Exec`) and run END-TO-END inside the daemon's
//! real `pre_merge` gate, producing a genuine `gate.passed` (tests pass) or
//! `gate.failed` (tests fail) fact carrying the verifier's recorded verdict +
//! `cost_ms`.
//!
//! This is a SEPARATE test file from `golden_path.rs` by design (DR-041
//! Decision 5): the golden-path demo board KEEPS its fast `tests-pass` exec STUB
//! — a real `cargo test` there would make the one-take demo slow and
//! host-toolchain-fragile for zero contract gain. The REAL cargo-test verifier is
//! proven HERE, on its own board, against a tiny fixture crate. This file follows
//! the S4 harness precedent (`make_gated_project`, `start_daemon`, socket `tail`
//! from `common`) but NEVER mutates `golden_path.rs`.
//!
//! RED MODE: the whole chain is red because the `rezidnt verify` subcommand does
//! not exist yet (DR-041's dispatch is unbuilt), AND the spec seam to name a
//! MULTI-TOKEN exec argv (`["rezidnt","verify","cargo-test"]`) against the
//! worktree is not wired (see the WIRING NOTE below). Concretely:
//! - `#[ignore]`-GATED with a tracking note: the fixtures this board needs — a
//!   gated project whose repo is a real cargo crate, and a `[gates.pre_merge]`
//!   entry naming `rezidnt verify cargo-test` as the exec verifier against the
//!   allocated worktree — do not exist in the testkit yet, and the `rezidnt`
//!   binary has no `verify` subcommand for the daemon's exec runner to invoke.
//!   The test is written to the pinned OBSERVABLE (a real cargo-test-gated
//!   `gate.passed`/`gate.failed` on the log) so the implementer un-ignores it
//!   once the dispatch + spec seam land. An ignored, compiling, RED-when-run test
//!   is the honest state for a slice whose production surface is entirely absent
//!   — NOT a test that passes vacuously (oracle house rule: a test that passes
//!   before implementation exists tests nothing).
//!
//! WIRING NOTE for the implementer (where the real wiring goes — the oracle can
//! only STUB criterion 4 because the subcommand is absent):
//!   1. `rezidnt verify cargo-test` must exist as a subcommand emitting the §8
//!      `VerifierOutput` on stdout (pinned by `bins/rezidnt/tests/verify_cargo_test_cli.rs`).
//!   2. The daemon must be able to name it as an exec argv in `[gates.pre_merge]`.
//!      Today `VerifierSpec.exec` is a SINGLE `PathBuf` and `resolve_one`
//!      (`bins/rezidentd/src/gates.rs`) builds a one-element argv
//!      (`vec![exec.display().to_string()]`) — there is NO way to express
//!      `["rezidnt","verify","cargo-test"]` (program + subcommand args) NOR to
//!      point the verifier at the allocated WORKTREE (cargo-test needs the tree,
//!      not just the diff summary the §8 `refs["diff"]` carries). The implementer
//!      resolves this (an `args`/argv field on `VerifierSpec`, and a worktree
//!      path reaching the verifier — a new §8 ref or the runner's cwd). The
//!      OBSERVABLE this board pins is stable across whichever seam is chosen: a
//!      real cargo-test verdict lands as the pre_merge `gate.passed`/`gate.failed`
//!      fact, with `cost_ms` recorded and inputs content-hash-pinned.
#![cfg(unix)]

mod common;

use std::time::Duration;

use common::{
    connect, make_cargo_test_gated_project, read_until, run_cli, send_line, start_daemon,
};
use serde_json::json;

/// Build the DR-041 cargo-test gated project via the shared testkit fixture
/// (migrated there per the implementer wiring note). The exec verifier's
/// `argv[0]` is the `rezidnt` CLI (`common::cli_bin()`), invoked as
/// `rezidnt verify cargo-test` and run by the daemon in the allocated worktree.
fn cargo_test_gated_project(pass: bool) -> (tempfile::TempDir, String) {
    make_cargo_test_gated_project(pass, 100, &common::cli_bin())
}

/// Open a gated project spec and drive the pre_merge chain to its terminal
/// verdict fact, returning the `tail` frames up to (and including) that fact.
/// Shared body for the pass / fail pins.
fn run_to_pre_merge_verdict(spec: &str, stop_subject: &str) -> Vec<serde_json::Value> {
    let daemon = start_daemon();
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
    // A real `cargo test` takes real wall-clock (compile + link + run); give the
    // chain a generous deadline — this is the "minutes of wall clock" DR-041
    // Decision 5 keeps OFF the golden-path board and on THIS one.
    read_until(&mut tail, Duration::from_secs(180), move |v| {
        v["subject"] == stop_subject && v["payload"]["gate"] == json!("pre_merge")
    })
}

/// CRITERION 4 (pass leg): a project whose worktree change is a PASSING cargo
/// test, gated on `rezidnt verify cargo-test`, produces a genuine `gate.passed`
/// for `pre_merge` carrying the verifier's record — `verifier = "cargo-test"`,
/// a recorded `cost_ms` (u64, value NOT asserted — wall-clock varies), and
/// content-hash-pinned inputs (§8 BINDING). This is the SAME `gate.passed`
/// per-verifier shape `golden_path.rs` reads, but produced by a REAL verifier.
///
/// LIVE (DR-041 `verify-subcommand` landed): the `rezidnt verify cargo-test`
/// subcommand and the multi-token / worktree-targeted exec seam
/// (`VerifierSpec.args` → `VerifierKind::Exec` argv; the exec runner's cwd = the
/// allocated worktree) now exist, so the `#[ignore]` gate is removed.
#[test]
fn e2e_cargo_test_pass_produces_real_gate_passed_with_recorded_cost() {
    let (_dir, spec) = cargo_test_gated_project(true);
    let lines = run_to_pre_merge_verdict(&spec, "gate.passed");

    let passed = lines
        .iter()
        .rfind(|v| v["subject"] == "gate.passed" && v["payload"]["gate"] == json!("pre_merge"))
        .expect("a real cargo-test pass lands a pre_merge gate.passed");
    let verifiers = passed["payload"]["verifiers"]
        .as_array()
        .expect("per-verifier records on gate.passed");
    let record = verifiers
        .iter()
        .find(|v| v["verifier"] == json!("cargo-test"))
        .unwrap_or_else(|| panic!("cargo-test record missing from {verifiers:#?}"));

    // cost_ms RECORDED (u64), value NOT asserted (wall-clock varies) — mirroring
    // golden_path.rs.
    assert!(record["cost_ms"].is_u64(), "cargo-test cost_ms recorded");
    // Inputs content-hash-pinned (§8 BINDING).
    assert!(
        record["inputs"]["refs"]["diff"]
            .as_str()
            .is_some_and(|r| r.starts_with("cas:blake3:")),
        "cargo-test inputs pinned by content hash (§8 BINDING); got {record:#?}"
    );
}

/// CRITERION 4 (fail leg, INTERROGABLE): a project whose worktree change makes a
/// test FAIL, gated on `rezidnt verify cargo-test`, produces a genuine
/// `gate.failed` for `pre_merge` naming `verifier = "cargo-test"` — a REAL test
/// failure, not the stub. The merge does NOT happen (no `diff.merged`), because
/// the merge follows only a VERIFIED pass (the golden-path contract). This proves
/// the verdict mapping travels end-to-end: a real defect blocks the merge.
///
/// LIVE (DR-041 `verify-subcommand` landed): un-ignored for the same reason as
/// the pass leg — the subcommand + exec seam exist.
#[test]
fn e2e_cargo_test_fail_produces_real_gate_failed_and_blocks_merge() {
    let (_dir, spec) = cargo_test_gated_project(false);
    let lines = run_to_pre_merge_verdict(&spec, "gate.failed");

    let failed = lines
        .iter()
        .rfind(|v| v["subject"] == "gate.failed" && v["payload"]["gate"] == json!("pre_merge"))
        .expect("a real cargo-test failure lands a pre_merge gate.failed");
    assert_eq!(
        failed["payload"]["verifier"], "cargo-test",
        "the failing verifier is named on gate.failed (§8 interrogability)"
    );
    // The merge is gated on a VERIFIED pass: a failing pre_merge must NOT merge.
    assert!(
        !lines.iter().any(|v| v["subject"] == "diff.merged"),
        "a failing pre_merge blocks the merge — no diff.merged on the log"
    );
}
