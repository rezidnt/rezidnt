//! DR-055 ORACLE — the `trial.opened` fold and the requested-vs-spawned delta
//! projection (`spec/ontology.md` "### trial" section + "### DR-055 set",
//! ratified 2026-07-26; trials slice B, oracle-first work orders (1) and (2)
//! named on the DR-055 set's reducer ruling).
//!
//! The warden's reducer ruling, restated as falsifiable assertions:
//!
//! - `Graph` gains `#[serde(default)] pub trials: BTreeMap<String, TrialState>`
//!   keyed by the payload `trial` id — a NEW ENTITY CLASS with its own keyed
//!   map (the `workspaces`/`worktrees`/`agent_runs` precedent), because a trial
//!   PRECEDES its N sample runs and is 1:1 with none of them.
//! - `TrialState` carries the fact VERBATIM: `{idempotency_key: String,
//!   variants: Vec<TrialVariant {agent, harness, model: Option<String>}>,
//!   samples: u64}` — and NOTHING derived: membership and the delta are
//!   PROJECTIONS, never stored (the `orchestration_graph` `fan_out =
//!   subs.len()` precedent).
//! - Fold semantics: entry minted on first fact, FIRST-FACT-WINS per trial id
//!   (the `run.contract.violated` dedup precedent, named by the ruling); a
//!   keyless fact folds counters-only, never panics (house rule).
//! - `AgentRunState` gains `#[serde(default)] pub trial: Option<String>` and
//!   `#[serde(default)] pub model: Option<String>`, folded inside the EXISTING
//!   `"agent.spawned"` arm (the `lead_run` fold pattern — no new match arm for
//!   a field), verbatim-when-present, absent-stays-`None` (DR-012).
//! - The requested-vs-spawned DELTA is a pure projection: requested =
//!   `variants × samples` off `trials[id]`, spawned = the `agent_runs` group
//!   with `trial == Some(id)`, per-variant counts off each sample's own
//!   (`agent`, `harness`, `model?`) triple. This is the necessity argument for
//!   the whole subject (DR-055 §Context 3): a 3×3 matrix that spawned 7 must
//!   NOT read as complete — the two missing samples are visible, not silently
//!   absent.
//!
//! ## API surface this board PINS (implementer builds to exactly this)
//!
//! In `rezidnt-state`: the `Graph.trials` field, `TrialState`, `TrialVariant`
//! (shapes above, all serde round-trippable — snapshots are re-loadable state),
//! and the projection
//!
//! ```ignore
//! pub struct TrialVariantDelta {
//!     pub agent: String,
//!     pub harness: String,
//!     pub model: Option<String>,
//!     pub requested: u64, // = trials[id].samples
//!     pub spawned: u64,   // samples in the group matching this triple
//! }
//! pub struct TrialDelta {
//!     pub requested: u64, // variants.len() * samples — DERIVED, never stored
//!     pub spawned: u64,   // size of the agent_runs group with trial == Some(id)
//!     pub per_variant: Vec<TrialVariantDelta>, // in the fact's VERBATIM order
//! }
//! /// `None` for a trial id no `trial.opened` fact minted — the requested end
//! /// of the delta does not exist, and the projection never invents it.
//! pub fn trial_delta(graph: &Graph, trial: &str) -> Option<TrialDelta>
//! ```
//!
//! DR-055 fixes the projection's SEMANTICS (group `agent_runs` by
//! `agent.spawned.trial?`, requested off `trials[id]`, never stored); the
//! symbol names above are this oracle's pin, in the `orchestration_graph`
//! house shape (a pure fn over `&Graph` in `rezidnt-state`), so the judge has
//! a callable. If the implementer needs a different shape, that is a
//! conversation with this board, not a silent rename.
//!
//! ## RED MODE (against the tree at cut time — session 33, post-`bcd0db9`)
//!
//! COMPILE-RED: `Graph` has no `trials` field, `TrialState` / `TrialVariant` /
//! `TrialDelta` / `trial_delta` do not exist, and `AgentRunState` has no
//! `trial` / `model` fields (all verified against
//! `crates/rezidnt-state/src/lib.rs` this session). Every test here is red for
//! that one right reason: the entity class this suite judges is unbuilt. The
//! moment the implementer lands the ruling, this file compiles and each test
//! judges its clause.

