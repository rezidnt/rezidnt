//! DR-042 ORACLE (I3 rebuild-equivalence leg) — the orchestration graph rebuilds
//! from the LOG ALONE, with no in-daemon orchestration-session object of record
//! (DR-042 §Decision 2, the wedge: the inverse of Omnigent's server-held state).
//! FAILING-FIRST: `rezidnt_state::orchestration_graph` / `OrchestrationView` do
//! NOT exist yet, and the `badge_id` fold onto `AgentRunState` is implementer
//! work, so this file fails to COMPILE until they land. That is the correct red
//! — mirrors the Materializer `fold(log) == snapshot` property and
//! `reducer_props.rs` for the fold-from-zero / rebuild family.
//!
//! The property DR-042 §Consequences names as OWED: the graph is a pure fold and
//! `rebuild`-able. Concretely:
//! - `orchestration_graph(fold(log)) == orchestration_graph(fold(replay(log)))`
//!   — folding the whole log, projecting, equals rebuilding from seq 0 and
//!   projecting. A divergence is a reducer bug and a release blocker (I3).
//! - Incremental `Materializer::apply` (live materialization) then projecting
//!   equals fold-from-zero then projecting — the live derived view never drifts
//!   from the replayable one, across ARBITRARY event interleavings.

use std::path::PathBuf;

use rezidnt_state::{Materializer, fold, orchestration_graph};
use rezidnt_types::{Event, SourceId, Subject};
use serde_json::{Value, json};
use ulid::Ulid;

fn fixtures_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../spec/fixtures")
}

fn load(name: &str) -> Vec<Event> {
    let path = fixtures_dir().join(name);
    std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("fixture {} must exist: {e}", path.display()))
        .lines()
        .filter(|l| !l.trim().is_empty())
        .map(|l| serde_json::from_str(l).unwrap_or_else(|e| panic!("{name}: bad line ({e}): {l}")))
        .collect()
}

/// CRITERION 3 (DR-042) — the orchestration view rebuilds from the log alone.
/// Folding the committed fan-out fixture and projecting equals rebuilding from
/// seq 0 (a fresh `Materializer` fed the same log in order) and projecting.
/// There is no in-daemon session object: the two derivations agree because the
/// LOG is the only source of record (I3).
#[test]
fn orchestration_view_rebuilds_from_the_log_alone() {
    let log = load("dr042_orchestration_fanout.jsonl");

    // Path A: one-shot fold-from-zero, then project.
    let from_fold = orchestration_graph(&fold(log.iter()));

    // Path B: incremental live materialization (the "daemon" path) replaying the
    // log from seq 0, then project. This is exactly `rezidnt rebuild`: refold
    // from the start, no retained session state.
    let mut live = Materializer::new();
    for event in &log {
        live.apply(event);
    }
    let from_rebuild = orchestration_graph(&live.snapshot());

    assert_eq!(
        from_fold, from_rebuild,
        "orchestration_graph(fold(log)) MUST EQUAL orchestration_graph(rebuild(log)) — \
         the graph rebuilds from the log alone, no in-daemon orchestration session (I3, DR-042 §Decision 2)"
    );

    // Non-vacuity: the compared view actually carries the folded fan-out (a
    // matching pair of EMPTY views would be an oracle bug, not a pass).
    assert_eq!(
        from_fold.leads.len(),
        1,
        "the compared view carries the folded lead (non-vacuity): {from_fold:#?}"
    );
    assert_eq!(from_fold.leads[0].fan_out, 2, "and its two-wide fan-out");
}

// --- property: the projection is rebuild-stable over arbitrary interleavings --

mod props {
    use super::*;
    use proptest::prelude::*;

    const LEAD: &str = "01ORCHPROPLEAD0000000RL01";
    const SUBS: [(&str, &str); 3] = [
        ("01ORCHPROPSUB0000000RA01", "5ubprop000000a01"),
        ("01ORCHPROPSUB0000000RB01", "5ubprop000000b01"),
        ("01ORCHPROPSUB0000000RC01", "5ubprop000000c01"),
    ];
    const LEAD_BADGE: &str = "1eadprop00000001";

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

    proptest! {
        /// For an ARBITRARY subset+interleaving of the lead's delegations and the
        /// subs' spawns, the projection over the incrementally-materialized graph
        /// equals the projection over the fold-from-zero graph (the release-
        /// blocking `fold(log) == snapshot` / rebuild family — a divergence is a
        /// reducer bug, I3). The orchestration view holds NO state of its own; it
        /// is a pure function of the folded graph, so it inherits rebuild-stability.
        #[test]
        fn projection_is_rebuild_stable(
            // which subs the lead delegates to (a subset), and an interleave seed
            picks in proptest::collection::vec(0usize..SUBS.len(), 1..8),
        ) {
            // Lead spawn.
            let mut events = vec![ev(
                "agent.spawned",
                json!({"run": LEAD, "agent": "lead", "harness": "claude-code", "badge_id": LEAD_BADGE}),
            )];
            // For each pick (deduped by sub index), delegate the edge and spawn the
            // sub. Distinct picks → distinct subs; a repeated pick re-emits the same
            // facts (idempotent fold — the log is truth, I3). `agent.spawned` carries
            // only ontology-conformant fields (run/agent/harness/badge_id) — no
            // `worktree` field exists on this subject (see the projection oracle).
            let mut seen = std::collections::BTreeSet::new();
            for &i in &picks {
                let (sub_run, sub_badge) = SUBS[i];
                events.push(ev(
                    "permit.delegated",
                    json!({
                        "run": LEAD,
                        "parent_badge_id": LEAD_BADGE,
                        "child_badge_id": sub_badge,
                        "added_caveats": [],
                    }),
                ));
                events.push(ev(
                    "agent.spawned",
                    json!({
                        "run": sub_run, "agent": "sub", "harness": "claude-code",
                        "badge_id": sub_badge,
                    }),
                ));
                seen.insert(i);
            }

            // fold-from-zero, projected.
            let folded_view = orchestration_graph(&fold(events.iter()));

            // incremental (live) materialization, projected.
            let mut live = Materializer::new();
            for event in &events {
                live.apply(event);
            }
            let rebuilt_view = orchestration_graph(&live.snapshot());

            prop_assert_eq!(
                &folded_view, &rebuilt_view,
                "projection over incremental == projection over fold-from-zero (rebuild, I3)"
            );

            // And the derived fan-out equals the number of DISTINCT subs the lead
            // delegated to — the width is derived, never a stored fact (DR-042).
            let lead = folded_view
                .leads
                .iter()
                .find(|l| l.lead_run == LEAD)
                .expect("the lead surfaces once it has delegated");
            prop_assert_eq!(
                lead.fan_out,
                seen.len(),
                "fan_out is the DERIVED count of distinct delegated subs"
            );
        }
    }
}
