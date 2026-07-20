//! SP0 oracle — the permit reducer fold (DR-008 / DR-009). The `permit.*`
//! subjects fold into a per-run permit ledger (`AgentRunState::permit_ledger`,
//! keyed by `request_id`) AND per-session accumulators
//! (`AgentRunState::permit_accumulators`).
//!
//! GREEN MODE (honest, and correct): the reducer was landed by the warden's
//! `/subject` scaffolding pass, so these fold tests PASS NOW. That is the
//! intent — they LOCK the warden's scaffolding (the fold half of "no
//! consumer-less subject", DR-006 precedent) so a later edit that breaks the
//! request→decision fold, drops `policy_ref`, coerces `escalated`, or
//! mis-accumulates spend/risk turns them red. The SP0 tests that must FAIL
//! pending implementation are the gate-engine ones (`rezidnt-gate`:
//! `permit_lifecycle.rs`, `permit_emit.rs`) — the lifecycle point + mapping +
//! emit path. This file pins the consumer those producers must match.
//!
//! Shapes asserted verbatim from `spec/ontology.md` "permit set" (payload
//! schemas): `permit.requested {run, request_id, action, ...}`;
//! `permit.granted|denied|escalated {run, request_id, policy_ref: CasRef,
//! reason?, risk_delta?, ...}`. Per DR-021 the C1 spend fold source moved OFF
//! the permit facts: `spend_delta_usd?` is RETIRED from the permit payloads and
//! now rides the post-action `action.metered` fact (folded there, not here).
//! `risk_delta?` STAYS on the permit path (C6, untouched).

use rezidnt_state::{Materializer, fold};
use rezidnt_types::{Event, SourceId, Subject};
use serde_json::{Value, json};
use ulid::Ulid;

const RUN: &str = "01SP0PERMITLEDGER00000000R01";

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

/// CRITERION 4 (fold leg) — a request→decision on ONE request folds onto ONE
/// ledger entry (keyed by `request_id`): the request records the action
/// pending, the decision fills the outcome + `policy_ref` so the entry is
/// interrogable (I6). request→granted here.
#[test]
fn request_then_grant_folds_to_one_interrogable_ledger_entry() {
    const REQ: &str = "01SP0REQ0GRANT0000000000Q01";
    let events = [
        ev(
            "permit.requested",
            json!({"run": RUN, "request_id": REQ, "action": "tool.invoke",
                   "target": {"tool": "Read"}}),
        ),
        ev(
            "permit.granted",
            json!({"run": RUN, "request_id": REQ,
                   "policy_ref": {"hash": "po11cygranted00000000000000000000000000000000000000000000000001", "bytes": 64, "mime": "application/octet-stream"},
                   "risk_delta": 1.0}),
        ),
        // The MEASURED spend for the granted action rides a POST-action
        // `action.metered` fact — the C1 fold source after DR-021, no longer the
        // permit fact.
        ev(
            "action.metered",
            json!({"run": RUN, "spend_delta_usd": 0.5, "input_tokens": 100u64, "output_tokens": 40u64}),
        ),
    ];
    let graph = fold(events.iter());
    let run = graph
        .agent_runs
        .get(RUN)
        .expect("a permit fact creates the run entry — no spawn required (I3)");
    assert_eq!(
        run.permit_ledger.len(),
        1,
        "request and decision fold onto ONE entry"
    );
    let entry = run
        .permit_ledger
        .get(REQ)
        .expect("the request_id keys the entry");
    assert_eq!(
        entry.action, "tool.invoke",
        "the requested action is recorded"
    );
    assert_eq!(
        entry.decision.as_deref(),
        Some("granted"),
        "the decision fills the ledger outcome"
    );
    assert_eq!(
        entry.policy_ref.as_deref(),
        Some("po11cygranted00000000000000000000000000000000000000000000000001"),
        "the deciding policy_ref folds so `gate_explain` can resolve it (I6)"
    );
}

