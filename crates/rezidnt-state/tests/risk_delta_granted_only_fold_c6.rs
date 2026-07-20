//! C6 oracle (DR-024 — the GRANTED-only risk fold) — the reducer half of the Q3
//! honesty property, at the state level (host-runnable, platform-neutral), the
//! RISK analogue of C1's `action_metered_fold.rs` denied-charges-zero test.
//!
//! DR-024 Q3 narrows the `risk_delta` fold (`rezidnt-state/src/lib.rs:750-752`)
//! from ALL outcomes to the `"granted"` arm ONLY. A DENIED or ESCALATED action
//! never ran, so folding its risk is a phantom charge — the exact I3 dishonesty
//! DR-021 B2 refused for spend. Only actions that HAPPENED contribute running
//! risk; the permit fact still RECORDS the pre-action assessment on all arms,
//! but the accumulator COUNTS only granted risk.
//!
//! RED MODE (honest — the fold still folds all outcomes):
//!   Today `apply_permit_decision` folds `risk_delta` UNCONDITIONALLY for
//!   granted/denied/escalated (`:750-752` has no decision guard). So a stray
//!   `risk_delta` on a `permit.denied`/`.escalated` fact WRONGLY folds into
//!   `risk_score` → the "denied/escalated fold ZERO" assertions FAIL today
//!   (they fold the stray value, not 0.0). GREEN only once the fold moves inside
//!   the `"granted"` arm.

use rezidnt_state::{Materializer, fold};
use rezidnt_types::{Event, SourceId, Subject};
use serde_json::{Value, json};
use ulid::Ulid;

const RUN: &str = "01C6RISKFOLD0000000000R01";

fn ev(subject: &str, payload: Value) -> Event {
    Event::new(
        SourceId::new("rezidnt-gate"),
        None,
        Subject::new(subject),
        Ulid::new(),
        None,
        1,
        payload,
    )
    .expect("test event under 32KiB")
}

/// A permit decision fact carrying a `risk_delta` (the pre-action assessment the
/// emit site stamps). `req` uniquely keys the ledger entry.
fn decision(subject: &str, req: &str, risk: f64) -> Event {
    ev(
        subject,
        json!({
            "run": RUN,
            "request_id": req,
            "policy_ref": {"hash": "r15kd3l7a0000000000000000000000000000000000000000000000000000c6", "bytes": 16, "mime": "application/octet-stream"},
            "risk_delta": risk,
        }),
    )
}

/// CRITERION 6 (the grant leg) — a GRANTED action folds its `risk_delta` into
/// `cumulative_risk_score` (the running score the next decision reads). The
/// action HAPPENED, so its assessed risk is honestly counted.
#[test]
fn granted_action_folds_its_risk_delta() {
    let graph = fold([decision("permit.granted", "01C6GRANTREQ00000000000Q1", 4.0)].iter());
    let acc = &graph
        .agent_runs
        .get(RUN)
        .expect("a permit fact mints the run entry (I3)")
        .permit_accumulators;
    assert_eq!(
        acc.risk_score, 4.0,
        "a GRANTED action's risk_delta folds into cumulative_risk_score (CRITERION 6)"
    );
    assert_eq!(acc.granted, 1, "the grant is counted");
}

/// CRITERION 6 (the DENIED honesty leg) — a DENIED action folds ZERO risk, even
/// carrying a stray `risk_delta`. The action never ran; charging its assessed
/// risk would be a phantom charge (the Q3 dishonesty). The denial is still
/// COUNTED as a decision — recorded-not-charged.
///
/// RED today: the unconditional fold at :750-752 charges the stray 3.0 → 3.0,
/// not 0.0. Green only once the fold narrows to the `"granted"` arm.
#[test]
fn denied_action_folds_zero_risk_no_phantom_charge() {
    let graph = fold([decision("permit.denied", "01C6DENYREQ000000000000Q2", 3.0)].iter());
    let acc = &graph.agent_runs[RUN].permit_accumulators;
    assert_eq!(
        acc.risk_score, 0.0,
        "a DENIED action never ran → its risk_delta folds ZERO — no phantom charge \
         (CRITERION 6, the Q3 honesty property)"
    );
    assert_eq!(acc.denied, 1, "the denial is still counted as a decision");
}