use rezidnt_state::{Graph, Materializer, TrialState, TrialVariant, fold, trial_delta};
use rezidnt_types::{Event, SourceId, Subject};
use serde_json::{Value, json};
use std::path::PathBuf;
use ulid::Ulid;

/// Payload trial ids (opaque strings to the reducer — folded verbatim, never
/// parsed; ULID-shaped because that is what the daemon will mint).
const TRIAL: &str = "01DR055TR1A000000000000001";
const OTHER_TRIAL: &str = "01DR055TR1B000000000000002";

const KEY: &str = "dr055-trial-key-1";

fn ev(subject: &str, payload: Value) -> Event {
    Event::new(
        SourceId::new("rezidentd"),
        None,
        Subject::new(subject),
        Ulid::new(),
        None,
        1,
        payload,
    )
    .expect("test event under 32KiB")
}

/// A well-formed `trial.opened` v1 payload: 3 variants x 3 samples, the DR-055
/// motivating matrix. Variant 0 declares no model (absent = the harness's own
/// default, never synthesized — DR-012).
fn opened_3x3(trial: &str, key: &str) -> Event {
    ev(
        "trial.opened",
        json!({
            "trial": trial,
            "idempotency_key": key,
            "variants": [
                {"agent": "impl", "harness": "claude-code"},
                {"agent": "impl", "harness": "claude-code", "model": "model-alpha"},
                {"agent": "impl", "harness": "claude-code", "model": "model-beta"},
            ],
            "samples": 3,
        }),
    )
}

/// One sample's `agent.spawned` fact: carries the daemon-derived per-sample
/// key on `idempotency_key` and its trial membership on `trial` (never the
/// envelope — DR-049 ruled the correlation join unsound).
fn sample_spawned(run: &str, trial: &str, model: Option<&str>, sample_key: &str) -> Event {
    let mut payload = json!({
        "run": run,
        "agent": "impl",
        "harness": "claude-code",
        "idempotency_key": sample_key,
        "trial": trial,
    });
    if let Some(model) = model {
        payload["model"] = json!(model);
    }
    ev("agent.spawned", payload)
}

// --- (a) the fold: mint, verbatim carry, first-fact-wins, counters-only -----

/// The core fold: a `trial.opened` fact folds into its OWN keyed map, carrying
/// the fact verbatim — key, variants (ordered as requested), samples. The
/// subject is not a dead-letter (DR-006 no-consumer-less rule; this fold is
/// named consumer (1) on the DR-055 set's reducer ruling).
#[test]
fn trial_opened_folds_to_its_own_keyed_map() {
    let graph = fold([opened_3x3(TRIAL, KEY)].iter());
    let trial = graph.trials.get(TRIAL).expect("trial entry minted");
    assert_eq!(
        trial.idempotency_key, KEY,
        "trial-level key folded verbatim"
    );
    assert_eq!(trial.samples, 3, "sample count folded verbatim");
    assert_eq!(
        trial.variants,
        vec![
            TrialVariant {
                agent: "impl".to_string(),
                harness: "claude-code".to_string(),
                model: None,
            },
            TrialVariant {
                agent: "impl".to_string(),
                harness: "claude-code".to_string(),
                model: Some("model-alpha".to_string()),
            },
            TrialVariant {
                agent: "impl".to_string(),
                harness: "claude-code".to_string(),
                model: Some("model-beta".to_string()),
            },
        ],
        "variants fold VERBATIM, in the fact's requested order (the order is \
         semantic: per-sample keys derive from (variant, sample-index)); a \
         declared-no-model variant folds model: None, never a synthesized \
         default (DR-012)"
    );
}

