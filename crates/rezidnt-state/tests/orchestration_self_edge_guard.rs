//! DR-044 ORACLE (self-edge regression + I6 rollup honesty) — the guards named
//! in DR-044 §Consequences "Test/criterion honesty" (c) and the projection half
//! of (e). Pure logic over `rezidnt-state`: no daemon, no IO, no clock, so this
//! file is HOST-LINTABLE and runs in the host `/vet` gauntlet.
//!
//! ## The defect this pins (DR-044 §Context, and it is RED right now)
//!
//! `bins/rezidentd/src/runs.rs:985-1005` emits `permit.delegated` keyed
//! `run` = the run BEING SPAWNED, with `parent_badge_id` = that run's base badge
//! and `child_badge_id` = its role-attenuated badge (`:998`/`:999`).
//! `bins/rezidentd/src/runs.rs:929` then puts that SAME attenuated badge on
//! `agent.spawned.badge_id`. The reducer keys the delegation on payload `run`
//! (`crates/rezidnt-state/src/lib.rs:1055`), and `orchestration_graph` matches a
//! sub purely by `lead.delegations[].child_badge_id == sub.badge_id`
//! (`:1634-1640`) with NO `sub_run != lead_run` guard. So today a run that merely
//! DECLARED A ROLE and never fanned out projects as a lead of ITSELF with
//! `fan_out: 1`.
//!
//! DR-042's shipped read-side tests never caught this: they fold a hand-authored
//! fixture of three DISTINCT runs, a shape no shipped emitter produces.
//!
//! ## The contract (DR-044 §Decision 2a)
//!
//! `orchestration_graph` gains a `sub_run != lead_run` guard. A same-run
//! delegation is a DR-017 capability-chain fact on a different axis (a
//! parent to child caveat hop within one run), not an orchestration edge, and it
//! yields no lead row and no sub row. The guard is depth-agnostic and must NOT
//! suppress genuine cross-run fan-out, including when a real lead ALSO carries
//! its own role self-edge (which production emits for every role-declaring run).
//!
//! ## The I6 legs (DR-044 §Consequences (e), projection half)
//!
//! A worktree-conflicted task mints NO run (DR-044 §Decision 3), so it folds as
//! nothing: it is never a passed, failed, OR inconclusive sub, and it never
//! inflates `fan_out`. A genuinely inconclusive sub folds back `inconclusive`,
//! never coerced (`roll_up_verdicts`, `crates/rezidnt-state/src/lib.rs:1691`).
//! Both are asserted against a graph that ALSO carries the lead's own self-edge,
//! which is what makes them fail today rather than pass vacuously.
//!
//! ## Ontology posture
//!
//! Every event here is built in code from ALREADY-RATIFIED v1 subjects
//! (`agent.spawned`, `permit.delegated`, `gate.passed`, `gate.inconclusive`).
//! This file emits NO `worktree.allocated` and therefore has ZERO dependence on
//! the `worktree.allocated.allocator` value vocabulary the parallel warden
//! `/subject` session is widening (DR-044 §Decision 6). The warden's outcome
//! cannot invalidate anything in this file.

use rezidnt_state::{OrchestrationView, fold, orchestration_graph};
use rezidnt_types::{Event, SourceId, Subject};
use serde_json::{Value, json};
use ulid::Ulid;

// --- runs (26-char ULID text form) ------------------------------------------

/// A run that DECLARED A ROLE and never fanned out — production's self-edge case.
const SELF_RUN: &str = "01DR044SE1F00000000000RN01";
/// A genuine lead that fanned out to distinct sub runs.
const LEAD_RUN: &str = "01DR0441EAD00000000000RN01";
const SUB_A_RUN: &str = "01DR044SBA000000000000RN01";
const SUB_B_RUN: &str = "01DR044SBB000000000000RN01";

// --- badge ids (`hex(blake3(sig)[..8])`, 16 lowercase hex — DR-018 §(a)) ------

