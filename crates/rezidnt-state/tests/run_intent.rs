//! SP-intent oracle scaffold — the run-intent reducer fold (DR-010). The
//! `run.intent.declared` subject folds into a per-run intent state
//! (`AgentRunState::intent`), keyed by the payload `run`.
//!
//! GREEN MODE (honest, and correct): the reducer was landed by the warden's
//! `/subject` scaffolding pass, so these fold tests PASS NOW. That is the
//! intent — they LOCK the warden's scaffolding (the fold half of "no
//! consumer-less subject", DR-006 precedent) so a later edit that breaks the
//! intent fold, drops `intent_ref`, mis-reads `allowed_tools`, or panics on a
//! keyless fact turns them red. The SP-intent tests that must FAIL pending
//! implementation are the gate-engine ones (the `intent-lock` native
//! permit-verifier: in-set → allow, off-task → escalate, intent-absent →
//! escalate) — the NEXT slice, oracle-first, NOT written here. This file pins
//! the consumer that verifier reads.
//!
//! Shape asserted verbatim from `spec/ontology.md` "run-intent set" (payload
//! schema): `run.intent.declared {run, intent_ref: CasRef, allowed_tools:
//! [string]}`.

use rezidnt_state::{Materializer, fold};
use rezidnt_types::{Event, SourceId, Subject};
use serde_json::{Value, json};
use ulid::Ulid;

const RUN: &str = "01SPINTENTRUN0000000000000R1";
const INTENT_HASH: &str = "in7en700000000000000000000000000000000000000000000000000000001";

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

/// DR-010 §4 — `run.intent.declared` folds to a rebuild-stable per-run intent
/// state: the declared `allowed_tools` (the intent-derived least-privilege set
/// the verifier enforces) and the `intent_ref` hash (the CAS ref of the intent
/// text) land on `AgentRunState::intent`, keyed by `run`. A permit fact needs
/// no prior spawn — the log is truth (I3).
#[test]
fn intent_declared_folds_to_per_run_intent_state() {
    let events = [ev(
        "run.intent.declared",
        json!({
            "run": RUN,
            "intent_ref": {"hash": INTENT_HASH, "bytes": 128, "mime": "text/plain"},
            "allowed_tools": ["Read", "Grep", "Glob"],
        }),
    )];
    let graph = fold(events.iter());
    let run = graph
        .agent_runs
        .get(RUN)
        .expect("a run.intent.declared fact creates the run entry — no spawn required (I3)");
    let intent = run
        .intent
        .as_ref()
        .expect("the intent state folds onto the run");
    assert_eq!(
        intent.allowed_tools,
        vec!["Read", "Grep", "Glob"],
        "the declared least-privilege tool set folds verbatim (the enforced set; DR-010 §4)"
    );
    assert_eq!(
        intent.intent_ref.as_deref(),
        Some(INTENT_HASH),
        "the intent CAS ref folds so gate_explain can name what the run was for (I6)"
    );
}

/// I3 — a `run.intent.declared` fact missing `run` folds as counters-only: the
/// reducer never guesses a key, never chokes (the permit-reducer discipline).
#[test]
fn keyless_intent_fact_folds_counters_only() {
    let events = [ev(
        "run.intent.declared",
        json!({
            "intent_ref": {"hash": INTENT_HASH, "bytes": 1, "mime": "text/plain"},
            "allowed_tools": ["Bash"],
        }),
    )];
    let graph = fold(events.iter());
    assert_eq!(graph.events_folded, 1, "the fact is still counted");
    assert!(
        graph.agent_runs.is_empty(),
        "a keyless run.intent.declared fact mints no run entry (I3)"
    );
}

/// I3 honesty — a malformed `allowed_tools` (absent / not an array) folds to an
/// empty set, never a panic; the intent entry is still created so absence is
/// honest, not a crash.
#[test]
fn malformed_allowed_tools_folds_empty_never_panics() {
    let events = [ev(
        "run.intent.declared",
        json!({"run": RUN, "intent_ref": {"hash": INTENT_HASH, "bytes": 1, "mime": "text/plain"}}),
    )];
    let graph = fold(events.iter());
    let intent = graph.agent_runs[RUN]
        .intent
        .as_ref()
        .expect("the entry is created even with no tools declared");
    assert!(
        intent.allowed_tools.is_empty(),
        "absent allowed_tools folds empty, never panics (I3)"
    );
}

/// Rebuild family (release-blocking) — incremental Materializer application
/// equals fold-from-zero for the intent fold, so `rezidnt rebuild` reproduces
/// the intent state. `fold(log) == snapshot`.
#[test]
fn intent_fold_incremental_equals_fold_from_zero() {
    let events = [
        ev("agent.spawned", json!({"run": RUN})),
        ev(
            "run.intent.declared",
            json!({
                "run": RUN,
                "intent_ref": {"hash": INTENT_HASH, "bytes": 64, "mime": "text/plain"},
                "allowed_tools": ["Read", "Edit"],
            }),
        ),
    ];
    let folded = fold(events.iter());

    let mut live = Materializer::new();
    for event in &events {
        live.apply(event);
    }
    assert_eq!(
        live.snapshot(),
        folded,
        "incremental == fold-from-zero (rebuild reproduces the intent state)"
    );
}