/// The warden's entity ruling, negatively: a trial is a NEW ENTITY CLASS, not
/// a run axis. Its fact mints NOTHING in `agent_runs` (there is no run to hang
/// it on — the fact PRECEDES every run it will ever relate to) and nothing in
/// `worktrees`.
#[test]
fn a_trial_is_a_new_entity_not_a_run_axis() {
    let graph = fold([opened_3x3(TRIAL, KEY)].iter());
    assert!(
        graph.agent_runs.is_empty(),
        "trial.opened folds onto Graph.trials, never onto agent_runs — the \
         DR-055 set ruled it CANNOT hang off AgentRunState (keyed per RUN, and \
         this fact precedes every run)"
    );
    assert!(graph.worktrees.is_empty(), "and never onto worktrees");
    assert_eq!(graph.events_folded, 1);
}

/// Cardinality (DR-055 set, verbatim): "entry minted on first fact,
/// FIRST-FACT-WINS per trial id (the `run.contract.violated` dedup precedent
/// — the emitter is at-most-once by idempotent construction and the reducer
/// dedups regardless, so a malformed log cannot refold)."
#[test]
fn duplicate_trial_facts_dedup_first_fact_wins() {
    let events = [
        opened_3x3(TRIAL, KEY),
        ev(
            "trial.opened",
            json!({
                "trial": TRIAL,
                "idempotency_key": "a-LATER-key-that-must-NOT-win",
                "variants": [{"agent": "other", "harness": "claude-code"}],
                "samples": 1,
            }),
        ),
    ];
    let graph = fold(events.iter());
    let trial = graph.trials.get(TRIAL).expect("first fact folded");
    assert_eq!(
        (
            trial.idempotency_key.as_str(),
            trial.samples,
            trial.variants.len()
        ),
        (KEY, 3, 3),
        "FIRST FACT WINS: a duplicate trial.opened on one trial id never \
         rewrites the recorded intent — the raw log still holds both facts \
         (I3, append-only), the derived entry holds the first"
    );
    assert_eq!(graph.trials.len(), 1, "one trial id, one entry");
}

/// House rule, named by the ruling: "a keyless fact folds counters-only,
/// never panics." A payload with no `trial` key is counted and mints nothing —
/// reducers never choke, never guess a key (I3).
#[test]
fn keyless_trial_fact_folds_counters_only() {
    let events = [ev(
        "trial.opened",
        json!({"idempotency_key": KEY, "variants": [], "samples": 1}),
    )];
    let graph = fold(events.iter());
    assert_eq!(graph.events_folded, 1, "the fact is still counted");
    assert!(
        graph.trials.is_empty(),
        "a keyless trial.opened mints no entry — reducers never guess a key (I3)"
    );
}

/// THE MALFORMED-FACT RULING, carried over from the precedent DR-055 names:
/// the ontology declares `idempotency_key`, `variants`, and `samples` REQUIRED
/// on `trial.opened` v1, and this fold enforces first-fact-wins — so a fact
/// missing a required field must NOT occupy the slot, or a defaulted empty
/// record would permanently beat a later well-formed fact (garbage beating
/// truth for the life of the log). Exactly the
/// `dr050_contract_violated_fold.rs` session-32 ruling
/// (`violation_missing_required_fields_never_occupies_the_slot`), on the
/// precedent the DR-055 set cites for its dedup semantics.
#[test]
fn trial_fact_missing_required_fields_never_occupies_the_slot() {
    for malformed in [
        json!({"trial": TRIAL, "variants": [{"agent": "impl", "harness": "claude-code"}], "samples": 3}), // no idempotency_key
        json!({"trial": TRIAL, "idempotency_key": KEY, "samples": 3}), // no variants
        json!({"trial": TRIAL, "idempotency_key": KEY, "variants": [{"agent": "impl", "harness": "claude-code"}]}), // no samples
    ] {
        let events = [
            ev("trial.opened", malformed.clone()),
            opened_3x3(TRIAL, KEY),
        ];
        let graph = fold(events.iter());
        let trial = graph
            .trials
            .get(TRIAL)
            .expect("the well-formed fact folded");
        assert_eq!(
            (
                trial.idempotency_key.as_str(),
                trial.samples,
                trial.variants.len()
            ),
            (KEY, 3, 3),
            "a trial.opened missing a REQUIRED field ({malformed}) must not \
             occupy the first-fact-wins slot — it folds counters-only, so the \
             LATER well-formed fact is the first VALID fact and wins (the \
             run.contract.violated malformed-fact ruling, which is the dedup \
             precedent DR-055's reducer ruling names)"
        );
    }
}

