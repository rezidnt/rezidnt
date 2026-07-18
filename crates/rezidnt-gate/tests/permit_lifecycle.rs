//! SP0 oracle — the `permit` gate lifecycle point + the verdict→decision
//! mapping (DR-008 / DR-009; design `docs/design/permit-engine.md` §4, §8, §11).
//!
//! RED MODE: **compile-red** (the honest "does not exist yet" failure). The
//! permit lifecycle point and its verdict→decision mapping are not built yet:
//! this file references `rezidnt_gate::permit` (the module the implementer adds
//! for the fourth lifecycle point), so the crate fails to compile until that
//! surface lands — exactly the SP0 §11 accept criterion "a permit-verifier
//! returns allow/deny/inconclusive, logged." Once the module exists these flip
//! to assert-red/green and pin the mapping.
//!
//! Why a module, not an enum arm: lifecycle points are modeled as STRINGS
//! today (`GateDef.name` is `"vet" | "pre_merge" | "post_run"` — a string, not
//! a closed enum; see `crates/rezidnt-gate/src/lib.rs`). SP0 therefore adds the
//! fourth point as (a) a canonical name constant `permit::LIFECYCLE_POINT` and
//! (b) the authorization mapping `permit::decision_for(Verdict)` that turns the
//! BINDING three-valued verdict into an authorization decision with ZERO new
//! vocabulary (design §4). The mapping is the load-bearing SP0 unit.
//!
//! Authority for every shape asserted here: `spec/ontology.md` "permit set"
//! (the verdict→subject table lines 144-146, 152-155) + design §4 table.

use rezidnt_gate::Verdict;
use rezidnt_gate::permit::{self, PermitDecision};

/// CRITERION 1 — **lifecycle point exists.** A `permit` gate lifecycle point
/// exists alongside `vet` / `pre_merge` / `post_run`. Because points are
/// string-modeled, the point is a canonical name constant the engine, the
/// project spec (`[gates.permit]`, design §6), and the ontology all agree on.
///
/// COMPILE-RED until `permit::LIFECYCLE_POINT` exists; then asserts the
/// canonical spelling matches the ontology / design `permit` point (design §4,
/// project-spec block `[gates.permit]`).
#[test]
fn permit_lifecycle_point_exists_and_is_named_permit() {
    assert_eq!(
        permit::LIFECYCLE_POINT,
        "permit",
        "the fourth lifecycle point is spelled `permit` (design §4; project spec `[gates.permit]`)"
    );
    // A GateDef on the permit point is a permit gate — the gate engine IS the
    // policy engine (design §4: no second engine). This pins that the point is
    // usable as a GateDef name, same as vet/pre_merge/post_run.
    let def = rezidnt_gate::GateDef {
        name: permit::LIFECYCLE_POINT.to_string(),
        ..Default::default()
    };
    assert_eq!(def.name, "permit");
}

/// CRITERION 2 — **verdict→decision mapping.** A permit-verifier's verdict
/// maps `pass → grant`, `fail → deny`, `inconclusive → escalate`. Table test
/// over ALL THREE verdicts (the mapping is total; every verdict has exactly one
/// decision — design §4 table, ontology lines 144-146).
///
/// COMPILE-RED until `permit::decision_for` + `PermitDecision` exist.
#[test]
fn verdict_maps_to_authorization_decision_over_all_three() {
    let table = [
        (Verdict::Pass, PermitDecision::Grant),
        (Verdict::Fail, PermitDecision::Deny),
        (Verdict::Inconclusive, PermitDecision::Escalate),
    ];
    for (verdict, expected) in table {
        assert_eq!(
            permit::decision_for(verdict),
            expected,
            "{verdict:?} must map to {expected:?} (design §4; ontology permit set)"
        );
    }
}

/// CRITERION 2 (fact-subject leg) — the decision is carried by the SUBJECT,
/// never a bar boolean (house pattern, same as `gate.passed`/`.failed`). Each
/// decision names the `permit.*` fact subject it is logged as, so the emit path
/// and the reducer key on the same string (ontology lines 152-155).
///
/// COMPILE-RED until `PermitDecision::subject` exists.
#[test]
fn each_decision_names_its_permit_fact_subject() {
    assert_eq!(PermitDecision::Grant.subject(), "permit.granted");
    assert_eq!(PermitDecision::Deny.subject(), "permit.denied");
    assert_eq!(PermitDecision::Escalate.subject(), "permit.escalated");
}

/// CRITERION 3 — **inconclusive is NEVER coerced (I6).** The load-bearing
/// honesty test: `inconclusive` maps to `Escalate` and is *never* silently
/// turned into `Grant` or `Deny`. Asserted both directions — inconclusive →
/// escalate, AND escalate is reachable from NO other verdict (so no verdict is
/// quietly funneled into allow/deny that should have escalated, and escalate is
/// not a synonym for grant/deny).
///
/// COMPILE-RED until the mapping exists; the whole point of the mapping is that
/// this cannot be weakened (I6, DR-008 §4).
#[test]
fn inconclusive_escalates_and_is_never_coerced_to_grant_or_deny() {
    let decision = permit::decision_for(Verdict::Inconclusive);
    assert_eq!(
        decision,
        PermitDecision::Escalate,
        "inconclusive → escalate-to-a-human, NEVER coerced (I6, DR-008 §4)"
    );
    assert_ne!(
        decision,
        PermitDecision::Grant,
        "inconclusive is NEVER silently granted — the honesty the product is built on"
    );
    assert_ne!(
        decision,
        PermitDecision::Deny,
        "inconclusive is NEVER silently denied — escalate is not a coercion to deny"
    );
    // Nothing else escalates: pass and fail have their own decisions, so
    // `escalate` is a distinct outcome reserved for can't-decide, never a
    // catch-all that hides a coerced grant/deny.
    assert_ne!(
        permit::decision_for(Verdict::Pass),
        PermitDecision::Escalate
    );
    assert_ne!(
        permit::decision_for(Verdict::Fail),
        PermitDecision::Escalate
    );
}

/// CRITERION 3 (property leg) — over EVERY verdict, the mapping is total and
/// `Escalate` is produced by `Inconclusive` alone. This is the "cannot be made
/// to coerce" invariant stated exhaustively rather than by example: iterate the
/// full closed verdict set and assert the escalate pre-image is exactly
/// `{Inconclusive}`.
///
/// COMPILE-RED until the mapping exists.
#[test]
fn escalate_preimage_is_exactly_inconclusive() {
    let all = [Verdict::Pass, Verdict::Fail, Verdict::Inconclusive];
    let escalating: Vec<Verdict> = all
        .into_iter()
        .filter(|&v| permit::decision_for(v) == PermitDecision::Escalate)
        .collect();
    assert_eq!(
        escalating,
        vec![Verdict::Inconclusive],
        "ONLY inconclusive escalates — no verdict is coerced past escalate into allow/deny (I6)"
    );
}
