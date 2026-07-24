//! DR-042 ORACLE (fold/projection leg) — the `orchestration_graph` pure
//! projection over a lead → parallel sub-runs fan-out, folded from EXISTING
//! facts (I3). FAILING-FIRST: `rezidnt_state::orchestration_graph` and the
//! `OrchestrationView` / `LeadRow` / `SubRow` types DO NOT EXIST YET, and the
//! `badge_id` fold onto `AgentRunState` is implementer work, so this file fails
//! to COMPILE (unresolved path / no field) until the fold + the projection land.
//! That is the correct red state — mirrors the `project` / `escalations`
//! projection oracles for `board_view` / `get_escalations`.
//!
//! ## The contract this pins (DR-042, derive-first; the graph is a PURE FOLD
//! over existing events, I3)
//!
//! Lead→sub edge derivation, RE-CUT 2026-07-24 under DR-046 §Decision 4/5: the
//! edge is `sub.agent.spawned.lead_run == <the lead's run>`, run-to-run, folded
//! to `AgentRunState::lead_run`. It was previously
//! `lead.delegations[].child_badge_id == sub.badge_id`, which read a
//! `permit.delegated` fact — an ATTENUATION fact — as a fan-out; that emit is
//! WITHDRAWN, so a genuine lead now folds ZERO delegations and a projection that
//! still gated on `delegations` would report `fan_out: 0` for every real lead.
//! Fan-out width stays DERIVED (count of subs per lead), never a stored fact.
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
//! plus the folds the implementer adds to the `agent.spawned` reducer arm
//! (`crates/rezidnt-state/src/lib.rs`), spawn-time properties the sub's own spawn
//! already knows, mirroring the existing `pep?` / `role?` optional folds:
//! - `pub badge_id: Option<String>`, folded VERBATIM from
//!   `agent.spawned.badge_id` (a REQUIRED v1 field) — this run's badge
//!   ATTRIBUTION. It keyed the edge under DR-042; since DR-046 it does not.
//! - `pub lead_run: Option<String>`, folded VERBATIM from the optional
//!   `agent.spawned.lead_run` (DR-046 §Decision 5) — the edge itself. Absent on
//!   every ordinary spawn, never synthesized.
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
use rezidnt_types::{Event, SourceId, Subject};
use serde_json::json;
use ulid::Ulid;

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
        "one SubRow per sub — the lead→sub edge is \
         sub.agent.spawned.lead_run == lead_run (DR-046 §Decision 5)"
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
/// scaffold. Sub runs whose spawns named no lead would produce no LeadRow; here
/// each sub's `agent.spawned.lead_run` names the lead, so the single LeadRow's
/// two subs are the proof the edge folded.
#[test]
fn fanout_lead_row_is_derived_from_the_subs_lead_run() {
    let events = load("dr042_orchestration_fanout.jsonl");
    let view = orchestration_graph(&fold(events.iter()));
    let total_subs: usize = view.leads.iter().map(|l| l.subs.len()).sum();
    assert_eq!(
        total_subs, 2,
        "the two lead_run edges fold to exactly two sub rows across the fleet (I3): {view:#?}"
    );
    // A matching-but-empty view (zero leads / zero subs) would be an oracle bug,
    // not a pass — pin non-vacuity.
    assert!(
        !view.leads.is_empty(),
        "the fixture's subs name a real lead; an empty view is a bug, not a pass"
    );
}

