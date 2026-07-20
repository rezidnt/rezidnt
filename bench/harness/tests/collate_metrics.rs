//! DR-022 benchmark-harness oracle — the COLLATION seam (CRITERIA 2, 3, 4).
//!
//! These tests fold the committed golden fixture `bench_two_case_run.jsonl`
//! (two runs: one that reaches a verified merge, one deliberately-failing that
//! fails `pre_merge` and never merges) directly through the collator. No daemon
//! is needed — the collator is PURE over the recorded log (I3), exactly like
//! the rezidnt-state reducer replay tests. This is the load-bearing surface:
//! replay-stability (fold twice → byte-identical) and the honest
//! precision/recall seam live here.
//!
//! RED MECHANISM (greenfield, stated honestly): the crate + public API EXIST as
//! `todo!()` stubs (`bench/harness/src/lib.rs`), so every test below LINKS and
//! FAILS AT RUNTIME when it calls `collate` (the stub panics with the
//! implementer TODO). This is the honest greenfield RED — the tests compile,
//! run, and fail against zero-logic stubs; the implementer turns them green by
//! filling the stub bodies WITHOUT touching the pinned signatures / report
//! shape. Confirmed red at board time by `cargo test -p rezidnt-bench-harness`
//! (every case panics inside `todo!()`).

use std::path::PathBuf;

use rezidnt_bench_harness::{Case, MetricsReport, NO_LABELED_SET, Seam, collate};
use rezidnt_types::Event;

fn fixtures_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../spec/fixtures")
}

/// Parse a committed `spec/fixtures/*.jsonl` into events (plain serde, so a
/// failure isolates the COLLATOR, not the wire codec — the fixture_replay
/// convention).
fn load_fixture(name: &str) -> Vec<Event> {
    let path = fixtures_dir().join(name);
    std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("fixture {name} must exist: {e}"))
        .lines()
        .filter(|l| !l.trim().is_empty())
        .map(|l| {
            serde_json::from_str(l).unwrap_or_else(|e| panic!("{name} line parses ({e}): {l}"))
        })
        .collect()
}

/// The two scenario intents the fixture records: case 1 is EXPECTED to merge,
/// case 2 is the DELIBERATELY-failing scenario (never reaches a verified
/// merge). The spec paths are nominal — the collator reads the LOG, not the
/// filesystem; the intents let it score case 2 as a MISS (CRITERION 3).
fn expected_cases() -> Vec<Case> {
    vec![
        Case {
            name: "golden_verified_merge".to_string(),
            spec_path: PathBuf::from("/nominal/case1/rezidnt.toml"),
            expect_merge: true,
        },
        Case {
            name: "deliberate_no_merge".to_string(),
            spec_path: PathBuf::from("/nominal/case2/rezidnt.toml"),
            expect_merge: false,
        },
    ]
}

/// CRITERION 2 (load-bearing): REPLAY-STABILITY. Folding the SAME recorded facts
/// twice yields BYTE-IDENTICAL metric numbers. This is the genuinely-falsifiable
/// determinism pin: a collator that read a fresh wall-clock or rng would diverge
/// between the two folds and fail here. Serialized to canonical JSON and
/// compared as bytes so the equality is total (every field, every fact-id in
/// `folded_from`), not just the headline floats.
#[test]
fn metrics_are_replay_stable_folding_twice_is_byte_identical() {
    let log = load_fixture("bench_two_case_run.jsonl");
    let cases = expected_cases();

    let first: MetricsReport = collate(&log, &cases);
    let second: MetricsReport = collate(&log, &cases);

    let a = serde_json::to_vec(&first).expect("report serializes");
    let b = serde_json::to_vec(&second).expect("report serializes");
    assert_eq!(
        a, b,
        "CRITERION 2: folding the same recorded facts twice MUST be byte-identical \
         (no fresh wall-clock / rng in the collator — I3 replay-stability)"
    );
}

/// CRITERION 2: the three in-repo metrics are folded FROM THE LOG. Task
/// completion = 1 of 2 cases reached a verified merge (0.5); worktree merge
/// success = 1 of 2 attempted merges landed a `diff.merged` (0.5). Both are
/// derived purely from recorded facts (`gate.passed`(pre_merge)→`diff.merged`
/// for completion; `diff.merged` count over merge-attempt count for merge
/// success).
#[test]
fn three_metrics_fold_from_the_log() {
    let log = load_fixture("bench_two_case_run.jsonl");
    let report = collate(&log, &expected_cases());

    assert_eq!(
        report.task_completion.value, 0.5,
        "1 of 2 cases reached a verified merge (case2 is the deliberate miss)"
    );
    assert_eq!(
        report.worktree_merge_success.value, 0.5,
        "1 of 2 merge attempts landed a diff.merged on the log"
    );
    assert!(
        report.cost_per_merged_verified_diff.value > 0.0,
        "cost-per-merged-diff folds off the shipped cost fields to a positive USD figure; got {}",
        report.cost_per_merged_verified_diff.value
    );
}

