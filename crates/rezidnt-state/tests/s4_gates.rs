//! S4 oracle — gate reducers: `gate.entered|passed|failed|inconclusive` fold
//! into `AgentRunState::gates`, `diff.merged` closes the worktree lifecycle.
//! The log is truth (I3); `gate_explain` and the dossier read THIS, so the
//! fold must be exact and deterministic.
//!
//! RED MODE: assert-red. The `Graph`/`GateState` fields exist (oracle
//! scaffold, `#[serde(default)]`), the reducer arms do not — every test here
//! fails on assertion until `apply` learns the S4 subjects.
//!
//! Payload-shape caveat: `gate.passed` v1 and `diff.merged` v1 are ORACLE
//! PROPOSALS pending warden ratification (flagged in the work order); the
//! ratified `gate.entered`/`gate.failed`/`gate.inconclusive` v1 shapes are
//! asserted verbatim.

use rezidnt_state::{Materializer, fold};
use rezidnt_types::{Event, SourceId, Subject};
use serde_json::{Value, json};
use ulid::Ulid;

const RUN: &str = "01S4RED0CERTEST00000000R01";

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

/// Ratified `gate.failed` v1 folds to an interrogable state: verdict, the
/// FAILING verifier, evidence hashes. A pre-spawn vet refusal has no
/// `agent.spawned` — the entry still exists (the log is truth).
#[test]
fn vet_failure_folds_to_interrogable_gate_state_without_a_spawn() {
    let events = [
        ev("gate.entered", json!({"run": RUN, "gate": "vet"})),
        ev(
            "gate.failed",
            json!({
                "run": RUN,
                "gate": "vet",
                "verifier": "bare-mode",
                "evidence": [{"hash": "6e840eefd54f94a9aefe2ca3b0f76b7ea28a8b306300c83567ac6175c0e800ad", "bytes": 47, "mime": "text/plain"}],
                "inputs": {"gate": "vet", "refs": {"spec": "cas:blake3:b13188b3c2a390bb1b4a5ef7863981df304efed8fbfa7379eb063c27222e36e2"}, "params": {}, "timeout_ms": 120000}
            }),
        ),
    ];
    let graph = fold(events.iter());
    let run = graph
        .agent_runs
        .get(RUN)
        .expect("a gate fact creates the run entry — no spawn required (I3)");
    assert_eq!(run.status, "", "no lifecycle fact, no synthesized status");
    let gate = run.gates.get("vet").expect("the vet gate folded");
    assert_eq!(gate.verdict, "fail");
    assert_eq!(
        gate.verifier.as_deref(),
        Some("bare-mode"),
        "the FAILING verifier is named"
    );
    assert_eq!(
        gate.evidence,
        vec!["6e840eefd54f94a9aefe2ca3b0f76b7ea28a8b306300c83567ac6175c0e800ad".to_string()],
        "evidence folds as hashes — refs, never bytes (I2)"
    );
    assert_eq!(gate.reason, None);
}

/// Ratified `gate.inconclusive` v1 (failed shape + reason) keeps its reason
/// verbatim — never coerced toward pass or fail (I6).
#[test]
fn inconclusive_folds_with_reason_never_coerced() {
    let events = [
        ev("gate.entered", json!({"run": RUN, "gate": "pre_merge"})),
        ev(
            "gate.inconclusive",
            json!({
                "run": RUN,
                "gate": "pre_merge",
                "verifier": "tests-pass",
                "reason": "timeout",
                "evidence": [],
                "inputs": {"gate": "pre_merge", "refs": {}, "params": {}, "timeout_ms": 120000}
            }),
        ),
    ];
    let graph = fold(events.iter());
    let gate = &graph.agent_runs[RUN].gates["pre_merge"];
    assert_eq!(gate.verdict, "inconclusive");
    assert_eq!(gate.reason.as_deref(), Some("timeout"));
    assert_eq!(gate.verifier.as_deref(), Some("tests-pass"));
}

