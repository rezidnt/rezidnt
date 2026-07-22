//! DR-033 slice-2 (operator-resolve-escalation) ORACLE — CRITERION 1: the
//! `permit.resolved` folding reducer (the pure deterministic judge). The subject
//! is minted (spec/ontology.md, DR-033 set, line 555-575); the FOLD is the
//! slice-2 implementer's obligation and DOES NOT EXIST YET, so these tests are
//! COMPILE-RED on the not-yet-existing field/type until the arm lands.
//!
//! ## Why a fold at all (the load-bearing PDP contract this feeds)
//! DR-033 §Decision 1 makes the resolution "honored on next ask" by having
//! `decide_permit` consult the FOLDED ledger BEFORE running verifiers. So the
//! resolution must be findable from the log by ACTION IDENTITY `(run, tool,
//! action/target)` — NOT by `request_id`, which is re-minted per ask (DR-033
//! §Context; `crates/rezidnt-mcp/src/lib.rs:871`). This board pins that fold:
//! the fact folds, and it is findable by the SAME key the PDP will match on.
//!
//! ## API surface this board PINS (implementer builds to exactly this)
//! A new `#[serde(default)]` field on `AgentRunState`
//! (`crates/rezidnt-state/src/lib.rs`), mirroring the `delegations` precedent so
//! every pre-DR-033 golden fixture parses and compares equal unchanged (I3):
//! ```ignore
//! #[serde(default)]
//! pub resolutions: Vec<PermitResolution>,
//! ```
//! with `pub struct PermitResolution { request_id: String, action: String,
//! target: serde_json::Value, decision: String, operator_badge_id: Option<String>,
//! reason: Option<String> }` (`Debug, Clone, Default, PartialEq, Serialize,
//! Deserialize`). The ontology (line 572) hands the EXACT shape to the
//! implementer as an oracle-first call; this board pins the BEHAVIOR the shape
//! must deliver:
//!   - a `permit.resolved` fact folds ONE `PermitResolution` onto the run,
//!     keyed/findable by `(run, action, target)`;
//!   - `decision` / `operator_badge_id` / `reason` fold VERBATIM (I3 — the
//!     reducer never re-derives; `decision` stays the human input verb
//!     `"allow"`/`"deny"`, never coerced to `granted`/`denied`);
//!   - `request_id` folds as the AUDIT correlation (which escalation this
//!     answers), NOT the match key;
//!   - a keyless fact (missing `run`) folds counters-only / no-op, never panics;
//!   - a NEW resolution for the same action appends (last-matching-wins per
//!     DR-033 §Decision 2 "a new resolve overrides") — the append-order
//!     discipline the `delegations` chain uses;
//!   - `fold(log) == incremental Materializer` (the release-blocking
//!     rebuild-stability property — a divergence is a reducer bug, I3).
//!
//! The implementer MAY instead extend `PermitLedgerEntry`; if so, `resolution_for`
//! below is the accessor to provide. Do NOT weaken the "findable by action
//! identity" requirement — that is what the PDP ledger-check keys on.
//!
//! RED MODE: COMPILE-RED — `AgentRunState::resolutions` / `PermitResolution` do
//! not exist, so the crate fails to compile until the fold lands. That is the
//! correct red state (mirrors `permit_delegation.rs`).

use rezidnt_state::{Materializer, PermitResolution, fold};
use rezidnt_types::{Event, SourceId, Subject};
use serde_json::{Value, json};
use ulid::Ulid;

const RUN: &str = "01DR033RES0LVE0000000000R1";

fn ev(subject: &str, payload: Value) -> Event {
    Event::new(
        SourceId::new("rezidnt-mcp"),
        None,
        Subject::new(subject),
        Ulid::new(),
        None,
        1,
        payload,
    )
    .expect("test event under 32KiB")
}

/// The full DR-033 payload the tool emits (line 555-562): the escalated
/// `request_id` (audit correlation), the reused `action` + `target` (the
/// next-ask match key), the human `decision`, the operator `reason?`, and the
/// loggable `operator_badge_id?` (never the token).
fn resolved(action: &str, tool: &str, decision: &str) -> Value {
    json!({
        "run": RUN,
        "request_id": "01DR033ESCALATEDREQ00000R1",
        "action": action,
        "target": { "tool": tool },
        "decision": decision,
        "reason": "operator approved this Bash invocation",
        "operator_badge_id": "0badc0de",
    })
}

/// Find the resolution the PDP would match for a `(run, action, target)` — the
/// accessor the implementer MUST expose (a `resolutions` scan, or a
/// `resolution_for` method). Kept in the test so the board pins BEHAVIOR, not a
/// private field name: the PDP ledger-check needs exactly this lookup.
fn resolution_for<'a>(
    run: &'a rezidnt_state::AgentRunState,
    action: &str,
    tool: &str,
) -> Option<&'a PermitResolution> {
    run.resolutions
        .iter()
        .rev()
        .find(|r| r.action == action && r.target.get("tool").and_then(Value::as_str) == Some(tool))
}

