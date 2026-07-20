//! SP4b ORACLE — the `permit.delegated` folding reducer (DR-017 §Decision 2,
//! ontology line 408-423). FAILING-FIRST: `AgentRunState::delegations` and the
//! `permit.delegated` reducer arm DO NOT EXIST YET, so these tests fail to
//! compile (`no field delegations` / `DelegationRecord` unresolved) until the
//! implementer lands the fold. That is the correct red state — mirrors the
//! `permit_ledger.rs` consumer but for the capability-chain fact.
//!
//! ## API surface this board PINS (implementer builds to exactly this)
//! A new `#[serde(default)]` field on `AgentRunState` in
//! `crates/rezidnt-state/src/lib.rs`:
//! ```ignore
//! #[serde(default)]
//! pub delegations: Vec<DelegationRecord>,
//! ```
//! with `pub struct DelegationRecord { parent_badge_id: String,
//! child_badge_id: String, added_caveats: Vec<serde_json::Value> }`
//! (`Debug, Clone, Default, PartialEq, Serialize, Deserialize`). Deterministic
//! order (append order — the log's order, which is deterministic), rebuild-
//! stable via the same `#[serde(default)]` discipline `integrity_alarms` /
//! `permit_ledger` / `intent` use so every pre-DR-017 golden fixture parses and
//! compares equal unchanged (I3). `added_caveats` folds VERBATIM (the tagged
//! Caveat JSON objects — reducers never re-derive; the log is truth).
//!
//! ## Ratified payload (ontology line 408-416)
//! `permit.delegated { run, parent_badge_id, child_badge_id, added_caveats: [Caveat] }`

use rezidnt_state::{Materializer, fold};
use rezidnt_types::{Event, SourceId, Subject};
use serde_json::{Value, json};
use ulid::Ulid;

const RUN: &str = "01SP4BDELEGATION0000000R01";

fn ev(subject: &str, payload: Value) -> Event {
    Event::new(
        SourceId::new("rezidnt-run"),
        None,
        Subject::new(subject),
        Ulid::new(),
        None,
        1,
        payload,
    )
    .expect("test event under 32KiB")
}

/// CRITERION 7 (fold leg) — a `permit.delegated` fact folds onto the run's
/// dossier as one `DelegationRecord` capturing the capability edge
/// (parent → child) and the caveats appended at this step, VERBATIM (I3).
#[test]
fn delegation_folds_onto_the_run_dossier() {
    let events = [ev(
        "permit.delegated",
        json!({
            "run": RUN,
            "parent_badge_id": "a1b2c3d4",
            "child_badge_id": "e5f6a7b8",
            "added_caveats": [
                {"kind": "verb", "verbs": ["open"]},
                {"kind": "expiry", "not_after": "2026-07-20T00:00:00Z"}
            ]
        }),
    )];
    let graph = fold(events.iter());
    let run = graph
        .agent_runs
        .get(RUN)
        .expect("a delegation fact creates the run entry — no spawn required (I3)");
    assert_eq!(
        run.delegations.len(),
        1,
        "one delegation folds to one record"
    );
    let rec = &run.delegations[0];
    assert_eq!(
        rec.parent_badge_id, "a1b2c3d4",
        "the parent end of the capability edge"
    );
    assert_eq!(
        rec.child_badge_id, "e5f6a7b8",
        "the child end of the capability edge"
    );
    assert_eq!(
        rec.added_caveats,
        vec![
            json!({"kind": "verb", "verbs": ["open"]}),
            json!({"kind": "expiry", "not_after": "2026-07-20T00:00:00Z"}),
        ],
        "the narrowing caveats fold verbatim — reducers never re-derive (I3)"
    );
}

