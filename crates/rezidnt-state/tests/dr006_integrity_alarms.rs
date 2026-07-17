//! DR-006 oracle — the `integrity.alarm` folding reducer.
//!
//! DR-006 ratifies: a replay divergence (recorded ≠ replayed) MUST land a
//! durable `integrity.alarm` fact on the log AND fold into queryable state —
//! "no consumer-less subject." This suite pins the FOLD half (the daemon-
//! routed EMIT half is `bins/rezidentd/tests/golden_path.rs`). The log is
//! truth (I3): `rebuild` refolds these alarms identically, forever.
//!
//! RED MODE: assert-red + compile-red. `AgentRunState::integrity_alarms`
//! (the queryable state DR-006 requires) does not exist yet, and `apply` has
//! no `integrity.alarm` arm — every test here fails to compile until the
//! implementer adds the field, then fails on assertion until the reducer
//! learns the subject. A reducer scaffold (field present, arm absent) flips
//! this to pure assert-red, matching the s4_gates.rs precedent.
//!
//! Payload shape (ORACLE PROPOSAL, warden `/subject` pending — flagged in the
//! work order): `integrity.alarm` v1 mirrors `rezidnt_gate::IntegrityAlarm`
//! (crates/rezidnt-gate/src/lib.rs) —
//!   `{run, gate, verifier, recorded: <verdict>, replayed: <verdict>}`
//! all short verdict/name strings (I2: evidence stays CAS-ref'd on the
//! originating gate.failed fact, never re-inlined here).

use rezidnt_state::{Materializer, fold};
use rezidnt_types::{Event, SourceId, Subject};
use serde_json::{Value, json};
use ulid::Ulid;

const RUN: &str = "01S6DR006ALARM0000000000R01";

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

fn alarm(run: &str, gate: &str, verifier: &str, recorded: &str, replayed: &str) -> Event {
    ev(
        "integrity.alarm",
        json!({
            "run": run,
            "gate": gate,
            "verifier": verifier,
            "recorded": recorded,
            "replayed": replayed,
        }),
    )
}

/// CRITERION 5 (reducer fold): an `integrity.alarm` fact folds into
/// queryable per-run state carrying the diverging verifier and both verdicts
/// — the alarm is not a dead-letter subject. A gate fact is NOT required to
/// exist first: the alarm creates the run entry (the log is truth, I3), same
/// as gate facts fold without a spawn.
#[test]
fn integrity_alarm_folds_to_queryable_run_state() {
    let events = [alarm(RUN, "pre_merge", "diff-scope", "fail", "pass")];
    let graph = fold(events.iter());
    let run = graph
        .agent_runs
        .get(RUN)
        .expect("an integrity.alarm creates the run entry — no spawn required (I3)");
    assert_eq!(
        run.integrity_alarms.len(),
        1,
        "the divergence is queryable on the run's dossier"
    );
    let a = &run.integrity_alarms[0];
    assert_eq!(a.gate, "pre_merge");
    assert_eq!(a.verifier, "diff-scope");
    assert_eq!(
        (a.recorded.as_str(), a.replayed.as_str()),
        ("fail", "pass"),
        "both verdicts recorded verbatim — never reconciled (§8)"
    );
}

/// CRITERION 4 (idempotency: DEDUP-BY-(run, gate, verifier)): re-running
/// debrief re-detects the same divergence and re-appends the same
/// `integrity.alarm` fact (at-least-once on the wire — the log records every
/// check). The FOLD collapses duplicates by (run, gate, verifier) so the
/// queryable state shows the divergence ONCE, not a growing pile. This keeps
/// the dossier honest under repeated debriefs while the log stays append-only
/// (I3): duplicate facts on the log, one deduped record in derived state.
#[test]
fn duplicate_alarms_for_the_same_verifier_dedup_in_state() {
    let events = [
        alarm(RUN, "pre_merge", "diff-scope", "fail", "pass"),
        alarm(RUN, "pre_merge", "diff-scope", "fail", "pass"),
        alarm(RUN, "pre_merge", "diff-scope", "fail", "pass"),
    ];
    let graph = fold(events.iter());
    let run = &graph.agent_runs[RUN];
    assert_eq!(
        run.integrity_alarms.len(),
        1,
        "three duplicate divergence facts collapse to ONE queryable record \
         (dedup by (run, gate, verifier)); the log still holds all three"
    );
}

