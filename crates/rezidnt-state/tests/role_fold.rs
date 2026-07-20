//! SP4a oracle — CRITERION 3 (fold, I3). The `agent.spawned` reducer arm folds
//! `role: "reviewer"` onto the run's `AgentRunState.role` (a new
//! `#[serde(default)] pub role: Option<String>`); an `agent.spawned` WITHOUT
//! `role` folds to `None` — ABSENT, never synthesized to a default (DR-016
//! §Decision 2; DR-012 declared-vs-absent; ontology `agent.spawned.role?` line
//! 195). A pre-DR-016 golden (no `role`) folds rebuild-stable (incremental ==
//! fold-from-zero), the `#[serde(default)]` keeping every earlier fixture
//! parsing and comparing equal (I3 rebuild-stability, release-blocking).
//!
//! Shape asserted verbatim from `spec/ontology.md` `agent.spawned` v1 baseline:
//! `role?: string` — present iff `AgentSpec.role` declared one; absent = no role.
//! This board reads the folded role through `AgentRunState.role`
//! (`Option<String>`) — the exact `Option<String>` storage the pep field uses
//! (`AgentRunState.pep`, crates/rezidnt-state/src/lib.rs:253); if the
//! implementer stores it differently, keep a `role` accessor of the same shape.
//!
//! API SHAPE THE IMPLEMENTER MUST MATCH:
//!   - `AgentRunState { .., #[serde(default)] pub role: Option<String> }`.
//!   - in `apply`, the `"agent.spawned"` arm folds
//!     `if let Some(role) = event.payload()["role"].as_str() { state.role =
//!     Some(role.into()); }` — VERBATIM, absent stays `None` (mirror the `pep`
//!     fold at crates/rezidnt-state/src/lib.rs:358-360).
//!
//! RED MODE — COMPILE-RED then behavior-red: `AgentRunState` has no `role`
//! field today, and the `agent.spawned` arm does not read `role`. This file
//! cannot compile until the field lands; the fold assertions decide green after.
//!
//! The golden fixtures (`spec/fixtures/sp4a_role_reviewer.jsonl`,
//! `spec/fixtures/sp4a_role_absent.jsonl`) are committed, minimal, and named for
//! the behavior they pin (testing-oracles fixture hygiene).

use std::path::PathBuf;

use rezidnt_state::{Materializer, fold};
use rezidnt_types::Event;

const REVIEWER_RUN: &str = "01SP4AR0LEREVIEWER00RN0001";
const ABSENT_RUN: &str = "01SP4AR0LEABSENT0000RN0001";

fn fixtures_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../spec/fixtures")
}

/// Load a committed golden fixture as its event vector (the fixture IS the log —
/// I3). Plain serde parse (isolates the reducer, not the wire codec — mirrors
/// `fixture_replay.rs`).
fn load_fixture(name: &str) -> Vec<Event> {
    let path = fixtures_dir().join(name);
    std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("fixture {} must exist: {e}", path.display()))
        .lines()
        .filter(|l| !l.trim().is_empty())
        .map(|l| serde_json::from_str(l).unwrap_or_else(|e| panic!("{name}: bad line ({e}): {l}")))
        .collect()
}

/// CRITERION 3 (positive leg) — an `agent.spawned` carrying `role: "reviewer"`
/// folds the role onto the run's state so `decide_permit` can inject it as a
/// permit input axis (DR-016 §Decision 2). The status fold is not regressed.
///
/// COMPILE-RED until `AgentRunState.role` exists + the arm reads it.
#[test]
fn agent_spawned_with_role_folds_onto_run_state() {
    let events = load_fixture("sp4a_role_reviewer.jsonl");
    let graph = fold(events.iter());
    let run = graph
        .agent_runs
        .get(REVIEWER_RUN)
        .expect("the spawn folds a run entry");
    assert_eq!(
        run.status, "running",
        "the existing status fold is not regressed by the additive role read"
    );
    assert_eq!(
        run.role.as_deref(),
        Some("reviewer"),
        "an agent.spawned carrying role=\"reviewer\" folds to \
         AgentRunState.role == Some(\"reviewer\") (DR-016; the axis decide_permit \
         injects)"
    );
}

/// CRITERION 3 (the honesty leg — load-bearing) — an `agent.spawned` WITHOUT a
/// `role` field folds to `None`: the role is ABSENT, NEVER synthesized to a
/// default. Absence is the honest "no role declared" (DR-012; ontology
/// `agent.spawned.role?`: "never synthesized to a default like `\"contributor\"`").
///
/// COMPILE-RED until the field exists; then this makes a synthesized-default
/// regression a test failure.
#[test]
fn agent_spawned_without_role_folds_none_never_synthesized() {
    let events = load_fixture("sp4a_role_absent.jsonl");
    let graph = fold(events.iter());
    let run = graph
        .agent_runs
        .get(ABSENT_RUN)
        .expect("the spawn folds a run entry");
    assert_eq!(
        run.role, None,
        "an agent.spawned WITHOUT role folds to None — absence is honest, NEVER \
         synthesized to a default (DR-012; ontology agent.spawned.role?)"
    );
}

/// CRITERION 3 (rebuild-stability, I3, release-blocking) — a PRE-DR-016 golden
/// fixture (an `agent.spawned` with no `role`, exactly the S1 shape) folds equal
/// under both fold-from-zero and incremental application. The new field being
/// `#[serde(default)]` is what keeps `rezidnt rebuild` reproducing identical
/// graph state across the schema addition (I3).
///
/// COMPILE-RED until the field lands; the equality is the I3 pin.
#[test]
fn pre_dr016_spawn_folds_rebuild_stable() {
    // The role-absent fixture IS the pre-DR-016 shape: an agent.spawned with no
    // `role` key at all, then a status delta — what every golden committed
    // before DR-016 looks like.
    let events = load_fixture("sp4a_role_absent.jsonl");

    let folded = fold(events.iter());
    let mut live = Materializer::new();
    for e in &events {
        live.apply(e);
    }
    assert_eq!(
        live.snapshot(),
        folded,
        "incremental == fold-from-zero across the role field addition — rebuild \
         is stable over a pre-DR-016 golden (I3, release blocker)"
    );
    // And the role is honestly absent, not defaulted.
    assert_eq!(
        folded.agent_runs[ABSENT_RUN].role, None,
        "a pre-DR-016 spawn (no role) folds role = None, not a synthesized \
         default (I3 honesty)"
    );
}
