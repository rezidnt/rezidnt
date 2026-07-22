//! DR-032 slice-1 (operator-kill-run) ORACLE — CRITERION 1 (reducer arm, pure —
//! the strongest deterministic judge). Folding an `agent.signaled` fact carrying
//! `operator_badge_id` + `reason` sets the run's NEW attribution fields
//! (`AgentRunState.killed_by` / `.kill_reason`); an `agent.signaled` WITHOUT them
//! (a daemon-initiated reaper stop) leaves them `None` — ABSENT, never
//! synthesized (DR-032 §Decision 5; ontology `agent.signaled.operator_badge_id?`
//! / `reason?`, spec/ontology.md:247-248; DR-012 declared-vs-absent discipline).
//!
//! ## The reducer GAP this pins (implementer work order)
//! `crates/rezidnt-state/src/lib.rs` has NO `"agent.signaled"` match arm today —
//! the signaled run-status transition rides `agent.status.changed`. The
//! implementer adds an `"agent.signaled"` arm keyed on the payload `run` that
//! folds `operator_badge_id?` → `killed_by` and `reason?` → `kill_reason` onto
//! new `#[serde(default)]` fields, mirroring the existing `pep`/`role` optional
//! fold at crates/rezidnt-state/src/lib.rs (VERBATIM when present, `None` when
//! absent — NEVER a sentinel/empty string).
//!
//! ## API SHAPE THE IMPLEMENTER MUST MATCH
//!   - `AgentRunState { .., #[serde(default)] pub killed_by: Option<String>,
//!      #[serde(default)] pub kill_reason: Option<String> }` — the `Option<String>`
//!     storage the `pep`/`role` fields already use (rebuild-stable via
//!     `#[serde(default)]`, keeping every pre-DR-032 golden parsing and comparing
//!     equal — I3, release-blocking).
//!   - a new `"agent.signaled"` arm in `apply`:
//!     ```ignore
//!     "agent.signaled" => {
//!         if let Some(run) = payload_run(event) {
//!             let state = graph.agent_runs.entry(run).or_default();
//!             if let Some(id) = event.payload()["operator_badge_id"].as_str() {
//!                 state.killed_by = Some(id.to_string());
//!             }
//!             if let Some(reason) = event.payload()["reason"].as_str() {
//!                 state.kill_reason = Some(reason.to_string());
//!             }
//!         }
//!     }
//!     ```
//!     ABSENT `operator_badge_id`/`reason` leave the fields `None` (a daemon stop
//!     is NOT an operator stop — the honest representation).
//!
//! RED MODE — COMPILE-RED then behavior-red: `AgentRunState` has no `killed_by`
//! / `kill_reason` field and `apply` has no `agent.signaled` arm today, so this
//! file cannot compile until they land; the fold assertions decide green after.
//!
//! Golden fixtures (committed, minimal, named for the behavior they pin —
//! testing-oracles fixture hygiene):
//!   - `spec/fixtures/dr032_kill_operator_attributed.jsonl` — an operator kill
//!     (`agent.signaled` with `operator_badge_id` + `reason`).
//!   - `spec/fixtures/dr032_kill_daemon_stop.jsonl` — a daemon/reaper stop
//!     (`agent.signaled` with `escalation` but NO `operator_badge_id`/`reason`).

use std::path::PathBuf;

use rezidnt_state::{Materializer, fold};
use rezidnt_types::Event;

/// The run the operator-attributed fixture keys on.
const OP_RUN: &str = "01DR032RVNK1110P0000000000";
/// The loggable operator badge id on that fixture's `agent.signaled` fact
/// (8-byte hex prefix, I2-safe — the ontology's `hex(blake3(sig)[..8])` shape).
const OP_BADGE_ID: &str = "0badf00dcafe1234";
/// The run the daemon-stop fixture keys on.
const DMN_RUN: &str = "01DR032RVNK111DMN000000000";

fn fixtures_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../spec/fixtures")
}

/// Load a committed golden fixture as its event vector (the fixture IS the log —
/// I3). Plain serde parse isolates the reducer, not the wire codec (mirrors
/// `fixture_replay.rs` / `role_fold.rs`).
fn load_fixture(name: &str) -> Vec<Event> {
    let path = fixtures_dir().join(name);
    std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("fixture {} must exist: {e}", path.display()))
        .lines()
        .filter(|l| !l.trim().is_empty())
        .map(|l| serde_json::from_str(l).unwrap_or_else(|e| panic!("{name}: bad line ({e}): {l}")))
        .collect()
}

