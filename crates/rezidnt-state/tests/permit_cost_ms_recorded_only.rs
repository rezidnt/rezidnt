//! Oracle (CRITERION 3) — `cost_ms` is RECORDED-ONLY: it folds into NO
//! accumulator. A decision fact bearing `cost_ms` (and no spend/risk) leaves
//! `cumulative_spend_usd` and `risk_score` at zero.
//!
//! GREEN MODE (honest, and correct — the testing-oracles rule): unlike the
//! rezidnt-mcp `permit_cost_ms.rs` suite (RED against the not-yet-built PDP
//! timer), THIS test is GREEN NOW. The reducer at
//! `crates/rezidnt-state/src/lib.rs:725-729` already reads ONLY
//! `spend_delta_usd`/`risk_delta` and never touches `cost_ms`, so a
//! `cost_ms`-only decision fact already folds to zero spend + zero risk. That is
//! the intent: this test is a REGRESSION LOCK that PINS the reducer STAYS
//! recorded-only — it turns RED the instant an edit makes `cost_ms` fold into an
//! accumulator (design §10.2 / SP5 documented `cost_ms` recorded-only). This
//! mirrors the SP0 `permit_ledger.rs` GREEN-by-design fold locks, and is NOT the
//! honesty defect the oracle refuses (a would-be test of unbuilt behavior that
//! passes vacuously) — it exercises a real, existing property whose invariant it
//! guards. The behavior under construction (the PDP MEASURING and populating
//! `cost_ms`) is oracle-tested RED in `rezidnt-mcp/tests/permit_cost_ms.rs`.

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
        // A spend+risk decision that ALSO carries cost_ms — the spend/risk fold,
        // the cost_ms does not.
        ev(
            "permit.denied",
            json!({"run": RUN, "request_id": REQ_SPEND,
                   "policy_ref": {"hash": "co57m5w17h5p3nd000000000000000000000000000000000000000000cost3", "bytes": 8, "mime": "application/octet-stream"},
                   "spend_delta_usd": 2.5, "risk_delta": 4.0, "cost_ms": 99}),
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