/// The counters-only half of that ruling in isolation: a malformed fact with
/// no well-formed successor mints NOTHING. Still counted; the raw log still
/// holds it (I3).
#[test]
fn trial_fact_missing_required_fields_folds_counters_only() {
    let events = [ev(
        "trial.opened",
        json!({"trial": TRIAL, "idempotency_key": KEY}),
    )];
    let graph = fold(events.iter());
    assert_eq!(graph.events_folded, 1, "still counted");
    assert!(
        graph.trials.is_empty(),
        "a trial.opened missing REQUIRED fields mints no entry — got {:?}",
        graph.trials.keys().collect::<Vec<_>>()
    );
}

/// "Membership and the delta are PROJECTIONS, never stored" — pinned on the
/// SHAPE: `TrialState` serializes to exactly the fact's three fields, and a
/// variant to at most its three axes. A stored membership list, sample count,
/// or per-variant tally would surface here as a new key — the
/// stored-derivation smell the ruling refuses.
#[test]
fn trial_state_stores_the_fact_verbatim_and_nothing_derived() {
    let graph = fold([opened_3x3(TRIAL, KEY)].iter());
    let entry = serde_json::to_value(graph.trials.get(TRIAL).expect("entry"))
        .expect("TrialState serializes");
    let mut keys: Vec<&str> = entry
        .as_object()
        .expect("TrialState is an object")
        .keys()
        .map(String::as_str)
        .collect();
    keys.sort_unstable();
    keys.retain(|k| !matches!(*k, "idempotency_key" | "variants" | "samples"));
    assert!(
        keys.is_empty(),
        "TrialState carries the fact VERBATIM — {{idempotency_key, variants, \
         samples}} and nothing else; membership and counts are projections, \
         never stored (DR-055 set reducer ruling). Unexpected keys: {keys:?}"
    );
    for variant in entry["variants"].as_array().expect("variants array") {
        let mut vkeys: Vec<&str> = variant
            .as_object()
            .expect("variant object")
            .keys()
            .map(String::as_str)
            .collect();
        vkeys.retain(|k| !matches!(*k, "agent" | "harness" | "model"));
        assert!(
            vkeys.is_empty(),
            "a TrialVariant carries only the three DR-048 axes — unexpected keys: {vkeys:?}"
        );
    }
}

// --- (a) rebuild stability: #[serde(default)], every fixture unedited -------