/// CRITERION 4 (fold leg) — a DENY folds with its `reason` verbatim so a
/// blocked agent can read WHY (I6), and the per-session accumulators count the
/// denial + fold its risk delta.
#[test]
fn deny_folds_reason_and_accumulates() {
    const REQ: &str = "01SP0REQ00DENY0000000000Q02";
    let events = [
        ev(
            "permit.requested",
            json!({"run": RUN, "request_id": REQ, "action": "tool.invoke",
                   "target": {"tool": "Bash"}}),
        ),
        ev(
            "permit.denied",
            json!({"run": RUN, "request_id": REQ,
                   "policy_ref": {"hash": "po11cydenied000000000000000000000000000000000000000000000000de", "bytes": 32, "mime": "application/octet-stream"},
                   "reason": "path outside allowed scope", "risk_delta": 3.0}),
        ),
    ];
    let graph = fold(events.iter());
    let run = &graph.agent_runs[RUN];
    let entry = &run.permit_ledger[REQ];
    assert_eq!(entry.decision.as_deref(), Some("denied"));
    assert_eq!(
        entry.reason.as_deref(),
        Some("path outside allowed scope"),
        "the denial reason folds verbatim (I6)"
    );
    assert_eq!(run.permit_accumulators.denied, 1);
    assert_eq!(run.permit_accumulators.risk_score, 3.0);
}

/// CRITERION 3 (fold leg) — an ESCALATE folds as `escalated`, NEVER coerced to
/// `granted` or `denied` (I6, DR-008 §4). The load-bearing honesty test on the
/// consumer side: the reducer records the inconclusive→human outcome as itself.
#[test]
fn escalate_folds_as_escalated_never_coerced() {
    const REQ: &str = "01SP0REQ0ESCAL0000000000Q03";
    let events = [
        ev(
            "permit.requested",
            json!({"run": RUN, "request_id": REQ, "action": "tool.invoke"}),
        ),
        ev(
            "permit.escalated",
            json!({"run": RUN, "request_id": REQ,
                   "policy_ref": {"hash": "po11cyescalated0000000000000000000000000000000000000000000000es", "bytes": 32, "mime": "application/octet-stream"},
                   "reason": "no policy matched"}),
        ),
    ];
    let graph = fold(events.iter());
    let entry = &graph.agent_runs[RUN].permit_ledger[REQ];
    assert_eq!(
        entry.decision.as_deref(),
        Some("escalated"),
        "inconclusive→human folds as escalated, never coerced (I6, DR-008 §4)"
    );
    assert_ne!(entry.decision.as_deref(), Some("granted"));
    assert_ne!(entry.decision.as_deref(), Some("denied"));
    assert_eq!(graph.agent_runs[RUN].permit_accumulators.escalated, 1);
}

/// I3 — a decision that folds BEFORE its request (out-of-order log) still
/// creates the ledger entry; the later request fills the action without
/// clobbering the decision. The log is truth; the reducer never gatekeeps.
#[test]
fn decision_before_request_still_folds_the_log_is_truth() {
    const REQ: &str = "01SP0REQ00OOO00000000000Q04";
    let events = [
        ev(
            "permit.granted",
            json!({"run": RUN, "request_id": REQ,
                   "policy_ref": {"hash": "po11cyoutoforder00000000000000000000000000000000000000000000oo", "bytes": 16, "mime": "application/octet-stream"}}),
        ),
        ev(
            "permit.requested",
            json!({"run": RUN, "request_id": REQ, "action": "tool.invoke"}),
        ),
    ];
    let graph = fold(events.iter());
    let entry = &graph.agent_runs[RUN].permit_ledger[REQ];
    assert_eq!(
        entry.decision.as_deref(),
        Some("granted"),
        "decision survives a later request"
    );
    assert_eq!(
        entry.action, "tool.invoke",
        "the late request fills the action"
    );
}

/// I3 — a `permit.*` fact missing `run`/`request_id` folds as counters-only:
/// the reducer never guesses a key, never chokes.
#[test]
fn keyless_permit_fact_folds_counters_only() {
    let events = [ev("permit.granted", json!({"policy_ref": {"hash": "x"}}))];
    let graph = fold(events.iter());
    assert_eq!(graph.events_folded, 1, "the fact is still counted");
    assert!(
        graph.agent_runs.is_empty(),
        "a keyless permit fact mints no run entry (I3)"
    );
}

