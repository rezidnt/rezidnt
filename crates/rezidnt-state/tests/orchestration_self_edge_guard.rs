//! DR-044 §Consequences (c)/(e) + DR-046 §Decision 4/5 — the self-lead guards
//! and the I6 rollup honesty legs of `orchestration_graph`. Pure logic over
//! `rezidnt-state`: no daemon, no IO, no clock, so this file is HOST-LINTABLE
//! and runs in the host `/vet` gauntlet.
//!
//! ## What this file guarded, and what it guards now (RE-CUT 2026-07-24)
//!
//! **Lane 1 (DR-044 §Decision 2a).** The lead→sub edge was
//! `lead.delegations[].child_badge_id == sub.badge_id`. A role-declaring spawn
//! emits a DR-017 `permit.delegated` keyed on the run BEING SPAWNED, whose
//! `child_badge_id` is the same attenuated badge that run's own
//! `agent.spawned.badge_id` carries — so a badge-only match read every
//! role-declaring run as a lead of ITSELF with `fan_out: 1`. The fix was a
//! `sub_run != lead_run` guard, and that guard was LOAD-BEARING: it was the only
//! thing separating a legitimate capability-chain fact from an orchestration
//! edge, on an axis where the two genuinely collided.
//!
//! **Now (DR-046 §Decision 4/5).** The lead-parented `permit.delegated` emit is
//! WITHDRAWN — a fan-out attenuates nothing, so that subject could not carry the
//! edge without asserting a narrowing that never happened — and the edge moved
//! to `agent.spawned.lead_run?` on the SUB's own spawn fact. The DR-017 self-edge
//! never touches that field, so **the self-lead class cannot arise on this axis
//! at all**. The guard survives as BELT-AND-BRACES against a malformed log (the
//! ontology makes `lead_run != run` BINDING on the emitter), and the assertions
//! below are re-pointed accordingly:
//!
//! - the role-declaring run still produces no lead row — but now because nothing
//!   names it, not because a guard filtered a colliding fact. Its `delegations`
//!   is NON-EMPTY, which is what makes it a live regression test against any
//!   return to a `delegations`-driven projection;
//! - the malformed self-lead (`lead_run == run`, which NO emitter produces) is
//!   asserted directly, since that is what the surviving guard is now for;
//! - a ROLELESS lead — zero delegations, real subs — pins DR-046 §Decision 5's
//!   named trap: an early-return on `delegations.is_empty()` reports `fan_out: 0`
//!   for every genuine lead while every other test still compiles and passes.
//!
//! ## The I6 legs (DR-044 §Consequences (e), projection half)
//!
//! A worktree-conflicted task mints NO run (DR-044 §Decision 3), so it folds as
//! nothing: it is never a passed, failed, OR inconclusive sub, and it never
//! inflates `fan_out`. A genuinely inconclusive sub folds back `inconclusive`,
//! never coerced (`roll_up_verdicts`).
//!
//! ## Ontology posture
//!
//! Every event here is built in code from ALREADY-RATIFIED v1 subjects
//! (`agent.spawned`, `permit.delegated`, `gate.passed`, `gate.inconclusive`) and
//! ratified v1 fields, including the `agent.spawned.lead_run?` this arc minted.
//! This file emits NO `worktree.allocated` and therefore has ZERO dependence on
//! the `worktree.allocated.allocator` value vocabulary.

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
/// A ROLELESS lead: it folds ZERO delegations, the shape every fanned-out lead
/// now has (DR-046 §Decision 4).
const BARE_LEAD_RUN: &str = "01DR046BAR000000000000RN01";
/// A run whose spawn fact names ITSELF as its lead. NO emitter can produce this
/// (the ontology's `lead_run != run` constraint is BINDING on the emitter); it
/// exists only to exercise the projection's belt-and-braces guard.
const MALFORMED_RUN: &str = "01DR046MAL000000000000RN01";

// --- badge ids (`hex(blake3(sig)[..8])`, 16 lowercase hex — DR-018 §(a)) ------

/// The self-edge run's BASE badge (the `parent_badge_id` production emits).
const SELF_BASE_BADGE: &str = "5e1fba5e00000001";
/// The self-edge run's ROLE-ATTENUATED badge. Production puts this on BOTH
/// `permit.delegated.child_badge_id` AND that same run's `agent.spawned.badge_id`
/// — the collision that fabricated the self-lead under the badge-keyed edge.
const SELF_CHILD_BADGE: &str = "5e1fc41d00000001";

/// The lead's base badge and its own role-attenuated child badge. A
/// role-declaring lead still emits its DR-017 self-edge (that path is untouched
/// by DR-046 §Decision 6), so a real lead can carry a non-empty `delegations`.
const LEAD_BASE_BADGE: &str = "1eadba5e00000001";
const LEAD_CHILD_BADGE: &str = "1eadc41d00000001";

