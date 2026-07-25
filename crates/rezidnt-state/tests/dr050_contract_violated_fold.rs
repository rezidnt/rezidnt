//! DR-050-set oracle — the `run.contract.violated` v1 folding reducer
//! (`spec/ontology.md` "### DR-050 set", ratified 2026-07-25; the
//! trials-slice-b entry criterion (b) SURFACING arm).
//!
//! The ontology's reducer obligation, restated as falsifiable assertions:
//! `AgentRunState` gains `#[serde(default)] pub contract_violated:
//! Option<ContractViolationRecord>` (`{harness: String, detail: String}`),
//! folded by a NEW `"run.contract.violated"` match arm that MINTS the run
//! entry if absent (the `integrity.alarm` precedent) and DEDUPS per run —
//! FIRST FACT WINS — so a malformed log cannot fold twice. `harness` and
//! `detail` fold VERBATIM from the payload: the emitter lifts them
//! STRUCTURALLY from `AdapterError::ContractViolated { harness, detail }`,
//! and the fold must never see (or preserve) the prose `Display` wrapping.
//!
//! This suite pins the FOLD half — the one arm of the work order that is
//! fully behaviorally testable today, because a reducer is a pure function
//! over events (I3). The daemon EMIT half has no reachable driver on this
//! tree (`drive_run` hardcodes `ClaudeCodeAdapter`; the variant's sole
//! construction site is `CodexAdapter::map_run_completed`), so its judge is
//! the disclosed structural backstop in
//! `bins/rezidentd/tests/dr050_contract_violated_surfacing.rs`.
//!
//! ## STATE (truth pass, session 32)
//!
//! COMPILE-RED when written; the type, the field, and the reducer arm have
//! since landed, and the original suite is a GREEN regression pin
//! (`violation_without_run_folds_counters_only`, GREEN-BY-ABSENCE at
//! introduction, is now a real pin on the arm's `payload_run` guard). The
//! session-32 remediation adds the MALFORMED-FACT ruling, and the arm now
//! satisfies it:
//!
//! - `violation_missing_required_fields_never_occupies_the_slot` and
//!   `violation_missing_required_fields_folds_counters_only` are GREEN pins.
//!   They were written ASSERT-RED against an arm that `unwrap_or_default()`ed
//!   BOTH payload fields inside `get_or_insert_with`, so a fact carrying `run`
//!   but no `detail` minted an EMPTY record that permanently won the
//!   first-fact-wins slot against a later well-formed fact — while the
//!   ontology declares `detail` REQUIRED. The arm now guards both with
//!   `if let (Some(harness), Some(detail))`, so the ruling these tests encode
//!   holds: a fact missing `harness` or `detail` folds as COUNTERS ONLY and
//!   never occupies the slot — the `integrity.alarm` precedent in the same
//!   match (required discriminator guarded by `if let Some(..)`;
//!   `unwrap_or_default` only for fields that do not discriminate).

use rezidnt_state::{ContractViolationRecord, Materializer, fold};
use rezidnt_types::{Event, SourceId, Subject};
use serde_json::{Value, json};
use ulid::Ulid;

const RUN: &str = "01DR050CV00000000000000R01";

/// The structural detail text, exactly as the variant's `detail` field would
/// carry it — NO `Display` wrapping ("{harness}: recorded stream contract
/// violated — …") anywhere in it.
const DETAIL: &str = "a second turn-terminal line arrived on one `codex exec --json` stream \
                      (terminal turn 2), falsifying the single-shot premise the recorded \
                      contract rests on";

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

fn violation(run: &str, harness: &str, detail: &str) -> Event {
    ev(
        "run.contract.violated",
        json!({"run": run, "harness": harness, "detail": detail}),
    )
}

/// The core fold: a `run.contract.violated` fact folds into queryable per-run
/// state carrying `harness` and `detail` VERBATIM — the subject is not a
/// dead-letter (DR-006 "no consumer-less subject"; the fold is named consumer
/// (1) in the ontology block).
#[test]
fn contract_violated_folds_to_queryable_run_state() {
    let events = [
        ev("agent.spawned", json!({"run": RUN})),
        ev(
            "agent.completed",
            json!({"run": RUN, "status": "completed", "cost": {"total_usd": 0.02}}),
        ),
        violation(RUN, "codex", DETAIL),
    ];
    let graph = fold(events.iter());
    let run = graph.agent_runs.get(RUN).expect("run entry exists");
    let record = run
        .contract_violated
        .as_ref()
        .expect("the withdrawal of trust in the run's recorded facts is queryable on the dossier");
    assert_eq!(record.harness, "codex", "harness folded verbatim");
    assert_eq!(
        record.detail, DETAIL,
        "detail folded verbatim from the payload — the payload carries the variant's \
         structural `detail` field, and the fold must not lose or rewrap a byte of it"
    );
    assert_eq!(
        run.status, "completed",
        "the published completion is NEVER retracted or rewritten (I3) — the violation \
         folds ALONGSIDE the completion's state, not over it"
    );
}