// --- property: permit folds accumulate correctly + rebuild-safe -------------

mod props {
    use super::*;
    use proptest::prelude::*;

    const RUNS: [&str; 2] = ["01SP0PR0PPERMIT0000000R01", "01SP0PR0PPERMIT0000000R02"];
    // 0 = granted, 1 = denied, 2 = escalated
    const DECISIONS: [&str; 3] = ["permit.granted", "permit.denied", "permit.escalated"];

    // The permit DECISION fact carries risk (C6, still on the permit path) and
    // the decision outcome — but NO spend. Per DR-021 the C1 spend fold source
    // moved off the permit fact.
    fn decision_ev(run: &str, req: &str, kind: usize, risk: f64) -> Event {
        ev(
            DECISIONS[kind],
            json!({
                "run": run,
                "request_id": req,
                "policy_ref": {"hash": "po11cyprop000000000000000000000000000000000000000000000000prop", "bytes": 8, "mime": "application/octet-stream"},
                "reason": "prop",
                "risk_delta": risk,
            }),
        )
    }

    // The MEASURED spend for an action rides a POST-action `action.metered` fact
    // (the C1 fold source after DR-021), keyed on the run.
    fn metered_ev(run: &str, spend: f64) -> Event {
        ev(
            "action.metered",
            json!({"run": run, "spend_delta_usd": spend, "input_tokens": 10u64, "output_tokens": 5u64}),
        )
    }

    proptest! {
        /// For ARBITRARY interleavings of permit decisions across two runs:
        /// (a) each run's accumulators equal an independently computed model —
        /// per-outcome counts and summed spend/risk deltas; and (b) incremental
        /// Materializer application equals fold-from-zero (the release-blocking
        /// `fold(log) == snapshot` / rebuild family). `rebuild` is exactly
        /// fold-from-zero, so (b) is "rebuild reproduces the permit state."
        #[test]
        fn permit_accumulators_match_model_and_incremental_equals_fold(
            seq in proptest::collection::vec(
                (0usize..2, 0usize..3, 0u64..5, 0u64..5),
                1..40,
            )
        ) {
            // Each event gets a unique request_id so entries do not overwrite
            // (independent decisions, not request→decision on one entry). The
            // permit decision fact folds risk + the outcome count; a PAIRED
            // `action.metered` fact folds the measured spend (DR-021 fold source).
            let events: Vec<Event> = seq
                .iter()
                .enumerate()
                .flat_map(|(i, &(r, k, s, rk))| {
                    let req = format!("01SP0PROPREQ{i:015}");
                    [
                        decision_ev(RUNS[r], &req, k, rk as f64),
                        metered_ev(RUNS[r], s as f64),
                    ]
                })
                .collect();

            // Independent model of the accumulators per run.
            #[derive(Default)]
            struct Model { granted: u64, denied: u64, escalated: u64, spend: f64, risk: f64 }
            let mut model: std::collections::BTreeMap<&str, Model> = std::collections::BTreeMap::new();
            for &(r, k, s, rk) in &seq {
                let m = model.entry(RUNS[r]).or_default();
                match k { 0 => m.granted += 1, 1 => m.denied += 1, _ => m.escalated += 1 }
                m.spend += s as f64;
                m.risk += rk as f64;
            }

            let folded = fold(events.iter());
            for (run, m) in &model {
                let acc = &folded.agent_runs.get(*run).expect("run entry exists").permit_accumulators;
                prop_assert_eq!(acc.granted, m.granted, "granted count matches model for {}", run);
                prop_assert_eq!(acc.denied, m.denied, "denied count matches model for {}", run);
                prop_assert_eq!(acc.escalated, m.escalated, "escalated count matches model for {}", run);
                prop_assert_eq!(acc.cumulative_spend_usd, m.spend, "spend sum matches model for {}", run);
                prop_assert_eq!(acc.risk_score, m.risk, "risk sum matches model for {}", run);
            }

            let mut live = Materializer::new();
            for event in &events {
                live.apply(event);
            }
            prop_assert_eq!(live.snapshot(), folded, "incremental == fold-from-zero (rebuild)");
        }
    }
}