/// DR-046 §Consequences (c), the OWED guard — the whole point of the withdrawal,
/// asserted on both sides at once so neither can drift:
///
/// - the fanned-out lead folds **ZERO** attenuation records, and `BoardRow`'s
///   `delegated` (delegation-chain DEPTH, `delegations.len()`, UNCHANGED by
///   DR-046) reports **0** for it. Before the withdrawal it reported 2 — two
///   attenuations that never happened, on the dossier `debrief` reads (I3).
/// - while `orchestration_graph` STILL folds both subs. That second half is what
///   makes this more than a deletion test: it is the exact pair of facts the
///   projection's removed `delegations.is_empty()` early-return would have
///   broken silently, reporting `fan_out: 0` for a genuine lead while every
///   other assertion in this file still passed.
#[test]
fn a_fanned_out_lead_folds_zero_attenuations_while_its_subs_still_project() {
    let events = load("dr042_orchestration_fanout.jsonl");
    let graph = fold(events.iter());

    // Side 1 — the attenuation chain is EMPTY. A fan-out narrows nothing.
    let lead_state = graph
        .agent_runs
        .get(LEAD_RUN)
        .unwrap_or_else(|| panic!("the lead folds: {:#?}", graph.agent_runs.keys()));
    assert!(
        lead_state.delegations.is_empty(),
        "a fan-out is NOT an attenuation: the lead folds ZERO DelegationRecords \
         (DR-046 §Decision 4). Got: {:#?}",
        lead_state.delegations
    );
    let board = rezidnt_state::project(&graph);
    let lead_row = board
        .runs
        .iter()
        .find(|r| r.run == LEAD_RUN)
        .unwrap_or_else(|| panic!("the lead has a board row: {board:#?}"));
    assert_eq!(
        lead_row.delegated, 0,
        "BoardRow.delegated is delegation-chain DEPTH and stays delegations.len() — a fanned-out \
         lead shows 0, which is it telling the TRUTH (DR-046 §Consequences (c)): {lead_row:#?}"
    );

    // Side 1, POSITIVE CONTROL — without this, side 1 is a tautology. The
    // fixture carries no `permit.delegated` at all, so "zero" above would hold
    // even if the reducer had stopped folding delegations entirely, and it
    // cannot fail unless the reducer starts INVENTING records.
    //
    // So: replay the same log with ONE lead-keyed `permit.delegated` appended —
    // the exact fact DR-046 §Decision 4 withdrew — and demand that both
    // observables MOVE. That proves the zero above is a fact about the LOG (the
    // emitter is silent) rather than about a dead code path, and it pins the
    // detector that would catch the withdrawn emit if it came back on a log.
    //
    // The EMITTER side of the withdrawal cannot be checked by folding any static
    // fixture — see `bins/rezidentd/tests/permit_delegated_is_attenuation_only.rs`
    // for the host-runnable source guard, and `fan_out_live_e2e.rs` (WSL) for the
    // runtime one.
    let mut with_restored_emit = events.clone();
    with_restored_emit.push(
        Event::new(
            SourceId::new("rezidnt-run"),
            None,
            Subject::new("permit.delegated"),
            Ulid::new(),
            None,
            1,
            json!({
                "run": LEAD_RUN,
                "parent_badge_id": "1eadbadge0000001",
                "child_badge_id": "5uba0000000000a1",
                "added_caveats": [],
            }),
        )
        .expect("test event under 32KiB"),
    );
    let contaminated = fold(with_restored_emit.iter());
    let contaminated_lead = contaminated
        .agent_runs
        .get(LEAD_RUN)
        .expect("the lead folds on the contaminated log too");
    assert_eq!(
        contaminated_lead.delegations.len(),
        1,
        "POSITIVE CONTROL: a lead-keyed permit.delegated on the log DOES fold onto the lead's \
         attenuation chain. If this is 0, the assertion above is vacuous and proves nothing \
         about the withdrawal: {:#?}",
        contaminated_lead.delegations
    );
    let contaminated_row = rezidnt_state::project(&contaminated)
        .runs
        .iter()
        .find(|r| r.run == LEAD_RUN)
        .cloned()
        .expect("the lead has a board row on the contaminated log");
    assert_eq!(
        contaminated_row.delegated, 1,
        "POSITIVE CONTROL: BoardRow.delegated tracks that same chain, so the 0 asserted above is \
         a live observable — this is the number that read 2 before the withdrawal, on the \
         dossier `debrief` reads (DR-046 §Risk register): {contaminated_row:#?}"
    );

    // Side 2 — and the graph still folds the fan-out. Zero delegations must NOT
    // mean zero subs: that is precisely the silent-wrong this guard exists for.
    let view = orchestration_graph(&graph);
    let lead = view
        .leads
        .iter()
        .find(|l| l.lead_run == LEAD_RUN)
        .unwrap_or_else(|| {
            panic!(
                "a lead with ZERO delegations still surfaces its fan-out — an early-return on \
                 `delegations.is_empty()` reports fan_out: 0 for every real lead while every \
                 other test still passes (DR-046 §Decision 5): {view:#?}"
            )
        });
    assert_eq!(
        lead.fan_out, 2,
        "fan-out width rides lead_run, a different axis from the attenuation chain: {lead:#?}"
    );
}