const BARE_LEAD_BADGE: &str = "1eadbare00000001";
const SUB_A_BADGE: &str = "5ba00000000000a1";
const SUB_B_BADGE: &str = "5bb00000000000b1";

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
/// run itself, `bins/rezidentd/src/runs.rs:1032-1052`) then `agent.spawned`
/// carrying the SAME attenuated badge. This path is UNTOUCHED by DR-046 — it is
/// a real attenuation and remains correct.
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

/// A fan-out SUB's spawn: it names its LEAD on its own fact — the DR-046
/// §Decision 5 edge. Bare ULID, not scheme-tagged.
fn sub_spawn(run: &str, badge: &str, lead_run: &str) -> Event {
    ev(
        "agent.spawned",
        json!({
            "run": run,
            "agent": "sub",
            "harness": "claude-code",
            "badge_id": badge,
            "lead_run": lead_run,
        }),
    )
}

/// An ORDINARY spawn: no lead, and absence is the honest representation.
fn plain_spawn(run: &str, badge: &str) -> Event {
    ev(
        "agent.spawned",
        json!({"run": run, "agent": "lead", "harness": "claude-code", "badge_id": badge}),
    )
}

fn gate(subject: &str, run: &str, gate: &str) -> Event {
    ev(subject, json!({"run": run, "gate": gate}))
}

fn project(events: &[Event]) -> OrchestrationView {
    orchestration_graph(&fold(events.iter()))
}

/// CRITERION (c), DR-044 §Consequences — the headline regression, re-pointed by
/// DR-046. A run that declared a role and NEVER fanned out produces ZERO lead
/// rows. Its `permit.delegated` is a DR-017 capability-chain fact (same run,
/// parent→child caveat hop); reading it as orchestration is what made the
/// projection report a lead of itself.
///
/// This run's `delegations` is NON-EMPTY and its `lead_run` is absent — the exact
/// inverse of a genuine lead after DR-046 — so it stays a live regression test
/// against any return to a `delegations`-driven projection.
#[test]
fn role_declaring_run_that_never_fanned_out_produces_no_lead_row() {
    let events = role_declaring_spawn(SELF_RUN, SELF_BASE_BADGE, SELF_CHILD_BADGE);

    // Precondition, asserted rather than assumed: the attenuation DID fold.
    let graph = fold(events.iter());
    let state = graph.agent_runs.get(SELF_RUN).expect("the run folds");
    assert_eq!(
        state.delegations.len(),
        1,
        "precondition: the DR-017 attenuation folds onto this run — it is a REAL narrowing and \
         DR-046 leaves it in place: {state:#?}"
    );
    assert_eq!(
        state.lead_run, None,
        "precondition: a role attenuation names NO lead — the two axes never touch"
    );

    let view = orchestration_graph(&graph);
    assert!(
        view.leads.is_empty(),
        "a role-declaring run that never fanned out is NOT a lead — its permit.delegated is a \
         DR-017 attenuation on the SAME run, invisible to an edge keyed on \
         agent.spawned.lead_run (DR-046 §Decision 5). Got: {view:#?}"
    );
}

/// CRITERION (c), sharpened — no lead may EVER carry itself as one of its own
/// subs, INCLUDING when a malformed log asserts exactly that. `lead_run == run`
/// is forbidden to emitters by the ontology (BINDING) and no shipped emitter can
/// produce it, so this is the projection's belt-and-braces guard under test:
/// a bad fact is DROPPED, never rendered as a self-lead row.
#[test]
fn no_lead_is_ever_its_own_sub() {
    // Leg 1: the DR-017 role case (cannot collide on this axis at all).
    let role_case = role_declaring_spawn(SELF_RUN, SELF_BASE_BADGE, SELF_CHILD_BADGE);
    // Leg 2: a MALFORMED spawn naming itself as its own lead.
    let malformed = vec![sub_spawn(MALFORMED_RUN, SUB_A_BADGE, MALFORMED_RUN)];

    for events in [role_case, malformed] {
        let view = project(&events);
        for lead in &view.leads {
            assert!(
                !lead.subs.iter().any(|s| s.sub_run == lead.lead_run),
                "lead {} lists ITSELF as a sub — a run is never its own lead (ontology, BINDING \
                 on the emitter; the projection guards it anyway): {lead:#?}",
                lead.lead_run
            );
        }
    }

    // And the malformed fact yields no row at all, rather than a zero-wide one.
    let view = project(&[sub_spawn(MALFORMED_RUN, SUB_A_BADGE, MALFORMED_RUN)]);
    assert!(
        view.leads.is_empty(),
        "a self-lead fact is DROPPED, not rendered: {view:#?}"
    );
}