/// I3 rebuild stability: a pre-DR-055 `Graph` snapshot (no `trials` key) and a
/// pre-DR-055 `AgentRunState` (no `trial`/`model` keys) must parse with the
/// new fields defaulted — absent folds to empty/None, never synthesized — and
/// a graph carrying a trial must survive a serde round-trip unchanged
/// (snapshots are re-loadable state, not lossy views).
#[test]
fn serde_default_keeps_pre_dr055_state_parsing_and_round_trips() {
    let pre: Graph = serde_json::from_value(json!({
        "events_folded": 0,
        "last_event": null,
        "counts_by_subject": {},
        "workspaces": {},
    }))
    .expect("a pre-DR-055 Graph JSON (no trials key) must parse");
    assert!(
        pre.trials.is_empty(),
        "absent `trials` defaults to the empty map — #[serde(default)]"
    );

    let pre_run: rezidnt_state::AgentRunState =
        serde_json::from_value(json!({"status": "spawning"}))
            .expect("a pre-DR-055 AgentRunState JSON must parse");
    assert!(pre_run.trial.is_none(), "absent trial parses to None");
    assert!(pre_run.model.is_none(), "absent model parses to None");

    let graph = fold(
        [
            opened_3x3(TRIAL, KEY),
            sample_spawned(
                "01DR055SAMPRVN000000000001",
                TRIAL,
                Some("model-alpha"),
                "dk-1",
            ),
        ]
        .iter(),
    );
    let round: Graph =
        serde_json::from_value(serde_json::to_value(&graph).expect("serialize graph"))
            .expect("deserialize graph");
    assert_eq!(
        round, graph,
        "a graph carrying a trial round-trips bit-identical"
    );
}

/// EVERY existing golden fixture, unedited: each committed
/// `spec/fixtures/*.expected.json` must still parse into the widened `Graph`,
/// and every pre-DR-055 expected graph must carry an EMPTY `trials` map — the
/// `#[serde(default)]` guarantee over the real committed corpus, not a
/// synthetic sample. (Bit-identical FOLD equality over the same corpus is
/// `fixture_replay.rs`'s standing job; this leg pins the parse half that a
/// missing `#[serde(default)]` would break first.) This test EDITS NOTHING.
#[test]
fn every_committed_expected_graph_parses_and_pre_dr055_ones_fold_no_trials() {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../spec/fixtures");
    let mut checked = 0usize;
    for entry in std::fs::read_dir(&dir).expect("fixtures dir exists") {
        let path = entry.expect("dir entry").path();
        let name = path
            .file_name()
            .expect("name")
            .to_string_lossy()
            .into_owned();
        if !name.ends_with(".expected.json") {
            continue;
        }
        let graph: Graph =
            serde_json::from_str(&std::fs::read_to_string(&path).expect("read expected graph"))
                .unwrap_or_else(|e| {
                    panic!(
                        "{name}: every committed expected graph must parse into the \
                 widened Graph (#[serde(default)] on trials — I3 rebuild \
                 stability, no fixture edited): {e}"
                    )
                });
        if !name.starts_with("dr055") {
            assert!(
                graph.trials.is_empty(),
                "{name}: a pre-DR-055 expected graph parses with trials EMPTY — \
                 absent is never synthesized"
            );
        }
        checked += 1;
    }
    assert!(
        checked >= 10,
        "the golden corpus went missing ({checked} files)"
    );
}

// --- (d)-fold: trial? / model? onto AgentRunState ---------------------------

/// The two new `agent.spawned` optionals fold inside the EXISTING arm, onto
/// `AgentRunState.trial` / `.model`, VERBATIM when present — the
/// `pep`/`role`/`lead_run` fold pattern exactly. Absent stays `None`, never
/// synthesized (DR-012): a non-trial run is not a sample of anything, and an
/// undeclared model is the harness's own default, never named.
#[test]
fn agent_spawned_folds_trial_and_model_verbatim_and_absent_stays_none() {
    let events = [
        sample_spawned(
            "01DR055SAMPRVN000000000001",
            TRIAL,
            Some("model-alpha"),
            "dk-1",
        ),
        // model WITHOUT trial: a declared model is a spawn posture like role?,
        // not a trial-only property (ontology model? bullet).
        ev(
            "agent.spawned",
            json!({"run": "01DR055PLA1NRVN00000000002", "agent": "impl",
                   "harness": "claude-code", "model": "model-solo"}),
        ),
        // neither: the pre-DR-055 payload, byte-identical semantics.
        ev(
            "agent.spawned",
            json!({"run": "01DR055PLA1NRVN00000000003", "agent": "impl",
                   "harness": "claude-code"}),
        ),
    ];
    let graph = fold(events.iter());

    let sample = &graph.agent_runs["01DR055SAMPRVN000000000001"];
    assert_eq!(
        sample.trial.as_deref(),
        Some(TRIAL),
        "trial folded verbatim"
    );
    assert_eq!(
        sample.model.as_deref(),
        Some("model-alpha"),
        "model folded verbatim"
    );

    let solo = &graph.agent_runs["01DR055PLA1NRVN00000000002"];
    assert_eq!(
        solo.model.as_deref(),
        Some("model-solo"),
        "model rides any spawn"
    );
    assert!(solo.trial.is_none(), "a non-trial run has trial: None");

    let plain = &graph.agent_runs["01DR055PLA1NRVN00000000003"];
    assert!(
        plain.trial.is_none() && plain.model.is_none(),
        "absent stays None (DR-012)"
    );
}