/// Mint-if-absent (the `integrity.alarm` precedent, named by the ontology's
/// reducer obligation): a violation fact on a run the log never spawned still
/// creates the run entry — the log is truth (I3), reducers never require a
/// prior fact to exist.
#[test]
fn violation_mints_the_run_entry_if_absent() {
    let events = [violation(RUN, "codex", DETAIL)];
    let graph = fold(events.iter());
    let run = graph
        .agent_runs
        .get(RUN)
        .expect("a run.contract.violated fact creates the run entry — no spawn required (I3)");
    assert_eq!(
        run.contract_violated
            .as_ref()
            .expect("the record folded")
            .harness,
        "codex"
    );
    assert_eq!(run.status, "", "a minted entry carries the default status");
}

/// Cardinality (ontology, verbatim): "the reducer dedups per run regardless —
/// FIRST FACT WINS — so a malformed log cannot fold twice." A duplicate fact
/// with a DIFFERENT detail must not overwrite the first record; the raw log
/// still holds both facts.
#[test]
fn duplicate_violations_dedup_first_fact_wins() {
    let events = [
        violation(RUN, "codex", DETAIL),
        violation(RUN, "codex", "a LATER duplicate detail that must NOT win"),
        violation(
            RUN,
            "some-future-harness",
            "not even a different harness reopens the slot",
        ),
    ];
    let graph = fold(events.iter());
    let record = graph.agent_runs[RUN]
        .contract_violated
        .as_ref()
        .expect("the first fact folded");
    assert_eq!(
        (record.harness.as_str(), record.detail.as_str()),
        ("codex", DETAIL),
        "FIRST FACT WINS: duplicate run.contract.violated facts on one run collapse to \
         the first record — a malformed log cannot fold twice, and a later fact never \
         rewrites the recorded refusal (I3, append-only)"
    );
}

/// A malformed fact (missing `run`) folds as counters-only — the reducer never
/// chokes, never guesses a key (I3), matching the `payload_run` guard every
/// run-keyed arm uses.
///
/// ORACLE HONESTY NOTE (truth pass, session 32): GREEN-BY-ABSENCE when
/// written (no arm existed, so the catch-all counted-only trivially; flagged
/// for the auditor then, per the dr006 precedent). The arm has since landed
/// WITH the `payload_run` guard, so this is now a real pin: an implementer
/// who unwraps a missing `run` turns it red.
#[test]
fn violation_without_run_folds_counters_only() {
    let events = [ev(
        "run.contract.violated",
        json!({"harness": "codex", "detail": DETAIL}),
    )];
    let graph = fold(events.iter());
    assert_eq!(graph.events_folded, 1, "the fact is still counted");
    assert!(
        graph.agent_runs.is_empty(),
        "a runless violation mints no run entry — reducers never guess a key (I3)"
    );
}

/// I3 rebuild stability: `#[serde(default)]` keeps every existing golden
/// fixture parsing and folding BIT-IDENTICAL — an `AgentRunState` JSON written
/// before DR-050 (no `contract_violated` key) must deserialize with the field
/// `None`, and a record must survive a serde round-trip unchanged (snapshots
/// are re-loadable state, not lossy views).
#[test]
fn serde_default_keeps_pre_dr050_state_parsing_and_round_trips() {
    let pre: rezidnt_state::AgentRunState = serde_json::from_value(json!({"status": "completed"}))
        .expect("a pre-DR-050 AgentRunState JSON (no contract_violated key) must parse");
    assert!(
        pre.contract_violated.is_none(),
        "absent in the JSON folds to None — never synthesized (DR-012 declared-vs-absent)"
    );

    let record = ContractViolationRecord {
        harness: "codex".to_string(),
        detail: DETAIL.to_string(),
    };
    let round: ContractViolationRecord =
        serde_json::from_value(serde_json::to_value(&record).expect("serialize"))
            .expect("deserialize");
    assert_eq!(round, record, "the record round-trips bit-identical");
}

