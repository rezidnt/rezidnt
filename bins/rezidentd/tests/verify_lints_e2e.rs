//! DR-041 slice `verify-lints` ORACLE — CRITERION 4: the `clippy` and
//! `fmt-check` production verifiers, invoked as `rezidnt verify <name>`,
//! resolved through `resolve_one` (`VerifierKind::Exec`) and run END-TO-END
//! inside the daemon's real `pre_merge` gate, producing a genuine `gate.passed`
//! (clean) or `gate.failed` (a lint / fmt violation, NAMING the check) fact
//! carrying the verifier's recorded verdict + `cost_ms`.
//!
//! This is a SEPARATE test file from `golden_path.rs` by design (DR-041
//! Decision 5) and a SIBLING of `verify_subcommand_e2e.rs` (the cargo-test e2e).
//! It reuses the SAME S4 harness precedent (`start_daemon`, socket `tail` from
//! `common`) and the SAME multi-token / worktree-targeted exec seam
//! (`VerifierSpec.args` → `VerifierKind::Exec` argv; the exec runner's cwd = the
//! allocated worktree) the cargo-test slice already shipped. It NEVER mutates
//! `golden_path.rs`.
//!
//! ## RED MODE (RED-when-run, for the RIGHT reason)
//!
//! The exec seam EXISTS (verify-subcommand landed), so the fixture resolves and
//! the daemon runs `rezidnt verify clippy` / `rezidnt verify fmt-check` in the
//! worktree TODAY. But those names are NOT dispatched yet — the `verify` match
//! arm only knows `cargo-test`, so the subcommand emits the unknown-verifier
//! FALLBACK document (`{"verdict":"inconclusive", … "unknown verifier"}`). The
//! daemon's exec runner reads that back and emits a pre_merge `gate.inconclusive`
//! — NOT the `gate.passed` (clean leg) / `gate.failed` (defect leg) these tests
//! pin. So `run_to_pre_merge_verdict(spec, "gate.passed")` (clean) and
//! `"gate.failed"` (defect) BLOCK until their deadline and PANIC — RED because
//! the verifier is not dispatched, never from a malformed assertion. Once the
//! implementer adds the two match arms, the runner reads real pass/fail
//! documents and these go green on their own merits (mirroring how
//! `verify_subcommand_e2e.rs` went green when cargo-test landed).
//!
//! Unix-only (the S4 daemon-harness precedent — it drives the daemon over a
//! `UnixStream`). A real `cargo clippy` / `cargo fmt` takes real wall-clock
//! (compile + lint), which DR-041 Decision 5 keeps OFF the golden-path board and
//! on THIS one.
#![cfg(unix)]

mod common;

use std::time::Duration;

use common::{
    LintGate, connect, make_lints_gated_project, read_until, run_cli, send_line, start_daemon,
};
use serde_json::json;

/// Build a DR-041 lint-gated project via the shared testkit fixture. The exec
/// verifier's `argv[0]` is the `rezidnt` CLI (`common::cli_bin()`), invoked as
/// `rezidnt verify <name>` and run by the daemon in the allocated worktree.
fn lints_gated_project(gate: LintGate, pass: bool) -> (tempfile::TempDir, String) {
    make_lints_gated_project(gate, pass, 100, &common::cli_bin())
}

/// Open a gated project spec and drive the pre_merge chain to its terminal
/// verdict fact, returning the `tail` frames up to (and including) that fact.
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
    // A real `cargo clippy` / `cargo fmt` takes real wall-clock (compile + lint);
    // give the chain a generous deadline — the "minutes of wall clock" DR-041
    // Decision 5 keeps OFF the golden-path board and on THIS one.
    read_until(&mut tail, Duration::from_secs(180), move |v| {
        v["subject"] == stop_subject && v["payload"]["gate"] == json!("pre_merge")
    })
}

// ===========================================================================
// CLIPPY (criterion 4)
// ===========================================================================

/// CRITERION 4 (clippy, pass leg): a worktree change that is CLIPPY-CLEAN, gated
/// on `rezidnt verify clippy`, produces a genuine `gate.passed` for `pre_merge`
/// carrying the verifier's record — `verifier = "clippy"`, a recorded `cost_ms`
/// (u64, value NOT asserted — wall-clock varies), and content-hash-pinned inputs
/// (§8 BINDING). Same `gate.passed` per-verifier shape `golden_path.rs` reads,
/// produced by a REAL verifier.
#[test]
fn e2e_clippy_clean_produces_real_gate_passed_with_recorded_cost() {
    let (_dir, spec) = lints_gated_project(LintGate::Clippy, true);
    let lines = run_to_pre_merge_verdict(&spec, "gate.passed");

    let passed = lines
        .iter()
        .rfind(|v| v["subject"] == "gate.passed" && v["payload"]["gate"] == json!("pre_merge"))
        .expect("a clippy-clean change lands a pre_merge gate.passed");
    let verifiers = passed["payload"]["verifiers"]
        .as_array()
        .expect("per-verifier records on gate.passed");
    let record = verifiers
        .iter()
        .find(|v| v["verifier"] == json!("clippy"))
        .unwrap_or_else(|| panic!("clippy record missing from {verifiers:#?}"));

    assert!(record["cost_ms"].is_u64(), "clippy cost_ms recorded");
    assert!(
        record["inputs"]["refs"]["diff"]
            .as_str()
            .is_some_and(|r| r.starts_with("cas:blake3:")),
        "clippy inputs pinned by content hash (§8 BINDING); got {record:#?}"
    );
}

