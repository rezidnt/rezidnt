//! Oracle (CRITERION 3) — `cost_ms` is RECORDED-ONLY: it folds into NO
//! accumulator. A decision fact bearing `cost_ms` (and no spend/risk) leaves
//! `cumulative_spend_usd` and `risk_score` at zero.
//!
//! REGRESSION-LOCK MODE: this test PINS that `cost_ms` folds into NO accumulator
//! — it turns RED the instant an edit makes `cost_ms` fold into one (design
//! §10.2 / SP5 documented `cost_ms` recorded-only). Per DR-021 the C1 spend fold
//! source moved OFF the permit facts onto the post-action `action.metered` fact;
//! the second test below seeds its measured spend there (the honest source),
//! `cost_ms` still folds nowhere. This mirrors the SP0 `permit_ledger.rs`
//! fold locks — it exercises a real property whose invariant it guards. The
//! behavior under construction (the PDP MEASURING and populating `cost_ms`) is
//! oracle-tested RED in `rezidnt-mcp/tests/permit_cost_ms.rs`.

use rezidnt_state::{Materializer, fold};
use rezidnt_types::{Event, SourceId, Subject};
use serde_json::{Value, json};
use ulid::Ulid;

const RUN: &str = "01COSTMSRECORDEDONLY0000R01";

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

/// CRITERION 3 — a granted decision carrying ONLY `cost_ms` (no
/// `spend_delta_usd`, no `risk_delta`) folds to zero spend + zero risk. The
/// `cost_ms` is present on the fact (recorded, replayable, I3) but contributes
/// to NO accumulator.
#[test]
fn cost_ms_only_grant_folds_to_zero_spend_and_zero_risk() {
    const REQ: &str = "01COSTMSREQ0GRANT000000Q01";
    let events = [
        ev(
            "permit.requested",
            json!({"run": RUN, "request_id": REQ, "action": "tool.invoke",
                   "target": {"tool": "Read"}}),
        ),
        ev(
            "permit.granted",
            json!({"run": RUN, "request_id": REQ,
                   "policy_ref": {"hash": "co57m5gr4n700000000000000000000000000000000000000000000000cost1", "bytes": 40, "mime": "application/octet-stream"},
                   // The recorded-only cost field: PRESENT on the fact, but the
                   // reducer must NOT fold it into any accumulator.
                   "cost_ms": 42}),
        ),
    ];
    let graph = fold(events.iter());
    let run = graph
        .agent_runs
        .get(RUN)
        .expect("the permit facts create the run entry (I3)");

    let acc = &run.permit_accumulators;
    assert_eq!(
        acc.cumulative_spend_usd, 0.0,
        "a `cost_ms`-only grant folds NO spend — cost_ms is recorded-only (CRITERION 3)"
    );
    assert_eq!(
        acc.risk_score, 0.0,
        "a `cost_ms`-only grant folds NO risk — cost_ms is recorded-only (CRITERION 3)"
    );
    // The grant IS counted (the decision happened) — recorded-only means it
    // moves no SPEND/RISK accumulator, not that the decision vanishes.
    assert_eq!(acc.granted, 1, "the grant is still counted as a decision");
}

/// CRITERION 3 (interleaving lock) — mixing a `cost_ms`-only fact with a
/// spend/risk-bearing fact leaves the accumulators at EXACTLY the spend/risk
/// contributions, proving `cost_ms` adds nothing. Also asserts incremental
/// application equals fold-from-zero (the rebuild family) so the recorded-only
/// property is rebuild-safe.
#[test]
fn cost_ms_never_perturbs_spend_or_risk_and_is_rebuild_safe() {
    const REQ_COST: &str = "01COSTMSREQ00COST000000Q02";
    const REQ_SPEND: &str = "01COSTMSREQ0SPEND000000Q03";
    let events = [
        // A pure cost_ms decision — must move neither accumulator.
        ev(
            "permit.granted",
            json!({"run": RUN, "request_id": REQ_COST,
                   "policy_ref": {"hash": "co57m50n1y00000000000000000000000000000000000000000000000cost2", "bytes": 8, "mime": "application/octet-stream"},
                   "cost_ms": 7}),
        ),
        // A risk decision that ALSO carries cost_ms — the risk folds, the cost_ms
        // does not. Spend NO LONGER rides the permit fact (DR-021 fold source
        // moved); risk_delta STAYS (C6, untouched).
        ev(
            "permit.denied",
            json!({"run": RUN, "request_id": REQ_SPEND,
                   "policy_ref": {"hash": "co57m5w17h5p3nd000000000000000000000000000000000000000000cost3", "bytes": 8, "mime": "application/octet-stream"},
                   "risk_delta": 4.0, "cost_ms": 99}),
        ),
        // The MEASURED spend rides the post-action `action.metered` fact (the C1
        // fold source after DR-021), also carrying no cost_ms accumulator effect.
        ev(
            "action.metered",
            json!({"run": RUN, "spend_delta_usd": 2.5, "input_tokens": 60u64, "output_tokens": 20u64}),
        ),
    ];

    let folded = fold(events.iter());
    let acc = &folded.agent_runs[RUN].permit_accumulators;
    assert_eq!(
        acc.cumulative_spend_usd, 2.5,
        "spend equals ONLY the spend_delta_usd contribution — cost_ms adds nothing (CRITERION 3)"
    );
    assert_eq!(
        acc.risk_score, 4.0,
        "risk equals ONLY the risk_delta contribution — cost_ms adds nothing (CRITERION 3)"
    );

    // Recorded-only must be rebuild-safe: incremental == fold-from-zero.
    let mut live = Materializer::new();
    for event in &events {
        live.apply(event);
    }
    assert_eq!(
        live.snapshot(),
        folded,
        "incremental application equals fold-from-zero (rebuild reproduces the recorded-only state)"
    );
}