// --- (b) the requested-vs-spawned delta projection ---------------------------

/// THE NECESSITY JUDGE (DR-055 §Context 3, the case that motivated the mint):
/// a 3x3 matrix that spawned 7 must NOT read as a complete trial. A refused
/// sample mints NO run and is reported on the tool response, never the log —
/// so the two missing samples are derivable ONLY as requested (this fact)
/// minus spawned (the `agent.spawned.trial?` group). Foreign runs — a non-trial
/// spawn and another trial's sample on the same log — never leak into the
/// group.
#[test]
fn a_three_by_three_that_spawned_seven_reads_incomplete() {
    let mut events = vec![opened_3x3(TRIAL, KEY)];
    // Variant 0 (no model): 3 of 3 samples spawned.
    for s in 0..3 {
        events.push(sample_spawned(
            &format!("01DR055SAMPV0S{s}0000000000"),
            TRIAL,
            None,
            &format!("dk-v0-s{s}"),
        ));
    }
    // Variant 1 (model-alpha): 3 of 3.
    for s in 0..3 {
        events.push(sample_spawned(
            &format!("01DR055SAMPV1S{s}0000000000"),
            TRIAL,
            Some("model-alpha"),
            &format!("dk-v1-s{s}"),
        ));
    }
    // Variant 2 (model-beta): 1 of 3 — two samples were refused (e.g.
    // worktree-conflicted) and exist on NO log fact.
    events.push(sample_spawned(
        "01DR055SAMPV2S00000000000X",
        TRIAL,
        Some("model-beta"),
        "dk-v2-s0",
    ));
    // Noise the grouping must exclude: an ordinary spawn (no trial) and a
    // sample of a DIFFERENT trial.
    events.push(ev(
        "agent.spawned",
        json!({"run": "01DR055N01SERVN00000000001", "agent": "impl", "harness": "claude-code"}),
    ));
    events.push(ev(
        "trial.opened",
        json!({"trial": OTHER_TRIAL, "idempotency_key": "other-key",
               "variants": [{"agent": "impl", "harness": "claude-code"}], "samples": 1}),
    ));
    events.push(sample_spawned(
        "01DR055N01SERVN00000000002",
        OTHER_TRIAL,
        None,
        "other-dk-0",
    ));

    let graph = fold(events.iter());
    let delta = trial_delta(&graph, TRIAL).expect("the trial exists, so its delta exists");

    assert_eq!(
        delta.requested, 9,
        "requested = V x N = 3 x 3, DERIVED off trials[id]"
    );
    assert_eq!(
        delta.spawned, 7,
        "spawned = the agent.spawned.trial? group — 7, not 9: the two refused \
         samples are VISIBLE as a deficit, not silently absent (DR-055 \
         §Context 3, the whole necessity argument)"
    );
    assert!(
        delta.spawned < delta.requested,
        "a 3x3 that spawned 7 must never read as a complete trial"
    );

    assert_eq!(
        delta.per_variant.len(),
        3,
        "one row per variant, verbatim order"
    );
    let by_variant: Vec<(Option<&str>, u64, u64)> = delta
        .per_variant
        .iter()
        .map(|v| (v.model.as_deref(), v.requested, v.spawned))
        .collect();
    assert_eq!(
        by_variant,
        vec![
            (None, 3, 3),
            (Some("model-alpha"), 3, 3),
            (Some("model-beta"), 3, 1),
        ],
        "per-variant: which cell is short is derivable via each sample's own \
         (agent, harness, model?) triple — model-beta is missing 2"
    );
}