/// The self-edge run's BASE badge (the `parent_badge_id` production emits).
const SELF_BASE_BADGE: &str = "5e1fba5e00000001";
/// The self-edge run's ROLE-ATTENUATED badge. Production puts this on BOTH
/// `permit.delegated.child_badge_id` AND that same run's `agent.spawned.badge_id`
/// — the collision that fabricates the self-lead.
const SELF_CHILD_BADGE: &str = "5e1fc41d00000001";

/// The lead's base badge and its own role-attenuated child badge. A real lead is
/// itself a role-declaring run, so production emits a self-edge for it TOO,
/// alongside the genuine lead-parented fan-out edges (DR-044 §Decision 2b).
const LEAD_BASE_BADGE: &str = "1eadba5e00000001";
const LEAD_CHILD_BADGE: &str = "1eadc41d00000001";

const SUB_A_BADGE: &str = "5ba00000000000a1";
const SUB_B_BADGE: &str = "5bb00000000000b1";
/// The badge a worktree-CONFLICTED task would have run under. No
/// `agent.spawned` ever folds it, because no run was minted (DR-044 §Decision 3).
const CONFLICTED_BADGE: &str = "c0bf1c7ed0000001";

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

/// The two facts a ROLE-DECLARING spawn emits, in the order production emits
/// them: the DR-017 capability-chain `permit.delegated` (keyed on the spawning
/// run itself, `runs.rs:997`) then `agent.spawned` carrying the SAME attenuated
/// badge (`runs.rs:929`).
fn role_declaring_spawn(run: &str, base_badge: &str, child_badge: &str) -> Vec<Event> {
    vec![
        ev(
            "permit.delegated",
            json!({
                "run": run,
                "parent_badge_id": base_badge,
                "child_badge_id": child_badge,
                "added_caveats": [{"kind": "role", "role": "reviewer"}],
            }),
        ),
        ev(
            "agent.spawned",
            json!({
                "run": run,
                "agent": "impl",
                "harness": "claude-code",
                "badge_id": child_badge,
                "role": "reviewer",
            }),
        ),
    ]
}

/// A genuine LEAD-PARENTED fan-out edge (DR-044 §Decision 2b): keyed `run` = the
/// LEAD's run, `child_badge_id` = the badge the SUB actually runs under, which
/// equals the sub's own `agent.spawned.badge_id`.
fn lead_parented_edge(lead_run: &str, lead_badge: &str, sub_badge: &str) -> Event {
    ev(
        "permit.delegated",
        json!({
            "run": lead_run,
            "parent_badge_id": lead_badge,
            "child_badge_id": sub_badge,
            "added_caveats": [],
        }),
    )
}

/// A sub run's spawn, folding the badge the lead delegated to.
fn sub_spawn(run: &str, badge: &str) -> Event {
    ev(
        "agent.spawned",
        json!({
            "run": run,
            "agent": "sub",
            "harness": "claude-code",
            "badge_id": badge,
        }),
    )
}

fn gate(subject: &str, run: &str, gate: &str) -> Event {
    ev(subject, json!({"run": run, "gate": gate}))
}

fn project(events: &[Event]) -> OrchestrationView {
    orchestration_graph(&fold(events.iter()))
}

/// CRITERION (c), DR-044 §Consequences — THE headline regression. A run that
/// declared a role and NEVER fanned out produces ZERO lead rows. Its
/// `permit.delegated` is a DR-017 capability-chain fact (same run, parent to
/// child caveat hop), not an orchestration edge; reading it as one makes the
/// projection report a lead of itself.
///
/// RED TODAY: `orchestration_graph` has no `sub_run != lead_run` guard
/// (`crates/rezidnt-state/src/lib.rs:1634-1640`), so this folds to exactly one
/// `LeadRow { lead_run: SELF_RUN, fan_out: 1, subs: [SELF_RUN] }`.
#[test]
fn role_declaring_run_that_never_fanned_out_produces_no_lead_row() {
    let events = role_declaring_spawn(SELF_RUN, SELF_BASE_BADGE, SELF_CHILD_BADGE);
    let view = project(&events);

    assert!(
        view.leads.is_empty(),
        "a role-declaring run that never fanned out is NOT a lead — its permit.delegated is a \
         DR-017 capability-chain fact on the SAME run (parent->child caveat hop), and the \
         projection must guard `sub_run != lead_run` (DR-044 §Decision 2a). Got: {view:#?}"
    );
}

