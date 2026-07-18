//! SP2 hook sub-slice oracle — CRITERION 2 (enforcement-mode fold; ontology
//! DR-014 set). The `agent.spawned` reducer arm folds `pep: "enforced"` onto the
//! run's `AgentRunState`, and an `agent.spawned` WITHOUT `pep` folds to the
//! edge-gated-only / absent state — NEVER synthesized to a truthy value
//! (DR-012 declared-vs-absent discipline; ontology `agent.spawned.pep?` line
//! 194: "Absence is the honest representation of 'no PEP wired' — never
//! synthesized to a `false` / `"unenforced"` value").
//!
//! Shape asserted verbatim from `spec/ontology.md` `agent.spawned` v1 baseline:
//! `pep?: "enforced"` — present iff the PEP was wired at spawn; absent = edge-
//! gated-only.
//!
//! RED MODE: **compile-red then behavior-red**. `AgentRunState` today has NO
//! enforcement-mode field (crates/rezidnt-state/src/lib.rs, lines ~200-243), and
//! the `agent.spawned` reducer arm (~line 329) folds `run → status = "spawning"`
//! ONLY — it does not read `pep`. This board references the field the warden's
//! DR-014 note names as implementer work; it cannot compile until that field
//! lands, and the fold assertions fail until the arm reads `pep`.
//!
//! NOTE FOR THE IMPLEMENTER (field name negotiable, semantics are not): the
//! DR-014 ontology note suggests `pep_enforced: bool` OR
//! `enforcement_mode: Option<String>`, `#[serde(default)]`. This board reads the
//! mode through a small accessor `AgentRunState::pep_enforced()` returning
//! `bool` (true iff `pep == "enforced"` folded) so the test does not couple to
//! the storage choice. If you store it differently, keep that accessor (or
//! rename here to match) — the LOAD-BEARING pins are: enforced folds true;
//! ABSENT folds false (never synthesized); the field is `#[serde(default)]` so a
//! pre-DR-014 golden folds equal (I3).

use rezidnt_state::{Materializer, fold};
use rezidnt_types::{Event, SourceId, Subject};
use serde_json::{Value, json};
use ulid::Ulid;

const ENFORCED_RUN: &str = "01SP2PEPENFORCEDRUN000000R1";
const EDGE_RUN: &str = "01SP2PEPEDGEGATEDRUN00000R2";

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

/// CRITERION 2 (positive leg) — an `agent.spawned` carrying `pep: "enforced"`
/// folds to a mid-run-enforced run state. The reducer surfaces the field so
/// `gate_explain` can read it (DR-006 read-for-a-field: the reducer MUST surface
/// the field; ontology DR-014 set, "Consumer obligation").
///
/// COMPILE-RED until `AgentRunState` gains the enforcement-mode field + accessor.
#[test]
fn agent_spawned_with_pep_enforced_folds_mid_run_enforced() {
    let events = [ev(
        "agent.spawned",
        json!({
            "run": ENFORCED_RUN,
            "agent": "impl",
            "harness": "claude-code",
            "badge_id": "beadf00d",
            "pep": "enforced",
        }),
    )];
    let graph = fold(events.iter());
    let run = graph
        .agent_runs
        .get(ENFORCED_RUN)
        .expect("the spawn folds a run entry");
    assert_eq!(
        run.status, "spawning",
        "the existing status fold is not regressed by the additive pep read"
    );
    assert!(
        run.pep_enforced(),
        "an agent.spawned carrying pep=\"enforced\" folds to a mid-run-PEP-enforced \
         run so gate_explain can distinguish it (DR-014 §Decision 5; I4)"
    );
}

/// CRITERION 2 (the honesty leg — load-bearing) — an `agent.spawned` WITHOUT a
/// `pep` field folds to edge-gated-only: the enforcement mode is ABSENT, NEVER
/// synthesized to a truthy value. Absence is the honest "no PEP wired" (DR-012;
/// ontology `agent.spawned.pep?`).
///
/// COMPILE-RED until the field + accessor exist; then this is the assertion that
/// makes a synthesized-enforcement regression a test failure.
#[test]
fn agent_spawned_without_pep_folds_edge_gated_never_synthesized() {
    let events = [ev(
        "agent.spawned",
        json!({
            "run": EDGE_RUN,
            "agent": "impl",
            "harness": "claude-code",
            "badge_id": "cafebabe",
        }),
    )];
    let graph = fold(events.iter());
    let run = graph
        .agent_runs
        .get(EDGE_RUN)
        .expect("the spawn folds a run entry");
    assert!(
        !run.pep_enforced(),
        "an agent.spawned WITHOUT pep folds to edge-gated-only — absence is honest, \
         NEVER synthesized to a truthy value (DR-012; ontology agent.spawned.pep?)"
    );
}

/// CRITERION 2 (rebuild-stability, I3, release-blocking) — a PRE-DR-014 golden
/// fixture (an `agent.spawned` with no `pep` field, exactly the S1 shape) still
/// folds equal under both fold-from-zero and incremental application. The new
/// field being `#[serde(default)]` is what keeps `rezidnt rebuild` reproducing
/// identical graph state across the schema addition (ontology DR-014 set,
/// "`#[serde(default)]` so pre-DR-014 golden fixtures parse and compare equal
/// unchanged, I3 rebuild-stability").
///
/// COMPILE-RED until the field lands; the equality itself is the I3 pin.
#[test]
fn pre_dr014_spawn_folds_rebuild_stable() {
    // The S1-shape spawn fact: no `pep` key at all. This is what every golden
    // fixture committed before DR-014 looks like.
    let events = [
        ev(
            "agent.spawned",
            json!({"run": EDGE_RUN, "agent": "impl", "harness": "claude-code", "badge_id": "0"}),
        ),
        ev(
            "agent.status.changed",
            json!({"run": EDGE_RUN, "from": "spawning", "to": "running"}),
        ),
    ];

    let folded = fold(events.iter());
    let mut live = Materializer::new();
    for e in &events {
        live.apply(e);
    }
    assert_eq!(
        live.snapshot(),
        folded,
        "incremental == fold-from-zero across the pep field addition — rebuild is \
         stable over a pre-DR-014 golden (I3, release blocker)"
    );
    // And the enforcement mode is honestly absent, not defaulted-to-true.
    assert!(
        !folded.agent_runs[EDGE_RUN].pep_enforced(),
        "a pre-DR-014 spawn (no pep) folds edge-gated-only, not enforced (I3 honesty)"
    );
}