/// Proposed `gate.passed` v1: per-verifier records (verifier, cost_ms,
/// evidence, inputs). The fold flattens evidence hashes in order; entered →
/// passed is last-write-wins on the same gate key.
#[test]
fn passed_overwrites_entered_and_flattens_verifier_evidence() {
    let events = [
        ev("gate.entered", json!({"run": RUN, "gate": "pre_merge"})),
        ev(
            "gate.passed",
            json!({
                "run": RUN,
                "gate": "pre_merge",
                "verifiers": [
                    {"verifier": "diff-scope", "cost_ms": 4, "evidence": [{"hash": "aa00000000000000000000000000000000000000000000000000000000000001", "bytes": 3, "mime": "text/plain"}], "inputs": {"gate": "pre_merge", "refs": {}, "params": {}, "timeout_ms": 120000}},
                    {"verifier": "forbidden-path", "cost_ms": 2, "evidence": [{"hash": "aa00000000000000000000000000000000000000000000000000000000000002", "bytes": 3, "mime": "text/plain"}], "inputs": {"gate": "pre_merge", "refs": {}, "params": {}, "timeout_ms": 120000}}
                ]
            }),
        ),
    ];
    let graph = fold(events.iter());
    let gate = &graph.agent_runs[RUN].gates["pre_merge"];
    assert_eq!(gate.verdict, "pass", "last write wins: entered then passed");
    assert_eq!(gate.verifier, None, "no failing verifier on a pass");
    assert_eq!(
        gate.evidence,
        vec![
            "aa00000000000000000000000000000000000000000000000000000000000001".to_string(),
            "aa00000000000000000000000000000000000000000000000000000000000002".to_string(),
        ],
        "verifiers' evidence hashes flattened in order"
    );
}

/// Proposed `diff.merged` v1 closes the worktree lifecycle: status
/// `"merged"`, `last_diff` pinned to the merged diff's hash — inserted even
/// if never allocated (the log is truth, I3).
#[test]
fn diff_merged_marks_the_worktree() {
    let events = [ev(
        "diff.merged",
        json!({
            "run": RUN,
            "worktree": "/tmp/rezidnt-s4/impl",
            "diff": {"hash": "1d50030ca17af09eb6fad0eadfb3492275bfc76635d0965260cde6bc685d785e", "bytes": 23, "mime": "text/plain"}
        }),
    )];
    let graph = fold(events.iter());
    let wt = graph
        .worktrees
        .get("/tmp/rezidnt-s4/impl")
        .expect("diff.merged inserts the entry — the log is truth");
    assert_eq!(wt.status, "merged");
    assert_eq!(
        wt.last_diff.as_deref(),
        Some("1d50030ca17af09eb6fad0eadfb3492275bfc76635d0965260cde6bc685d785e")
    );
}

// --- property: gate folds are deterministic and last-write-wins ------------

mod props {
    use super::*;
    use proptest::prelude::*;
    use std::collections::BTreeMap;

    const RUNS: [&str; 2] = ["01S4PR0PRVN000000000000R01", "01S4PR0PRVN000000000000R02"];
    const GATES: [&str; 3] = ["vet", "pre_merge", "post_run"];
    const VERDICTS: [&str; 4] = ["entered", "pass", "fail", "inconclusive"];

    fn gate_event(run: &str, gate: &str, verdict: &str) -> Event {
        match verdict {
            "entered" => ev("gate.entered", json!({"run": run, "gate": gate})),
            "pass" => ev(
                "gate.passed",
                json!({"run": run, "gate": gate, "verifiers": []}),
            ),
            "fail" => ev(
                "gate.failed",
                json!({"run": run, "gate": gate, "verifier": "diff-scope", "evidence": [], "inputs": {"gate": gate, "refs": {}, "params": {}, "timeout_ms": 120000}}),
            ),
            _ => ev(
                "gate.inconclusive",
                json!({"run": run, "gate": gate, "verifier": "diff-scope", "reason": "timeout", "evidence": [], "inputs": {"gate": gate, "refs": {}, "params": {}, "timeout_ms": 120000}}),
            ),
        }
    }

    proptest! {
        /// For ARBITRARY gate-event interleavings: (a) the folded verdict per
        /// (run, gate) equals an independently computed last-write-wins model,
        /// and (b) incremental Materializer application equals fold-from-zero
        /// (the release-blocking `fold(log) == snapshot` family).
        #[test]
        fn gate_folds_are_last_write_wins_and_incremental_equals_fold(
            seq in proptest::collection::vec((0usize..2, 0usize..3, 0usize..4), 1..40)
        ) {
            let events: Vec<Event> = seq
                .iter()
                .map(|&(r, g, v)| gate_event(RUNS[r], GATES[g], VERDICTS[v]))
                .collect();

            // Independent model: last verdict per (run, gate), in order.
            let mut model: BTreeMap<(&str, &str), &str> = BTreeMap::new();
            for &(r, g, v) in &seq {
                model.insert((RUNS[r], GATES[g]), VERDICTS[v]);
            }

            let folded = fold(events.iter());
            for ((run, gate), verdict) in &model {
                let got = folded
                    .agent_runs
                    .get(*run)
                    .and_then(|r| r.gates.get(*gate))
                    .map(|g| g.verdict.as_str());
                prop_assert_eq!(
                    got, Some(*verdict),
                    "({}, {}) must fold to the LAST recorded verdict", run, gate
                );
            }

            let mut live = Materializer::new();
            for event in &events {
                live.apply(event);
            }
            prop_assert_eq!(live.snapshot(), folded, "incremental == fold-from-zero");
        }
    }
}