/// DR-046 §Decision 5's NAMED TRAP — a ROLELESS lead. It declares no role, so it
/// emits no DR-017 self-edge and folds ZERO delegations; this is the shape every
/// fanned-out lead now has after the §Decision 4 withdrawal. A projection that
/// kept its `if lead.delegations.is_empty() { return None }` early-return reports
/// `fan_out: 0` here — for EVERY real lead — while still compiling and while
/// every badge-era assertion still passes. That is why this leg exists.
#[test]
fn a_roleless_lead_with_zero_delegations_still_projects_its_fan_out() {
    let events = [
        plain_spawn(BARE_LEAD_RUN, BARE_LEAD_BADGE),
        sub_spawn(SUB_A_RUN, SUB_A_BADGE, BARE_LEAD_RUN),
        sub_spawn(SUB_B_RUN, SUB_B_BADGE, BARE_LEAD_RUN),
    ];

    // Precondition, asserted: the lead really does fold ZERO attenuations.
    let graph = fold(events.iter());
    let state = graph.agent_runs.get(BARE_LEAD_RUN).expect("the lead folds");
    assert!(
        state.delegations.is_empty(),
        "precondition: a fan-out is not an attenuation, so a roleless lead folds ZERO \
         DelegationRecords (DR-046 §Decision 4): {state:#?}"
    );

    let view = orchestration_graph(&graph);
    let lead = view
        .leads
        .iter()
        .find(|l| l.lead_run == BARE_LEAD_RUN)
        .unwrap_or_else(|| {
            panic!(
                "a lead with ZERO delegations MUST still surface its fan-out — gating the \
                 projection on `delegations` reports fan_out: 0 for every genuine lead \
                 (DR-046 §Decision 5, the named trap): {view:#?}"
            )
        });
    assert_eq!(lead.fan_out, 2, "both subs name this lead: {lead:#?}");
}

/// CRITERION (c), the guard must not over-fire — a GENUINE lead that ALSO
/// declares a role (so it carries its own DR-017 attenuation and a non-empty
/// `delegations`) still projects its real cross-run fan-out, and ONLY that. This
/// is the test that distinguishes "read the right axis" from "delete the
/// projection": the attenuation must contribute NOTHING, in either direction.
#[test]
fn a_real_lead_with_its_own_self_edge_counts_only_its_cross_run_subs() {
    let mut events = role_declaring_spawn(LEAD_RUN, LEAD_BASE_BADGE, LEAD_CHILD_BADGE);
    events.push(sub_spawn(SUB_A_RUN, SUB_A_BADGE, LEAD_RUN));
    events.push(sub_spawn(SUB_B_RUN, SUB_B_BADGE, LEAD_RUN));

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
        "fan_out counts the two subs that NAME this lead — its own role attenuation is a \
         capability-chain fact and contributes nothing (DR-046 §Decision 4/5): {lead:#?}"
    );
    let subs: Vec<&str> = lead.subs.iter().map(|s| s.sub_run.as_str()).collect();
    assert_eq!(
        subs,
        vec![SUB_A_RUN, SUB_B_RUN],
        "the subs are exactly the two runs naming this lead, in deterministic key order: {lead:#?}"
    );
}

/// CRITERION (e), projection half — a worktree-CONFLICTED task is never counted.
/// DR-044 §Decision 3: a double-claim returns an error and mints NO run, so
/// NOTHING folds for it — no spawn, therefore no `lead_run`, therefore no edge.
/// The task is not a passed sub, not a failed sub, and not an inconclusive sub.
/// The remaining subs stand. The lead's own role attenuation is present too, so
/// this cannot pass vacuously through an empty graph.
#[test]
fn a_conflicted_task_is_never_counted_as_passed_failed_or_inconclusive() {
    let mut events = role_declaring_spawn(LEAD_RUN, LEAD_BASE_BADGE, LEAD_CHILD_BADGE);
    // Two tasks were delegated; only one could allocate a worktree. The surviving
    // sub spawned and passed its gate. The conflicted task has NO agent.spawned
    // at all — there is no run to fold, and so no fact naming the lead.
    events.push(sub_spawn(SUB_A_RUN, SUB_A_BADGE, LEAD_RUN));
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
/// (`roll_up_verdicts`). Asserted over a graph that also carries the lead's own
/// role attenuation so the bucket arithmetic is checked against a `fan_out` that
/// the attenuation must not have touched.
#[test]
fn an_inconclusive_sub_folds_back_inconclusive_and_is_never_coerced() {
    let mut events = role_declaring_spawn(LEAD_RUN, LEAD_BASE_BADGE, LEAD_CHILD_BADGE);
    events.push(sub_spawn(SUB_A_RUN, SUB_A_BADGE, LEAD_RUN));
    events.push(sub_spawn(SUB_B_RUN, SUB_B_BADGE, LEAD_RUN));
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