/// CRITERION 1 (positive leg) — an `agent.signaled` carrying `operator_badge_id` +
/// `reason` folds the operator attribution onto the run's state, VERBATIM. This
/// is the interrogable "a human killed this run" record `debrief` / `gate why`
/// reads (I6, DR-032 §Decision 5).
///
/// COMPILE-RED until `AgentRunState.killed_by`/`.kill_reason` exist + the arm
/// reads them.
#[test]
fn operator_kill_folds_attribution_onto_run() {
    let events = load_fixture("dr032_kill_operator_attributed.jsonl");
    let graph = fold(events.iter());
    let run = graph
        .agent_runs
        .get(OP_RUN)
        .expect("the spawn folds a run entry");
    assert_eq!(
        run.killed_by.as_deref(),
        Some(OP_BADGE_ID),
        "an operator kill folds operator_badge_id VERBATIM onto killed_by \
         (DR-032 §Decision 5; the interrogable operator attribution, I6)"
    );
    assert_eq!(
        run.kill_reason.as_deref(),
        Some("runaway spend"),
        "the operator-supplied reason folds VERBATIM onto kill_reason (I6)"
    );
}

/// CRITERION 1 (the honesty leg — LOAD-BEARING) — an `agent.signaled` WITHOUT
/// `operator_badge_id`/`reason` (a daemon-initiated reaper TERM→KILL stop) folds
/// both fields to `None`: a daemon stop is NOT operator-attributed. Absence is
/// the honest representation — NEVER synthesized to a sentinel/empty string
/// (DR-032 §Decision 5; ontology `operator_badge_id?` value semantics). This is
/// the `debrief` distinction between a human kill and a daemon-timeout stop.
///
/// COMPILE-RED until the fields exist; then a synthesized-attribution regression
/// on the daemon path becomes a test failure.
#[test]
fn daemon_stop_leaves_attribution_none_never_synthesized() {
    let events = load_fixture("dr032_kill_daemon_stop.jsonl");
    let graph = fold(events.iter());
    let run = graph
        .agent_runs
        .get(DMN_RUN)
        .expect("the spawn folds a run entry");
    assert_eq!(
        run.killed_by, None,
        "a daemon/reaper stop carries NO operator_badge_id → killed_by stays None, \
         NEVER synthesized to a sentinel (DR-032; the daemon-vs-operator distinction)"
    );
    assert_eq!(
        run.kill_reason, None,
        "a daemon stop has no operator and no reason → kill_reason stays None (honest absence)"
    );
}

/// CRITERION 1 (rebuild-stability, I3, release-blocking) — the operator-kill
/// golden folds equal under fold-from-zero AND incremental application. The new
/// fields being `#[serde(default)]` is what keeps `rezidnt rebuild` reproducing
/// identical graph state across the schema addition (I3, the property that blocks
/// a release on divergence). Also re-asserts the attribution survives the
/// incremental path.
///
/// COMPILE-RED until the fields land; the equality is the I3 pin.
#[test]
fn operator_kill_folds_rebuild_stable() {
    let events = load_fixture("dr032_kill_operator_attributed.jsonl");

    let folded = fold(events.iter());
    let mut live = Materializer::new();
    for e in &events {
        live.apply(e);
    }
    assert_eq!(
        live.snapshot(),
        folded,
        "incremental == fold-from-zero across the kill-attribution field addition — \
         rebuild is stable (I3, release blocker)"
    );
    // And the attribution is present after the incremental path, not just the
    // batch fold.
    assert_eq!(
        live.graph().agent_runs[OP_RUN].killed_by.as_deref(),
        Some(OP_BADGE_ID),
        "the incremental materializer folds the operator attribution too (I3 parity)"
    );
}

/// CRITERION 1 (pre-DR-032 rebuild-stability, I3) — the daemon-stop golden is
/// the PRE-DR-032 `agent.signaled` shape (no `operator_badge_id`/`reason`,
/// exactly what the reaper emitted before this slice). It folds equal under both
/// paths, and the attribution fields are honestly absent — proof the additive
/// fields never disturb an older fact.
///
/// COMPILE-RED until the fields land.
#[test]
fn pre_dr032_signaled_folds_rebuild_stable() {
    let events = load_fixture("dr032_kill_daemon_stop.jsonl");

    let folded = fold(events.iter());
    let mut live = Materializer::new();
    for e in &events {
        live.apply(e);
    }
    assert_eq!(
        live.snapshot(),
        folded,
        "incremental == fold-from-zero over a pre-DR-032 agent.signaled (I3, release blocker)"
    );
    assert_eq!(
        folded.agent_runs[DMN_RUN].killed_by, None,
        "a pre-DR-032 signaled fact folds killed_by = None, not a synthesized value (I3 honesty)"
    );
}
