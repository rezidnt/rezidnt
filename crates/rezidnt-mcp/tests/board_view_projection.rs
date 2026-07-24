//! DR-039 oracle (C2) — `board_view` result == the pure projection.
//!
//! Criterion (I3: the tool re-interprets nothing): `board_view` returns exactly
//! `rezidnt_state::project(&rezidnt_state::fold(&events))` over the same log.
//! There is no second derivation — the served payload deserializes to a
//! `rezidnt_state::BoardView` that EQUALS the pure fold-then-project of the
//! seeded events. Seeds the committed `s4_verified_run.jsonl` golden fixture
//! (one agent run + one allocated/merged worktree + several subjects: gates,
//! worktree, agent, diff), calls the tool through the server core, and asserts
//! byte-for-byte projection equality.
//!
//! RED MODE — intended reds, all "missing type/tool/hoist", not typos:
//! - `rezidnt_state::project` / `rezidnt_state::BoardView` do NOT exist yet:
//!   DR-039 Decision 3 hoists them DOWN from `rezidnt-tui` into `rezidnt-state`.
//!   Until that hoist, this file fails to COMPILE (unresolved path). That red is
//!   the hoist work order.
//! - `board_view` is not advertised/dispatched yet, so once it compiles the
//!   `tool_call` assertion goes red until the tool is served.

mod util;

use serde_json::json;

/// The tool result IS the pure projection of the folded log — no
/// re-interpretation (DR-039 Decision 2, I3). Fold the seeded events with the
/// real reducers, project with the (hoisted) pure projection, and assert the
/// served `board_view` payload deserializes to the identical `BoardView`.
#[tokio::test]
async fn board_view_equals_pure_project_of_fold() {
    let (_dir, core) = util::core();
    // The s4 fixture folds to one run (completed, recorded cost), one merged
    // worktree, and a spread of subjects (gate.entered/passed, worktree.allocated,
    // agent.spawned/completed, diff.ready/merged) — enough fleet state that an
    // empty-scaffold projection could never match by accident.
    let seeded = util::seed_fixture(&core, "s4_verified_run.jsonl");

    // The oracle's ground truth: the pure fold-then-project. This is the exact
    // pipeline DR-039 pins the tool to (`fold` then `project`), computed here
    // independently of the tool so equality means "the tool re-derives nothing".
    let expected = rezidnt_state::project(&rezidnt_state::fold(seeded.iter()));

    // The tool takes an empty snapshot arg (full fold), mirroring tail_events.
    let result = util::tool_call(&core, 1, "board_view", json!({})).await;
    assert_ne!(
        result["isError"],
        json!(true),
        "board_view is a read; it must not error: {result:#}"
    );

    let payload = util::tool_payload(&result);
    let served: rezidnt_state::BoardView =
        serde_json::from_value(payload.clone()).unwrap_or_else(|e| {
            panic!("board_view payload must deserialize to a BoardView ({e}): {payload:#}")
        });

    assert_eq!(
        served, expected,
        "board_view result MUST EQUAL rezidnt_state::project(&fold(&events)) — \
         the tool is exactly the pure projection, it re-interprets nothing (I3)"
    );

    // Non-vacuity guard: the fixture is a real run+worktree, so a matching
    // EMPTY view would be a bug in the oracle, not a pass. Pin that the compared
    // view actually carries the folded fleet state.
    assert_eq!(served.runs.len(), 1, "s4 fixture folds to exactly one run");
    assert_eq!(
        served.worktrees.len(),
        1,
        "s4 fixture folds to exactly one worktree"
    );
    assert_eq!(
        served.events_folded,
        seeded.len() as u64,
        "events_folded is the whole seeded log"
    );
}
