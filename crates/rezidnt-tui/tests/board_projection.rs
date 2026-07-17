//! S5 oracle — the read-only fleet projection is a PURE fn of `&Graph`.
//!
//! Criterion: "ratatui read-only fleet board consuming only watch channels —
//! proof I1 held." This suite pins the STATE-VIEW-NOT-EVENT-VIEW half: fold a
//! known event log into a `rezidnt_state::Graph`, project it, and assert the
//! `BoardView` reflects fleet state (run count, statuses, per-run cost,
//! workspace open/closed counts). The render path takes a `BoardView`, never an
//! `Event` — the projection is the I1 seam in the type system.
//!
//! RED MODE: assert-red. `project` exists (oracle scaffold) but returns an
//! empty `BoardView`, so every assertion below fails until the implementer
//! fills the projection in. This mirrors the S4 `s4_gates.rs` /
//! DR-006 `dr006_integrity_alarms.rs` scaffold discipline.
//!
//! Fixtures reused (real folded state, not hand-built graphs): the committed
//! `spec/fixtures/s1_agent_run.jsonl` (one run: spawn -> running -> completed
//! with recorded cost) and `spec/fixtures/s4_verified_run.jsonl` (a verified
//! run + an allocated/merged worktree).

use std::path::PathBuf;

use rezidnt_state::{Graph, WorkspaceStatus, fold};
use rezidnt_tui::project;
use rezidnt_types::{Event, EventParts, SourceId, Subject, WorkspaceId};
use serde_json::json;
use time::OffsetDateTime;
use ulid::Ulid;

fn fixtures_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../spec/fixtures")
}

/// Fold a committed golden fixture the same way `rezidnt rebuild` would.
fn graph_from_fixture(name: &str) -> Graph {
    let path = fixtures_dir().join(name);
    let events: Vec<Event> = std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("fixture {} must exist: {e}", path.display()))
        .lines()
        .map(|l| serde_json::from_str(l).unwrap_or_else(|e| panic!("{name} line must parse: {e}")))
        .collect();
    fold(events.iter())
}

/// A workspace-lifecycle event built from parts (S0 envelope path).
fn ws_event(subject: &str, ws: WorkspaceId) -> Event {
    Event::from_parts(EventParts {
        id: Ulid::new(),
        ts: OffsetDateTime::UNIX_EPOCH,
        v: 1,
        source: SourceId::new("test"),
        workspace: Some(ws),
        subject: Subject::new(subject),
        correlation: Ulid::new(),
        causation: None,
        payload: json!({}),
    })
    .expect("test event under 32KiB")
}

/// The s1 fixture folds to exactly one run, completed, with the recorded cost;
/// `project` must surface that run row verbatim.
#[test]
fn projects_the_single_completed_run_with_recorded_cost() {
    let graph = graph_from_fixture("s1_agent_run.jsonl");
    let view = project(&graph);

    assert_eq!(view.runs.len(), 1, "s1 fixture folds to exactly one run");
    let row = &view.runs[0];
    assert_eq!(row.run, "01ARZ3NDEKTSV4RRFFQ69G5A01");
    assert_eq!(row.status, "completed", "agent.completed folded through");
    assert_eq!(row.total_usd, Some(0.190075), "recorded cost surfaced");
    assert_eq!(row.input_tokens, Some(7441));
    assert_eq!(row.output_tokens, Some(45));
    assert_eq!(
        row.integrity_alarms, 0,
        "a healthy run shows zero divergence alarms"
    );
}

/// The projection is a pure fn of the fleet STATE: fold the s4 verified-run
/// fixture and assert the run row + the merged worktree row are both present,
/// in deterministic key order. The board renders derived state, not the log.
#[test]
fn projects_verified_run_and_merged_worktree_from_state() {
    let graph = graph_from_fixture("s4_verified_run.jsonl");
    let view = project(&graph);

    assert_eq!(view.runs.len(), 1, "s4 fixture folds to one run");
    let run = &view.runs[0];
    assert_eq!(run.run, "01S4VER1F1ED00000000000R01");
    assert_eq!(run.status, "completed");
    assert_eq!(run.total_usd, Some(0.001));

    assert_eq!(view.worktrees.len(), 1, "one worktree in the fixture");
    let wt = &view.worktrees[0];
    assert_eq!(wt.path, "/tmp/rezidnt-s4/impl");
    assert_eq!(wt.status, "merged", "diff.merged closed the lifecycle");
    assert_eq!(
        wt.last_diff.as_deref(),
        Some("1d50030ca17af09eb6fad0eadfb3492275bfc76635d0965260cde6bc685d785e")
    );
}

/// Fleet summary counters: `events_folded` mirrors the graph, and open/closed
/// workspace counts are derived from `graph.workspaces` by status.
#[test]
fn projects_fleet_summary_counters_and_workspace_open_closed_split() {
    let open_a = WorkspaceId::new(Ulid::new());
    let open_b = WorkspaceId::new(Ulid::new());
    let closed_c = WorkspaceId::new(Ulid::new());
    let events = [
        ws_event("workspace.opened", open_a),
        ws_event("workspace.opened", open_b),
        ws_event("workspace.opened", closed_c),
        ws_event("workspace.closed", closed_c),
    ];
    let graph = fold(events.iter());
    // Sanity on the fold itself (guards the fixture, not the projection).
    assert_eq!(graph.workspaces[&open_a], WorkspaceStatus::Open);
    assert_eq!(graph.workspaces[&closed_c], WorkspaceStatus::Closed);

    let view = project(&graph);
    assert_eq!(
        view.events_folded, 4,
        "fleet heartbeat mirrors graph.events_folded"
    );
    assert_eq!(view.workspaces_open, 2, "two workspaces left open");
    assert_eq!(view.workspaces_closed, 1, "one workspace closed");
}

/// I1 read-only, at the projection layer: `project` takes `&Graph` (a shared,
/// immutable borrow) and returns an owned view — it CANNOT mutate the graph and
/// there is no event/emit surface in the signature. Encoded structurally: the
/// same graph projected twice is identical, and the graph is unchanged. The
/// deeper structural proof (no writer dependency) lives in `read_only.rs`.
#[test]
fn projection_is_pure_and_cannot_mutate_the_graph() {
    let graph = graph_from_fixture("s1_agent_run.jsonl");
    let before = graph.clone();

    let a = project(&graph);
    let b = project(&graph);

    assert_eq!(a, b, "projection is deterministic (pure fn of &Graph)");
    assert_eq!(
        graph, before,
        "project takes &Graph and cannot mutate the fleet state (I1 read-only)"
    );
    // Non-trivial: an empty scaffold view would make the equality above pass
    // vacuously, so assert the view actually carries the folded run.
    assert_eq!(
        a.runs.len(),
        1,
        "the projected view must reflect the folded run (not an empty scaffold)"
    );
}
