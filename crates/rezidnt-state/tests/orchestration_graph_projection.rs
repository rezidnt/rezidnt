//! DR-042 ORACLE (fold/projection leg) — the `orchestration_graph` pure
//! projection over a lead → parallel sub-runs fan-out, folded from EXISTING
//! facts (I3). FAILING-FIRST: `rezidnt_state::orchestration_graph` and the
//! `OrchestrationView` / `LeadRow` / `SubRow` types DO NOT EXIST YET, and the
//! `badge_id` fold onto `AgentRunState` is implementer work, so this file fails
//! to COMPILE (unresolved path / no field) until the fold + the projection land.
//! That is the correct red state — mirrors the `project` / `escalations`
//! projection oracles for `board_view` / `get_escalations`.
//!
//! ## The contract this pins (DR-042, derive-first; NO new subject, NO ontology
//! change — the graph is a PURE FOLD over existing events, I3)
//!
//! Lead→sub edge derivation (DR-042 §Decision 2, ontology line 223 + DR-018):
//! for an agent badge `badge_id == hex(blake3(sig)[..8])`, the SAME derivation
//! that keys `permit.delegated.child_badge_id`. So the edge is
//! `lead.delegations[].child_badge_id == sub.agent.spawned.badge_id`. Fan-out
//! width is DERIVED (count of subs per lead), never a stored fact.
//!
//! ## API SURFACE this board PINS (implementer builds to EXACTLY this)
//! In `crates/rezidnt-state/src/lib.rs`, mirroring `project(&Graph) -> BoardView`
//! and `escalations(&Graph, filter) -> Vec<EscalationRow>`:
//! ```ignore
//! /// PURE: `&Graph -> OrchestrationView`. No IO, no clock, deterministic
//! /// (BTreeMap key order). Carries derived state VERBATIM (I3): re-interprets
//! /// nothing. Verdicts three-valued, `inconclusive` NEVER coerced (I6).
//! pub fn orchestration_graph(graph: &Graph) -> OrchestrationView;
//!
//! #[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
//! pub struct OrchestrationView { pub leads: Vec<LeadRow> }
//!
//! #[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
//! pub struct LeadRow {
//!     pub lead_run: String,        // the lead's run ULID key
//!     pub fan_out: usize,          // DERIVED count of subs (no stored fact)
//!     pub subs: Vec<SubRow>,       // one row per delegated sub, deterministic order
//! }
//!
//! #[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
//! pub struct SubRow {
//!     pub sub_run: String,         // the sub's run ULID key
//!     pub status: String,          // verbatim AgentRunState.status (I3)
//!     pub verdicts: Vec<(String,String)>, // (gate, verdict) from AgentRunState.gates, verbatim
//!     pub integrity_alarms: usize, // AgentRunState.integrity_alarms.len()
//! }
//! ```
//! plus ONE fold the implementer adds to the `agent.spawned` reducer arm
//! (`crates/rezidnt-state/src/lib.rs`), a spawn-time property the sub's own spawn
//! already knows, mirroring the existing `pep?` / `role?` optional folds:
//! - `pub badge_id: Option<String>` on `AgentRunState`, folded VERBATIM from
//!   `agent.spawned.badge_id` (ontology line 223, a REQUIRED v1 field — the id
//!   the lead↔sub edge keys on). This is the field DR-042 names as "emitted but
//!   not yet folded"; the edge cannot be derived until it is folded.
//!
//! NO `worktree` on `SubRow` (orchestrator scope-correction 2026-07-24, verified
//! against the ontology + reducers): `agent.spawned` has NO `worktree` field
//! (ontology lines 217-230: run/agent/harness/harness_version?/pid?/badge_id/
//! idempotency_key?/bare?/allowed_tools?/pep?/role?), and `worktree.allocated`
//! carries NO run linkage (`allocator` is the delegating run, not a per-run join
//! the reducer folds onto `AgentRunState`). A sub's worktree is only derivable
//! AFTER a `diff.ready`/`diff.merged` fact (keyed `{run, worktree}`) — not at
//! spawn, not for a running sub. Folding it would need a NEW ontology field →
//! a warden `/subject`, which DR-042 §Decision 6 FORBIDS without a warden pass.
//! Keeping the prototype zero-ontology-change is the settled derive-first result.

use std::path::PathBuf;

use rezidnt_state::{fold, orchestration_graph};
use rezidnt_types::Event;

fn fixtures_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../spec/fixtures")
}

/// Load a committed golden fixture as its event vec (the fixture IS the log — I3).
fn load(name: &str) -> Vec<Event> {
    let path = fixtures_dir().join(name);
    std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("fixture {} must exist: {e}", path.display()))
        .lines()
        .filter(|l| !l.trim().is_empty())
        .map(|l| serde_json::from_str(l).unwrap_or_else(|e| panic!("{name}: bad line ({e}): {l}")))
        .collect()
}

// The lead and the two subs the `dr042_orchestration_fanout.jsonl` fixture folds.
const LEAD_RUN: &str = "01RCHRN00000000000000EAD01";
const SUB_A_RUN: &str = "01RCHRN00000000000000SBA01";
const SUB_B_RUN: &str = "01RCHRN00000000000000SBB01";