/// CRITERION (c), sharpened — no lead may EVER carry itself as one of its own
/// subs. Stated separately from the emptiness assertion above so the failure
/// output names the exact defect rather than just a count mismatch, and so the
/// invariant survives any future change to which leads surface at all.
///
/// RED TODAY: the single fabricated row has `lead_run == subs[0].sub_run`.
#[test]
fn no_lead_is_ever_its_own_sub() {
    let events = role_declaring_spawn(SELF_RUN, SELF_BASE_BADGE, SELF_CHILD_BADGE);
    let view = project(&events);

    for lead in &view.leads {
        assert!(
            !lead.subs.iter().any(|s| s.sub_run == lead.lead_run),
            "lead {} lists ITSELF as a sub — a same-run delegation is a capability-chain fact, \
             never an orchestration edge (DR-044 §Decision 2a): {lead:#?}",
            lead.lead_run
        );
    }
}

/// CRITERION (c), the guard must not over-fire — a GENUINE lead that ALSO
/// carries its own role self-edge (which production emits for every
/// role-declaring run, so every real lead has one) still projects its real
/// cross-run fan-out, and ONLY that. This is the test that distinguishes "add
/// the guard" from "delete the projection".
///
/// RED TODAY: the self-edge is counted, so `fan_out` folds to 3 (two real subs
/// plus the lead itself) and `subs` contains `LEAD_RUN`.
#[test]
fn a_real_lead_with_its_own_self_edge_counts_only_its_cross_run_subs() {
    let mut events = role_declaring_spawn(LEAD_RUN, LEAD_BASE_BADGE, LEAD_CHILD_BADGE);
    events.push(lead_parented_edge(LEAD_RUN, LEAD_CHILD_BADGE, SUB_A_BADGE));
    events.push(lead_parented_edge(LEAD_RUN, LEAD_CHILD_BADGE, SUB_B_BADGE));
    events.push(sub_spawn(SUB_A_RUN, SUB_A_BADGE));
    events.push(sub_spawn(SUB_B_RUN, SUB_B_BADGE));

    let view = project(&events);

    assert_eq!(
        view.leads.len(),
        1,
        "exactly one lead surfaces — the fan-out lead, not its subs and not a self-lead: {view:#?}"
    );
    let lead = &view.leads[0];
    assert_eq!(
        lead.lead_run, LEAD_RUN,
        "the row is keyed on the lead's run"
    );
    assert_eq!(
        lead.fan_out, 2,
        "fan_out counts the two CROSS-RUN subs only — the lead's own role self-edge is a \
         capability-chain fact and contributes nothing (DR-044 §Decision 2a): {lead:#?}"
    );
    let subs: Vec<&str> = lead.subs.iter().map(|s| s.sub_run.as_str()).collect();
    assert_eq!(
        subs,
        vec![SUB_A_RUN, SUB_B_RUN],
        "the subs are exactly the two delegated runs, in deterministic key order: {lead:#?}"
    );
}

