//! DR-022 benchmark-harness oracle — the ORCHESTRATION seam (CRITERIA 1, 3).
//!
//! `run_cases` generalizes the S4 `golden_path.rs` single-run flow into an
//! N-case run (DR-022). To make the orchestration LOGIC genuinely falsifiable —
//! and NOT satisfiable by a stub that echoes `Case::expect_merge` — the driver
//! is DEPENDENCY-INJECTED (`run_cases(cases, driver)`). These tests inject a
//! DETERMINISTIC FAKE `Driver` whose per-case result the TEST controls, then
//! assert the loop: one outcome per case, correct name attribution, and — the
//! load-bearing resilience pin — one case's failure (a driver MISS, or a driver
//! PANIC) does NOT abort the batch. Because the assertions read the DRIVER's
//! controlled results (not the case's declared intent), an implementation that
//! parroted `expect_merge` and drove nothing CANNOT pass them.
//!
//! RED MECHANISM (greenfield): `run_cases` is a `todo!()` stub — every test
//! LINKS and FAILS AT RUNTIME (the stub panics before the fake driver is ever
//! consulted). Confirmed red at board time by
//! `cargo test -p rezidnt-bench-harness --test orchestration`. The real
//! DaemonDriver's end-to-end driving is pinned separately by
//! `tests/real_driver.rs` (`#[cfg(unix)]`, WSL-only).

use std::collections::HashMap;
use std::path::PathBuf;

use rezidnt_bench_harness::{Case, CaseOutcome, Driver, run_cases};

/// A fully test-controlled [`Driver`]: it returns a scripted [`CaseOutcome`] per
/// case name, and (optionally) PANICS on a named case to prove the batch is not
/// aborted by a single driving failure. It reads NOTHING from
/// `Case::expect_merge` — its results are whatever the test scripted, so a
/// `run_cases` that echoed `expect_merge` would disagree with this driver and
/// fail the assertions.
struct FakeDriver {
    /// case name -> the (reached_verified_merge, run) the driver reports.
    scripted: HashMap<String, (bool, Option<String>)>,
    /// case name the driver PANICS on (simulates a real DaemonDriver blowing up
    /// on one case); `None` = never panics.
    panic_on: Option<String>,
}

impl Driver for FakeDriver {
    fn drive(&self, case: &Case) -> CaseOutcome {
        if self.panic_on.as_deref() == Some(case.name.as_str()) {
            panic!(
                "FakeDriver: simulated driving failure on case {}",
                case.name
            );
        }
        let (reached, run) = self
            .scripted
            .get(&case.name)
            .cloned()
            .unwrap_or((false, None));
        CaseOutcome {
            name: case.name.clone(),
            reached_verified_merge: reached,
            run,
        }
    }
}

fn cases() -> Vec<Case> {
    vec![
        Case {
            name: "case_a".to_string(),
            spec_path: PathBuf::from("/nominal/case_a/rezidnt.toml"),
            // INTENT deliberately CONTRADICTS the scripted driver result below
            // (expect_merge=false but the driver reports a HIT) so an
            // echo-of-expect_merge implementation is caught red-handed.
            expect_merge: false,
        },
        Case {
            name: "case_b".to_string(),
            spec_path: PathBuf::from("/nominal/case_b/rezidnt.toml"),
            // Also contradictory: expect_merge=true but the driver reports a MISS.
            expect_merge: true,
        },
    ]
}