/// CRITERION 4 (clippy, fail leg, INTERROGABLE): a worktree change that trips a
/// clippy lint, gated on `rezidnt verify clippy`, produces a genuine
/// `gate.failed` for `pre_merge` naming `verifier = "clippy"` — a REAL lint
/// violation, not a stub. The merge does NOT happen (no `diff.merged`): the merge
/// follows only a VERIFIED pass. A real defect blocks the merge end-to-end.
#[test]
fn e2e_clippy_lint_produces_real_gate_failed_and_blocks_merge() {
    let (_dir, spec) = lints_gated_project(LintGate::Clippy, false);
    let lines = run_to_pre_merge_verdict(&spec, "gate.failed");

    let failed = lines
        .iter()
        .rfind(|v| v["subject"] == "gate.failed" && v["payload"]["gate"] == json!("pre_merge"))
        .expect("a clippy lint lands a pre_merge gate.failed");
    assert_eq!(
        failed["payload"]["verifier"], "clippy",
        "the failing verifier is named on gate.failed (§8 interrogability)"
    );
    assert!(
        !lines.iter().any(|v| v["subject"] == "diff.merged"),
        "a failing pre_merge blocks the merge — no diff.merged on the log"
    );
}

// ===========================================================================
// FMT-CHECK (criterion 4)
// ===========================================================================

/// CRITERION 4 (fmt-check, pass leg): a worktree change that is RUSTFMT-CLEAN,
/// gated on `rezidnt verify fmt-check`, produces a genuine `gate.passed` for
/// `pre_merge` carrying `verifier = "fmt-check"`, a recorded `cost_ms`, and
/// content-hash-pinned inputs (§8 BINDING).
#[test]
fn e2e_fmt_check_clean_produces_real_gate_passed_with_recorded_cost() {
    let (_dir, spec) = lints_gated_project(LintGate::FmtCheck, true);
    let lines = run_to_pre_merge_verdict(&spec, "gate.passed");

    let passed = lines
        .iter()
        .rfind(|v| v["subject"] == "gate.passed" && v["payload"]["gate"] == json!("pre_merge"))
        .expect("a rustfmt-clean change lands a pre_merge gate.passed");
    let verifiers = passed["payload"]["verifiers"]
        .as_array()
        .expect("per-verifier records on gate.passed");
    let record = verifiers
        .iter()
        .find(|v| v["verifier"] == json!("fmt-check"))
        .unwrap_or_else(|| panic!("fmt-check record missing from {verifiers:#?}"));

    assert!(record["cost_ms"].is_u64(), "fmt-check cost_ms recorded");
    assert!(
        record["inputs"]["refs"]["diff"]
            .as_str()
            .is_some_and(|r| r.starts_with("cas:blake3:")),
        "fmt-check inputs pinned by content hash (§8 BINDING); got {record:#?}"
    );
}

/// CRITERION 4 (fmt-check, fail leg, INTERROGABLE): a worktree change that
/// mis-formats a file, gated on `rezidnt verify fmt-check`, produces a genuine
/// `gate.failed` for `pre_merge` naming `verifier = "fmt-check"` — a REAL
/// formatting violation. The merge does NOT happen (no `diff.merged`).
#[test]
fn e2e_fmt_check_misformat_produces_real_gate_failed_and_blocks_merge() {
    let (_dir, spec) = lints_gated_project(LintGate::FmtCheck, false);
    let lines = run_to_pre_merge_verdict(&spec, "gate.failed");

    let failed = lines
        .iter()
        .rfind(|v| v["subject"] == "gate.failed" && v["payload"]["gate"] == json!("pre_merge"))
        .expect("a mis-formatted change lands a pre_merge gate.failed");
    assert_eq!(
        failed["payload"]["verifier"], "fmt-check",
        "the failing verifier is named on gate.failed (§8 interrogability)"
    );
    assert!(
        !lines.iter().any(|v| v["subject"] == "diff.merged"),
        "a failing pre_merge blocks the merge — no diff.merged on the log"
    );
}