/// THE MALFORMED-FACT RULING (session-32 auditor finding): the ontology
/// declares `harness` and `detail` REQUIRED, and this fold enforces
/// first-fact-wins — so a fact missing either field must NOT occupy the
/// slot. If it folded a defaulted record instead, an EMPTY record would
/// permanently win the run's slot and a later WELL-FORMED fact could never
/// fold: garbage beating truth for the life of the log. The precedent is
/// `integrity.alarm` in the same match: its required discriminator is
/// guarded (`if let Some(verifier)`), and only non-discriminating fields
/// `unwrap_or_default()`.
///
/// GREEN pin. Written ASSERT-RED against an arm that `unwrap_or_default()`ed
/// both fields inside `get_or_insert_with` — the malformed fact minted the
/// empty record and won. The arm now guards both fields, so this holds.
#[test]
fn violation_missing_required_fields_never_occupies_the_slot() {
    for malformed in [
        json!({"run": RUN, "harness": "codex"}), // no `detail`
        json!({"run": RUN, "detail": DETAIL}),   // no `harness`
    ] {
        let events = [
            ev("run.contract.violated", malformed.clone()),
            violation(RUN, "codex", DETAIL),
        ];
        let graph = fold(events.iter());
        let record = graph.agent_runs[RUN]
            .contract_violated
            .as_ref()
            .expect("the well-formed fact folded");
        assert_eq!(
            (record.harness.as_str(), record.detail.as_str()),
            ("codex", DETAIL),
            "a fact missing a REQUIRED field ({malformed}) must not occupy the \
             first-fact-wins slot — it folds counters-only, so the LATER well-formed \
             fact is the first VALID fact and wins. A defaulted empty record winning \
             here is garbage beating truth permanently (the integrity.alarm \
             precedent: guard required fields, unwrap_or_default only what does not \
             discriminate)"
        );
    }
}

/// The counters-only half of the ruling, in isolation: a malformed fact with
/// no well-formed successor mints NOTHING — no run entry, no record — exactly
/// as a runless fact does, and exactly as `integrity.alarm` treats a missing
/// `verifier` (its `entry()` sits inside the discriminator guard). The fact
/// is still counted; the raw log still holds it (I3).
///
/// GREEN pin. Written ASSERT-RED against an arm that minted the entry and a
/// defaulted record for any fact carrying `run`; the guard now sits above the
/// `entry()` call, so a malformed fact mints nothing.
#[test]
fn violation_missing_required_fields_folds_counters_only() {
    let events = [
        ev(
            "run.contract.violated",
            json!({"run": RUN, "harness": "codex"}),
        ),
        ev(
            "run.contract.violated",
            json!({"run": RUN, "detail": DETAIL}),
        ),
    ];
    let graph = fold(events.iter());
    assert_eq!(graph.events_folded, 2, "malformed facts are still counted");
    assert!(
        graph.agent_runs.is_empty(),
        "a run.contract.violated fact missing a REQUIRED field (`harness` or \
         `detail`) folds as counters only — it must not mint the run entry or a \
         defaulted record (the integrity.alarm precedent: a missing discriminator \
         mints nothing). Got run entries for {:?}",
        graph.agent_runs.keys().collect::<Vec<_>>()
    );
}

// --- property: violation folds are deterministic, first-wins, rebuild-safe --

mod props {
    use super::*;
    use proptest::prelude::*;

    const RUNS: [&str; 2] = ["01DR050PR0P0000000000000R1", "01DR050PR0P0000000000000R2"];
    const HARNESSES: [&str; 2] = ["codex", "claude-code"];
    const DETAILS: [&str; 3] = [
        "second turn-terminal line (terminal turn 2)",
        "second turn-terminal line (terminal turn 3)",
        "recorded item ordering falsified",
    ];

    proptest! {
        /// For ARBITRARY sequences of `run.contract.violated` facts (with
        /// duplicates, across runs): (a) each run's folded record equals the
        /// FIRST fact seen for that run in log order — first-wins is exact,
        /// not merely "some fact wins"; and (b) incremental Materializer
        /// application equals fold-from-zero (the release-blocking
        /// `fold(log) == snapshot` / rebuild family — `rebuild` IS
        /// fold-from-zero, so (b) is the rebuild-stability guarantee).
        #[test]
        fn first_fact_wins_per_run_and_incremental_equals_fold(
            seq in proptest::collection::vec((0usize..2, 0usize..2, 0usize..3), 1..40)
        ) {
            let events: Vec<Event> = seq
                .iter()
                .map(|&(r, h, d)| violation(RUNS[r], HARNESSES[h], DETAILS[d]))
                .collect();

            // Independent model: the first (harness, detail) per run, in
            // log order.
            let mut model: std::collections::BTreeMap<&str, (&str, &str)> =
                std::collections::BTreeMap::new();
            for &(r, h, d) in &seq {
                model.entry(RUNS[r]).or_insert((HARNESSES[h], DETAILS[d]));
            }

            let folded = fold(events.iter());
            for (run, (harness, detail)) in &model {
                let got = folded
                    .agent_runs
                    .get(*run)
                    .expect("run entry minted")
                    .contract_violated
                    .as_ref()
                    .expect("record folded");
                prop_assert_eq!(
                    (got.harness.as_str(), got.detail.as_str()),
                    (*harness, *detail),
                    "run {} folds the FIRST fact's record, whatever arrived later",
                    run
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