/// CRITERION 1 (DR-042) — one lead that delegated to TWO subs folds to ONE
/// `LeadRow` with `fan_out == 2` and both `SubRow`s, each carrying its own
/// sub_run / status, with the inconclusive gate verdict surfaced VERBATIM (NOT
/// coerced — I6). The whole graph is a PURE FOLD over existing events (I3): no
/// orchestration subject, no in-daemon session object.
#[test]
fn fanout_folds_to_one_lead_with_two_subs() {
    let events = load("dr042_orchestration_fanout.jsonl");
    let view = orchestration_graph(&fold(events.iter()));

    // Exactly one lead (the run that owns the two `permit.delegated` edges).
    assert_eq!(
        view.leads.len(),
        1,
        "one lead fanning out to two subs folds to exactly one LeadRow: {view:#?}"
    );
    let lead = &view.leads[0];
    assert_eq!(
        lead.lead_run, LEAD_RUN,
        "the LeadRow is keyed on the lead's run"
    );

    // Fan-out width is DERIVED (count of subs), never a stored fact (DR-042).
    assert_eq!(
        lead.fan_out, 2,
        "fan_out is the DERIVED count of subs delegated by this lead: {lead:#?}"
    );
    assert_eq!(
        lead.subs.len(),
        2,
        "one SubRow per delegated sub — the lead→sub edge is \
         lead.delegations[].child_badge_id == sub.agent.spawned.badge_id (DR-042 §Decision 2)"
    );

    // The subs surface in deterministic order (BTreeMap key order over the run
    // ids): SUB_A ("...RA01") sorts before SUB_B ("...RB01").
    let sub_a = &lead.subs[0];
    let sub_b = &lead.subs[1];
    assert_eq!(
        sub_a.sub_run, SUB_A_RUN,
        "subs surface in deterministic key order"
    );
    assert_eq!(
        sub_b.sub_run, SUB_B_RUN,
        "subs surface in deterministic key order"
    );

    // Per-sub status folds from AgentRunState.status, VERBATIM (I3). Sub A
    // completed; sub B is mid-flight (running) — distinct statuses so an
    // all-one-status projection could never match by accident.
    assert_eq!(
        sub_a.status, "completed",
        "sub A ran to completion — status folds verbatim from agent.completed (I3)"
    );
    assert_eq!(
        sub_b.status, "running",
        "sub B is mid-flight — status folds verbatim from agent.status.changed (I3)"
    );

    // I6 NON-COERCION — the load-bearing assertion. Sub A passed its `vet` gate;
    // sub B's `pre_merge` verdict is `inconclusive`, and it MUST surface as
    // `inconclusive` VERBATIM — never coerced to pass/fail (I6, DR-042 §Invariant
    // I6). Verdicts fold from AgentRunState.gates as (gate, verdict) pairs.
    assert_eq!(
        sub_a.verdicts,
        vec![("vet".to_string(), "pass".to_string())],
        "sub A's passed gate surfaces verbatim as (gate, verdict)"
    );
    assert_eq!(
        sub_b.verdicts,
        vec![("pre_merge".to_string(), "inconclusive".to_string())],
        "sub B's INCONCLUSIVE verdict surfaces VERBATIM — never coerced to pass/fail (I6)"
    );
    // Redundant, explicit I6 guard so a regression that coerced the verdict is
    // unambiguous in the failure output.
    assert!(
        sub_b.verdicts.iter().any(|(_, v)| v == "inconclusive"),
        "an inconclusive sub folds back inconclusive — I6 is never coerced: {sub_b:#?}"
    );
    assert!(
        !sub_b.verdicts.iter().any(|(_, v)| v == "pass"),
        "the inconclusive verdict must NOT be coerced to pass (I6): {sub_b:#?}"
    );

    // No integrity alarms on a clean fan-out — honest zero.
    assert_eq!(
        sub_a.integrity_alarms, 0,
        "clean sub A has no divergence alarms"
    );
    assert_eq!(
        sub_b.integrity_alarms, 0,
        "clean sub B has no divergence alarms"
    );
}

/// I3 non-vacuity — the projection carries the FOLDED edge, not an empty
/// scaffold. A lead with NO delegations (the sub-runs alone, keyed off no lead)
/// would not produce a LeadRow; here the two `permit.delegated` facts create the
/// edge, so the single LeadRow's two subs are the proof the edge folded.
#[test]
fn fanout_lead_row_is_derived_from_the_delegation_edges() {
    let events = load("dr042_orchestration_fanout.jsonl");
    let view = orchestration_graph(&fold(events.iter()));
    let total_subs: usize = view.leads.iter().map(|l| l.subs.len()).sum();
    assert_eq!(
        total_subs, 2,
        "the two delegation edges fold to exactly two sub rows across the fleet (I3): {view:#?}"
    );
    // A matching-but-empty view (zero leads / zero subs) would be an oracle bug,
    // not a pass — pin non-vacuity.
    assert!(
        !view.leads.is_empty(),
        "the fixture delegates to real subs; an empty view is a bug, not a pass"
    );
}
