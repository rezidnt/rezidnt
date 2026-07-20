//! C1 oracle (DR-021 — the fold-source move) — the `action.metered` reducer arm.
//!
//! DR-021 B2 moved the C1 spend fold OFF the pre-action `permit.*` decision facts
//! onto a POST-action `action.metered` fact. This suite pins the reducer half of
//! that move at the state level (host-runnable, platform-neutral):
//!
//!   - CRITERION 4: an `action.metered` fact folds its MEASURED `spend_delta_usd`
//!     into `cumulative_spend_usd` (keyed on `run`); a `permit.*` fact folds NO
//!     spend, even one carrying a stray (retired) `spend_delta_usd`. The fold
//!     SOURCE move is asserted, not merely the total.
//!   - CRITERION 5: a DENIED action contributes ZERO to `cumulative_spend_usd` —
//!     no phantom charge (the B2 honesty property). Because spend now rides a
//!     SEPARATE post-action fact, a denial (no `action.metered` follows it) adds
//!     nothing. Replayable from the log.
//!
//! RED MODE (honest — the arm does NOT exist yet):
//!   - The `"action.metered"` match arm is absent from `apply`
//!     (crates/rezidnt-state/src/lib.rs), so an `action.metered` fact folds to the
//!     counters-only `_ => {}` branch → `cumulative_spend_usd` stays 0.0 → the
//!     fold assertions FAIL.
//!   - The permit reducer STILL folds `spend_delta_usd` off `permit.granted`
//!     (:725-726), so `permit_granted_folds_no_spend...` FAILS today (it folds 8.0,
//!     not 0.0). Green only once :725-726 is deleted.
//!
//! `input_tokens`/`output_tokens` on `action.metered` are recorded-only — they must
//! NOT fold into any accumulator (asserted here so the arm folds spend ONLY).

use rezidnt_state::{Materializer, fold};
use rezidnt_types::{Event, SourceId, Subject};
use serde_json::{Value, json};
use ulid::Ulid;

const RUN: &str = "01C1METEREDFOLD000000R01";

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

/// A post-action metering fact carrying the MEASURED delta (0.0 means
/// measured-zero, per the ontology, not absent).
fn metered(run: &str, spend_delta_usd: f64) -> Event {
    ev(
        "action.metered",
        json!({
            "run": run,
            "spend_delta_usd": spend_delta_usd,
            "input_tokens": 1200u64,
            "output_tokens": 300u64,
        }),
    )
}

/// CRITERION 4 (fold arm) — a single `action.metered` fact folds its measured
/// delta into `cumulative_spend_usd`, keyed on `run`. The measured $ is the
/// C1 fold source (DR-021 B2).
#[test]
fn action_metered_folds_measured_delta_into_cumulative_spend() {
    let graph = fold([metered(RUN, 3.5)].iter());
    let run = graph
        .agent_runs
        .get(RUN)
        .expect("an action.metered fact mints the run entry (I3)");
    assert_eq!(
        run.permit_accumulators.cumulative_spend_usd, 3.5,
        "the measured spend_delta_usd folds into cumulative_spend_usd (CRITERION 4)"
    );
}

/// CRITERION 4 — measured deltas ACCUMULATE across multiple `action.metered`
/// facts (each action's measured cost adds), and a measured-ZERO fact adds
/// nothing but does not vanish the run.
#[test]
fn action_metered_deltas_accumulate() {
    let graph = fold(
        [
            metered(RUN, 2.0),
            metered(RUN, 0.0), // measured-zero — present, adds nothing
            metered(RUN, 4.25),
        ]
        .iter(),
    );
    assert_eq!(
        graph.agent_runs[RUN]
            .permit_accumulators
            .cumulative_spend_usd,
        6.25,
        "measured deltas accumulate: 2.0 + 0.0 + 4.25 = 6.25 (CRITERION 4)"
    );
}

/// CRITERION 4 (recorded-only tokens) — `input_tokens`/`output_tokens` on
/// `action.metered` are RECORDED, NOT folded. A fact carrying only tokens and a
/// measured-zero spend leaves `cumulative_spend_usd` at 0.0. This pins that the
/// arm folds SPEND only, never the token counts.
#[test]
fn action_metered_tokens_are_recorded_only_never_folded() {
    let big_tokens = ev(
        "action.metered",
        json!({"run": RUN, "spend_delta_usd": 0.0, "input_tokens": 999999u64, "output_tokens": 888888u64}),
    );
    let graph = fold([big_tokens].iter());
    assert_eq!(
        graph.agent_runs[RUN]
            .permit_accumulators
            .cumulative_spend_usd,
        0.0,
        "tokens are recorded-only — a measured-zero metering fact folds zero spend (CRITERION 4)"
    );
}

/// CRITERION 4 (I3) — an `action.metered` fact MISSING `run` folds as
/// counters-only: the reducer never guesses a key, never mints a run, never
/// panics (the established permit-reducer discipline).
#[test]
fn keyless_action_metered_folds_counters_only() {
    let graph = fold([ev("action.metered", json!({"spend_delta_usd": 5.0}))].iter());
    assert_eq!(graph.events_folded, 1, "the fact is still counted");
    assert!(
        graph.agent_runs.is_empty(),
        "a keyless action.metered fact mints no run entry (I3)"
    );
}