/// CRITERION 1 (fold + findable-by-action) — a `permit.resolved` fact folds ONE
/// resolution onto the run, findable by the `(run, tool, action/target)` key the
/// PDP will match on the NEXT ask. `decision`/`operator_badge_id`/`reason` fold
/// VERBATIM. COMPILE-RED on `resolutions`/`PermitResolution`.
#[test]
fn resolved_folds_findable_by_action_identity() {
    let events = [ev(
        "permit.resolved",
        resolved("tool.invoke", "Bash", "allow"),
    )];
    let graph = fold(events.iter());
    let run = graph
        .agent_runs
        .get(RUN)
        .expect("a resolved fact creates the run entry — no spawn required (I3)");

    let res = resolution_for(run, "tool.invoke", "Bash")
        .expect("the resolution is findable by (run, tool, action/target) — the PDP match key");
    assert_eq!(
        res.decision, "allow",
        "the human decision folds VERBATIM as the input verb `allow` — NEVER \
         coerced to `granted` (that coercion is the PDP's job on the next ask, I6)"
    );
    assert_eq!(
        res.operator_badge_id.as_deref(),
        Some("0badc0de"),
        "the loggable operator badge id folds verbatim (never the token, §12/I2)"
    );
    assert_eq!(
        res.reason.as_deref(),
        Some("operator approved this Bash invocation"),
        "the operator reason folds verbatim (I6 interrogability)"
    );
    assert_eq!(
        res.request_id, "01DR033ESCALATEDREQ00000R1",
        "the escalated request_id folds as the AUDIT correlation (which escalation \
         this answers) — NOT the match key (request_id is re-minted per ask, DR-033 §Context)"
    );
}

/// CRITERION 1 (deny arm) — a `deny` resolution folds with `decision = "deny"`,
/// verbatim (never coerced to `denied` — the human input verb is preserved).
#[test]
fn resolved_deny_folds_verbatim() {
    let events = [ev(
        "permit.resolved",
        resolved("tool.invoke", "Bash", "deny"),
    )];
    let graph = fold(events.iter());
    let run = &graph.agent_runs[RUN];
    let res = resolution_for(run, "tool.invoke", "Bash").expect("deny resolution is findable");
    assert_eq!(
        res.decision, "deny",
        "a deny resolution folds `decision = \"deny\"` verbatim (I3)"
    );
}

/// CRITERION 1 (request-scoped, not over-broad) — a resolution for `Bash` is NOT
/// found for a DIFFERENT action target (`Write`). This is the fold-side guard
/// that pins DR-033 §Decision 3: the resolution answers ONE action identity, it
/// does NOT match unrelated actions on the run.
#[test]
fn resolution_does_not_match_a_different_action() {
    let events = [ev(
        "permit.resolved",
        resolved("tool.invoke", "Bash", "allow"),
    )];
    let graph = fold(events.iter());
    let run = &graph.agent_runs[RUN];
    assert!(
        resolution_for(run, "tool.invoke", "Bash").is_some(),
        "the resolved action IS findable"
    );
    assert!(
        resolution_for(run, "tool.invoke", "Write").is_none(),
        "a DIFFERENT action target has NO resolution — the resolution is \
         request-scoped, not a broad grant (DR-033 §Decision 3)"
    );
}

/// CRITERION 1 (override discipline, DR-033 §Decision 2) — a NEW resolution for
/// the SAME action stands as the applied one (last-matching-wins, append-only).
/// A mistaken `deny` is corrected by a later `allow`, no clock dependency.
#[test]
fn a_new_resolution_overrides_the_prior_one() {
    let events = [
        ev("permit.resolved", resolved("tool.invoke", "Bash", "deny")),
        ev("permit.resolved", resolved("tool.invoke", "Bash", "allow")),
    ];
    let graph = fold(events.iter());
    let run = &graph.agent_runs[RUN];
    let res =
        resolution_for(run, "tool.invoke", "Bash").expect("the overriding resolution is findable");
    assert_eq!(
        res.decision, "allow",
        "the LATEST resolution for the action wins (DR-033 §Decision 2: a new \
         resolve overrides; append-only, no TTL/clock) — got {:?}",
        res.decision
    );
}

/// I3 — a keyless `permit.resolved` (missing `run`) folds counters-only / no-op,
/// never panics; the reducer never guesses a key (the established permit-reducer
/// discipline, mirrors `apply_permit_decision` / delegations).
#[test]
fn keyless_resolved_folds_counters_only() {
    let events = [ev(
        "permit.resolved",
        json!({
            "request_id": "01DR033ORPHANRESOLVE0000R1",
            "action": "tool.invoke",
            "target": { "tool": "Bash" },
            "decision": "allow"
        }),
    )];
    let graph = fold(events.iter());
    assert_eq!(graph.events_folded, 1, "the fact is still counted");
    assert!(
        graph.agent_runs.is_empty(),
        "a keyless resolution mints no run entry (I3)"
    );
}

