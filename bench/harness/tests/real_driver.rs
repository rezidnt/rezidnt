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
//!
//! ANTI-ECHO STRENGTHENING (DR-023 oracle pass): `reached_verified_merge` for a
//! genuine golden case coincides with the case's `expect_merge == true`, so —
//! unlike `orchestration.rs`, which defeats an `expect_merge` echo by
//! CONTRADICTING intent with a fake driver — this test cannot rely on
//! contradiction. It defeats the echo STRUCTURALLY: the outcome must also carry a
//! real 26-char Crockford `run` ULID read off the log (the run the daemon
//! actually spawned). An implementation that merely parroted `Case::expect_merge`
//! into `reached_verified_merge` reports no such run and fails here — so a hollow
//! echo cannot satisfy this oracle.
//!
//! ── WHO STAGES THE FIXTURE (DR-023 §(C) — the criterion-4 contract this test
//! ENFORCES): fixture construction (git init, harness/verifier scripts, chmod,
//! synthesizing a gated `rezidnt.toml`) is DEV-ONLY test-support and lives in the
//! `rezidnt-testkit` crate — NEVER in production `DaemonDriver::drive`. So THIS
//! test stages the gated project via `rezidnt_testkit::make_gated_project` (a
//! dev-dependency of the harness) and writes its `rezidnt.toml` to a real temp
//! dir, then hands `drive` a REAL, EXISTING `spec_path`. That makes
//! `case.spec_path.exists()` TRUE, so production `drive` takes its honest
//! open-existing-spec path (open → run CLI → tail-for-merge) and has NO reason to
//! contain any inline `stage_gated_fixture` branch. A `drive` that still stages a
//! fixture when the spec is missing is dead code under this test — the contract
//! here is that the driver is HANDED a real repo, it never builds one.
#![cfg(unix)]

use rezidnt_bench_harness::{Case, DaemonDriver, Driver};

/// CRITERION 1 (real e2e): the real `DaemonDriver` drives ONE case's golden path
/// against an actual project spec and reports `reached_verified_merge == true`
/// from the log's terminal facts (`gate.passed`(pre_merge) → `diff.merged`).
///
/// The gated fixture is staged HERE via the dev-only testkit (DR-023 §(C)); the
/// driver is handed the resulting real `rezidnt.toml`. The oracle pins the
/// ASSERTION (real drive → real verified merge + real run ULID) AND the boundary
/// (production `drive` receives a staged repo, it does not construct one). This
/// is the real-driving counterpart to the fake-driver loop tests, so criterion
/// 1's "end-to-end against a target repo" is not left unpinned.
#[test]
fn real_daemon_driver_reaches_a_verified_merge_for_a_golden_case() {
    // Stage the gated fixture via the DEV-ONLY testkit (DR-023 §(C)): git repo
    // with a committed src/checkout/cart.rs, the diff-writing stub harness, an
    // exec pass-verifier, and a §13 spec wiring gates = ["vet", "pre_merge"].
    // `make_gated_project` returns (TempDir, spec_toml); the TempDir must stay
    // bound for the whole test so the staged repo/scripts survive the drive.
    let (project, spec) = rezidnt_testkit::make_gated_project(100);
    let spec_path = project.path().join("rezidnt.toml");
    std::fs::write(&spec_path, &spec).expect("write staged gated rezidnt.toml");
    assert!(
        spec_path.exists(),
        "the driver must be handed a REAL, EXISTING spec — production drive opens \
         it, it does NOT stage a fixture (DR-023 §(C)): {}",
        spec_path.display()
    );

    // Hand the driver the REAL staged spec path. Because it exists, the
    // production driver takes the open-existing-spec path; the fixture was built
    // by the testkit above, never by `drive`.
    let case = Case {
        name: "golden_verified_merge_real".to_string(),
        spec_path: spec_path.clone(),
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

    // ANTI-ECHO PIN (the load-bearing strengthening): `reached_verified_merge ==
    // true` alone is NOT enough — because this real case's `expect_merge` is also
    // `true` (a real golden case IS expected to merge), a hollow implementation
    // that simply echoed `Case::expect_merge` into `reached_verified_merge` would
    // satisfy the assertion above without ever driving the daemon. `orchestration.rs`
    // defeats the echo by CONTRADICTING intent with a fake driver; `real_driver.rs`
    // cannot (reality and intent coincide for a genuine golden case), so the echo
    // is defeated STRUCTURALLY instead: the outcome must carry a REAL run ULID that
    // the driver read off the log (the run the daemon actually spawned). An
    // `expect_merge` echo produces no such id — the `Case` has no run field to
    // parrot, and a driver that never drove has no spawned run to report. A valid
    // 26-char Crockford ULID here is only obtainable by ACTUALLY driving a run.
    let run = outcome.run.as_deref().unwrap_or_else(|| {
        panic!(
            "CRITERION 1 anti-echo: the real drive must report the run ULID it spawned \
             (CaseOutcome.run), read off the log — an expect_merge echo carries no run. \
             Got run == None for a case the driver claims reached a verified merge."
        )
    });
    assert_eq!(
        run.len(),
        26,
        "the reported run must be a 26-char Crockford ULID minted by the real daemon \
         (a driven run, not a fabricated/echoed value): got {run:?}"
    );
    assert!(
        run.chars()
            .all(|c| c.is_ascii_digit() || c.is_ascii_uppercase())
            && !run.contains(['I', 'L', 'O', 'U']),
        "the reported run must be a Crockford-alphabet ULID (0-9 A-Z minus I L O U) — \
         a real daemon-minted run id, not a placeholder: got {run:?}"
    );
}