/// Two DIFFERENT verifiers diverging on the same run are DISTINCT alarms —
/// dedup keys on the verifier, not the run. Deterministic order (by (gate,
/// verifier)) so whole-graph equality in the property test is stable.
#[test]
fn distinct_verifiers_are_distinct_alarms_in_deterministic_order() {
    let events = [
        alarm(RUN, "pre_merge", "forbidden-path", "pass", "fail"),
        alarm(RUN, "pre_merge", "diff-scope", "fail", "pass"),
    ];
    let graph = fold(events.iter());
    let run = &graph.agent_runs[RUN];
    assert_eq!(run.integrity_alarms.len(), 2, "two verifiers, two alarms");
    let keys: Vec<(&str, &str)> = run
        .integrity_alarms
        .iter()
        .map(|a| (a.gate.as_str(), a.verifier.as_str()))
        .collect();
    assert_eq!(
        keys,
        vec![("pre_merge", "diff-scope"), ("pre_merge", "forbidden-path")],
        "alarms fold in a deterministic key order (whole-graph equality is stable)"
    );
}

/// A malformed `integrity.alarm` (missing `run`) folds as counters-only —
/// the reducer never chokes, never guesses (I3), matching the gate reducers'
/// payload_run guard.
///
/// ORACLE HONESTY NOTE (GREEN today, by absence): with no `integrity.alarm`
/// arm the catch-all already counts-only and mints no run — this holds
/// trivially. It cannot be made red pre-implementation. Retained as the guard
/// that pins the arm KEEPS the `payload_run` guard once it lands (an
/// implementer who unwraps a missing `run` turns this red). Flagged for the
/// auditor: green-by-absence.
#[test]
fn alarm_without_run_folds_counters_only() {
    let events = [ev(
        "integrity.alarm",
        json!({"gate": "pre_merge", "verifier": "diff-scope", "recorded": "fail", "replayed": "pass"}),
    )];
    let graph = fold(events.iter());
    assert_eq!(graph.events_folded, 1, "the fact is still counted");
    assert!(
        graph.agent_runs.is_empty(),
        "a runless alarm mints no run entry — reducers never guess a key (I3)"
    );
}

// --- property: alarm folds are deterministic, deduped, and rebuild-safe -----

mod props {
    use super::*;
    use proptest::prelude::*;
    use std::collections::BTreeSet;

    const RUNS: [&str; 2] = ["01S6PR0PALARM000000000R01", "01S6PR0PALARM000000000R02"];
    const GATES: [&str; 2] = ["vet", "pre_merge"];
    const VERIFIERS: [&str; 3] = ["diff-scope", "forbidden-path", "tests-pass"];

    proptest! {
        /// For ARBITRARY interleavings of `integrity.alarm` facts (with
        /// duplicates): (a) the folded alarm set per run equals the set of
        /// DISTINCT (gate, verifier) keys seen for that run — dedup is
        /// order-independent and duplicate-proof; and (b) incremental
        /// Materializer application equals fold-from-zero (the release-
        /// blocking `fold(log) == snapshot` / rebuild family). `rebuild` is
        /// exactly fold-from-zero, so (b) is the "rebuild reproduces the
        /// alarms" guarantee DR-006 requires.
        #[test]
        fn alarm_folds_dedup_by_key_and_incremental_equals_fold(
            seq in proptest::collection::vec((0usize..2, 0usize..2, 0usize..3), 1..40)
        ) {
            let events: Vec<Event> = seq
                .iter()
                .map(|&(r, g, v)| alarm(RUNS[r], GATES[g], VERIFIERS[v], "fail", "pass"))
                .collect();

            // Independent model: distinct (gate, verifier) keys per run.
            let mut model: std::collections::BTreeMap<&str, BTreeSet<(&str, &str)>> =
                std::collections::BTreeMap::new();
            for &(r, g, v) in &seq {
                model
                    .entry(RUNS[r])
                    .or_default()
                    .insert((GATES[g], VERIFIERS[v]));
            }

            let folded = fold(events.iter());
            for (run, keys) in &model {
                let got: BTreeSet<(&str, &str)> = folded
                    .agent_runs
                    .get(*run)
                    .expect("run entry exists")
                    .integrity_alarms
                    .iter()
                    .map(|a| (a.gate.as_str(), a.verifier.as_str()))
                    .collect();
                prop_assert_eq!(
                    &got, keys,
                    "run {} folds to the DISTINCT (gate, verifier) alarm set", run
                );
            }

            let mut live = Materializer::new();
            for event in &events {
                live.apply(event);
            }
            prop_assert_eq!(live.snapshot(), folded, "incremental == fold-from-zero (rebuild)");
        }
    }
}