/// CRITERION 1 (golden replay) — the committed
/// `spec/fixtures/dr033_resolve_escalation.jsonl` (spawn → requested → escalated
/// → resolved on one run) folds so the resolution is findable by action identity
/// and its human decision/attribution fold verbatim. Pins the log→graph contract
/// against a committed, minimal, behavior-named fixture (testing-oracles golden
/// event-log fixture). No `.expected.json` companion: the exact `resolutions`
/// field shape is the implementer's oracle-first call (ontology line 572), so
/// this board asserts the BEHAVIOR via the `resolution_for` accessor, not a
/// brittle whole-graph snapshot that presumes private field names.
///
/// COMPILE-RED on `resolutions`; then a green fold makes it pass.
#[test]
fn golden_fixture_resolution_folds_findable() {
    let path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../spec/fixtures/dr033_resolve_escalation.jsonl");
    let text = std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("fixture {} must exist: {e}", path.display()));
    let events: Vec<Event> = text
        .lines()
        .filter(|l| !l.trim().is_empty())
        .map(|l| Event::from_json_line(l).unwrap_or_else(|e| panic!("bad fixture line ({e}): {l}")))
        .collect();
    let graph = fold(events.iter());

    const FIXTURE_RUN: &str = "01DR033FIXTURE0000000RN01";
    let run = graph
        .agent_runs
        .get(FIXTURE_RUN)
        .expect("the fixture's run folds");
    let res = resolution_for(run, "tool.invoke", "Bash")
        .expect("the golden fixture's resolution is findable by (run, tool, action/target)");
    assert_eq!(res.decision, "allow", "the human decision folds verbatim");
    assert_eq!(
        res.operator_badge_id.as_deref(),
        Some("0badc0de"),
        "the operator attribution folds verbatim (never the token)"
    );
    assert_eq!(
        res.request_id, "01DR033FIXTURE00000RQESC1",
        "the escalated request_id folds as the audit correlation"
    );
}

// --- property: resolution folds are ordered + rebuild-stable ----------------

mod props {
    use super::*;
    use proptest::prelude::*;

    const RUNS: [&str; 2] = ["01DR033PROPRES0LVE0000R01", "01DR033PROPRES0LVE0000R02"];
    const DECISIONS: [&str; 2] = ["allow", "deny"];

    fn resolution_ev(run: &str, tool: &str, decision: &str, req: u32) -> Event {
        ev(
            "permit.resolved",
            json!({
                "run": run,
                "request_id": format!("01DR033PROPREQ{req:012}"),
                "action": "tool.invoke",
                "target": { "tool": tool },
                "decision": decision,
                "operator_badge_id": "0badc0de",
            }),
        )
    }

    proptest! {
        /// For ARBITRARY interleavings of resolutions across two runs and tools:
        /// (a) the LATEST resolution folded for a `(run, tool)` is the one an
        /// action-identity lookup returns (last-matching-wins, DR-033 §Decision 2);
        /// and (b) incremental Materializer application equals fold-from-zero (the
        /// release-blocking `fold(log) == snapshot` rebuild property — a divergence
        /// is a reducer bug, I3). No wall-clock is read; the judge is pure.
        #[test]
        fn resolutions_fold_last_wins_and_rebuild_stable(
            seq in proptest::collection::vec((0usize..2, 0usize..2, 0usize..2), 1..40),
        ) {
            let tools = ["Bash", "Write"];
            let events: Vec<Event> = seq
                .iter()
                .enumerate()
                .map(|(i, &(r, t, d))| resolution_ev(RUNS[r], tools[t], DECISIONS[d], i as u32))
                .collect();

            // Independent model: last decision seen per (run, tool).
            let mut model: std::collections::BTreeMap<(&str, &str), &str> =
                std::collections::BTreeMap::new();
            for &(r, t, d) in &seq {
                model.insert((RUNS[r], tools[t]), DECISIONS[d]);
            }

            let folded = fold(events.iter());
            for (&(run, tool), &decision) in &model {
                let run_state = folded.agent_runs.get(run).expect("run entry exists");
                let got = resolution_for(run_state, "tool.invoke", tool)
                    .expect("a folded (run, tool) resolution is findable");
                prop_assert_eq!(
                    &got.decision, decision,
                    "last-matching-wins for ({}, {})", run, tool
                );
            }

            let mut live = Materializer::new();
            for event in &events {
                live.apply(event);
            }
            prop_assert_eq!(live.snapshot(), folded, "incremental == fold-from-zero (rebuild, I3)");
        }
    }
}