/// I3 — two delegations at sub-spawns on one run fold as an ordered chain (the
/// capability chain replays). Append order == log order == deterministic.
#[test]
fn multiple_delegations_fold_as_an_ordered_chain() {
    let events = [
        ev(
            "permit.delegated",
            json!({
                "run": RUN, "parent_badge_id": "root0000", "child_badge_id": "mid00000",
                "added_caveats": [{"kind": "workspace", "workspace": "01ARZ3NDEKTSV4RRFFQ69G5FAV"}]
            }),
        ),
        ev(
            "permit.delegated",
            json!({
                "run": RUN, "parent_badge_id": "mid00000", "child_badge_id": "leaf0000",
                "added_caveats": [{"kind": "role", "role": "sub"}]
            }),
        ),
    ];
    let graph = fold(events.iter());
    let run = &graph.agent_runs[RUN];
    assert_eq!(run.delegations.len(), 2, "each delegation earns a row (I3)");
    assert_eq!(
        run.delegations[0].child_badge_id, "mid00000",
        "first hop folds first"
    );
    assert_eq!(
        run.delegations[1].parent_badge_id, "mid00000",
        "the chain links: hop 2's parent is hop 1's child"
    );
    assert_eq!(
        run.delegations[1].child_badge_id, "leaf0000",
        "second hop folds second"
    );
}

/// I3 — a keyless `permit.delegated` (missing `run`) folds counters-only /
/// no-op, never panics (the established permit-reducer discipline). The reducer
/// never guesses a key.
#[test]
fn keyless_delegation_folds_counters_only() {
    let events = [ev(
        "permit.delegated",
        json!({"parent_badge_id": "orphan00", "child_badge_id": "orphan01", "added_caveats": []}),
    )];
    let graph = fold(events.iter());
    assert_eq!(graph.events_folded, 1, "the fact is still counted");
    assert!(
        graph.agent_runs.is_empty(),
        "a keyless delegation mints no run entry (I3)"
    );
}

/// I3 — a delegation with no `added_caveats` array folds an empty caveat list,
/// never chokes (a malformed/partial payload is still a fact in the log).
#[test]
fn delegation_missing_caveats_folds_empty_never_chokes() {
    let events = [ev(
        "permit.delegated",
        json!({"run": RUN, "parent_badge_id": "aa", "child_badge_id": "bb"}),
    )];
    let graph = fold(events.iter());
    let rec = &graph.agent_runs[RUN].delegations[0];
    assert!(
        rec.added_caveats.is_empty(),
        "absent added_caveats folds empty, never panics (I3)"
    );
}

// --- property: delegation folds are ordered + rebuild-safe ------------------

mod props {
    use super::*;
    use proptest::prelude::*;

    const RUNS: [&str; 2] = ["01SP4BPROPDELEG000000R01", "01SP4BPROPDELEG000000R02"];

    fn delegation_ev(run: &str, parent: &str, child: &str) -> Event {
        ev(
            "permit.delegated",
            json!({
                "run": run,
                "parent_badge_id": parent,
                "child_badge_id": child,
                "added_caveats": [{"kind": "verb", "verbs": ["open"]}],
            }),
        )
    }

    proptest! {
        /// For ARBITRARY interleavings of delegations across two runs:
        /// (a) each run's delegation count equals the number of facts folded to
        /// it, in log order (the capability chain replays deterministically); and
        /// (b) incremental Materializer application equals fold-from-zero (the
        /// release-blocking `fold(log) == snapshot` / rebuild family — a
        /// divergence is a reducer bug, DR-017 rebuild-stability, I3).
        #[test]
        fn delegations_fold_ordered_and_rebuild_stable(
            seq in proptest::collection::vec((0usize..2, 0u32..1000), 1..40),
        ) {
            let events: Vec<Event> = seq
                .iter()
                .enumerate()
                .map(|(i, &(r, n))| {
                    let parent = format!("p{n:07}");
                    let child = format!("c{i:07}");
                    delegation_ev(RUNS[r], &parent, &child)
                })
                .collect();

            // Independent model: per-run ordered list of child ids.
            let mut model: std::collections::BTreeMap<&str, Vec<String>> = std::collections::BTreeMap::new();
            for (i, &(r, _)) in seq.iter().enumerate() {
                model.entry(RUNS[r]).or_default().push(format!("c{i:07}"));
            }

            let folded = fold(events.iter());
            for (run, children) in &model {
                let run_state = folded.agent_runs.get(*run).expect("run entry exists");
                let got: Vec<String> = run_state.delegations.iter().map(|d| d.child_badge_id.clone()).collect();
                prop_assert_eq!(&got, children, "delegations fold in log order for {}", run);
            }

            let mut live = Materializer::new();
            for event in &events {
                live.apply(event);
            }
            prop_assert_eq!(live.snapshot(), folded, "incremental == fold-from-zero (rebuild, I3)");
        }
    }
}