/// Non-vacuity: a fully spawned matrix reads complete. Without this, a
/// projection that always reported a deficit would pass the judge above.
#[test]
fn a_fully_spawned_matrix_reads_complete() {
    let events = [
        ev(
            "trial.opened",
            json!({"trial": TRIAL, "idempotency_key": KEY,
                   "variants": [{"agent": "impl", "harness": "claude-code", "model": "model-alpha"}],
                   "samples": 2}),
        ),
        sample_spawned(
            "01DR055FVLLRVN000000000001",
            TRIAL,
            Some("model-alpha"),
            "dk-0",
        ),
        sample_spawned(
            "01DR055FVLLRVN000000000002",
            TRIAL,
            Some("model-alpha"),
            "dk-1",
        ),
    ];
    let graph = fold(events.iter());
    let delta = trial_delta(&graph, TRIAL).expect("delta exists");
    assert_eq!(
        (delta.requested, delta.spawned),
        (2, 2),
        "1x2, fully spawned"
    );
    assert_eq!(delta.per_variant[0].spawned, 2);
}

/// A trial id no `trial.opened` fact minted has NO delta: the requested end
/// does not exist and the projection never invents it (`None`, not a zero-row
/// fabrication).
#[test]
fn an_unknown_trial_id_has_no_delta() {
    let graph = fold(
        [sample_spawned(
            "01DR055STRAYRVN00000000001",
            TRIAL,
            None,
            "dk-stray",
        )]
        .iter(),
    );
    assert!(
        trial_delta(&graph, TRIAL).is_none(),
        "membership facts without the intent fact cannot produce a requested \
         count — the delta needs both ends (DR-055 set necessity bullet)"
    );
    assert!(
        trial_delta(&graph, OTHER_TRIAL).is_none(),
        "and an id on no fact at all"
    );
}

/// Grouping primacy: membership is `agent.spawned.trial?`, full stop. A sample
/// whose triple matches NO variant cell (a malformed or foreign-emitter fact)
/// still counts in the trial's spawned total — the group is by trial id, not
/// by triple — while no per-variant bucket claims it.
#[test]
fn an_unmatched_triple_still_counts_in_the_trial_group() {
    let events = [
        ev(
            "trial.opened",
            json!({"trial": TRIAL, "idempotency_key": KEY,
                   "variants": [{"agent": "impl", "harness": "claude-code", "model": "model-alpha"}],
                   "samples": 2}),
        ),
        sample_spawned(
            "01DR055GRPRVN0000000000001",
            TRIAL,
            Some("model-alpha"),
            "dk-0",
        ),
        // trial-tagged but a triple the matrix never requested:
        ev(
            "agent.spawned",
            json!({"run": "01DR055GRPRVN0000000000002", "agent": "rogue",
                   "harness": "claude-code", "trial": TRIAL, "idempotency_key": "dk-x"}),
        ),
    ];
    let graph = fold(events.iter());
    let delta = trial_delta(&graph, TRIAL).expect("delta exists");
    assert_eq!(
        delta.spawned, 2,
        "spawned counts the trial GROUP (membership = trial?, the ontology's \
         projection ruling), even when a member's triple matches no cell"
    );
    assert_eq!(
        delta.per_variant[0].spawned, 1,
        "but no variant cell claims the unmatched sample"
    );
}

// --- property: fold determinism, first-wins, rebuild equality ---------------

mod props {
    use super::*;
    use proptest::prelude::*;