/// CRITERION (e), projection half — a worktree-CONFLICTED task is never counted.
/// DR-044 §Decision 3: a double-claim returns an error and mints NO run, so
/// nothing folds; the task is not a passed sub, not a failed sub, and not an
/// inconclusive sub. The remaining subs stand. The lead's own self-edge is
/// present too, which is what makes this fail today instead of passing
/// vacuously.
///
/// RED TODAY: `fan_out` folds to 2 (the surviving sub plus the lead itself) and
/// the rollup gains a spurious `pending` bucket for the lead.
#[test]
fn a_conflicted_task_is_never_counted_as_passed_failed_or_inconclusive() {
    let mut events = role_declaring_spawn(LEAD_RUN, LEAD_BASE_BADGE, LEAD_CHILD_BADGE);
    // Two tasks were delegated; only one could allocate a worktree.
    events.push(lead_parented_edge(LEAD_RUN, LEAD_CHILD_BADGE, SUB_A_BADGE));
    events.push(lead_parented_edge(
        LEAD_RUN,
        LEAD_CHILD_BADGE,
        CONFLICTED_BADGE,
    ));
    // The surviving sub spawned and passed its gate. The conflicted task has NO
    // agent.spawned at all — there is no run to fold (DR-044 §Decision 3).
    events.push(sub_spawn(SUB_A_RUN, SUB_A_BADGE));
    events.push(gate("gate.passed", SUB_A_RUN, "vet"));

    let view = project(&events);

    assert_eq!(view.leads.len(), 1, "the lead still surfaces: {view:#?}");
    let lead = &view.leads[0];
    assert_eq!(
        lead.fan_out, 1,
        "the conflicted task minted no run, so it does not inflate fan_out; the surviving sub \
         stands (DR-044 §Decision 3): {lead:#?}"
    );

    let rollup = &lead.verdict_rollup;
    assert_eq!(
        (
            rollup.passed,
            rollup.failed,
            rollup.inconclusive,
            rollup.pending
        ),
        (1, 0, 0, 0),
        "the conflicted task is counted in NO bucket — not passed, not failed, not inconclusive, \
         and not pending (there is no run to be pending). Only the surviving passed sub is \
         tallied (I6, DR-044 §Consequences (e)): {lead:#?}"
    );
    assert_eq!(
        rollup.passed + rollup.failed + rollup.inconclusive + rollup.pending,
        lead.fan_out,
        "the rollup buckets sum to fan_out — conservation, with nothing invented for the \
         refused task: {lead:#?}"
    );
}

/// CRITERION (e), non-coercion half — a genuinely INCONCLUSIVE sub folds back
/// `inconclusive`, in its own bucket, never coerced to pass or fail
/// (`roll_up_verdicts`, `crates/rezidnt-state/src/lib.rs:1691`). Asserted over a
/// graph that also carries the lead's self-edge so the bucket arithmetic is
/// checked against the CORRECTED `fan_out`.
///
/// RED TODAY: the self-edge adds a third sub and a spurious `pending`, so the
/// tuple folds to `(1, 0, 1, 1)` with `fan_out == 3`.
#[test]
fn an_inconclusive_sub_folds_back_inconclusive_and_is_never_coerced() {
    let mut events = role_declaring_spawn(LEAD_RUN, LEAD_BASE_BADGE, LEAD_CHILD_BADGE);
    events.push(lead_parented_edge(LEAD_RUN, LEAD_CHILD_BADGE, SUB_A_BADGE));
    events.push(lead_parented_edge(LEAD_RUN, LEAD_CHILD_BADGE, SUB_B_BADGE));
    events.push(sub_spawn(SUB_A_RUN, SUB_A_BADGE));
    events.push(sub_spawn(SUB_B_RUN, SUB_B_BADGE));
    events.push(gate("gate.passed", SUB_A_RUN, "vet"));
    events.push(gate("gate.inconclusive", SUB_B_RUN, "pre_merge"));

    let view = project(&events);
    let lead = view
        .leads
        .first()
        .unwrap_or_else(|| panic!("the lead surfaces: {view:#?}"));

    assert_eq!(lead.fan_out, 2, "two cross-run subs: {lead:#?}");

    let sub_b = lead
        .subs
        .iter()
        .find(|s| s.sub_run == SUB_B_RUN)
        .unwrap_or_else(|| panic!("sub B surfaces: {lead:#?}"));
    assert_eq!(
        sub_b.verdicts,
        vec![("pre_merge".to_string(), "inconclusive".to_string())],
        "the inconclusive verdict surfaces VERBATIM, never rewritten (I6): {sub_b:#?}"
    );

    let rollup = &lead.verdict_rollup;
    assert_eq!(
        (
            rollup.passed,
            rollup.failed,
            rollup.inconclusive,
            rollup.pending
        ),
        (1, 0, 1, 0),
        "the inconclusive sub occupies its OWN bucket — never coerced up into passed, never \
         down into failed (I6): {lead:#?}"
    );
}
