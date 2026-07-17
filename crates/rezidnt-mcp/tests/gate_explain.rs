//! S3 oracle — `gate_explain` interrogability (I6, doc §8): the failing
//! verifier, its evidence refs, and the EXACT inputs.
//!
//! S3's honest slice of "forced failure", pinned here: a stub verdict ON THE
//! LOG. The S4 verifier engine does not exist; what exists is a `gate.failed`
//! fact (golden fixture `s3_gate_forced_failure.jsonl`) that `gate_explain`
//! must interrogate — log is truth (I3), explanation is derived. Nothing in
//! this board requires executing a verifier.
//!
//! Payload-shape caveat (flagged in the oracle work order): the ontology
//! ratifies no v1 payload baseline for `gate.entered` / `gate.failed` /
//! `gate.inconclusive` / `gate.explained` — the fixtures and assertions pin
//! the semantically forced minimum of doc §8 (run, gate, verifier, evidence
//! refs, exact inputs). Warden ratification via /subject is required before
//! the implementer freezes a richer shape.

mod util;

use serde_json::json;

const FAILED_RUN: &str = "01S3GATEFA1DED000000000R01";
const INCONCLUSIVE_RUN: &str = "01S3GATE1NC0NC000000000R02";

/// THE criterion (exit d): explaining a forced failure returns the failing
/// verifier, its evidence refs, and the exact inputs — and leaves a
/// `gate.explained` fact on the log (interrogations are facts too).
#[tokio::test]
async fn forced_failure_is_explained_with_verifier_evidence_and_exact_inputs() {
    let (_dir, core) = util::core();
    let seeded = util::seed_fixture(&core, "s3_gate_forced_failure.jsonl");
    let failed = seeded
        .iter()
        .find(|e| e.subject.as_str() == "gate.failed")
        .expect("fixture carries the forced failure");

    let result = util::tool_call(&core, 1, "gate_explain", json!({"run": FAILED_RUN})).await;
    assert_ne!(
        result["isError"],
        json!(true),
        "a run with a verdict on the log must be explainable: {result:#}"
    );
    let explain = util::tool_payload(&result);

    assert_eq!(explain["run"], json!(FAILED_RUN));
    assert_eq!(
        explain["verdict"],
        json!("fail"),
        "the recorded verdict, verbatim — pass|fail|inconclusive, never a bool (I6)"
    );
    assert_eq!(
        explain["verifier"],
        json!("tests-pass"),
        "the FAILING verifier is named (doc §8)"
    );
    assert_eq!(
        explain["evidence"],
        failed.payload()["evidence"],
        "evidence refs are returned exactly as recorded (CAS refs, I2)"
    );
    assert_eq!(
        explain["inputs"],
        failed.payload()["inputs"],
        "the EXACT inputs, verbatim from the log — this is what lets a blocked agent fix the defect instead of thrashing (doc §8)"
    );

    // The interrogation itself lands on the log (minimum shape: run).
    let explained: Vec<_> = util::log_events(&core)
        .into_iter()
        .filter(|e| e.subject.as_str() == "gate.explained")
        .collect();
    assert_eq!(
        explained.len(),
        1,
        "one interrogation, one gate.explained fact"
    );
    assert_eq!(explained[0].payload()["run"], json!(FAILED_RUN));
}

/// I6: `inconclusive` is NEVER coerced. Explaining an inconclusive verdict
/// reports `inconclusive` — not pass, not fail.
#[tokio::test]
async fn inconclusive_verdict_is_never_coerced() {
    let (_dir, core) = util::core();
    util::seed_fixture(&core, "s3_gate_inconclusive.jsonl");

    let result = util::tool_call(&core, 2, "gate_explain", json!({"run": INCONCLUSIVE_RUN})).await;
    let explain = util::tool_payload(&result);
    assert_eq!(
        explain["verdict"],
        json!("inconclusive"),
        "inconclusive stays inconclusive (I6) — got {explain:#}"
    );
    assert_eq!(explain["verifier"], json!("tests-pass"));
}

/// A run with NO gate verdict on the log gets a machine-readable
/// `gate.no_verdict` refusal — honest absence, never an implicit pass.
#[tokio::test]
async fn run_without_verdict_is_a_machine_readable_absence_not_a_pass() {
    let (_dir, core) = util::core();
    util::seed_fixture(&core, "s1_agent_run.jsonl"); // a real run, zero gate facts

    let result = util::tool_call(
        &core,
        3,
        "gate_explain",
        json!({"run": "01ARZ3NDEKTSV4RRFFQ69G5A01"}),
    )
    .await;
    util::assert_tool_refusal(&result, rezidnt_mcp::codes::GATE_NO_VERDICT);
}