/// CRITERION 2 (interrogability, I6): each folded metric NAMES the facts it
/// folded from. The report is auditable — a reader can trace every number back
/// to the exact event ids on the log. An empty trail on a nonzero metric is a
/// non-interrogable (theater) metric and fails here.
#[test]
fn metrics_name_the_facts_they_folded_from() {
    let log = load_fixture("bench_two_case_run.jsonl");
    let report = collate(&log, &expected_cases());

    // The verified-merge case's diff.merged id — the fact task-completion and
    // merge-success MUST fold from.
    let merged_id = "01BENCHAC10000000000000E10";
    assert!(
        report
            .task_completion
            .folded_from
            .contains(&merged_id.to_string()),
        "task-completion names the diff.merged fact it folded (I6 interrogability); trail: {:?}",
        report.task_completion.folded_from
    );
    assert!(
        report
            .worktree_merge_success
            .folded_from
            .contains(&merged_id.to_string()),
        "merge-success names the diff.merged fact it folded; trail: {:?}",
        report.worktree_merge_success.folded_from
    );
    assert!(
        !report.cost_per_merged_verified_diff.folded_from.is_empty(),
        "cost-per-merged-diff names the cost facts it folded (I6); trail was empty"
    );
}

/// CRITERION 2: cost-per-merged-verified-diff reads ONLY already-shipped fields.
/// The fixture's verified case carries all three shipped sources — the
/// `agent.completed.cost`, per-verifier `cost_ms` on `gate.passed`, and
/// `action.metered.spend_delta_usd`. This test pins that the metric's trail is
/// drawn from facts bearing those EXISTING fields (no new field/subject minted
/// by this slice — the collator reads the log as-shipped). The trail must
/// reference the verified case's cost-bearing facts, never invent a subject.
#[test]
fn cost_reads_only_already_shipped_fields_no_new_subject() {
    let log = load_fixture("bench_two_case_run.jsonl");
    let report = collate(&log, &expected_cases());

    // Every subject the collator folds cost from must already exist in the
    // taxonomy — assert the trail references only known cost-bearing facts by
    // id, and that the collator did not require a bench-specific subject. We
    // check the trail is a subset of the ids actually present on the log.
    let known_ids: Vec<String> = log.iter().map(|e| e.id.to_string()).collect();
    for id in &report.cost_per_merged_verified_diff.folded_from {
        assert!(
            known_ids.contains(id),
            "cost folded from an id ({id}) not on the recorded log — the collator must read \
             already-shipped facts, never mint a new event/field (CRITERION 2)"
        );
    }
}

/// CRITERION 3: the DELIBERATELY-failing scenario (case2 — `pre_merge`
/// `gate.failed`, no `diff.merged`) is counted as a task-completion MISS (a
/// scored zero), NOT a harness crash. The collator folds it as a legible miss:
/// `collate` returns normally (no panic), task-completion is `0.5` (one of two),
/// and case2's per-case outcome shows `reached_verified_merge == false`.
#[test]
fn deliberately_failing_case_scores_a_miss_not_a_crash() {
    let log = load_fixture("bench_two_case_run.jsonl");
    // `collate` returning at all (rather than panicking on the failed case) is
    // half the assertion — the harness stays a deterministic judge (I6).
    let report = collate(&log, &expected_cases());

    let case2 = report
        .cases
        .iter()
        .find(|c| c.name == "deliberate_no_merge")
        .expect("the deliberately-failing case is scored, present in the report");
    assert!(
        !case2.reached_verified_merge,
        "CRITERION 3: a case that never reaches a verified merge is a scored MISS, \
         not a crash — reached_verified_merge must be false"
    );
    assert_eq!(
        report.task_completion.value, 0.5,
        "the miss is counted in the denominator (1 hit / 2 cases), not dropped"
    );
}

/// CRITERION 4: the precision/recall seam is PRESENT but, with no labeled set
/// supplied to `collate`, returns `inconclusive — no labeled set present` —
/// never a fabricated score, never a blank silently read as zero (I6). This is
/// the honest disclosure of a permanently-external measurement (§17).
#[test]
fn precision_recall_seam_is_inconclusive_without_a_labeled_set() {
    let log = load_fixture("bench_two_case_run.jsonl");
    let report = collate(&log, &expected_cases());

    match report.precision_recall {
        Seam::Inconclusive { reason } => assert_eq!(
            reason, NO_LABELED_SET,
            "CRITERION 4: the unfed seam announces itself with the exact I6 disclosure string"
        ),
        Seam::Scored { precision, recall } => panic!(
            "CRITERION 4 VIOLATION: no labeled set was supplied, yet the seam returned a \
             fabricated score (precision={precision}, recall={recall}) — I6 forbids coercing a \
             missing measurement into a pass"
        ),
    }
}
