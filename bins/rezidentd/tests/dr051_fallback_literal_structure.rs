//! TRIALS-SLICE-B ENTRY ORACLE — criterion (c)'s HOST-VISIBLE backstop. The
//! real judge is `dr051_fallback_completion_fidelity_e2e.rs` (unix): it runs a
//! dying harness through a live daemon and compares the published fallback
//! `agent.completed` against `Completion::into_fact`'s failure rendering,
//! cross-crate. That judge is `#[cfg(unix)]` and invisible to host `/vet`, so
//! this file pins the one clause of the criterion that is judgeable as source
//! text, host-side (the `registry_convergence_structure.rs` pattern).
//!
//! THE CLAUSE: the fallback fires only when the child died without publishing
//! a completion — a run whose harness reported NO token accounting. The
//! ontology's `agent.completed` v1 cost bullet ratifies "a failed candidate's
//! cost is ABSENT, not zero", so the fallback literal may carry NO token keys
//! at all: not zeros (a present claim of a measurement that never happened),
//! and not any other value (the fallback has nothing measured to report).
//!
//! ## Disclosure (what this guard is, and is not)
//!
//! A SOURCE-TEXT guard over the `completed_id.is_none()` arm of `drive_run` in
//! `bins/rezidentd/src/runs.rs`: the window from that anchor must not mention
//! the token keys. A fallback payload assembled outside the window (a helper,
//! another crate) passes this guard — correctly, because the e2e judge then
//! owns the verdict on what that helper renders. The window width is disclosed
//! in the helper below, not hidden.
//!
//! ## RED MODE (stated plainly)
//!
//! ASSERT-RED today: the fallback literal hardcodes
//! `"cost": {"total_usd": 0.0, "input_tokens": 0, "output_tokens": 0}`.

use std::path::PathBuf;

fn runs_rs() -> String {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("src/runs.rs");
    std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()))
}

/// The fallback arm: a fixed-width window (1400 chars — the whole literal plus
/// its publish today measures well under that) starting at the
/// `completed_id.is_none()` firing condition, whitespace-stripped so macro
/// formatting cannot dodge the match.
fn fallback_window(source: &str) -> String {
    let anchor = source.find("completed_id.is_none()").expect(
        "runs.rs no longer keys the fallback on `completed_id.is_none()` — the arm moved; \
         re-anchor this oracle against the new firing condition",
    );
    source[anchor..]
        .chars()
        .take(1400)
        .filter(|c| !c.is_whitespace())
        .collect()
}

/// The fallback literal claims no token measurement: a run that died without a
/// result line was never measured, and the token keys are omitted, never
/// zeroed (ontology `agent.completed` v1 cost bullet; DR-051 §Decision 4).
///
/// ASSERT-RED today: both keys are hardcoded as `0` in the literal.
#[test]
fn fallback_literal_claims_no_token_measurement() {
    let window = fallback_window(&runs_rs());
    for key in ["\"input_tokens\"", "\"output_tokens\""] {
        assert!(
            !window.contains(key),
            "the daemon's fallback `agent.completed` literal still mentions {key} — a run \
             that died without reporting usage was NEVER measured, so the token keys must \
             be ABSENT from the fallback's cost, not emitted as zeros: `0` says \"measured, \
             and it was nothing\" (ontology `agent.completed` v1 cost bullet, ratified \
             \"absent, not zero\"; DR-050 §Decision 2(c) as sharpened by DR-051 §Decision \
             4). The behavioral judge is dr051_fallback_completion_fidelity_e2e.rs (unix)."
        );
    }
}