/// CRITERION 4 (the fold-SOURCE move) — a `permit.granted` carrying a STRAY,
/// RETIRED `spend_delta_usd` folds NO spend. After DR-021 the permit reducer must
/// STOP reading `spend_delta_usd`; only `action.metered` moves the accumulator.
/// This asserts the source move directly: the permit fact's spend is ignored.
///
/// RED today: the reducer at :725-726 still folds it → 8.0, not 0.0.
#[test]
fn permit_granted_stray_spend_folds_zero_source_moved() {
    const REQ: &str = "01C1SRCMOVEPERMITREQ00Q01";
    let graph = fold(
        [ev(
            "permit.granted",
            json!({
                "run": RUN,
                "request_id": REQ,
                "policy_ref": {"hash": "50urc3m0v3d0000000000000000000000000000000000000000000000000001", "bytes": 32, "mime": "application/octet-stream"},
                // RETIRED as the C1 fold source (DR-021) — must be IGNORED.
                "spend_delta_usd": 8.0,
                // risk_delta rides the permit path; on the GRANTED arm it folds
                // into the running score (DR-024 Q3 — granted-only fold).
                "risk_delta": 2.0,
            }),
        )]
        .iter(),
    );
    let acc = &graph.agent_runs[RUN].permit_accumulators;
    assert_eq!(
        acc.cumulative_spend_usd, 0.0,
        "a stray spend_delta_usd on permit.granted folds ZERO — the C1 fold source moved to \
         action.metered (CRITERION 4)"
    );
    assert_eq!(
        acc.risk_score, 2.0,
        "a GRANTED action's risk_delta folds off the permit fact into the running score \
         (DR-024 Q3 granted-only fold — this is the granted arm, so it counts)"
    );
    assert_eq!(acc.granted, 1, "the grant is still counted as a decision");
}

/// CRITERION 5 (the B2 honesty property) — a DENIED action contributes ZERO to
/// `cumulative_spend_usd`: no phantom charge. Because spend now rides a SEPARATE
/// post-action `action.metered` fact, a denial that is NOT followed by an
/// `action.metered` (the action never ran, so it cost nothing) adds nothing. Here
/// a metered action ($4.0) that DID run is charged, and a subsequent DENIED action
/// (never run, no metering fact) adds nothing — cumulative stays 4.0. Replayable
/// from the log.
///
/// RED today: the deny carries a stray `spend_delta_usd: 3.0` which the current
/// reducer WRONGLY folds → 7.0, a phantom charge on a denied action. Green only
/// once the permit fold-source is removed and spend rides action.metered alone.
#[test]
fn denied_action_charges_zero_no_phantom() {
    const REQ_DENY: &str = "01C1NOPHANTOMDENYREQ00Q01";
    let graph = fold(
        [
            // An action that RAN and was measured at $4.0 — the honest charge.
            metered(RUN, 4.0),
            // A DENIED action — never ran, so no action.metered follows it. The
            // stray spend_delta_usd is a retired field the reducer must IGNORE.
            ev(
                "permit.denied",
                json!({
                    "run": RUN,
                    "request_id": REQ_DENY,
                    "policy_ref": {"hash": "n0ph4n70md3n1ed000000000000000000000000000000000000000000000001", "bytes": 32, "mime": "application/octet-stream"},
                    "reason": "cumulative spend crossed hard cap",
                    "spend_delta_usd": 3.0, // retired — must NOT be charged (phantom)
                }),
            ),
        ]
        .iter(),
    );
    let acc = &graph.agent_runs[RUN].permit_accumulators;
    assert_eq!(
        acc.cumulative_spend_usd, 4.0,
        "only the action that RAN ($4.0, folded from action.metered) is charged; the DENIED \
         action adds ZERO — no phantom charge (CRITERION 5, the B2 honesty property)"
    );
    assert_eq!(acc.denied, 1, "the denial is still counted as a decision");
}

/// CRITERION 4/5 (rebuild-safe) — the `action.metered` fold is rebuild-safe:
/// incremental `Materializer::apply` equals `fold`-from-zero over an interleaving
/// of metering + permit facts (the release-blocking `fold(log) == snapshot`
/// family). A divergence here is a reducer bug, not a flaky test.
#[test]
fn action_metered_fold_is_rebuild_safe() {
    let events = [
        metered(RUN, 1.5),
        ev(
            "permit.granted",
            json!({"run": RUN, "request_id": "01C1RBLDPERMITREQ0000Q01",
                   "policy_ref": {"hash": "r3bu1ld54f30000000000000000000000000000000000000000000000000001", "bytes": 8, "mime": "application/octet-stream"},
                   "spend_delta_usd": 99.0}), // retired stray — must not fold
        ),
        metered(RUN, 2.5),
    ];
    let folded = fold(events.iter());
    assert_eq!(
        folded.agent_runs[RUN]
            .permit_accumulators
            .cumulative_spend_usd,
        4.0,
        "only the two metered deltas fold: 1.5 + 2.5 = 4.0 (the permit stray is ignored)"
    );

    let mut live = Materializer::new();
    for event in &events {
        live.apply(event);
    }
    assert_eq!(
        live.snapshot(),
        folded,
        "incremental application equals fold-from-zero — the action.metered fold is rebuild-safe"
    );
}
