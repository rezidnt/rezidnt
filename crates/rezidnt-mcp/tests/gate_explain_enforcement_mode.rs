//! SP2 hook sub-slice oracle — CRITERION 3 (gate_explain distinguishes; DR-014
//! §Decision 5, design §6). The `gate_explain` read path must honestly report
//! whether a run was mid-run-PEP-enforced or edge-gated-only, derived from the
//! run's `agent.spawned.pep?` field on the log (I4 degradation honesty; I3 —
//! derived from the log, never a side store).
//!
//! The judge: seed a run whose `agent.spawned` carried `pep: "enforced"` (plus a
//! permit decision so the run is interrogable) and assert `gate_explain` marks
//! it mid-run-enforced; seed an edge-gated-only run (spawn with NO `pep`, plus a
//! gate verdict) and assert `gate_explain` marks it edge-gated-only. The two
//! MUST differ — that difference is the whole point of the field (a
//! `debrief`/`gate_explain` reader must not present an edge-gated run as if it
//! had live interception).
//!
//! RED MODE: **assert-red**. `call_gate_explain` (crates/rezidnt-mcp/src/lib.rs
//! ~line 830) resolves the verdict + policy/evidence/reason but does NOT read
//! the run's enforcement mode — there is no `enforcement` key on the explain
//! payload today. These assertions fail on the absent key until the read path
//! folds `agent.spawned.pep` into the answer.
//!
//! NOTE FOR THE IMPLEMENTER (key/value strings negotiable, the DISTINCTION is
//! not): this board reads `explain["enforcement"]` and expects two distinct
//! machine-readable values — `"mid-run-enforced"` when the run's spawn carried
//! `pep = "enforced"`, `"edge-gated-only"` when it did not. If you name the key
//! or the values differently, adjust here to match — the LOAD-BEARING pin is
//! that the two runs get DIFFERENT, honest values and the edge-gated run is
//! never labelled as enforced.

mod util;

use serde_json::json;

const ENFORCED_RUN: &str = "01SP2PEPENF0RCED0000RN0001";
const EDGE_RUN: &str = "01SP2PEPEDGE0N1Y0000RN0002";

const MID_RUN: &str = "mid-run-enforced";
const EDGE_GATED: &str = "edge-gated-only";

/// CRITERION 3 (mid-run leg) — a run whose `agent.spawned` carried
/// `pep: "enforced"` is reported by `gate_explain` as mid-run-enforced. The
/// permit decision on the same run keeps it interrogable (not `gate.no_verdict`).
///
/// ASSERT-RED until `gate_explain` surfaces the enforcement mode.
#[tokio::test]
async fn gate_explain_reports_mid_run_enforced_for_a_pep_wired_run() {
    let (_dir, core) = util::core();
    util::seed_fixture(&core, "sp2_pep_mid_run_enforced.jsonl");

    let result = util::tool_call(&core, 1, "gate_explain", json!({"run": ENFORCED_RUN})).await;
    assert_ne!(
        result["isError"],
        json!(true),
        "a PEP-enforced run with a permit decision is interrogable: {result:#}"
    );
    let explain = util::tool_payload(&result);
    assert_eq!(
        explain["enforcement"],
        json!(MID_RUN),
        "a run whose agent.spawned carried pep=\"enforced\" is reported \
         mid-run-enforced (DR-014 §Decision 5; I4): {explain:#}"
    );
    // The verdict axis is unchanged — the permit deny still surfaces.
    assert_eq!(
        explain["verdict"],
        json!("deny"),
        "the permit deny still surfaces (SP1 leg not regressed): {explain:#}"
    );
}

/// CRITERION 3 (edge-gated leg — the honesty half) — a run whose `agent.spawned`
/// carried NO `pep` field is reported edge-gated-only: it got pre-spawn `vet` +
/// post-hoc `debrief` evidence but NO mid-run interception (design §6). This is
/// the assertion that stops the reader from presenting an edge-gated run as if
/// it had live enforcement.
///
/// ASSERT-RED until the enforcement mode is surfaced.
#[tokio::test]
async fn gate_explain_reports_edge_gated_only_for_a_run_with_no_pep() {
    let (_dir, core) = util::core();
    util::seed_fixture(&core, "sp2_pep_edge_gated_only.jsonl");

    let result = util::tool_call(&core, 2, "gate_explain", json!({"run": EDGE_RUN})).await;
    assert_ne!(
        result["isError"],
        json!(true),
        "an edge-gated run with a gate verdict is interrogable: {result:#}"
    );
    let explain = util::tool_payload(&result);
    assert_eq!(
        explain["enforcement"],
        json!(EDGE_GATED),
        "a run whose agent.spawned carried NO pep is reported edge-gated-only — \
         never labelled as if mid-run-enforced (I4 degradation honesty; design §6): {explain:#}"
    );
    assert_ne!(
        explain["enforcement"],
        json!(MID_RUN),
        "an edge-gated run is NEVER reported mid-run-enforced (the load-bearing \
         distinction; DR-014 §Decision 5): {explain:#}"
    );
}

/// CRITERION 3 (the distinction, side by side) — the two runs get DIFFERENT
/// enforcement values from `gate_explain`. If they ever collapse to one value,
/// the field is surfaced-nowhere and the I4 honesty is gone.
///
/// ASSERT-RED until the read path distinguishes them.
#[tokio::test]
async fn gate_explain_enforcement_modes_are_distinct() {
    let (_dir, core) = util::core();
    util::seed_fixture(&core, "sp2_pep_mid_run_enforced.jsonl");
    util::seed_fixture(&core, "sp2_pep_edge_gated_only.jsonl");

    let enforced = util::tool_payload(
        &util::tool_call(&core, 3, "gate_explain", json!({"run": ENFORCED_RUN})).await,
    );
    let edge = util::tool_payload(
        &util::tool_call(&core, 4, "gate_explain", json!({"run": EDGE_RUN})).await,
    );
    assert_ne!(
        enforced["enforcement"], edge["enforcement"],
        "mid-run-enforced and edge-gated-only must be DISTINGUISHABLE at the \
         gate_explain surface (DR-014 §Decision 5; I4): enforced={enforced:#}, edge={edge:#}"
    );
}
