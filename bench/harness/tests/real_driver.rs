//! DR-022 benchmark-harness oracle — the REAL-driving proof (CRITERION 1, e2e).
//!
//! `orchestration.rs` pins the orchestration LOOP against a fake driver (host-
//! testable, deterministic). THIS file pins the OTHER half: that the real
//! [`DaemonDriver`] actually drives a case's golden path end-to-end and reads a
//! genuine verified merge off the log — the "runs the golden path end-to-end
//! against a target repo" half of criterion 1 that a fake driver deliberately
//! does not exercise.
//!
//! PLATFORM / VET LIMITATION (flagged, [[vet-is-host-side-wsl-insufficient]]):
//! the real driver stands up the daemon and drives the S4 golden path, which the
//! S4 integration suite already gates behind `#[cfg(unix)]` (`golden_path.rs`
//! is unix-only). Host `/vet` runs on Windows and will COMPILE-SKIP this whole
//! module (the `#[cfg(unix)]` gate), so host vet does not execute it. It exists
//! as the WSL-side real-driving proof; run it with the WSL workspace-test
//! invocation, NOT concurrently with a host vet run ([[vet-concurrency-flake]]).
//!
//! RELATIONSHIP to `golden_path.rs`: the single-case golden path (one
//! open→…→diff.merged→debrief) is ALREADY covered end-to-end by
//! `bins/rezidentd/tests/golden_path.rs:38-157`. This test does NOT re-derive
//! that flow — it asserts the harness's `DaemonDriver` REUSES it for ONE case
//! and returns `reached_verified_merge == true` from the real terminal facts, so
//! the benchmark's real-driving seam is proven, not just its loop.
//!
//! RED MECHANISM: `DaemonDriver`/`run_cases_default` are `todo!()` stubs, so on
//! unix this test LINKS and FAILS AT RUNTIME (the stub panics). On the Windows
//! host it is cfg'd out entirely (compiles to nothing) — stated plainly so the
//! board is not misread as green on host. It turns green only when the
//! implementer wires the real driver against a fixture spec reaching a verified
//! merge.
#![cfg(unix)]

use std::path::PathBuf;

use rezidnt_bench_harness::{Case, DaemonDriver, Driver};

/// CRITERION 1 (real e2e): the real `DaemonDriver` drives ONE case's golden path
/// against an actual project spec and reports `reached_verified_merge == true`
/// from the log's terminal facts (`gate.passed`(pre_merge) → `diff.merged`).
///
/// IMPLEMENTER-OWNED: build a gated fixture project (the `make_gated_project`
/// shape from `bins/rezidentd/tests/common/mod.rs`), point the `Case` at its
/// `rezidnt.toml`, drive it via the real `DaemonDriver`, and assert the hit. The
/// oracle pins the ASSERTION (real drive → real verified merge); the fixture
/// wiring and daemon staging are the implementer's. This is the real-driving
/// counterpart to the fake-driver loop tests, so criterion 1's "end-to-end
/// against a target repo" is not left unpinned.
#[test]
fn real_daemon_driver_reaches_a_verified_merge_for_a_golden_case() {
    // A real spec path is required here (the driver opens it for real). The
    // implementer stages a gated fixture project and points this at its
    // rezidnt.toml; the nominal path below is a placeholder that the RED stub
    // never reaches (it panics first), flagged so the implementer replaces it
    // with the staged fixture spec rather than shipping a bogus path.
    let case = Case {
        name: "golden_verified_merge_real".to_string(),
        spec_path: PathBuf::from("<implementer: staged gated fixture rezidnt.toml>"),
        expect_merge: true,
    };

    let driver = DaemonDriver::default();
    let outcome = driver.drive(&case);

    assert_eq!(
        outcome.name, case.name,
        "attribution rides the real outcome"
    );
    assert!(
        outcome.reached_verified_merge,
        "CRITERION 1 (real e2e): the real DaemonDriver must reach a genuine \
         verified merge (gate.passed(pre_merge) → diff.merged) read off the log, \
         reusing the golden_path.rs flow for one case — not an expect_merge echo"
    );
}