    const TRIALS: [&str; 2] = ["01DR055PR0PTR000000000000A", "01DR055PR0PTR000000000000B"];
    const KEYS: [&str; 3] = ["prop-key-a", "prop-key-b", "prop-key-c"];

    fn opened(trial: &str, key: &str, samples: u64) -> Event {
        ev(
            "trial.opened",
            json!({
                "trial": trial,
                "idempotency_key": key,
                "variants": [{"agent": "impl", "harness": "claude-code"}],
                "samples": samples,
            }),
        )
    }

    proptest! {
        /// For ARBITRARY interleavings of `trial.opened` facts (duplicate ids,
        /// varying payloads) and trial-tagged / untagged `agent.spawned`
        /// facts:
        /// (a) each trial entry equals the FIRST fact seen for that id in log
        /// order — first-wins is exact, not merely "some fact wins";
        /// (b) each trial's delta.spawned equals an independent count of the
        /// membership group; and
        /// (c) incremental `Materializer` application equals fold-from-zero —
        /// the release-blocking `fold(log) == snapshot` / rebuild family
        /// (`rebuild` IS fold-from-zero), which is DR-055's whole-graph
        /// rebuild-equality criterion.
        #[test]
        fn first_fact_wins_and_incremental_equals_fold(
            seq in proptest::collection::vec(
                prop_oneof![
                    // a trial.opened: (trial idx, key idx, samples)
                    (0usize..2, 0usize..3, 1u64..4).prop_map(|(t, k, s)| (0usize, t, k, s)),
                    // an agent.spawned: trial membership 0/1 = tagged, 2 = untagged
                    (0usize..3, 0usize..8).prop_map(|(t, r)| (1usize, t, r, 0u64)),
                ],
                1..40,
            )
        ) {
            let mut events = Vec::new();
            let mut opened_model: std::collections::BTreeMap<&str, (&str, u64)> =
                std::collections::BTreeMap::new();
            let mut group_model: std::collections::BTreeMap<&str, std::collections::BTreeSet<String>> =
                std::collections::BTreeMap::new();

            for (i, &(kind, a, b, c)) in seq.iter().enumerate() {
                if kind == 0 {
                    events.push(opened(TRIALS[a], KEYS[b], c));
                    opened_model.entry(TRIALS[a]).or_insert((KEYS[b], c));
                } else {
                    // Unique run id per op index so agent_runs entries never collide.
                    let run = format!("01DR055PR0PRVN{i:012}");
                    let mut payload = json!({"run": run, "agent": "impl", "harness": "claude-code"});
                    if a < 2 {
                        payload["trial"] = json!(TRIALS[a]);
                        group_model.entry(TRIALS[a]).or_default().insert(run.clone());
                    }
                    events.push(ev("agent.spawned", payload));
                }
            }

            let folded = fold(events.iter());

            // (a) first-fact-wins, exactly.
            prop_assert_eq!(folded.trials.len(), opened_model.len());
            for (trial, (key, samples)) in &opened_model {
                let entry: &TrialState = folded.trials.get(*trial).expect("entry minted");
                prop_assert_eq!(
                    (entry.idempotency_key.as_str(), entry.samples),
                    (*key, *samples),
                    "trial {} folds the FIRST fact, whatever arrived later",
                    trial
                );
            }

            // (b) the delta's spawned end equals the independent group model.
            for (trial, runs) in &group_model {
                if opened_model.contains_key(trial) {
                    let delta = trial_delta(&folded, trial).expect("opened => delta");
                    prop_assert_eq!(delta.spawned, runs.len() as u64);
                } else {
                    prop_assert!(trial_delta(&folded, trial).is_none());
                }
            }

            // (c) rebuild equality.
            let mut live = Materializer::new();
            for event in &events {
                live.apply(event);
            }
            prop_assert_eq!(live.snapshot(), folded, "incremental == fold-from-zero (rebuild)");
        }
    }
}