/// CRITERION 6 (the ESCALATED honesty leg) — an ESCALATED action folds ZERO
/// risk, even carrying a stray `risk_delta`. Escalation routes to a human; the
/// action did not run, so it charges no running risk. Counted, not charged.
///
/// RED today: the unconditional fold charges the stray 5.0 → 5.0, not 0.0.
#[test]
fn escalated_action_folds_zero_risk_no_phantom_charge() {
    let graph = fold(
        [decision(
            "permit.escalated",
            "01C6ESCREQ0000000000000Q3",
            5.0,
        )]
        .iter(),
    );
    let acc = &graph.agent_runs[RUN].permit_accumulators;
    assert_eq!(
        acc.risk_score, 0.0,
        "an ESCALATED action never ran → its risk_delta folds ZERO — no phantom charge \
         (CRITERION 6, the Q3 honesty property)"
    );
    assert_eq!(
        acc.escalated, 1,
        "the escalation is still counted as a decision"
    );
}

/// CRITERION 6 (the source is the outcome, not the field) — over a mix of all
/// three outcomes each carrying a `risk_delta`, ONLY the granted deltas sum. This
/// asserts the fold keys on the DECISION arm, not merely on key-presence: the
/// denied/escalated facts carry the same key and are ignored.
///
/// RED today: the unconditional fold sums all three (2.0 + 3.0 + 5.0 = 10.0),
/// not the granted-only 2.0.
#[test]
fn only_granted_deltas_sum_across_a_mixed_run() {
    let graph = fold(
        [
            decision("permit.granted", "01C6MIXGRANT000000000Q10", 2.0),
            decision("permit.denied", "01C6MIXDENY0000000000Q11", 3.0),
            decision("permit.escalated", "01C6MIXESC00000000000Q12", 5.0),
        ]
        .iter(),
    );
    let acc = &graph.agent_runs[RUN].permit_accumulators;
    assert_eq!(
        acc.risk_score, 2.0,
        "only the GRANTED action's risk (2.0) folds; the denied (3.0) and escalated (5.0) \
         deltas fold ZERO — the fold keys on the OUTCOME, not the field (CRITERION 6)"
    );
    assert_eq!(acc.granted, 1);
    assert_eq!(acc.denied, 1);
    assert_eq!(acc.escalated, 1);
}

/// CRITERION 6 (rebuild-safe) — the granted-only risk fold is rebuild-safe:
/// incremental `Materializer::apply` equals `fold`-from-zero over an interleaving
/// of all three outcomes (the release-blocking `fold(log) == snapshot` family). A
/// divergence here is a reducer bug, not a flaky test.
#[test]
fn granted_only_risk_fold_is_rebuild_safe() {
    let events = [
        decision("permit.granted", "01C6RBLDGRANT00000000Q20", 1.5),
        decision("permit.denied", "01C6RBLDDENY000000000Q21", 9.0),
        decision("permit.granted", "01C6RBLDGRANT200000000Q22", 2.5),
        decision("permit.escalated", "01C6RBLDESC0000000000Q23", 9.0),
    ];
    let folded = fold(events.iter());
    assert_eq!(
        folded.agent_runs[RUN].permit_accumulators.risk_score, 4.0,
        "only the two granted deltas fold: 1.5 + 2.5 = 4.0 (denied/escalated ignored)"
    );

    let mut live = Materializer::new();
    for event in &events {
        live.apply(event);
    }
    assert_eq!(
        live.snapshot(),
        folded,
        "incremental application equals fold-from-zero — the granted-only risk fold is rebuild-safe"
    );
}