/// CRITERION 1: the orchestration loop returns one machine-readable outcome per
/// case, HEADLESS (a plain `Vec<CaseOutcome>` — no TUI, I1), attributed to the
/// case name, and carrying the DRIVER's result — NOT `Case::expect_merge`. The
/// scripted driver contradicts each case's `expect_merge`, so this test is
/// impossible to pass by echoing intent.
#[test]
fn run_cases_maps_each_case_to_its_driver_result_not_its_intent() {
    let cases = cases();
    let driver = FakeDriver {
        scripted: HashMap::from([
            // case_a: intent says false, driver says HIT — the report must follow
            // the driver.
            ("case_a".to_string(), (true, Some("01RUNAAAA".to_string()))),
            // case_b: intent says true, driver says MISS.
            ("case_b".to_string(), (false, None)),
        ]),
        panic_on: None,
    };

    let outcomes = run_cases(&cases, &driver);

    assert_eq!(
        outcomes.len(),
        cases.len(),
        "CRITERION 1: one headless outcome per case"
    );
    let by_name: HashMap<&str, &CaseOutcome> =
        outcomes.iter().map(|o| (o.name.as_str(), o)).collect();

    let a = by_name
        .get("case_a")
        .expect("case_a is named in the outcomes (attribution)");
    assert!(
        a.reached_verified_merge,
        "case_a follows the DRIVER (hit), NOT expect_merge (false) — a hollow \
         expect_merge echo would report false here and fail"
    );
    assert_eq!(
        a.run.as_deref(),
        Some("01RUNAAAA"),
        "the driver's run rides through"
    );

    let b = by_name
        .get("case_b")
        .expect("case_b is named in the outcomes");
    assert!(
        !b.reached_verified_merge,
        "case_b follows the DRIVER (miss), NOT expect_merge (true) — again, an \
         echo of intent would disagree with the driver and fail"
    );
}

/// CRITERION 3: a driver MISS on one case is a scored miss, and the SUCCEEDING
/// case in the same batch is unaffected — one case's failure does not poison the
/// run. Asserted against the driver's controlled results.
#[test]
fn a_driver_miss_scores_a_miss_and_does_not_poison_the_batch() {
    let cases = cases();
    let driver = FakeDriver {
        scripted: HashMap::from([
            ("case_a".to_string(), (true, Some("01RUNAAAA".to_string()))),
            ("case_b".to_string(), (false, None)),
        ]),
        panic_on: None,
    };

    let outcomes = run_cases(&cases, &driver);
    let by_name: HashMap<&str, &CaseOutcome> =
        outcomes.iter().map(|o| (o.name.as_str(), o)).collect();

    assert!(
        !by_name["case_b"].reached_verified_merge,
        "CRITERION 3: the driver-missed case scores a miss"
    );
    assert!(
        by_name["case_a"].reached_verified_merge,
        "the succeeding case survives the missed case in the same batch"
    );
}

/// CRITERION 3 (resilience, load-bearing): a driver that PANICS on one case (a
/// real DaemonDriver can blow up mid-drive) does NOT abort the whole run — the
/// orchestrator isolates the failure, scores THAT case a miss, and still returns
/// an outcome for every other case. This is the "stays a deterministic judge of
/// its own runs" pin (I6): a single case's driving fault cannot crash the
/// benchmark.
#[test]
fn a_driver_panic_on_one_case_does_not_abort_the_batch() {
    let cases = cases();
    let driver = FakeDriver {
        scripted: HashMap::from([
            // case_b would be a hit if driven — but the driver panics on it.
            ("case_b".to_string(), (true, Some("01RUNBBBB".to_string()))),
            ("case_a".to_string(), (true, Some("01RUNAAAA".to_string()))),
        ]),
        panic_on: Some("case_b".to_string()),
    };

    // The whole call returning at all (not unwinding out of run_cases) is half
    // the assertion — the orchestrator must catch the per-case driving fault.
    let outcomes = run_cases(&cases, &driver);

    assert_eq!(
        outcomes.len(),
        cases.len(),
        "CRITERION 3: every case still has an outcome even though the driver \
         panicked on one — the batch is not aborted"
    );
    let by_name: HashMap<&str, &CaseOutcome> =
        outcomes.iter().map(|o| (o.name.as_str(), o)).collect();
    assert!(
        !by_name["case_b"].reached_verified_merge,
        "the case the driver panicked on scores a MISS, not a crash (CRITERION 3)"
    );
    assert!(
        by_name["case_a"].reached_verified_merge,
        "the unaffected case still scores its driver hit"
    );
}
